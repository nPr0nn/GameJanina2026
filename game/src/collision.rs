// A general 2D collision toolkit, built around continuous (swept) resolution.
//
// The player moves through [`resolve_aabb`], which sweeps an AABB against a set
// of [`Collider`]s (rectangles and circles alike) and slides along the first
// surface it touches, so fast movers never tunnel. [`depenetrate_aabb`] is the
// static safety net run afterwards. The chain uses the point-based swept queries
// ([`move_point_swept`]). Some helpers are kept as a library to build on, so
// unused entries here are expected.
#![allow(dead_code)]

use juni::prelude::*;

// ── Swept AABB ─────────────────────────────────────────────────────────────

/// Below this speed an axis counts as *parallel* to its slabs: the swept query
/// can't divide by it, so it falls back to a containment test instead of the
/// `1.0 / vel` reciprocal that would otherwise be `±∞`.
const PARALLEL_EPS: f32 = 1e-8;

/// Entry/exit times `(t0, t1)` for one axis of a swept ray against the slab
/// `[lo, hi]` (with `lo < hi`). `origin` is the ray's start on that axis and
/// `vel` its displacement over the frame.
///
/// The reciprocal `1.0 / vel` blows up to `±∞` as `vel → 0`, and `0.0 * ∞`
/// is `NaN` — which is exactly what happens when a mover travels *parallel* to
/// a slab while one of its edges sits *exactly* on the slab boundary (the
/// aligned-box case). A stray `NaN` then slips through `min`/`max` and silently
/// erases this axis's constraint, so a box that merely *touches* a neighbour
/// gets treated as overlapping it and snags.
///
/// To stay robust we special-case the parallel axis explicitly:
/// - `vel ≈ 0` and `origin` **strictly inside** `(lo, hi)`: the axis never
///   constrains the sweep → `(-∞, +∞)`.
/// - `vel ≈ 0` and `origin` on or outside the boundary: contact is impossible
///   this frame → `None`, which short-circuits the whole sweep. Treating exact
///   edge alignment as *outside* (the comparisons are strict) is what lets two
///   flush-aligned boxes slide past each other instead of catching.
fn slab(origin: f32, vel: f32, lo: f32, hi: f32) -> Option<(f32, f32)> {
    if vel.abs() <= PARALLEL_EPS {
        return if origin > lo && origin < hi {
            Some((f32::NEG_INFINITY, f32::INFINITY))
        } else {
            None
        };
    }
    let inv = 1.0 / vel;
    let t_lo = (lo - origin) * inv;
    let t_hi = (hi - origin) * inv;
    // `inv` flips the ordering when `vel < 0`; sort so `t0` is always entry.
    Some((t_lo.min(t_hi), t_lo.max(t_hi)))
}

/// Combine per-axis slab spans into a first-contact `(t, normal)`, or `None` if
/// the spans never overlap within `[0, 1]`. Shared by the box- and point-sweep
/// against an AABB — both reduce to intersecting two 1-D intervals.
fn slab_hit(
    (tx0, tx1): (f32, f32),
    (ty0, ty1): (f32, f32),
    vel: Vec2D,
) -> Option<(f32, Vec2D)> {
    let t_entry = tx0.max(ty0);
    let t_exit = tx1.min(ty1);
    if t_entry >= t_exit || t_entry >= 1.0 || t_exit <= 0.0 {
        return None;
    }
    let t = t_entry.max(0.0);
    // The axis that entered *last* is the one actually struck.
    let normal = if tx0 > ty0 {
        Vec2D::new(if vel.x < 0.0 { 1.0 } else { -1.0 }, 0.0)
    } else {
        Vec2D::new(0.0, if vel.y < 0.0 { 1.0 } else { -1.0 })
    };
    Some((t, normal))
}

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

    let tx = slab(pos.x, vel.x, ex, ex + ew)?;
    let ty = slab(pos.y, vel.y, ey, ey + eh)?;
    slab_hit(tx, ty, vel)
}

// ── Continuous AABB resolution (the player's movement phase) ────────────────

