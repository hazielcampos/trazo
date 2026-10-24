//! Visual style: dark, neutral and touch-first, so the drawing's colors read
//! true. One accent (violet) marks what is active, selected or the main action.
//! Tokens match the design in the "Trazo — Interfaz responsiva" canvas.

use egui::{Color32, CornerRadius, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals};

/// Whole-window background.
pub const BG: Color32 = Color32::from_rgb(0, 0, 0);
/// Behind the canvas.
pub const CANVAS_BG: Color32 = Color32::from_rgb(14, 14, 16);
/// Bars, panels, sheets.
pub const SURFACE: Color32 = Color32::from_rgb(28, 28, 30);
/// Rows, controls and cards on a surface.
pub const ELEVATED: Color32 = Color32::from_rgb(44, 44, 46);
pub const SEPARATOR: Color32 = Color32::from_rgb(58, 58, 60);
/// Slider rails, switch off, dashed outlines.
pub const CONTROL: Color32 = Color32::from_rgb(72, 72, 74);
pub const TEXT: Color32 = Color32::from_rgb(245, 245, 247);
pub const TEXT2: Color32 = Color32::from_rgb(161, 161, 166);
pub const TEXT3: Color32 = Color32::from_rgb(99, 99, 102);
pub const ACCENT: Color32 = Color32::from_rgb(140, 123, 255);
/// Icons and text drawn on the accent.
pub const ON_ACCENT: Color32 = Color32::from_rgb(28, 28, 30);
pub const DANGER: Color32 = Color32::from_rgb(255, 105, 97);

/// Accent at low opacity: selected rows and toggled chips.
pub fn accent_soft() -> Color32 {
    Color32::from_rgba_unmultiplied(140, 123, 255, 46)
}

/// Floating controls over the canvas (dock, rails, pills).
pub fn floating() -> Color32 {
    Color32::from_rgba_unmultiplied(44, 44, 46, 240)
}

pub const R_CONTROL: u8 = 10;
pub const R_ROW: u8 = 14;
pub const R_SHEET: u8 = 22;

/// Minimum touch target.
pub const TOUCH: f32 = 44.0;

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = SURFACE;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BG;
    v.faint_bg_color = ELEVATED;
    v.code_bg_color = ELEVATED;
    v.window_stroke = Stroke::new(1.0, SEPARATOR);
    v.window_corner_radius = CornerRadius::same(R_ROW);
    v.menu_corner_radius = CornerRadius::same(R_ROW);
    v.window_shadow = Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(140) };
    v.popup_shadow = Shadow { offset: [0, 6], blur: 20, spread: 0, color: Color32::from_black_alpha(120) };
    v.selection.bg_fill = ACCENT.linear_multiply(0.55);
    v.selection.stroke = Stroke::new(1.5, ACCENT);
    v.hyperlink_color = ACCENT;
    v.warn_fg_color = Color32::from_rgb(255, 204, 0);
    v.error_fg_color = DANGER;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;

    let radius = CornerRadius::same(R_CONTROL);
    for (w, fill, fg) in [
        (&mut v.widgets.noninteractive, SURFACE, TEXT2),
        (&mut v.widgets.inactive, ELEVATED, TEXT),
        (&mut v.widgets.hovered, SEPARATOR, TEXT),
        (&mut v.widgets.active, CONTROL, TEXT),
        (&mut v.widgets.open, ELEVATED, TEXT),
    ] {
        w.corner_radius = radius;
        w.bg_fill = fill;
        w.weak_bg_fill = fill;
        w.bg_stroke = Stroke::NONE;
        w.fg_stroke = Stroke::new(1.5, fg);
        w.expansion = 0.0;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, SEPARATOR);
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    ctx.set_visuals(v);

    ctx.global_style_mut(|s| {
        s.text_styles = [
            (TextStyle::Small, FontId::new(12.0, FontFamily::Proportional)),
            (TextStyle::Body, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Button, FontId::new(15.0, FontFamily::Proportional)),
            (TextStyle::Heading, FontId::new(20.0, FontFamily::Proportional)),
            (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 8.0);
        s.spacing.interact_size = egui::vec2(40.0, 36.0);
        s.spacing.slider_width = 160.0;
        s.spacing.slider_rail_height = 4.0;
        s.spacing.icon_width = 20.0;
        s.spacing.icon_width_inner = 12.0;
        s.spacing.window_margin = Margin::same(16);
        s.spacing.menu_margin = Margin::same(8);
        s.spacing.combo_height = 320.0;
        s.spacing.scroll.floating = true;
    });
}
