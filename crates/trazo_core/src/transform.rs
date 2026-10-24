//! Selections and free transform (move / scale / rotate / flip).
//!
//! A [`Transform`] "lifts" the selected pixels off a cel into a floating
//! buffer. Every time the transform changes, the affected tiles are restored
//! from their originals and the floating pixels are drawn again, so the result
//! is always exact and the whole operation becomes a single undo step.

use crate::document::{FrameId, LayerId};
use crate::geom::IRect;
use crate::history::{PixelEdit, PixelEditBuilder};
use crate::raster::TiledImage;

/// Selection mask covering `rect` (255 = fully selected).
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub rect: IRect,
    pub alpha: Vec<u8>,
}

impl Mask {
    /// Rectangular selection, clipped to `bounds`.
    pub fn from_rect(r: IRect, bounds: IRect) -> Option<Mask> {
        let rect = r.intersect(&bounds);
        if rect.is_empty() {
            return None;
        }
        Some(Mask { rect, alpha: vec![255; (rect.width() * rect.height()) as usize] })
    }

    /// Free-form (lasso) selection; pixel centers inside the polygon are selected.
    pub fn from_polygon(points: &[(f32, f32)], bounds: IRect) -> Option<Mask> {
        if points.len() < 3 {
            return None;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in points {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let rect = IRect::new(x0.floor() as i32, y0.floor() as i32, x1.ceil() as i32 + 1, y1.ceil() as i32 + 1).intersect(&bounds);
        if rect.is_empty() {
            return None;
        }
        let (w, h) = (rect.width() as usize, rect.height() as usize);
        let mut alpha = vec![0u8; w * h];
        let mut xs = Vec::new();
        for row in 0..h {
            let py = rect.y0 as f32 + row as f32 + 0.5;
            xs.clear();
            for i in 0..points.len() {
                let (ax, ay) = points[i];
                let (bx, by) = points[(i + 1) % points.len()];
                if (ay <= py && by > py) || (by <= py && ay > py) {
                    xs.push(ax + (py - ay) / (by - ay) * (bx - ax));
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks_exact(2) {
                let from = ((pair[0] - 0.5).ceil() as i32 - rect.x0).max(0) as usize;
                let to = (((pair[1] - 0.5).floor() as i32 - rect.x0 + 1).max(0) as usize).min(w);
                for v in &mut alpha[row * w + from.min(w)..row * w + to.max(from.min(w))] {
                    *v = 255;
                }
            }
        }
        alpha.iter().any(|&a| a > 0).then_some(Mask { rect, alpha })
    }

    /// Mask from a full-image boolean map (magic wand) cropped to `rect`.
    pub fn from_bits(bits: &[bool], image_width: i32, rect: IRect) -> Option<Mask> {
        if rect.is_empty() {
            return None;
        }
        let mut alpha = Vec::with_capacity((rect.width() * rect.height()) as usize);
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                alpha.push(if bits[(y * image_width + x) as usize] { 255 } else { 0 });
            }
        }
        Some(Mask { rect, alpha })
    }

    pub fn at(&self, x: i32, y: i32) -> u8 {
        if !self.rect.contains(x, y) {
            return 0;
        }
        self.alpha[((y - self.rect.y0) * self.rect.width() + (x - self.rect.x0)) as usize]
    }
}

/// Affine transform around the center of the source rectangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xform {
    pub dx: f32,
    pub dy: f32,
    /// Negative values mirror.
    pub sx: f32,
    pub sy: f32,
    /// Radians, clockwise on screen.
    pub angle: f32,
}

impl Default for Xform {
    fn default() -> Self {
        Self { dx: 0.0, dy: 0.0, sx: 1.0, sy: 1.0, angle: 0.0 }
    }
}

impl Xform {
    pub fn is_identity(&self) -> bool {
        *self == Xform::default()
    }

