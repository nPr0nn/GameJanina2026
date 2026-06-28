use juni::prelude::*;

use crate::animation::{Animation, SpriteSheet};
use crate::collision::{move_point_swept, push_point_out_of_all, Collider};

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
    color: Color,
    /// Chain-link sprite sheet. Cloning is cheap (reference-counted texture).
    sheet: SpriteSheet,
    /// Sprite animation for the metallic link shimmer.
    anim: Animation,
    /// Scratch buffer reused every frame to track which joints were moved by
    /// a distance constraint (avoids a heap allocation per update).
    was_constrained: Vec<bool>,
    /// Scratch buffer: joints pushed out of an obstacle this frame.
    /// These skip the stiffness snap and the straightening pass so the snap
    /// cannot push them back through geometry.
    obstacle_constrained: Vec<bool>,
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
    /// * `sheet` — chain-link sprite sheet (64×64 frames).
    pub fn new(
        start: Vec2D,
        end: Vec2D,
        total_length: f32,
        link_size: f32,
        color: Color,
        sheet: SpriteSheet,
    ) -> Self {
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
        let anim = Animation::new(sheet.clone(), 0, 8.0, true);
        Self {
            joints,
            segment_length,
            link_size,
            damping: 0.025,
            straightness: 0.7,
            constraint_iterations: 20,
            color,
            sheet,
            anim,
            was_constrained: vec![false; n],
            obstacle_constrained: vec![false; n],
        }
    }

    /// Build a chain directly from an explicit polyline of joint positions.
    ///
    /// Used for **frozen snippets** created when a chain is split at a portal:
    /// the captured points become the joints verbatim and the snippet is only
    /// ever drawn and straightened (via [`set_joint_positions`]), never
    /// simulated. `segment_length` should match the parent chain so the links
    /// render at a consistent size.
    pub fn from_points(
        points: &[Vec2D],
        segment_length: f32,
        link_size: f32,
        color: Color,
        sheet: SpriteSheet,
    ) -> Self {
        assert!(points.len() >= 2, "a chain snippet needs at least two points");
        let joints: Vec<Joint> = points
            .iter()
            .map(|&p| Joint { pos: p, old_pos: p })
            .collect();
        let n = joints.len();
        let anim = Animation::new(sheet.clone(), 0, 8.0, true);
        Self {
            joints,
            segment_length,
            link_size,
            damping: 0.025,
            straightness: 0.7,
            constraint_iterations: 20,
            color,
            sheet,
            anim,
            was_constrained: vec![false; n],
            obstacle_constrained: vec![false; n],
        }
    }

    /// The chain's per-segment rest length.
    pub fn segment_length(&self) -> f32 {
        self.segment_length
    }

    /// The current joint positions as a polyline, in anchor → player order.
    pub fn path_points(&self) -> Vec<Vec2D> {
        self.joints.iter().map(|j| j.pos).collect()
    }

    /// Current geometric path length (sum of the segment distances).
    pub fn path_length(&self) -> f32 {
        self.joints
            .windows(2)
            .map(|w| w[0].pos.distance(w[1].pos))
            .sum()
    }

    /// Overwrite every joint position. Used to straighten a frozen snippet
    /// toward taut as rope is pulled through a portal. `old_pos` is synced so
    /// the snippet never carries Verlet velocity if it is later re-simulated.
    pub fn set_joint_positions(&mut self, points: &[Vec2D]) {
        debug_assert_eq!(points.len(), self.joints.len());
        for (j, &p) in self.joints.iter_mut().zip(points) {
            j.pos = p;
            j.old_pos = p;
        }
    }

    /// Pin the fixed anchor to `pos`. Call before [`update`](Self::update).
    pub fn set_start(&mut self, pos: Vec2D) {
        self.joints[0].pos = pos;
        self.joints[0].old_pos = pos;
    }

    /// Pin the player-end anchor to `pos`. Call before [`update`](Self::update).
    pub fn set_end(&mut self, pos: Vec2D) {
        let last = self.joints.len() - 1;
        self.joints[last].pos = pos;
        self.joints[last].old_pos = pos;
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
    /// Must be called **after** [`update`](Self::update) so `obstacle_constrained`
    /// reflects the current frame.
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
        (self.joints[contact_idx].pos, free_length)
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
    pub fn update(&mut self, dt: f32, obstacles: &[Collider]) {
        let n = self.joints.len();
        if n < 2 {
            return;
        }

        // Advance the metallic link shimmer.
        self.anim.update(dt);

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
                let neighbour = self.joints[i + 1].pos;
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
                let neighbour = self.joints[i - 1].pos;
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
                    let ideal = (self.joints[i - 1].pos + self.joints[i + 1].pos) * 0.5;
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
                    let neighbour = self.joints[i + 1].pos;
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
                    let neighbour = self.joints[i - 1].pos;
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

    /// Draw the chain: a thin tinted line between joints plus a metallic link
    /// sprite at each joint. The sprite frames are offset by joint index so the
    /// metallic shimmer appears to travel along the chain.
    ///
    /// `alpha` scales the tint's opacity (`0.0`–`1.0`), used by the caller to
    /// fade longer chains behind the shorter ones stacked on top.
    pub fn draw(&self, canvas: &mut Canvas, alpha: f32) {
        let n = self.joints.len();
        if n < 2 {
            return;
        }

        // Mix the chain colour with white so the metallic link keeps some of
        // its base grey/silver look instead of becoming fully saturated.
        let tint = mix_with_white(self.color, 0.35).with_alpha(alpha);

        let frame_count = self.sheet.frame_count(0).max(1);
        // Draw one link every few joints so the links are bigger and visibly
        // separated from each other, while the physics simulation stays fine.
        const DRAW_EVERY: usize = 2;
        let scale = self.link_size * 3.0 / self.sheet.frame_width();
        let half_dest = Vec2D::new(
            self.sheet.frame_width() * scale * 0.5,
            self.sheet.frame_height() * scale * 0.5,
        );

        let mut draw_index = 0usize;
        let mut i = 0usize;
        while i < n {
            let joint = self.joints[i];
            // Orient the link along the chain. Endpoints use their single
            // neighbour; internal joints use the average of in/out directions.
            let prev = if i == 0 { joint.pos } else { self.joints[i - 1].pos };
            let next = if i == n - 1 { joint.pos } else { self.joints[i + 1].pos };
            // `draw_texture_pro` takes rotation in degrees.
            let rotation = (next - prev).to_angle().to_degrees();

            // Offset the shimmer along the chain so adjacent drawn links show
            // different frames and the effect travels as the animation advances.
            let frame = (self.anim.current_frame() + draw_index) % frame_count;
            let pos = joint.pos - half_dest;
            self.sheet.draw_frame_rotated(
                canvas,
                frame as u32,
                0,
                pos,
                scale,
                rotation,
                tint,
            );

            draw_index += 1;
            i += DRAW_EVERY;
        }

        // Always draw the player-end link so the chain tip doesn't look empty.
        let last = n - 1;
        if last % DRAW_EVERY != 0 {
            let joint = self.joints[last];
            let prev = self.joints[last - 1].pos;
            let rotation = (joint.pos - prev).to_angle().to_degrees();
            let frame = (self.anim.current_frame() + draw_index) % frame_count;
            let pos = joint.pos - half_dest;
            self.sheet.draw_frame_rotated(
                canvas,
                frame as u32,
                0,
                pos,
                scale,
                rotation,
                tint,
            );
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

/// Blend `color` toward white by `amount` (`0.0` = unchanged, `1.0` = white).
fn mix_with_white(color: Color, amount: f32) -> Color {
    let t = amount.clamp(0.0, 1.0);
    Color::new(
        (color.r as f32 * (1.0 - t) + 255.0 * t) as u8,
        (color.g as f32 * (1.0 - t) + 255.0 * t) as u8,
        (color.b as f32 * (1.0 - t) + 255.0 * t) as u8,
        color.a,
    )
}
