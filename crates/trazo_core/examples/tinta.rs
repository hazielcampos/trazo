//! Demo animation: a figure in a hooded coat with a scarf in the wind, drawn
//! with Trazo's own brushes (pencil sketch, dry ink, watercolor) on textured
//! paper. Every drawing is re-traced with a little wobble ("line boil").
//!
//! cargo run --release -p trazo_core --example tinta -- <out.anim> [preview_dir]

use std::f32::consts::TAU;
use std::path::PathBuf;

use trazo_core::brush::{builtin_brushes, Brush};
use trazo_core::color::BlendMode;
use trazo_core::composite::{render_region, unpremultiply, RenderOptions};
use trazo_core::document::{CanvasSettings, Frame, Layer, LayerProps, Project};
use trazo_core::io::save_project;
use trazo_core::raster::TiledImage;
use trazo_core::stroke::{Stroke, StrokePoint, StrokeSettings};
use trazo_core::{IRect, Rgba};

const W: u32 = 1080;
const H: u32 = 1350;
const FPS: u32 = 12;
/// Drawings in the loop (2 s at 12 fps).
const N: usize = 24;

type P = (f32, f32);

// ---- small helpers ------------------------------------------------------------

#[derive(Clone)]
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xD1B54A32D192ED03)
    }
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn s(&mut self) -> f32 {
        self.f() * 2.0 - 1.0
    }
}

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374761393) ^ (y as u32).wrapping_mul(668265263) ^ seed.wrapping_mul(2246822519);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    ((h ^ (h >> 16)) & 0xffff) as f32 / 65535.0
}

fn value_noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (u, v) = (s(fx), s(fy));
    let a = hash2(xi, yi, seed);
    let b = hash2(xi + 1, yi, seed);
    let c = hash2(xi, yi + 1, seed);
    let d = hash2(xi + 1, yi + 1, seed);
    a + (b - a) * u + (c - a) * v + (a - b - c + d) * u * v
}

fn fbm(x: f32, y: f32, oct: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut f, mut norm) = (0.0, 1.0, 1.0, 0.0);
    for o in 0..oct {
        sum += value_noise(x * f, y * f, seed + o * 31) * amp;
        norm += amp;
        amp *= 0.5;
        f *= 2.0;
    }
    sum / norm
}

/// Catmull-Rom through the points, sampled about every `step` pixels.
fn spline(pts: &[P], step: f32) -> Vec<P> {
    if pts.len() < 2 {
        return pts.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..pts.len() - 1 {
        let p0 = if i == 0 { pts[0] } else { pts[i - 1] };
        let (p1, p2) = (pts[i], pts[i + 1]);
        let p3 = if i + 2 < pts.len() { pts[i + 2] } else { pts[i + 1] };
        let len = ((p2.0 - p1.0).powi(2) + (p2.1 - p1.1).powi(2)).sqrt();
        let n = (len / step).ceil().max(1.0) as usize;
        for k in 0..n {
            let t = k as f32 / n as f32;
            let (t2, t3) = (t * t, t * t * t);
            let f = |a: f32, b: f32, c: f32, d: f32| 0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3);
            out.push((f(p0.0, p1.0, p2.0, p3.0), f(p0.1, p1.1, p2.1, p3.1)));
        }
    }
    out.push(*pts.last().unwrap());
    out
}

fn lerp(a: P, b: P, t: f32) -> P {
    (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
}

fn inside(poly: &[P], p: P) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
        j = i;
    }
    c
}

// ---- drawing with the app's brushes ---------------------------------------------

struct Pens {
    ink: Brush,
    sketch: Brush,
    pencil: Brush,
    wash: Brush,
}

impl Pens {
    fn new() -> Self {
        let all = builtin_brushes();
        let get = |n: &str| all.iter().find(|b| b.name == n).cloned().unwrap_or_else(|| panic!("brush {n}"));
        Pens { ink: get("Tinta seca"), sketch: get("Lápiz de boceto"), pencil: get("Lápiz"), wash: get("Acuarela") }
    }
}

/// How the pressure changes along a stroke.
#[derive(Clone, Copy)]
enum Press {
    /// Thin – full – thin.
    Swell,
    /// Starts full, flicks out thin (hair, fringe).
    Flick,
    /// Even.
    Flat,
}

struct Pen<'a> {
    img: &'a mut TiledImage,
    rng: Rng,
    /// Wobble of the control points, in pixels (line boil).
    boil: f32,
    seed: u64,
}

