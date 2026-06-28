// A general 2D collision toolkit. The gameplay screen currently only uses the
// swept-AABB resolver, but the point/circle/push helpers are kept as a library
// to build on, so unused entries here are expected.
#![allow(dead_code)]

use juni::prelude::*;

// ── Swept AABB ─────────────────────────────────────────────────────────────

/// Sweep a moving AABB (top-left `pos`, dimensions `size`, frame velocity `vel`)
/// against a static AABB `rect`.  Returns the first-contact time `t ∈ [0,1]`
/// and the outward surface normal, or `None` if there is no collision this frame.
///
/// Uses the Minkowski-difference expansion + ray-box intersection:
///   expand `rect` by `size` on every side → sweep the *corner point* of the
///   moving rect along `vel` through that enlarged rect.
pub fn sweep_rect(pos: Vec2D, size: Vec2D, vel: Vec2D, rect: Rect) -> Option<(f32, Vec2D)> {
    // Expanded target the corner point must enter for an overlap.
    let ex = rect.x - size.x;
    let ey = rect.y - size.y;
    let ew = rect.width + size.x;
    let eh = rect.height + size.y;

    let inv_vx = if vel.x.abs() > 1e-6 { 1.0 / vel.x } else { f32::INFINITY };
    let inv_vy = if vel.y.abs() > 1e-6 { 1.0 / vel.y } else { f32::INFINITY };

    let (tx0, tx1) = if vel.x >= 0.0 {
        ((ex - pos.x) * inv_vx, (ex + ew - pos.x) * inv_vx)
    } else {
        ((ex + ew - pos.x) * inv_vx, (ex - pos.x) * inv_vx)
    };
    let (ty0, ty1) = if vel.y >= 0.0 {
        ((ey - pos.y) * inv_vy, (ey + eh - pos.y) * inv_vy)
    } else {
        ((ey + eh - pos.y) * inv_vy, (ey - pos.y) * inv_vy)
    };

    let t_entry = tx0.max(ty0);
    let t_exit  = tx1.min(ty1);

    if t_entry >= t_exit || t_entry >= 1.0 || t_exit <= 0.0 {
        return None;
    }

    let t = t_entry.max(0.0);
    let normal = if tx0 > ty0 {
        Vec2D::new(if vel.x < 0.0 { 1.0 } else { -1.0 }, 0.0)
    } else {
        Vec2D::new(0.0, if vel.y < 0.0 { 1.0 } else { -1.0 })
    };
    Some((t, normal))
}

/// Move a rect (`pos`, `size`) by `vel` against all `obstacles`, resolving
/// collisions with sliding (up to 3 bounces/steps per frame).
///
/// Returns the final top-left position.
pub fn resolve_swept(mut pos: Vec2D, size: Vec2D, mut vel: Vec2D, obstacles: &[Rect]) -> Vec2D {
    const EPS: f32 = 0.001;
    for _ in 0..3 {
        if vel.length_squared() < 1e-6 {
            break;
        }
        // Find earliest collision across all obstacles.
        let mut t_min = 1.0f32;
        let mut hit_normal = Vec2D::ZERO;
        for &rect in obstacles {
            if let Some((t, n)) = sweep_rect(pos, size, vel, rect) {
                if t < t_min {
                    t_min = t;
                    hit_normal = n;
                }
            }
        }

        // Advance to contact (back off by epsilon to avoid sticking).
        pos += vel * (t_min - EPS).max(0.0);

        if t_min < 1.0 {
            // Slide: project remaining velocity onto the surface plane.
            let remaining = (1.0 - t_min) * vel;
            let dot = remaining.dot(hit_normal);
            vel = remaining - hit_normal * dot;
        } else {
            break;
        }
    }
    pos
}

// ── Generic colliders (the chain wraps around any of these) ─────────────────

/// A static collision shape the chain solver resolves joints against.
///
/// Both variants support a swept *point* query (used by the chain) and a static
/// point push-out (used for the once-per-frame "un-stick" safety).  Adding a new
/// obstacle shape only requires extending these two methods — every consumer
/// (chain joints, squeeze detection) works through `Collider` and needs no
/// changes.
#[derive(Clone, Copy, Debug)]
pub enum Collider {
    Aabb(Rect),
    Circle { center: Vec2D, radius: f32 },
}

impl Collider {
    /// Swept point query: cast `pos` along `vel` (one frame) against this shape.
    /// Returns first-contact time `t ∈ [0,1]` and the outward surface normal.
    fn sweep_point(&self, pos: Vec2D, vel: Vec2D) -> Option<(f32, Vec2D)> {
        match *self {
            Collider::Aabb(rect) => sweep_point_aabb(pos, vel, rect),
            Collider::Circle { center, radius } => sweep_point_circle(pos, vel, center, radius),
        }
    }

