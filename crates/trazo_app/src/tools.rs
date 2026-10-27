//! Tool selection, per-tool settings and the brush library (built-in + user).

use crate::i18n::tr;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use trazo_core::brush::{builtin_brushes, Brush, BrushCategory};
use trazo_core::brush_io::{self, BRUSH_EXTENSION};
use trazo_core::fill::FillOptions;
use trazo_core::shapes::ShapeKind;
use trazo_core::stroke::StrokeSettings;
use trazo_core::Rgba;
use egui::Color32;
use serde::{Deserialize, Serialize};

use crate::icons::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    Brush,
    Eraser,
    Fill,
    Eyedropper,
    Select,
    Lasso,
    Wand,
    Shape,
    Text,
    Hand,
    /// Magnifier: zoom without a mouse wheel or pinch.
    Zoom,
    /// Magnifier that zooms out.
    ZoomOut,
}

/// What the lasso does when the loop is closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LassoMode {
    Select,
    Fill,
    Erase,
}

impl Tool {
    pub const ALL: [Tool; 12] =
        [Tool::Brush, Tool::Eraser, Tool::Fill, Tool::Eyedropper, Tool::Select, Tool::Lasso, Tool::Wand, Tool::Shape, Tool::Text, Tool::Hand, Tool::Zoom, Tool::ZoomOut];

    pub fn icon(self) -> Icon {
        match self {
            Tool::Brush => Icon::Pencil,
            Tool::Eraser => Icon::Eraser,
            Tool::Fill => Icon::Bucket,
            Tool::Eyedropper => Icon::Eyedropper,
            Tool::Select => Icon::SelectRect,
            Tool::Lasso => Icon::Lasso,
            Tool::Wand => Icon::Wand,
            Tool::Shape => Icon::Shapes,
            Tool::Text => Icon::Text,
            Tool::Hand => Icon::Hand,
            Tool::Zoom => Icon::Zoom,
            Tool::ZoomOut => Icon::ZoomOut,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Tool::Brush => tr("Pincel (B)"),
            Tool::Eraser => tr("Borrador (E)"),
            Tool::Fill => tr("Relleno (G)"),
            Tool::Eyedropper => tr("Cuentagotas (I · Alt mientras dibujas)"),
            Tool::Select => tr("Selección rectangular (S)"),
            Tool::Lasso => tr("Lazo: seleccionar, rellenar o borrar (L)"),
            Tool::Wand => tr("Varita mágica: selecciona por color (W)"),
            Tool::Shape => tr("Figuras: línea, rectángulo, elipse (U)"),
            Tool::Text => tr("Texto: toca el lienzo para escribir (T)"),
            Tool::Hand => tr("Mover lienzo (H / espacio)"),
            Tool::Zoom => tr("Lupa para acercar: toca o arrastra a la derecha (Z cambia a alejar)"),
            Tool::ZoomOut => tr("Lupa para alejar: toca o arrastra a la izquierda (Z cambia a acercar)"),
        }
    }

    pub fn is_selection(self) -> bool {
        matches!(self, Tool::Select | Tool::Lasso | Tool::Wand)
    }
}

/// User adjustments remembered per brush.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BrushTweak {
    pub size: f32,
    pub opacity: f32,
}

