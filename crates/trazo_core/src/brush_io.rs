//! Brush files (`.animbrush`): a zip with `brush.json` and an optional
//! `tip.png`. PNG images can also be imported directly as new brush tips.

use std::io::{Cursor, Write};
use std::path::Path;
use std::sync::Arc;

use crate::brush::{Accumulate, Brush, BrushCategory, BrushTip, DabShape};
use crate::io::{decode_png, png_bytes, read_entry, ProjectError, Result};

pub const BRUSH_EXTENSION: &str = "animbrush";
/// Largest tip side we keep; bigger images are downscaled.
const MAX_TIP: u32 = 256;

pub fn save_brush(brush: &Brush, path: &Path) -> Result<()> {
    let mut buf = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buf));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("brush.json", opts)?;
        zip.write_all(&serde_json::to_vec_pretty(brush)?)?;
        if let Some(tip) = &brush.tip {
            zip.start_file("tip.png", opts)?;
            zip.write_all(&png_bytes(tip.width, tip.height, &tip.to_rgba())?)?;
        }
        zip.finish()?;
    }
    std::fs::write(path, buf)?;
    Ok(())
}

pub fn load_brush(path: &Path) -> Result<Brush> {
    load_brush_bytes(&std::fs::read(path)?)
}

pub fn load_brush_bytes(bytes: &[u8]) -> Result<Brush> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    let json = read_entry(&mut zip, "brush.json", 1024 * 1024)?;
    let mut brush: Brush = serde_json::from_slice(&json)?;
    if zip.index_for_name("tip.png").is_some() {
        let png = read_entry(&mut zip, "tip.png", 32 * 1024 * 1024)?;
        let (w, h, rgba) = decode_png(&png)?;
        let (w, h, rgba) = downscale(w, h, rgba, MAX_TIP);
        brush.tip = Some(Arc::new(BrushTip::from_rgba(w, h, &rgba)));
    }
    if brush.shape == DabShape::Image && brush.tip.is_none() {
        brush.shape = DabShape::Round;
    }
    brush.category = BrushCategory::Custom;
    sanitize(&mut brush);
    Ok(brush)
}

/// Clamp values from untrusted files to ranges the engine handles well.
fn sanitize(b: &mut Brush) {
    b.size = b.size.clamp(0.5, 1000.0);
    b.min_size = b.min_size.clamp(0.0, 1.0);
    b.opacity = b.opacity.clamp(0.0, 1.0);
    b.flow = b.flow.clamp(0.0, 1.0);
    b.hardness = b.hardness.clamp(0.0, 1.0);
    b.spacing = b.spacing.clamp(0.01, 5.0);
    b.roundness = b.roundness.clamp(0.05, 1.0);
    b.grain = b.grain.clamp(0.0, 1.0);
    b.grain_scale = b.grain_scale.clamp(0.5, 64.0);
    b.density = b.density.min(500);
    b.jitter = b.jitter.clamp(0.0, 2.0);
    b.size_jitter = b.size_jitter.clamp(0.0, 1.0);
    b.taper_start = b.taper_start.clamp(0.0, 2000.0);
    b.taper_end = b.taper_end.clamp(0.0, 2000.0);
    b.rotation_jitter = b.rotation_jitter.clamp(0.0, 180.0);
    b.angle_deg = b.angle_deg.clamp(-360.0, 360.0);
    if !b.size.is_finite() || !b.spacing.is_finite() {
        *b = Brush { name: b.name.clone(), ..Brush::base("Pincel", BrushCategory::Custom) };
    }
    if b.name.trim().is_empty() {
        b.name = "Pincel".into();
    }
    if b.name.chars().count() > 60 {
        b.name = b.name.chars().take(60).collect();
    }
}