    fn forward(&self, c: (f32, f32), p: (f32, f32)) -> (f32, f32) {
        let (vx, vy) = ((p.0 - c.0) * self.sx, (p.1 - c.1) * self.sy);
        let (s, co) = self.angle.sin_cos();
        (c.0 + self.dx + vx * co - vy * s, c.1 + self.dy + vx * s + vy * co)
    }

    fn inverse(&self, c: (f32, f32), q: (f32, f32)) -> (f32, f32) {
        let (vx, vy) = (q.0 - c.0 - self.dx, q.1 - c.1 - self.dy);
        let (s, co) = self.angle.sin_cos();
        let (rx, ry) = (vx * co + vy * s, -vx * s + vy * co);
        let sx = if self.sx.abs() < 1e-4 { 1e-4 } else { self.sx };
        let sy = if self.sy.abs() < 1e-4 { 1e-4 } else { self.sy };
        (c.0 + rx / sx, c.1 + ry / sy)
    }
}

/// Pixels being moved, plus the bookkeeping to apply them back.
pub struct Transform {
    owner: FrameId,
    layer: LayerId,
    edit: PixelEditBuilder,
    /// Source rectangle of the floating pixels.
    rect: IRect,
    /// Premultiplied RGBA8, `rect`-sized, already multiplied by the mask.
    pixels: Vec<u8>,
    /// Present when the pixels were cut out of the cel (so the hole is kept).
    mask: Option<Mask>,
    xf: Xform,
    last: IRect,
    /// Content erased with `clear_pixels`.
    cleared: bool,
}

impl Transform {
    /// Cut the selected pixels out of `img`. None if the selection is empty.
    pub fn lift(img: &mut TiledImage, owner: FrameId, layer: LayerId, mask: Mask) -> Option<Self> {
        let rect = mask.rect;
        let mut pixels = img.read_region(&rect);
        let mut any = false;
        for (i, px) in pixels.chunks_exact_mut(4).enumerate() {
            let m = mask.alpha[i] as u32;
            for c in px.iter_mut() {
                *c = ((*c as u32 * m + 127) / 255) as u8;
            }
            any |= px[3] > 0;
        }
        if !any {
            return None;
        }
        let mut t = Self { owner, layer, edit: PixelEditBuilder::new(), rect, pixels, mask: Some(mask), xf: Xform::default(), last: rect, cleared: false };
        t.redraw(img, rect);
        Some(t)
    }

    /// Paste pixels (premultiplied, `rect`-sized) on top of `img` as a new floating selection.
    pub fn paste(img: &mut TiledImage, owner: FrameId, layer: LayerId, rect: IRect, pixels: Vec<u8>) -> Self {
        let mut t = Self { owner, layer, edit: PixelEditBuilder::new(), rect, pixels, mask: None, xf: Xform::default(), last: rect, cleared: false };
        t.redraw(img, rect);
        t
    }

    /// Lifted from the layer and never moved or erased: applying it would change nothing.
    pub fn is_untouched(&self) -> bool {
        self.mask.is_some() && self.xf.is_identity() && !self.cleared
    }

    pub fn owner(&self) -> FrameId {
        self.owner
    }

    pub fn layer(&self) -> LayerId {
        self.layer
    }

    pub fn xform(&self) -> Xform {
        self.xf
    }

    pub fn source_rect(&self) -> IRect {
        self.rect
    }

    /// The floating pixels (for copy / paste).
    pub fn pixels(&self) -> (IRect, &[u8]) {
        (self.rect, &self.pixels)
    }

    fn center(&self) -> (f32, f32) {
        ((self.rect.x0 + self.rect.x1) as f32 * 0.5, (self.rect.y0 + self.rect.y1) as f32 * 0.5)
    }

    /// Corners of the transformed box (top-left, top-right, bottom-right, bottom-left).
    pub fn corners(&self) -> [(f32, f32); 4] {
        let r = self.rect;
        let c = self.center();
        [(r.x0, r.y0), (r.x1, r.y0), (r.x1, r.y1), (r.x0, r.y1)].map(|(x, y)| self.xf.forward(c, (x as f32, y as f32)))
    }

