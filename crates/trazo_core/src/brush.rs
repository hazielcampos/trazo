//! Brush definitions and the built-in brush library.
//!
//! A [`Brush`] is pure data, so presets can be saved, shared and edited by the
//! user. The stroke engine in [`crate::stroke`] interprets it.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::color::BlendMode;

/// What the brush does to the layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrushMode {
    Paint,
    Erase,
    /// Drags existing color along the stroke.
    Smudge,
    /// Softens pixels under the brush.
    Blur,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DabShape {
    /// Elliptical dab with soft or hard edge.
    Round,
    /// Hard, aliased square aligned to the pixel grid (pixel art).
    Pixel,
    /// Cloud of tiny random dots (spray can).
    Spray,
    /// Stamp from a grayscale image (see [`Brush::tip`]).
    Image,
}

/// Grayscale brush tip image. 255 = full paint.
#[derive(Debug, Clone, PartialEq)]
pub struct BrushTip {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

impl BrushTip {
    /// Build a tip from straight RGBA pixels. Images with transparency use
    /// their alpha; fully opaque images use darkness (black = paint).
    pub fn from_rgba(width: u32, height: u32, rgba: &[u8]) -> Self {
        let has_alpha = rgba.chunks_exact(4).any(|p| p[3] < 250);
        let alpha = rgba
            .chunks_exact(4)
            .map(|p| {
                if has_alpha {
                    p[3]
                } else {
                    let luma = (p[0] as u32 * 54 + p[1] as u32 * 183 + p[2] as u32 * 19) >> 8;
                    255 - luma.min(255) as u8
                }
            })
            .collect();
        Self { width, height, alpha }
    }

    /// Bilinear sample in tip pixel coordinates (0 outside), returns 0..=1.
    pub fn sample(&self, x: f32, y: f32) -> f32 {
        let (w, h) = (self.width as i32, self.height as i32);
        let (x, y) = (x - 0.5, y - 0.5);
        let (x0, y0) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |xx: i32, yy: i32| -> f32 {
            if xx < 0 || yy < 0 || xx >= w || yy >= h {
                0.0
            } else {
                self.alpha[(yy * w + xx) as usize] as f32 / 255.0
            }
        };
        let top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * fx;
        let bot = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * fx;
        top + (bot - top) * fy
    }

    /// Straight RGBA (black paint on transparent) for saving as PNG.
    pub fn to_rgba(&self) -> Vec<u8> {
        self.alpha.iter().flat_map(|&a| [0, 0, 0, a]).collect()
    }
}

/// How overlapping dabs of the same stroke combine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Accumulate {
    /// Never darker than one dab: markers, ink, flat color.
    Max,
    /// Dabs build up: airbrush, watercolor, pencil shading.
    Buildup,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Brush {
    pub name: String,
    pub category: BrushCategory,
    pub mode: BrushMode,
    pub shape: DabShape,
    pub accumulate: Accumulate,
    /// Default diameter in canvas pixels.
    pub size: f32,
    /// Size multiplier at zero pressure (0..=1) when `pressure_size` is on.
    pub min_size: f32,
    pub opacity: f32,
    /// Per-dab strength.
    pub flow: f32,
    /// 0 = very soft edge, 1 = hard edge.
    pub hardness: f32,
    /// Distance between dabs as a fraction of the diameter.
    pub spacing: f32,
    pub pressure_size: bool,
    pub pressure_opacity: bool,
    /// Random position offset, fraction of the diameter.
    pub jitter: f32,
    /// Random size variation (0..=1).
    pub size_jitter: f32,
    pub angle_deg: f32,
    /// 1 = circle, smaller = flatter ellipse (calligraphy nib).
    pub roundness: f32,
    /// Paper texture strength (0..=1).
    pub grain: f32,
    /// Paper texture feature size in pixels.
    pub grain_scale: f32,
    /// Dots per dab for the spray shape.
    pub density: u32,
    /// Length in pixels over which the stroke grows from thin to full size.
    pub taper_start: f32,
    /// Length in pixels over which the stroke thins out at the end.
    #[serde(default)]
    pub taper_end: f32,
    /// How the paint mixes with what is already on the layer.
    #[serde(default)]
    pub blend: BlendMode,
    /// Random rotation per dab, in degrees (image tips, textures).
    #[serde(default)]
    pub rotation_jitter: f32,
    /// Rotate the tip along the stroke direction.
    #[serde(default)]
    pub follow_direction: bool,
    /// Tip image for [`DabShape::Image`]; stored next to the brush file.
    #[serde(skip)]
    pub tip: Option<Arc<BrushTip>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BrushCategory {
    Sketch,
    Ink,
    Paint,
    Texture,
    Effects,
    Eraser,
    /// Brushes created or imported by the user.
    Custom,
}

impl BrushCategory {
    pub const ALL: [BrushCategory; 7] = [
        BrushCategory::Custom,
        BrushCategory::Sketch,
        BrushCategory::Ink,
        BrushCategory::Paint,
        BrushCategory::Texture,
        BrushCategory::Effects,
        BrushCategory::Eraser,
    ];

    pub fn label(self) -> &'static str {
        match self {
            BrushCategory::Sketch => "Boceto",
            BrushCategory::Ink => "Tinta",
            BrushCategory::Paint => "Pintura",
            BrushCategory::Texture => "Textura",
            BrushCategory::Effects => "Efectos",
            BrushCategory::Eraser => "Borradores",
            BrushCategory::Custom => "Mis pinceles",
        }
    }
}

impl Brush {
    pub fn base(name: &str, category: BrushCategory) -> Self {
        Self {
            name: name.to_string(),
            category,
            mode: BrushMode::Paint,
            shape: DabShape::Round,
            accumulate: Accumulate::Max,
            size: 8.0,
            min_size: 0.2,
            opacity: 1.0,
            flow: 1.0,
            hardness: 0.9,
            spacing: 0.1,
            pressure_size: true,
            pressure_opacity: false,
            jitter: 0.0,
            size_jitter: 0.0,
            angle_deg: 0.0,
            roundness: 1.0,
            grain: 0.0,
            grain_scale: 2.0,
            density: 0,
            taper_start: 0.0,
            taper_end: 0.0,
            blend: BlendMode::Normal,
            rotation_jitter: 0.0,
            follow_direction: false,
            tip: None,
        }
    }

