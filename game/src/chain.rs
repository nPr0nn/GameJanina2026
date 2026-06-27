use juni::prelude::*;

/// A single particle in the chain simulation.
#[derive(Debug, Clone, Copy)]
struct Joint {
    pos: Vec2D,
    old_pos: Vec2D,
}

/// A chain with two configurable anchor points, simulated as a series of rigid
/// links (virtual squares).
///
/// # Design
///
/// The game is **top-down**, so there is **no gravity**. Instead, the chain
/// behaves like a heavy cable dragged across a flat surface:
///
/// * **Inertia / mass** — modelled with Verlet integration and strong velocity
///   damping (friction).  When one anchor moves, the disturbance propagates
///   slowly and dies out quickly.  This makes the chain feel heavy rather than
///   weightless or bouncy.
/// * **Continuous stretch** — [`stretch`](Self::stretch) measures how much of
///   the chain's total length is currently "used up" by the joint positions.
///   It is a value in `[0, 1]` that is independent of the anchor distance, so
///   the chain can be fully stretched even when wrapped around obstacles.
///   As stretch increases, a bend-resistance constraint smoothly straightens
///   the chain, giving a realistic transition from floppy to stiff.
/// * **Slack** — when the chain is not fully extended, it is free to bend.
///   Segments may become shorter than their limit, but they are never allowed
///   to stretch longer.
///
/// # Collision
///
/// Each link can be queried as an axis-aligned [`Rect`] via [`link_rects`](Self::link_rects).
/// Collision against world objects is not yet implemented but the geometry is
/// exposed for a future broad-phase.
pub struct Chain {
    joints: Vec<Joint>,
    segment_length: f32,
    /// Width / height of the virtual square used for each link.
    pub link_size: f32,
    /// Velocity retention factor **per second**.  `0.90` means 10 % of the
    /// kinetic energy is lost every second.  Lower values make the chain feel
    /// heavier and more sluggish.  The factor is automatically adjusted for the
    /// current fixed timestep so the damping rate is frame-rate independent.
    pub damping: f32,
    /// How aggressively the chain resists bending as it stretches.
    /// `0.0` = never straightens (purely floppy), `1.0` = maximum straightening.
    pub straightness: f32,
    constraint_iterations: usize,
    /// If `true`, each joint is drawn as a small square in addition to the
    /// connecting line.
    pub show_debug: bool,
    color: Color,
}

impl Chain {
    /// Create a new chain.
    ///
    /// * `start` and `end` — initial anchor positions (used to distribute joints).
    /// * `total_length` — maximum length of the chain.  Anchors cannot be
    ///   separated farther than this.
    /// * `link_size` — size of each virtual square link.  Smaller values give
    ///   more segments and a smoother curve.
    /// * `color` — tint used when drawing the chain.
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

