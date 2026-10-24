//! Starter projects that teach by example: a guide drawn on a locked,
//! semi-transparent layer and an empty layer on top to trace it.

use std::f32::consts::{PI, TAU};

use crate::document::{CanvasSettings, Frame, Layer, LayerProps, Project};
use crate::raster::TiledImage;
use crate::color::Rgba;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    BouncingBall,
    Blink,
    Walk,
}

impl Template {
    pub const ALL: [Template; 3] = [Template::BouncingBall, Template::Blink, Template::Walk];

    /// Name (Spanish; the app translates it).
    pub fn name(self) -> &'static str {
        match self {
            Template::BouncingBall => "Pelota que rebota",
            Template::Blink => "Parpadeo",
            Template::Walk => "Caminata",
        }
    }

    /// What it teaches (Spanish; the app translates it).
    pub fn hint(self) -> &'static str {
        match self {
            Template::BouncingBall => "Arcos, espaciado y aplastar y estirar",
            Template::Blink => "Ritmo: cerrar rápido, abrir despacio",
            Template::Walk => "Ciclo de 8 dibujos que se repite",
        }
    }

    /// The finished starter project.
    pub fn build(self, name: &str) -> Project {
        let canvas = CanvasSettings { width: 1280, height: 720, fps: 12, background: Rgba::WHITE, ..Default::default() };
        match self {
            Template::BouncingBall => bouncing_ball(name, canvas),
            Template::Blink => blink(name, canvas),
            Template::Walk => walk(name, canvas),
        }
    }
}

const GUIDE: [u8; 4] = [70, 120, 235, 255];
const LINE: [u8; 4] = [40, 40, 46, 255];

/// Project with: a fixed background layer (optional), a guide layer with
/// one drawing per pose, and an empty "Dibujo" layer on top.
fn with_guide(name: &str, canvas: CanvasSettings, background: Option<TiledImage>, poses: Vec<TiledImage>) -> Project {
    let (cw, ch) = (canvas.width, canvas.height);
    let mut p = Project::new(name, canvas);
    let draw_id = p.layers[0].id;
    p.layers[0].props.name = "Dibujo".into();
    let guide = p.alloc_id();
    let mut props = LayerProps::named("Guía");
    props.locked = true;
    props.opacity = 0.45;
    props.guide = true;
    props.onion_skin = false;
    p.layers.insert(0, Layer { id: guide, props });
    if let Some(bg) = background {
        let id = p.alloc_id();
        let mut props = LayerProps::named("Fondo");
        props.shared = true;
        props.locked = true;
        p.layers.insert(0, Layer { id, props });
        p.shared_cels.insert(id, bg);
    }
    p.frames.clear();
    for img in poses {
        let fid = p.alloc_id();
        let mut f = Frame::new(fid);
        f.cels.insert(guide, img);
        f.cels.insert(draw_id, TiledImage::new(cw, ch));
        p.frames.push(f);
    }
    p
}

fn bouncing_ball(name: &str, c: CanvasSettings) -> Project {
    let (w, h) = (c.width, c.height);
    let ground = h as f32 - 110.0;
    let mut bg = TiledImage::new(w, h);
    line(&mut bg, (40.0, ground), (w as f32 - 40.0, ground), 4.0, LINE);
    let r = 42.0;
    // Falls from the top left and bounces three times, each lower; the
    // ball travels right at an even speed.
    let floor_y = ground - r;
    let mut poses = Vec::new();
    let frames = 24;
    for i in 0..frames {
        let t = i as f32 / frames as f32;
        let x = 150.0 + t * (w as f32 - 300.0);
        // |cos| gives a fall then bounces; contacts at t = 0.2, 0.6, 1.0.
        let phase = PI * 2.5 * t;
        let bounce = ((phase + PI / 2.0) / PI).floor() as i32; // 0 before the first contact
        let height = 420.0 * 0.5f32.powi(bounce.max(0));
        let lift = phase.cos().abs();
        let y = floor_y - height * lift;
        let mut img = TiledImage::new(w, h);
        let contact = lift < 0.12;
        let fast = lift < 0.45 && !contact;
        let (rx, ry) = if contact { (r * 1.3, r * 0.72) } else if fast { (r * 0.86, r * 1.18) } else { (r, r) };
        let cy = if contact { ground - ry } else { y };
        ellipse(&mut img, (x, cy), rx, ry, 4.0, GUIDE);
        poses.push(img);
    }
    with_guide(name, c, Some(bg), poses)
}

