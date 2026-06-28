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
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Collider {
    Aabb(Rect),
    Circle { center: Vec2D, radius: f32 },
}

impl Collider {
    /// Tight axis-aligned bounding box of this collider.
    pub fn bounds(&self) -> Rect {
        match *self {
            Collider::Aabb(r) => r,
            Collider::Circle { center, radius } => Rect::new(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ),
        }
    }

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

// ── Spatial index (quad tree) for broad-phase chain-world collision ─────────

/// Maximum recursion depth for the quad tree. Deeper trees cost more traversal
/// and allocation; shallower ones reject less empty space.
const QT_MAX_DEPTH: usize = 8;
/// Maximum item count before a leaf node is split into four children.
const QT_MAX_ITEMS: usize = 8;

#[derive(Clone, Debug)]
enum QtNode {
    Leaf { bounds: Rect, items: Vec<usize> },
    Branch { bounds: Rect, children: [Box<QtNode>; 4] },
}

impl QtNode {
    fn new_leaf(bounds: Rect) -> Self {
        QtNode::Leaf {
            bounds,
            items: Vec::new(),
        }
    }
}

/// A quad tree spatial index over a snapshot of [`Collider`]s.
///
/// Built once per frame from the current world colliders and queried by the
/// chain solver to narrow each swept point test to only nearby obstacles.
/// Colliders are owned as a cheap `Copy` snapshot so the tree has no lifetime
/// ties to the original slice.
#[derive(Debug)]
pub struct ColliderTree {
    root: QtNode,
    colliders: Vec<Collider>,
    /// Per-collider tag used to suppress duplicates when a collider's bounds
    /// intersect several leaves that all overlap a single query region.
    last_query: Vec<u32>,
    /// Incremented each time [`query`](Self::query) is called.
    next_query_id: u32,
}

impl ColliderTree {
    /// Build a tree from a slice of colliders. Empty slices produce an empty
    /// tree with a 1×1 placeholder bounds.
    pub fn new(colliders: &[Collider]) -> Self {
        let colliders: Vec<Collider> = colliders.to_vec();
        let bounds = if colliders.is_empty() {
            Rect::new(0.0, 0.0, 1.0, 1.0)
        } else {
            let mut min_x = f32::INFINITY;
            let mut min_y = f32::INFINITY;
            let mut max_x = f32::NEG_INFINITY;
            let mut max_y = f32::NEG_INFINITY;
            for c in &colliders {
                let b = c.bounds();
                min_x = min_x.min(b.x);
                min_y = min_y.min(b.y);
                max_x = max_x.max(b.x + b.width);
                max_y = max_y.max(b.y + b.height);
            }
            Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
        };

        let mut root = QtNode::new_leaf(bounds);
        for i in 0..colliders.len() {
            Self::insert(&mut root, &colliders, i, 0);
        }
        let last_query = vec![0u32; colliders.len()];
        Self {
            root,
            colliders,
            last_query,
            next_query_id: 0,
        }
    }

    /// Returns true if the tree contains no colliders.
    pub fn is_empty(&self) -> bool {
        self.colliders.is_empty()
    }

    /// Append every collider whose bounds intersect `area` to `out`.
    /// `out` is not cleared first — callers can reuse one buffer.
    /// Colliders that live in multiple leaves are returned only once.
    pub fn query(&mut self, area: Rect, out: &mut Vec<Collider>) {
        self.next_query_id += 1;
        if self.next_query_id == 0 {
            // Wraparound: reset all tags and start again at 1 (0 means unused).
            self.last_query.fill(0);
            self.next_query_id = 1;
        }
        let id = self.next_query_id;
        Self::query_node(
            &self.root,
            &self.colliders,
            area,
            out,
            id,
            &mut self.last_query,
        );
    }

