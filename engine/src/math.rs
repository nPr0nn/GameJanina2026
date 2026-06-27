//! Math types for the engine.

pub use glam::Vec2 as Vec2D;

/// Coordinates are in virtual-canvas pixels with the origin at the top-left
/// corner and +Y pointing down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Top-left corner.
    pub fn position(&self) -> Vec2D {
        Vec2D::new(self.x, self.y)
    }

    /// Width/height as a vector.
    pub fn size(&self) -> Vec2D {
        Vec2D::new(self.width, self.height)
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.width
            && self.x + self.width > other.x
            && self.y < other.y + other.height
            && self.y + self.height > other.y
    }

}
