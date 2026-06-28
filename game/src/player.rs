use juni::prelude::*;

use crate::animation::{Animation, SpriteSheet};

/// Row indices into `assets/ducky_spritesheet.png` (each row is one state).
const ANIM_IDLE: u32 = 0;
const ANIM_WALK: u32 = 1;
/// Pixel scale applied to the 32×32 ducky frames when drawn.
const SPRITE_SCALE: f32 = 1.0;

/// How fast the player gathers speed toward the input direction (px/s²).
const ACCELERATION: f32 = 1500.0;
/// How fast the player coasts to a stop when there is no input (px/s²).
const FRICTION: f32 = 4000.0;
/// Below this speed (px/s) the player is treated as idle (animation/facing).
const MOVING_EPS: f32 = 1.0;

pub struct Player {
    pub pos: Vec2D,
    pub shape: Vec2D,
    /// Maximum movement speed in px/s (the velocity magnitude is clamped to it).
    pub speed: f32,
    /// Current movement velocity in px/s. Integrated from input via
    /// [`Player::input_direction`]; the caller applies it against collision.
    pub velocity: Vec2D,
    /// Where the chain attaches on the player, as an offset from the top-left
    /// `pos` (so `(0,0)` is the corner, `shape/2` the centre). Tune this to move
    /// the tether point around within the sprite.
    pub chain_offset: Vec2D,
    /// The ducky sprite-sheet animation that represents the player on screen.
    anim: Animation,
    /// `true` when the ducky should be mirrored to face left.
    facing_left: bool,
    portal_activated: bool,
    both_portal_activated: bool,
    portal_in: Circle,
    portal_out: Circle,
    last_portal_ativated_in: bool,
    can_teleportate: bool,
}

impl Player {
    pub fn new(sheet: SpriteSheet) -> Player {
        Self {
            pos: Vec2D::ZERO,
            // Hit-box sized to the drawn duck (32×32 frame at SPRITE_SCALE 1).
            shape: Vec2D::new(28.0, 28.0),
            speed: 500.0, // Maximum movement speed (px/s)
            velocity: Vec2D::ZERO,
            chain_offset: Vec2D::new(14.0, 14.0), // Tether at the hit-box centre
            // Loop the idle row to start; `input_direction` switches to walking.
            anim: Animation::new(sheet, ANIM_IDLE, 10.0, true),
            facing_left: false,
            portal_activated: false,
            both_portal_activated: false,
            last_portal_ativated_in: false,
            portal_in: Circle::new(Vec2D::ZERO, 18.0),
            portal_out: Circle::new(Vec2D::ZERO, 18.0),
            can_teleportate: true,
        }
    }

    /// Reads WASD / arrow input and integrates the player's `velocity` from it
    /// (acceleration toward the input direction, friction when idle, clamped to
    /// `speed`). Also handles portal activation / teleportation. Returns the
    /// normalized input direction for callers that need the raw intent.
    pub fn input_direction(&mut self, ctx: &Context) -> Vec2D {
        let mut dir = Vec2D::ZERO;

        if ctx.is_key_down(Key::W) || ctx.is_key_down(Key::Up) {
            dir.y -= 1.0;
        }
        if ctx.is_key_down(Key::S) || ctx.is_key_down(Key::Down) {
            dir.y += 1.0;
        }
        if ctx.is_key_down(Key::A) || ctx.is_key_down(Key::Left) {
            dir.x -= 1.0;
        }
        if ctx.is_key_down(Key::D) || ctx.is_key_down(Key::Right) {
            dir.x += 1.0;
        }
        if dir != Vec2D::ZERO {
            dir = dir.normalize();
        }

        // Velocity integration: accelerate toward the input while keys are held,
        // otherwise shed speed with friction until the player coasts to a stop.
        if dir != Vec2D::ZERO {
            self.velocity += dir * ACCELERATION * ctx.dt;
            let speed = self.velocity.length();
            if speed > self.speed {
                self.velocity *= self.speed / speed;
            }
        } else {
            let speed = self.velocity.length();
            let drop = FRICTION * ctx.dt;
            self.velocity = if drop >= speed {
                Vec2D::ZERO
            } else {
                self.velocity * ((speed - drop) / speed)
            };
        }

        if ctx.is_key_pressed(Key::Space) {
            self.portal_activated = true;
            if self.last_portal_ativated_in {
                self.last_portal_ativated_in = false;
                self.portal_out.center = self.pos + Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0);
                self.both_portal_activated = true;
            } else {
                self.portal_in.center = self.pos + Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0);
                self.last_portal_ativated_in = true;
            }
        }

        if let Some(new_pos) = self.detect_portal_collision() {
            if self.can_teleportate {
                self.can_teleportate = false;
                self.pos = new_pos - Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0);
            }
        } else {
            self.can_teleportate = true;
        }

        // Drive the ducky animation from the actual motion: walk while the player
        // is moving (so it keeps walking while decelerating), idle once stopped,
        // and face the last horizontal direction travelled.
        let moving = self.velocity.length_squared() > MOVING_EPS * MOVING_EPS;
        self.anim
            .set_state(if moving { ANIM_WALK } else { ANIM_IDLE });
        if self.velocity.x < -MOVING_EPS {
            self.facing_left = true;
        } else if self.velocity.x > MOVING_EPS {
            self.facing_left = false;
        }
        self.anim.update(ctx.dt);

        dir
    }

    /// The player's collision hit-box (top-left `pos`, size `shape`) in world space.
    pub fn collider(&self) -> Rect {
        Rect::new(self.pos.x, self.pos.y, self.shape.x, self.shape.y)
    }

    /// World-space point where the chain attaches to the player.
    pub fn chain_point(&self) -> Vec2D {
        self.pos + self.chain_offset
    }

    /// Draw the player's collision hit-box and chain-attachment point. Called by
    /// the gameplay screen's F3 debug overlay so the collider and tether point
    /// line up with the level's collision layer.
    pub fn draw_collider(&self, canvas: &mut Canvas) {
        let r = self.collider();
        canvas.rectangle(r.x, r.y, r.width, r.height, GREEN.with_alpha(0.4));
        canvas.circle(self.chain_point(), 6.0, GOLD);
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        if self.portal_activated {
            canvas.circle(self.portal_in.center, self.portal_in.radius, PURPLE);
        }
        if self.both_portal_activated {
            canvas.circle(self.portal_out.center, self.portal_out.radius, PURPLE);
        }

        self.anim
            .draw(canvas, self.pos, SPRITE_SCALE, self.facing_left, WHITE);
    }

    fn detect_portal_collision(&self) -> Option<Vec2D> {
        if self.portal_activated && self.both_portal_activated {
            let center = self.pos + Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0);
            if self.portal_in.intersects_point(center) {
                return Some(self.portal_out.center);
            } else if self.portal_out.intersects_point(center) {
                return Some(self.portal_in.center);
            }
        }
        None
    }
}
