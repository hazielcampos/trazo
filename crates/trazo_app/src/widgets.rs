//! Shared building blocks of the interface: size classes, sheets and the
//! controls every panel uses (tool button, switch, segmented control,
//! vertical slider). One look everywhere, sized for fingers.

use crate::i18n::tr;
use egui::{Align2, Color32, CornerRadius, Id, Order, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

use crate::icons::{paint, Icon};
use crate::theme::{self, ACCENT, CONTROL, ELEVATED, ON_ACCENT, SEPARATOR, TEXT, TEXT2};

/// Which layout the screen gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeClass {
    /// Phone held upright (narrower than 600 pt).
    Phone,
    /// Phone on its side (short screen).
    PhoneLandscape,
    /// Tablet or desktop.
    Wide,
}

impl SizeClass {
    pub fn of(ctx: &egui::Context) -> Self {
        let r = ctx.content_rect();
        if r.width() < 600.0 {
            SizeClass::Phone
        } else if r.height() < 520.0 {
            SizeClass::PhoneLandscape
        } else {
            SizeClass::Wide
        }
    }
}

/// Width for a dialog window: its preferred width, but never wider than the screen.
pub fn dialog_width(ctx: &egui::Context, preferred: f32) -> f32 {
    preferred.min(ctx.content_rect().width() - 24.0).max(200.0)
}

/// Safe insets on Android (status bar on top, gesture bar below).
pub fn insets(touch: bool) -> (f32, f32) {
    if touch {
        (26.0, 22.0)
    } else {
        (0.0, 0.0)
    }
}

/// Square tool button: accent fill when selected.
pub fn tool_button(ui: &mut Ui, icon: Icon, selected: bool, size: Vec2, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let fg = if selected {
            p.rect_filled(rect, 14.0, ACCENT);
            ON_ACCENT
        } else {
            if resp.hovered() || resp.is_pointer_button_down_on() {
                p.rect_filled(rect, 14.0, ELEVATED);
            }
            if ui.is_enabled() { TEXT } else { TEXT2.gamma_multiply(0.5) }
        };
        let s = 22.0f32.min(size.y - 12.0);
        paint(p, Rect::from_center_size(rect.center(), Vec2::splat(s)), icon, fg);
    }
    resp.on_hover_text(tip)
}

/// iOS-style switch. Returns true when toggled.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(46.0, 28.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let p = ui.painter();
    p.rect_filled(rect, 14.0, lerp_color(CONTROL, ACCENT, t));
    let x = egui::lerp(rect.left() + 14.0..=rect.right() - 14.0, t);
    let knob = Pos2::new(x, rect.center().y);
    p.circle_filled(knob, 11.0, lerp_color(TEXT2, ON_ACCENT, t));
    if t > 0.5 {
        paint(p, Rect::from_center_size(knob, Vec2::splat(13.0)), Icon::Check, ACCENT);
    }
    resp
}

/// A list row with a label on the left and a switch on the right.
pub fn switch_row(ui: &mut Ui, label: &str, on: &mut bool) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_min_height(46.0);
        ui.add_space(4.0);
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            changed = switch(ui, on).changed();
        });
    });
    changed
}

/// Segmented control. Returns true when the selection changed.
pub fn segmented(ui: &mut Ui, selected: &mut usize, labels: &[&str], width: f32) -> bool {
    let n = labels.len().max(1);
    let w = width.min(ui.available_width()).max(60.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 44.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 12.0, Color32::from_rgb(17, 17, 19));
    let seg_w = (w - 6.0) / n as f32;
    let mut changed = false;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(rect.left() + 3.0 + seg_w * i as f32, rect.top() + 3.0), Vec2::new(seg_w, 38.0));
        let resp = ui.interact(r, ui.scope_id().with(("seg", i, *label)), Sense::click());
        let on = *selected == i;
        if on {
            p.rect_filled(r, 10.0, SEPARATOR);
        }
        let font = theme::font(14.0, if on { theme::W::Semi } else { theme::W::Regular });
        p.text(r.center(), Align2::CENTER_CENTER, *label, font, if on { TEXT } else { TEXT2 });
        if resp.clicked() && !on {
            *selected = i;
            changed = true;
        }
    }
    changed
}