impl Pen<'_> {
    fn line(&mut self, brush: &Brush, pts: &[P], size: f32, color: Rgba, opacity: f32, press: Press) {
        if pts.len() < 2 {
            return;
        }
        let pts: Vec<P> = pts.iter().map(|&(x, y)| (x + self.rng.s() * self.boil, y + self.rng.s() * self.boil)).collect();
        let path = spline(&pts, 2.5);
        self.seed += 1;
        let settings = StrokeSettings { color, size, opacity, ..Default::default() };
        let mut s = Stroke::new(brush.clone(), settings, 0, 0, self.seed);
        let n = path.len().max(2) as f32 - 1.0;
        let wob = self.rng.f() * TAU;
        for (i, &(x, y)) in path.iter().enumerate() {
            let t = i as f32 / n;
            let base = match press {
                Press::Swell => (t * std::f32::consts::PI).sin().powf(0.6),
                Press::Flick => (1.0 - t).powf(0.7) * (t * 12.0).min(1.0),
                Press::Flat => 0.85,
            };
            let p = (base * (0.85 + 0.15 * (t * 9.0 + wob).sin())).clamp(0.05, 1.0);
            s.add_point(self.img, StrokePoint { x, y, pressure: p });
        }
        let _ = s.finish(self.img);
    }

    /// Ink line, sometimes gone over twice like a quick hand does.
    fn ink(&mut self, pens: &Pens, pts: &[P], size: f32, press: Press) {
        let ink = Rgba::new(0.13, 0.11, 0.10, 1.0);
        self.line(&pens.ink, pts, size, ink, 1.0, press);
        if self.rng.f() < 0.35 {
            let dx = self.rng.s() * 2.5;
            let dy = self.rng.s() * 2.5;
            let shifted: Vec<P> = pts.iter().map(|&(x, y)| (x + dx, y + dy)).collect();
            self.line(&pens.ink, &shifted, size * 0.5, ink, 0.55, press);
        }
    }

    /// Loose watercolor scribble filling a shape (misregistered on purpose).
    fn wash(&mut self, pens: &Pens, poly: &[P], color: Rgba, size: f32, angle: f32, gap: f32, opacity: f32) {
        // Two crossing passes with wide, overlapping strokes: patchy, not striped.
        self.wash_pass(pens, poly, color, size * 1.5, angle, gap, opacity * 0.6);
        self.wash_pass(pens, poly, color, size * 1.2, angle + 0.7, gap * 1.6, opacity * 0.4);
    }

    fn wash_pass(&mut self, pens: &Pens, poly: &[P], color: Rgba, size: f32, angle: f32, gap: f32, opacity: f32) {
        let off = (self.rng.s() * 10.0 + 8.0, self.rng.s() * 8.0 + 5.0);
        let poly: Vec<P> = poly.iter().map(|&(x, y)| (x + off.0, y + off.1)).collect();
        let (minx, maxx) = poly.iter().fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.0), m.1.max(p.0)));
        let (miny, maxy) = poly.iter().fold((f32::MAX, f32::MIN), |m, p| (m.0.min(p.1), m.1.max(p.1)));
        let (ca, sa) = (angle.cos(), angle.sin());
        let c = ((minx + maxx) / 2.0, (miny + maxy) / 2.0);
        let r = ((maxx - minx).powi(2) + (maxy - miny).powi(2)).sqrt() / 2.0;
        // Parallel passes across the shape; each run inside becomes one stroke.
        let mut k = -r;
        let mut runs: Vec<Vec<P>> = Vec::new();
        while k < r {
            let mut run: Vec<P> = Vec::new();
            let mut s = -r;
            let ph = self.rng.f() * TAU;
            while s < r {
                let kk = k + (s * 0.02 + ph).sin() * gap * 0.35;
                let p = (c.0 + ca * s - sa * kk, c.1 + sa * s + ca * kk);
                if inside(&poly, p) {
                    run.push(p);
                } else if run.len() > 3 {
                    runs.push(std::mem::take(&mut run));
                } else {
                    run.clear();
                }
                s += 12.0;
            }
            if run.len() > 3 {
                runs.push(run);
            }
            k += gap * (0.8 + self.rng.f() * 0.5);
        }
        for (i, run) in runs.iter().enumerate() {
            // Leave a few gaps: it reads as painted by hand.
            if self.rng.f() < 0.12 {
                continue;
            }
            let pts: Vec<P> = if i % 2 == 0 { run.clone() } else { run.iter().rev().cloned().collect() };
            let thin: Vec<P> = pts.iter().step_by(3).cloned().chain(std::iter::once(*pts.last().unwrap())).collect();
            let sz = size * (0.8 + self.rng.f() * 0.4);
            self.line(&pens.wash, &thin, sz, color, opacity, Press::Flat);
        }
    }
}

// ---- the figure -------------------------------------------------------------------

