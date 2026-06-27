//! A raylib-style 2D camera.
//!
//! [`Camera2D`] maps world coordinates to canvas (screen) coordinates. Wrap
//! drawing in [`Canvas::begin_mode_2d`](crate::Canvas::begin_mode_2d) /
//! [`end_mode_2d`](crate::Canvas::end_mode_2d) and everything in between is
//! transformed by the camera. Because the transform is applied per vertex,
//! `zoom` scales line thickness, radii and texture sizes uniformly — just like
//! raylib.

use crate::math::Vec2D;

/// A 2D camera. World points are mapped to the screen as
/// `screen = rotate(world - target) * zoom + offset`.
#[derive(Debug, Clone, Copy)]
pub struct Camera2D {
    /// Where `target` lands on screen. Set to the screen center to keep the
    /// target centered.
    pub offset: Vec2D,
    /// World point the camera is centered on (the zoom/rotation origin).
    pub target: Vec2D,
    /// Rotation in degrees.
    pub rotation: f32,
    /// Scale factor; `1.0` is 1:1. Larger zooms in.
    pub zoom: f32,
}

impl Default for Camera2D {
    fn default() -> Self {
        Self {
            offset: Vec2D::ZERO,
            target: Vec2D::ZERO,
            rotation: 0.0,
            zoom: 1.0,
        }
    }
}

impl Camera2D {
    /// A camera that centers `target` on `offset` at the given `zoom`, no
    /// rotation.
    pub fn new(target: Vec2D, offset: Vec2D, zoom: f32) -> Self {
        Self {
            offset,
            target,
            rotation: 0.0,
            zoom,
        }
    }

    /// Map a world-space point to screen (canvas) space. Raylib's
    /// `GetWorldToScreen2D`.
    pub fn world_to_screen(&self, world: Vec2D) -> Vec2D {
        let p = world - self.target;
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let rotated = Vec2D::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
        rotated * self.zoom + self.offset
    }

    /// Map a screen-space point back to world space (the inverse of
    /// [`world_to_screen`](Self::world_to_screen)). Raylib's
    /// `GetScreenToWorld2D`. Useful for turning the cursor into world coords.
    pub fn screen_to_world(&self, screen: Vec2D) -> Vec2D {
        if self.zoom == 0.0 {
            return self.target;
        }
        let p = (screen - self.offset) / self.zoom;
        let (sin, cos) = (-self.rotation).to_radians().sin_cos();
        let rotated = Vec2D::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
        self.target + rotated
    }
}
