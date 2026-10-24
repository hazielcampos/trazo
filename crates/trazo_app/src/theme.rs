//! Visual style: graphite, paper and a single accent (see the "Trazo —
//! Rediseño" design canvas, board "Sistema visual").
//!
//! Rules:
//! - Lime (`ACCENT`) only marks what is active: the tool in use, the current
//!   drawing, the playhead and options or switches that are on.
//! - One white (`PRIMARY`) button per screen: the main action (play, save…).
//! - Hanken Grotesk for the interface (tabular figures), JetBrains Mono only
//!   for the frame counter.

use egui::{Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow, Stroke, TextStyle, Visuals};

/// Whole-window background (OLED black-ish graphite).
pub const BG: Color32 = Color32::from_rgb(14, 14, 15);
/// Behind the canvas.
pub const CANVAS_BG: Color32 = Color32::from_rgb(14, 14, 15);
/// Docked panels on desktop.
pub const PANEL: Color32 = Color32::from_rgb(20, 20, 22);
/// Sheets and the timeline dock.
pub const SHEET: Color32 = Color32::from_rgb(22, 22, 24);
/// Floating islands (tool dock, rails).
pub const SURFACE: Color32 = Color32::from_rgb(27, 27, 29);
/// Rows, controls, menus.
pub const ELEVATED: Color32 = Color32::from_rgb(37, 37, 40);
/// Selected rows and segments; separators.
pub const SEPARATOR: Color32 = Color32::from_rgb(47, 47, 51);
/// Slider rails, switch off, outlines.
pub const CONTROL: Color32 = Color32::from_rgb(58, 58, 62);
pub const TEXT: Color32 = Color32::from_rgb(242, 242, 240);
pub const TEXT2: Color32 = Color32::from_rgb(161, 161, 166);
/// Disabled icons and quiet hints.
pub const TEXT3: Color32 = Color32::from_rgb(117, 117, 123);
/// What is active.
pub const ACCENT: Color32 = Color32::from_rgb(217, 244, 90);
/// The main action of a screen.
pub const PRIMARY: Color32 = Color32::from_rgb(242, 242, 240);
/// Icons and text drawn on lime or white.
pub const ON_ACCENT: Color32 = Color32::from_rgb(18, 18, 18);
pub const DANGER: Color32 = Color32::from_rgb(255, 122, 110);
pub const RECORDING: Color32 = Color32::from_rgb(255, 45, 122);
pub const SAVED: Color32 = Color32::from_rgb(91, 214, 192);
pub const PAPER: Color32 = Color32::from_rgb(236, 230, 214);

/// Track colors of the timeline: (dot, cell).
pub const TRACKS: [(Color32, Color32); 5] = [
    (Color32::from_rgb(143, 192, 240), Color32::from_rgb(53, 88, 122)),
    (Color32::from_rgb(237, 162, 135), Color32::from_rgb(122, 70, 54)),
    (Color32::from_rgb(175, 211, 155), Color32::from_rgb(63, 85, 54)),
    (Color32::from_rgb(212, 194, 146), Color32::from_rgb(79, 71, 48)),
    (Color32::from_rgb(183, 166, 224), Color32::from_rgb(70, 63, 92)),
];
pub const AUDIO_TRACK: (Color32, Color32) = (Color32::from_rgb(157, 179, 234), Color32::from_rgb(37, 48, 77));

/// Background of an option that is on (its icon goes lime).
pub fn accent_soft() -> Color32 {
    Color32::from_rgb(42, 42, 46)
}

/// Floating controls over the canvas (dock, rails, pills).
pub fn floating() -> Color32 {
    Color32::from_rgba_unmultiplied(27, 27, 29, 245)
}

/// Shadow of floating islands.
pub fn island_shadow() -> Shadow {
    Shadow { offset: [0, 10], blur: 30, spread: 0, color: Color32::from_black_alpha(128) }
}

pub const R_CONTROL: u8 = 12;
pub const R_ROW: u8 = 14;
pub const R_ISLAND: u8 = 20;
pub const R_SHEET: u8 = 24;

