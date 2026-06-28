use juni::prelude::*;

use crate::collision::{move_point_swept, push_point_out_of_all, Collider};
use crate::portals::Portals;

#[derive(Debug, Clone, Copy)]
struct Joint {
    pos: Vec2D,
    /// Previous position, used only for Verlet inertia on slack (un-tensioned) joints.
    old_pos: Vec2D,
}

/// A chain simulated as a series of rigid links between two pinned endpoints.
///
/// # Physics model
///
/// Designed for a **top-down game with no gravity**.  The chain behaves like a
/// heavy cable dragged across a flat surface:
///
/// * **Propagation from the player end** — constraint solving runs end→anchor
///   first.  Tension only reaches a joint when the segment connecting it to its
///   player-side neighbour is pulled taut.  When the chain is slack, only the
///   links nearest the player end move; links farther toward the fixed anchor
///   stay put.  As the chain becomes more extended, the tension propagates
///   farther and a larger section moves together.
///
/// * **Immediate settling** — after every constraint pass the velocity of any
///   joint that was moved by a constraint is zeroed.  This prevents the
///   correction from becoming a Verlet velocity next frame and producing the
///   elastic "rubber-band" oscillation.  The chain stops in at most one or two
///   frames when the player stops.
///
/// * **Slack inertia** — joints that were *not* tensioned this frame keep a
///   small amount of residual velocity, controlled by [`damping`](Self::damping).
///   At the default value they settle in well under a tenth of a second.
///
/// * **Bend resistance** — [`straightness`](Self::straightness) pulls each
///   joint toward the midpoint of its neighbours, scaled by how stretched the
///   chain currently is.  The chain transitions smoothly from floppy (slack) to
///   cable-straight (taut).
///
/// # Collision readiness
///
/// Each link is exposed as an axis-aligned [`Rect`] via
/// [`link_rects`](Self::link_rects) for future broad-phase collision detection.
pub struct Chain {
    joints: Vec<Joint>,
    segment_length: f32,
    /// Width / height of each virtual square link (visual and collision size).
    pub link_size: f32,
    /// Velocity retention **per second** for slack (un-tensioned) joints.
    ///
    /// `0.0` = stops immediately (pure position-based).
    /// `0.5` = 50 % velocity kept after one second.
    /// Constrained joints always have their velocity zeroed regardless of this
    /// setting.
    pub damping: f32,
    /// Bend resistance as the chain stretches (`0.0`–`1.0`).
    ///
    /// `0.0` = always floppy; `1.0` = fully cable-straight when taut.
    pub straightness: f32,
    constraint_iterations: usize,
    /// Draw each joint as a square in addition to the connecting lines.
    pub show_debug: bool,
    color: Color,
    /// Scratch buffer reused every frame to track which joints were moved by
    /// a distance constraint (avoids a heap allocation per update).
    was_constrained: Vec<bool>,
    /// Scratch buffer: joints pushed out of an obstacle this frame.
    /// These skip the stiffness snap and the straightening pass so the snap
    /// cannot push them back through geometry.
    obstacle_constrained: Vec<bool>,
    /// Per-joint **portal phase**: the net number of `in` → `out` portal
    /// crossings between this joint and the fixed anchor. Adjacent joints whose
    /// phases differ are linked *through* a portal; their distance constraint is
    /// measured across the portal seam (see [`neighbour_pos`](Self::neighbour_pos)).
    phases: Vec<i32>,
    /// Per-joint teleport latch: cleared right after a joint crosses a portal and
    /// re-armed once it has left both portal mouths, so a joint cannot bounce
    /// back and forth on the frames it sits inside a portal.
    can_teleport: Vec<bool>,
    /// Portal translation (`out − in`) cached for the current frame; `ZERO` when
    /// no portal pair is active, which makes every portal term vanish.
    portal_disp: Vec2D,
}

impl Chain {
    /// Create a new chain.
    ///
    /// Joints are distributed evenly between `start` and `end`.
    ///
    /// * `total_length` — maximum chain length; anchors cannot be farther apart.
    /// * `link_size` — size of each virtual link; determines segment count and
    ///   is also used as the visual line thickness.
    /// * `color` — tint used when drawing.
    pub fn new(start: Vec2D, end: Vec2D, total_length: f32, link_size: f32, color: Color) -> Self {
        assert!(total_length > 0.0, "chain total_length must be positive");
        assert!(link_size > 0.0, "chain link_size must be positive");

        let num_segments = (total_length / link_size).max(1.0).ceil() as usize;
        let segment_length = total_length / num_segments as f32;

        let mut joints = Vec::with_capacity(num_segments + 1);
        for i in 0..=num_segments {
            let t = i as f32 / num_segments as f32;
            let pos = start + (end - start) * t;
            joints.push(Joint { pos, old_pos: pos });
        }

        let n = joints.len();
        Self {
            joints,
            segment_length,
            link_size,
            damping: 0.025,
            straightness: 0.7,
            constraint_iterations: 20,
            show_debug: true,
            color,
            was_constrained: vec![false; n],
            obstacle_constrained: vec![false; n],
            phases: vec![0; n],
            can_teleport: vec![true; n],
            portal_disp: Vec2D::ZERO,
        }
    }

