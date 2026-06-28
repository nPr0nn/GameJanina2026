//! World-level portal system. Portals are placed by the player (Space) and come
//! in pairs: stepping into one circle teleports you to its partner. The world
//! can hold any number of completed pairs.
//!
//! Portals are rendered with the engine's sprite-sheet animation system:
//!   * opening animation when placed,
//!   * looping idle spin while active,
//!   * closing animation when the level resets.
//!
//! Each completed pair also gets a small coloured side ball (the same colour on
//! both portals) so pairs are easy to tell apart.

use juni::prelude::*;

use crate::animation::{Animation, SpriteSheet};

/// Radius of every placed portal, in world pixels.
const PORTAL_RADIUS: f32 = 30.0;
/// Size of one portal sprite frame, in texture pixels.
const PORTAL_FRAME_SIZE: f32 = 64.0;
/// Animation frames per second for the portal effects.
const PORTAL_FPS: f32 = 10.0;
/// Sprite-sheet row for the idle spin loop.
const ROW_IDLE: u32 = 0;
/// Sprite-sheet row for the opening animation.
const ROW_OPENING: u32 = 1;
/// Sprite-sheet row for the closing animation.
const ROW_CLOSING: u32 = 2;
/// Radius of the small pair-identifying ball, in world pixels.
const PAIR_BALL_RADIUS: f32 = 1.5;
/// Offset of the pair ball from the portal centre (upper-right).
const PAIR_BALL_OFFSET: Vec2D = Vec2D::new(0.75, -0.75);

