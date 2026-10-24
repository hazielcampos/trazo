//! Icons from the design, drawn from their SVG path data (24×24 grid,
//! 1.75 stroke, round ends) so the app matches the "Iconos" board exactly.

use egui::{Color32, Pos2, Shape, Stroke};

use crate::icons::Icon;

/// One element of an icon.
#[derive(Clone, Copy)]
pub enum El {
    /// Stroked path.
    P(&'static str),
    /// Stroked path with dashes (dash, gap).
    D(&'static str, f32, f32),
    /// Filled path (convex shapes only).
    F(&'static str),
    /// Stroked circle (cx, cy, r).
    C(f32, f32, f32),
    /// Dashed circle.
    CD(f32, f32, f32, f32, f32),
    /// Filled dot.
    Dot(f32, f32, f32),
    /// Stroked rounded rectangle (x, y, w, h, r).
    R(f32, f32, f32, f32, f32),
    /// Dashed rounded rectangle.
    RD(f32, f32, f32, f32, f32, f32, f32),
}

use El::*;

/// The design's icons; `None` keeps the older drawing for icons the design
/// does not redraw.
pub fn shapes(icon: Icon) -> Option<&'static [El]> {
    Some(match icon {
        Icon::Pencil => &[P("M4.5 19.5l1.2-4.3 9.6-9.6a2.2 2.2 0 0 1 3.1 3.1l-9.6 9.6z"), P("M13.6 7.3l3.1 3.1")],
        Icon::Eraser => &[P("M9 19.5h10"), P("M5.6 15.1l8-8a2 2 0 0 1 2.8 0l2.5 2.5a2 2 0 0 1 0 2.8l-7.1 7.1H9.9z"), P("M9.6 11.1l4.3 4.3")],
        Icon::Bucket => &[P("M6 10.5l5-5.5 7.5 7.5-5.4 5.4a2 2 0 0 1-2.8 0L6 13.6a2.2 2.2 0 0 1 0-3.1z"), P("M6.2 12.4h12"), P("M20.5 17c0 1.1-.7 1.9-1.5 1.9s-1.5-.8-1.5-1.9c0-1 1.5-2.6 1.5-2.6s1.5 1.6 1.5 2.6z")],
        Icon::Eyedropper => &[P("M14.5 5.5l4 4"), P("M16.8 3.2l4 4-2.6 2.6-4-4z"), P("M15 7.8l-8.6 8.6-.9 3.1 3.1-.9 8.6-8.6")],
        Icon::Lasso => &[P("M12 4.5c4.3 0 7.7 2 7.7 4.6S16.3 13.7 12 13.7 4.3 11.7 4.3 9.1 7.7 4.5 12 4.5z"), P("M8.2 13.1c-1.4.5-2.1 1.6-1.6 2.6.5 1 2 1.2 2.8.3"), P("M9.4 16c.1 1.6-.8 3-2.4 3.7")],
        Icon::SelectRect => &[RD(4.5, 5.5, 15.0, 13.0, 1.5, 3.0, 2.4)],
        Icon::Wand => &[P("M4.5 19.5L14 10"), P("M15.5 3.5v2.5M20.5 8.5H18M19 5l-1.8 1.8M14 10l1.8 1.8")],
        Icon::Shapes => &[R(4.0, 4.0, 9.0, 9.0, 2.0), C(15.5, 15.5, 4.5)],
        Icon::Text => &[P("M5.5 7V5h13v2"), P("M12 5v14"), P("M9 19h6")],
        Icon::Hand => &[P("M12 3.5v17M3.5 12h17"), P("M9.5 6L12 3.5 14.5 6M9.5 18l2.5 2.5 2.5-2.5M6 9.5L3.5 12 6 14.5M18 9.5l2.5 2.5-2.5 2.5")],
        Icon::Play => &[F("M8.5 5.8v12.4l11.2-6.2z")],
        Icon::Undo => &[P("M9 14L4.5 9.5 9 5"), P("M4.5 9.5H15a5 5 0 0 1 0 10h-3")],
        Icon::Redo => &[P("M15 14l4.5-4.5L15 5"), P("M19.5 9.5H9a5 5 0 0 0 0 10h3")],
        Icon::Export => &[P("M12 15V4.5"), P("M8 8.5l4-4 4 4"), P("M5 13.5V18a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-4.5")],
        Icon::Import => &[P("M12 4.5V15"), P("M8 11l4 4 4-4"), P("M5 17.5v1a1.5 1.5 0 0 0 1.5 1.5h11a1.5 1.5 0 0 0 1.5-1.5v-1")],
        Icon::Layers => &[P("M12 3.8l8.5 4.6-8.5 4.6-8.5-4.6z"), P("M3.5 12.6l8.5 4.6 8.5-4.6"), P("M3.5 16.6l8.5 4.6 8.5-4.6")],
        Icon::Brush => &[P("M14 4.5l5.5 5.5-7 7-5.5-5.5z"), P("M7 11.5L4.5 19.5l8-2.5")],
        Icon::Onion => &[C(9.5, 12.0, 5.5), CD(14.5, 12.0, 5.5, 2.2, 2.2)],
        Icon::Loop => &[P("M17 3.5l3 3-3 3"), P("M4 11.5v-1a4 4 0 0 1 4-4h12"), P("M7 20.5l-3-3 3-3"), P("M20 12.5v1a4 4 0 0 1-4 4H4")],
        Icon::Plus => &[P("M12 5v14M5 12h14")],
        Icon::New => &[R(4.5, 4.0, 11.0, 16.0, 2.5), P("M19.5 9v6M16.5 12h6")],
        Icon::More => &[Dot(5.5, 12.0, 1.5), Dot(12.0, 12.0, 1.5), Dot(18.5, 12.0, 1.5)],
        Icon::Close => &[P("M6 6l12 12M18 6L6 18")],
        Icon::ArrowLeft | Icon::Prev => &[P("M14.5 18l-6-6 6-6")],
        Icon::Next => &[P("M9 6l6 6-6 6")],
        Icon::Down => &[P("M7 10l5 5 5-5")],
        Icon::Up => &[P("M7 14l5-5 5 5")],
        Icon::First => &[P("M6 5v14M18 6l-8 6 8 6z")],
        Icon::Last => &[P("M18 5v14M6 6l8 6-8 6z")],
        Icon::Eye => &[P("M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"), C(12.0, 12.0, 3.0)],
        Icon::EyeOff => &[P("M4 4l16 16"), P("M9.9 5.8A9.6 9.6 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a16 16 0 0 1-2.6 3.4M6.3 7.6A15.6 15.6 0 0 0 2.5 12S6 18.5 12 18.5a9 9 0 0 0 4.1-1")],
        Icon::Lock => &[R(5.0, 11.0, 14.0, 9.0, 2.0), P("M8 11V8a4 4 0 0 1 8 0v3")],
        Icon::Unlock => &[R(5.0, 11.0, 14.0, 9.0, 2.0), P("M8 11V8a4 4 0 0 1 8 0")],
        Icon::Trash => &[P("M4.5 7h15"), P("M9.5 7V5a1.5 1.5 0 0 1 1.5-1.5h2A1.5 1.5 0 0 1 14.5 5v2"), P("M6.5 7l.9 11.3A2 2 0 0 0 9.4 20h5.2a2 2 0 0 0 2-1.7L17.5 7")],
        Icon::Duplicate => &[R(3.5, 6.0, 8.0, 12.0, 2.0), R(12.5, 6.0, 8.0, 12.0, 2.0), P("M16.5 10v4M14.5 12h4")],
        Icon::Copy => &[R(8.0, 8.0, 11.5, 11.5, 2.5), P("M15.5 8V6.5A2 2 0 0 0 13.5 4.5h-7a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2H8")],
        Icon::Paste => &[R(5.0, 5.5, 14.0, 15.0, 2.5), P("M9 5.5V4.5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v1"), P("M9 12h6M9 15.5h4")],
        Icon::Camera => &[R(3.5, 6.0, 13.0, 12.0, 2.5), P("M16.5 10.5l4-2.5v8l-4-2.5")],
        Icon::Film => &[R(3.5, 5.5, 12.5, 13.0, 2.5), P("M16 10.5l4.5-2.8v8.6L16 13.5")],
        Icon::Image => &[R(3.5, 4.5, 17.0, 15.0, 2.5), C(9.0, 10.0, 1.8), P("M20 16l-5-5-8.5 8.5")],
        Icon::Mic => &[R(9.0, 3.5, 6.0, 11.0, 3.0), P("M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5v3")],
        Icon::Music => &[P("M9 18V6l10-2v12"), C(6.5, 18.0, 2.5), C(16.5, 16.0, 2.5)],
        Icon::Speaker => &[P("M4 9.5v5h3.5L12 18.5v-13L7.5 9.5z"), P("M15.5 9a4 4 0 0 1 0 6")],
        Icon::Clock => &[C(12.0, 12.0, 8.0), P("M12 7.5V12l3 2")],
        Icon::Search => &[C(11.0, 11.0, 6.5), P("M16 16l4 4")],
        Icon::Globe => &[C(12.0, 12.0, 8.5), P("M3.5 12h17M12 3.5c2.5 2.5 3.5 5.5 3.5 8.5s-1 6-3.5 8.5c-2.5-2.5-3.5-5.5-3.5-8.5s1-6 3.5-8.5z")],
        Icon::Settings => &[P("M4 7h9M17 7h3M4 17h3M11 17h9"), C(15.0, 7.0, 2.0), C(9.0, 17.0, 2.0)],
        Icon::Tween => &[C(5.0, 15.0, 2.0), CD(12.0, 10.0, 2.0, 2.0, 2.0), C(19.0, 7.0, 2.0), P("M6.8 13.8l3.4-2.4M13.9 9.1l3.3-1.4")],
        Icon::Fit => &[P("M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5")],
        Icon::FlipH => &[D("M12 3.5v17", 2.0, 2.2), P("M9 7L4 17h5zM15 7l5 10h-5z")],
        Icon::RotateRight => &[P("M19 12a7 7 0 1 1-2.1-5"), P("M17.5 3.5v3.6h-3.6")],
        Icon::Grid => &[P("M4 9h16M4 15h16M9 4v16M15 4v16")],
        Icon::Symmetry => &[P("M12 3.5v17"), P("M8.5 7.5L4 12l4.5 4.5M15.5 7.5L20 12l-4.5 4.5")],
        Icon::Check => &[P("M6 12.5l4 4 8-9")],
        _ => return None,
    })
}

// ---- path data -------------------------------------------------------------------

/// Polylines (24×24 units) and whether each is closed.
fn parse(d: &str) -> Vec<(Vec<(f32, f32)>, bool)> {
    let mut out: Vec<(Vec<(f32, f32)>, bool)> = Vec::new();
    let mut cur: Vec<(f32, f32)> = Vec::new();
    let mut toks = Tokens::new(d);
    let (mut x, mut y) = (0.0f32, 0.0f32);
    let (mut sx, mut sy) = (0.0f32, 0.0f32);
    let mut last_ctrl: Option<(f32, f32)> = None;
    let mut cmd = 'M';
    while let Some(t) = toks.peek_cmd() {
        if let Some(c) = t {
            cmd = c;
            toks.advance();
        }
        let rel = cmd.is_ascii_lowercase();
        let (ox, oy) = if rel { (x, y) } else { (0.0, 0.0) };
        match cmd.to_ascii_uppercase() {
            'M' => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), false));
                }
                cur.clear();
                x = ox + toks.num();
                y = oy + toks.num();
                sx = x;
                sy = y;
                cur.push((x, y));
                cmd = if rel { 'l' } else { 'L' };
                last_ctrl = None;
            }
            'L' => {
                x = ox + toks.num();
                y = oy + toks.num();
                cur.push((x, y));
                last_ctrl = None;
            }
            'H' => {
                x = if rel { x + toks.num() } else { toks.num() };
                cur.push((x, y));
                last_ctrl = None;
            }
            'V' => {
                y = if rel { y + toks.num() } else { toks.num() };
                cur.push((x, y));
                last_ctrl = None;
            }
            'C' => {
                let (x1, y1) = (ox + toks.num(), oy + toks.num());
                let (x2, y2) = (ox + toks.num(), oy + toks.num());
                let (ex, ey) = (ox + toks.num(), oy + toks.num());
                cubic(&mut cur, (x, y), (x1, y1), (x2, y2), (ex, ey));
                last_ctrl = Some((x2, y2));
                x = ex;
                y = ey;
            }
            'S' => {
                let (x1, y1) = match last_ctrl {
                    Some((cx, cy)) => (2.0 * x - cx, 2.0 * y - cy),
                    None => (x, y),
                };
                let (x2, y2) = (ox + toks.num(), oy + toks.num());
                let (ex, ey) = (ox + toks.num(), oy + toks.num());
                cubic(&mut cur, (x, y), (x1, y1), (x2, y2), (ex, ey));
                last_ctrl = Some((x2, y2));
                x = ex;
                y = ey;
            }
            'Q' => {
                let (x1, y1) = (ox + toks.num(), oy + toks.num());
                let (ex, ey) = (ox + toks.num(), oy + toks.num());
                let c1 = (x + 2.0 / 3.0 * (x1 - x), y + 2.0 / 3.0 * (y1 - y));
                let c2 = (ex + 2.0 / 3.0 * (x1 - ex), ey + 2.0 / 3.0 * (y1 - ey));
                cubic(&mut cur, (x, y), c1, c2, (ex, ey));
                last_ctrl = None;
                x = ex;
                y = ey;
            }
            'A' => {
                let (rx, ry, rot) = (toks.num(), toks.num(), toks.num());
                let large = toks.flag();
                let sweep = toks.flag();
                let (ex, ey) = (ox + toks.num(), oy + toks.num());
                arc(&mut cur, (x, y), rx, ry, rot, large, sweep, (ex, ey));
                last_ctrl = None;
                x = ex;
                y = ey;
            }
            'Z' => {
                if cur.len() > 1 {
                    out.push((std::mem::take(&mut cur), true));
                }
                x = sx;
                y = sy;
                cur.push((x, y));
                last_ctrl = None;
                // A following command must be explicit.
                if toks.peek_cmd().is_some_and(|c| c.is_none()) {
                    break;
                }
            }
            _ => break,
        }
    }
    if cur.len() > 1 {
        out.push((cur, false));
    }
    out
}

