//! Color type and palette — re-exported from `level_format`.
//!
//! The `Color` struct and its palette constants live in `level_format` so the
//! editor can share them without depending on wgpu/bytemuck. This module
//! re-exports everything and adds the one wgpu-specific helper the renderer
//! needs internally.

pub use level_format::{
    Color, BEIGE, BLACK, BLANK, BLUE, BROWN, DARKBLUE, DARKBROWN, DARKGRAY, DARKGREEN, DARKPURPLE,
    GOLD, GRAY, GREEN, LIGHTGRAY, LIME, MAGENTA, MAROON, ORANGE, PINK, PURPLE, RED, SKYBLUE,
    VIOLET, WHITE, YELLOW,
};

/// Convert a `Color` to the `wgpu::Color` (linear f64) used as a render-pass
/// clear value. Only called by the renderer, so this stays `pub(crate)`.
pub(crate) fn to_wgpu(c: Color) -> wgpu::Color {
    let [r, g, b, a] = c.to_linear();
    wgpu::Color {
        r: r as f64,
        g: g as f64,
        b: b as f64,
        a: a as f64,
    }
}
