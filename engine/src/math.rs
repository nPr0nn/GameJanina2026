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


pub struct Circle {
    pub center: Vec2D,
    pub radius: f32,
}

impl Circle {
    pub fn new(center: Vec2D, radius: f32) -> Self {
        Self { center, radius }
    }

    pub fn intersects(&self, other: &Circle) -> bool {
        let distance_squared = (self.center - other.center).length_squared();
        let radius_sum = self.radius + other.radius;
        distance_squared < (radius_sum * radius_sum)
    }

    pub fn intersects_point(&self, point: Vec2D) -> bool {
        let distance_squared = (self.center - point).length_squared();
        distance_squared < (self.radius * self.radius)
    }
}