fn blink(name: &str, c: CanvasSettings) -> Project {
    let (w, h) = (c.width, c.height);
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    let mut bg = TiledImage::new(w, h);
    ellipse(&mut bg, (cx, cy), 230.0, 260.0, 5.0, LINE);
    line(&mut bg, (cx - 70.0, cy + 130.0), (cx + 70.0, cy + 130.0), 5.0, LINE);
    // Openness per drawing: hold open, close fast, reopen a bit slower.
    let open = [1.0, 1.0, 1.0, 1.0, 1.0, 1.0, 0.5, 0.0, 0.0, 0.35, 0.75, 1.0, 1.0, 1.0];
    let poses = open
        .iter()
        .map(|&o: &f32| {
            let mut img = TiledImage::new(w, h);
            for side in [-1.0, 1.0] {
                let ex = cx + side * 95.0;
                let ey = cy - 40.0;
                if o <= 0.05 {
                    arc(&mut img, (ex, ey - 10.0), 40.0, 0.15 * PI, 0.85 * PI, 5.0, GUIDE);
                } else {
                    ellipse(&mut img, (ex, ey), 34.0, 46.0 * o, 5.0, GUIDE);
                    disc(&mut img, (ex, ey + 6.0 * o), 14.0 * o.max(0.4), GUIDE);
                }
            }
            img
        })
        .collect();
    with_guide(name, c, Some(bg), poses)
}

fn walk(name: &str, c: CanvasSettings) -> Project {
    let (w, h) = (c.width, c.height);
    let ground = h as f32 - 90.0;
    let mut bg = TiledImage::new(w, h);
    line(&mut bg, (40.0, ground), (w as f32 - 40.0, ground), 4.0, LINE);
    let (thigh, shin, torso, arm) = (95.0f32, 95.0f32, 150.0f32, 120.0f32);
    let cx = w as f32 / 2.0;
    let poses = (0..8)
        .map(|i| {
            let ph = i as f32 / 8.0 * TAU;
            let mut img = TiledImage::new(w, h);
            // Down on the contact, up on the passing position.
            let bob = (2.0 * ph).cos() * 9.0;
            let hip = (cx, ground - thigh - shin + 12.0 + bob);
            let neck = (hip.0 + 6.0, hip.1 - torso);
            line(&mut img, hip, neck, 6.0, GUIDE);
            disc_ring(&mut img, (neck.0 + 4.0, neck.1 - 42.0), 36.0, 6.0, GUIDE);
            for k in [0.0f32, PI] {
                // Frame 0 is the contact pose: legs apart.
                let a = ph + PI / 2.0 + k;
                // Leg: swing from the hip, bend the knee when lifting.
                let swing = 0.45 * a.sin();
                let knee_bend = 0.7 * (a + PI / 2.0).sin().max(0.0);
                let knee = (hip.0 + thigh * swing.sin(), hip.1 + thigh * swing.cos());
                let foot = (knee.0 + shin * (swing - knee_bend).sin(), knee.1 + shin * (swing - knee_bend).cos());
                line(&mut img, hip, knee, 6.0, GUIDE);
                line(&mut img, knee, foot, 6.0, GUIDE);
                // Each arm swings opposite to its leg.
                let sw = -0.5 * a.sin();
                let shoulder = (neck.0, neck.1 + 18.0);
                let elbow = (shoulder.0 + arm * 0.5 * sw.sin(), shoulder.1 + arm * 0.5 * sw.cos());
                let hand = (elbow.0 + arm * 0.5 * (sw * 1.4 + 0.25).sin(), elbow.1 + arm * 0.5 * (sw * 1.4 + 0.25).cos());
                line(&mut img, shoulder, elbow, 6.0, GUIDE);
                line(&mut img, elbow, hand, 6.0, GUIDE);
            }
            img
        })
        .collect();
    with_guide(name, c, Some(bg), poses)
}

// ---- tiny anti-aliased drawing -------------------------------------------

