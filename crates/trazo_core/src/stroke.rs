//! The stroke engine: turns pointer samples into brush dabs on a cel.
//!
//! Paint and erase strokes use a per-stroke *coverage* buffer. Each pixel's
//! final value is recomputed from the untouched original tile, so a stroke's
//! opacity is capped by the brush opacity (markers do not darken where they
//! overlap themselves) and the stroke can be cancelled exactly.
//! Smudge and blur modify the layer directly.

use std::collections::HashMap;

use crate::brush::{Accumulate, Brush, BrushMode, DabShape};
use crate::color::{BlendMode, Rgba};
use crate::document::{FrameId, LayerId};
use crate::geom::IRect;
use crate::history::{PixelEdit, PixelEditBuilder};
use crate::raster::{TiledImage, TILE, TILE_PX};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokePoint {
    pub x: f32,
    pub y: f32,
    /// 0..=1. Mouse input uses 1.0 (or a speed-based estimate).
    pub pressure: f32,
}

/// Mirror drawing around the canvas axes.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Symmetry {
    /// Mirror left ↔ right around x = `cx`.
    pub vertical: bool,
    /// Mirror top ↔ bottom around y = `cy`.
    pub horizontal: bool,
    pub cx: f32,
    pub cy: f32,
}

/// User-adjustable settings applied on top of the brush preset.
#[derive(Debug, Clone, Copy)]
pub struct StrokeSettings {
    pub color: Rgba,
    pub size: f32,
    pub opacity: f32,
    /// 0 = raw input, up to ~0.95 = very smooth lines ("lazy rope" length).
    pub stabilizer: f32,
    pub alpha_lock: bool,
    /// Pressure curve: output = input^gamma (<1 softer pen, >1 firmer pen).
    pub pressure_gamma: f32,
    pub symmetry: Symmetry,
}

impl Default for StrokeSettings {
    fn default() -> Self {
        Self {
            color: Rgba::BLACK,
            size: 10.0,
            opacity: 1.0,
            stabilizer: 0.0,
            alpha_lock: false,
            pressure_gamma: 1.0,
            symmetry: Symmetry::default(),
        }
    }
}

/// Longest rope (in canvas pixels) at stabilizer = 1.
const ROPE_MAX: f32 = 40.0;

/// Small deterministic PRNG (xorshift); avoids an external dependency.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 32) as u32
    }

    /// Uniform in 0..1.
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in -1..1.
    pub fn signed(&mut self) -> f32 {
        self.next_f32() * 2.0 - 1.0
    }
}

/// Paper texture: two octaves of smooth value noise, fixed in canvas space so
/// overlapping strokes share the same "tooth".
pub fn paper_grain(x: f32, y: f32, scale: f32) -> f32 {
    let s = scale.max(0.5);
    let a = value_noise(x / s, y / s);
    let b = value_noise(x / (s * 0.45) + 17.3, y / (s * 0.45) - 5.1);
    (a * 0.65 + b * 0.35).clamp(0.0, 1.0)
}

/// Pre-computed tileable paper texture (avoids hashing noise per pixel).
pub struct GrainTexture {
    data: Vec<u8>,
}

const GRAIN_SIZE: i32 = 512;

impl GrainTexture {
    /// Shared texture for this grain scale (built once per scale).
    pub fn new(scale: f32) -> std::sync::Arc<Self> {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};
        static CACHE: OnceLock<Mutex<HashMap<u32, std::sync::Arc<GrainTexture>>>> = OnceLock::new();
        let key = (scale * 16.0).round() as u32;
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
        if let Some(t) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
            return t;
        }
        let t = Self::build(scale);
        if let Ok(mut c) = cache.lock() {
            c.insert(key, t.clone());
        }
        t
    }

    fn build(scale: f32) -> std::sync::Arc<Self> {
        let mut data = Vec::with_capacity((GRAIN_SIZE * GRAIN_SIZE) as usize);
        for y in 0..GRAIN_SIZE {
            for x in 0..GRAIN_SIZE {
                data.push((paper_grain(x as f32, y as f32, scale) * 255.0) as u8);
            }
        }
        std::sync::Arc::new(Self { data })
    }

    #[inline]
    pub fn sample(&self, x: i32, y: i32) -> f32 {
        let (x, y) = (x.rem_euclid(GRAIN_SIZE), y.rem_euclid(GRAIN_SIZE));
        self.data[(y * GRAIN_SIZE + x) as usize] as f32 / 255.0
    }
}