/// Pose for drawing `i` of the loop.
struct Pose {
    /// Wind phase (radians, one turn per loop).
    w: f32,
    /// Breathing: shoulders and chest rise a little.
    breath: f32,
}

impl Pose {
    fn at(i: usize) -> Self {
        let w = i as f32 / N as f32 * TAU;
        Pose { w, breath: (w).sin() }
    }
    fn head(&self) -> P {
        (self.breath * 1.5, -self.breath * 3.0)
    }
    fn body(&self, y: f32) -> P {
        // More lift near the shoulders than at the bottom of the coat.
        let k = ((1350.0 - y) / 830.0).clamp(0.0, 1.0);
        (0.0, -self.breath * 4.0 * k)
    }
}

fn mv(pts: &[P], d: P) -> Vec<P> {
    pts.iter().map(|&(x, y)| (x + d.0, y + d.1)).collect()
}

fn mvb(pose: &Pose, pts: &[P]) -> Vec<P> {
    pts.iter().map(|&(x, y)| {
        let d = pose.body(y);
        (x + d.0, y + d.1)
    }).collect()
}

/// A lock of hair: from `base` out along `dir` (radians), bent by the wind.
fn lock(base: P, dir: f32, len: f32, bend: f32) -> Vec<P> {
    let mut pts = Vec::new();
    for k in 0..=4 {
        let t = k as f32 / 4.0;
        let a = dir + bend * t * t;
        let prev: P = *pts.last().unwrap_or(&base);
        let p = if k == 0 { base } else { (prev.0 + a.cos() * len / 4.0, prev.1 + a.sin() * len / 4.0) };
        pts.push(p);
    }
    pts
}

/// A cloth tail in the wind: centerline from `root`, waving.
fn tail(root: P, len: f32, width: f32, w: f32, phase: f32, droop: f32) -> (Vec<P>, Vec<P>, Vec<P>) {
    let mut c = Vec::new();
    let segs = 10;
    for k in 0..=segs {
        let s = k as f32 / segs as f32;
        let x = root.0 - s * len;
        let wave = (w * 2.0 - s * 5.5 + phase).sin() * 46.0 * s.powf(1.2) + (w * 3.0 - s * 9.0 + phase).sin() * 10.0 * s;
        let y = root.1 + s * droop + wave;
        c.push((x, y));
    }
    let mut a = Vec::new();
    let mut b = Vec::new();
    for k in 0..c.len() {
        let p = c[k];
        let q = if k + 1 < c.len() { c[k + 1] } else { c[k] };
        let o = if k > 0 { c[k - 1] } else { c[k] };
        let (dx, dy) = (q.0 - o.0, q.1 - o.1);
        let l = (dx * dx + dy * dy).sqrt().max(1e-3);
        let (nx, ny) = (-dy / l, dx / l);
        let s = k as f32 / segs as f32;
        let hw = width * (1.0 - 0.25 * s) / 2.0;
        a.push((p.0 + nx * hw, p.1 + ny * hw));
        b.push((p.0 - nx * hw, p.1 - ny * hw));
    }
    (c, a, b)
}

/// Shapes shared by the sketch, ink and color passes.
#[allow(dead_code)]
struct Shapes {
    face: Vec<P>,
    skull: Vec<P>,
    ear: Vec<P>,
    ear_in: Vec<P>,
    neck_front: Vec<P>,
    neck_back: Vec<P>,
    fringe: Vec<Vec<P>>,
    locks: Vec<Vec<P>>,
    flyaways: Vec<Vec<P>>,
    scarf_top: Vec<P>,
    scarf_bottom: Vec<P>,
    scarf_folds: Vec<Vec<P>>,
    tails: Vec<(Vec<P>, Vec<P>, Vec<P>)>,
    hood: Vec<P>,
    back: Vec<P>,
    front: Vec<P>,
    opening: Vec<P>,
    sleeve_back: Vec<P>,
    sleeve_front: Vec<P>,
    cuff: Vec<P>,
    creases: Vec<Vec<P>>,
    string: Vec<P>,
    coat_poly: Vec<P>,
    scarf_poly: Vec<P>,
    hair_poly: Vec<P>,
    skin_poly: Vec<P>,
}

