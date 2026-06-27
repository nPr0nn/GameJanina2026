//! Color type and a predefined palette.

use bytemuck::{Pod, Zeroable};

/// An RGBA color stored as 8 bits per channel
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Return this color with its alpha scaled by `alpha` (clamped to `0..=1`).
    /// Raylib's `Fade`: `WHITE.with_alpha(0.5)` is 50%-transparent white. Handy
    /// for fade overlays and translucent panels.
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ..self
        }
    }

    /// Convert to linear-space `[f32; 4]` suitable for an sRGB swapchain/texture.
    ///
    /// The vertex colors are interpreted as sRGB (the convention everyone draws
    /// in), so we convert to linear here; the GPU then writes them into the
    /// sRGB render target which applies the inverse transform on display.
    pub fn to_linear(self) -> [f32; 4] {
        [
            srgb_to_linear(self.r),
            srgb_to_linear(self.g),
            srgb_to_linear(self.b),
            self.a as f32 / 255.0,
        ]
    }

    /// `wgpu::Color` (linear) for use as a clear value.
    pub fn to_wgpu(self) -> wgpu::Color {
        let [r, g, b, a] = self.to_linear();
        wgpu::Color {
            r: r as f64,
            g: g as f64,
            b: b as f64,
            a: a as f64,
        }
    }
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

// Raylib palette subset.
pub const LIGHTGRAY: Color = Color::new(200, 200, 200, 255);
pub const GRAY: Color = Color::new(130, 130, 130, 255);
pub const DARKGRAY: Color = Color::new(80, 80, 80, 255);
pub const YELLOW: Color = Color::new(253, 249, 0, 255);
pub const GOLD: Color = Color::new(255, 203, 0, 255);
pub const ORANGE: Color = Color::new(255, 161, 0, 255);
pub const PINK: Color = Color::new(255, 109, 194, 255);
pub const RED: Color = Color::new(230, 41, 55, 255);
pub const MAROON: Color = Color::new(190, 33, 55, 255);
pub const GREEN: Color = Color::new(0, 228, 48, 255);
pub const LIME: Color = Color::new(0, 158, 47, 255);
pub const DARKGREEN: Color = Color::new(0, 117, 44, 255);
pub const SKYBLUE: Color = Color::new(102, 191, 255, 255);
pub const BLUE: Color = Color::new(0, 121, 241, 255);
pub const DARKBLUE: Color = Color::new(0, 82, 172, 255);
pub const PURPLE: Color = Color::new(200, 122, 255, 255);
pub const VIOLET: Color = Color::new(135, 60, 190, 255);
pub const DARKPURPLE: Color = Color::new(112, 31, 126, 255);
pub const BEIGE: Color = Color::new(211, 176, 131, 255);
pub const BROWN: Color = Color::new(127, 106, 79, 255);
pub const DARKBROWN: Color = Color::new(76, 63, 47, 255);
pub const WHITE: Color = Color::new(255, 255, 255, 255);
pub const BLACK: Color = Color::new(0, 0, 0, 255);
pub const BLANK: Color = Color::new(0, 0, 0, 0);
pub const MAGENTA: Color = Color::new(255, 0, 255, 255);
