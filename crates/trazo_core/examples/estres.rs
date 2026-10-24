//! Stress test: a long 1080p animation, to measure memory, render, save and load.
//!
//! cargo run --release -p trazo_core --example estres -- <out_path.anim>

use std::path::PathBuf;
use std::time::{Duration, Instant};

use trazo_core::composite::{render_region, OnionSkin, RenderOptions};
use trazo_core::document::{CanvasSettings, Frame, Layer, LayerProps, Project};
use trazo_core::io::{load_project, save_project};
use trazo_core::raster::{TiledImage, TILE};
use trazo_core::{IRect, Rgba};

const W: u32 = 1920;
const H: u32 = 1080;
const FPS: u32 = 24;
const FRAMES: usize = 1200;

/// Src-over a straight color with coverage `cov` onto a premultiplied pixel.
fn blend(img: &mut TiledImage, x: i32, y: i32, c: [f32; 3], cov: f32) {
    if cov <= 0.0 {
        return;
    }
    let a = cov.min(1.0);
    let d = img.pixel(x, y);
    let inv = 1.0 - a;
    let out = [
        (c[0] * a * 255.0 + d[0] as f32 * inv).round() as u8,
        (c[1] * a * 255.0 + d[1] as f32 * inv).round() as u8,
        (c[2] * a * 255.0 + d[2] as f32 * inv).round() as u8,
        (a * 255.0 + d[3] as f32 * inv).round() as u8,
    ];
    img.set_pixel(x, y, out);
}

/// Anti-aliased capsule (thick line with round caps) from a to b.
fn capsule(img: &mut TiledImage, a: (f32, f32), b: (f32, f32), r: f32, c: [f32; 3]) {
    let (x0, x1) = (a.0.min(b.0) - r - 1.0, a.0.max(b.0) + r + 1.0);
    let (y0, y1) = (a.1.min(b.1) - r - 1.0, a.1.max(b.1) + r + 1.0);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    for y in y0.floor() as i32..=y1.ceil() as i32 {
        for x in x0.floor() as i32..=x1.ceil() as i32 {
            let (px, py) = (x as f32 + 0.5 - a.0, y as f32 + 0.5 - a.1);
            let t = ((px * dx + py * dy) / len2).clamp(0.0, 1.0);
            let (ex, ey) = (px - t * dx, py - t * dy);
            let dist = (ex * ex + ey * ey).sqrt();
            blend(img, x, y, c, r + 0.5 - dist);
        }
    }
}

fn circle(img: &mut TiledImage, c0: (f32, f32), r: f32, c: [f32; 3]) {
    capsule(img, c0, c0, r, c);
}

/// Background: sky gradient, ground, sun and some hills (covers the whole canvas).
fn draw_background(img: &mut TiledImage) {
    let mut row = vec![0u8; W as usize * 4];
    for y in 0..H as i32 {
        let t = y as f32 / H as f32;
        let ground = y > 760;
        for x in 0..W as usize {
            let (r, g, b) = if ground {
                let n = ((x as f32 * 0.05).sin() * 0.03) + 0.0;
                (0.35 + n, 0.6 + n, 0.3)
            } else {
                (0.45 + 0.4 * t, 0.7 + 0.2 * t, 1.0)
            };
            row[x * 4] = (r * 255.0) as u8;
            row[x * 4 + 1] = (g * 255.0) as u8;
            row[x * 4 + 2] = (b * 255.0) as u8;
            row[x * 4 + 3] = 255;
        }
        img.write_region(0, y, W as usize, 1, &row);
    }
    circle(img, (1600.0, 180.0), 90.0, [1.0, 0.9, 0.4]);
    for k in 0..5 {
        circle(img, (200.0 + k as f32 * 400.0, 900.0), 220.0, [0.3, 0.52, 0.27]);
    }
}

