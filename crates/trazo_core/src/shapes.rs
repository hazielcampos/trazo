//! Geometric shapes (line, rectangle, ellipse) as point paths for the brush engine.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeKind {
    Line,
    Rect,
    Ellipse,
}

impl ShapeKind {
    pub const ALL: [ShapeKind; 3] = [ShapeKind::Line, ShapeKind::Rect, ShapeKind::Ellipse];

    pub fn label(self) -> &'static str {
        match self {
            ShapeKind::Line => "Línea",
            ShapeKind::Rect => "Rectángulo",
            ShapeKind::Ellipse => "Elipse",
        }
    }
}

/// Points along the shape dragged from `a` to `b`. With `constrain`, lines
/// snap to 15° steps and rectangles/ellipses become squares/circles.
/// Closed shapes end where they start.
pub fn shape_points(kind: ShapeKind, a: (f32, f32), b: (f32, f32), constrain: bool) -> Vec<(f32, f32)> {
    let (mut dx, mut dy) = (b.0 - a.0, b.1 - a.1);
    if constrain {
        match kind {
            ShapeKind::Line => {
                let len = (dx * dx + dy * dy).sqrt();
                let step = 15f32.to_radians();
                let ang = (dy.atan2(dx) / step).round() * step;
                (dx, dy) = (ang.cos() * len, ang.sin() * len);
            }
            _ => {
                let m = dx.abs().max(dy.abs());
                (dx, dy) = (m.copysign(dx), m.copysign(dy));
            }
        }
    }
    let b = (a.0 + dx, a.1 + dy);
    let seg = |p: (f32, f32), q: (f32, f32), out: &mut Vec<(f32, f32)>| {
        let len = ((q.0 - p.0).powi(2) + (q.1 - p.1).powi(2)).sqrt();
        let n = (len / 2.0).ceil().max(1.0) as usize;
        for i in 0..n {
            let t = i as f32 / n as f32;
            out.push((p.0 + (q.0 - p.0) * t, p.1 + (q.1 - p.1) * t));
        }
    };
    let mut out = Vec::new();
    match kind {
        ShapeKind::Line => {
            seg(a, b, &mut out);
            out.push(b);
        }
        ShapeKind::Rect => {
            let c = [a, (b.0, a.1), b, (a.0, b.1)];
            for i in 0..4 {
                seg(c[i], c[(i + 1) % 4], &mut out);
            }
            out.push(a);
        }
        ShapeKind::Ellipse => {
            let (cx, cy) = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5);
            let (rx, ry) = ((dx * 0.5).abs(), (dy * 0.5).abs());
            let n = ((rx + ry) * 1.6).clamp(16.0, 2000.0) as usize;
            for i in 0..=n {
                let t = i as f32 / n as f32 * std::f32::consts::TAU;
                out.push((cx + rx * t.cos(), cy + ry * t.sin()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_close_and_constrain() {
        let r = shape_points(ShapeKind::Rect, (0.0, 0.0), (10.0, 4.0), true);
        assert_eq!(r.first(), r.last());
        assert!(r.iter().any(|p| (p.1 - 10.0).abs() < 1e-3), "square when constrained");
        let l = shape_points(ShapeKind::Line, (0.0, 0.0), (10.0, 1.0), true);
        assert!(l.last().unwrap().1.abs() < 1e-3, "snapped to horizontal");
        let e = shape_points(ShapeKind::Ellipse, (0.0, 0.0), (20.0, 10.0), false);
        assert!(e.iter().all(|p| p.0 >= -0.01 && p.0 <= 20.01));
    }
}