const PAIR_COLORS: &[Color] = &[
    RED,
    SKYBLUE,
    YELLOW,
    LIME,
    PINK,
    ORANGE,
    VIOLET,
    DARKGREEN,
    BROWN,
    DARKPURPLE,
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum PortalState {
    Opening,
    Idle,
    Closing,
}

/// A single placed portal: its collision/teleport circle plus its sprite anim.
struct PortalInstance {
    circle: Circle,
    sheet: SpriteSheet,
    anim: Animation,
    state: PortalState,
}

impl PortalInstance {
    fn new_open(circle: Circle, sheet: SpriteSheet) -> Self {
        Self {
            circle,
            sheet: sheet.clone(),
            anim: Animation::new(sheet, ROW_OPENING, PORTAL_FPS, false),
            state: PortalState::Opening,
        }
    }

    fn start_closing(&mut self) {
        if self.state == PortalState::Closing {
            return;
        }
        self.state = PortalState::Closing;
        self.anim = Animation::new(self.sheet.clone(), ROW_CLOSING, PORTAL_FPS, false);
    }

    fn update(&mut self, dt: f32) {
        self.anim.update(dt);
        if self.state == PortalState::Opening && self.anim.is_finished() {
            self.state = PortalState::Idle;
            self.anim = Animation::new(self.sheet.clone(), ROW_IDLE, PORTAL_FPS, true);
        }
    }
}

/// A linked pair of portal circles. Entering either teleports to the other.
/// The first portal placed (`a`) uses the purple sheet; its partner (`b`) uses
/// the green sheet. Both share a small colour ball so the pair is identifiable.
pub struct PortalPair {
    a: PortalInstance,
    b: PortalInstance,
    color: Color,
}

/// All portal pairs in the world, plus a half-placed portal waiting for its
/// partner.
pub struct Portals {
    pairs: Vec<PortalPair>,
    /// The first circle of a pair, placed but not yet paired. The next
    /// [`place`](Self::place) completes the pair.
    pending: Option<PortalInstance>,
    purple_sheet: SpriteSheet,
    green_sheet: SpriteSheet,
}

impl Portals {
    pub fn new(purple_sheet: SpriteSheet, green_sheet: SpriteSheet) -> Self {
        Self {
            pairs: Vec::new(),
            pending: None,
            purple_sheet,
            green_sheet,
        }
    }

    /// Place a portal at `center`. The first placement is held pending; the
    /// second completes a pair and is retained alongside any earlier pairs.
    pub fn place(&mut self, center: Vec2D) {
        let circle = Circle::new(center, PORTAL_RADIUS);
        let sheet = if self.pending.is_some() {
            self.green_sheet.clone()
        } else {
            self.purple_sheet.clone()
        };
        let instance = PortalInstance::new_open(circle, sheet);

        match self.pending.take() {
            None => self.pending = Some(instance),
            Some(first) => {
                let color = PAIR_COLORS[self.pairs.len() % PAIR_COLORS.len()];
                self.pairs.push(PortalPair {
                    a: first,
                    b: instance,
                    color,
                });
            }
        }
    }

    /// Advance every portal animation and promote finished openings to idle.
    pub fn update(&mut self, dt: f32) {
        for pair in &mut self.pairs {
            pair.a.update(dt);
            pair.b.update(dt);
        }
        if let Some(p) = self.pending.as_mut() {
            p.update(dt);
        }

        // Remove pairs whose closing animation has finished on both ends.
        self.pairs.retain(|pair| {
            !(pair.a.state == PortalState::Closing
                && pair.a.anim.is_finished()
                && pair.b.state == PortalState::Closing
                && pair.b.anim.is_finished())
        });
        if self
            .pending
            .as_ref()
            .is_some_and(|p| p.state == PortalState::Closing && p.anim.is_finished())
        {
            self.pending = None;
        }
    }

    /// Begin the closing animation on every live portal. Closing portals stop
    /// being interactive and are removed once the animation finishes.
    pub fn start_closing_all(&mut self) {
        for pair in &mut self.pairs {
            pair.a.start_closing();
            pair.b.start_closing();
        }
        if let Some(p) = self.pending.as_mut() {
            p.start_closing();
        }
    }

    /// `true` when every portal has finished closing (or there are none).
    pub fn is_finished_closing(&self) -> bool {
        let all_done = |i: &PortalInstance| i.state == PortalState::Closing && i.anim.is_finished();
        self.pairs.iter().all(|pair| all_done(&pair.a) && all_done(&pair.b))
            && self.pending.as_ref().is_none_or(all_done)
    }

    /// If `point` is inside some portal circle (other than `exclude`), return
    /// that entry circle together with the partner circle it teleports to.
    ///
    /// `exclude` is the circle the player just exited; skipping it prevents an
    /// immediate re-trigger while the player is still standing on the exit.
    /// Closing portals are ignored.
    pub fn find_entry(&self, point: Vec2D, exclude: Option<Circle>) -> Option<(Circle, Circle)> {
        for pair in &self.pairs {
            for (entry, exit) in [(&pair.a, &pair.b), (&pair.b, &pair.a)] {
                if entry.state == PortalState::Closing {
                    continue;
                }
                if Some(entry.circle) == exclude {
                    continue;
                }
                if entry.circle.intersects_point(point) {
                    return Some((entry.circle, exit.circle));
                }
            }
        }
        None
    }

    /// Draw every completed pair plus any pending half-placed portal.
    pub fn draw(&self, canvas: &mut Canvas) {
        for pair in &self.pairs {
            draw_instance(canvas, &pair.a);
            draw_instance(canvas, &pair.b);
            draw_pair_ball(canvas, pair.a.circle.center, pair.color);
            draw_pair_ball(canvas, pair.b.circle.center, pair.color);
        }
        if let Some(p) = &self.pending {
            draw_instance(canvas, p);
            draw_pair_ball(canvas, p.circle.center, LIGHTGRAY);
        }
    }
}

/// Draw one portal sprite centered on its circle.
fn draw_instance(canvas: &mut Canvas, instance: &PortalInstance) {
    let scale = instance.circle.radius * 2.0 / PORTAL_FRAME_SIZE;
    let pos = instance.circle.center - Vec2D::splat(instance.circle.radius);
    let tint = if instance.state == PortalState::Closing {
        WHITE.with_alpha(0.6)
    } else {
        WHITE
    };
    instance.anim.draw(canvas, pos, scale, false, tint);
}

/// Draw the small coloured ball that identifies which portals belong to the
/// same pair.
fn draw_pair_ball(canvas: &mut Canvas, center: Vec2D, color: Color) {
    let offset = PAIR_BALL_OFFSET * PORTAL_RADIUS;
    canvas.circle(center + offset, PAIR_BALL_RADIUS, color);
}
