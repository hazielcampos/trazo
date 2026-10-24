//! Paint bucket (flood fill).

use crate::color::Rgba;
use crate::geom::IRect;
use crate::history::PixelEditBuilder;
use crate::raster::TiledImage;

#[derive(Debug, Clone, Copy)]
pub struct FillOptions {
    /// Max per-channel difference (0..=255) still counted as "same color".
    pub tolerance: u8,
    /// Expand the filled area by this many pixels to hide anti-aliased halos
    /// under line art.
    pub grow: u32,
    /// Treat gaps in line art up to about this many pixels as closed.
    pub gap_close: u32,
    /// Only recolor pixels that already exist on the target (layer alpha lock).
    pub alpha_lock: bool,
}

impl Default for FillOptions {
    fn default() -> Self {
        Self { tolerance: 32, grow: 1, gap_close: 0, alpha_lock: false }
    }
}

fn similar(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
    (0..4).all(|i| (a[i] as i16 - b[i] as i16).unsigned_abs() <= tol as u16)
}

/// Pixels connected to `(x, y)` in `reference` that look like the pixel
/// there (honouring tolerance and gap closing). Returns a full-image mask and
/// its bounding box. Used by the paint bucket and the magic wand.
pub fn flood_region(reference: &TiledImage, x: i32, y: i32, opts: FillOptions) -> Option<(Vec<bool>, IRect)> {
    let (w, h) = (reference.width() as i32, reference.height() as i32);
    if x < 0 || y < 0 || x >= w || y >= h {
        return None;
    }
    let seed = reference.pixel(x, y);
    // Pixels the fill may not enter (line art).
    let mut blocked: Vec<bool> = Vec::with_capacity((w * h) as usize);
    for py in 0..h {
        for px in 0..w {
            blocked.push(!similar(reference.pixel(px, py), seed, opts.tolerance));
        }
    }
    let gap = opts.gap_close as i32;
    if gap > 0 {
        blocked = dilate(&blocked, w, h, gap);
        if blocked[(y * w + x) as usize] {
            // Tapped right next to a line: fall back to the plain fill.
            blocked = (0..w * h).map(|i| !similar(reference.pixel(i % w, i / w), seed, opts.tolerance)).collect();
        }
    }
    let open = |px: i32, py: i32| !blocked[(py * w + px) as usize];
    let mut mask = vec![false; (w * h) as usize];
    let mut bbox = IRect::EMPTY;
    let mut stack = vec![(x, y)];
    // Scanline flood fill.
    while let Some((sx, sy)) = stack.pop() {
        let row = (sy * w) as usize;
        if mask[row + sx as usize] {
            continue;
        }
        let mut lx = sx;
        while lx > 0 && !mask[row + (lx - 1) as usize] && open(lx - 1, sy) {
            lx -= 1;
        }
        let mut rx = sx;
        while rx + 1 < w && !mask[row + (rx + 1) as usize] && open(rx + 1, sy) {
            rx += 1;
        }
        for px in lx..=rx {
            mask[row + px as usize] = true;
        }
        bbox = bbox.union(&IRect::new(lx, sy, rx + 1, sy + 1));
        for ny in [sy - 1, sy + 1] {
            if ny < 0 || ny >= h {
                continue;
            }
            let nrow = (ny * w) as usize;
            let mut px = lx;
            while px <= rx {
                if !mask[nrow + px as usize] && open(px, ny) {
                    stack.push((px, ny));
                    // Skip the rest of this run; it will be found by the scan.
                    while px <= rx && open(px, ny) {
                        px += 1;
                    }
                } else {
                    px += 1;
                }
            }
        }
    }
    if bbox.is_empty() {
        return None;
    }

    // Grow back under the line (and over the closed gaps).
    let g = opts.grow as i32 + gap;
    if g > 0 {
        let src = mask.clone();
        let grown = IRect::new(bbox.x0 - g, bbox.y0 - g, bbox.x1 + g, bbox.y1 + g).intersect(&reference.bounds());
        for py in grown.y0..grown.y1 {
            for px in grown.x0..grown.x1 {
                if src[(py * w + px) as usize] {
                    continue;
                }
                'search: for oy in -g..=g {
                    for ox in -g..=g {
                        let (qx, qy) = (px + ox, py + oy);
                        if qx >= 0 && qy >= 0 && qx < w && qy < h && src[(qy * w + qx) as usize] {
                            mask[(py * w + px) as usize] = true;
                            break 'search;
                        }
                    }
                }
            }
        }
        bbox = grown;
    }

    Some((mask, bbox))
}

/// Fill the region connected to `(x, y)` in `reference` with `color`, painting
/// onto `target`. `reference` may be the target itself or a flattened copy of
/// all visible layers. Returns the changed area.
pub fn flood_fill(
    target: &mut TiledImage,
    reference: &TiledImage,
    x: i32,
    y: i32,
    color: Rgba,
    opts: FillOptions,
    edit: &mut PixelEditBuilder,
) -> IRect {
    let Some((mask, bbox)) = flood_region(reference, x, y, opts) else { return IRect::EMPTY };
    let w = reference.width() as i32;
    paint_mask(target, &|px, py| mask[(py * w + px) as usize], bbox, color, opts.alpha_lock, edit);
    bbox
}