    /// Static push-out for a point that has ended up inside the shape.
    fn push_point(&self, pos: Vec2D) -> Option<(Vec2D, Vec2D)> {
        match *self {
            Collider::Aabb(rect) => push_point_out_of_aabb(pos, rect),
            Collider::Circle { center, radius } => push_point_out_of_circle(pos, center, radius),
        }
    }
}

/// Swept **point** vs static AABB.  Returns first-contact time `t ∈ [0,1]` and
/// the outward face normal, or `None` if the segment `pos → pos+vel` misses.
fn sweep_point_aabb(pos: Vec2D, vel: Vec2D, rect: Rect) -> Option<(f32, Vec2D)> {
    let r = rect.x + rect.width;
    let b = rect.y + rect.height;
    let inv_x = if vel.x.abs() > 1e-8 { 1.0 / vel.x } else { f32::INFINITY };
    let inv_y = if vel.y.abs() > 1e-8 { 1.0 / vel.y } else { f32::INFINITY };

    let (tx0, tx1) = if vel.x >= 0.0 {
        ((rect.x - pos.x) * inv_x, (r - pos.x) * inv_x)
    } else {
        ((r - pos.x) * inv_x, (rect.x - pos.x) * inv_x)
    };
    let (ty0, ty1) = if vel.y >= 0.0 {
        ((rect.y - pos.y) * inv_y, (b - pos.y) * inv_y)
    } else {
        ((b - pos.y) * inv_y, (rect.y - pos.y) * inv_y)
    };

    let t_entry = tx0.max(ty0);
    let t_exit = tx1.min(ty1);
    if t_entry >= t_exit || t_entry >= 1.0 || t_exit <= 0.0 {
        return None;
    }
    let t = t_entry.max(0.0);
    let normal = if tx0 > ty0 {
        Vec2D::new(if vel.x < 0.0 { 1.0 } else { -1.0 }, 0.0)
    } else {
        Vec2D::new(0.0, if vel.y < 0.0 { 1.0 } else { -1.0 })
    };
    Some((t, normal))
}

/// Swept **point** vs static circle (ray–circle, entering from outside).
fn sweep_point_circle(pos: Vec2D, vel: Vec2D, center: Vec2D, radius: f32) -> Option<(f32, Vec2D)> {
    let d = pos - center;
    let a = vel.dot(vel);
    if a < 1e-12 {
        return None;
    }
    let c = d.dot(d) - radius * radius;
    // Already inside (shouldn't happen under the invariant) → immediate radial contact.
    if c < 0.0 {
        let n = d.try_normalize().unwrap_or(Vec2D::Y);
        return Some((0.0, n));
    }
    let b = 2.0 * d.dot(vel);
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / (2.0 * a); // earliest (entry) root
    if !(0.0..=1.0).contains(&t) {
        return None;
    }
    let n = (pos + vel * t - center).try_normalize().unwrap_or(Vec2D::Y);
    Some((t, n))
}

/// Move a **point** from `from` toward `to`, but never let it pass through any
/// collider.  On contact it stops at the surface and slides the remaining
/// motion along it (up to 3 surfaces per call).
///
/// Returns `(final_pos, hit)` where `hit` is true if any collider blocked the
/// move.  Provided `from` is outside every collider, `final_pos` is guaranteed
/// to be outside every collider as well — this is the invariant the chain
/// solver relies on to make joint teleporting impossible.
pub fn move_point_swept(from: Vec2D, to: Vec2D, colliders: &[Collider]) -> (Vec2D, bool) {
    const SKIN: f32 = 0.02; // keep the point a hair outside the surface
    let mut pos = from;
    let mut vel = to - from;
    let mut hit = false;

    for _ in 0..3 {
        if vel.length_squared() < 1e-10 {
            break;
        }
        let mut t_min = 1.0f32;
        let mut normal = Vec2D::ZERO;
        for c in colliders {
            if let Some((t, n)) = c.sweep_point(pos, vel) {
                if t < t_min {
                    t_min = t;
                    normal = n;
                }
            }
        }
        pos += vel * t_min;
        if t_min < 1.0 {
            hit = true;
            pos += normal * SKIN; // nudge clear of the surface
            // Slide: drop the velocity component into the surface.
            let remaining = vel * (1.0 - t_min);
            vel = remaining - normal * remaining.dot(normal);
        } else {
            break;
        }
    }
    (pos, hit)
}

/// Un-stick a point from every collider in `colliders` (static push-out).
pub fn push_point_out_of_all(mut pos: Vec2D, colliders: &[Collider]) -> Vec2D {
    for c in colliders {
        if let Some((p, _)) = c.push_point(pos) {
            pos = p;
        }
    }
    pos
}

// ── Static push-out (for constraint corrections, not velocity movement) ─────

const PUSH_EPS: f32 = 0.1;

