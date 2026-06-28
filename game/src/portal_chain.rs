//! A chain that can thread through portals.
//!
//! A [`PortalChain`] holds an ordered list of [`Chain`] **snippets**. Only the
//! last one is *active* — simulated and following the player. Every earlier
//! snippet is *frozen*: pinned between two fixed anchors (the world anchor or a
//! portal exit on one side, a portal entrance on the other) and never
//! simulated. As the player drags rope through a portal, the frozen snippets
//! straighten toward taut, conserving a single fixed rope budget.
//!
//! # Rope conservation
//!
//! The chain owns a fixed `budget` (its total rope length). Each frozen snippet
//! consumes between `f_min` (its straight-line, fully-taut length) and `f_init`
//! (its captured path length at split time). The active snippet's reach cap is
//! `budget − Σ f_min`, which is what tethers the player at maximum stretch. The
//! per-frame [`apply_pullthrough`](PortalChain::apply_pullthrough) pass decides
//! how straight each frozen snippet currently is, nearest-the-player first.

use juni::prelude::*;

use crate::animation::SpriteSheet;
use crate::chain::Chain;
use crate::collision::ColliderTree;

/// Bookkeeping for one frozen snippet (parallel to `snippets[i]`).
struct FrozenMeta {
    /// The snippet's shape captured at split time (anchor → portal-in order).
    captured: Vec<Vec2D>,
    /// Path length of `captured` — the rope this snippet consumes when slack.
    f_init: f32,
    /// Straight-line `a`→`b` length — the rope it consumes when fully taut.
    f_min: f32,
    /// Start anchor (world anchor or a portal exit).
    a: Vec2D,
    /// End anchor (a portal entrance).
    b: Vec2D,
}

pub struct PortalChain {
    /// `[frozen…, active]`; always `frozen.len() + 1` entries.
    snippets: Vec<Chain>,
    frozen: Vec<FrozenMeta>,
    /// Total rope length, conserved across all snippets.
    budget: f32,
    link_size: f32,
    color: Color,
    /// Rest segment length carried into frozen snippets so links stay uniform.
    segment_length: f32,
    /// Sprite sheet shared by every snippet in this chain. Cloning is cheap.
    sheet: SpriteSheet,
}

impl PortalChain {
    /// Create an un-split chain (a single active snippet) from `start` to `end`.
    pub fn new(
        start: Vec2D,
        end: Vec2D,
        total_length: f32,
        link_size: f32,
        color: Color,
        sheet: SpriteSheet,
    ) -> Self {
        let active = Chain::new(start, end, total_length, link_size, color, sheet.clone());
        let segment_length = active.segment_length();
        Self {
            snippets: vec![active],
            frozen: Vec::new(),
            budget: total_length,
            link_size,
            color,
            segment_length,
            sheet,
        }
    }

    fn sum_f_min(&self) -> f32 {
        self.frozen.iter().map(|f| f.f_min).sum()
    }

    fn sum_f_init(&self) -> f32 {
        self.frozen.iter().map(|f| f.f_init).sum()
    }

    /// Reach cap of the active snippet: the rope left once every frozen snippet
    /// is pulled fully taut. Floored at one segment so `Chain::new` stays valid.
    fn active_budget(&self) -> f32 {
        (self.budget - self.sum_f_min()).max(self.segment_length)
    }

    fn active(&self) -> &Chain {
        self.snippets.last().expect("always at least one snippet")
    }

    fn active_mut(&mut self) -> &mut Chain {
        self.snippets.last_mut().expect("always at least one snippet")
    }

    /// Total rope budget (used to order chains for layered drawing).
    pub fn max_length(&self) -> f32 {
        self.budget
    }

    /// Pin the world anchor. Only meaningful while the chain is un-split — once
    /// frozen snippets exist, snippet[0]'s start is a captured fixed anchor.
    pub fn set_start(&mut self, anchor: Vec2D) {
        if self.frozen.is_empty() {
            self.snippets[0].set_start(anchor);
        }
    }

    /// Pin the active snippet's player-side end.
    pub fn set_end(&mut self, player_pt: Vec2D) {
        self.active_mut().set_end(player_pt);
    }

    /// Simulate the active snippet only.
    pub fn update_active(&mut self, dt: f32, obstacles: &mut ColliderTree) {
        self.active_mut().update(dt, obstacles);
    }

