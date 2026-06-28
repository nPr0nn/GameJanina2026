//! A pair of linked portals (`in` and `out`) that teleport whatever enters one
//! to the matching spot at the other.
//!
//! The teleport is **offset-preserving**: a point entering `in` re-appears at
//! `out` keeping the same position *relative to the portal centre*, so the whole
//! chain can flow through smoothly (a pure translation, no snapping to centre).
//!
//! Placement alternates `in` → `out` → `in` … on each [`try_place`](Portals::try_place).
//! A portal that a chain is currently threading **cannot be closed / moved**:
//! `try_place` refuses to re-place an already-placed portal while `blocked`.

use juni::prelude::*;

const FRAME_SIZE: f32 = 64.0;
const FRAME_COUNT: usize = 8;
const ANIM_FPS: f32 = 12.0;
const DRAW_SIZE: f32 = 150.0; // matches the 75.0 radius collision circle

/// Which portal the next placement targets.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Slot {
    In,
    Out,
}

pub struct Portals {
    in_portal: Circle,
    out_portal: Circle,
    in_placed: bool,
    out_placed: bool,
    next: Slot,
    in_texture: Texture,
    out_texture: Texture,
    anim_time: f32,
}

impl Portals {
    pub fn new(in_texture: Texture, out_texture: Texture) -> Self {
        Self {
            in_portal: Circle::new(Vec2D::ZERO, 75.0),
            out_portal: Circle::new(Vec2D::ZERO, 75.0),
            in_placed: false,
            out_placed: false,
            next: Slot::In,
            in_texture,
            out_texture,
            anim_time: 0.0,
        }
    }

    /// Forget both portals (used on level reset).
    pub fn clear(&mut self) {
        self.in_placed = false;
        self.out_placed = false;
        self.next = Slot::In;
        self.anim_time = 0.0;
    }

    /// Advance the portal animation. Call once per fixed update step.
    pub fn update(&mut self, dt: f32) {
        self.anim_time += dt;
    }

    /// Both portals exist, so teleporting is possible.
    pub fn active(&self) -> bool {
        self.in_placed && self.out_placed
    }

    pub fn in_center(&self) -> Vec2D {
        self.in_portal.center
    }

    pub fn out_center(&self) -> Vec2D {
        self.out_portal.center
    }

    pub fn radius(&self) -> f32 {
        self.in_portal.radius
    }

    /// Translation applied when travelling `in` → `out`. Reverse for `out` → `in`.
    pub fn displacement(&self) -> Vec2D {
        self.out_portal.center - self.in_portal.center
    }

    /// Place (or re-place) the next portal at `pos`.
    ///
    /// `blocked` is true when a chain is currently threading the portals; in that
    /// case an already-placed portal is **not** moved (you cannot close a portal a
    /// chain is crossing). Returns whether a placement happened.
    pub fn try_place(&mut self, pos: Vec2D, blocked: bool) -> bool {
        match self.next {
            Slot::In => {
                if self.in_placed && blocked {
                    return false;
                }
                self.in_portal.center = pos;
                self.in_placed = true;
                self.next = Slot::Out;
            }
            Slot::Out => {
                if self.out_placed && blocked {
                    return false;
                }
                self.out_portal.center = pos;
                self.out_placed = true;
                self.next = Slot::In;
            }
        }
        true
    }

    /// If `point` sits inside one of the active portals, return where it should
    /// re-appear at the other (offset-preserving) together with the phase delta
    /// it accrues (`+1` for `in` → `out`, `-1` for `out` → `in`). Otherwise `None`.
    pub fn teleport(&self, point: Vec2D) -> Option<(Vec2D, i32)> {
        if !self.active() {
            return None;
        }
        if self.in_portal.intersects_point(point) {
            Some((point + self.displacement(), 1))
        } else if self.out_portal.intersects_point(point) {
            Some((point - self.displacement(), -1))
        } else {
            None
        }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        let frame = ((self.anim_time * ANIM_FPS) as usize) % FRAME_COUNT;
        let src_x = frame as f32 * FRAME_SIZE;
        let src_y = 0.0; // top row: vertical portal frames
        let src = Rect::new(src_x, src_y, FRAME_SIZE, FRAME_SIZE);

        if self.in_placed {
            let center = self.in_portal.center;
            let dest = Rect::new(
                center.x - DRAW_SIZE * 0.5,
                center.y - DRAW_SIZE * 0.5,
                DRAW_SIZE,
                DRAW_SIZE,
            );
            canvas.draw_texture_pro(&self.in_texture, src, dest, Vec2D::ZERO, 0.0, WHITE);
        }
        if self.out_placed {
            let center = self.out_portal.center;
            let dest = Rect::new(
                center.x - DRAW_SIZE * 0.5,
                center.y - DRAW_SIZE * 0.5,
                DRAW_SIZE,
                DRAW_SIZE,
            );
            canvas.draw_texture_pro(&self.out_texture, src, dest, Vec2D::ZERO, 0.0, WHITE);
        }
    }
}
