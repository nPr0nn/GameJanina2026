//! Squeezable objects: round things a chain can lasso.  When a chain winds a
//! full loop around one and cinches it tight (a full loop "plus a little
//! force"), the object is crushed and disappears — and every registered
//! listener is notified through a [`SqueezeEvent`].

use std::f32::consts::{PI, TAU};

use juni::prelude::*;

use crate::chain::Chain;
use crate::collision::Collider;

/// Emitted the moment a squeezable is crushed.  Anyone interested registers a
/// listener via [`Squeezables::on_squeeze`].
#[derive(Clone, Copy, Debug)]
#[allow(dead_code)] // public payload consumed by external listeners
pub struct SqueezeEvent {
    pub id: u32,
    pub pos: Vec2D,
    pub radius: f32,
}

/// Boxed listener callback.  `FnMut` so a listener may hold mutable state (a
/// score counter, an effect spawner, …).
type Listener = Box<dyn FnMut(&SqueezeEvent)>;

/// A single round object.
struct Squeezable {
    id: u32,
    pos: Vec2D,
    radius: f32,
    alive: bool,
    /// Group this object belongs to, if any.  Objects sharing a group id are
    /// only crushed when *all* of the group's living members are cinched in the
    /// same frame.  Ungrouped objects (`None`) crush individually.
    group: Option<u32>,
}

/// Manager owning the squeezable objects and the squeeze-event listeners.
pub struct Squeezables {
    items: Vec<Squeezable>,
    listeners: Vec<Listener>,
    next_id: u32,
    next_group_id: u32,
}

// ── Tuning ──────────────────────────────────────────────────────────────────

/// A squeeze needs at least one full revolution of winding around the object.
const FULL_LOOP_TURNS: f32 = 1.0;
/// Inner/outer radius factors for picking joints that belong to the loop.
const LOOP_INNER_FACTOR: f32 = 0.95;
const LOOP_OUTER_FACTOR: f32 = 1.10;
/// Maximum allowed area difference between the chain loop and the object.
const MAX_AREA_DIFF_RATIO: f32 = 0.07;
/// Minimum overall chain stretch (path length / max length); proves force is
/// being applied to pull the loop tight.
const MIN_CHAIN_STRETCH: f32 = 0.999;

impl Default for Squeezables {
    fn default() -> Self {
        Self::new()
    }
}

impl Squeezables {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            listeners: Vec::new(),
            next_id: 0,
            next_group_id: 0,
        }
    }

    /// Add a standalone round object at `pos` and return its id.  It is crushed
    /// the moment a chain cinches tight around it.
    pub fn spawn(&mut self, pos: Vec2D, radius: f32) -> u32 {
        self.spawn_in_group(pos, radius, None)
    }

    /// Add a group of round objects that share a fate: none of them is crushed
    /// until a chain is simultaneously cinched around *every* member.  Each
    /// entry is a `(position, radius)` pair.  Returns the new group's id.
    pub fn spawn_group(&mut self, objects: &[(Vec2D, f32)]) -> u32 {
        let group = self.next_group_id;
        self.next_group_id += 1;
        for &(pos, radius) in objects {
            self.spawn_in_group(pos, radius, Some(group));
        }
        group
    }

    /// Shared spawn path: push one object with an optional group, return its id.
    fn spawn_in_group(&mut self, pos: Vec2D, radius: f32, group: Option<u32>) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Squeezable {
            id,
            pos,
            radius,
            alive: true,
            group,
        });
        id
    }

    /// Register a listener invoked synchronously whenever an object is squeezed.
    pub fn on_squeeze(&mut self, listener: impl FnMut(&SqueezeEvent) + 'static) {
        self.listeners.push(Box::new(listener));
    }

    /// Bring every object back to life (for a fresh run).  Listeners are kept.
    pub fn revive_all(&mut self) {
        for s in &mut self.items {
            s.alive = true;
        }
    }

    /// Append a [`Collider`] for each living object so the chains wrap them.
    pub fn extend_colliders(&self, out: &mut Vec<Collider>) {
        for s in &self.items {
            if s.alive {
                out.push(Collider::Circle {
                    center: s.pos,
                    radius: s.radius,
                });
            }
        }
    }

    /// Position + radius of each living object (for drawing and player push-out).
    pub fn alive(&self) -> impl Iterator<Item = (Vec2D, f32)> + '_ {
        self.items
            .iter()
            .filter(|s| s.alive)
            .map(|s| (s.pos, s.radius))
    }

    /// Check every living object against every chain; crush the ones a chain has
    /// looped tight, and fire the registered listeners for each crush.
    ///
    /// Call once per frame, after the chains have been simulated.
    pub fn update(&mut self, chains: &[Chain]) {
        // 1. Snapshot which living object each chain is cinching this frame.
        let cinched: Vec<bool> = self
            .items
            .iter()
            .map(|s| s.alive && chains.iter().any(|c| chain_cinches(c, s.pos, s.radius)))
            .collect();

        // 2. Decide who is crushed. An ungrouped object crushes on its own; a
        //    grouped object only crushes when every still-living member of its
        //    group is cinched in this same frame. Collect indices first so the
        //    group lookups can borrow `self.items` immutably.
        let to_crush: Vec<usize> = (0..self.items.len())
            .filter(|&i| {
                let s = &self.items[i];
                if !s.alive {
                    return false;
                }
                match s.group {
                    None => cinched[i],
                    Some(g) => self
                        .items
                        .iter()
                        .enumerate()
                        .filter(|(_, o)| o.alive && o.group == Some(g))
                        .all(|(j, _)| cinched[j]),
                }
            })
            .collect();

        // 3. Mutate, then notify.
        let mut crushed: Vec<SqueezeEvent> = Vec::new();
        for i in to_crush {
            let s = &mut self.items[i];
            s.alive = false;
            crushed.push(SqueezeEvent {
                id: s.id,
                pos: s.pos,
                radius: s.radius,
            });
        }
        for ev in &crushed {
            for listener in &mut self.listeners {
                listener(ev);
            }
        }
    }
}

