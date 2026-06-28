//! World-level portal system. Portals are placed by the player (Space) and come
//! in pairs: stepping into one circle teleports you to its partner. The world
//! can hold any number of completed pairs.
//!
//! This owns only the portal *geometry* and placement. Crossing detection,
//! player teleportation, and the chain split/merge it drives all live in the
//! gameplay loop, which queries [`Portals::find_entry`].

use juni::prelude::*;

/// Radius of every placed portal, in world pixels.
const PORTAL_RADIUS: f32 = 18.0;

/// A linked pair of portal circles. Entering either teleports to the other.
#[derive(Clone, Copy)]
pub struct PortalPair {
    pub a: Circle,
    pub b: Circle,
}

/// All portal pairs in the world, plus a half-placed portal waiting for its
/// partner.
pub struct Portals {
    pairs: Vec<PortalPair>,
    /// The first circle of a pair, placed but not yet paired. The next
    /// [`place`](Self::place) completes the pair.
    pending: Option<Circle>,
}

impl Default for Portals {
    fn default() -> Self {
        Self::new()
    }
}

impl Portals {
    pub fn new() -> Self {
        Self {
            pairs: Vec::new(),
            pending: None,
        }
    }

    /// Place a portal at `center`. The first placement is held pending; the
    /// second completes a pair and is retained alongside any earlier pairs.
    pub fn place(&mut self, center: Vec2D) {
        let circle = Circle::new(center, PORTAL_RADIUS);
        match self.pending.take() {
            None => self.pending = Some(circle),
            Some(first) => self.pairs.push(PortalPair {
                a: first,
                b: circle,
            }),
        }
    }

    /// If `point` is inside some portal circle (other than `exclude`), return
    /// that entry circle together with the partner circle it teleports to.
    ///
    /// `exclude` is the circle the player just exited; skipping it prevents an
    /// immediate re-trigger while the player is still standing on the exit.
    pub fn find_entry(&self, point: Vec2D, exclude: Option<Circle>) -> Option<(Circle, Circle)> {
        for pair in &self.pairs {
            for (entry, exit) in [(pair.a, pair.b), (pair.b, pair.a)] {
                if Some(entry) == exclude {
                    continue;
                }
                if entry.intersects_point(point) {
                    return Some((entry, exit));
                }
            }
        }
        None
    }

    /// Draw every completed pair plus any pending half-placed portal.
    pub fn draw(&self, canvas: &mut Canvas) {
        for pair in &self.pairs {
            canvas.circle(pair.a.center, pair.a.radius, PURPLE);
            canvas.circle(pair.b.center, pair.b.radius, PURPLE);
        }
        if let Some(p) = self.pending {
            canvas.circle(p.center, p.radius, PURPLE.with_alpha(0.5));
        }
    }
}
