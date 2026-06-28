//! Squeezable objects: round things a chain can lasso.  When a chain winds a
//! full loop around one and cinches it tight (a full loop "plus a little
//! force"), the object is crushed and disappears — and every registered
//! listener is notified through a [`SqueezeEvent`].

use std::f32::consts::TAU;

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
}

/// Manager owning the squeezable objects and the squeeze-event listeners.
pub struct Squeezables {
    items: Vec<Squeezable>,
    listeners: Vec<Listener>,
    next_id: u32,
}

// ── Tuning ──────────────────────────────────────────────────────────────────

/// A squeeze needs nearly a full revolution of winding around the object.
const FULL_LOOP_TURNS: f32 = 0.97;
/// How far past the surface a joint still counts as "hugging" the object.
const CONTACT_BAND: f32 = 5.0;
/// Minimum joints hugging the surface — proves the loop has cinched tight
/// against the object ("a little bit of force") rather than merely winding
/// around it from far away.
const MIN_CONTACT_JOINTS: usize = 6;

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
        }
    }

    /// Add a round object at `pos` and return its id.
    pub fn spawn(&mut self, pos: Vec2D, radius: f32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Squeezable {
            id,
            pos,
            radius,
            alive: true,
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
        // Decide first (immutable borrow of chains), mutate items, then notify —
        // this keeps the borrow checker happy without juggling indices.
        let mut crushed: Vec<SqueezeEvent> = Vec::new();
        for s in &mut self.items {
            if !s.alive {
                continue;
            }
            if chains.iter().any(|c| chain_cinches(c, s.pos, s.radius)) {
                s.alive = false;
                crushed.push(SqueezeEvent {
                    id: s.id,
                    pos: s.pos,
                    radius: s.radius,
                });
            }
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
/// Two conditions must hold together:
/// 1. **Winding** — the chain path's radial vector around `center` sweeps at
///    least [`FULL_LOOP_TURNS`] of a full turn.  A chain merely draped over the
///    object sweeps forward then back and nets ~0 turns, so only a real loop
///    qualifies.
/// 2. **Cinch** — at least [`MIN_CONTACT_JOINTS`] joints hug the surface,
///    proving the loop has been pulled tight against the object.
fn chain_cinches(chain: &Chain, center: Vec2D, radius: f32) -> bool {
    let contact_band = radius + CONTACT_BAND;
    let mut winding = 0.0f32;
    let mut contacts = 0usize;
    let mut prev: Option<Vec2D> = None;

    for p in chain.positions() {
        let v = p - center;
        if v.length() <= contact_band {
            contacts += 1;
        }
        if let Some(pv) = prev {
            // Signed angle swept from the previous radial vector to this one.
            let cross = pv.x * v.y - pv.y * v.x;
            let dot = pv.dot(v);
            winding += cross.atan2(dot);
        }
        prev = Some(v);
    }

    (winding.abs() / TAU) >= FULL_LOOP_TURNS && contacts >= MIN_CONTACT_JOINTS
}
