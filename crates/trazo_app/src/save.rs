//! Background project saving.
//!
//! One worker thread handles every save in order, so two saves can never race
//! on the same file. Blocking saves (app shutdown, switching projects) go
//! through the same queue and simply wait for their turn.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};

use trazo_core::{io, Project};

struct Job {
    project: Project,
    path: PathBuf,
    revision: u64,
    doc_serial: u64,
    announce: bool,
    reply: Option<Sender<Result<(), String>>>,
}

/// Result of a finished save, reported back to the UI thread.
pub struct SaveDone {
    pub revision: u64,
    pub doc_serial: u64,
    pub announce: bool,
    pub result: Result<(), String>,
}

pub struct SaveWorker {
    tx: Sender<Job>,
    done: Receiver<SaveDone>,
    in_flight: usize,
}

impl Default for SaveWorker {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel();
        std::thread::Builder::new()
            .name("guardado".into())
            .spawn(move || {
                for job in rx {
                    let result = io::save_project(&job.project, &job.path).map_err(|e| e.to_string());
                    if let Some(reply) = job.reply {
                        let _ = reply.send(result.clone());
                    }
                    let _ = done_tx.send(SaveDone {
                        revision: job.revision,
                        doc_serial: job.doc_serial,
                        announce: job.announce,
                        result,
                    });
                }
            })
            .expect("no se pudo crear el hilo de guardado");
        Self { tx, done, in_flight: 0 }
    }
}

impl SaveWorker {
    pub fn is_busy(&self) -> bool {
        self.in_flight > 0
    }

    /// Queue a save. `project` is a cheap clone (tiles are shared).
    pub fn save(&mut self, project: Project, path: PathBuf, revision: u64, doc_serial: u64, announce: bool) {
        self.in_flight += 1;
        let job = Job { project, path, revision, doc_serial, announce, reply: None };
        if self.tx.send(job).is_err() {
            self.in_flight -= 1;
        }
    }

    /// Save and wait until it (and anything queued before it) is written.
    pub fn save_and_wait(&mut self, project: Project, path: PathBuf, revision: u64, doc_serial: u64) -> Result<(), String> {
        let (reply, wait) = mpsc::channel();
        self.in_flight += 1;
        let job = Job { project, path, revision, doc_serial, announce: false, reply: Some(reply) };
        self.tx.send(job).map_err(|_| "el hilo de guardado no responde".to_string())?;
        wait.recv().map_err(|_| "el hilo de guardado se detuvo".to_string())?
    }

    /// Finished saves since the last call.
    pub fn poll(&mut self) -> Vec<SaveDone> {
        let v: Vec<SaveDone> = self.done.try_iter().collect();
        self.in_flight = self.in_flight.saturating_sub(v.len());
        v
    }
}
