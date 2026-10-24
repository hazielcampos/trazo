//! Text turned into pixels (for the text tool).

use ab_glyph::{Font, FontRef, PxScale, ScaleFont};

use crate::color::Rgba;

/// Text drawn into a premultiplied RGBA8 buffer.
pub struct TextImage {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Left,
    Center,
}

/// Draw `text` (several lines allowed) at `size` pixels with the TrueType
/// font in `font`. `bold` thickens the strokes. None if the font can't be
/// read or there is nothing to draw.
pub fn render(font: &[u8], text: &str, size: f32, color: Rgba, bold: bool, align: Align) -> Option<TextImage> {
    let font = FontRef::try_from_slice(font).ok()?;
    let size = size.clamp(4.0, 1000.0);
    let scaled = font.as_scaled(PxScale::from(size));
    let line_h = scaled.height() + scaled.line_gap();
    let lines: Vec<&str> = text.lines().collect();
    if lines.iter().all(|l| l.trim().is_empty()) {
        return None;
    }
    // Lay out each line: glyphs with their pen positions.
    let mut laid = Vec::new();
    let mut widths = Vec::new();
    for line in &lines {
        let mut x = 0.0f32;
        let mut prev = None;
        let mut glyphs = Vec::new();
        for ch in line.chars() {
            let id = font.glyph_id(ch);
            if let Some(p) = prev {
                x += scaled.kern(p, id);
            }
            glyphs.push((id, x));
            x += scaled.h_advance(id);
            prev = Some(id);
        }
        widths.push(x);
        laid.push(glyphs);
    }
    let thick = if bold { (size * 0.035).max(1.0) } else { 0.0 };
    let pad = thick.ceil() as i32 + 2;
    let text_w = widths.iter().cloned().fold(0.0, f32::max);
    let width = text_w.ceil() as i32 + pad * 2;
    let height = (line_h * lines.len() as f32).ceil() as i32 + pad * 2;
    if width <= 0 || height <= 0 || width as i64 * height as i64 > 64_000_000 {
        return None;
    }
    let mut cover = vec![0f32; (width * height) as usize];
    for (row, glyphs) in laid.iter().enumerate() {
        let off_x = match align {
            Align::Left => 0.0,
            Align::Center => (text_w - widths[row]) * 0.5,
        };
        let base = pad as f32 + scaled.ascent() + line_h * row as f32;
        for &(id, x) in glyphs {
            let g = id.with_scale_and_position(PxScale::from(size), ab_glyph::point(pad as f32 + off_x + x, base));
            let Some(outline) = font.outline_glyph(g) else { continue };
            let b = outline.px_bounds();
            outline.draw(|gx, gy, c| {
                let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if px >= 0 && py >= 0 && px < width && py < height {
                    let i = (py * width + px) as usize;
                    cover[i] = (cover[i] + c).min(1.0);
                }
            });
        }
    }
    if thick > 0.0 {
        // Thicken: each pixel takes the strongest coverage within `thick`.
        let r = thick.ceil() as i32;
        let src = cover.clone();
        for y in 0..height {
            for x in 0..width {
                let mut best = 0f32;
                for dy in -r..=r {
                    for dx in -r..=r {
                        let d = ((dx * dx + dy * dy) as f32).sqrt();
                        if d > thick + 0.5 {
                            continue;
                        }
                        let (sx, sy) = (x + dx, y + dy);
                        if sx >= 0 && sy >= 0 && sx < width && sy < height {
                            let w = (thick + 0.5 - d).clamp(0.0, 1.0);
                            best = best.max(src[(sy * width + sx) as usize] * w);
                        }
                    }
                }
                cover[(y * width + x) as usize] = best;
            }
        }
    }
    let c = color.to_rgba8();
    let mut pixels = vec![0u8; (width * height * 4) as usize];
    let mut any = false;
    for (i, &k) in cover.iter().enumerate() {
        let a = k * c[3] as f32 / 255.0;
        if a <= 0.0 {
            continue;
        }
        any = true;
        let o = i * 4;
        for ch in 0..3 {
            pixels[o + ch] = (c[ch] as f32 * a + 0.5) as u8;
        }
        pixels[o + 3] = (a * 255.0 + 0.5) as u8;
    }
    any.then_some(TextImage { width, height, pixels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font() -> Vec<u8> {
        // Any TrueType font on the machine running the tests.
        for p in ["C:/Windows/Fonts/arial.ttf", "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", "/System/Library/Fonts/Supplemental/Arial.ttf"] {
            if let Ok(b) = std::fs::read(p) {
                return b;
            }
        }
        Vec::new()
    }

    #[test]
    fn draws_lines_and_bold_is_heavier() {
        let f = font();
        if f.is_empty() {
            return; // no system font to test with
        }
        let red = Rgba::new(1.0, 0.0, 0.0, 1.0);
        let one = render(&f, "Hola", 40.0, red, false, Align::Left).unwrap();
        let two = render(&f, "Hola\nmundo", 40.0, red, false, Align::Left).unwrap();
        assert!(two.height > one.height, "two lines are taller");
        let ink = |t: &TextImage| t.pixels.chunks_exact(4).map(|p| p[3] as u64).sum::<u64>();
        let bold = render(&f, "Hola", 40.0, red, true, Align::Left).unwrap();
        assert!(ink(&bold) > ink(&one));
        // Premultiplied red only.
        assert!(one.pixels.chunks_exact(4).all(|p| p[1] == 0 && p[2] == 0 && p[0] <= p[3]));
        assert!(render(&f, "  \n ", 40.0, red, false, Align::Left).is_none());
    }
}