/// Push a **point** out of an AABB.  Returns `(new_pos, outward_normal)`, or
/// `None` when the point is already outside.
///
/// Used inside the chain constraint solver where movements are small (< 1 px
/// per step) and swept tests are not needed.
pub fn push_point_out_of_aabb(pos: Vec2D, rect: Rect) -> Option<(Vec2D, Vec2D)> {
    let r = rect.x + rect.width;
    let b = rect.y + rect.height;
    if pos.x <= rect.x || pos.x >= r || pos.y <= rect.y || pos.y >= b {
        return None;
    }
    let dl = pos.x - rect.x;
    let dr = r - pos.x;
    let dt = pos.y - rect.y;
    let db = b - pos.y;
    Some(if dl <= dr && dl <= dt && dl <= db {
        (Vec2D::new(rect.x - PUSH_EPS, pos.y), Vec2D::new(-1.0, 0.0))
    } else if dr <= dl && dr <= dt && dr <= db {
        (Vec2D::new(r + PUSH_EPS, pos.y), Vec2D::new(1.0, 0.0))
    } else if dt <= db {
        (Vec2D::new(pos.x, rect.y - PUSH_EPS), Vec2D::new(0.0, -1.0))
    } else {
        (Vec2D::new(pos.x, b + PUSH_EPS), Vec2D::new(0.0, 1.0))
    })
}

/// Push a **point** out of a circle, radially.  Returns `(new_pos, normal)` or
/// `None` when the point is already outside.
pub fn push_point_out_of_circle(pos: Vec2D, center: Vec2D, radius: f32) -> Option<(Vec2D, Vec2D)> {
    let d = pos - center;
    let dist_sq = d.length_squared();
    if dist_sq >= radius * radius {
        return None;
    }
    let dist = dist_sq.sqrt();
    let n = if dist > 1e-6 { d / dist } else { Vec2D::Y };
    Some((center + n * (radius + PUSH_EPS), n))
}

/// Push an axis-aligned **rectangle** (`pos` = top-left, `size`) out of a circle.
///
/// Used for the player vs. round objects.  The player moves only a few pixels
/// per frame, far less than an object radius, so a discrete closest-point
/// push-out is robust enough here (no sweep needed).
pub fn push_rect_out_of_circle(
    pos: Vec2D,
    size: Vec2D,
    center: Vec2D,
    radius: f32,
) -> Option<(Vec2D, Vec2D)> {
    // Closest point on the rectangle to the circle centre.
    let cx = center.x.clamp(pos.x, pos.x + size.x);
    let cy = center.y.clamp(pos.y, pos.y + size.y);
    let d = Vec2D::new(cx, cy) - center;
    let dist_sq = d.length_squared();
    if dist_sq >= radius * radius {
        return None;
    }
    if dist_sq > 1e-6 {
        // Centre outside the rect: push along centre→closest-point.
        let dist = dist_sq.sqrt();
        let n = d / dist;
        return Some((pos + n * (radius - dist + PUSH_EPS), n));
    }
    // Centre inside the rect: escape through the nearest edge.
    let dl = center.x - pos.x;
    let dr = (pos.x + size.x) - center.x;
    let dt = center.y - pos.y;
    let db = (pos.y + size.y) - center.y;
    let m = dl.min(dr).min(dt).min(db);
    let (n, push) = if m == dl {
        (Vec2D::new(-1.0, 0.0), dl + radius)
    } else if m == dr {
        (Vec2D::new(1.0, 0.0), dr + radius)
    } else if m == dt {
        (Vec2D::new(0.0, -1.0), dt + radius)
    } else {
        (Vec2D::new(0.0, 1.0), db + radius)
    };
    Some((pos + n * (push + PUSH_EPS), n))
}

/// Push an axis-aligned **rectangle** (`pos` = top-left, `size`) out of an AABB.
///
/// Used for small post-constraint position corrections where swept detection
/// is unnecessary.  Returns `(new_pos, outward_normal)` or `None`.
pub fn push_rect_out_of_aabb(pos: Vec2D, size: Vec2D, rect: Rect) -> Option<(Vec2D, Vec2D)> {
    let pr = pos.x + size.x;
    let pb = pos.y + size.y;
    let or_ = rect.x + rect.width;
    let ob  = rect.y + rect.height;
    let ox = pr.min(or_) - pos.x.max(rect.x);
    let oy = pb.min(ob)  - pos.y.max(rect.y);
    if ox <= 0.0 || oy <= 0.0 {
        return None;
    }
    Some(if ox < oy {
        if pos.x < rect.x {
            (Vec2D::new(rect.x - size.x - PUSH_EPS, pos.y), Vec2D::new(-1.0, 0.0))
        } else {
            (Vec2D::new(or_ + PUSH_EPS, pos.y), Vec2D::new(1.0, 0.0))
        }
    } else if pos.y < rect.y {
        (Vec2D::new(pos.x, rect.y - size.y - PUSH_EPS), Vec2D::new(0.0, -1.0))
    } else {
        (Vec2D::new(pos.x, ob + PUSH_EPS), Vec2D::new(0.0, 1.0))
    })
}