/// Slim vertical slider for floating rails (brush size, opacity).
pub fn vslider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>, height: f32, logarithmic: bool, tip: &str) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(36.0, height), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let to_t = |v: f32| if logarithmic { (v.max(lo).ln() - lo.ln()) / (hi.ln() - lo.ln()) } else { (v - lo) / (hi - lo) };
    let from_t = |t: f32| if logarithmic { (lo.ln() + t * (hi.ln() - lo.ln())).exp() } else { lo + t * (hi - lo) };
    let rail = Rect::from_center_size(rect.center(), Vec2::new(5.0, height - 24.0));
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            let t = ((rail.bottom() - p.y) / rail.height()).clamp(0.0, 1.0);
            *value = from_t(t);
            resp.mark_changed();
        }
    }
    let t = to_t(*value).clamp(0.0, 1.0);
    let painter = ui.painter();
    painter.rect_filled(rail, 3.0, SEPARATOR);
    let y = rail.bottom() - t * rail.height();
    painter.rect_filled(Rect::from_min_max(Pos2::new(rail.left(), y), rail.max), 3.0, TEXT);
    painter.circle_filled(Pos2::new(rail.center().x, y + 1.5), 11.5, Color32::from_black_alpha(90));
    painter.circle_filled(Pos2::new(rail.center().x, y), 11.0, TEXT);
    resp.on_hover_text(tip)
}

/// Where a panel opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SheetEdge {
    /// Slides up from the bottom, dims the canvas (phone upright).
    Bottom,
    /// Docked on the right, canvas stays usable (phone on its side).
    Right,
}

/// A panel shown as a sheet. Returns true when the user closed it.
pub fn sheet(ctx: &egui::Context, id: &str, edge: SheetEdge, touch: bool, header: impl FnOnce(&mut Ui), add: impl FnOnce(&mut Ui)) -> bool {
    let screen = ctx.content_rect();
    let (top_inset, _) = insets(touch);
    let id = Id::unique(("sheet", id));
    let mut close = false;
    let rect = match edge {
        SheetEdge::Bottom => {
            let h = (screen.height() * 0.78).min(screen.height() - top_inset - 40.0);
            // Dim and close on tap outside.
            egui::Area::new(id.with("dim")).order(Order::Middle).fixed_pos(screen.min).show(ctx, |ui| {
                let r = ui.allocate_rect(screen, Sense::click());
                ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(110));
                if r.clicked() {
                    close = true;
                }
            });
            Rect::from_min_max(Pos2::new(screen.left(), screen.bottom() - h), screen.max)
        }
        SheetEdge::Right => {
            let w = 340.0f32.min(screen.width() * 0.5);
            Rect::from_min_max(Pos2::new(screen.right() - w, screen.top() + top_inset), screen.max)
        }
    };
    let radius = match edge {
        SheetEdge::Bottom => CornerRadius { nw: theme::R_SHEET, ne: theme::R_SHEET, sw: 0, se: 0 },
        SheetEdge::Right => CornerRadius::ZERO,
    };
    egui::Area::new(id).order(Order::Foreground).fixed_pos(rect.min).show(ctx, |ui| {
        let frame = egui::Frame::new()
            .fill(theme::SHEET)
            .corner_radius(radius)
            .shadow(egui::Shadow { offset: [0, -10], blur: 40, spread: 0, color: Color32::from_black_alpha(140) })
            .inner_margin(egui::Margin { left: 8, right: 8, top: 8, bottom: if touch { 26 } else { 12 } });
        let bottom_pad = if touch { 26.0 } else { 12.0 };
        frame.show(ui, |ui| {
            ui.set_width(rect.width() - 16.0);
            ui.set_height(rect.height() - 8.0 - bottom_pad);
            if edge == SheetEdge::Bottom {
                let (g, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), Sense::hover());
                ui.painter().rect_filled(Rect::from_center_size(g.center(), Vec2::new(36.0, 5.0)), 3.0, CONTROL);
            }
            ui.horizontal(|ui| {
                ui.set_min_height(44.0);
                ui.add_space(12.0);
                header(ui);
                if edge == SheetEdge::Right {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::widgets::icon_button(ui, Icon::Close, tr("Cerrar")).clicked() {
                            close = true;
                        }
                    });
                }
            });
            egui::ScrollArea::vertical().id_salt(id.with("scroll")).auto_shrink([false, false]).show(ui, add);
        });
    });
    close
}

