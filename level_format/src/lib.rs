//! Shared level-format types used by both the `game` and `editor` crates.
//!
//! This crate is intentionally kept free of engine-specific dependencies
//! (wgpu, winit, bytemuck). Drawing methods that require a `Canvas` live in
//! the engine as extension traits ([`ShapeDrawExt`], [`LevelDrawExt`]) so that
//! this crate can be a lightweight shared dependency.
//!
//! [`ShapeDrawExt`]: <https://docs.rs/juni/latest/juni/trait.ShapeDrawExt.html>
//! [`LevelDrawExt`]: <https://docs.rs/juni/latest/juni/trait.LevelDrawExt.html>

use serde::{Deserialize, Serialize};

/// `Vec2D` is [`glam::Vec2`]; re-exported here so dependents don't need a
/// direct glam dependency just for [`Shape::contains`].
pub use glam::Vec2 as Vec2D;

/// Default level file path, relative to the working directory. Both `cargo run`
/// (the game) and `cargo run -p editor` run from the workspace root, so they
/// agree on this location out of the box.
pub const DEFAULT_LEVEL_PATH: &str = "level.json";

// ---------------------------------------------------------------------------
// Color
// ---------------------------------------------------------------------------

/// An RGBA color stored as 8 bits per channel (sRGB convention).
///
/// Colors are authored in sRGB. The engine's renderer converts them to linear
/// space before uploading to the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Raylib's `Fade`: `WHITE.with_alpha(0.5)` is 50%-transparent white.
    pub fn with_alpha(self, alpha: f32) -> Self {
        Self {
            a: (alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            ..self
        }
    }

    /// Convert to a linear-space `[f32; 4]` suitable for an sRGB render target.
    ///
    /// The GPU writes vertex colors into an sRGB texture, which applies the
    /// inverse transfer on display, so we convert from sRGB to linear here.
    pub fn to_linear(self) -> [f32; 4] {
        [
            srgb_to_linear(self.r),
            srgb_to_linear(self.g),
            srgb_to_linear(self.b),
            self.a as f32 / 255.0,
        ]
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

// ---------------------------------------------------------------------------
// Shape
// ---------------------------------------------------------------------------

/// A single placed primitive. Coordinates are world-space pixels (what the
/// camera looks at), so what the editor shows is what the game gets.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub enum Shape {
    /// Axis-aligned rectangle with top-left at `(x, y)`.
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
    },
    /// Circle centered at `(x, y)`.
    Circle {
        x: f32,
        y: f32,
        radius: f32,
        color: Color,
    },
}

impl Shape {
    /// Return a copy of this shape with its color alpha scaled to `alpha`.
    pub fn with_alpha(&self, alpha: f32) -> Self {
        match *self {
            Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            } => Shape::Rect {
                x,
                y,
                width,
                height,
                color: color.with_alpha(alpha),
            },
            Shape::Circle {
                x,
                y,
                radius,
                color,
            } => Shape::Circle {
                x,
                y,
                radius,
                color: color.with_alpha(alpha),
            },
        }
    }

    /// `true` if `p` (world-space pixels) lies inside the shape. Used by the
    /// editor for click-to-delete hit testing.
    pub fn contains(&self, p: Vec2D) -> bool {
        match *self {
            Shape::Rect {
                x,
                y,
                width,
                height,
                ..
            } => p.x >= x && p.x <= x + width && p.y >= y && p.y <= y + height,
            Shape::Circle { x, y, radius, .. } => Vec2D::new(x, y).distance(p) <= radius,
        }
    }
}

// ---------------------------------------------------------------------------
// SpriteInstance
// ---------------------------------------------------------------------------

/// A placed sprite image in the level, stored as a path + transform so it can
/// be serialized. The editor resolves the path to a GPU texture at runtime.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpriteInstance {
    /// Path to the PNG, relative to the workspace root (e.g. `"sprites/player.png"`).
    pub path: String,
    /// Top-left position in world-space pixels.
    pub x: f32,
    pub y: f32,
    /// Uniform scale factor (1.0 = native pixel size).
    pub scale: f32,
}

// ---------------------------------------------------------------------------
// Level
// ---------------------------------------------------------------------------

/// An ordered collection of planning layers. Later shapes within each layer
/// draw on top of earlier ones.
///
/// The on-disk form is plain JSON so levels are hand-editable and diff-friendly.
/// Use the engine's [`LevelDrawExt`] trait (re-exported in `juni::prelude`) to
/// render the sprite-planning layer.
///
/// [`LevelDrawExt`]: <https://docs.rs/juni/latest/juni/trait.LevelDrawExt.html>
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Level {
    #[serde(default, alias = "shapes")]
    pub sprite_shapes: Vec<Shape>,
    #[serde(default)]
    pub collision_shapes: Vec<Shape>,
    /// Sprite images placed in the sprite-planning layer.
    #[serde(default)]
    pub sprite_instances: Vec<SpriteInstance>,
}

impl Level {
    /// An empty level.
    pub fn new() -> Self {
        Self::default()
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> String {
        // Both ends control the type, so this can't realistically fail.
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|_| "{\"sprite_shapes\":[],\"collision_shapes\":[]}".to_string())
    }

    /// Parse from JSON text.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Read a level from a JSON file (native only — the web has no synchronous
    /// filesystem). On any IO/parse error an `Err` is returned so callers can
    /// fall back to an empty level.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text).map_err(std::io::Error::other)
    }

    /// Write this level to a JSON file (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, self.to_json())
    }
}