/// Minimum touch target.
pub const TOUCH: f32 = 44.0;

/// Font weights of the interface face.
#[derive(Clone, Copy)]
pub enum W {
    Regular,
    Medium,
    Semi,
    Bold,
}

pub fn font(size: f32, w: W) -> FontId {
    let family = match w {
        W::Regular => FontFamily::Proportional,
        W::Medium => FontFamily::Name("medium".into()),
        W::Semi => FontFamily::Name("semibold".into()),
        W::Bold => FontFamily::Name("bold".into()),
    };
    FontId::new(size, family)
}

/// The frame counter face.
pub fn counter_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

fn install_fonts(ctx: &egui::Context) {
    let mut f = FontDefinitions::default();
    let add = |f: &mut FontDefinitions, name: &str, bytes: &'static [u8]| {
        f.font_data.insert(name.to_owned(), std::sync::Arc::new(FontData::from_static(bytes)));
    };
    add(&mut f, "hanken-regular", include_bytes!("../../../assets/fonts/HankenGrotesk-Regular.ttf"));
    add(&mut f, "hanken-medium", include_bytes!("../../../assets/fonts/HankenGrotesk-Medium.ttf"));
    add(&mut f, "hanken-semibold", include_bytes!("../../../assets/fonts/HankenGrotesk-SemiBold.ttf"));
    add(&mut f, "hanken-bold", include_bytes!("../../../assets/fonts/HankenGrotesk-Bold.ttf"));
    add(&mut f, "jetbrains-semibold", include_bytes!("../../../assets/fonts/JetBrainsMono-SemiBold.ttf"));
    // egui's own fonts stay as fallback (symbols, other scripts).
    let fallback = f.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut fam = |key: FontFamily, first: &str| {
        let mut v = vec![first.to_owned()];
        v.extend(fallback.iter().cloned());
        f.families.insert(key, v);
    };
    fam(FontFamily::Proportional, "hanken-regular");
    fam(FontFamily::Name("medium".into()), "hanken-medium");
    fam(FontFamily::Name("semibold".into()), "hanken-semibold");
    fam(FontFamily::Name("bold".into()), "hanken-bold");
    fam(FontFamily::Monospace, "jetbrains-semibold");
    ctx.set_fonts(f);
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut v = Visuals::dark();
    v.dark_mode = true;
    v.panel_fill = SHEET;
    v.window_fill = SURFACE;
    v.extreme_bg_color = BG;
    v.faint_bg_color = ELEVATED;
    v.code_bg_color = ELEVATED;
    v.window_stroke = Stroke::new(1.0, Color32::from_white_alpha(14));
    v.window_corner_radius = CornerRadius::same(R_ISLAND);
    v.menu_corner_radius = CornerRadius::same(R_ROW);
    v.window_shadow = Shadow { offset: [0, 20], blur: 50, spread: 0, color: Color32::from_black_alpha(166) };
    v.popup_shadow = Shadow { offset: [0, 16], blur: 40, spread: 0, color: Color32::from_black_alpha(153) };
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(217, 244, 90, 70);
    v.selection.stroke = Stroke::new(1.5, TEXT);
    v.hyperlink_color = TEXT;
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
            (TextStyle::Button, FontId::new(15.0, FontFamily::Name("medium".into()))),
            (TextStyle::Heading, FontId::new(20.0, FontFamily::Name("semibold".into()))),
            (TextStyle::Monospace, FontId::new(14.0, FontFamily::Monospace)),
        ]
        .into();
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(12.0, 8.0);
        s.spacing.interact_size = egui::vec2(40.0, 36.0);
        s.spacing.slider_width = 160.0;
        s.spacing.slider_rail_height = 5.0;
        s.spacing.icon_width = 20.0;
        s.spacing.icon_width_inner = 12.0;
        s.spacing.window_margin = Margin::same(16);
        s.spacing.menu_margin = Margin::same(6);
        s.spacing.combo_height = 320.0;
        s.spacing.scroll.floating = true;
    });
}