/// Round the corners of something drawn as a mesh by painting the
/// background over them.
fn round_corners(p: &egui::Painter, r: Rect, rad: f32, bg: Color32) {
    // A thick rounded stroke whose inner edge is the rounded rect, clipped
    // to the rect: only the corners get covered, with no seams.
    let rad = rad.min(r.height() / 2.0).min(r.width() / 2.0);
    let w = rad * 2.0;
    p.with_clip_rect(r).rect_stroke(r.expand(w / 2.0), CornerRadius::same((rad + w / 2.0).round() as u8), Stroke::new(w, bg), egui::StrokeKind::Middle);
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

/// Big touch color picker: saturation/brightness square and a hue bar.
/// Returns true when the color changed.
pub fn hsv_picker(ui: &mut Ui, color: &mut Color32) -> bool {
    use egui::ecolor::Hsva;
    let id = ui.make_persistent_id("hsv_picker");
    // Keep the hue while the color is grey or black (it can't be recovered from RGB).
    let mut hsv = ui.data(|d| d.get_temp::<[f32; 3]>(id)).filter(|h| Color32::from(Hsva::new(h[0], h[1], h[2], 1.0)) == *color).unwrap_or_else(|| {
        let h = Hsva::from(*color);
        [h.h, h.s, h.v]
    });
    let w = ui.available_width().min(420.0);
    let mut changed = false;

    // Saturation (x) / brightness (y) square.
    let (sq, resp) = ui.allocate_exact_size(Vec2::new(w, (w * 0.5).min(170.0)), Sense::click_and_drag());
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            hsv[1] = ((p.x - sq.left()) / sq.width()).clamp(0.0, 1.0);
            hsv[2] = 1.0 - ((p.y - sq.top()) / sq.height()).clamp(0.0, 1.0);
            changed = true;
        }
    }
    let n = 24;
    let mut mesh = egui::Mesh::default();
    for j in 0..=n {
        for i in 0..=n {
            let (s, v) = (i as f32 / n as f32, 1.0 - j as f32 / n as f32);
            let pos = Pos2::new(sq.left() + sq.width() * i as f32 / n as f32, sq.top() + sq.height() * j as f32 / n as f32);
            mesh.colored_vertex(pos, Color32::from(Hsva::new(hsv[0], s, v, 1.0)));
        }
    }
    for j in 0..n {
        for i in 0..n {
            let a = (j * (n + 1) + i) as u32;
            mesh.add_triangle(a, a + 1, a + n as u32 + 1);
            mesh.add_triangle(a + 1, a + n as u32 + 2, a + n as u32 + 1);
        }
    }
    let painter = ui.painter();
    painter.add(egui::Shape::mesh(mesh));
    round_corners(painter, sq, 14.0, theme::SURFACE);
    let inner = sq.shrink(13.0);
    let knob = Pos2::new(sq.left() + hsv[1] * sq.width(), sq.top() + (1.0 - hsv[2]) * sq.height()).clamp(inner.min, inner.max);
    painter.circle(knob, 11.5, Color32::from(Hsva::new(hsv[0], hsv[1], hsv[2], 1.0)), Stroke::new(3.0, Color32::WHITE));

    ui.add_space((12.0 - ui.spacing().item_spacing.y).max(0.0));
    // Hue bar.
    let (bar, resp) = ui.allocate_exact_size(Vec2::new(w, 20.0), Sense::click_and_drag());
    let bar_draw = bar;
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            hsv[0] = ((p.x - bar.left() - 15.0) / (bar.width() - 30.0)).clamp(0.0, 0.9999);
            changed = true;
        }
    }
    let mut mesh = egui::Mesh::default();
    let m = 36;
    for i in 0..=m {
        let t = i as f32 / m as f32;
        let c = Color32::from(Hsva::new(t, 1.0, 1.0, 1.0));
        let x = bar_draw.left() + bar_draw.width() * t;
        mesh.colored_vertex(Pos2::new(x, bar_draw.top()), c);
        mesh.colored_vertex(Pos2::new(x, bar_draw.bottom()), c);
    }
    for i in 0..m as u32 {
        let a = i * 2;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    let painter = ui.painter();
    painter.add(egui::Shape::mesh(mesh));
    round_corners(painter, bar_draw, 10.0, theme::SURFACE);
    let x = bar.left() + 15.0 + hsv[0] * (bar.width() - 30.0);
    painter.circle(Pos2::new(x, bar.center().y), 13.5, Color32::from(Hsva::new(hsv[0], 1.0, 1.0, 1.0)), Stroke::new(3.0, Color32::WHITE));

    if changed {
        *color = Color32::from(Hsva::new(hsv[0], hsv[1], hsv[2], 1.0));
    }
    ui.data_mut(|d| d.insert_temp(id, hsv));
    changed
}