fn cubic(out: &mut Vec<(f32, f32)>, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) {
    let n = 12;
    for i in 1..=n {
        let t = i as f32 / n as f32;
        let u = 1.0 - t;
        let a = u * u * u;
        let b = 3.0 * u * u * t;
        let c = 3.0 * u * t * t;
        let d = t * t * t;
        out.push((a * p0.0 + b * p1.0 + c * p2.0 + d * p3.0, a * p0.1 + b * p1.1 + c * p2.1 + d * p3.1));
    }
}

/// SVG elliptical arc, endpoint form (SVG 1.1, appendix F.6.5).
#[allow(clippy::too_many_arguments)]
fn arc(out: &mut Vec<(f32, f32)>, p0: (f32, f32), mut rx: f32, mut ry: f32, rot_deg: f32, large: bool, sweep: bool, p1: (f32, f32)) {
    if rx.abs() < 1e-6 || ry.abs() < 1e-6 {
        out.push(p1);
        return;
    }
    rx = rx.abs();
    ry = ry.abs();
    let phi = rot_deg.to_radians();
    let (cp, sp) = (phi.cos(), phi.sin());
    let dx = (p0.0 - p1.0) / 2.0;
    let dy = (p0.1 - p1.1) / 2.0;
    let x1 = cp * dx + sp * dy;
    let y1 = -sp * dx + cp * dy;
    let lam = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lam > 1.0 {
        rx *= lam.sqrt();
        ry *= lam.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let mut co = (num / den).max(0.0).sqrt();
    if large == sweep {
        co = -co;
    }
    let cx1 = co * rx * y1 / ry;
    let cy1 = -co * ry * x1 / rx;
    let cx = cp * cx1 - sp * cy1 + (p0.0 + p1.0) / 2.0;
    let cy = sp * cx1 + cp * cy1 + (p0.1 + p1.1) / 2.0;
    let ang = |ux: f32, uy: f32, vx: f32, vy: f32| {
        let a = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
        a
    };
    let t1 = ang(1.0, 0.0, (x1 - cx1) / rx, (y1 - cy1) / ry);
    let mut dt = ang((x1 - cx1) / rx, (y1 - cy1) / ry, (-x1 - cx1) / rx, (-y1 - cy1) / ry);
    if !sweep && dt > 0.0 {
        dt -= std::f32::consts::TAU;
    } else if sweep && dt < 0.0 {
        dt += std::f32::consts::TAU;
    }
    let n = ((dt.abs() / std::f32::consts::TAU) * 32.0).ceil().max(4.0) as usize;
    for i in 1..=n {
        let t = t1 + dt * i as f32 / n as f32;
        let (ct, st) = (t.cos(), t.sin());
        out.push((cp * rx * ct - sp * ry * st + cx, sp * rx * ct + cp * ry * st + cy));
    }
}

struct Tokens<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Tokens<'a> {
    fn new(d: &'a str) -> Self {
        Tokens { s: d.as_bytes(), i: 0 }
    }
    fn skip(&mut self) {
        while self.i < self.s.len() && (self.s[self.i] == b' ' || self.s[self.i] == b',' || self.s[self.i] == b'\n') {
            self.i += 1;
        }
    }
    /// `Some(Some(cmd))` for a command letter, `Some(None)` when a number
    /// follows (implicit repeat), `None` at the end.
    fn peek_cmd(&mut self) -> Option<Option<char>> {
        self.skip();
        let c = *self.s.get(self.i)? as char;
        Some(if c.is_ascii_alphabetic() { Some(c) } else { None })
    }
    fn advance(&mut self) {
        self.i += 1;
    }
    fn flag(&mut self) -> bool {
        self.skip();
        let c = self.s.get(self.i).copied().unwrap_or(b'0');
        self.i += 1;
        c == b'1'
    }
    fn num(&mut self) -> f32 {
        self.skip();
        let start = self.i;
        let mut seen_dot = false;
        if self.i < self.s.len() && (self.s[self.i] == b'-' || self.s[self.i] == b'+') {
            self.i += 1;
        }
        while self.i < self.s.len() {
            let c = self.s[self.i];
            if c.is_ascii_digit() {
                self.i += 1;
            } else if c == b'.' && !seen_dot {
                seen_dot = true;
                self.i += 1;
            } else if (c == b'e' || c == b'E') && self.i > start {
                self.i += 1;
                if self.i < self.s.len() && (self.s[self.i] == b'-' || self.s[self.i] == b'+') {
                    self.i += 1;
                }
            } else {
                break;
            }
        }
        std::str::from_utf8(&self.s[start..self.i]).ok().and_then(|t| t.parse().ok()).unwrap_or(0.0)
    }
}