    fn insert(node: &mut QtNode, colliders: &[Collider], idx: usize, depth: usize) {
        match node {
            QtNode::Leaf { bounds, items } => {
                if items.len() < QT_MAX_ITEMS || depth >= QT_MAX_DEPTH {
                    items.push(idx);
                    return;
                }

                // Split the leaf into four quadrants and redistribute items.
                let half_w = bounds.width * 0.5;
                let half_h = bounds.height * 0.5;
                let bx = bounds.x;
                let by = bounds.y;
                let mut children: [Box<QtNode>; 4] = [
                    Box::new(QtNode::new_leaf(Rect::new(bx, by, half_w, half_h))),
                    Box::new(QtNode::new_leaf(Rect::new(bx + half_w, by, half_w, half_h))),
                    Box::new(QtNode::new_leaf(Rect::new(bx, by + half_h, half_w, half_h))),
                    Box::new(QtNode::new_leaf(Rect::new(bx + half_w, by + half_h, half_w, half_h))),
                ];

                let old_items = std::mem::take(items);
                for &old_idx in &old_items {
                    Self::insert_into_children(&mut children, colliders, old_idx, depth + 1);
                }
                Self::insert_into_children(&mut children, colliders, idx, depth + 1);
                *node = QtNode::Branch { bounds: *bounds, children };
            }
            QtNode::Branch { children, .. } => {
                Self::insert_into_children(children, colliders, idx, depth + 1);
            }
        }
    }

    fn insert_into_children(
        children: &mut [Box<QtNode>; 4],
        colliders: &[Collider],
        idx: usize,
        depth: usize,
    ) {
        let b = colliders[idx].bounds();
        for child in children.iter_mut() {
            let cb = match child.as_ref() {
                QtNode::Leaf { bounds, .. } | QtNode::Branch { bounds, .. } => *bounds,
            };
            if b.intersects(&cb) {
                Self::insert(child, colliders, idx, depth);
            }
        }
    }