pub struct ToolState {
    pub tool: Tool,
    pub library: Vec<Brush>,
    pub tweaks: Vec<BrushTweak>,
    /// File backing each user brush (None for built-ins).
    pub files: Vec<Option<PathBuf>>,
    pub brush: usize,
    pub eraser: usize,
    pub color: Color32,
    pub recent_colors: Vec<Color32>,
    pub stabilizer: f32,
    /// Fake pressure from pointer speed for mice and trackpads.
    pub simulate_pressure: bool,
    pub fill: FillOptions,
    pub fill_all_layers: bool,
    /// Fill the same spot on every drawing of the layer.
    pub fill_every_drawing: bool,
    pub shape: ShapeKind,
    /// Square / circle / 15° lines (also with Shift held).
    pub shape_constrain: bool,
    /// Bumped when any brush changes (previews are regenerated).
    pub brush_version: u64,
    pub lasso_mode: LassoMode,
    /// Pressure curve exponent (1 = linear).
    pub pressure_gamma: f32,
    pub symmetry_vertical: bool,
    pub symmetry_horizontal: bool,
    /// Eyedropper reads only the current layer.
    pub pick_layer_only: bool,
    /// Colors saved by the user.
    pub user_palette: Vec<Color32>,
    pub show_grid: bool,
    pub grid_size: u32,
    /// User brushes waiting to be written to disk.
    dirty_files: HashSet<usize>,
}

pub const PALETTE: [Color32; 16] = [
    Color32::from_rgb(0, 0, 0),
    Color32::from_rgb(255, 255, 255),
    Color32::from_rgb(128, 128, 128),
    Color32::from_rgb(64, 48, 40),
    Color32::from_rgb(230, 57, 70),
    Color32::from_rgb(255, 140, 50),
    Color32::from_rgb(255, 214, 64),
    Color32::from_rgb(120, 200, 80),
    Color32::from_rgb(30, 140, 90),
    Color32::from_rgb(70, 190, 230),
    Color32::from_rgb(40, 90, 200),
    Color32::from_rgb(120, 70, 190),
    Color32::from_rgb(240, 120, 180),
    Color32::from_rgb(255, 205, 170),
    Color32::from_rgb(190, 130, 90),
    Color32::from_rgb(110, 70, 45),
];

/// Tool settings kept between sessions.
#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
pub struct SavedTools {
    pub pressure_gamma: Option<f32>,
    pub user_palette: Vec<[u8; 4]>,
    pub grid_size: Option<u32>,
    pub color: Option<[u8; 4]>,
    pub brush: Option<String>,
    pub eraser: Option<String>,
    pub tweaks: Vec<(String, BrushTweak)>,
    pub stabilizer: Option<f32>,
    pub simulate_pressure: Option<bool>,
    pub recent: Vec<[u8; 4]>,
}