        Self {
            joints,
            segment_length,
            link_size,
            damping: 0.90,
            straightness: 0.5,
            constraint_iterations: 10,
            show_debug: true,
            color,
        }
    }

    /// Move the start anchor to `pos`.  Call before [`update`](Self::update).
    pub fn set_start(&mut self, pos: Vec2D) {
        self.joints[0].pos = pos;
        self.joints[0].old_pos = pos;
    }

    /// Move the end anchor to `pos`.  Call before [`update`](Self::update).
    pub fn set_end(&mut self, pos: Vec2D) {
        let last = self.joints.len() - 1;
        self.joints[last].pos = pos;
        self.joints[last].old_pos = pos;
    }

    /// Total maximum length of the chain.
    pub fn max_length(&self) -> f32 {
        self.segment_length * (self.joints.len().saturating_sub(1)) as f32
    }

    /// Position of the start anchor.
    pub fn start(&self) -> Vec2D {
        self.joints[0].pos
    }

    /// Position of the end anchor.
    #[allow(dead_code)]
    pub fn end(&self) -> Vec2D {
        self.joints[self.joints.len() - 1].pos
    }

    /// A continuous measure of how extended the chain is, in `[0, 1]`.
    ///
    /// Computed from the sum of actual segment lengths divided by the chain's
    /// maximum length.  This is a **local geometric** property: it reflects the
    /// real path the chain follows, not just the straight-line distance between
    /// anchors.  Consequently a chain wrapped around an obstacle can read
    /// `1.0` (fully stretched) even though the anchors are much closer than
    /// [`max_length`](Self::max_length).
    pub fn stretch(&self) -> f32 {
        let actual: f32 = self
            .joints
            .windows(2)
            .map(|w| w[0].pos.distance(w[1].pos))
            .sum();
        (actual / self.max_length()).clamp(0.0, 1.0)
    }

    /// Advance the physics simulation by `dt` seconds.
    ///
    /// The chain is integrated with heavy damping and distance constraints.
    /// A continuous bend-resistance term (scaled by [`stretch`](Self::stretch))
    /// smoothly transitions the chain from floppy to stiff as it extends.
    pub fn update(&mut self, dt: f32) {
        // Per-frame damping factor derived from the per-second rate so the
        // heavy "mass" feel is independent of the fixed timestep.
        let frame_damping = self.damping.powf(dt * 60.0);

        // 1. Integrate internal joints.
        for i in 1..self.joints.len() - 1 {
            let joint = &self.joints[i];
            let velocity = joint.pos - joint.old_pos;
            let damped_velocity = velocity * frame_damping;
            let temp = joint.pos;
            self.joints[i].pos += damped_velocity;
            self.joints[i].old_pos = temp;
        }

        // 2. Iterative distance constraints (segments may not stretch).
        for _ in 0..self.constraint_iterations {
            for i in 0..self.joints.len() - 1 {
                let a = self.joints[i].pos;
                let b = self.joints[i + 1].pos;
                let delta = b - a;
                let dist = delta.length();

                if dist > self.segment_length && dist > 0.0 {
                    let diff = (dist - self.segment_length) / dist;
                    let correction = delta * diff;

                    let (weight_a, weight_b) = if i == 0 {
                        (0.0, 1.0) // start anchor is immovable
                    } else if i + 1 == self.joints.len() - 1 {
                        (1.0, 0.0) // end anchor is immovable
                    } else {
                        (0.5, 0.5)
                    };

                    self.joints[i].pos += correction * weight_a;
                    self.joints[i + 1].pos -= correction * weight_b;
                }
            }
        }

        // 3. Continuous straightening based on stretch.
        //
        // As the chain extends, it progressively resists bending.  The
        // correction pulls each internal joint toward the midpoint of its
        // neighbours; the strength is proportional to stretch, giving a
        // smooth visual transition from floppy to stiff.
        let stretch = self.stretch();
        if stretch > 0.0 {
            let strength = self.straightness * stretch;
            for _ in 0..3 {
                for i in 1..self.joints.len() - 1 {
                    let ideal = (self.joints[i - 1].pos + self.joints[i + 1].pos) * 0.5;
                    let correction = ideal - self.joints[i].pos;
                    self.joints[i].pos += correction * strength;
                }
            }
        }
    }

    /// Draw the chain.
    ///
    /// A thick line is always drawn between consecutive joints.  When
    /// [`show_debug`](Self::show_debug) is enabled each joint is also drawn as
    /// a square.
    pub fn draw(&self, canvas: &mut Canvas) {
        // Draw connecting lines.
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

    /// Given a current position and a desired movement delta, return a clamped
    /// delta that keeps the entity within [`max_length`](Self::max_length) of
    /// the chain's fixed start point.
    ///
    /// The returned delta preserves the tangential component (allowing the
    /// entity to slide along the circular boundary) while capping the radial
    /// component so the chain can never be overextended.
    pub fn constrain_movement(&self, current_pos: Vec2D, desired_delta: Vec2D) -> Vec2D {
        let fixed = self.start();
        let next_pos = current_pos + desired_delta;
        let next_dist = fixed.distance(next_pos);
        let max_dist = self.max_length();

        if next_dist <= max_dist {
            return desired_delta;
        }

        // Clamp next_pos to the circle of radius max_dist around fixed.
        let dir = (next_pos - fixed).try_normalize().unwrap_or(Vec2D::X);
        let clamped_pos = fixed + dir * max_dist;
        clamped_pos - current_pos
    }

    /// Iterator over the bounding rectangles of each virtual link.
    ///
    /// These are axis-aligned squares centred on each joint.  In the future
    /// they can be rotated to follow the segment orientation.
    #[allow(dead_code)]
    pub fn link_rects(&self) -> impl Iterator<Item = Rect> + '_ {
        let half = self.link_size * 0.5;
        self.joints
            .iter()
            .map(move |j| Rect::new(j.pos.x - half, j.pos.y - half, self.link_size, self.link_size))
    }
}