    fn query_node(
        node: &QtNode,
        colliders: &[Collider],
        area: Rect,
        out: &mut Vec<Collider>,
        query_id: u32,
        last_query: &mut [u32],
    ) {
        match node {
            QtNode::Leaf { bounds, items } => {
                if !area.intersects(bounds) {
                    return;
                }
                for &idx in items {
                    if last_query[idx] == query_id {
                        continue; // already emitted this query
                    }
                    last_query[idx] = query_id;
                    let c = colliders[idx];
                    if c.bounds().intersects(&area) {
                        out.push(c);
                    }
                }
            }
            QtNode::Branch { bounds, children } => {
                if !area.intersects(bounds) {
                    return;
                }
                for child in children.iter() {
                    Self::query_node(child, colliders, area, out, query_id, last_query);
                }
            }
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
///
/// `static_tree` is the pre-built index of immovable world geometry;
/// `dynamics` are the few moving objects (boxes, squeezables) scanned linearly.
/// `scratch` is a reusable buffer for the broad-phase query results; it is
/// cleared on entry.
pub fn move_point_swept(
    from: Vec2D,
    to: Vec2D,
    static_tree: &mut ColliderTree,
    dynamics: &[Collider],
    scratch: &mut Vec<Collider>,
) -> (Vec2D, bool) {
    const SKIN: f32 = 0.02; // keep the point a hair outside the surface
    let mut pos = from;
    let mut vel = to - from;
    let mut hit = false;

    for _ in 0..3 {
        if vel.length_squared() < 1e-10 {
            break;
        }

        // Broad phase: only colliders whose bounds overlap the remaining
        // swept segment. Re-query each iteration because sliding can redirect
        // the path into a different neighbourhood.
        scratch.clear();
        static_tree.query(segment_bounds(pos, pos + vel), scratch);

        let mut t_min = 1.0f32;
        let mut normal = Vec2D::ZERO;
        for c in scratch.iter().chain(dynamics.iter()) {
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

/// Un-stick a point from every nearby collider (static push-out).
///
/// `static_tree` is the pre-built index of immovable world geometry;
/// `dynamics` are the few moving objects scanned linearly.
/// `scratch` is a reusable buffer for the broad-phase query results; it is
/// cleared on entry.
pub fn push_point_out_of_all(
    mut pos: Vec2D,
    static_tree: &mut ColliderTree,
    dynamics: &[Collider],
    scratch: &mut Vec<Collider>,
) -> Vec2D {
    // Broad phase: only colliders whose bounds contain (or nearly contain) `pos`.
    scratch.clear();
    static_tree.query(point_query_bounds(pos), scratch);

    for c in scratch.iter().chain(dynamics.iter()) {
        if let Some((p, _)) = c.push_point(pos) {
            pos = p;
        }
    }
    pos
}

/// Tight bounds of the segment `a → b`.
fn segment_bounds(a: Vec2D, b: Vec2D) -> Rect {
    let min_x = a.x.min(b.x);
    let min_y = a.y.min(b.y);
    let max_x = a.x.max(b.x);
    let max_y = a.y.max(b.y);
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

/// Tiny bounds around a point so that any collider containing the point is
/// returned by the broad-phase query. A zero-area rect would fail the strict
/// `Rect::intersects` test for boundary contacts.
fn point_query_bounds(p: Vec2D) -> Rect {
    const EPS: f32 = 0.001;
    Rect::new(p.x - EPS, p.y - EPS, EPS * 2.0, EPS * 2.0)
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

    // Quad tree: only colliders overlapping the query area are returned.
    #[test]
    fn tree_query_filters_by_area() {
        let colliders = vec![
            Collider::Aabb(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Collider::Aabb(Rect::new(100.0, 100.0, 10.0, 10.0)),
        ];
        let mut tree = ColliderTree::new(&colliders);
        let mut out = Vec::new();
        tree.query(Rect::new(-5.0, -5.0, 20.0, 20.0), &mut out);
        assert_eq!(out.len(), 1, "expected only the first collider");
        assert_eq!(out[0], colliders[0]);
    }

    // Quad tree: a collider straddling a split boundary is returned once.
    #[test]
    fn tree_query_does_not_duplicate_straddling_colliders() {
        // A big collider forces the root bounds, a small one sits at the centre
        // and therefore lives in all four children after the first split.
        let colliders = vec![
            Collider::Aabb(Rect::new(0.0, 0.0, 100.0, 100.0)),
            Collider::Aabb(Rect::new(48.0, 48.0, 4.0, 4.0)),
        ];
        let mut tree = ColliderTree::new(&colliders);
        let mut out = Vec::new();
        tree.query(Rect::new(40.0, 40.0, 20.0, 20.0), &mut out);
        assert_eq!(out.len(), 2, "expected both colliders, not duplicates");
    }

    // Quad tree: move_point_swept produces the same result via the tree as the
    // old full-scan did for a simple scenario.
    #[test]
    fn tree_swept_point_matches_full_scan() {
        let colliders = vec![
            Collider::Aabb(Rect::new(10.0, -10.0, 10.0, 20.0)), // vertical wall
        ];
        let mut tree = ColliderTree::new(&colliders);
        let mut scratch = Vec::new();
        let (pos, hit) = move_point_swept(Vec2D::ZERO, Vec2D::new(30.0, 0.0), &mut tree, &[], &mut scratch);
        assert!(hit, "should hit the wall");
        assert!(
            pos.x < 10.0 && pos.x > 9.0,
            "expected to stop just before the wall, got {pos:?}"
        );
    }

    // Dynamics are scanned linearly and still block movement.
    #[test]
    fn swept_point_hits_dynamic_collider() {
        let mut tree = ColliderTree::new(&[]);
        let dynamics = vec![Collider::Aabb(Rect::new(10.0, -10.0, 10.0, 20.0))];
        let mut scratch = Vec::new();
        let (pos, hit) = move_point_swept(Vec2D::ZERO, Vec2D::new(30.0, 0.0), &mut tree, &dynamics, &mut scratch);
        assert!(hit, "should hit the dynamic collider");
        assert!(
            pos.x < 10.0 && pos.x > 9.0,
            "expected to stop just before the dynamic wall, got {pos:?}"
        );
    }
}