    /// Pin the fixed anchor to `pos`. Call before [`update`](Self::update).
    ///
    /// The anchor is always portal phase 0 — the reference frame every other
    /// joint's phase is measured against.
    pub fn set_start(&mut self, pos: Vec2D) {
        self.joints[0].pos = pos;
        self.joints[0].old_pos = pos;
        self.phases[0] = 0;
    }

    /// Pin the player-end anchor to `pos`, at portal phase `phase`.
    ///
    /// `phase` is the player's own net portal-crossing count: when the player
    /// walks through a portal the chain end inherits its phase, which seeds the
    /// seam that then propagates inward through the constraint solver. Call
    /// before [`update`](Self::update).
    pub fn set_end(&mut self, pos: Vec2D, phase: i32) {
        let last = self.joints.len() - 1;
        self.joints[last].pos = pos;
        self.joints[last].old_pos = pos;
        self.phases[last] = phase;
    }

    /// Position of joint `j` expressed in joint `i`'s portal frame.
    ///
    /// When the two joints share a phase this is just `joints[j].pos`. When they
    /// straddle a portal seam the neighbour is translated *through* the portal so
    /// the segment is measured across the wormhole rather than across the map —
    /// this is what keeps a chain threaded through a portal taut and continuous.
    fn neighbour_pos(&self, i: usize, j: usize) -> Vec2D {
        self.joints[j].pos - (self.phases[j] - self.phases[i]) as f32 * self.portal_disp
    }

    /// True when the chain currently threads a portal (some adjacent pair of
    /// joints is linked through the wormhole). Used to forbid closing a portal a
    /// chain is crossing.
    pub fn is_crossing_portal(&self) -> bool {
        self.phases.windows(2).any(|w| w[0] != w[1])
    }

    /// Total maximum length of the chain.
    pub fn max_length(&self) -> f32 {
        self.segment_length * self.joints.len().saturating_sub(1) as f32
    }

    /// Position of the fixed anchor (start).
    pub fn start(&self) -> Vec2D {
        self.joints[0].pos
    }

    /// Position of the player-end anchor.
    #[allow(dead_code)]
    pub fn end(&self) -> Vec2D {
        self.joints[self.joints.len() - 1].pos
    }

    /// True when every joint's frame-to-frame displacement is below `threshold`.
    pub fn is_still(&self, threshold: f32) -> bool {
        self.joints
            .iter()
            .all(|j| j.pos.distance_squared(j.old_pos) < threshold * threshold)
    }

    /// Geometric stretch in `[0, 1]`: actual path length divided by max length.
    ///
    /// Reflects the real chain path, not just the straight-line anchor distance,
    /// so a chain wrapped around an obstacle reads `1.0` when fully extended.
    pub fn stretch(&self) -> f32 {
        let actual: f32 = self
            .joints
            .windows(2)
            .map(|w| w[0].pos.distance(w[1].pos))
            .sum();
        (actual / self.max_length()).clamp(0.0, 1.0)
    }