fn shapes(pose: &Pose) -> Shapes {
    let h = pose.head();
    let w = pose.w;
    let face = mv(
        &[(600.0, 270.0), (624.0, 298.0), (636.0, 326.0), (641.0, 352.0), (660.0, 376.0), (684.0, 398.0), (671.0, 409.0), (661.0, 414.0), (667.0, 425.0), (660.0, 435.0), (651.0, 439.0), (659.0, 451.0), (653.0, 466.0), (638.0, 480.0), (616.0, 491.0), (590.0, 497.0)],
        h,
    );
    let skull = mv(&[(600.0, 272.0), (586.0, 238.0), (552.0, 214.0), (508.0, 212.0), (468.0, 228.0), (440.0, 260.0), (428.0, 300.0), (434.0, 344.0), (452.0, 384.0), (478.0, 414.0), (502.0, 436.0)], h);
    let ear = mv(&[(541.0, 392.0), (531.0, 380.0), (518.0, 386.0), (514.0, 406.0), (521.0, 424.0), (536.0, 432.0)], h);
    let ear_in = mv(&[(530.0, 394.0), (524.0, 407.0), (531.0, 418.0)], h);
    let neck_front = mvb(pose, &[(590.0 + h.0, 500.0 + h.1), (590.0, 530.0), (598.0, 560.0)]);
    let neck_back = mvb(pose, &[(504.0 + h.0, 440.0 + h.1), (506.0, 490.0), (512.0, 538.0)]);

    // Hair: a ring of pointed clumps around the skull, blown back by the wind
    // (the wind comes from the front, on the right), plus loose hairs.
    let gust = (w).sin() * 0.5 + (w * 2.0 + 1.0).sin() * 0.25;
    let arc = spline(&skull, 4.0);
    let at = |t: f32| arc[(t.clamp(0.0, 1.0) * (arc.len() - 1) as f32) as usize];
    let center = (520.0 + h.0, 330.0 + h.1);
    // (position along the arc, length) of each clump; the arc runs from the
    // forehead over the crown to the nape.
    let clumps = [(0.0, 34.0), (0.08, 52.0), (0.17, 66.0), (0.27, 74.0), (0.38, 72.0), (0.49, 66.0), (0.60, 58.0), (0.71, 50.0), (0.82, 40.0), (0.92, 30.0)];
    let curve = |p: P, q: P, bow: f32| -> Vec<P> {
        let m = lerp(p, q, 0.5);
        let (ex, ey) = (q.0 - p.0, q.1 - p.1);
        vec![p, (m.0 - ey * bow, m.1 + ex * bow), q]
    };
    let mut fringe = Vec::new();
    let mut locks = Vec::new();
    for (j, (t, len)) in clumps.iter().enumerate() {
        let next = clumps.get(j + 1).map(|c| c.0).unwrap_or(1.0);
        let (a, b) = (at(*t), at(next));
        let mid = at((t + next) / 2.0);
        // Swept back along the head (toward the nape), lifted a little off it
        // and pushed left by the wind.
        let (tx, ty) = (b.0 - a.0, b.1 - a.1);
        let tl = (tx * tx + ty * ty).sqrt().max(1e-3);
        let (tx, ty) = (tx / tl, ty / tl);
        let ang = (mid.1 - center.1).atan2(mid.0 - center.0);
        let (nx, ny) = (ang.cos(), ang.sin());
        let ph = j as f32 * 0.9;
        let flutter = (w * 2.0 + ph).sin() * 0.5 + (w * 3.0 + ph * 1.7).sin() * 0.25 + gust * 0.3;
        let l = len * (1.0 + 0.08 * (w * 2.0 + ph).sin());
        let lift = 0.2 + 0.1 * flutter;
        let tip = (mid.0 + (tx * 0.8 + nx * lift) * l - 14.0 - flutter * 6.0, mid.1 + (ty * 0.8 + ny * lift) * l);
        locks.push(curve(a, tip, 0.12));
        locks.push(curve(tip, b, 0.10).into_iter().rev().collect());
        // A strand inside the clump.
        let inner = lerp(a, b, 0.5);
        fringe.push(curve(lerp(inner, center, 0.2), lerp(tip, inner, 0.2), 0.1));
    }
    let mut flyaways = Vec::new();
    for k in 0..4 {
        let ph = k as f32 * 1.3;
        let base = at(0.2 + k as f32 * 0.15);
        let a = -2.75 + (w * 2.0 + ph).sin() * 0.3;
        flyaways.push(lock(base, a, 70.0 + k as f32 * 14.0, -0.8 + (w * 3.0 + ph).sin() * 0.4));
    }
    // Scarf wrap and its two tails.
    let scarf_top = mvb(pose, &[(470.0, 520.0), (520.0, 512.0), (566.0, 524.0), (612.0, 532.0), (652.0, 544.0), (666.0, 562.0)]);
    let scarf_bottom = mvb(pose, &[(462.0, 610.0), (520.0, 626.0), (580.0, 628.0), (632.0, 620.0), (676.0, 600.0)]);
    let scarf_folds = vec![
        mvb(pose, &[(520.0, 540.0), (552.0, 566.0), (600.0, 580.0), (650.0, 576.0)]),
        mvb(pose, &[(500.0, 578.0), (548.0, 600.0), (610.0, 604.0)]),
        mvb(pose, &[(640.0, 556.0), (664.0, 584.0), (668.0, 604.0)]),
    ];
    let lift = pose.body(560.0);
    let tails = vec![
        tail((486.0 + lift.0, 548.0 + lift.1), 400.0, 74.0, w, 0.0, 30.0),
        tail((480.0 + lift.0, 592.0 + lift.1), 300.0, 60.0, w, 1.7, 70.0),
    ];

    // Oversized hooded coat.
    let hood = mvb(pose, &[(504.0, 538.0), (452.0, 540.0), (404.0, 566.0), (382.0, 610.0), (392.0, 652.0), (380.0, 690.0), (412.0, 716.0), (468.0, 712.0)]);
    let flap = |y: f32| ((w * 2.0 - y * 0.012).sin() * 7.0 + (w * 3.0 - y * 0.02).sin() * 3.0) * ((y - 860.0) / 500.0).clamp(0.0, 1.0);
    let back: Vec<P> = mvb(pose, &[(420.0, 690.0), (378.0, 760.0), (352.0, 880.0), (340.0, 1030.0), (334.0, 1200.0), (330.0, 1360.0)]).into_iter().map(|(x, y)| (x - flap(y), y)).collect();
    let front = mvb(pose, &[(668.0, 612.0), (694.0, 690.0), (716.0, 820.0), (730.0, 980.0), (740.0, 1160.0), (748.0, 1360.0)]);
    let opening = mvb(pose, &[(640.0, 628.0), (660.0, 720.0), (676.0, 880.0), (690.0, 1060.0), (700.0, 1360.0)]);
    // The near arm covers the side of the body: its back edge is the coat's
    // back, so only the bottom of the sleeve is drawn.
    let sleeve_back = mvb(pose, &[(346.0, 990.0), (366.0, 1080.0), (418.0, 1140.0), (482.0, 1168.0), (540.0, 1174.0)]);
    let sleeve_front = mvb(pose, &[(470.0, 664.0), (530.0, 650.0), (582.0, 672.0), (618.0, 760.0), (642.0, 880.0), (648.0, 1000.0), (634.0, 1098.0), (606.0, 1150.0)]);
    let cuff = mvb(pose, &[(536.0, 1172.0), (572.0, 1178.0), (606.0, 1150.0)]);
    let creases = vec![
        mvb(pose, &[(470.0, 560.0), (430.0, 600.0), (428.0, 650.0), (452.0, 676.0)]),
        mvb(pose, &[(606.0, 1176.0), (650.0, 1170.0), (696.0, 1178.0)]),
        mvb(pose, &[(604.0, 1150.0), (626.0, 1166.0), (630.0, 1178.0)]),
        mvb(pose, &[(460.0, 900.0), (500.0, 884.0), (530.0, 890.0)]),
        mvb(pose, &[(500.0, 952.0), (548.0, 934.0), (586.0, 940.0)]),
        mvb(pose, &[(470.0, 1040.0), (520.0, 1030.0)]),
        mvb(pose, &[(560.0, 760.0), (540.0, 840.0)]),
        mvb(pose, &[(380.0, 1100.0), (400.0, 1220.0)]),
    ];
    let sway = (w + 0.6).sin() * 10.0 + (w * 2.0).sin() * 4.0;
    let string = mvb(pose, &[(652.0, 630.0), (656.0 + sway * 0.3, 690.0), (662.0 + sway * 0.7, 750.0), (668.0 + sway, 800.0)]);

    // Areas for the watercolor.
    let mut coat_poly = vec![];
    coat_poly.extend(hood.iter().cloned());
    coat_poly.extend(back.iter().cloned());
    coat_poly.push((748.0, 1360.0));
    coat_poly.extend(front.iter().rev().cloned());
    coat_poly.extend(scarf_bottom.iter().rev().cloned());
    let mut scarf_poly = scarf_top.clone();
    scarf_poly.extend(scarf_bottom.iter().rev().cloned());
    let mut hair_poly = skull.clone();
    hair_poly.extend(mv(&[(520.0, 360.0), (560.0, 300.0), (598.0, 278.0)], h));
    let mut skin_poly = mv(&[(598.0, 284.0), (636.0, 330.0), (684.0, 398.0), (660.0, 450.0), (620.0, 490.0), (588.0, 500.0), (596.0, 560.0)], h);
    skin_poly.extend(mv(&[(512.0, 540.0), (506.0, 440.0), (530.0, 360.0), (566.0, 300.0)], h));

    Shapes {
        face, skull, ear, ear_in, neck_front, neck_back, fringe, locks, flyaways, scarf_top, scarf_bottom, scarf_folds, tails, hood, back, front, opening,
        sleeve_back, sleeve_front, cuff, creases, string, coat_poly, scarf_poly, hair_poly, skin_poly,
    }
}

