//! Brush previews: a sample stroke drawn with the real stroke engine, so the
//! preview always matches what the brush actually paints.

use crate::brush::{Brush, BrushMode};
use crate::color::Rgba;
use crate::stroke::{Stroke, StrokePoint, StrokeSettings};
use crate::TiledImage;

/// Render an S-shaped stroke (thin → thick → thin pressure) into a
/// `w`×`h` premultiplied RGBA8 buffer with a transparent background.
/// `size` is the brush diameter to use (clamped to fit the preview).
pub fn brush_preview(brush: &Brush, w: u32, h: u32, color: Rgba, size: f32) -> Vec<u8> {
    let mut img = TiledImage::new(w, h);
    let (wf, hf) = (w as f32, h as f32);
    let size = size.clamp(2.0, hf * 0.42);
    let margin = size * 0.5 + 6.0;

    // Erasers, smudge and blur need something underneath to show their effect.
    if brush.mode != BrushMode::Paint {
        let band = Brush { mode: BrushMode::Paint, tip: None, ..crate::brush::builtin_brushes()[0].clone() };
        let stripes = [Rgba::new(0.95, 0.45, 0.2, 1.0), Rgba::new(0.25, 0.5, 0.95, 1.0)];
        for (i, c) in stripes.iter().enumerate() {
            let s = StrokeSettings { color: *c, size: hf * 0.32, ..Default::default() };
            let mut b = band.clone();
            b.grain = 0.0;
            b.hardness = 0.95;
            b.pressure_size = false;
            b.taper_end = 0.0;
            let mut st = Stroke::new(b, s, 1, 1, 7);
            let y = hf * (0.33 + 0.34 * i as f32);
            for k in 0..=20 {
                st.add_point(&mut img, StrokePoint { x: 4.0 + (wf - 8.0) * k as f32 / 20.0, y, pressure: 1.0 });
            }
            st.finish(&mut img);
        }
    }

    let settings = StrokeSettings { color, size, opacity: brush.opacity.max(0.35), ..Default::default() };
    let mut st = Stroke::new(brush.clone(), settings, 1, 1, 12345);
    let n = 64;
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let x = margin + (wf - 2.0 * margin) * t;
        let y = hf * 0.5 - (t * std::f32::consts::TAU).sin() * (hf * 0.5 - margin) * 0.8;
        // Pressure rises and falls along the stroke.
        let pressure = 0.15 + 0.85 * (t * std::f32::consts::PI).sin();
        st.add_point(&mut img, StrokePoint { x, y, pressure });
    }
    st.finish(&mut img);
    img.read_region(&img.bounds())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::builtin_brushes;

    #[test]
    fn every_brush_preview_has_paint() {
        for b in builtin_brushes() {
            let px = brush_preview(&b, 160, 48, Rgba::BLACK, b.size);
            assert_eq!(px.len(), 160 * 48 * 4);
            assert!(px.chunks_exact(4).any(|p| p[3] > 0), "{} preview is empty", b.name);
        }
    }
}