/// Maximum slide iterations per movement step. Each iteration resolves one
/// surface, so this caps how many corners a single move can wrap around.
const MAX_SLIDES: usize = 4;
/// Perpendicular clearance kept between the mover and a surface after a contact,
/// so the next sweep starts cleanly outside the surface's slab. Stepping off
/// along the *normal* (not the travel direction) keeps this clearance true even
/// when grazing a wall at a shallow angle — otherwise the box drifts inside the
/// perpendicular slab and the sweep misreads which axis it hit, snagging it.
const SKIN: f32 = 0.1;

/// Move an axis-aligned box (top-left `pos`, dimensions `size`) by `vel` against
/// every [`Collider`] using **continuous collision detection**: each step sweeps
/// to the first time-of-impact, stops just short of the surface, then slides the
/// remaining motion along it. Because contact is found by sweeping (not by
/// testing the destination), the box cannot tunnel through thin or small shapes
/// no matter how fast it moves.
///
/// Returns the final top-left position. `vel` is the intended displacement for
/// the whole step (already multiplied by `dt`).
pub fn resolve_aabb(mut pos: Vec2D, size: Vec2D, mut vel: Vec2D, colliders: &[Collider]) -> Vec2D {
    for _ in 0..MAX_SLIDES {
        if vel.length_squared() < 1e-8 {
            break;
        }
        // Broad sweep: earliest time-of-impact across every collider.
        let mut t_min = 1.0f32;
        let mut normal = Vec2D::ZERO;
        let mut hit = false;
        for c in colliders {
            if let Some((t, n)) = c.sweep_aabb(pos, size, vel) {
                if t < t_min {
                    t_min = t;
                    normal = n;
                    hit = true;
                }
            }
        }

        if !hit {
            pos += vel;
            break;
        }

        // Orient the contact normal to oppose the motion (point back at the
        // mover). The swept queries don't promise a consistent sign, so we fix
        // it here: stepping off along this normal always grows the perpendicular
        // clearance rather than shrinking it.
        let n = if normal.dot(vel) > 0.0 { -normal } else { normal };

        // Advance to the contact point, then lift off the surface by SKIN *along
        // the normal* so the next sweep begins outside the obstacle's slab.
        pos += vel * t_min + n * SKIN;

        // Slide: drop the into-surface component and continue with the remainder.
        let remaining = vel * (1.0 - t_min);
        vel = remaining - n * remaining.dot(n);
    }
    pos
}

/// Static depenetration pass: nudge the box out of any collider it currently
/// overlaps. The continuous resolver keeps the box outside during motion, so
/// this is only a safety net for edge cases (spawning inside a shape, a shape
/// growing into the box, float drift). Run once after the movement phase.
pub fn depenetrate_aabb(mut pos: Vec2D, size: Vec2D, colliders: &[Collider]) -> Vec2D {
    for c in colliders {
        let push = match *c {
            Collider::Aabb(rect) => push_rect_out_of_aabb(pos, size, rect),
            Collider::Circle { center, radius } => {
                push_rect_out_of_circle(pos, size, center, radius)
            }
        };
        if let Some((new_pos, _)) = push {
            pos = new_pos;
        }
    }
    pos
}

