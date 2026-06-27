//! A simple, serializable **level format** shared by the game and the editor.
//!
//! A [`Level`] is just a list of [`Shape`]s (rectangles and circles) in
//! virtual-canvas coordinates. The `editor` binary writes one out with
//! [`Level::save`]; the game reads it back with [`Level::load`] and renders it
//! with [`Level::draw`]. The on-disk form is plain JSON, so levels are
//! hand-editable and diff-friendly.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::math::Vec2D;
use serde::{Deserialize, Serialize};

/// Default level file path, relative to the working directory. Both `cargo run`
/// (the game) and `cargo run -p editor` run from the workspace root, so they
/// agree on this location out of the box.
pub const DEFAULT_LEVEL_PATH: &str = "level.json";

/// A single placed primitive. Coordinates are virtual-canvas pixels (the same
/// space you draw and click in), so what the editor shows is what the game gets.
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
    /// Draw this shape into `canvas`.
    pub fn draw(&self, canvas: &mut Canvas) {
        match *self {
            Shape::Rect { x, y, width, height, color } => canvas.rectangle(x, y, width, height, color),
            Shape::Circle { x, y, radius, color } => canvas.circle(Vec2D::new(x, y), radius, color),
        }
    }

    /// `true` if `p` (virtual-canvas pixels) lies inside the shape. Used by the
    /// editor for click-to-delete hit testing.
    pub fn contains(&self, p: Vec2D) -> bool {
        match *self {
            Shape::Rect { x, y, width, height, .. } => {
                p.x >= x && p.x <= x + width && p.y >= y && p.y <= y + height
            }
            Shape::Circle { x, y, radius, .. } => Vec2D::new(x, y).distance(p) <= radius,
        }
    }
}

/// An ordered collection of shapes. Later shapes draw on top of earlier ones.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Level {
    pub shapes: Vec<Shape>,
}

impl Level {
    /// An empty level.
    pub fn new() -> Self {
        Self::default()
    }

    /// Draw every shape, in order.
    pub fn draw(&self, canvas: &mut Canvas) {
        for shape in &self.shapes {
            shape.draw(canvas);
        }
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> String {
        // Both ends control the type, so this can't realistically fail.
        serde_json::to_string_pretty(self).unwrap_or_else(|_| "{\"shapes\":[]}".to_string())
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