/// A dialog window: always in front, centered, never wider or taller than
/// the screen, contents scroll when needed.
pub fn dialog(ctx: &egui::Context, title: &str, width: f32) -> egui::Window<'static> {
    let screen = ctx.content_rect();
    let w = dialog_width(ctx, width);
    egui::Window::new(title.to_string())
        .order(Order::Foreground)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(w)
        .min_width(w.min(280.0))
        .max_width(w)
        .max_height(screen.height() - 80.0)
        .min_height((screen.height() - 140.0).clamp(200.0, 600.0))
        .vscroll(false)
}

/// Dims everything behind a dialog (and swallows taps on it).
pub fn backdrop(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    egui::Area::new(Id::unique("modal_backdrop")).order(Order::Middle).fixed_pos(screen.min).interactable(true).show(ctx, |ui| {
        ui.allocate_rect(screen, Sense::click_and_drag());
        ui.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(140));
    });
}

/// A full-width choice row (radio-like) with a check mark when selected.
pub fn choice_row(ui: &mut Ui, selected: bool, label: &str, detail: &str) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, 48.0), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, theme::R_ROW, if selected { SEPARATOR } else { ELEVATED });
    if selected {
        p.rect_stroke(rect, theme::R_ROW, Stroke::new(2.0, TEXT), egui::StrokeKind::Inside);
    }
    p.text(Pos2::new(rect.left() + 14.0, rect.center().y), Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), TEXT);
    let right = if selected { rect.right() - 40.0 } else { rect.right() - 14.0 };
    p.text(Pos2::new(right, rect.center().y), Align2::RIGHT_CENTER, detail, egui::FontId::proportional(13.0), TEXT2);
    if selected {
        paint(p, Rect::from_center_size(Pos2::new(rect.right() - 22.0, rect.center().y), Vec2::splat(18.0)), Icon::Check, TEXT);
    }
    resp
}

/// Main action button (white, full width): one per screen.
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::click());
    let fill = if resp.is_pointer_button_down_on() { theme::PRIMARY.gamma_multiply(0.85) } else { theme::PRIMARY };
    ui.painter().rect_filled(rect, theme::R_ROW, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, theme::font(16.0, theme::W::Bold), ON_ACCENT);
    resp
}

/// Secondary button (graphite).
pub fn secondary_button(ui: &mut Ui, label: &str, height: f32) -> Response {
    secondary_button_icon(ui, None, label, height)
}

pub fn secondary_button_icon(ui: &mut Ui, icon: Option<Icon>, label: &str, height: f32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let fill = if resp.hovered() || resp.is_pointer_button_down_on() { SEPARATOR } else { ELEVATED };
    let p = ui.painter();
    p.rect_filled(rect, theme::R_CONTROL, fill);
    let g = p.layout_no_wrap(label.to_string(), theme::font(14.0, theme::W::Semi), TEXT);
    let iw = if icon.is_some() { 26.0 } else { 0.0 };
    let x = rect.center().x - (g.size().x + iw) / 2.0;
    if let Some(ic) = icon {
        crate::icons::paint(p, Rect::from_center_size(Pos2::new(x + 9.0, rect.center().y), Vec2::splat(18.0)), ic, TEXT);
    }
    p.galley(Pos2::new(x + iw, rect.center().y - g.size().y / 2.0), g, TEXT);
    resp
}

/// Plain 44 pt icon button (graphite on hover).
pub fn icon_button(ui: &mut Ui, icon: Icon, tip: &str) -> Response {
    icon_button_ex(ui, icon, tip, true, None)
}