fn draw_ink(img: &mut TiledImage, pens: &Pens, s: &Shapes, seed: u64) {
    let mut pen = Pen { img, rng: Rng::new(seed), boil: 1.6, seed: seed * 1000 };
    pen.ink(pens, &s.face, 5.0, Press::Swell);
    pen.ink(pens, &s.ear, 4.0, Press::Swell);
    pen.ink(pens, &s.ear_in, 3.0, Press::Swell);
    pen.ink(pens, &s.neck_front, 4.5, Press::Swell);
    pen.ink(pens, &s.neck_back, 4.5, Press::Swell);
    // Hair outline only in parts: the locks finish it.
    for f in &s.fringe {
        pen.ink(pens, f, 3.0, Press::Swell);
    }
    for l in &s.locks {
        pen.ink(pens, l, 5.0, Press::Swell);
    }
    for l in &s.flyaways {
        pen.line(&pens.pencil, l, 3.0, Rgba::new(0.15, 0.13, 0.12, 1.0), 0.8, Press::Flick);
    }
    pen.ink(pens, &s.scarf_top, 5.5, Press::Swell);
    pen.ink(pens, &s.scarf_bottom, 5.5, Press::Swell);
    for f in &s.scarf_folds {
        pen.ink(pens, f, 3.5, Press::Swell);
    }
    for (_, a, b) in &s.tails {
        pen.ink(pens, a, 5.0, Press::Swell);
        pen.ink(pens, b, 5.0, Press::Swell);
        // Frayed end.
        let (ea, eb) = (*a.last().unwrap(), *b.last().unwrap());
        for k in 0..5 {
            let p = lerp(ea, eb, k as f32 / 4.0);
            pen.ink(pens, &[p, (p.0 - 26.0, p.1 + 4.0 + k as f32 * 2.0)], 3.0, Press::Flick);
        }
    }
    pen.ink(pens, &s.hood, 6.0, Press::Swell);
    pen.ink(pens, &s.back, 6.5, Press::Swell);
    pen.ink(pens, &s.front, 6.0, Press::Swell);
    pen.ink(pens, &s.opening, 4.0, Press::Swell);
    pen.ink(pens, &s.sleeve_back, 6.0, Press::Swell);
    pen.ink(pens, &s.sleeve_front, 5.5, Press::Swell);
    pen.ink(pens, &s.cuff, 5.0, Press::Swell);
    for c in &s.creases {
        pen.ink(pens, c, 3.5, Press::Swell);
    }
    pen.ink(pens, &s.string, 3.5, Press::Flick);
}