fn hash2(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841);
    h = (h ^ (h >> 13)).wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

fn value_noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash2(xi, yi);
    let b = hash2(xi + 1, yi);
    let c = hash2(xi, yi + 1);
    let d = hash2(xi + 1, yi + 1);
    let top = a + (b - a) * sx;
    let bot = c + (d - c) * sx;
    top + (bot - top) * sy
}

/// Alpha of the dab shape at offset (dx, dy) from its center.
fn shape_alpha(b: &Brush, angle: f32, dx: f32, dy: f32, radius: f32, force_round: bool) -> f32 {
    if b.shape == DabShape::Pixel && !force_round {
        let half = (radius * 2.0).max(1.0) / 2.0;
        return if dx.abs() < half && dy.abs() < half { 1.0 } else { 0.0 };
    }
    if b.shape == DabShape::Image && !force_round {
        if let Some(tip) = &b.tip {
            let (s, c) = angle.sin_cos();
            let x = dx * c + dy * s;
            let y = (-dx * s + dy * c) / b.roundness.clamp(0.05, 1.0);
            // The tip's longest side spans the brush diameter.
            let k = (radius * 2.0) / tip.width.max(tip.height) as f32;
            return tip.sample(x / k + tip.width as f32 * 0.5, y / k + tip.height as f32 * 0.5);
        }
    }
    let (rx, ry) = if (b.roundness - 1.0).abs() > 1e-3 || angle != 0.0 {
        let (s, c) = angle.sin_cos();
        let x = dx * c + dy * s;
        let y = (-dx * s + dy * c) / b.roundness.clamp(0.05, 1.0);
        (x, y)
    } else {
        (dx, dy)
    };
    let dist = (rx * rx + ry * ry).sqrt();
    // 1px anti-aliased edge.
    let edge = (radius - dist + 0.5).clamp(0.0, 1.0);
    if edge <= 0.0 {
        return 0.0;
    }
    let h = if force_round { 0.8 } else { b.hardness.clamp(0.0, 1.0) };
    let inner = radius * h;
    if dist <= inner || radius - inner < 0.5 {
        return edge;
    }
    let t = ((dist - inner) / (radius - inner)).clamp(0.0, 1.0);
    let soft = 1.0 - t * t * (3.0 - 2.0 * t);
    soft.min(edge)
}

pub struct Stroke {
    brush: Brush,
    settings: StrokeSettings,
    frame: FrameId,
    layer: LayerId,
    edit: PixelEditBuilder,
    coverage: HashMap<usize, Vec<f32>>,
    rng: Rng,
    /// Last emitted (smoothed) point.
    last: Option<StrokePoint>,
    /// Last raw input point, used to catch up the stabilizer at the end.
    last_raw: Option<StrokePoint>,
    /// Distance carried over to the next dab.
    residual: f32,
    travelled: f32,
    dirty: IRect,
    smudge: Option<[f32; 4]>,
    /// Every coverage dab, so the stroke can be re-rendered with an end taper.
    dabs: Vec<Dab>,
    replaying: bool,
    /// Direction of travel in radians (for tips that follow the stroke).
    direction: f32,
    /// Stabilizer position (end of the rope).
    lazy: Option<StrokePoint>,
    /// Last points used for curve smoothing.
    ctrl: Vec<StrokePoint>,
    /// Paper texture, built on first use.
    grain: Option<std::sync::Arc<GrainTexture>>,
}

#[derive(Debug, Clone, Copy)]
struct Dab {
    x: f32,
    y: f32,
    radius: f32,
    strength: f32,
    spray_dot: bool,
    angle: f32,
    /// Stroke length when this dab was placed.
    dist: f32,
}