/// Icon button with an optional resting fill; disabled ones are dimmed.
pub fn icon_button_ex(ui: &mut Ui, icon: Icon, tip: &str, enabled: bool, fill: Option<Color32>) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(theme::TOUCH), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let hover = enabled && (resp.hovered() || resp.is_pointer_button_down_on());
    match (fill, hover) {
        (_, true) => {
            p.rect_filled(rect, theme::R_CONTROL, SEPARATOR);
        }
        (Some(f), false) => {
            p.rect_filled(rect, theme::R_CONTROL, f);
        }
        _ => {}
    }
    paint(p, Rect::from_center_size(rect.center(), Vec2::splat(22.0)), icon, if enabled { TEXT } else { theme::TEXT3 });
    resp.on_hover_text(tip)
}

/// An option that is on or off: lime icon and a dot when on.
pub fn toggle_icon(ui: &mut Ui, icon: Icon, on: bool, tip: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(theme::TOUCH), Sense::click());
    let p = ui.painter();
    if on || resp.hovered() {
        p.rect_filled(rect, theme::R_CONTROL, if on { theme::accent_soft() } else { SEPARATOR });
    }
    paint(p, Rect::from_center_size(rect.center() - Vec2::new(0.0, if on { 2.0 } else { 0.0 }), Vec2::splat(22.0)), icon, if on { ACCENT } else { TEXT2 });
    if on {
        p.circle_filled(Pos2::new(rect.center().x, rect.bottom() - 7.0), 3.0, ACCENT);
    }
    resp.on_hover_text(tip)
}

/// A labelled pill button (icon + text) on graphite, e.g. "Capas · Tinta".
pub fn pill_button(ui: &mut Ui, icon: Icon, label: &str, icon_color: Color32, dot: bool) -> Response {
    let galley = ui.painter().layout_no_wrap(label.to_string(), theme::font(14.0, theme::W::Semi), TEXT);
    let w = 10.0 + 20.0 + 6.0 + galley.size().x + if dot { 12.0 } else { 0.0 } + 12.0;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, theme::TOUCH), Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, theme::R_CONTROL, if resp.hovered() || resp.is_pointer_button_down_on() { SEPARATOR } else { ELEVATED });
    paint(p, Rect::from_center_size(Pos2::new(rect.left() + 20.0, rect.center().y), Vec2::splat(20.0)), icon, icon_color);
    let x = rect.left() + 36.0;
    p.galley(Pos2::new(x, rect.center().y - galley.size().y / 2.0), galley.clone(), TEXT);
    if dot {
        p.circle_filled(Pos2::new(x + galley.size().x + 8.0, rect.center().y), 3.0, ACCENT);
    }
    resp
}

/// Labelled horizontal slider: label, white rail with a knob, value.
pub fn hslider(ui: &mut Ui, label: &str, value: &mut f32, range: std::ops::RangeInclusive<f32>, logarithmic: bool, fmt: impl Fn(f32) -> String) -> Response {
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let to_t = |v: f32| if logarithmic { (v.max(lo).ln() - lo.ln()) / (hi.ln() - lo.ln()) } else { (v - lo) / (hi - lo) };
    let from_t = |t: f32| if logarithmic { (lo.ln() + t * (hi.ln() - lo.ln())).exp() } else { lo + t * (hi - lo) };
    let rail = Rect::from_min_max(Pos2::new(rect.left() + 86.0, rect.center().y - 3.0), Pos2::new(rect.right() - 62.0, rect.center().y + 3.0));
    if let Some(p) = resp.interact_pointer_pos() {
        if (resp.dragged() || resp.clicked()) && p.x >= rail.left() - 12.0 {
            *value = from_t(((p.x - rail.left()) / rail.width()).clamp(0.0, 1.0));
            resp.mark_changed();
        }
    }
    let t = to_t(*value).clamp(0.0, 1.0);
    let p = ui.painter();
    p.text(Pos2::new(rect.left(), rect.center().y), Align2::LEFT_CENTER, label, theme::font(13.0, theme::W::Regular), TEXT2);
    p.rect_filled(rail, 3.0, SEPARATOR);
    let x = rail.left() + t * rail.width();
    p.rect_filled(Rect::from_min_max(rail.min, Pos2::new(x, rail.bottom())), 3.0, TEXT);
    p.circle_filled(Pos2::new(x, rail.center().y + 1.5), 11.5, Color32::from_black_alpha(90));
    p.circle_filled(Pos2::new(x, rail.center().y), 11.0, TEXT);
    p.text(Pos2::new(rect.right(), rect.center().y), Align2::RIGHT_CENTER, fmt(*value), theme::font(14.0, theme::W::Semi), TEXT);
    resp
}
