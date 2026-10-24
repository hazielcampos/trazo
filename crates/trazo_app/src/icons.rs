//! Vector icons drawn with the egui painter.
//!
//! Icons are drawn from simple shapes on a 24×24 grid instead of font glyphs,
//! so they look the same on every platform and never show up as empty boxes.

use std::f32::consts::{FRAC_PI_2, PI, TAU};

use egui::{Color32, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2, Widget};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    // Tools
    Pencil,
    Eraser,
    Bucket,
    Eyedropper,
    Hand,
    SelectRect,
    Lasso,
    Shapes,
    Line,
    Rect,
    Ellipse,
    Wand,
    Grid,
    Symmetry,
    Image,
    Film,
    RangeIn,
    RangeOut,
    PingPong,
    Mic,
    Speaker,
    SpeakerOff,
    Music,
    Record,
    Stop,
    // Top bar
    Menu,
    Undo,
    Redo,
    Play,
    Pause,
    Loop,
    Onion,
    RotateLeft,
    RotateRight,
    FlipH,
    FlipV,
    Fit,
    Layers,
    Brush,
    Palette,
    Save,
    Open,
    New,
    Settings,
    // Timeline / lists
    First,
    Prev,
    Next,
    Last,
    Plus,
    Minus,
    Duplicate,
    Trash,
    ArrowLeft,
    ArrowRight,
    Copy,
    Paste,
    Cut,
    Eye,
    EyeOff,
    Lock,
    Unlock,
    Up,
    Down,
    Merge,
    Clear,
    Import,
    Export,
    Check,
    Close,
    Background,
    More,
    Globe,
    Tween,
    Text,
    Camera,
    Clock,
    Search,
}