    /// Each joint's position together with how stretched its most-stretched
    /// neighbour segment is, as a ratio of [`segment_length`](Self::segment_length).
    pub fn joint_stretches(&self) -> impl Iterator<Item = (Vec2D, f32)> + '_ {
        let sl = self.segment_length;
        self.joints.iter().enumerate().map(move |(i, j)| {
            let mut max_ratio = 0.0f32;
            if i > 0 {
                max_ratio = max_ratio.max(j.pos.distance(self.joints[i - 1].pos) / sl);
            }
            if i + 1 < self.joints.len() {
                max_ratio = max_ratio.max(j.pos.distance(self.joints[i + 1].pos) / sl);
            }
            (j.pos, max_ratio)
        })
    }

    /// Returns the tether point and maximum reach for the player this frame.
    ///
    /// When the chain is wrapping around obstacles, the player's reachable
    /// radius is *not* the full chain length from the anchor — it is the
    /// remaining free length measured from the last joint that was in contact
    /// with an obstacle surface.  This method exposes that contact point and
    /// the remaining length so the caller can apply the correct constraint.
    ///
    /// When no obstacle contact exists (chain is freely swinging), returns the
    /// fixed anchor position and the total chain length, which is identical to
    /// a straight-line anchor constraint.
    ///
    /// **Portal-aware:** the tether point is returned *in the player end's phase
    /// frame* — translated through the wormhole by the phase difference between
    /// the contact joint and the player end. This means the straight-line clamp
    /// the caller applies measures the chain's remaining length *through* the
    /// portal, so the max length still limits the player while the chain threads
    /// a portal (any number of windings). When no portal pair is active
    /// `portal_disp` is `ZERO`, so this reduces exactly to the world tether.
    ///
    /// Must be called **after** [`update`](Self::update) so `obstacle_constrained`
    /// and `portal_disp` reflect the current frame.
    pub fn player_tether(&self) -> (Vec2D, f32) {
        let n = self.joints.len();
        // Walk backwards from the player end to find the last obstacle contact.
        let contact_idx = self.obstacle_constrained[..n - 1]
            .iter()
            .enumerate()
            .rev()
            .find(|(_, &c)| c)
            .map(|(i, _)| i)
            .unwrap_or(0);
        let free_length = (n - 1 - contact_idx) as f32 * self.segment_length;
        let player_phase = self.phases[n - 1];
        let tether = self.joints[contact_idx].pos
            + (player_phase - self.phases[contact_idx]) as f32 * self.portal_disp;
        (tether, free_length)
    }

    /// Teleport internal joints that have entered a portal mouth this frame.
    ///
    /// Crossing `in` → `out` translates the joint by `+displacement` and bumps
    /// its phase by `+1`; `out` → `in` does the reverse.  Because positions stay
    /// *world-true* (only the phase records which side of the wormhole a joint is
    /// on), obstacle collision keeps working unchanged.  A per-joint latch stops
    /// a freshly-teleported joint — which lands inside the far mouth — from
    /// immediately bouncing back; it re-arms once the joint clears both mouths.
    ///
    /// **The crossing propagates from the player end inward.** Joints are
    /// processed from `n-2` down to `1`, and each joint compares its phase to
    /// its player-side neighbour (`i + 1`).  A joint only crosses to catch up
    /// to that neighbour, so the seam seeded at the player end zips toward the
    /// anchor one joint at a time.  This is stable for any integer winding:
    /// the player can cross the portal pair arbitrarily many times and the
    /// chain will gradually unwrap/rewind to match.
    ///
    /// The endpoints are skipped: the anchor is fixed at phase 0 and the player
    /// end's phase is supplied each frame by [`set_end`](Self::set_end).
    fn cross_portals(&mut self, in_center: Vec2D, out_center: Vec2D, radius: f32) {
        let n = self.joints.len();
        let r_sq = radius * radius;
        let disp = self.portal_disp;
        // Only let a single joint cross per update. This keeps the passage
        // natural: the chain feeds through the portal mouth one link at a time
        // instead of several joints teleporting together.
        let mut already_crossed = false;
        for i in (1..n - 1).rev() {
            let pos = self.joints[i].pos;
            let in_in = (pos - in_center).length_squared() < r_sq;
            let in_out = (pos - out_center).length_squared() < r_sq;

            if !self.can_teleport[i] {
                // Re-arm only once the joint has cleared both mouths.
                if !in_in && !in_out {
                    self.can_teleport[i] = true;
                }
                continue;
            }

            let ph = self.phases[i];
            let next_ph = self.phases[i + 1];

            if in_in && next_ph > ph && !already_crossed {
                // Player-side neighbour is further through the wormhole; follow
                // it in → out.
                self.joints[i].pos += disp;
                self.joints[i].old_pos += disp;
                self.phases[i] += 1;
                self.can_teleport[i] = false;
                already_crossed = true;
            } else if in_out && next_ph < ph && !already_crossed {
                // Player-side neighbour is further back; follow it out → in.
                self.joints[i].pos -= disp;
                self.joints[i].old_pos -= disp;
                self.phases[i] -= 1;
                self.can_teleport[i] = false;
                already_crossed = true;
            }
        }
    }

    /// Pull each seam joint toward the portal mouth it threads.
    ///
    /// When a chain crosses a portal, the two joints that straddle the seam
    /// behave as anchors for their respective fragments: the anchor-side joint
    /// sits at the entry mouth and the player-side joint sits at the exit mouth.
    /// This keeps the chain from flinging across the map and makes the two
    /// fragments act as if they are independently tethered to the portal pair.
    fn anchor_seams_to_portals(&mut self, in_center: Vec2D, out_center: Vec2D, obstacles: &[Collider]) {
        let n = self.joints.len();
        let sl = self.segment_length;
        for i in 0..n - 1 {
            if self.phases[i] == self.phases[i + 1] {
                continue;
            }
            // Higher phase is on the `out` side (more in → out crossings).
            let (mouth_a, mouth_b) = if self.phases[i + 1] > self.phases[i] {
                (in_center, out_center)
            } else {
                (out_center, in_center)
            };

            // Keep joint i within one segment length of its mouth.
            let delta = mouth_a - self.joints[i].pos;
            let dist = delta.length();
            if dist > sl {
                let target = self.joints[i].pos + delta * ((dist - sl) / dist);
                let (moved, _) = move_point_swept(self.joints[i].pos, target, obstacles);
                self.joints[i].pos = moved;
            }

            // Keep joint i+1 within one segment length of its mouth.
            let delta = mouth_b - self.joints[i + 1].pos;
            let dist = delta.length();
            if dist > sl {
                let target = self.joints[i + 1].pos + delta * ((dist - sl) / dist);
                let (moved, _) = move_point_swept(self.joints[i + 1].pos, target, obstacles);
                self.joints[i + 1].pos = moved;
            }
        }
    }

    /// Safety net that clears a single-joint winding **spike** — an interior
    /// joint whose phase differs from *both* of its (equal-phase) neighbours.
    ///
    /// The zipper in [`cross_portals`](Self::cross_portals) can never *produce*
    /// such a state: a joint only crosses when a chain-neighbour is already one
    /// step across, so a winding always spans a run of joints, never a lone one.
    /// A spike can therefore only come from a desync (e.g. a teleport-latch edge
    /// case under very fast motion). Left alone it would make the seam's distance
    /// constraint measure across the wormhole on both sides at once and fling the
    /// joint across the map.
    ///
    /// It is repaired the **same way a real crossing is** — translating the joint
    /// by the phase delta times the portal displacement — so the joint lands at
    /// the world position that winding actually corresponds to. Nothing streaks:
    /// this is an undo/redo of teleports, not an in-place relabel.
    fn repair_phase_spikes(&mut self) {
        let n = self.joints.len();
        for i in 1..n - 1 {
            let (before, after) = (self.phases[i - 1], self.phases[i + 1]);
            if before == after && self.phases[i] != before {
                let delta = (before - self.phases[i]) as f32;
                self.joints[i].pos += delta * self.portal_disp;
                self.joints[i].old_pos += delta * self.portal_disp;
                self.phases[i] = before;
                self.can_teleport[i] = false;
            }
        }
    }

    /// Advance the simulation by `dt` seconds.
    ///
    /// Both anchors must be pinned via [`set_start`](Self::set_start) /
    /// [`set_end`](Self::set_end) **before** calling this.
    ///
    /// `obstacles` is a slice of axis-aligned rectangles that chain joints
    /// cannot pass through.  The collision pass runs interleaved with the
    /// distance constraints so wrapping around corners converges naturally:
    /// joints near a corner get pushed to different faces by successive
    /// iterations, threading the chain around the obstacle automatically.
    pub fn update(&mut self, dt: f32, obstacles: &[Collider], portals: Option<&Portals>) {
        let n = self.joints.len();
        if n < 2 {
            return;
        }

        // ── Portal frame ──────────────────────────────────────────────────────
        //
        // Cache this frame's portal translation and, when a pair is active, let
        // each internal joint cross the wormhole.  `portal_disp` is ZERO when no
        // pair is active, which makes every `neighbour_pos` portal term vanish so
        // the solver below behaves exactly as it did before portals existed.
        match portals {
            Some(p) if p.active() => {
                self.portal_disp = p.displacement();
                self.cross_portals(p.in_center(), p.out_center(), p.radius());
                self.anchor_seams_to_portals(p.in_center(), p.out_center(), obstacles);
                self.repair_phase_spikes();
            }
            _ => {
                // No active pair: collapse every joint back to phase 0 so the
                // chain reads as one continuous strand again.
                self.portal_disp = Vec2D::ZERO;
                self.phases.iter_mut().for_each(|ph| *ph = 0);
                self.can_teleport.iter_mut().for_each(|c| *c = true);
            }
        }

        // ── 0. Re-establish the "outside all obstacles" invariant ─────────────
        //
        // The whole no-teleport guarantee rests on one invariant: at the start
        // of every operation each internal joint is OUTSIDE every obstacle.
        // That holds frame-to-frame because all movement below is swept.  The
        // only way a joint can be inside here is a fresh spawn whose initial
        // straight line crossed a block, so a one-time static push-out (nearest
        // face) is correct — it is not a gameplay teleport.
        for i in 1..n - 1 {
            let unstuck = push_point_out_of_all(self.joints[i].pos, obstacles);
            if unstuck != self.joints[i].pos {
                self.joints[i].pos = unstuck;
                self.joints[i].old_pos = unstuck;
            }
        }

        // ── 1. Residual inertia for slack joints ──────────────────────────────
        //
        // A small Verlet step is applied to internal joints.  Constrained joints
        // have their velocity zeroed in step 3, so inertia only manifests for
        // links that are genuinely free (not under tension).  The integrated
        // motion is applied through `move_point_swept` so a fast-moving slack
        // joint can never tunnel into a block.
        if self.damping > 0.0 {
            let retention = self.damping.powf(dt);
            for i in 1..n - 1 {
                let vel = (self.joints[i].pos - self.joints[i].old_pos) * retention;
                let target = self.joints[i].pos + vel;
                self.joints[i].old_pos = self.joints[i].pos;
                let (moved, _) = move_point_swept(self.joints[i].pos, target, obstacles);
                self.joints[i].pos = moved;
            }
        } else {
            // Sync old_pos so old data never leaks if damping is later enabled.
            for i in 1..n - 1 {
                self.joints[i].old_pos = self.joints[i].pos;
            }
        }

        // ── 2. Bidirectional distance constraints ─────────────────────────────
        //
        // Segments may never *exceed* segment_length (max-length constraint).
        // Segments may be shorter than segment_length (slack is fine).
        //
        // Pass A — player end → fixed anchor (primary pass).
        //   Working from joint n-2 down to joint 1, each joint is pulled
        //   toward its player-side neighbour if the segment is overstretched.
        //   This is what produces "only the nearby links follow when slack":
        //   if a link is within range of its player-side neighbour it is not
        //   moved, and the tension stops propagating inward.
        //
        // Pass B — fixed anchor → player end (secondary pass).
        //   Enforce consistency from the anchor side.  Both passes together
        //   converge on a valid configuration after several iterations even
        //   when the chain is fully taut between two fixed points.

        let sl = self.segment_length;
        let sl_sq = sl * sl;
        self.was_constrained.iter_mut().for_each(|c| *c = false);
        self.obstacle_constrained.iter_mut().for_each(|c| *c = false);

        // Every constraint correction is applied through `move_point_swept`:
        // the joint is moved *from its current (known-good) position* toward the
        // corrected target, stopping at any block surface in between.  Because a
        // max-length correction always pulls a joint TOWARD a neighbour (never
        // away), swept-clamping it can only make the segment shorter — it can
        // never re-introduce a length violation, and it can never carry the
        // joint to the far side of a block.  Wrapping around corners falls out
        // for free: a blocked joint slides along the face toward the corner over
        // successive iterations.
        for _ in 0..self.constraint_iterations {
            // Pass A: player end → anchor.
            for i in (1..n - 1).rev() {
                let neighbour = self.neighbour_pos(i, i + 1);
                let delta = self.joints[i].pos - neighbour;
                let dist_sq = delta.length_squared();
                if dist_sq > sl_sq {
                    let dist = dist_sq.sqrt();
                    let target = neighbour + delta * (sl / dist);
                    let (moved, hit) = move_point_swept(self.joints[i].pos, target, obstacles);
                    self.joints[i].pos = moved;
                    self.was_constrained[i] = true;
                    if hit {
                        self.obstacle_constrained[i] = true;
                    }
                }
            }

            // Pass B: anchor → player end.
            for i in 1..n - 1 {
                let neighbour = self.neighbour_pos(i, i - 1);
                let delta = self.joints[i].pos - neighbour;
                let dist_sq = delta.length_squared();
                if dist_sq > sl_sq {
                    let dist = dist_sq.sqrt();
                    let target = neighbour + delta * (sl / dist);
                    let (moved, hit) = move_point_swept(self.joints[i].pos, target, obstacles);
                    self.joints[i].pos = moved;
                    self.was_constrained[i] = true;
                    if hit {
                        self.obstacle_constrained[i] = true;
                    }
                }
            }
        }

        // ── 3. Partial velocity retention for constrained joints ─────────────
        //
        // Previously we zeroed velocity here completely, which killed all
        // inertia and made the chain feel mechanical.  Now we keep a small
        // fraction (INERTIA_KEEP) of the post-constraint velocity so that
        // past movement bleeds into the next frame: sudden pulls create
        // ripples, and direction changes feel organic rather than instant.
        //
        // INERTIA_KEEP is kept low (0.15) so the chain does not re-introduce
        // the elastic oscillation that the earlier zeroing was meant to cure —
        // at this level the ripple decays within a second under normal damping.
        const INERTIA_KEEP: f32 = 0.15;
        for i in 1..n - 1 {
            if self.obstacle_constrained[i] {
                // Joints resting against a wall must not carry velocity or they
                // would try to move through the obstacle next frame.
                self.joints[i].old_pos = self.joints[i].pos;
            } else if self.was_constrained[i] {
                let vel = self.joints[i].pos - self.joints[i].old_pos;
                self.joints[i].old_pos = self.joints[i].pos - vel * INERTIA_KEEP;
            }
        }

        // ── 4. Bend resistance ────────────────────────────────────────────────
        //
        // Pull each joint toward the midpoint of its neighbours to make the
        // chain look like a taut cable as it extends.
        //
        // Guard: only run when at least one joint was constrained this frame
        // (the chain is actively under tension).  Without this guard the chain
        // would slowly straighten itself every frame even while the player is
        // still — a heavy cable on a flat surface does not do that.
        //
        // Strength uses a quadratic ramp so the effect is negligible when
        // slightly extended and full-force only near maximum stretch.
        // Eight sub-iterations per frame ensures the chain converges to a
        // cable-straight line within one or two frames when fully taut.
        // Stiffness is driven by PATH stretch (`self.stretch()`), not straight-
        // line endpoint distance, so a chain wrapped tightly around a block
        // still reads as "taut" and stiffens its free spans correctly.  The old
        // straight-line "snap toward the anchor→player line" step was removed
        // entirely: that line cuts through any block the chain wraps, and no
        // amount of guarding made it safe — it was the primary teleport source.
        //
        // Every bend correction is swept, so pulling a joint toward its
        // neighbours' midpoint can at most press it against a block face; it can
        // never drag it through to the other side.  This is what lets the chain
        // pull taut around a corner — even loop fully around a block — without
        // ever popping through.
        let under_tension = self.was_constrained[1..n - 1].iter().any(|&c| c);
        let stretch = self.stretch();
        if under_tension && self.straightness > 0.0 && stretch > 0.1 {
            let strength = self.straightness * stretch * stretch;
            for _ in 0..8 {
                let mut max_delta_sq = 0.0f32;
                for i in 1..n - 1 {
                    // Leave seam joints (whose neighbours sit at a different
                    // phase) to the distance constraints alone; straightening
                    // across a wormhole seam is what spirals the chain into loops.
                    if self.phases[i] != self.phases[i - 1] || self.phases[i] != self.phases[i + 1] {
                        continue;
                    }
                    let ideal = (self.neighbour_pos(i, i - 1) + self.neighbour_pos(i, i + 1)) * 0.5;
                    let current = self.joints[i].pos;
                    let target = current + (ideal - current) * strength;
                    let (moved, hit) = move_point_swept(current, target, obstacles);
                    self.joints[i].pos = moved;
                    if hit {
                        self.obstacle_constrained[i] = true;
                    }
                    max_delta_sq = max_delta_sq.max(moved.distance_squared(current));
                }
                if max_delta_sq < 0.01 {
                    break; // converged – no need for more sub-iterations
                }
            }

            // ── 4b. Re-enforce max-length broken by straightening (swept) ──────
            //
            // Pulling toward the midpoint can stretch a joint past segment_length
            // from one of its neighbours.  A short swept constraint pass resolves
            // those violations in-frame so they cannot feed back as wobble.
            for _ in 0..5 {
                for i in (1..n - 1).rev() {
                    let neighbour = self.neighbour_pos(i, i + 1);
                    let delta = self.joints[i].pos - neighbour;
                    let dist_sq = delta.length_squared();
                    if dist_sq > sl_sq {
                        let dist = dist_sq.sqrt();
                        let target = neighbour + delta * (sl / dist);
                        let (moved, hit) = move_point_swept(self.joints[i].pos, target, obstacles);
                        self.joints[i].pos = moved;
                        if hit {
                            self.obstacle_constrained[i] = true;
                        }
                    }
                }
                for i in 1..n - 1 {
                    let neighbour = self.neighbour_pos(i, i - 1);
                    let delta = self.joints[i].pos - neighbour;
                    let dist_sq = delta.length_squared();
                    if dist_sq > sl_sq {
                        let dist = dist_sq.sqrt();
                        let target = neighbour + delta * (sl / dist);
                        let (moved, hit) = move_point_swept(self.joints[i].pos, target, obstacles);
                        self.joints[i].pos = moved;
                        if hit {
                            self.obstacle_constrained[i] = true;
                        }
                    }
                }
            }

            // Suppress bend/straightening-induced velocity.  Under tension the
            // chain should settle crisply, so all internal joints (including any
            // newly pressed against a wall during this step) drop their velocity.
            for i in 1..n - 1 {
                self.joints[i].old_pos = self.joints[i].pos;
            }
        }
    }

    /// Draw the chain: thick lines between consecutive joints, and (when
    /// [`show_debug`](Self::show_debug) is enabled) a square at each joint.
    ///
    /// A link that bridges a portal seam (its two joints sit at different phases)
    /// is **not** drawn straight across the map. When `portals` is supplied each
    /// seam-side joint is instead connected to the portal mouth it threads, so
    /// the chain visibly dives into one portal and re-emerges from the other —
    /// for any number of windings. Without portal info the seam link is simply
    /// left as a gap.
    pub fn draw(&self, canvas: &mut Canvas, portals: Option<&Portals>) {
        let thickness = self.link_size * 0.5;
        for i in 0..self.joints.len() - 1 {
            let (a, b) = (self.joints[i].pos, self.joints[i + 1].pos);
            if self.phases[i] == self.phases[i + 1] {
                canvas.line(a, b, thickness, self.color);
            } else if let Some(p) = portals.filter(|p| p.active()) {
                // Seam: joint `i` enters the portal whose phase it leaves, and
                // joint `i+1` emerges from the other. Higher phase == the `out`
                // side (one more `in → out` crossing recorded).
                let (a_mouth, b_mouth) = if self.phases[i + 1] > self.phases[i] {
                    (p.in_center(), p.out_center())
                } else {
                    (p.out_center(), p.in_center())
                };
                canvas.line(a, a_mouth, thickness, self.color);
                canvas.line(b_mouth, b, thickness, self.color);
            }
        }

        if self.show_debug {
            let half = self.link_size * 0.5;
            for joint in &self.joints {
                canvas.rectangle(
                    joint.pos.x - half,
                    joint.pos.y - half,
                    self.link_size,
                    self.link_size,
                    self.color,
                );
            }
        }
    }

    /// Clamp a desired movement delta so the entity stays within
    /// [`max_length`](Self::max_length) of the fixed anchor.
    ///
    /// The tangential component (sliding along the boundary arc) is preserved;
    /// only the radial component that would overextend the chain is removed.
    pub fn constrain_movement(&self, current_pos: Vec2D, desired_delta: Vec2D) -> Vec2D {
        let fixed = self.start();
        let next_pos = current_pos + desired_delta;
        let next_dist = fixed.distance(next_pos);
        let max_dist = self.max_length();

        if next_dist <= max_dist {
            return desired_delta;
        }

        let dir = (next_pos - fixed).try_normalize().unwrap_or(Vec2D::X);
        let clamped_pos = fixed + dir * max_dist;
        clamped_pos - current_pos
    }

    /// Iterator over the bounding rectangles of each virtual link.
    ///
    /// Axis-aligned squares centred on each joint.  Intended for future
    /// broad-phase world collision detection.
    #[allow(dead_code)]
    pub fn link_rects(&self) -> impl Iterator<Item = Rect> + '_ {
        let half = self.link_size * 0.5;
        self.joints.iter().map(move |j| {
            Rect::new(j.pos.x - half, j.pos.y - half, self.link_size, self.link_size)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_chain(n: usize) -> Chain {
        let link_size = 20.0;
        let total = ((n - 1).max(1) as f32) * link_size;
        Chain::new(Vec2D::ZERO, Vec2D::X * total, total, link_size, WHITE)
    }

    #[test]
    fn seam_propagates_in_to_out() {
        let mut chain = test_chain(5);
        let disp = Vec2D::new(200.0, 0.0);
        chain.portal_disp = disp;

        // Anchor at phase 0, player end at phase 1; seam between joints 3 and 4.
        chain.phases[4] = 1;
        chain.joints[4].pos = Vec2D::new(250.0, 0.0); // out side
        chain.joints[3].pos = Vec2D::new(60.0, 0.0); // inside in portal
        chain.can_teleport[3] = true;

        chain.cross_portals(Vec2D::new(50.0, 0.0), Vec2D::new(250.0, 0.0), 30.0);

        assert_eq!(chain.phases[3], 1);
        assert_eq!(chain.joints[3].pos, Vec2D::new(260.0, 0.0));
        assert!(!chain.can_teleport[3]);
    }

    #[test]
    fn seam_propagates_out_to_in() {
        let mut chain = test_chain(5);
        let disp = Vec2D::new(200.0, 0.0);
        chain.portal_disp = disp;

        // Anchor at phase 0, player end stepped back to phase 0 while an
        // interior joint is still at phase 1.
        chain.phases[3] = 1;
        chain.joints[3].pos = Vec2D::new(250.0, 0.0); // inside out portal
        chain.joints[4].pos = Vec2D::new(50.0, 0.0); // player end, in side
        chain.can_teleport[3] = true;

        chain.cross_portals(Vec2D::new(50.0, 0.0), Vec2D::new(250.0, 0.0), 30.0);

        assert_eq!(chain.phases[3], 0);
        assert_eq!(chain.joints[3].pos, Vec2D::new(50.0, 0.0));
        assert!(!chain.can_teleport[3]);
    }

    #[test]
    fn multiple_windings_propagate_inward() {
        let mut chain = test_chain(5);
        let disp = Vec2D::new(200.0, 0.0);
        chain.portal_disp = disp;

        // Player end at phase 2; joint 3 is at phase 1 and inside the in portal,
        // ready to follow the player one step deeper through the wormhole.
        chain.phases[4] = 2;
        chain.phases[3] = 1;
        chain.joints[4].pos = Vec2D::new(250.0, 0.0);
        chain.joints[3].pos = Vec2D::new(60.0, 0.0); // inside in portal
        chain.can_teleport[3] = true;

        chain.cross_portals(Vec2D::new(50.0, 0.0), Vec2D::new(250.0, 0.0), 30.0);

        assert_eq!(chain.phases[3], 2);
        assert_eq!(chain.joints[3].pos, Vec2D::new(260.0, 0.0));
    }

    #[test]
    fn latch_prevents_immediate_bounce() {
        let mut chain = test_chain(5);
        chain.portal_disp = Vec2D::new(200.0, 0.0);

        chain.phases[4] = 1;
        chain.phases[3] = 0;
        chain.joints[3].pos = Vec2D::new(260.0, 0.0); // inside out portal
        chain.can_teleport[3] = false; // latched from a recent crossing

        chain.cross_portals(Vec2D::new(50.0, 0.0), Vec2D::new(250.0, 0.0), 30.0);

        // Even though the player-side neighbour is shallower, the latch stops
        // an instant reverse crossing.
        assert_eq!(chain.phases[3], 0);
        assert_eq!(chain.joints[3].pos, Vec2D::new(260.0, 0.0));
    }

    #[test]
    fn is_crossing_portal_detects_seams() {
        let mut chain = test_chain(5);
        assert!(!chain.is_crossing_portal());

        chain.phases[2] = 1;
        chain.phases[3] = 1;
        assert!(chain.is_crossing_portal());

        chain.phases.iter_mut().for_each(|p| *p = 2);
        assert!(!chain.is_crossing_portal());
    }

    #[test]
    fn only_one_joint_crosses_per_update() {
        let mut chain = test_chain(5);
        chain.portal_disp = Vec2D::new(200.0, 0.0);

        // Both joints 1 and 2 are phase 0 with phase-1 player-side neighbours
        // and both sit inside the in portal. Only the one closest to the player
        // end (joint 2) should cross this frame.
        chain.phases = vec![0, 0, 0, 1, 1];
        chain.joints[1].pos = Vec2D::new(60.0, 0.0);
        chain.joints[2].pos = Vec2D::new(55.0, 0.0);
        chain.can_teleport = vec![true; 5];

        chain.cross_portals(Vec2D::new(50.0, 0.0), Vec2D::new(250.0, 0.0), 30.0);

        let crossings = chain.phases.iter().filter(|&&p| p == 1).count();
        assert_eq!(crossings, 3); // joints 3, 4 were already 1, plus one new crossing
        assert_eq!(chain.phases[2], 1);
        assert_eq!(chain.phases[1], 0); // blocked by the one-crossing limit
    }

    #[test]
    fn seam_anchors_to_portal_mouths() {
        let mut chain = test_chain(5);
        chain.portal_disp = Vec2D::new(200.0, 0.0);

        // Seam between joint 2 (phase 0) and joint 3 (phase 1). Both are far
        // from their portal mouths and should be pulled to within segment
        // length of them.
        chain.phases = vec![0, 0, 0, 1, 1];
        chain.joints[2].pos = Vec2D::ZERO; // far from in portal at (50,0)
        chain.joints[3].pos = Vec2D::new(400.0, 0.0); // far from out portal at (250,0)

        chain.anchor_seams_to_portals(
            Vec2D::new(50.0, 0.0),
            Vec2D::new(250.0, 0.0),
            &[],
        );

        assert!(chain.joints[2].pos.distance(Vec2D::new(50.0, 0.0)) <= chain.segment_length + 1e-4);
        assert!(chain.joints[3].pos.distance(Vec2D::new(250.0, 0.0)) <= chain.segment_length + 1e-4);
    }
}
