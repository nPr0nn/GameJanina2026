//! Level format — re-exported from `level_format`, plus Canvas drawing traits.
//!
//! The data types ([`Level`], [`Shape`], [`SpriteInstance`], [`Color`],
//! [`DEFAULT_LEVEL_PATH`]) all live in the `level_format` crate so that the
//! editor can share them without pulling in wgpu or bytemuck.
//!
//! This module re-exports those types and adds two extension traits that bind
//! them to the engine's [`Canvas`]:
//!
//! - [`ShapeDrawExt`] — adds `shape.draw(canvas)` to any `Shape`.
//! - [`LevelDrawExt`] — adds `level.draw(canvas)` to any `Level`.
//!
//! Both traits are included in [`crate::prelude`], so a bare
//! `use juni::prelude::*` is enough to call `level.draw(canvas)` or
//! `shape.draw(canvas)` without any additional imports.

pub use level_format::{
    Color, Level, Shape, SpriteInstance, Vec2D, BEIGE, BLACK, BLANK, BLUE, BROWN, DARKBLUE,
    DARKBROWN, DARKGRAY, DARKGREEN, DARKPURPLE, DEFAULT_LEVEL_PATH, GOLD, GRAY, GREEN, LIGHTGRAY,
    LIME, MAGENTA, MAROON, ORANGE, PINK, PURPLE, RED, SKYBLUE, VIOLET, WHITE, YELLOW,
};

use crate::canvas::Canvas;

// ---------------------------------------------------------------------------
// ShapeDrawExt
// ---------------------------------------------------------------------------

/// Draws a [`Shape`] into a [`Canvas`]. Included in `juni::prelude`.
///
/// The orphan rule prevents adding `draw` as an inherent method on [`Shape`]
/// from the engine crate, so it lives here as an extension trait. Import it
/// (or `use juni::prelude::*`) to call `shape.draw(canvas)`.
pub trait ShapeDrawExt {
    /// Draw this shape into `canvas`.
    fn draw(&self, canvas: &mut Canvas);
}

impl ShapeDrawExt for Shape {
    fn draw(&self, canvas: &mut Canvas) {
        match *self {
            Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            } => canvas.rectangle(x, y, width, height, color),
            Shape::Circle {
                x,
                y,
                radius,
                color,
            } => canvas.circle(Vec2D::new(x, y), radius, color),
        }
    }
}

// ---------------------------------------------------------------------------
// LevelDrawExt
// ---------------------------------------------------------------------------

/// Draws a [`Level`]'s sprite-planning layer into a [`Canvas`]. Included in
/// `juni::prelude`.
pub trait LevelDrawExt {
    /// Draw the sprite-planning layer shapes, in order.
    fn draw(&self, canvas: &mut Canvas);
}

impl LevelDrawExt for Level {
    fn draw(&self, canvas: &mut Canvas) {
        for shape in &self.sprite_shapes {
            shape.draw(canvas);
        }
    }
}