fn draw_sketch(img: &mut TiledImage, pens: &Pens, s: &Shapes, pose: &Pose, seed: u64) {
    let graphite = Rgba::new(0.30, 0.30, 0.34, 1.0);
    let mut pen = Pen { img, rng: Rng::new(seed ^ 0xABCD), boil: 6.0, seed: seed * 1000 + 500 };
    let h = pose.head();
    // Construction: head oval, jaw, gesture of the back, shoulder line.
    let oval: Vec<P> = (0..=24).map(|k| {
        let a = k as f32 / 24.0 * TAU + 0.3;
        (532.0 + h.0 + a.cos() * 112.0, 330.0 + h.1 + a.sin() * 122.0)
    }).collect();
    pen.line(&pens.sketch, &oval, 9.0, graphite, 1.0, Press::Swell);
    pen.line(&pens.sketch, &mv(&[(470.0, 200.0), (440.0, 420.0), (430.0, 700.0), (360.0, 1000.0), (320.0, 1350.0)], h), 8.0, graphite, 0.9, Press::Swell);
    pen.line(&pens.sketch, &mvb(pose, &[(380.0, 690.0), (560.0, 650.0), (720.0, 640.0)]), 8.0, graphite, 0.9, Press::Swell);
    pen.line(&pens.sketch, &mv(&[(560.0, 250.0), (690.0, 400.0)], h), 7.0, graphite, 0.8, Press::Flat);
    // Overshooting outlines, like a quick under-drawing.
    for line in [&s.face, &s.back, &s.front, &s.sleeve_back, &s.hood] {
        let ext: Vec<P> = line.iter().map(|&(x, y)| (x - 6.0, y + 4.0)).collect();
        pen.line(&pens.sketch, &ext, 8.0, graphite, 0.8, Press::Swell);
    }
    for (c, _, _) in &s.tails {
        pen.line(&pens.sketch, c, 8.0, graphite, 0.8, Press::Swell);
    }
    // Hatching in the shadow side of the coat and under the hair.
    let mut r = Rng::new(seed ^ 0x77);
    for k in 0..18 {
        let x = 360.0 + k as f32 * 9.0 + r.s() * 3.0;
        let y = 760.0 + k as f32 * 22.0;
        let d = pose.body(y);
        pen.line(&pens.pencil, &[(x + d.0, y + d.1), (x + 34.0 + d.0, y - 40.0 + d.1)], 3.0, graphite, 0.9, Press::Swell);
    }
    for k in 0..9 {
        let x = 470.0 + k as f32 * 8.0;
        let y = 420.0 + k as f32 * 6.0;
        pen.line(&pens.pencil, &mv(&[(x, y), (x + 20.0, y - 32.0)], h), 2.5, graphite, 0.9, Press::Swell);
    }
}

