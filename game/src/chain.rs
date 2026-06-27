use juni::prelude::*;

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
    /// a constraint (avoids a heap allocation per update).
    was_constrained: Vec<bool>,
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
            damping: 0.05,
            straightness: 0.7,
            constraint_iterations: 20,
            show_debug: true,
            color,
            was_constrained: vec![false; n],
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

    /// Advance the simulation by `dt` seconds.
    ///
    /// Both anchors must be pinned via [`set_start`](Self::set_start) /
    /// [`set_end`](Self::set_end) **before** calling this.
    pub fn update(&mut self, dt: f32) {
        let n = self.joints.len();
        if n < 2 {
            return;
        }

        // ── 1. Residual inertia for slack joints ──────────────────────────────
        //
        // A small Verlet step is applied to internal joints.  With the default
        // damping = 0.05, the per-frame retention is ~0.95 (at 60 fps), so slack
        // joints settle in well under a second.  Constrained joints will have
        // their velocity zeroed in step 3, so inertia only manifests for links
        // that are genuinely free (not under tension).
        if self.damping > 0.0 {
            let retention = self.damping.powf(dt);
            for i in 1..n - 1 {
                let vel = (self.joints[i].pos - self.joints[i].old_pos) * retention;
                self.joints[i].old_pos = self.joints[i].pos;
                self.joints[i].pos += vel;
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

        for _ in 0..self.constraint_iterations {
            // Pass A: player end → anchor
            for i in (1..n - 1).rev() {
                // Joint i+1 is the player-side neighbour (higher index = closer to player).
                let delta = self.joints[i].pos - self.joints[i + 1].pos;
                let dist_sq = delta.length_squared();
                if dist_sq > sl_sq {
                    let dist = dist_sq.sqrt();
                    // Move joint i toward joint i+1 until the segment is exactly sl.
                    self.joints[i].pos = self.joints[i + 1].pos + delta * (sl / dist);
                    self.was_constrained[i] = true;
                }
                // i == 0 is the fixed anchor — skipped by the range 1..n-1.
            }

            // Pass B: anchor → player end
            for i in 1..n - 1 {
                // Joint i-1 is the anchor-side neighbour (lower index = closer to anchor).
                let delta = self.joints[i].pos - self.joints[i - 1].pos;
                let dist_sq = delta.length_squared();
                if dist_sq > sl_sq {
                    let dist = dist_sq.sqrt();
                    // Move joint i toward joint i-1 until the segment is exactly sl.
                    self.joints[i].pos = self.joints[i - 1].pos + delta * (sl / dist);
                    self.was_constrained[i] = true;
                }
                // i == n-1 is the player-end anchor — skipped by the range 1..n-1.
            }
        }

        // ── 3. Kill elastic rebound ───────────────────────────────────────────
        //
        // Zero the velocity (old_pos = pos) of every joint that was moved by
        // a constraint.  Without this, the constraint correction becomes a
        // Verlet velocity next frame and the chain oscillates back — the
        // "elastic cable" effect.  Joints that were *not* tensioned keep their
        // (already-damped) residual velocity for a brief natural settle.
        for i in 1..n - 1 {
            if self.was_constrained[i] {
                self.joints[i].old_pos = self.joints[i].pos;
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
        let under_tension = self.was_constrained[1..n - 1].iter().any(|&c| c);
        let stretch = self.stretch();
        if under_tension && self.straightness > 0.0 && stretch > 0.1 {
            let strength = self.straightness * stretch * stretch;
            for _ in 0..8 {
                let mut max_delta_sq = 0.0f32;
                for i in 1..n - 1 {
                    let ideal = (self.joints[i - 1].pos + self.joints[i + 1].pos) * 0.5;
                    let current = self.joints[i].pos;
                    let correction = (ideal - current) * strength;
                    self.joints[i].pos += correction;
                    max_delta_sq = max_delta_sq.max(correction.length_squared());
                }
                if max_delta_sq < 0.01 {
                    break; // converged – no need for more sub-iterations
                }
            }
            // Suppress straightening-induced velocity.
            for i in 1..n - 1 {
                self.joints[i].old_pos = self.joints[i].pos;
            }

            // ── 4b. Re-enforce constraints broken by straightening ─────────────
            //
            // Straightening can push a joint toward its neighbours' midpoint in a
            // way that exceeds segment_length with one of its adjacent joints.
            // A short constraint pass here resolves those violations within the
            // same frame so they cannot feed back as wobble next frame.
            for _ in 0..5 {
                for i in (1..n - 1).rev() {
                    let delta = self.joints[i].pos - self.joints[i + 1].pos;
                    let dist_sq = delta.length_squared();
                    if dist_sq > sl_sq {
                        let dist = dist_sq.sqrt();
                        self.joints[i].pos = self.joints[i + 1].pos + delta * (sl / dist);
                        self.joints[i].old_pos = self.joints[i].pos;
                    }
                }
                for i in 1..n - 1 {
                    let delta = self.joints[i].pos - self.joints[i - 1].pos;
                    let dist_sq = delta.length_squared();
                    if dist_sq > sl_sq {
                        let dist = dist_sq.sqrt();
                        self.joints[i].pos = self.joints[i - 1].pos + delta * (sl / dist);
                        self.joints[i].old_pos = self.joints[i].pos;
                    }
                }
            }
        }

        // ── 5. Endpoint-stretch stiffness snap ────────────────────────────────
        //
        // When the straight-line distance between the two anchors is near the
        // chain's maximum length, the chain physically cannot bend — every
        // valid configuration is essentially a straight line.  We enforce this
        // explicitly by lerping all internal joints toward the perfect straight
        // line between the two anchor positions.
        //
        // The blend uses a quadratic ramp that activates from 85 % endpoint
        // stretch to 100 %, so the transition from floppy to rigid feels
        // natural rather than a sudden snap.
        //
        // This step does NOT violate the max-length constraint: the straight
        // line between two points is the *shortest* path, so every segment on
        // that line is shorter than or equal to segment_length.
        //
        // NOTE: when world collision is added, skip this for joints held
        // against a wall so the snap cannot push them through geometry.
        let anchor_pos = self.joints[0].pos;
        let end_pos = self.joints[n - 1].pos;
        let endpoint_dist = anchor_pos.distance(end_pos);
        let endpoint_stretch = (endpoint_dist / self.max_length()).clamp(0.0, 1.0);

        if endpoint_stretch > 0.85 {
            let t = (endpoint_stretch - 0.85) / 0.15;
            let stiffness = t * t;
            for i in 1..n - 1 {
                let ratio = i as f32 / (n - 1) as f32;
                let straight_pos = anchor_pos.lerp(end_pos, ratio);
                self.joints[i].pos = self.joints[i].pos.lerp(straight_pos, stiffness);
                self.joints[i].old_pos = self.joints[i].pos;
            }
        }
    }

    /// Draw the chain: thick lines between consecutive joints, and (when
    /// [`show_debug`](Self::show_debug) is enabled) a square at each joint.
    pub fn draw(&self, canvas: &mut Canvas) {
        for i in 0..self.joints.len() - 1 {
            canvas.line(
                self.joints[i].pos,
                self.joints[i + 1].pos,
                self.link_size * 0.5,
                self.color,
            );
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
