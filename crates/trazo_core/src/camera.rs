//! Camera: the part of the canvas that ends up in the video. It moves from a
//! start framing to an end framing over the whole animation.

use serde::{Deserialize, Serialize};

use crate::transform::Ease;

/// A framing: center of the view (as a fraction of the canvas, 0.5 = middle),
/// zoom (1 = the whole canvas, 2 = half of it fills the video) and turn.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CamPose {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
    /// Radians, clockwise.
    pub angle: f32,
}

impl Default for CamPose {
    fn default() -> Self {
        Self { x: 0.5, y: 0.5, zoom: 1.0, angle: 0.0 }
    }
}

impl CamPose {
    pub fn lerp(&self, to: &CamPose, t: f32) -> CamPose {
        let mix = |a: f32, b: f32| a + (b - a) * t;
        // Zoom changes geometrically so a push-in feels even.
        let zoom = self.zoom.max(0.05) * (to.zoom.max(0.05) / self.zoom.max(0.05)).powf(t);
        CamPose { x: mix(self.x, to.x), y: mix(self.y, to.y), zoom, angle: mix(self.angle, to.angle) }
    }

    /// Corners of the area seen, in canvas pixels (top-left, top-right,
    /// bottom-right, bottom-left).
    pub fn corners(&self, w: u32, h: u32) -> [(f32, f32); 4] {
        let (cx, cy) = (self.x * w as f32, self.y * h as f32);
        let (hw, hh) = (w as f32 / 2.0 / self.zoom, h as f32 / 2.0 / self.zoom);
        let (s, c) = self.angle.sin_cos();
        [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh)].map(|(x, y)| (cx + x * c - y * s, cy + x * s + y * c))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct Camera {
    pub enabled: bool,
    pub start: CamPose,
    pub end: CamPose,
    #[serde(default)]
    pub ease: Ease,
}

impl Camera {
    /// Framing at `frame` of `frames`.
    pub fn pose_at(&self, frame: usize, frames: usize) -> CamPose {
        let t = if frames > 1 { frame as f32 / (frames - 1) as f32 } else { 0.0 };
        self.start.lerp(&self.end, self.ease.apply(t))
    }

    /// Whether the camera changes the picture at all.
    pub fn is_active(&self) -> bool {
        self.enabled && (self.start != CamPose::default() || self.end != CamPose::default())
    }
}

/// Look at a rendered canvas (premultiplied RGBA, `w`×`h`) through the
/// camera: same size out, `fill` where the view leaves the canvas.
pub fn view(src: &[u8], w: u32, h: u32, pose: &CamPose, fill: [u8; 4]) -> Vec<u8> {
    let (wi, hi) = (w as i32, h as i32);
    let mut out = vec![0u8; src.len()];
    let (cx, cy) = (pose.x * w as f32, pose.y * h as f32);
    let (s, c) = pose.angle.sin_cos();
    let inv = 1.0 / pose.zoom.max(0.05);
    let at = |x: i32, y: i32| -> [f32; 4] {
        if x < 0 || y < 0 || x >= wi || y >= hi {
            return fill.map(|v| v as f32);
        }
        let o = ((y * wi + x) * 4) as usize;
        [src[o] as f32, src[o + 1] as f32, src[o + 2] as f32, src[o + 3] as f32]
    };
    for oy in 0..hi {
        for ox in 0..wi {
            // Output pixel → offset from the view center → canvas.
            let (vx, vy) = ((ox as f32 + 0.5 - w as f32 / 2.0) * inv, (oy as f32 + 0.5 - h as f32 / 2.0) * inv);
            let (sx, sy) = (cx + vx * c - vy * s - 0.5, cy + vx * s + vy * c - 0.5);
            let (x0, y0) = (sx.floor() as i32, sy.floor() as i32);
            let (fx, fy) = (sx - x0 as f32, sy - y0 as f32);
            let (a, b, d, e) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
            let o = ((oy * wi + ox) * 4) as usize;
            for i in 0..4 {
                let top = a[i] + (b[i] - a[i]) * fx;
                let bot = d[i] + (e[i] - d[i]) * fx;
                out[o + i] = (top + (bot - top) * fy + 0.5).clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_view_keeps_the_picture() {
        let (w, h) = (8u32, 6u32);
        let src: Vec<u8> = (0..w * h * 4).map(|i| (i % 251) as u8).collect();
        let out = view(&src, w, h, &CamPose::default(), [0; 4]);
        assert_eq!(out, src);
    }

    #[test]
    fn zoom_two_shows_the_middle() {
        let (w, h) = (4u32, 4u32);
        // A bright 2×2 square in the middle.
        let mut src = vec![0u8; (w * h * 4) as usize];
        for (x, y) in [(1, 1), (2, 1), (1, 2), (2, 2)] {
            let o = ((y * w + x) * 4) as usize;
            src[o..o + 4].copy_from_slice(&[255, 255, 255, 255]);
        }
        let out = view(&src, w, h, &CamPose { zoom: 2.0, ..Default::default() }, [0; 4]);
        // The middle square now fills most of the picture.
        let lit = out.chunks_exact(4).filter(|p| p[3] > 128).count();
        assert!(lit >= 12, "lit {lit}");
    }

    #[test]
    fn pose_moves_from_start_to_end() {
        let cam = Camera { enabled: true, start: CamPose::default(), end: CamPose { x: 0.7, zoom: 2.0, ..Default::default() }, ease: Ease::Linear };
        assert_eq!(cam.pose_at(0, 11), cam.start);
        let mid = cam.pose_at(5, 11);
        assert!((mid.x - 0.6).abs() < 1e-5);
        assert!((mid.zoom - 2f32.sqrt()).abs() < 1e-4);
        assert_eq!(cam.pose_at(10, 11).x, 0.7);
    }
}