fn draw_color(img: &mut TiledImage, pens: &Pens, s: &Shapes, seed: u64) {
    let mut pen = Pen { img, rng: Rng::new(seed ^ 0x5151), boil: 3.0, seed: seed * 1000 + 800 };
    // Muted coat, rust scarf, a blush of skin, dark hair.
    pen.wash(pens, &s.coat_poly, Rgba::new(0.48, 0.57, 0.63, 1.0), 46.0, 1.05, 30.0, 0.75);
    pen.wash(pens, &s.scarf_poly, Rgba::new(0.76, 0.32, 0.22, 1.0), 40.0, 0.2, 22.0, 0.95);
    for (_, a, b) in &s.tails {
        let mut poly = a.clone();
        poly.extend(b.iter().rev().cloned());
        pen.wash(pens, &poly, Rgba::new(0.76, 0.32, 0.22, 1.0), 34.0, 0.0, 20.0, 0.9);
    }
    pen.wash(pens, &s.hair_poly, Rgba::new(0.28, 0.22, 0.20, 1.0), 40.0, 2.3, 26.0, 0.6);
    pen.wash(pens, &s.skin_poly, Rgba::new(0.96, 0.78, 0.66, 1.0), 40.0, 0.9, 30.0, 0.35);
}

/// Petals blown across the frame (ink + color), one loop per animation loop.
fn draw_petals(ink_img: &mut TiledImage, color_img: &mut TiledImage, pens: &Pens, pose: &Pose, seed: u64) {
    let mut pen = Pen { img: ink_img, rng: Rng::new(seed ^ 0x9191), boil: 0.8, seed: seed * 1000 + 900 };
    let mut shapes = Vec::new();
    for k in 0..6 {
        let t = (pose.w / TAU + k as f32 / 6.0) % 1.0;
        let x = 1160.0 - t * 1300.0;
        let y = 180.0 + k as f32 * 170.0 + (pose.w * 2.0 + k as f32).sin() * 40.0 + t * 80.0;
        let a = pose.w * 2.0 + k as f32 * 1.1;
        let size = 16.0 + (k % 3) as f32 * 5.0;
        let (ca, sa) = (a.cos(), a.sin());
        let pt = |u: f32, v: f32| (x + ca * u - sa * v, y + sa * u + ca * v);
        let side1 = vec![pt(-size, 0.0), pt(0.0, -size * 0.55), pt(size, 0.0)];
        let side2 = vec![pt(-size, 0.0), pt(0.0, size * 0.45), pt(size, 0.0)];
        pen.ink(pens, &side1, 3.0, Press::Swell);
        pen.ink(pens, &side2, 3.0, Press::Swell);
        let mut poly = side1.clone();
        poly.extend(side2.iter().rev().cloned());
        shapes.push(poly);
    }
    // Speed lines of the wind, in pencil.
    let mut r = Rng::new(7);
    for k in 0..4 {
        let t = (pose.w / TAU * 2.0 + k as f32 / 4.0) % 1.0;
        let y = 140.0 + r.f() * 1000.0;
        let x = 1200.0 - t * 1500.0;
        pen.line(&pens.pencil, &[(x, y), (x - 90.0, y + 6.0), (x - 160.0, y + 2.0)], 2.5, Rgba::new(0.3, 0.3, 0.32, 1.0), 0.45, Press::Swell);
    }
    let mut cp = Pen { img: color_img, rng: Rng::new(seed ^ 0x4242), boil: 1.0, seed: seed * 1000 + 950 };
    for poly in shapes {
        cp.wash(pens, &poly, Rgba::new(0.86, 0.50, 0.42, 1.0), 14.0, 0.5, 8.0, 0.8);
    }
}