// ---- drawing ------------------------------------------------------------------------

fn rounded_rect(x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<(f32, f32)> {
    let r = r.min(w / 2.0).min(h / 2.0);
    let mut v = Vec::new();
    let corner = |v: &mut Vec<(f32, f32)>, cx: f32, cy: f32, a0: f32| {
        for i in 0..=6 {
            let a = a0 + std::f32::consts::FRAC_PI_2 * i as f32 / 6.0;
            v.push((cx + r * a.cos(), cy + r * a.sin()));
        }
    };
    corner(&mut v, x + w - r, y + r, -std::f32::consts::FRAC_PI_2);
    corner(&mut v, x + w - r, y + h - r, 0.0);
    corner(&mut v, x + r, y + h - r, std::f32::consts::FRAC_PI_2);
    corner(&mut v, x + r, y + r, std::f32::consts::PI);
    v
}

fn circle_pts(cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
    (0..48).map(|i| {
        let a = i as f32 / 48.0 * std::f32::consts::TAU;
        (cx + r * a.cos(), cy + r * a.sin())
    }).collect()
}

/// Draw the icon into `rect`; returns false if it has no design shapes.
pub fn paint(painter: &egui::Painter, rect: egui::Rect, icon: Icon, color: Color32) -> bool {
    let Some(els) = shapes(icon) else { return false };
    let s = rect.width().min(rect.height()) / 24.0;
    let o = rect.center() - egui::Vec2::splat(12.0 * s);
    let map = |pts: &[(f32, f32)]| -> Vec<Pos2> { pts.iter().map(|&(x, y)| Pos2::new(o.x + x * s, o.y + y * s)).collect() };
    let w = (1.75 * s).max(1.1);
    let st = Stroke::new(w, color);
    let caps = |pts: &[Pos2]| {
        if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
            painter.circle_filled(*a, w / 2.0, color);
            painter.circle_filled(*b, w / 2.0, color);
        }
    };
    let stroke = |poly: Vec<Pos2>, closed: bool| {
        if closed {
            painter.add(Shape::closed_line(poly, st));
        } else {
            caps(&poly);
            painter.add(Shape::line(poly, st));
        }
    };
    let dashed = |mut poly: Vec<Pos2>, closed: bool, dash: f32, gap: f32| {
        if closed {
            if let Some(f) = poly.first().copied() {
                poly.push(f);
            }
        }
        painter.extend(Shape::dashed_line(&poly, st, dash * s, gap * s));
    };
    for el in els {
        match *el {
            P(d) => {
                for (pts, closed) in parse(d) {
                    stroke(map(&pts), closed);
                }
            }
            D(d, dash, gap) => {
                for (pts, closed) in parse(d) {
                    dashed(map(&pts), closed, dash, gap);
                }
            }
            F(d) => {
                for (pts, _) in parse(d) {
                    painter.add(Shape::convex_polygon(map(&pts), color, Stroke::NONE));
                }
            }
            C(cx, cy, r) => stroke(map(&circle_pts(cx, cy, r)), true),
            CD(cx, cy, r, dash, gap) => dashed(map(&circle_pts(cx, cy, r)), true, dash, gap),
            Dot(cx, cy, r) => {
                painter.circle_filled(Pos2::new(o.x + cx * s, o.y + cy * s), r * s, color);
            }
            R(x, y, wd, h, r) => stroke(map(&rounded_rect(x, y, wd, h, r)), true),
            RD(x, y, wd, h, r, dash, gap) => dashed(map(&rounded_rect(x, y, wd, h, r)), true, dash, gap),
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_design_icon_parses() {
        for icon in [Icon::Pencil, Icon::Eraser, Icon::Bucket, Icon::Lasso, Icon::Undo, Icon::Eye, Icon::EyeOff, Icon::Trash, Icon::Copy, Icon::Globe, Icon::Bucket] {
            for el in shapes(icon).unwrap() {
                if let P(d) | D(d, ..) | F(d) = el {
                    let polys = parse(d);
                    assert!(!polys.is_empty(), "{d}");
                    for (pts, _) in polys {
                        assert!(pts.iter().all(|(x, y)| x.is_finite() && y.is_finite() && *x > -2.0 && *x < 26.0 && *y > -2.0 && *y < 26.0), "{d}");
                    }
                }
            }
        }
    }

    #[test]
    fn arc_ends_at_target() {
        let mut v = vec![(4.5, 9.5)];
        arc(&mut v, (15.0, 9.5), 5.0, 5.0, 0.0, false, true, (15.0, 19.5));
        let last = *v.last().unwrap();
        assert!((last.0 - 15.0).abs() < 0.01 && (last.1 - 19.5).abs() < 0.01);
    }
}