fn blend(img: &mut TiledImage, x: i32, y: i32, color: [u8; 4], cover: f32) {
    if cover <= 0.0 || x < 0 || y < 0 || x >= img.width() as i32 || y >= img.height() as i32 {
        return;
    }
    let a = cover.min(1.0) * color[3] as f32 / 255.0;
    let d = img.pixel(x, y);
    let k = 1.0 - a;
    let out: [u8; 4] = std::array::from_fn(|i| {
        let src = if i == 3 { 255.0 * a } else { color[i] as f32 * a };
        (src + d[i] as f32 * k + 0.5).min(255.0) as u8
    });
    img.set_pixel(x, y, out);
}

/// Draw where `dist(x, y)` (distance to the shape's line) is under half the width.
fn stroke_shape(img: &mut TiledImage, bounds: (f32, f32, f32, f32), width: f32, color: [u8; 4], dist: impl Fn(f32, f32) -> f32) {
    let pad = width;
    let (x0, y0, x1, y1) = ((bounds.0 - pad).floor() as i32, (bounds.1 - pad).floor() as i32, (bounds.2 + pad).ceil() as i32, (bounds.3 + pad).ceil() as i32);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let d = dist(x as f32 + 0.5, y as f32 + 0.5);
            blend(img, x, y, color, width / 2.0 + 0.5 - d);
        }
    }
}

fn line(img: &mut TiledImage, a: (f32, f32), b: (f32, f32), width: f32, color: [u8; 4]) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = (dx * dx + dy * dy).max(1e-6);
    stroke_shape(img, (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1)), width, color, |x, y| {
        let t = (((x - a.0) * dx + (y - a.1) * dy) / len2).clamp(0.0, 1.0);
        ((x - a.0 - t * dx).powi(2) + (y - a.1 - t * dy).powi(2)).sqrt()
    });
}

fn ellipse(img: &mut TiledImage, c: (f32, f32), rx: f32, ry: f32, width: f32, color: [u8; 4]) {
    let (rx, ry) = (rx.max(1.0), ry.max(1.0));
    stroke_shape(img, (c.0 - rx, c.1 - ry, c.0 + rx, c.1 + ry), width, color, |x, y| {
        let (u, v) = ((x - c.0) / rx, (y - c.1) / ry);
        let k = (u * u + v * v).sqrt();
        // Approximate distance to the outline.
        ((k - 1.0) * rx.min(ry)).abs()
    });
}

fn disc_ring(img: &mut TiledImage, c: (f32, f32), r: f32, width: f32, color: [u8; 4]) {
    ellipse(img, c, r, r, width, color);
}

fn disc(img: &mut TiledImage, c: (f32, f32), r: f32, color: [u8; 4]) {
    let r = r.max(1.0);
    for y in (c.1 - r - 1.0).floor() as i32..=(c.1 + r + 1.0).ceil() as i32 {
        for x in (c.0 - r - 1.0).floor() as i32..=(c.0 + r + 1.0).ceil() as i32 {
            let d = ((x as f32 + 0.5 - c.0).powi(2) + (y as f32 + 0.5 - c.1).powi(2)).sqrt();
            blend(img, x, y, color, r + 0.5 - d);
        }
    }
}

fn arc(img: &mut TiledImage, c: (f32, f32), r: f32, a0: f32, a1: f32, width: f32, color: [u8; 4]) {
    let n = 24;
    let pts: Vec<(f32, f32)> = (0..=n).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / n as f32;
        (c.0 + r * a.cos(), c.1 + r * a.sin())
    }).collect();
    for p in pts.windows(2) {
        line(img, p[0], p[1], width, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_have_guides_and_an_empty_layer_on_top() {
        for t in Template::ALL {
            let p = t.build(t.name());
            assert!(p.frames.len() >= 8, "{t:?}");
            let top = p.layers.last().unwrap();
            assert_eq!(top.props.name, "Dibujo");
            let guide = p.layers.iter().find(|l| l.props.name == "Guía").unwrap();
            assert!(guide.props.locked && guide.props.guide);
            // The guide never reaches the exported picture.
            let out = crate::composite::render_frame(&p, 0, false);
            let guide_only = p.layers.iter().all(|l| l.props.name != "Fondo");
            if guide_only {
                assert!(out.chunks_exact(4).all(|px| px[3] == 0), "{t:?}: guide exported");
            }
            // Every frame has its own guide drawing with something on it.
            for f in &p.frames {
                let g = f.cels.get(&guide.id).expect("guide drawing");
                assert!(g.allocated_tiles() > 0, "{t:?}: empty guide");
                assert!(f.cels.contains_key(&top.id));
            }
        }
    }
}