/// Cold-press watercolor paper: warm cream, embossed grain, faint stains.
fn paper() -> TiledImage {
    let mut img = TiledImage::new(W, H);
    let mut row = vec![0u8; W as usize * 4];
    for y in 0..H as i32 {
        for x in 0..W as i32 {
            let (fx, fy) = (x as f32, y as f32);
            // Height of the paper surface, lit from the top left.
            let hgt = |px: f32, py: f32| fbm(px / 9.0, py / 9.0, 4, 3) * 0.7 + fbm(px / 2.5, py / 2.5, 2, 9) * 0.3;
            let shade = (hgt(fx - 1.0, fy - 1.0) - hgt(fx + 1.0, fy + 1.0)) * 0.55;
            let stain = (fbm(fx / 260.0, fy / 260.0, 3, 21) - 0.5).max(0.0) * 0.12;
            let cx = (fx / W as f32 - 0.5) * 2.0;
            let cy = (fy / H as f32 - 0.5) * 2.0;
            let vig = (cx * cx + cy * cy) * 0.035;
            let base = [0.925, 0.905, 0.850];
            let tint = [0.0, -0.01, -0.05];
            for c in 0..3 {
                let v = base[c] + shade - stain * (1.0 + c as f32 * 0.4) + tint[c] * stain * 6.0 - vig;
                row[x as usize * 4 + c] = (v.clamp(0.0, 1.0) * 255.0) as u8;
            }
            row[x as usize * 4 + 3] = 255;
        }
        img.write_region(0, y, W as usize, 1, &row);
    }
    img
}

fn save_png(p: &Project, i: usize, path: &std::path::Path, scale: u32) {
    let mut px = Vec::new();
    render_region(p, i, IRect::from_size(W, H), &RenderOptions { background: true, ..Default::default() }, &mut px);
    unpremultiply(&mut px);
    let img = image::RgbaImage::from_raw(W, H, px).unwrap();
    let img = if scale > 1 { image::imageops::resize(&img, W / scale, H / scale, image::imageops::FilterType::Triangle) } else { img };
    img.save(path).unwrap();
}

fn main() {
    let out: PathBuf = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| "Viento.anim".into());
    let preview = std::env::args().nth(2).map(PathBuf::from);
    let pens = Pens::new();
    let canvas = CanvasSettings { width: W, height: H, fps: FPS, background: Rgba::new(0.925, 0.905, 0.85, 1.0), ..Default::default() };
    let mut p = Project::new("Viento", canvas);
    p.layers.clear();
    p.frames.clear();
    let id = |p: &mut Project| p.alloc_id();
    let paper_id = id(&mut p);
    let sketch_id = id(&mut p);
    let color_id = id(&mut p);
    let ink_id = id(&mut p);
    p.layers.push(Layer { id: paper_id, props: LayerProps { shared: true, onion_skin: false, locked: true, ..LayerProps::named("Paper") } });
    p.layers.push(Layer { id: sketch_id, props: LayerProps { opacity: 0.8, onion_skin: false, ..LayerProps::named("Sketch") } });
    p.layers.push(Layer { id: color_id, props: LayerProps { blend: BlendMode::Multiply, opacity: 0.85, onion_skin: false, ..LayerProps::named("Watercolor") } });
    p.layers.push(Layer { id: ink_id, props: LayerProps::named("Ink") });
    p.shared_cels.insert(paper_id, paper());

    for i in 0..N {
        let pose = Pose::at(i);
        let s = shapes(&pose);
        let fid = p.alloc_id();
        let mut f = Frame::new(fid);
        let mut ink = TiledImage::new(W, H);
        let mut color = TiledImage::new(W, H);
        let mut sketch = TiledImage::new(W, H);
        draw_color(&mut color, &pens, &s, i as u64 + 1);
        draw_sketch(&mut sketch, &pens, &s, &pose, i as u64 + 1);
        draw_ink(&mut ink, &pens, &s, i as u64 + 1);
        draw_petals(&mut ink, &mut color, &pens, &pose, i as u64 + 1);
        f.cels.insert(ink_id, ink);
        f.cels.insert(color_id, color);
        f.cels.insert(sketch_id, sketch);
        p.frames.push(f);
        eprint!("\rdibujo {}/{N}", i + 1);
    }
    eprintln!();
    if let Some(dir) = preview {
        std::fs::create_dir_all(&dir).unwrap();
        save_png(&p, 0, &dir.join("f00.png"), 1);
        for i in [0, 6, 12, 18] {
            save_png(&p, i, &dir.join(format!("s{i:02}.png")), 3);
        }
    }
    save_project(&p, &out).expect("save");
    println!("{} -> {} drawings", out.display(), N);
}
