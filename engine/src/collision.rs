//! Lightweight 2D collision helpers.
//!
//! All functions work with axis-aligned [`Rect`]angles and [`Vec2D`] points.
//! They are intentionally simple (AABB and circle tests) and match the engine's
//! top-left origin, +Y-down coordinate system.

use crate::math::{Rect, Vec2D};

/// `true` if `a` and `b` overlap.
pub fn rect_rect(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width
        && a.x + a.width > b.x
        && a.y < b.y + b.height
        && a.y + a.height > b.y
}

/// `true` if `rect` contains `point` (edges count as inside).
pub fn rect_point(rect: &Rect, point: Vec2D) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}

/// `true` if `rect` and the circle (`center`, `radius`) overlap.
pub fn rect_circle(rect: &Rect, center: Vec2D, radius: f32) -> bool {
    let closest_x = center.x.clamp(rect.x, rect.x + rect.width);
    let closest_y = center.y.clamp(rect.y, rect.y + rect.height);
    let dx = center.x - closest_x;
    let dy = center.y - closest_y;
    dx * dx + dy * dy <= radius * radius
}

/// `true` if two circles overlap.
pub fn circle_circle(a: Vec2D, r_a: f32, b: Vec2D, r_b: f32) -> bool {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let r = r_a + r_b;
    dx * dx + dy * dy <= r * r
}

/// Move `rect` by `velocity` against a slice of solid rectangles.
///
/// Movement is resolved per-axis (X then Y), stopping the rectangle at the
/// nearest edge of any solid it would cross. This gives classic "slide along
/// walls" behaviour: a player walking into a vertical wall will continue to
/// slide up/down if those keys are held.
///
/// Solids are checked in order; the first overlap on each axis wins.
pub fn move_rect(rect: Rect, velocity: Vec2D, solids: &[Rect]) -> Rect {
    let mut result = rect;

    // Resolve X first.
    if velocity.x != 0.0 {
        result.x += velocity.x;
        for solid in solids {
            if rect_rect(&result, solid) {
                if velocity.x > 0.0 {
                    result.x = solid.x - rect.width;
                } else {
                    result.x = solid.x + solid.width;
                }
                break;
            }
        }
    }

    // Then resolve Y.
    if velocity.y != 0.0 {
        result.y += velocity.y;
        for solid in solids {
            if rect_rect(&result, solid) {
                if velocity.y > 0.0 {
                    result.y = solid.y - rect.height;
                } else {
                    result.y = solid.y + solid.height;
                }
                break;
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_rects() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert!(rect_rect(&a, &b));
    }

    #[test]
    fn separated_rects() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 10.0, 10.0);
        assert!(!rect_rect(&a, &b));
    }

    #[test]
    fn point_inside() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(rect_point(&r, Vec2D::new(5.0, 5.0)));
        assert!(rect_point(&r, Vec2D::new(0.0, 0.0)));
    }

    #[test]
    fn point_outside() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(!rect_point(&r, Vec2D::new(15.0, 5.0)));
    }

    #[test]
    fn rect_circle_overlap() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert!(rect_circle(&r, Vec2D::new(5.0, 5.0), 1.0));
        assert!(rect_circle(&r, Vec2D::new(15.0, 5.0), 6.0));
        assert!(!rect_circle(&r, Vec2D::new(15.0, 5.0), 4.0));
    }

    #[test]
    fn move_rect_stops_at_wall() {
        let mover = Rect::new(0.0, 0.0, 10.0, 10.0);
        let wall = Rect::new(20.0, 0.0, 10.0, 100.0);
        let resolved = move_rect(mover, Vec2D::new(25.0, 0.0), &[wall]);
        // Should snap to the left edge of the wall.
        assert_eq!(resolved.x, 20.0 - 10.0);
        assert_eq!(resolved.y, 0.0);
    }

    #[test]
    fn move_rect_slides_along_wall() {
        let mover = Rect::new(0.0, 0.0, 10.0, 10.0);
        let wall = Rect::new(15.0, 0.0, 20.0, 100.0);
        // X movement is blocked, but Y movement should still go through.
        let resolved = move_rect(mover, Vec2D::new(25.0, 25.0), &[wall]);
        assert_eq!(resolved.x, 15.0 - 10.0);
        assert_eq!(resolved.y, 25.0);
    }
}