    /// Tether point + remaining free length for clamping the player. Delegates
    /// to the active snippet, whose `max_length` already encodes max stretch.
    pub fn active_tether(&self) -> (Vec2D, f32) {
        self.active().player_tether()
    }

    /// True when the active snippet has effectively stopped moving.
    pub fn is_still(&self, threshold: f32) -> bool {
        self.active().is_still(threshold)
    }

    /// All snippets (frozen then active), for drawing and squeeze detection.
    pub fn snippets(&self) -> &[Chain] {
        &self.snippets
    }

    /// Split the chain at a portal: freeze the active snippet's current shape
    /// (its player end snapped to `in_center`) and start a fresh, collapsed
    /// active snippet emerging from `out_center` at the player's new position.
    pub fn split(&mut self, in_center: Vec2D, out_center: Vec2D, player_pt: Vec2D) {
        let mut captured = self.active().path_points();
        // Snap the player-side end onto the portal entrance.
        if let Some(last) = captured.last_mut() {
            *last = in_center;
        }
        if captured.len() < 2 {
            return; // degenerate active snippet — nothing meaningful to freeze
        }
        let a = captured[0];
        let b = in_center;
        let f_init = polyline_length(&captured);
        let f_min = a.distance(b);
        let frozen_chain = Chain::from_points(
            &captured,
            self.segment_length,
            self.link_size,
            self.color,
            self.sheet.clone(),
        );

        // The old active snippet becomes frozen; append a new active snippet.
        let active_idx = self.snippets.len() - 1;
        self.snippets[active_idx] = frozen_chain;
        self.frozen.push(FrozenMeta {
            captured,
            f_init,
            f_min,
            a,
            b,
        });

        let new_active = Chain::new(
            out_center,
            player_pt,
            self.active_budget(),
            self.link_size,
            self.color,
            self.sheet.clone(),
        );
        self.snippets.push(new_active);
    }

    /// Undo the most recent split (the player went back through the portal the
    /// same way). Drops the active snippet and the frozen snippet that fed it,
    /// rebuilding a single active snippet from the merged anchor to the player.
    pub fn merge(&mut self, player_pt: Vec2D) {
        let Some(meta) = self.frozen.pop() else {
            return;
        };
        self.snippets.pop(); // remove the active snippet
        self.snippets.pop(); // remove the snippet that was frozen at this crossing
        let new_active = Chain::new(
            meta.a,
            player_pt,
            self.active_budget(),
            self.link_size,
            self.color,
            self.sheet.clone(),
        );
        self.snippets.push(new_active);
    }

    /// Straighten each frozen snippet according to how much rope the active
    /// snippet is currently pulling through. Call every frame after
    /// [`update_active`](Self::update_active).
    pub fn apply_pullthrough(&mut self) {
        if self.frozen.is_empty() {
            return;
        }
        let active_path = self.active().path_length();
        let available = self.budget - self.sum_f_init(); // active length before any pulling
        let slack = self.sum_f_init() - self.sum_f_min(); // total rope frozen snippets can give
        let mut remaining = (active_path - available).clamp(0.0, slack);

        // Nearest-the-player frozen snippet (last) gives up rope first.
        for idx in (0..self.frozen.len()).rev() {
            let (captured, a, b, give_cap) = {
                let m = &self.frozen[idx];
                (m.captured.clone(), m.a, m.b, m.f_init - m.f_min)
            };
            let t = if give_cap > 1e-4 {
                let give = remaining.min(give_cap);
                remaining -= give;
                give / give_cap
            } else {
                0.0
            };
            let n = captured.len();
            let straight: Vec<Vec2D> = (0..n)
                .map(|j| {
                    let frac = j as f32 / (n - 1) as f32;
                    let on_line = a + (b - a) * frac;
                    captured[j].lerp(on_line, t)
                })
                .collect();
            self.snippets[idx].set_joint_positions(&straight);
        }
    }

    /// Draw every snippet at the given opacity.
    pub fn draw(&self, canvas: &mut Canvas, alpha: f32) {
        for snippet in &self.snippets {
            snippet.draw(canvas, alpha);
        }
    }
}

/// Total length of a polyline.
fn polyline_length(points: &[Vec2D]) -> f32 {
    points.windows(2).map(|w| w[0].distance(w[1])).sum()
}
