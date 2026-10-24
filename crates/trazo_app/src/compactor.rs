//! Keeps memory low on big projects: every couple of seconds, drawings away
//! from the frame being worked on (and the undo history) are packed in
//! memory on a background thread. Reading them still works; painting on one
//! opens it again.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::app::Document;

/// Frames on each side of the current one that stay open for drawing.
const KEEP_NEAR: usize = 2;
/// Seconds between checks.
const EVERY: f64 = 2.0;

#[derive(Default)]
pub struct Compactor {
    busy: Arc<AtomicBool>,
    last: f64,
    /// (content version, frame, history revision) of the last pass.
    last_key: Option<(u64, usize, u64)>,
}

impl Compactor {
    /// Call once per UI frame. `idle` is false while drawing or playing.
    pub fn tick(&mut self, now: f64, doc: &Document, idle: bool) {
        if !idle || now - self.last < EVERY || self.busy.load(Ordering::Acquire) {
            return;
        }
        let key = (doc.content_version, doc.frame, doc.history.revision());
        if self.last_key == Some(key) {
            return;
        }
        self.last = now;
        self.last_key = Some(key);
        let keep = doc.frame.saturating_sub(KEEP_NEAR)..=doc.frame + KEEP_NEAR;
        let tiles = trazo_core::memory::cold_tiles(&doc.project, Some(&doc.history), keep);
        if tiles.is_empty() {
            return;
        }
        self.busy.store(true, Ordering::Release);
        let busy = self.busy.clone();
        let spawned = std::thread::Builder::new().name("compactar".into()).spawn(move || {
            let saved = trazo_core::memory::pack(&tiles);
            log::debug!("packed {} tiles, {} KB saved", tiles.len(), saved / 1024);
            busy.store(false, Ordering::Release);
        });
        if spawned.is_err() {
            self.busy.store(false, Ordering::Release);
        }
    }
}
