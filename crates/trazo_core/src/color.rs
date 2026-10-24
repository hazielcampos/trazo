//! Color types and per-pixel blend math (premultiplied alpha).

use serde::{Deserialize, Serialize};

/// Straight (non-premultiplied) sRGB color, 0..=1 per channel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const BLACK: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 1.0 };
    pub const WHITE: Rgba = Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 };
    pub const TRANSPARENT: Rgba = Rgba { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_rgba8(c: [u8; 4]) -> Self {
        Self::new(c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0)
    }

    pub fn to_rgba8(self) -> [u8; 4] {
        [to_u8(self.r), to_u8(self.g), to_u8(self.b), to_u8(self.a)]
    }

    /// Premultiplied RGBA8 for this color at the given extra coverage.
    pub fn premul8(self, coverage: f32) -> [f32; 4] {
        let a = (self.a * coverage).clamp(0.0, 1.0);
        [self.r * a * 255.0, self.g * a * 255.0, self.b * a * 255.0, a * 255.0]
    }

    pub fn from_premul8(p: [u8; 4]) -> Self {
        if p[3] == 0 {
            return Self::TRANSPARENT;
        }
        let a = p[3] as f32 / 255.0;
        Self::new(
            (p[0] as f32 / 255.0 / a).min(1.0),
            (p[1] as f32 / 255.0 / a).min(1.0),
            (p[2] as f32 / 255.0 / a).min(1.0),
            a,
        )
    }
}

#[inline]
pub fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
fn f(v: u8) -> f32 {
    v as f32 / 255.0
}

/// Layer blend modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
    Add,
    Overlay,
}

impl BlendMode {
    pub const ALL: [BlendMode; 5] =
        [BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen, BlendMode::Add, BlendMode::Overlay];

    pub fn label(self) -> &'static str {
        match self {
            BlendMode::Normal => "Normal",
            BlendMode::Multiply => "Multiplicar",
            BlendMode::Screen => "Trama",
            BlendMode::Add => "Añadir",
            BlendMode::Overlay => "Superponer",
        }
    }
}

/// Composite premultiplied `src` (scaled by `opacity`) onto premultiplied `dst`
/// in floating point (0..=1 per channel). Returns the new destination.
#[inline]
pub fn blend_px(dst: [f32; 4], src: [u8; 4], opacity: f32, mode: BlendMode) -> [f32; 4] {
    let s = [f(src[0]) * opacity, f(src[1]) * opacity, f(src[2]) * opacity, f(src[3]) * opacity];
    let sa = s[3];
    if sa <= 0.0 {
        return dst;
    }
    let da = dst[3];
    let mut out = [0.0f32; 4];
    out[3] = sa + da * (1.0 - sa);
    for c in 0..3 {
        let (sc, dc) = (s[c], dst[c]);
        out[c] = match mode {
            BlendMode::Normal => sc + dc * (1.0 - sa),
            BlendMode::Multiply => sc * dc + sc * (1.0 - da) + dc * (1.0 - sa),
            BlendMode::Screen => sc + dc - sc * dc,
            BlendMode::Add => (sc + dc).min(out[3]),
            BlendMode::Overlay => {
                // Separable overlay on premultiplied values (W3C compositing spec).
                let b = if 2.0 * dc <= da {
                    2.0 * sc * dc
                } else {
                    sa * da - 2.0 * (da - dc) * (sa - sc)
                };
                b + sc * (1.0 - da) + dc * (1.0 - sa)
            }
        };
    }
    out
}

#[inline]
pub fn pack(p: [f32; 4]) -> [u8; 4] {
    [to_u8(p[0]), to_u8(p[1]), to_u8(p[2]), to_u8(p[3])]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_over_opaque() {
        let dst = [1.0, 1.0, 1.0, 1.0];
        let out = blend_px(dst, [255, 0, 0, 255], 1.0, BlendMode::Normal);
        assert_eq!(pack(out), [255, 0, 0, 255]);
        let half = blend_px(dst, [255, 0, 0, 255], 0.5, BlendMode::Normal);
        assert_eq!(pack(half), [255, 128, 128, 255]);
    }

    #[test]
    fn multiply_white_is_identity() {
        let dst = [0.2, 0.4, 0.6, 1.0];
        let out = blend_px(dst, [255, 255, 255, 255], 1.0, BlendMode::Multiply);
        assert_eq!(pack(out), pack(dst));
    }

    #[test]
    fn premul_roundtrip() {
        let c = Rgba::new(1.0, 0.5, 0.0, 0.5);
        let p = c.premul8(1.0);
        let back = Rgba::from_premul8([p[0] as u8, p[1] as u8, p[2] as u8, p[3] as u8]);
        assert!((back.g - 0.5).abs() < 0.02);
    }
}