    pub fn is_eraser(&self) -> bool {
        self.mode == BrushMode::Erase
    }
}

/// The built-in brush library shown in the brush picker.
pub fn builtin_brushes() -> Vec<Brush> {
    use BrushCategory::*;
    let mut v = Vec::new();

    v.push(Brush {
        size: 4.0,
        hardness: 0.8,
        opacity: 1.0,
        flow: 0.7,
        accumulate: Accumulate::Max,
        pressure_opacity: true,
        min_size: 0.5,
        grain: 0.4,
        grain_scale: 1.5,
        spacing: 0.12,
        taper_end: 6.0,
        ..Brush::base("Lápiz", Sketch)
    });
    v.push(Brush {
        size: 10.0,
        hardness: 0.4,
        opacity: 0.45,
        flow: 0.5,
        accumulate: Accumulate::Max,
        pressure_opacity: true,
        grain: 0.8,
        grain_scale: 2.0,
        ..Brush::base("Lápiz de boceto", Sketch)
    });
    v.push(Brush {
        size: 22.0,
        hardness: 0.35,
        opacity: 0.8,
        flow: 0.35,
        accumulate: Accumulate::Buildup,
        grain: 0.85,
        grain_scale: 3.0,
        jitter: 0.04,
        pressure_opacity: true,
        ..Brush::base("Carboncillo", Sketch)
    });
    v.push(Brush {
        size: 5.0,
        hardness: 1.0,
        spacing: 0.05,
        pressure_size: false,
        ..Brush::base("Monolínea", Sketch)
    });
    v.push(Brush {
        size: 8.0,
        hardness: 0.5,
        opacity: 0.5,
        flow: 0.6,
        grain: 0.6,
        grain_scale: 2.0,
        pressure_opacity: true,
        ..Brush::base("Lápiz de color (rough)", Sketch)
    });
    v.push(Brush { size: 4.0, hardness: 0.95, spacing: 0.06, min_size: 0.6, ..Brush::base("Bolígrafo", Ink) });
    v.push(Brush {
        size: 6.0,
        hardness: 0.92,
        roundness: 0.8,
        jitter: 0.01,
        spacing: 0.05,
        min_size: 0.2,
        taper_start: 10.0,
        taper_end: 14.0,
        ..Brush::base("Tinta seca", Ink)
    });
    v.push(Brush {
        size: 7.0,
        hardness: 1.0,
        spacing: 0.05,
        min_size: 0.15,
        taper_start: 16.0,
        taper_end: 14.0,
        ..Brush::base("Tinta G-pen", Ink)
    });
    v.push(Brush {
        size: 9.0,
        hardness: 0.97,
        roundness: 0.25,
        angle_deg: 40.0,
        spacing: 0.04,
        min_size: 0.4,
        ..Brush::base("Caligrafía", Ink)
    });
    v.push(Brush {
        size: 20.0,
        hardness: 0.85,
        opacity: 0.65,
        pressure_size: false,
        ..Brush::base("Rotulador", Ink)
    });
    v.push(Brush {
        size: 26.0,
        hardness: 0.9,
        opacity: 0.5,
        roundness: 0.35,
        angle_deg: 30.0,
        pressure_size: false,
        blend: BlendMode::Multiply,
        ..Brush::base("Resaltador", Ink)
    });
    v.push(Brush {
        size: 1.0,
        shape: DabShape::Pixel,
        hardness: 1.0,
        spacing: 0.5,
        pressure_size: false,
        ..Brush::base("Píxel", Ink)
    });
    v.push(Brush {
        size: 18.0,
        hardness: 0.55,
        flow: 0.25,
        accumulate: Accumulate::Buildup,
        pressure_opacity: true,
        min_size: 0.3,
        ..Brush::base("Pincel redondo", Paint)
    });
    v.push(Brush {
        size: 40.0,
        hardness: 0.15,
        opacity: 0.6,
        flow: 0.12,
        accumulate: Accumulate::Buildup,
        grain: 0.45,
        grain_scale: 6.0,
        size_jitter: 0.1,
        spacing: 0.08,
        ..Brush::base("Acuarela", Paint)
    });
    v.push(Brush {
        size: 24.0,
        hardness: 0.75,
        flow: 0.3,
        pressure_opacity: true,
        accumulate: Accumulate::Buildup,
        grain: 0.25,
        grain_scale: 4.0,
        ..Brush::base("Gouache", Paint)
    });
    v.push(Brush {
        size: 70.0,
        hardness: 0.0,
        flow: 0.06,
        accumulate: Accumulate::Buildup,
        pressure_size: false,
        pressure_opacity: true,
        spacing: 0.05,
        ..Brush::base("Aerógrafo", Paint)
    });
    v.push(Brush {
        size: 50.0,
        shape: DabShape::Spray,
        density: 30,
        flow: 0.9,
        accumulate: Accumulate::Buildup,
        pressure_size: false,
        spacing: 0.15,
        ..Brush::base("Spray", Texture)
    });
    v.push(Brush {
        size: 14.0,
        hardness: 0.8,
        opacity: 0.95,
        grain: 0.9,
        grain_scale: 2.5,
        jitter: 0.03,
        ..Brush::base("Crayón", Texture)
    });
    v.push(Brush {
        size: 30.0,
        hardness: 0.6,
        flow: 0.7,
        opacity: 0.9,
        grain: 0.6,
        grain_scale: 8.0,
        accumulate: Accumulate::Buildup,
        size_jitter: 0.25,
        jitter: 0.08,
        ..Brush::base("Esponja", Texture)
    });
    v.push(Brush {
        size: 30.0,
        mode: BrushMode::Smudge,
        hardness: 0.3,
        flow: 0.7,
        pressure_size: false,
        spacing: 0.08,
        ..Brush::base("Difuminar", Effects)
    });
    v.push(Brush {
        size: 40.0,
        mode: BrushMode::Blur,
        hardness: 0.2,
        flow: 0.5,
        pressure_size: false,
        spacing: 0.15,
        ..Brush::base("Desenfocar", Effects)
    });
    v.push(Brush {
        size: 24.0,
        mode: BrushMode::Erase,
        hardness: 0.95,
        pressure_size: false,
        ..Brush::base("Borrador duro", Eraser)
    });
    v.push(Brush {
        size: 50.0,
        mode: BrushMode::Erase,
        hardness: 0.1,
        flow: 0.12,
        spacing: 0.06,
        accumulate: Accumulate::Buildup,
        pressure_size: false,
        ..Brush::base("Borrador suave", Eraser)
    });
    v.push(Brush {
        size: 2.0,
        mode: BrushMode::Erase,
        shape: DabShape::Pixel,
        hardness: 1.0,
        spacing: 0.5,
        pressure_size: false,
        ..Brush::base("Borrador píxel", Eraser)
    });
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_is_large_and_names_unique() {
        let b = builtin_brushes();
        assert!(b.len() >= 24);
        let mut names: Vec<_> = b.iter().map(|b| b.name.clone()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), b.len());
        for cat in BrushCategory::ALL.into_iter().filter(|c| *c != BrushCategory::Custom) {
            assert!(b.iter().any(|x| x.category == cat), "empty category {cat:?}");
        }
    }
}
