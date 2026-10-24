//! Shared building blocks of the interface: size classes, sheets and the
//! controls every panel uses (tool button, switch, segmented control,
//! vertical slider). One look everywhere, sized for fingers.

use egui::{Align2, Color32, CornerRadius, Id, Order, Pos2, Rect, Response, RichText, Sense, Stroke, Ui, Vec2};

use crate::icons::{paint, Icon};
use crate::theme::{self, ACCENT, CONTROL, ELEVATED, ON_ACCENT, SURFACE, TEXT, TEXT2};

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
    let (rect, mut resp) = ui.allocate_exact_size(Vec2::new(51.0, 31.0), Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool(resp.id, *on);
    let p = ui.painter();
    let bg = lerp_color(CONTROL, ACCENT, t);
    p.rect_filled(rect, 15.5, bg);
    let x = egui::lerp(rect.left() + 15.5..=rect.right() - 15.5, t);
    p.circle_filled(Pos2::new(x, rect.center().y), 13.5, Color32::WHITE);
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
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 36.0), Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, 10.0, ELEVATED);
    let seg_w = (w - 4.0) / n as f32;
    let mut changed = false;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::from_min_size(Pos2::new(rect.left() + 2.0 + seg_w * i as f32, rect.top() + 2.0), Vec2::new(seg_w, 32.0));
        let resp = ui.interact(r, ui.scope_id().with(("seg", i, *label)), Sense::click());
        let on = *selected == i;
        if on {
            p.rect_filled(r, 8.0, CONTROL);
        }
        let font = egui::FontId::proportional(13.0);
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
    let rail = Rect::from_center_size(rect.center(), Vec2::new(6.0, height - 24.0));
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            let t = ((rail.bottom() - p.y) / rail.height()).clamp(0.0, 1.0);
            *value = from_t(t);
            resp.mark_changed();
        }
    }
    let t = to_t(*value).clamp(0.0, 1.0);
    let painter = ui.painter();
    painter.rect_filled(rail, 3.0, CONTROL);
    let y = rail.bottom() - t * rail.height();
    painter.rect_filled(Rect::from_min_max(Pos2::new(rail.left(), y), rail.max), 3.0, TEXT2);
    painter.circle_filled(Pos2::new(rail.center().x, y), 12.0, TEXT);
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
            .fill(SURFACE)
            .corner_radius(radius)
            .stroke(Stroke::new(1.0, theme::ELEVATED))
            .shadow(egui::Shadow { offset: [0, -6], blur: 30, spread: 0, color: Color32::from_black_alpha(130) })
            .inner_margin(egui::Margin { left: 16, right: 12, top: 8, bottom: if touch { 26 } else { 12 } });
        frame.show(ui, |ui| {
            ui.set_width(rect.width() - 28.0);
            ui.set_height(rect.height() - 42.0);
            if edge == SheetEdge::Bottom {
                let (g, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 10.0), Sense::hover());
                ui.painter().rect_filled(Rect::from_center_size(g.center(), Vec2::new(36.0, 5.0)), 3.0, CONTROL);
            }
            ui.horizontal(|ui| {
                ui.set_min_height(44.0);
                header(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if edge == SheetEdge::Bottom { "Listo" } else { "Cerrar" };
                    if ui.add(egui::Button::new(RichText::new(label).color(ACCENT).strong()).frame(false).min_size(Vec2::new(56.0, 44.0))).clicked() {
                        close = true;
                    }
                });
            });
            egui::ScrollArea::vertical().id_salt(id.with("scroll")).auto_shrink([false, false]).show(ui, add);
        });
    });
    close
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
    let (sq, resp) = ui.allocate_exact_size(Vec2::new(w, (w * 0.62).min(260.0)), Sense::click_and_drag());
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
    let knob = Pos2::new(sq.left() + hsv[1] * sq.width(), sq.top() + (1.0 - hsv[2]) * sq.height());
    painter.circle(knob, 11.0, Color32::from(Hsva::new(hsv[0], hsv[1], hsv[2], 1.0)), Stroke::new(3.0, Color32::WHITE));

    ui.add_space(10.0);
    // Hue bar.
    let (bar, resp) = ui.allocate_exact_size(Vec2::new(w, 32.0), Sense::click_and_drag());
    if let Some(p) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() {
            hsv[0] = ((p.x - bar.left()) / bar.width()).clamp(0.0, 0.9999);
            changed = true;
        }
    }
    let mut mesh = egui::Mesh::default();
    let m = 36;
    for i in 0..=m {
        let t = i as f32 / m as f32;
        let c = Color32::from(Hsva::new(t, 1.0, 1.0, 1.0));
        let x = bar.left() + bar.width() * t;
        mesh.colored_vertex(Pos2::new(x, bar.top()), c);
        mesh.colored_vertex(Pos2::new(x, bar.bottom()), c);
    }
    for i in 0..m as u32 {
        let a = i * 2;
        mesh.add_triangle(a, a + 1, a + 2);
        mesh.add_triangle(a + 1, a + 3, a + 2);
    }
    let painter = ui.painter();
    painter.add(egui::Shape::mesh(mesh));
    let x = bar.left() + hsv[0] * bar.width();
    painter.circle(Pos2::new(x, bar.center().y), 14.0, Color32::from(Hsva::new(hsv[0], 1.0, 1.0, 1.0)), Stroke::new(3.0, Color32::WHITE));

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
    p.rect_filled(rect, theme::R_ROW, if selected { theme::accent_soft() } else { ELEVATED });
    if selected {
        p.rect_stroke(rect, theme::R_ROW, Stroke::new(1.5, ACCENT), egui::StrokeKind::Inside);
    }
    p.text(Pos2::new(rect.left() + 14.0, rect.center().y), Align2::LEFT_CENTER, label, egui::FontId::proportional(15.0), TEXT);
    let right = if selected { rect.right() - 40.0 } else { rect.right() - 14.0 };
    p.text(Pos2::new(right, rect.center().y), Align2::RIGHT_CENTER, detail, egui::FontId::proportional(13.0), TEXT2);
    if selected {
        paint(p, Rect::from_center_size(Pos2::new(rect.right() - 22.0, rect.center().y), Vec2::splat(18.0)), Icon::Check, ACCENT);
    }
    resp
}

/// Main action button (accent, full width).
pub fn primary_button(ui: &mut Ui, label: &str) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 50.0), Sense::click());
    let fill = if resp.is_pointer_button_down_on() { theme::ACCENT.gamma_multiply(0.8) } else { ACCENT };
    ui.painter().rect_filled(rect, theme::R_ROW, fill);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, egui::FontId::proportional(17.0), ON_ACCENT);
    resp
}