impl ToolState {
    /// Built-in brushes plus every `.animbrush` in `brushes_dir`.
    pub fn load(brushes_dir: &Path) -> Self {
        let mut library = builtin_brushes();
        let mut files: Vec<Option<PathBuf>> = vec![None; library.len()];
        let mut user: Vec<(PathBuf, Brush)> = std::fs::read_dir(brushes_dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == BRUSH_EXTENSION))
            .filter_map(|p| match brush_io::load_brush(&p) {
                Ok(b) => Some((p, b)),
                Err(e) => {
                    log::warn!("pincel ignorado {}: {e}", p.display());
                    None
                }
            })
            .collect();
        user.sort_by(|a, b| a.1.name.cmp(&b.1.name));
        for (p, b) in user {
            library.push(b);
            files.push(Some(p));
        }
        let tweaks = library.iter().map(|b| BrushTweak { size: b.size, opacity: b.opacity }).collect();
        let find = |name: &str| library.iter().position(|b| b.name == name).unwrap_or(0);
        Self {
            tool: Tool::Brush,
            brush: find("Lápiz"),
            eraser: find("Borrador duro"),
            library,
            tweaks,
            files,
            color: Color32::BLACK,
            recent_colors: Vec::new(),
            stabilizer: 0.3,
            simulate_pressure: false,
            fill: FillOptions::default(),
            fill_all_layers: true,
            fill_every_drawing: false,
            shape: ShapeKind::Line,
            shape_constrain: false,
            brush_version: 0,
            dirty_files: HashSet::new(),
            lasso_mode: LassoMode::Select,
            pressure_gamma: 1.0,
            symmetry_vertical: false,
            symmetry_horizontal: false,
            pick_layer_only: false,
            user_palette: Vec::new(),
            show_grid: false,
            grid_size: 64,
        }
    }

    pub fn saved(&self) -> SavedTools {
        let c = self.color.to_srgba_unmultiplied();
        SavedTools {
            color: Some(c),
            brush: Some(self.library[self.brush].name.clone()),
            eraser: Some(self.library[self.eraser].name.clone()),
            tweaks: self.library.iter().zip(&self.tweaks).map(|(b, t)| (b.name.clone(), *t)).collect(),
            stabilizer: Some(self.stabilizer),
            simulate_pressure: Some(self.simulate_pressure),
            recent: self.recent_colors.iter().map(|c| c.to_srgba_unmultiplied()).collect(),
            pressure_gamma: Some(self.pressure_gamma),
            user_palette: self.user_palette.iter().map(|c| c.to_srgba_unmultiplied()).collect(),
            grid_size: Some(self.grid_size),
        }
    }

    pub fn restore(&mut self, s: SavedTools) {
        if let Some([r, g, b, a]) = s.color {
            self.color = Color32::from_rgba_unmultiplied(r, g, b, a);
        }
        let find = |name: &Option<String>| name.as_ref().and_then(|n| self.library.iter().position(|b| &b.name == n));
        if let Some(i) = find(&s.brush) {
            self.brush = i;
        }
        if let Some(i) = find(&s.eraser) {
            self.eraser = i;
        }
        for (name, t) in s.tweaks {
            if let Some(i) = self.library.iter().position(|b| b.name == name) {
                self.tweaks[i] = BrushTweak { size: t.size.clamp(0.5, 1000.0), opacity: t.opacity.clamp(0.01, 1.0) };
            }
        }
        if let Some(v) = s.stabilizer {
            self.stabilizer = v.clamp(0.0, 0.95);
        }
        if let Some(v) = s.simulate_pressure {
            self.simulate_pressure = v;
        }
        if let Some(g) = s.pressure_gamma {
            self.pressure_gamma = g.clamp(0.3, 3.0);
        }
        if let Some(g) = s.grid_size {
            self.grid_size = g.clamp(4, 1024);
        }
        self.user_palette = s.user_palette.into_iter().take(64).map(|[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a)).collect();
        self.recent_colors = s.recent.into_iter().take(12).map(|[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a)).collect();
    }

    /// Index of the brush used by the current tool (eraser for the eraser tool).
    pub fn active_index(&self) -> usize {
        if self.tool == Tool::Eraser {
            self.eraser
        } else {
            self.brush
        }
    }

    pub fn active_brush(&self) -> &Brush {
        &self.library[self.active_index()]
    }

    pub fn active_tweak_mut(&mut self) -> &mut BrushTweak {
        let i = self.active_index();
        &mut self.tweaks[i]
    }

    pub fn select_brush(&mut self, index: usize) {
        if self.library[index].is_eraser() {
            self.eraser = index;
            self.tool = Tool::Eraser;
        } else {
            self.brush = index;
            if self.tool != Tool::Shape {
                self.tool = Tool::Brush;
            }
        }
    }

    pub fn is_user_brush(&self, index: usize) -> bool {
        self.files.get(index).is_some_and(|f| f.is_some())
    }

    /// Add a user brush, saving it to `dir`. Returns its index.
    pub fn add_user_brush(&mut self, mut brush: Brush, dir: &Path) -> Result<usize, String> {
        brush.category = BrushCategory::Custom;
        brush.name = self.unique_name(&brush.name);
        let path = unique_file(dir, &brush.name);
        brush_io::save_brush(&brush, &path).map_err(|e| e.to_string())?;
        self.tweaks.push(BrushTweak { size: brush.size, opacity: brush.opacity });
        self.library.push(brush);
        self.files.push(Some(path));
        self.brush_version += 1;
        Ok(self.library.len() - 1)
    }

    pub fn delete_user_brush(&mut self, index: usize) -> Result<(), String> {
        let Some(Some(path)) = self.files.get(index).cloned() else { return Err(tr("es un pincel de serie").into()) };
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        self.library.remove(index);
        self.tweaks.remove(index);
        self.files.remove(index);
        self.dirty_files = self.dirty_files.iter().filter(|&&i| i != index).map(|&i| if i > index { i - 1 } else { i }).collect();
        let fix = |i: usize| if i == index { 0 } else if i > index { i - 1 } else { i };
        self.brush = fix(self.brush);
        self.eraser = fix(self.eraser);
        if !self.library[self.eraser].is_eraser() {
            self.eraser = self.library.iter().position(|b| b.is_eraser()).unwrap_or(0);
        }
        self.brush_version += 1;
        Ok(())
    }

    /// Call after editing a brush's settings.
    pub fn brush_changed(&mut self, index: usize) {
        self.brush_version += 1;
        if self.is_user_brush(index) {
            self.dirty_files.insert(index);
        }
    }

    /// Write edited user brushes to disk (call when no slider is being dragged).
    pub fn flush_user_brushes(&mut self) -> Option<String> {
        let mut err = None;
        for i in std::mem::take(&mut self.dirty_files) {
            if let (Some(Some(path)), Some(b)) = (self.files.get(i), self.library.get(i)) {
                if let Err(e) = brush_io::save_brush(b, path) {
                    err = Some(tf!("No se pudo guardar el pincel: {}", crate::i18n::tr_err(&e.to_string())));
                }
            }
        }
        err
    }

    fn unique_name(&self, base: &str) -> String {
        let base = base.trim();
        if !self.library.iter().any(|b| b.name == base) {
            return base.to_string();
        }
        (2..).map(|n| format!("{base} {n}")).find(|n| !self.library.iter().any(|b| &b.name == n)).expect("infinite range")
    }

    pub fn color_rgba(&self) -> Rgba {
        let [r, g, b, a] = self.color.to_srgba_unmultiplied();
        Rgba::from_rgba8([r, g, b, a])
    }

    pub fn set_color(&mut self, c: Color32) {
        self.color = c;
    }

    /// Remember a color in the "recent" strip.
    pub fn push_recent(&mut self) {
        let c = self.color;
        self.recent_colors.retain(|x| *x != c);
        self.recent_colors.insert(0, c);
        self.recent_colors.truncate(12);
    }

    /// Settings for a new stroke. `center` is the canvas center (symmetry axes).
    pub fn stroke_settings(&self, alpha_lock: bool, center: (f32, f32)) -> StrokeSettings {
        let i = self.active_index();
        StrokeSettings {
            color: self.color_rgba(),
            size: self.tweaks[i].size,
            opacity: self.tweaks[i].opacity,
            stabilizer: self.stabilizer,
            rope_scale: 1.0,
            alpha_lock,
            pressure_gamma: self.pressure_gamma,
            symmetry: trazo_core::stroke::Symmetry {
                vertical: self.symmetry_vertical,
                horizontal: self.symmetry_horizontal,
                cx: center.0,
                cy: center.1,
            },
        }
    }

    /// Rename a user brush, keeping names unique (saved settings use names).
    pub fn rename_brush(&mut self, index: usize, name: &str) {
        let name = name.trim();
        if name.is_empty() || self.library[index].name == name {
            return;
        }
        let old = std::mem::take(&mut self.library[index].name);
        let unique = self.unique_name(name);
        let _ = old;
        self.library[index].name = unique;
        self.brush_changed(index);
    }
}

fn unique_file(dir: &Path, name: &str) -> PathBuf {
    let slug: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();
    let mut p = dir.join(format!("{slug}.{BRUSH_EXTENSION}"));
    let mut n = 2;
    while p.exists() {
        p = dir.join(format!("{slug}_{n}.{BRUSH_EXTENSION}"));
        n += 1;
    }
    p
}