/// Draw `icon` filling `rect` (square works best).
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32) {
    let s = rect.width().min(rect.height()) / 24.0;
    let o = rect.center() - Vec2::splat(12.0 * s);
    let p = |x: f32, y: f32| Pos2::new(o.x + x * s, o.y + y * s);
    let w = (1.8 * s).max(1.2);
    let st = Stroke::new(w, color);
    let thin = Stroke::new((1.3 * s).max(1.0), color);
    let line = |pts: &[(f32, f32)]| {
        painter.add(Shape::line(pts.iter().map(|&(x, y)| p(x, y)).collect(), st));
    };
    let closed = |pts: &[(f32, f32)]| {
        painter.add(Shape::closed_line(pts.iter().map(|&(x, y)| p(x, y)).collect(), st));
    };
    let fill = |pts: &[(f32, f32)]| {
        painter.add(Shape::convex_polygon(pts.iter().map(|&(x, y)| p(x, y)).collect(), color, Stroke::NONE));
    };
    let circle = |x: f32, y: f32, r: f32| {
        painter.circle_stroke(p(x, y), r * s, st);
    };
    let dot = |x: f32, y: f32, r: f32| {
        painter.circle_filled(p(x, y), r * s, color);
    };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| -> Vec<Pos2> {
        let n = 24;
        (0..=n).map(|i| {
            let a = a0 + (a1 - a0) * i as f32 / n as f32;
            p(cx + r * a.cos(), cy + r * a.sin())
        }).collect()
    };
    let arrow_head = |tip: (f32, f32), dir: f32| {
        // Filled triangle pointing along `dir` (radians).
        let (dx, dy) = (dir.cos(), dir.sin());
        let (nx, ny) = (-dy, dx);
        let back = (tip.0 - dx * 4.5, tip.1 - dy * 4.5);
        fill(&[tip, (back.0 + nx * 3.2, back.1 + ny * 3.2), (back.0 - nx * 3.2, back.1 - ny * 3.2)]);
    };
    let dashed = |pts: &[(f32, f32)]| {
        let v: Vec<Pos2> = pts.iter().map(|&(x, y)| p(x, y)).collect();
        painter.extend(Shape::dashed_line(&v, thin, 2.5 * s, 2.0 * s));
    };
    let rrect = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        painter.rect_stroke(Rect::from_min_max(p(x0, y0), p(x1, y1)), r * s, st, egui::StrokeKind::Middle);
    };

    match icon {
        Icon::Pencil => {
            closed(&[(4.0, 20.0), (5.5, 15.0), (15.5, 5.0), (19.0, 8.5), (9.0, 18.5)]);
            line(&[(13.0, 7.5), (16.5, 11.0)]);
            fill(&[(4.0, 20.0), (5.0, 17.0), (7.0, 19.0)]);
        }
        Icon::Eraser => {
            closed(&[(8.0, 19.0), (3.5, 14.5), (12.5, 5.5), (20.5, 13.5), (15.0, 19.0)]);
            line(&[(8.0, 10.0), (16.5, 18.5)]);
            line(&[(8.0, 19.0), (21.0, 19.0)]);
        }
        Icon::Bucket => {
            closed(&[(4.0, 11.0), (11.0, 4.0), (19.0, 12.0), (12.0, 19.0)]);
            line(&[(4.0, 11.0), (19.0, 12.0)]);
            fill(&[(20.0, 14.5), (22.0, 18.5), (18.0, 18.5)]);
            dot(20.0, 18.8, 2.0);
        }
        Icon::Eyedropper => {
            line(&[(4.0, 20.0), (5.0, 16.5), (13.0, 8.5), (15.5, 11.0), (7.5, 19.0), (4.0, 20.0)]);
            line(&[(11.5, 7.0), (17.0, 12.5)]);
            dot(17.5, 6.5, 3.6);
        }
        Icon::Hand => {
            line(&[(8.0, 13.0), (8.0, 6.0)]);
            line(&[(11.0, 12.0), (11.0, 4.5)]);
            line(&[(14.0, 12.0), (14.0, 5.0)]);
            line(&[(17.0, 13.0), (17.0, 7.5)]);
            line(&[(8.0, 13.0), (8.0, 15.0), (5.0, 12.5)]);
            line(&[(17.0, 13.0), (17.0, 16.0), (14.5, 20.0), (9.5, 20.0), (5.0, 12.5)]);
        }
        Icon::SelectRect => {
            dashed(&[(4.0, 4.0), (20.0, 4.0), (20.0, 20.0), (4.0, 20.0), (4.0, 4.0)]);
        }
        Icon::Lasso => {
            let pts: Vec<(f32, f32)> = (0..=28).map(|i| {
                let a = i as f32 / 28.0 * TAU;
                (12.0 + 8.0 * a.cos(), 9.0 + 5.5 * a.sin())
            }).collect();
            dashed(&pts);
            line(&[(8.0, 13.5), (7.0, 17.0), (9.0, 20.5)]);
        }
        Icon::Shapes => {
            circle(8.5, 8.5, 5.0);
            rrect(11.0, 11.0, 20.5, 20.5, 1.0);
        }
        Icon::Rect => rrect(4.0, 6.0, 20.0, 18.0, 1.0),
        Icon::Mic => {
            rrect(9.0, 3.0, 15.0, 14.0, 3.0);
            painter.add(Shape::line(arc(12.0, 11.0, 6.0, 0.0, PI), st));
            line(&[(12.0, 17.0), (12.0, 21.0)]);
            line(&[(8.5, 21.0), (15.5, 21.0)]);
        }
        Icon::Speaker | Icon::SpeakerOff => {
            fill(&[(3.0, 9.0), (7.0, 9.0), (12.0, 4.5), (12.0, 19.5), (7.0, 15.0), (3.0, 15.0)]);
            if icon == Icon::Speaker {
                painter.add(Shape::line(arc(12.0, 12.0, 4.0, -0.9, 0.9), st));
                painter.add(Shape::line(arc(12.0, 12.0, 8.0, -0.9, 0.9), st));
            } else {
                line(&[(15.0, 9.0), (21.0, 15.0)]);
                line(&[(21.0, 9.0), (15.0, 15.0)]);
            }
        }
        Icon::Music => {
            line(&[(9.0, 18.0), (9.0, 5.0), (19.0, 3.0), (19.0, 16.0)]);
            dot(7.0, 18.0, 2.6);
            dot(17.0, 16.0, 2.6);
        }
        Icon::Record => dot(12.0, 12.0, 7.0),
        Icon::Stop => fill(&[(6.0, 6.0), (18.0, 6.0), (18.0, 18.0), (6.0, 18.0)]),
        Icon::Wand => {
            line(&[(4.0, 20.0), (15.0, 9.0)]);
            fill(&[(17.0, 2.5), (18.2, 5.8), (21.5, 7.0), (18.2, 8.2), (17.0, 11.5), (15.8, 8.2), (12.5, 7.0), (15.8, 5.8)]);
            dot(8.0, 6.0, 1.2);
            dot(20.0, 15.0, 1.2);
        }
        Icon::Grid => {
            rrect(4.0, 4.0, 20.0, 20.0, 1.0);
            line(&[(9.33, 4.0), (9.33, 20.0)]);
            line(&[(14.66, 4.0), (14.66, 20.0)]);
            line(&[(4.0, 9.33), (20.0, 9.33)]);
            line(&[(4.0, 14.66), (20.0, 14.66)]);
        }
        Icon::Symmetry => {
            dashed(&[(12.0, 2.0), (12.0, 22.0)]);
            line(&[(9.0, 6.0), (4.0, 12.0), (9.0, 18.0)]);
            line(&[(15.0, 6.0), (20.0, 12.0), (15.0, 18.0)]);
        }
        Icon::Image => {
            rrect(3.0, 5.0, 21.0, 19.0, 1.5);
            fill(&[(4.0, 18.0), (10.0, 11.0), (14.0, 16.0)]);
            fill(&[(11.0, 18.0), (16.0, 12.0), (20.0, 18.0)]);
            dot(16.5, 8.5, 1.8);
        }
        Icon::Film => {
            rrect(5.0, 3.0, 19.0, 21.0, 1.0);
            line(&[(8.5, 3.0), (8.5, 21.0)]);
            line(&[(15.5, 3.0), (15.5, 21.0)]);
            line(&[(8.5, 12.0), (15.5, 12.0)]);
        }
        Icon::RangeIn => {
            line(&[(9.0, 4.0), (6.0, 4.0), (6.0, 20.0), (9.0, 20.0)]);
            fill(&[(11.0, 7.0), (19.0, 12.0), (11.0, 17.0)]);
        }
        Icon::RangeOut => {
            line(&[(15.0, 4.0), (18.0, 4.0), (18.0, 20.0), (15.0, 20.0)]);
            fill(&[(5.0, 7.0), (13.0, 12.0), (5.0, 17.0)]);
        }
        Icon::PingPong => {
            line(&[(5.0, 9.0), (17.0, 9.0)]);
            arrow_head((21.0, 9.0), 0.0);
            line(&[(19.0, 15.0), (7.0, 15.0)]);
            arrow_head((3.0, 15.0), PI);
        }
        Icon::Ellipse => {
            let pts: Vec<Pos2> = (0..=32).map(|i| {
                let a = i as f32 / 32.0 * TAU;
                p(12.0 + 8.5 * a.cos(), 12.0 + 6.0 * a.sin())
            }).collect();
            painter.add(Shape::line(pts, st));
        }
        Icon::Line => {
            line(&[(5.0, 19.0), (19.0, 5.0)]);
            dot(5.0, 19.0, 2.2);
            dot(19.0, 5.0, 2.2);
        }
        Icon::More => {
            dot(5.0, 12.0, 1.9);
            dot(12.0, 12.0, 1.9);
            dot(19.0, 12.0, 1.9);
        }
        Icon::Menu => {
            line(&[(4.0, 7.0), (20.0, 7.0)]);
            line(&[(4.0, 12.0), (20.0, 12.0)]);
            line(&[(4.0, 17.0), (20.0, 17.0)]);
        }
        Icon::Undo | Icon::Redo => {
            let flip = icon == Icon::Redo;
            let fx = |x: f32| if flip { 24.0 - x } else { x };
            let pts = arc(12.0, 13.0, 7.0, -PI, 0.4);
            let pts: Vec<Pos2> = pts.into_iter().map(|q| if flip { Pos2::new(2.0 * rect.center().x - q.x, q.y) } else { q }).collect();
            painter.add(Shape::line(pts, st));
            arrow_head((fx(5.0), 15.5), if flip { -FRAC_PI_2 + 0.5 } else { FRAC_PI_2 + 0.6 });
        }
        Icon::Play => fill(&[(7.0, 4.5), (19.5, 12.0), (7.0, 19.5)]),
        Icon::Pause => {
            fill(&[(6.0, 5.0), (10.0, 5.0), (10.0, 19.0), (6.0, 19.0)]);
            fill(&[(14.0, 5.0), (18.0, 5.0), (18.0, 19.0), (14.0, 19.0)]);
        }
        Icon::Loop => {
            painter.add(Shape::line(arc(12.0, 12.0, 7.0, -PI * 0.15, PI * 1.6), st));
            arrow_head((18.8, 9.0), -PI * 0.45);
        }
        Icon::Onion => {
            painter.circle_stroke(p(8.0, 12.0), 5.5 * s, Stroke::new(w, color.gamma_multiply(0.35)));
            painter.circle_stroke(p(12.0, 12.0), 5.5 * s, Stroke::new(w, color.gamma_multiply(0.6)));
            circle(16.0, 12.0, 5.5);
        }
        Icon::RotateLeft | Icon::RotateRight => {
            let right = icon == Icon::RotateRight;
            let pts = arc(12.0, 13.0, 7.0, -PI * 0.9, PI * 0.9);
            let pts: Vec<Pos2> = pts.into_iter().map(|q| if right { Pos2::new(2.0 * rect.center().x - q.x, q.y) } else { q }).collect();
            painter.add(Shape::line(pts, st));
            if right {
                arrow_head((18.5, 9.0), -FRAC_PI_2 + 0.2);
            } else {
                arrow_head((5.5, 9.0), -FRAC_PI_2 - 0.2);
            }
        }
        Icon::FlipH => {
            dashed(&[(12.0, 3.0), (12.0, 21.0)]);
            fill(&[(10.0, 6.0), (10.0, 18.0), (3.0, 18.0)]);
            closed(&[(14.0, 6.0), (14.0, 18.0), (21.0, 18.0)]);
        }
        Icon::FlipV => {
            dashed(&[(3.0, 12.0), (21.0, 12.0)]);
            fill(&[(6.0, 10.0), (18.0, 10.0), (18.0, 3.0)]);
            closed(&[(6.0, 14.0), (18.0, 14.0), (18.0, 21.0)]);
        }
        Icon::Fit => {
            line(&[(4.0, 9.0), (4.0, 4.0), (9.0, 4.0)]);
            line(&[(15.0, 4.0), (20.0, 4.0), (20.0, 9.0)]);
            line(&[(20.0, 15.0), (20.0, 20.0), (15.0, 20.0)]);
            line(&[(9.0, 20.0), (4.0, 20.0), (4.0, 15.0)]);
        }
        Icon::Layers => {
            closed(&[(12.0, 3.5), (21.0, 8.0), (12.0, 12.5), (3.0, 8.0)]);
            line(&[(3.0, 12.0), (12.0, 16.5), (21.0, 12.0)]);
            line(&[(3.0, 16.0), (12.0, 20.5), (21.0, 16.0)]);
        }
        Icon::Brush => {
            line(&[(20.0, 4.0), (11.0, 13.0)]);
            fill(&[(10.0, 11.5), (12.5, 14.0), (9.0, 19.5), (4.0, 20.0), (5.0, 15.0)]);
        }
        Icon::Palette => {
            painter.add(Shape::closed_line(arc(12.0, 12.0, 8.5, 0.0, TAU), st));
            dot(8.0, 9.0, 1.6);
            dot(12.5, 7.5, 1.6);
            dot(16.5, 10.0, 1.6);
            dot(14.0, 16.0, 2.2);
        }
        Icon::Save => {
            closed(&[(4.0, 4.0), (17.0, 4.0), (20.0, 7.0), (20.0, 20.0), (4.0, 20.0)]);
            rrect(8.0, 4.0, 15.0, 9.0, 0.5);
            rrect(7.0, 13.0, 17.0, 20.0, 0.5);
        }
        Icon::Open => {
            closed(&[(3.0, 6.0), (9.0, 6.0), (11.0, 8.0), (20.0, 8.0), (20.0, 19.0), (3.0, 19.0)]);
            line(&[(3.0, 11.0), (20.0, 11.0)]);
        }
        Icon::New => {
            closed(&[(5.0, 3.0), (14.0, 3.0), (19.0, 8.0), (19.0, 21.0), (5.0, 21.0)]);
            line(&[(12.0, 10.0), (12.0, 17.0)]);
            line(&[(8.5, 13.5), (15.5, 13.5)]);
        }
        Icon::Settings => {
            circle(12.0, 12.0, 3.0);
            for i in 0..8 {
                let a = i as f32 / 8.0 * TAU;
                line(&[(12.0 + 5.5 * a.cos(), 12.0 + 5.5 * a.sin()), (12.0 + 8.5 * a.cos(), 12.0 + 8.5 * a.sin())]);
            }
            painter.add(Shape::closed_line(arc(12.0, 12.0, 6.0, 0.0, TAU), st));
        }
        Icon::First => {
            line(&[(6.0, 5.0), (6.0, 19.0)]);
            fill(&[(19.0, 5.0), (19.0, 19.0), (8.0, 12.0)]);
        }
        Icon::Prev => fill(&[(17.0, 5.0), (17.0, 19.0), (6.0, 12.0)]),
        Icon::Next => fill(&[(7.0, 5.0), (7.0, 19.0), (18.0, 12.0)]),
        Icon::Last => {
            line(&[(18.0, 5.0), (18.0, 19.0)]);
            fill(&[(5.0, 5.0), (5.0, 19.0), (16.0, 12.0)]);
        }
        Icon::Plus => {
            line(&[(12.0, 5.0), (12.0, 19.0)]);
            line(&[(5.0, 12.0), (19.0, 12.0)]);
        }
        Icon::Minus => line(&[(5.0, 12.0), (19.0, 12.0)]),
        Icon::Duplicate | Icon::Copy => {
            rrect(8.0, 8.0, 20.0, 20.0, 1.5);
            line(&[(5.0, 16.0), (4.0, 16.0), (4.0, 4.0), (16.0, 4.0), (16.0, 5.0)]);
            if icon == Icon::Duplicate {
                line(&[(14.0, 11.0), (14.0, 17.0)]);
                line(&[(11.0, 14.0), (17.0, 14.0)]);
            }
        }
        Icon::Paste => {
            rrect(5.0, 5.0, 19.0, 21.0, 1.5);
            fill(&[(9.0, 3.0), (15.0, 3.0), (15.0, 7.0), (9.0, 7.0)]);
            line(&[(8.5, 12.0), (15.5, 12.0)]);
            line(&[(8.5, 16.0), (13.5, 16.0)]);
        }
        Icon::Cut => {
            circle(7.0, 17.0, 3.0);
            circle(17.0, 17.0, 3.0);
            line(&[(9.0, 15.0), (17.0, 4.0)]);
            line(&[(15.0, 15.0), (7.0, 4.0)]);
        }
        Icon::Trash => {
            line(&[(4.0, 6.5), (20.0, 6.5)]);
            line(&[(9.0, 6.5), (9.5, 3.5), (14.5, 3.5), (15.0, 6.5)]);
            closed(&[(6.0, 6.5), (18.0, 6.5), (16.8, 21.0), (7.2, 21.0)]);
            line(&[(10.0, 10.0), (10.3, 17.5)]);
            line(&[(14.0, 10.0), (13.7, 17.5)]);
        }
        Icon::ArrowLeft => {
            line(&[(20.0, 12.0), (8.0, 12.0)]);
            arrow_head((4.0, 12.0), PI);
        }
        Icon::ArrowRight => {
            line(&[(4.0, 12.0), (16.0, 12.0)]);
            arrow_head((20.0, 12.0), 0.0);
        }
        Icon::Eye | Icon::EyeOff => {
            let top = arc(12.0, 19.0, 11.0, -PI * 0.82, -PI * 0.18);
            let bot = arc(12.0, 5.0, 11.0, PI * 0.18, PI * 0.82);
            painter.add(Shape::line(top, st));
            painter.add(Shape::line(bot, st));
            dot(12.0, 12.0, 3.0);
            if icon == Icon::EyeOff {
                painter.line_segment([p(4.0, 20.0), p(20.0, 4.0)], Stroke::new(w * 1.4, color));
            }
        }
        Icon::Lock | Icon::Unlock => {
            fill(&[(5.0, 11.0), (19.0, 11.0), (19.0, 21.0), (5.0, 21.0)]);
            let (a0, a1, cx) = if icon == Icon::Lock { (PI, TAU, 12.0) } else { (PI, TAU, 15.5) };
            let mut pts = vec![p(cx - 4.5, 11.0)];
            pts.extend(arc(cx, 8.0, 4.5, a0, a1));
            if icon == Icon::Lock {
                pts.push(p(cx + 4.5, 11.0));
            }
            painter.add(Shape::line(pts, st));
        }
        Icon::Up => line(&[(5.0, 15.0), (12.0, 8.0), (19.0, 15.0)]),
        Icon::Down => line(&[(5.0, 9.0), (12.0, 16.0), (19.0, 9.0)]),
        Icon::Merge => {
            line(&[(12.0, 3.0), (12.0, 13.0)]);
            arrow_head((12.0, 17.0), FRAC_PI_2);
            line(&[(4.0, 20.5), (20.0, 20.5)]);
        }
        Icon::Clear => {
            rrect(4.0, 4.0, 20.0, 20.0, 2.0);
            line(&[(8.0, 16.0), (16.0, 8.0)]);
            dot(17.5, 6.5, 1.3);
            dot(6.5, 17.5, 1.3);
        }
        Icon::Import => {
            line(&[(12.0, 3.0), (12.0, 12.0)]);
            arrow_head((12.0, 16.0), FRAC_PI_2);
            line(&[(4.0, 15.0), (4.0, 20.5), (20.0, 20.5), (20.0, 15.0)]);
        }
        Icon::Export => {
            line(&[(12.0, 16.0), (12.0, 8.0)]);
            arrow_head((12.0, 3.5), -FRAC_PI_2);
            line(&[(4.0, 15.0), (4.0, 20.5), (20.0, 20.5), (20.0, 15.0)]);
        }
        Icon::Globe => {
            circle(12.0, 12.0, 8.5);
            line(&[(3.5, 12.0), (20.5, 12.0)]);
            line(&[(5.0, 7.5), (19.0, 7.5)]);
            line(&[(5.0, 16.5), (19.0, 16.5)]);
            let meridian: Vec<(f32, f32)> = (0..=16).map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 / 16.0 * std::f32::consts::PI;
                (12.0 + 3.8 * a.cos(), 12.0 + 8.5 * a.sin())
            }).collect();
            line(&meridian);
            let back: Vec<(f32, f32)> = meridian.iter().map(|&(x, y)| (24.0 - x, y)).collect();
            line(&back);
        }
        Icon::Search => {
            circle(11.0, 11.0, 7.0);
            line(&[(16.0, 16.0), (20.5, 20.5)]);
        }
        Icon::Clock => {
            circle(12.0, 12.0, 8.5);
            line(&[(12.0, 7.0), (12.0, 12.0), (15.5, 14.5)]);
        }
        Icon::Camera => {
            rrect(3.0, 7.0, 16.0, 18.0, 2.0);
            line(&[(16.0, 11.0), (21.0, 8.0), (21.0, 17.0), (16.0, 14.0)]);
            circle(7.0, 5.0, 2.0);
            circle(12.5, 4.5, 2.5);
        }
        Icon::Text => {
            line(&[(5.0, 7.0), (5.0, 4.5), (19.0, 4.5), (19.0, 7.0)]);
            line(&[(12.0, 4.5), (12.0, 19.5)]);
            line(&[(9.0, 19.5), (15.0, 19.5)]);
        }
        Icon::Tween => {
            dot(5.0, 17.0, 1.6);
            dot(9.5, 13.5, 2.0);
            circle(15.0, 10.0, 2.6);
            rrect(16.5, 3.5, 22.0, 9.0, 1.0);
            line(&[(3.0, 21.0), (21.0, 3.5)]);
        }
        Icon::Check => line(&[(4.5, 12.5), (9.5, 17.5), (19.5, 6.5)]),
        Icon::Close => {
            line(&[(6.0, 6.0), (18.0, 18.0)]);
            line(&[(18.0, 6.0), (6.0, 18.0)]);
        }
        Icon::Background => {
            rrect(3.0, 4.0, 21.0, 20.0, 1.5);
            fill(&[(4.0, 19.0), (10.0, 11.0), (14.0, 16.0)]);
            fill(&[(11.0, 19.0), (16.0, 12.0), (20.0, 19.0)]);
            dot(16.5, 8.0, 1.8);
        }
    }
}