/// A walking figure (~700 px tall) whose position and pose depend on `i`.
fn draw_character(img: &mut TiledImage, i: usize) {
    let t = i as f32 / FRAMES as f32;
    let phase = i as f32 * std::f32::consts::TAU / 16.0; // 16-frame walk cycle
    let cx = 200.0 + t * 1500.0;
    let bob = phase.sin().abs() * 15.0;
    let hip = (cx, 560.0 - bob);
    let neck = (cx + 10.0, 300.0 - bob);
    let head = (cx + 15.0, 210.0 - bob);
    let body = [0.85, 0.3, 0.25];
    let skin = [0.98, 0.8, 0.65];
    let pants = [0.2, 0.25, 0.55];
    let line = [0.1, 0.08, 0.08];
    let swing = phase.sin() * 0.6;
    let leg = |s: f32| {
        let knee = (hip.0 + (s * 0.8).sin() * 110.0, hip.1 + 120.0);
        let foot = (knee.0 + (s * 0.8).sin() * 40.0 - 10.0, hip.1 + 300.0);
        (knee, foot)
    };
    let arm = |s: f32| {
        let elbow = (neck.0 + s.sin() * 90.0, neck.1 + 130.0);
        let hand = (elbow.0 + s.sin() * 60.0 + 20.0, elbow.1 + 110.0);
        (elbow, hand)
    };
    // Back limbs, body, front limbs: color fill plus dark outline strokes.
    for (s, front) in [(-swing, false), (swing, true)] {
        let (k, f) = leg(s);
        if !front {
            capsule(img, hip, k, 30.0, pants);
            capsule(img, k, f, 26.0, pants);
            capsule(img, f, (f.0 + 45.0, f.1 + 5.0), 18.0, line);
        }
    }
    capsule(img, neck, hip, 60.0, body);
    circle(img, head, 75.0, skin);
    circle(img, (head.0 + 30.0, head.1 - 10.0), 8.0, line);
    capsule(img, (head.0 + 5.0, head.1 + 35.0), (head.0 + 40.0, head.1 + 30.0), 4.0, line);
    for (s, front) in [(-swing, false), (swing, true)] {
        if front {
            let (k, f) = leg(s);
            capsule(img, hip, k, 30.0, pants);
            capsule(img, k, f, 26.0, pants);
            capsule(img, f, (f.0 + 45.0, f.1 + 5.0), 18.0, line);
        }
        let (e, h) = arm(-s);
        capsule(img, neck, e, 22.0, skin);
        capsule(img, e, h, 20.0, skin);
    }
}

/// Sparkles around the figure (an effect redrawn every 4 frames).
fn draw_effects(img: &mut TiledImage, i: usize) {
    let t = i as f32 / FRAMES as f32;
    let cx = 200.0 + t * 1500.0;
    let mut seed = i as u32 * 2654435761 + 1;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for _ in 0..25 {
        let x = cx + (rnd() - 0.5) * 700.0;
        let y = 150.0 + rnd() * 700.0;
        let r = 6.0 + rnd() * 18.0;
        circle(img, (x, y), r, [1.0, 0.95, 0.5]);
        capsule(img, (x - r * 2.0, y), (x + r * 2.0, y), 2.0, [1.0, 1.0, 0.8]);
    }
}

fn count_tiles(p: &Project) -> (usize, usize) {
    let mut tiles = 0;
    let mut cels = 0;
    for f in &p.frames {
        for c in f.cels.values() {
            tiles += c.allocated_tiles();
            cels += 1;
        }
    }
    for c in p.shared_cels.values() {
        tiles += c.allocated_tiles();
        cels += 1;
    }
    (tiles, cels)
}

fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

fn report_tiles(label: &str, p: &Project) {
    let (tiles, cels) = count_tiles(p);
    let bytes = tiles * TILE * TILE * 4;
    let per_canvas = (W as usize).div_ceil(TILE) * (H as usize).div_ceil(TILE);
    println!(
        "{label}: {cels} cels, {tiles} tiles allocated (canvas = {per_canvas} tiles), ~{:.1} MB pixel RAM, {:.2} MB/frame",
        mb(bytes),
        mb(bytes) / FRAMES as f64
    );
}

#[cfg(windows)]
fn peak_memory() -> Option<(usize, usize)> {
    #[repr(C)]
    #[derive(Default)]
    struct Pmc {
        cb: u32,
        page_fault_count: u32,
        peak_working_set: usize,
        working_set: usize,
        quota_peak_paged: usize,
        quota_paged: usize,
        quota_peak_nonpaged: usize,
        quota_nonpaged: usize,
        pagefile: usize,
        peak_pagefile: usize,
    }
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn K32GetProcessMemoryInfo(process: isize, counters: *mut Pmc, cb: u32) -> i32;
    }
    let mut c = Pmc { cb: std::mem::size_of::<Pmc>() as u32, ..Default::default() };
    // SAFETY: plain Win32 calls with a correctly sized struct.
    let ok = unsafe { K32GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) };
    (ok != 0).then_some((c.working_set, c.peak_working_set))
}

#[cfg(not(windows))]
fn peak_memory() -> Option<(usize, usize)> {
    None
}