impl Stroke {
    pub fn new(brush: Brush, settings: StrokeSettings, frame: FrameId, layer: LayerId, seed: u64) -> Self {
        Self {
            brush,
            settings,
            frame,
            layer,
            edit: PixelEditBuilder::new(),
            coverage: HashMap::new(),
            rng: Rng::new(seed),
            last: None,
            last_raw: None,
            residual: 0.0,
            travelled: 0.0,
            dirty: IRect::EMPTY,
            smudge: None,
            dabs: Vec::new(),
            replaying: false,
            direction: 0.0,
            lazy: None,
            ctrl: Vec::with_capacity(4),
            grain: None,
        }
    }

    pub fn frame(&self) -> FrameId {
        self.frame
    }

    pub fn layer(&self) -> LayerId {
        self.layer
    }

    /// Area changed since the last call (for partial redraws).
    pub fn take_dirty(&mut self) -> IRect {
        std::mem::replace(&mut self.dirty, IRect::EMPTY)
    }

    /// Feed one input sample.
    pub fn add_point(&mut self, img: &mut TiledImage, mut raw: StrokePoint) {
        let gamma = self.settings.pressure_gamma.clamp(0.2, 5.0);
        raw.pressure = raw.pressure.clamp(0.0, 1.0).powf(gamma);
        self.last_raw = Some(raw);
        // "Lazy rope" stabilizer: the brush only moves when the pointer pulls
        // the rope tight. Unlike averaging, it doesn't depend on event rate.
        let rope = self.settings.stabilizer.clamp(0.0, 1.0) * ROPE_MAX;
        let p = match self.lazy {
            None => raw,
            Some(l) => {
                let (dx, dy) = (raw.x - l.x, raw.y - l.y);
                let d = (dx * dx + dy * dy).sqrt();
                if d <= rope {
                    self.lazy = Some(StrokePoint { pressure: raw.pressure, ..l });
                    return;
                }
                let k = if d > 0.0 { (d - rope) / d } else { 0.0 };
                StrokePoint { x: l.x + dx * k, y: l.y + dy * k, pressure: raw.pressure }
            }
        };
        self.lazy = Some(p);
        self.push_smooth(img, p);
    }

    /// Catmull-Rom smoothing: segments are drawn one input point behind, so
    /// fast curves come out round instead of as polygons.
    fn push_smooth(&mut self, img: &mut TiledImage, p: StrokePoint) {
        if let Some(last) = self.ctrl.last() {
            if (last.x - p.x).abs() < 0.05 && (last.y - p.y).abs() < 0.05 {
                return;
            }
        }
        self.ctrl.push(p);
        if self.ctrl.len() > 4 {
            self.ctrl.remove(0);
        }
        let c = &self.ctrl;
        match c.len() {
            1 => self.emit_to(img, p),
            3 => {
                let (a, b, d) = (c[0], c[1], c[2]);
                self.curve(img, a, a, b, d);
            }
            4 => {
                let (a, b, cc, d) = (c[0], c[1], c[2], c[3]);
                self.curve(img, a, b, cc, d);
            }
            _ => {}
        }
    }

