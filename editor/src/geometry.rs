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