fn report_mem(label: &str) {
    if let Some((cur, peak)) = peak_memory() {
        println!("{label}: process working set {:.1} MB (peak {:.1} MB)", mb(cur), mb(peak));
    }
}

fn bench_render(p: &Project, label: &str, opts: &RenderOptions) {
    let full = IRect::from_size(W, H);
    let mut out = Vec::new();
    let samples = 120;
    let mut total = Duration::ZERO;
    let mut worst = Duration::ZERO;
    for k in 0..samples {
        let i = k * p.frames.len() / samples;
        let t = Instant::now();
        render_region(p, i, full, opts, &mut out);
        let d = t.elapsed();
        total += d;
        worst = worst.max(d);
    }
    let avg = total / samples as u32;
    println!(
        "render_region full frame [{label}] over {samples} frames: avg {:.2} ms, worst {:.2} ms (=> {:.1} fps max; 24 fps needs <= 41.7 ms)",
        avg.as_secs_f64() * 1e3,
        worst.as_secs_f64() * 1e3,
        1.0 / avg.as_secs_f64()
    );
}

fn main() {
    let out: PathBuf = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| "Prueba_de_rendimiento.anim".into());

    // ---- build ----
    let t = Instant::now();
    let canvas = CanvasSettings { width: W, height: H, fps: FPS, background: Rgba::WHITE, ..Default::default() };
    let mut p = Project::new("Prueba de rendimiento", canvas);
    p.layers.clear();
    p.frames.clear();
    let bg = p.alloc_id();
    p.layers.push(Layer { id: bg, props: LayerProps { shared: true, onion_skin: false, ..LayerProps::named("Fondo") } });
    let ch = p.alloc_id();
    p.layers.push(Layer { id: ch, props: LayerProps::named("Personaje") });
    let fx = p.alloc_id();
    p.layers.push(Layer { id: fx, props: LayerProps { onion_skin: false, ..LayerProps::named("Efectos") } });

    let mut bg_img = TiledImage::new(W, H);
    draw_background(&mut bg_img);
    p.shared_cels.insert(bg, bg_img);

    for i in 0..FRAMES {
        let id = p.alloc_id();
        let mut f = Frame::new(id);
        let mut c = TiledImage::new(W, H);
        draw_character(&mut c, i);
        f.cels.insert(ch, c);
        if i % 4 == 0 {
            let mut e = TiledImage::new(W, H);
            draw_effects(&mut e, i);
            f.cels.insert(fx, e);
        } else {
            f.cont.insert(fx);
        }
        p.frames.push(f);
    }
    println!("build: {} frames ({:.1} s at {FPS} fps) in {:.2} s", p.frames.len(), p.duration_secs(), t.elapsed().as_secs_f64());
    report_tiles("in memory", &p);
    let ch_tiles: usize = p.frames.iter().filter_map(|f| f.cel(ch)).map(|c| c.allocated_tiles()).sum();
    println!("  Personaje avg {:.1} tiles/drawing", ch_tiles as f64 / FRAMES as f64);
    report_mem("after build");

    // ---- render ----
    bench_render(&p, "background, no onion", &RenderOptions { background: true, ..Default::default() });
    bench_render(&p, "background + onion 1/1", &RenderOptions { background: true, onion: Some(OnionSkin::default()), ..Default::default() });

    // ---- what an Edit::Timeline snapshot costs (clone of the frame list) ----
    let t = Instant::now();
    let snap = p.frames.clone();
    println!("clone of frame list (Edit::Timeline snapshot): {:.2} ms", t.elapsed().as_secs_f64() * 1e3);
    drop(snap);

    // ---- save ----
    if let Some(dir) = out.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let t = Instant::now();
    save_project(&p, &out).expect("save");
    let save_s = t.elapsed().as_secs_f64();
    let size = std::fs::metadata(&out).map(|m| m.len()).unwrap_or(0);
    println!("save_project: {save_s:.2} s, file {:.1} MB -> {}", mb(size as usize), out.display());
    report_mem("after save");
    drop(p);

    // ---- load ----
    let t = Instant::now();
    let q = load_project(&out).expect("load");
    println!("load_project: {:.2} s ({} frames, {} layers)", t.elapsed().as_secs_f64(), q.frames.len(), q.layers.len());
    report_tiles("after load", &q);
    report_mem("after load");
    bench_render(&q, "loaded, background, no onion", &RenderOptions { background: true, ..Default::default() });
}