/// Does `chain` wind a full, tight loop around the circle at `center`?
///
/// Three conditions must hold together:
/// 1. **Winding** — the chain path's radial vector around `center` sweeps at
///    least [`FULL_LOOP_TURNS`] of a full turn.
/// 2. **Tight loop** — the polygon area of joints hugging the object is close
///    to the object's own area.
/// 3. **Force** — the overall chain is stretched, proving it's being pulled
///    tight rather than just draped.
fn chain_cinches(chain: &Chain, center: Vec2D, radius: f32) -> bool {
    let inner = radius * LOOP_INNER_FACTOR;
    let outer = radius * LOOP_OUTER_FACTOR;
    let mut winding = 0.0f32;
    let mut loop_points: Vec<Vec2D> = Vec::new();
    let mut prev: Option<Vec2D> = None;

    for (p, _) in chain.joint_stretches() {
        let v = p - center;
        let dist = v.length();

        // Collect joints that are hugging the object's surface.
        if dist >= inner && dist <= outer {
            loop_points.push(p);
        }

        if let Some(pv) = prev {
            // Signed angle swept from the previous radial vector to this one.
            let cross = pv.x * v.y - pv.y * v.x;
            let dot = pv.dot(v);
            winding += cross.atan2(dot);
        }
        prev = Some(v);
    }

    if loop_points.len() < 3 {
        return false;
    }

    let loop_area = loop_polygon_area(&loop_points, center);
    let circle_area = PI * radius * radius;
    let area_diff_ratio = (loop_area - circle_area).abs() / circle_area;

    (winding.abs() / TAU) >= FULL_LOOP_TURNS
        && area_diff_ratio <= MAX_AREA_DIFF_RATIO
        && chain.stretch() >= MIN_CHAIN_STRETCH
}

/// Area of the polygon formed by sorting loop points by angle around `center`.
fn loop_polygon_area(points: &[Vec2D], center: Vec2D) -> f32 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut ordered: Vec<Vec2D> = points.to_vec();
    ordered.sort_by(|a, b| {
        let angle_a = (a.y - center.y).atan2(a.x - center.x);
        let angle_b = (b.y - center.y).atan2(b.x - center.x);
        angle_a.partial_cmp(&angle_b).unwrap()
    });
    polygon_area(&ordered)
}

/// Shoelace formula for a simple polygon.
fn polygon_area(points: &[Vec2D]) -> f32 {
    let n = points.len();
    if n < 3 {
        return 0.0;
    }
    let mut sum = 0.0f32;
    for i in 0..n {
        let j = (i + 1) % n;
        sum += points[i].x * points[j].y - points[j].x * points[i].y;
    }
    sum.abs() * 0.5
}