/// Paint `color` over every pixel of `area` where `inside` is true.
pub fn paint_mask(
    target: &mut TiledImage,
    inside: &dyn Fn(i32, i32) -> bool,
    area: IRect,
    color: Rgba,
    alpha_lock: bool,
    edit: &mut PixelEditBuilder,
) {
    let bbox = area.intersect(&target.bounds());
    for (tx, ty) in target.tiles_in_rect(&bbox).collect::<Vec<_>>() {
        let idx = target.tile_index(tx, ty);
        edit.touch(target, idx);
    }
    let src = color.premul8(1.0);
    let sa = src[3] / 255.0;
    for py in bbox.y0..bbox.y1 {
        for px in bbox.x0..bbox.x1 {
            if !inside(px, py) {
                continue;
            }
            let d = target.pixel(px, py);
            if alpha_lock {
                if d[3] == 0 {
                    continue;
                }
                let da = d[3] as f32 / 255.0;
                let out = [color.r * da * 255.0, color.g * da * 255.0, color.b * da * 255.0];
                target.set_pixel(px, py, [out[0] as u8, out[1] as u8, out[2] as u8, d[3]]);
                continue;
            }
            let out: [u8; 4] = std::array::from_fn(|i| (src[i] + d[i] as f32 * (1.0 - sa) + 0.5).clamp(0.0, 255.0) as u8);
            target.set_pixel(px, py, out);
        }
    }
}

/// Separable square dilation of a boolean mask by `r` pixels.
fn dilate(src: &[bool], w: i32, h: i32, r: i32) -> Vec<bool> {
    let mut tmp = vec![false; src.len()];
    for y in 0..h {
        let row = (y * w) as usize;
        // Running count of `true` pixels inside the window.
        let mut count = 0;
        for x in -r..w + r {
            let add = x + r;
            if add < w && add >= 0 && src[row + add as usize] {
                count += 1;
            }
            let rem = x - r - 1;
            if rem >= 0 && rem < w && src[row + rem as usize] {
                count -= 1;
            }
            if (0..w).contains(&x) {
                tmp[row + x as usize] = count > 0;
            }
        }
    }
    let mut out = vec![false; src.len()];
    for x in 0..w {
        let mut count = 0;
        for y in -r..h + r {
            let add = y + r;
            if add < h && add >= 0 && tmp[(add * w + x) as usize] {
                count += 1;
            }
            let rem = y - r - 1;
            if rem >= 0 && rem < h && tmp[(rem * w + x) as usize] {
                count -= 1;
            }
            if (0..h).contains(&y) {
                out[(y * w + x) as usize] = count > 0;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fills_inside_a_box_only() {
        let mut img = TiledImage::new(100, 100);
        for i in 10..=50 {
            for &(x, y) in &[(i, 10), (i, 50), (10, i), (50, i)] {
                img.set_pixel(x, y, [0, 0, 0, 255]);
            }
        }
        let reference = img.clone();
        let mut edit = PixelEditBuilder::new();
        let r = flood_fill(&mut img, &reference, 30, 30, Rgba::new(1.0, 0.0, 0.0, 1.0), FillOptions { tolerance: 10, grow: 0, gap_close: 0, alpha_lock: false }, &mut edit);
        assert_eq!(r, IRect::new(11, 11, 50, 50));
        assert_eq!(img.pixel(30, 30), [255, 0, 0, 255]);
        assert_eq!(img.pixel(70, 70), [0; 4]);
        assert_eq!(img.pixel(10, 30), [0, 0, 0, 255]);
    }

    #[test]
    fn gap_close_stops_leak() {
        let mut img = TiledImage::new(100, 100);
        for i in 10..=50 {
            // Box with a 2px gap in the top edge.
            if !(29..=30).contains(&i) {
                img.set_pixel(i, 10, [0, 0, 0, 255]);
            }
            img.set_pixel(i, 50, [0, 0, 0, 255]);
            img.set_pixel(10, i, [0, 0, 0, 255]);
            img.set_pixel(50, i, [0, 0, 0, 255]);
        }
        let reference = img.clone();
        let mut edit = PixelEditBuilder::new();
        let opts = FillOptions { tolerance: 10, grow: 0, gap_close: 2, alpha_lock: false };
        flood_fill(&mut img, &reference, 30, 30, Rgba::new(1.0, 0.0, 0.0, 1.0), opts, &mut edit);
        assert_eq!(img.pixel(30, 30), [255, 0, 0, 255]);
        assert_eq!(img.pixel(80, 80), [0; 4], "fill leaked through the gap");
    }
}