/// Swept **box** vs static circle, via the Minkowski sum of the box and the
/// disk: a rectangle rounded by `radius`. In the box's centre frame the circle
/// centre becomes a moving point, tested against that rounded rectangle — the
/// union of an x-expanded box, a y-expanded box, and four corner circles. The
/// earliest entry into any piece is the earliest contact with the whole shape.
fn sweep_aabb_circle(
    pos: Vec2D,
    size: Vec2D,
    vel: Vec2D,
    center: Vec2D,
    radius: f32,
) -> Option<(f32, Vec2D)> {
    let half = size * 0.5;
    let box_center = pos + half;
    // Circle centre relative to the box centre, and its motion in that frame.
    let origin = center - box_center;
    let dir = -vel;

    let candidates = [
        // Box expanded by the radius along each axis (the flat-face contacts).
        sweep_point_aabb(
            origin,
            dir,
            Rect::new(-(half.x + radius), -half.y, 2.0 * (half.x + radius), 2.0 * half.y),
        ),
        sweep_point_aabb(
            origin,
            dir,
            Rect::new(-half.x, -(half.y + radius), 2.0 * half.x, 2.0 * (half.y + radius)),
        ),
        // The four rounded corners.
        sweep_point_circle(origin, dir, Vec2D::new(-half.x, -half.y), radius),
        sweep_point_circle(origin, dir, Vec2D::new(-half.x, half.y), radius),
        sweep_point_circle(origin, dir, Vec2D::new(half.x, -half.y), radius),
        sweep_point_circle(origin, dir, Vec2D::new(half.x, half.y), radius),
    ];

    // Earliest entry into any piece is the earliest contact with the union.
    candidates
        .into_iter()
        .flatten()
        .min_by(|a, b| a.0.total_cmp(&b.0))
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
    /// Swept **box** query: sweep an AABB (top-left `pos`, dimensions `size`)
    /// along `vel` against this shape. Returns first-contact time `t ∈ [0,1]`
    /// and a contact normal (orientation unspecified — used only for sliding).
    fn sweep_aabb(&self, pos: Vec2D, size: Vec2D, vel: Vec2D) -> Option<(f32, Vec2D)> {
        match *self {
            Collider::Aabb(rect) => sweep_rect(pos, size, vel, rect),
            Collider::Circle { center, radius } => {
                sweep_aabb_circle(pos, size, vel, center, radius)
            }
        }
    }

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
    // A point is the zero-size case of the box sweep, so it shares the same
    // NaN-safe slab math (see [`slab`]) — vital when the point travels along a
    // wall it's flush against.
    let tx = slab(pos.x, vel.x, rect.x, rect.x + rect.width)?;
    let ty = slab(pos.y, vel.y, rect.y, rect.y + rect.height)?;
    slab_hit(tx, ty, vel)
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

#[cfg(test)]
mod tests {
    use super::*;

    // A box sliding along +x, resting flush on top of a static box (its bottom
    // edge exactly equal to the static's top edge), must NOT register a contact:
    // the y-axis is parallel and edge-aligned, the classic `0 * ∞ = NaN` trap.
    #[test]
    fn flush_aligned_parallel_edge_does_not_snag() {
        let size = Vec2D::new(32.0, 32.0);
        // Static box top at y = 100; the mover rests exactly on it (bottom = 100).
        let stat = Rect::new(200.0, 100.0, 32.0, 32.0);
        let mover_pos = Vec2D::new(160.0, 100.0 - 32.0); // bottom edge == stat top
        // Push straight right toward (but not into) the static box.
        let hit = sweep_rect(mover_pos, size, Vec2D::new(8.0, 0.0), stat);
        assert!(hit.is_none(), "edge-aligned slide reported a phantom hit: {hit:?}");
    }

    // The same head-on contact on the *moving* axis must still block: a box
    // pushed +x whose right edge is flush with the static's left edge, fully
    // overlapping in y, collides immediately (t == 0).
    #[test]
    fn flush_face_contact_on_moving_axis_blocks() {
        let size = Vec2D::new(32.0, 32.0);
        let stat = Rect::new(200.0, 100.0, 32.0, 32.0);
        let mover_pos = Vec2D::new(200.0 - 32.0, 100.0); // right edge == stat left
        let hit = sweep_rect(mover_pos, size, Vec2D::new(8.0, 0.0), stat);
        let (t, n) = hit.expect("flush face contact should block");
        assert!(t.abs() < 1e-6, "expected immediate contact, got t = {t}");
        assert_eq!(n.x.signum(), -1.0, "normal should oppose +x motion");
    }

    // Sanity: no NaN escapes for a zero-velocity axis with an interior overlap.
    #[test]
    fn parallel_axis_inside_slab_is_unconstrained() {
        let stat = Rect::new(0.0, 0.0, 10.0, 10.0);
        // Point already inside the x-span, drifting down through the top edge.
        let hit = sweep_point_aabb(Vec2D::new(5.0, -4.0), Vec2D::new(0.0, 8.0), stat);
        let (t, _) = hit.expect("vertical entry should hit");
        assert!(t.is_finite() && (0.0..=1.0).contains(&t));
    }
}
