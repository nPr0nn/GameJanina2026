//! juni-`Canvas` drawing helpers (rewritten for egui in PR2).

use juni::prelude::*;

/// Draw a rectangle outline using four line segments.
pub(crate) fn draw_rect_outline(canvas: &mut Canvas, x: f32, y: f32, w: f32, h: f32, t: f32, c: Color) {
    canvas.line(Vec2D::new(x, y), Vec2D::new(x + w, y), t, c);
    canvas.line(Vec2D::new(x + w, y), Vec2D::new(x + w, y + h), t, c);
    canvas.line(Vec2D::new(x + w, y + h), Vec2D::new(x, y + h), t, c);
    canvas.line(Vec2D::new(x, y + h), Vec2D::new(x, y), t, c);
}