    fn target_bounds(&self) -> IRect {
        let cs = self.corners();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (x, y) in cs {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        IRect::new(x0.floor() as i32 - 1, y0.floor() as i32 - 1, x1.ceil() as i32 + 1, y1.ceil() as i32 + 1)
    }

    /// Change the transform; returns the canvas area that changed.
    pub fn set_xform(&mut self, img: &mut TiledImage, xf: Xform) -> IRect {
        let before = self.last;
        self.xf = xf;
        let target = self.target_bounds().intersect(&img.bounds());
        let area = before.union(&target);
        self.redraw(img, area);
        area
    }

    /// Restore `area` to the original, cut the hole again, and draw the pixels.
    fn redraw(&mut self, img: &mut TiledImage, area: IRect) {
        let area = area.intersect(&img.bounds());
        for (tx, ty) in img.tiles_in_rect(&area).collect::<Vec<_>>() {
            let idx = img.tile_index(tx, ty);
            self.edit.touch(img, idx);
            let orig = self.edit.original(idx).cloned().flatten();
            img.set_tile_ref(idx, orig);
        }
        // Restoring whole tiles may have brought back pixels outside `area`
        // that belonged to the previous drawing: widen to tile edges.
        let t = crate::raster::TILE as i32;
        let tiles_area = IRect::new(area.x0.div_euclid(t) * t, area.y0.div_euclid(t) * t, (area.x1 + t - 1).div_euclid(t) * t, (area.y1 + t - 1).div_euclid(t) * t)
            .intersect(&img.bounds());
        if let Some(mask) = &self.mask {
            let r = mask.rect.intersect(&tiles_area);
            for y in r.y0..r.y1 {
                for x in r.x0..r.x1 {
                    let m = mask.at(x, y) as u32;
                    if m == 0 {
                        continue;
                    }
                    let p = img.pixel(x, y);
                    img.set_pixel(x, y, p.map(|c| ((c as u32 * (255 - m) + 127) / 255) as u8));
                }
            }
        }
        let target = self.target_bounds().intersect(&tiles_area);
        let c = self.center();
        let (w, h) = (self.rect.width(), self.rect.height());
        let identity = self.xf.is_identity();
        for y in target.y0..target.y1 {
            for x in target.x0..target.x1 {
                let src = if identity {
                    let (lx, ly) = (x - self.rect.x0, y - self.rect.y0);
                    if lx < 0 || ly < 0 || lx >= w || ly >= h {
                        continue;
                    }
                    let o = ((ly * w + lx) * 4) as usize;
                    [self.pixels[o] as f32, self.pixels[o + 1] as f32, self.pixels[o + 2] as f32, self.pixels[o + 3] as f32]
                } else {
                    let (sx, sy) = self.xf.inverse(c, (x as f32 + 0.5, y as f32 + 0.5));
                    self.sample(sx - self.rect.x0 as f32, sy - self.rect.y0 as f32)
                };
                if src[3] <= 0.5 {
                    continue;
                }
                let d = img.pixel(x, y);
                let k = 1.0 - src[3] / 255.0;
                let out: [u8; 4] = std::array::from_fn(|i| (src[i] + d[i] as f32 * k + 0.5).clamp(0.0, 255.0) as u8);
                img.set_pixel(x, y, out);
            }
        }
        self.last = self.target_bounds().intersect(&img.bounds());
    }

    /// Bilinear sample of the floating pixels at local coordinates.
    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let (w, h) = (self.rect.width(), self.rect.height());
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |xx: i32, yy: i32| -> [f32; 4] {
            if xx < 0 || yy < 0 || xx >= w || yy >= h {
                return [0.0; 4];
            }
            let o = ((yy * w + xx) * 4) as usize;
            [self.pixels[o] as f32, self.pixels[o + 1] as f32, self.pixels[o + 2] as f32, self.pixels[o + 3] as f32]
        };
        let (a, b, c, d) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
        std::array::from_fn(|i| {
            let top = a[i] + (b[i] - a[i]) * fx;
            let bot = c[i] + (d[i] - c[i]) * fx;
            top + (bot - top) * fy
        })
    }

