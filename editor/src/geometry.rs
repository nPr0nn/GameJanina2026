//! Pure rect/shape geometry helpers.

use juni::prelude::*;

use crate::id::random_id;
use crate::types::Tool;

/// Update an existing shape's geometry from two drag endpoints while
/// preserving its ID and color. The shape keeps its original type.
pub(crate) fn update_shape_geometry(shape: &mut Shape, a: Vec2D, b: Vec2D) -> bool {
    match shape {
        Shape::Rect {
            x,
            y,
            width,
            height,
            ..
        } => {
            let nx = a.x.min(b.x);
            let ny = a.y.min(b.y);
            let nw = (a.x - b.x).abs();
            let nh = (a.y - b.y).abs();
            if nw >= 2.0 && nh >= 2.0 {
                *x = nx;
                *y = ny;
                *width = nw;
                *height = nh;
                true
            } else {
                false
            }
        }
        Shape::Circle { x, y, radius, .. } => {
            let nr = a.distance(b);
            if nr >= 2.0 {
                *x = a.x;
                *y = a.y;
                *radius = nr;
                true
            } else {
                false
            }
        }
    }
}

/// Build a shape from two drag endpoints, assigning a fresh random ID.
pub(crate) fn make_shape(tool: Tool, a: Vec2D, b: Vec2D, color: Color) -> Option<Shape> {
    match tool {
        Tool::Rect => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let width = (a.x - b.x).abs();
            let height = (a.y - b.y).abs();
            (width >= 2.0 && height >= 2.0).then_some(Shape::Rect {
                id: random_id(),
                x,
                y,
                width,
                height,
                color,
            })
        }
        Tool::Circle => {
            let radius = a.distance(b);
            (radius >= 2.0).then_some(Shape::Circle {
                id: random_id(),
                x: a.x,
                y: a.y,
                radius,
                color,
            })
        }
    }
}

/// Build a normalized rectangle from two corner points.
pub(crate) fn rect_from_points(a: Vec2D, b: Vec2D) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let width = (a.x - b.x).abs();
    let height = (a.y - b.y).abs();
    Rect::new(x, y, width, height)
}

/// Clamp a rectangle to lie within `(0,0..max_w,max_h)` and have positive size.
pub(crate) fn clamp_rect(rect: Rect, max_w: u32, max_h: u32) -> Rect {
    let x = rect.x.clamp(0.0, max_w as f32);
    let y = rect.y.clamp(0.0, max_h as f32);
    let width = (rect.width.min(max_w as f32 - x)).max(1.0);
    let height = (rect.height.min(max_h as f32 - y)).max(1.0);
    Rect::new(x, y, width, height)
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_rect_keeps_selection_inside_image() {
        let rect = Rect::new(-5.0, -5.0, 100.0, 100.0);
        let clamped = clamp_rect(rect, 16, 16);
        assert_eq!(clamped.x, 0.0);
        assert_eq!(clamped.y, 0.0);
        assert_eq!(clamped.width, 16.0);
        assert_eq!(clamped.height, 16.0);
    }
}