/// New stamp brush from a PNG image (dark or opaque areas paint).
pub fn brush_from_png(name: &str, png: &[u8]) -> Result<Brush> {
    let (w, h, rgba) = decode_png(png)?;
    if w == 0 || h == 0 {
        return Err(ProjectError::Invalid("imagen vacía".into()));
    }
    let (w, h, rgba) = downscale(w, h, rgba, MAX_TIP);
    let tip = BrushTip::from_rgba(w, h, &rgba);
    Ok(Brush {
        shape: DabShape::Image,
        tip: Some(Arc::new(tip)),
        size: 40.0,
        spacing: 0.25,
        hardness: 1.0,
        accumulate: Accumulate::Buildup,
        flow: 0.9,
        pressure_size: true,
        min_size: 0.3,
        ..Brush::base(name, BrushCategory::Custom)
    })
}

/// Box-filter downscale so the longest side is at most `max`.
fn downscale(w: u32, h: u32, rgba: Vec<u8>, max: u32) -> (u32, u32, Vec<u8>) {
    let longest = w.max(h);
    if longest <= max {
        return (w, h, rgba);
    }
    let f = longest as f32 / max as f32;
    let (nw, nh) = (((w as f32 / f).round() as u32).max(1), ((h as f32 / f).round() as u32).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let (x0, y0) = ((x as f32 * f) as u32, (y as f32 * f) as u32);
            let x1 = (((x + 1) as f32 * f) as u32).min(w).max(x0 + 1);
            let y1 = (((y + 1) as f32 * f) as u32).min(h).max(y0 + 1);
            let mut acc = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let o = ((sy * w + sx) * 4) as usize;
                    for (i, a) in acc.iter_mut().enumerate() {
                        *a += rgba[o + i] as u32;
                    }
                }
            }
            let n = (x1 - x0) * (y1 - y0);
            let o = ((y * nw + x) * 4) as usize;
            for (i, a) in acc.iter().enumerate() {
                out[o + i] = (a / n) as u8;
            }
        }
    }
    (nw, nh, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stroke::{Stroke, StrokePoint, StrokeSettings};
    use crate::{Rgba, TiledImage};

    fn plus_png() -> Vec<u8> {
        // 8×8 white image with a black plus sign.
        let mut rgba = vec![255u8; 8 * 8 * 4];
        for i in 0..8 {
            for (x, y) in [(i, 3), (i, 4), (3, i), (4, i)] {
                let o = (y * 8 + x) * 4;
                rgba[o..o + 3].copy_from_slice(&[0, 0, 0]);
            }
        }
        png_bytes(8, 8, &rgba).unwrap()
    }

    #[test]
    fn png_tip_paints_and_roundtrips() {
        let brush = brush_from_png("Cruz", &plus_png()).unwrap();
        let tip = brush.tip.clone().unwrap();
        assert_eq!(tip.alpha[3 * 8 + 3], 255);
        assert_eq!(tip.alpha[0], 0);

        let mut img = TiledImage::new(100, 100);
        let s = StrokeSettings { color: Rgba::BLACK, size: 40.0, ..Default::default() };
        let mut st = Stroke::new(brush.clone(), s, 1, 1, 1);
        st.add_point(&mut img, StrokePoint { x: 50.0, y: 50.0, pressure: 1.0 });
        st.finish(&mut img);
        assert!(img.pixel(50, 50)[3] > 200, "center of the plus is painted");
        assert_eq!(img.pixel(38, 38)[3], 0, "corner of the tip stays empty");

        let path = std::env::temp_dir().join(format!("brush_{}.animbrush", std::process::id()));
        save_brush(&brush, &path).unwrap();
        let back = load_brush(&path).unwrap();
        assert_eq!(back.name, "Cruz");
        assert_eq!(back.tip.as_deref(), Some(&*tip));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn large_png_is_downscaled() {
        let rgba = vec![0u8; 600 * 300 * 4];
        let png = png_bytes(600, 300, &rgba).unwrap();
        let b = brush_from_png("Grande", &png).unwrap();
        let t = b.tip.unwrap();
        assert_eq!((t.width, t.height), (256, 128));
    }
}