    /// Erase the floating pixels (delete the selection's content).
    pub fn clear_pixels(&mut self, img: &mut TiledImage) -> IRect {
        self.cleared = true;
        self.pixels.fill(0);
        let area = self.last.union(&self.rect);
        self.redraw(img, area);
        area
    }

    /// Keep the result; returns the undo record.
    pub fn commit(self, img: &mut TiledImage) -> Option<PixelEdit> {
        let (owner, layer) = (self.owner, self.layer);
        self.edit.finish(img, owner, layer)
    }

    /// Throw the transform away and restore the cel. Returns the area to redraw.
    pub fn cancel(self, img: &mut TiledImage) -> IRect {
        let area = self.last.union(&self.rect);
        self.edit.revert(img);
        area
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img_with_dot() -> TiledImage {
        let mut img = TiledImage::new(200, 200);
        for y in 10..20 {
            for x in 10..20 {
                img.set_pixel(x, y, [255, 0, 0, 255]);
            }
        }
        img
    }

    #[test]
    fn move_selection() {
        let mut img = img_with_dot();
        let mask = Mask::from_rect(IRect::new(5, 5, 25, 25), img.bounds()).unwrap();
        let mut t = Transform::lift(&mut img, 1, 1, mask).unwrap();
        t.set_xform(&mut img, Xform { dx: 100.0, dy: 50.0, ..Default::default() });
        assert_eq!(img.pixel(15, 15), [0; 4], "source is cut");
        assert_eq!(img.pixel(115, 65), [255, 0, 0, 255]);
        // Moving again must not leave a copy behind.
        t.set_xform(&mut img, Xform { dx: 150.0, dy: 150.0, ..Default::default() });
        assert_eq!(img.pixel(115, 65), [0; 4]);
        assert_eq!(img.pixel(165, 165), [255, 0, 0, 255]);
        let edit = t.commit(&mut img).unwrap();
        assert!(!edit.tiles.is_empty());
    }

    #[test]
    fn cancel_restores() {
        let mut img = img_with_dot();
        let mask = Mask::from_rect(IRect::new(0, 0, 30, 30), img.bounds()).unwrap();
        let mut t = Transform::lift(&mut img, 1, 1, mask).unwrap();
        t.set_xform(&mut img, Xform { dx: 80.0, sx: 2.0, sy: 2.0, angle: 0.5, ..Default::default() });
        t.cancel(&mut img);
        assert_eq!(img.pixel(15, 15), [255, 0, 0, 255]);
        assert_eq!(img.pixel(95, 15), [0; 4]);
    }

    #[test]
    fn flip_mirrors_pixels() {
        let mut img = TiledImage::new(100, 100);
        img.set_pixel(10, 10, [0, 0, 255, 255]);
        let mask = Mask::from_rect(IRect::new(10, 10, 20, 20), img.bounds()).unwrap();
        let mut t = Transform::lift(&mut img, 1, 1, mask).unwrap();
        t.set_xform(&mut img, Xform { sx: -1.0, ..Default::default() });
        assert_eq!(img.pixel(19, 10)[3], 255);
        assert_eq!(img.pixel(10, 10)[3], 0);
    }

    #[test]
    fn lasso_mask() {
        let tri = [(0.0, 0.0), (20.0, 0.0), (0.0, 20.0)];
        let m = Mask::from_polygon(&tri, IRect::new(0, 0, 100, 100)).unwrap();
        assert_eq!(m.at(2, 2), 255);
        assert_eq!(m.at(18, 18), 0);
        assert!(Mask::from_polygon(&[(0.0, 0.0), (1.0, 1.0)], IRect::new(0, 0, 10, 10)).is_none());
    }

    #[test]
    fn empty_selection_is_none() {
        let mut img = TiledImage::new(64, 64);
        let mask = Mask::from_rect(IRect::new(0, 0, 10, 10), img.bounds()).unwrap();
        assert!(Transform::lift(&mut img, 1, 1, mask).is_none());
    }
}