    /// Draw the Catmull-Rom segment p1 → p2.
    fn curve(&mut self, img: &mut TiledImage, p0: StrokePoint, p1: StrokePoint, p2: StrokePoint, p3: StrokePoint) {
        let len = ((p2.x - p1.x).powi(2) + (p2.y - p1.y).powi(2)).sqrt();
        let n = (len / 2.0).ceil().clamp(1.0, 256.0) as usize;
        for i in 1..=n {
            let t = i as f32 / n as f32;
            let (t2, t3) = (t * t, t * t * t);
            let cr = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b + (-a + c) * t + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2 + (-a + 3.0 * b - 3.0 * c + d) * t3)
            };
            let q = StrokePoint {
                x: cr(p0.x, p1.x, p2.x, p3.x),
                y: cr(p0.y, p1.y, p2.y, p3.y),
                pressure: p1.pressure + (p2.pressure - p1.pressure) * t,
            };
            self.emit_to(img, q);
        }
    }

    /// Finish the stroke and return its undo record (None if nothing changed).
    pub fn finish(mut self, img: &mut TiledImage) -> Option<PixelEdit> {
        // Let the stabilized line catch up with the pen, then draw the last
        // smoothed segment.
        if let Some(raw) = self.last_raw {
            if self.lazy.is_some_and(|l| (l.x - raw.x).abs() + (l.y - raw.y).abs() > 0.1) {
                self.push_smooth(img, raw);
            }
        }
        let c = self.ctrl.clone();
        if c.len() >= 2 {
            let n = c.len();
            let (a, b) = (c[n - 2], c[n - 1]);
            let prev = if n >= 3 { c[n - 3] } else { a };
            self.curve(img, prev, a, b, b);
        }
        self.apply_end_taper(img);
        let (frame, layer) = (self.frame, self.layer);
        self.edit.finish(img, frame, layer)
    }

    /// Thin out the last `taper_end` pixels by re-rendering the stroke from
    /// its recorded dabs (only paint/erase strokes use coverage dabs).
    fn apply_end_taper(&mut self, img: &mut TiledImage) {
        let taper = self.brush.taper_end;
        if taper <= 0.0 || self.dabs.is_empty() {
            return;
        }
        let total = self.travelled;
        self.edit.restore(img);
        for c in self.coverage.values_mut() {
            c.fill(0.0);
        }
        self.replaying = true;
        let dabs = std::mem::take(&mut self.dabs);
        for d in &dabs {
            let f = ((total - d.dist) / taper).clamp(0.12, 1.0);
            self.coverage_dab(img, d.x, d.y, d.radius * f, d.strength, d.spray_dot, d.angle);
        }
        self.dabs = dabs;
        self.replaying = false;
    }

    /// Abort and restore the cel (e.g. when a second finger starts a pinch).
    pub fn cancel(self, img: &mut TiledImage) -> IRect {
        let mut r = IRect::EMPTY;
        // Only the touched tiles can have changed.
        let tiles: Vec<usize> = self.edit.touched().collect();
        self.edit.revert(img);
        for idx in tiles {
            let (tx, ty) = ((idx % img.tiles_x()) as i32, (idx / img.tiles_x()) as i32);
            let t = TILE as i32;
            r = r.union(&IRect::new(tx * t, ty * t, tx * t + t, ty * t + t));
        }
        r
    }

    /// Diameter for a pressure value, before tapering and jitter.
    fn base_diameter(&self, pressure: f32) -> f32 {
        let b = &self.brush;
        let mut d = self.settings.size.max(0.5);
        if b.pressure_size {
            d *= b.min_size + (1.0 - b.min_size) * pressure.clamp(0.0, 1.0);
        }
        d
    }

    fn diameter(&mut self, pressure: f32) -> f32 {
        let b = &self.brush;
        let mut d = self.base_diameter(pressure);
        if b.taper_start > 0.0 {
            d *= (self.travelled / b.taper_start).clamp(0.15, 1.0);
        }
        if b.size_jitter > 0.0 {
            d *= 1.0 + b.size_jitter * self.rng.signed();
        }
        d.max(0.5)
    }

    fn emit_to(&mut self, img: &mut TiledImage, p: StrokePoint) {
        let Some(prev) = self.last else {
            self.last = Some(p);
            self.dab(img, p);
            return;
        };
        let (dx, dy) = (p.x - prev.x, p.y - prev.y);
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 1e-3 {
            return;
        }
        self.direction = dy.atan2(dx);
        // Spacing follows the current dab size, so big brushes stay fast.
        let base_d = self.base_diameter(prev.pressure.max(p.pressure));
        let step = (self.brush.spacing * base_d.max(1.0)).max(0.5);
        let mut t = step - self.residual;
        while t <= dist {
            let f = t / dist;
            let q = StrokePoint {
                x: prev.x + dx * f,
                y: prev.y + dy * f,
                pressure: prev.pressure + (p.pressure - prev.pressure) * f,
            };
            self.travelled += step;
            self.dab(img, q);
            t += step;
        }
        self.residual = dist - (t - step);
        self.last = Some(p);
    }

    fn dab(&mut self, img: &mut TiledImage, p: StrokePoint) {
        let d = self.diameter(p.pressure);
        let (mut cx, mut cy) = (p.x, p.y);
        if self.brush.jitter > 0.0 {
            cx += self.rng.signed() * self.brush.jitter * d;
            cy += self.rng.signed() * self.brush.jitter * d;
        }
        let mut angle = self.brush.angle_deg.to_radians();
        if self.brush.follow_direction {
            angle += self.direction;
        }
        if self.brush.rotation_jitter > 0.0 {
            angle += self.rng.signed() * self.brush.rotation_jitter.to_radians();
        }
        let mut strength = self.brush.flow;
        if self.brush.pressure_opacity {
            strength *= 0.3 + 0.7 * p.pressure.clamp(0.0, 1.0);
        }
        match self.brush.mode {
            BrushMode::Paint | BrushMode::Erase => match self.brush.shape {
                DabShape::Spray => {
                    let n = self.brush.density.max(1);
                    let dot = (d * 0.025).max(0.6);
                    for _ in 0..n {
                        let ang = self.rng.next_f32() * std::f32::consts::TAU;
                        let rad = self.rng.next_f32().sqrt() * d * 0.5;
                        for (x, y, _) in self.mirrored(cx + ang.cos() * rad, cy + ang.sin() * rad, 0.0) {
                            self.coverage_dab(img, x, y, dot, strength, true, 0.0);
                        }
                    }
                }
                _ => {
                    for (x, y, ang) in self.mirrored(cx, cy, angle) {
                        self.coverage_dab(img, x, y, d * 0.5, strength, false, ang);
                    }
                }
            },
            BrushMode::Smudge => self.smudge_dab(img, cx, cy, d * 0.5, strength),
            BrushMode::Blur => self.blur_dab(img, cx, cy, d * 0.5, strength),
        }
    }

    /// The dab position plus its mirror images when symmetry is on.
    fn mirrored(&self, x: f32, y: f32, angle: f32) -> Vec<(f32, f32, f32)> {
        let sym = self.settings.symmetry;
        let mut v = vec![(x, y, angle)];
        if sym.vertical {
            v.push((2.0 * sym.cx - x, y, std::f32::consts::PI - angle));
        }
        if sym.horizontal {
            v.push((x, 2.0 * sym.cy - y, -angle));
        }
        if sym.vertical && sym.horizontal {
            v.push((2.0 * sym.cx - x, 2.0 * sym.cy - y, angle + std::f32::consts::PI));
        }
        v
    }

    #[allow(clippy::too_many_arguments)]
    fn coverage_dab(&mut self, img: &mut TiledImage, cx: f32, cy: f32, radius: f32, strength: f32, spray_dot: bool, angle: f32) {
        if !self.replaying && self.brush.taper_end > 0.0 {
            self.dabs.push(Dab { x: cx, y: cy, radius, strength, spray_dot, angle, dist: self.travelled });
        }
        let (cx, cy) = if self.brush.shape == DabShape::Pixel && !spray_dot {
            // Snap to the pixel grid so pixel-art lines stay crisp.
            let size = (radius * 2.0).max(1.0).round();
            let off = if size as i32 % 2 == 1 { 0.5 } else { 0.0 };
            (cx.floor() + off, cy.floor() + off)
        } else {
            (cx, cy)
        };
        let reach = if self.brush.shape == DabShape::Image && !spray_dot { radius * 1.42 } else { radius };
        let area = IRect::around(cx, cy, reach).intersect(&img.bounds());
        if area.is_empty() {
            return;
        }
        let erase = self.brush.mode == BrushMode::Erase;
        let color = self.settings.color;
        let opacity = (self.settings.opacity * if erase { 1.0 } else { color.a }).clamp(0.0, 1.0);
        let grain = self.brush.grain;
        let grain_tex = self.grain.get_or_insert_with(|| GrainTexture::new(self.brush.grain_scale)).clone();
        let accumulate = self.brush.accumulate;
        let alpha_lock = self.settings.alpha_lock && !erase;
        let blend = self.brush.blend;
        let tiles: Vec<(usize, usize)> = img.tiles_in_rect(&area).collect();
        for (tx, ty) in tiles {
            let idx = img.tile_index(tx, ty);
            self.edit.touch(img, idx);
            let original = self.edit.original(idx).cloned().flatten();
            let cov = self.coverage.entry(idx).or_insert_with(|| vec![0.0; TILE_PX]);
            let tile_rect = IRect::new((tx * TILE) as i32, (ty * TILE) as i32, ((tx + 1) * TILE) as i32, ((ty + 1) * TILE) as i32);
            let r = area.intersect(&tile_rect);
            // Alpha-locked strokes on empty tiles can't change anything.
            if alpha_lock && original.is_none() {
                continue;
            }
            let tile = img.tile_mut(idx).px_mut();
            let original = original.as_ref().map(|t| t.px());
            for y in r.y0..r.y1 {
                for x in r.x0..r.x1 {
                    let mut a = shape_alpha(&self.brush, angle, x as f32 + 0.5 - cx, y as f32 + 0.5 - cy, radius, spray_dot);
                    if a <= 0.0 {
                        continue;
                    }
                    a *= strength;
                    if grain > 0.0 {
                        let g = grain_tex.sample(x, y);
                        a *= 1.0 - grain * (1.0 - g * g * 1.6).clamp(0.0, 1.0);
                    }
                    let li = (y as usize % TILE) * TILE + (x as usize % TILE);
                    let c = &mut cov[li];
                    *c = match accumulate {
                        Accumulate::Max => c.max(a),
                        Accumulate::Buildup => *c + a * (1.0 - *c),
                    };
                    let alpha = *c * opacity;
                    let o = li * 4;
                    let before = match &original {
                        Some(t) => [t[o] as f32, t[o + 1] as f32, t[o + 2] as f32, t[o + 3] as f32],
                        None => [0.0; 4],
                    };
                    let out = if erase {
                        [before[0] * (1.0 - alpha), before[1] * (1.0 - alpha), before[2] * (1.0 - alpha), before[3] * (1.0 - alpha)]
                    } else if alpha_lock {
                        let da = before[3];
                        [
                            before[0] * (1.0 - alpha) + color.r * da * alpha,
                            before[1] * (1.0 - alpha) + color.g * da * alpha,
                            before[2] * (1.0 - alpha) + color.b * da * alpha,
                            da,
                        ]
                    } else if blend != BlendMode::Normal {
                        let src = color.premul8(alpha).map(|v| (v + 0.5).clamp(0.0, 255.0) as u8);
                        let b = crate::color::blend_px(before.map(|v| v / 255.0), src, 1.0, blend);
                        b.map(|v| v * 255.0)
                    } else {
                        let k = 1.0 - alpha;
                        [
                            color.r * alpha * 255.0 + before[0] * k,
                            color.g * alpha * 255.0 + before[1] * k,
                            color.b * alpha * 255.0 + before[2] * k,
                            alpha * 255.0 + before[3] * k,
                        ]
                    };
                    for i in 0..4 {
                        tile[o + i] = (out[i] + 0.5).clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
        self.dirty = self.dirty.union(&area);
    }

    fn smudge_dab(&mut self, img: &mut TiledImage, cx: f32, cy: f32, radius: f32, strength: f32) {
        let area = IRect::around(cx, cy, radius).intersect(&img.bounds());
        if area.is_empty() {
            return;
        }
        // Average color under the dab (premultiplied).
        let mut sum = [0.0f32; 4];
        let mut wsum = 0.0;
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let w = shape_alpha(&self.brush, 0.0, x as f32 + 0.5 - cx, y as f32 + 0.5 - cy, radius, false);
                if w > 0.0 {
                    let p = img.pixel(x, y);
                    for i in 0..4 {
                        sum[i] += p[i] as f32 * w;
                    }
                    wsum += w;
                }
            }
        }
        if wsum <= 0.0 {
            return;
        }
        let avg = sum.map(|v| v / wsum);
        let carry = *self.smudge.get_or_insert(avg);
        for (tx, ty) in img.tiles_in_rect(&area).collect::<Vec<_>>() {
            let idx = img.tile_index(tx, ty);
            self.edit.touch(img, idx);
        }
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let w = shape_alpha(&self.brush, 0.0, x as f32 + 0.5 - cx, y as f32 + 0.5 - cy, radius, false) * strength;
                if w <= 0.0 {
                    continue;
                }
                let p = img.pixel(x, y);
                let out = std::array::from_fn(|i| p[i] as f32 + (carry[i] - p[i] as f32) * w);
                img.set_pixel(x, y, out.map(|v: f32| (v + 0.5).clamp(0.0, 255.0) as u8));
            }
        }
        // Pick up some of the new color for the next dab.
        let k = 0.35;
        self.smudge = Some(std::array::from_fn(|i| carry[i] * (1.0 - k) + avg[i] * k));
        self.dirty = self.dirty.union(&area);
    }

    fn blur_dab(&mut self, img: &mut TiledImage, cx: f32, cy: f32, radius: f32, strength: f32) {
        let area = IRect::around(cx, cy, radius).intersect(&img.bounds());
        if area.is_empty() {
            return;
        }
        let src_rect = IRect::new(area.x0 - 1, area.y0 - 1, area.x1 + 1, area.y1 + 1);
        let src = img.read_region(&src_rect);
        let sw = src_rect.width() as usize;
        for (tx, ty) in img.tiles_in_rect(&area).collect::<Vec<_>>() {
            let idx = img.tile_index(tx, ty);
            self.edit.touch(img, idx);
        }
        for y in area.y0..area.y1 {
            for x in area.x0..area.x1 {
                let w = shape_alpha(&self.brush, 0.0, x as f32 + 0.5 - cx, y as f32 + 0.5 - cy, radius, false) * strength;
                if w <= 0.0 {
                    continue;
                }
                let (lx, ly) = ((x - src_rect.x0) as usize, (y - src_rect.y0) as usize);
                let mut acc = [0.0f32; 4];
                for oy in 0..3 {
                    for ox in 0..3 {
                        let o = ((ly + oy - 1) * sw + (lx + ox - 1)) * 4;
                        for i in 0..4 {
                            acc[i] += src[o + i] as f32;
                        }
                    }
                }
                let o = (ly * sw + lx) * 4;
                let out: [u8; 4] = std::array::from_fn(|i| {
                    let p = src[o + i] as f32;
                    (p + (acc[i] / 9.0 - p) * w + 0.5).clamp(0.0, 255.0) as u8
                });
                img.set_pixel(x, y, out);
            }
        }
        self.dirty = self.dirty.union(&area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::builtin_brushes;

    fn settings() -> StrokeSettings {
        StrokeSettings::default()
    }

    fn brush(name: &str) -> Brush {
        builtin_brushes().into_iter().find(|b| b.name == name).expect("brush exists")
    }

    fn draw_line(b: Brush, s: StrokeSettings, img: &mut TiledImage) -> Option<PixelEdit> {
        let mut st = Stroke::new(b, s, 1, 1, 42);
        for i in 0..=50 {
            st.add_point(img, StrokePoint { x: 20.0 + i as f32 * 2.0, y: 50.0, pressure: 1.0 });
        }
        st.finish(img)
    }

    #[test]
    fn pen_draws_and_eraser_removes() {
        let mut img = TiledImage::new(200, 100);
        let e = draw_line(brush("Bolígrafo"), settings(), &mut img);
        assert!(e.is_some());
        assert_eq!(img.pixel(60, 50)[3], 255);
        assert_eq!(img.pixel(60, 80)[3], 0);
        let s = StrokeSettings { size: 30.0, ..settings() };
        draw_line(brush("Borrador duro"), s, &mut img);
        assert_eq!(img.pixel(60, 50)[3], 0);
        assert!(img.is_empty(), "erased tiles are freed");
    }

    #[test]
    fn marker_caps_opacity_within_one_stroke() {
        let mut img = TiledImage::new(200, 100);
        let b = brush("Rotulador");
        let s = StrokeSettings { opacity: 0.5, ..settings() };
        let mut st = Stroke::new(b, s, 1, 1, 1);
        // Go back and forth over the same pixels.
        for i in 0..200 {
            let x = 40.0 + ((i % 40) as f32 - 20.0).abs() * 3.0;
            st.add_point(&mut img, StrokePoint { x, y: 50.0, pressure: 1.0 });
        }
        st.finish(&mut img);
        let a = img.pixel(70, 50)[3];
        assert!((120..=135).contains(&a), "alpha {a} should stay near 50%");
    }

    #[test]
    fn cancel_restores_original() {
        let mut img = TiledImage::new(100, 100);
        img.set_pixel(10, 10, [9, 9, 9, 255]);
        let mut st = Stroke::new(brush("Pincel redondo"), settings(), 1, 1, 3);
        st.add_point(&mut img, StrokePoint { x: 10.0, y: 10.0, pressure: 1.0 });
        st.add_point(&mut img, StrokePoint { x: 60.0, y: 60.0, pressure: 1.0 });
        st.cancel(&mut img);
        assert_eq!(img.pixel(10, 10), [9, 9, 9, 255]);
        assert_eq!(img.pixel(40, 40), [0; 4]);
    }

    #[test]
    fn alpha_lock_keeps_transparency() {
        let mut img = TiledImage::new(100, 100);
        img.set_pixel(50, 50, [0, 0, 0, 255]);
        let s = StrokeSettings { color: Rgba::new(1.0, 0.0, 0.0, 1.0), size: 20.0, alpha_lock: true, ..settings() };
        let mut st = Stroke::new(brush("Bolígrafo"), s, 1, 1, 3);
        st.add_point(&mut img, StrokePoint { x: 50.5, y: 50.5, pressure: 1.0 });
        st.finish(&mut img);
        assert_eq!(img.pixel(50, 50), [255, 0, 0, 255]);
        assert_eq!(img.pixel(52, 50)[3], 0);
    }

    #[test]
    fn symmetry_mirrors_paint() {
        let mut img = TiledImage::new(200, 100);
        let sym = Symmetry { vertical: true, horizontal: false, cx: 100.0, cy: 50.0 };
        let s = StrokeSettings { symmetry: sym, ..settings() };
        let mut st = Stroke::new(brush("Bolígrafo"), s, 1, 1, 1);
        st.add_point(&mut img, StrokePoint { x: 30.0, y: 50.0, pressure: 1.0 });
        st.add_point(&mut img, StrokePoint { x: 40.0, y: 50.0, pressure: 1.0 });
        st.finish(&mut img);
        assert_eq!(img.pixel(35, 50)[3], 255);
        assert_eq!(img.pixel(165, 50)[3], 255, "mirrored side painted");
    }

    #[test]
    fn stabilizer_lags_then_catches_up() {
        let mut img = TiledImage::new(200, 100);
        let s = StrokeSettings { stabilizer: 1.0, ..settings() };
        let mut st = Stroke::new(brush("Bolígrafo"), s, 1, 1, 1);
        st.add_point(&mut img, StrokePoint { x: 20.0, y: 50.0, pressure: 1.0 });
        st.add_point(&mut img, StrokePoint { x: 50.0, y: 50.0, pressure: 1.0 });
        // Within the rope: nothing drawn yet past the start.
        assert_eq!(img.pixel(45, 50)[3], 0);
        st.finish(&mut img);
        assert_eq!(img.pixel(45, 50)[3], 255, "finish draws to the pen");
    }

    #[test]
    fn every_builtin_brush_runs() {
        for b in builtin_brushes() {
            let mut img = TiledImage::new(160, 100);
            img.set_pixel(60, 50, [200, 0, 0, 255]);
            draw_line(b, StrokeSettings { size: 12.0, ..settings() }, &mut img);
        }
    }
}