/// Square button showing an icon (optionally with a text label to its right).
pub struct IconButton<'a> {
    icon: Icon,
    text: Option<&'a str>,
    size: f32,
    selected: bool,
    /// Show the text even on phones (menus, names, risky actions).
    keep_text: bool,
}

impl<'a> IconButton<'a> {
    pub fn new(icon: Icon) -> Self {
        Self { icon, text: None, size: 30.0, selected: false, keep_text: false }
    }

    pub fn text(mut self, text: &'a str) -> Self {
        self.text = Some(text);
        self
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Keep the text on small screens too.
    pub fn keep_text(mut self) -> Self {
        self.keep_text = true;
        self
    }

}

impl Widget for IconButton<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        use crate::theme::{ACCENT, ELEVATED, ON_ACCENT, SEPARATOR};
        let font = egui::FontId::proportional(15.0);
        // On phones an icon is enough: the text only takes room. The
        // hover text still names it (long-press on touch).
        let compact = {
            let r = ui.ctx().content_rect();
            r.width() < 600.0 || r.height() < 520.0
        };
        let tip = self.text.filter(|_| compact && !self.keep_text).map(str::to_string);
        let text = if compact && !self.keep_text { None } else { self.text };
        let self_text = text;
        let galley = self_text.map(|t| ui.painter().layout_no_wrap(t.to_string(), font, Color32::WHITE));
        let text_w = galley.as_ref().map(|g| g.size().x + 12.0).unwrap_or(0.0);
        let size = Vec2::new(self.size + text_w, self.size);
        let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
        if ui.is_rect_visible(rect) {
            let enabled = ui.is_enabled();
            let visuals = ui.style().interact_selectable(&resp, self.selected);
            let radius = (self.size * 0.3).clamp(6.0, 14.0);
            let pressed = resp.is_pointer_button_down_on();
            let mut color = visuals.fg_stroke.color;
            if self.selected {
                ui.painter().rect_filled(rect, radius, ACCENT);
                color = ON_ACCENT;
            } else if pressed || resp.hovered() {
                ui.painter().rect_filled(rect, radius, SEPARATOR);
            } else if self_text.is_some() {
                ui.painter().rect_filled(rect, radius, ELEVATED);
            }
            if !enabled {
                color = color.gamma_multiply(0.35);
            }
            let icon_rect = Rect::from_min_size(rect.min, Vec2::splat(self.size)).shrink(self.size * 0.2);
            paint(ui.painter(), icon_rect, self.icon, color);
            if let Some(g) = galley {
                let pos = Pos2::new(rect.min.x + self.size - 2.0, rect.center().y - g.size().y / 2.0);
                ui.painter().galley(pos, g, color);
            }
        }
        match tip {
            Some(t) => resp.on_hover_text(t),
            None => resp,
        }
    }
}
