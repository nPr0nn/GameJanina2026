use juni::prelude::*;

use crate::animation::{Animation, SpriteSheet};

/// Row indices into `assets/ducky_spritesheet.png` (each row is one state).
const ANIM_IDLE: u32 = 0;
const ANIM_WALK: u32 = 1;
/// Pixel scale applied to the 32×32 ducky frames when drawn.
const SPRITE_SCALE: f32 = 4.0;

pub struct Player {
    pub pos: Vec2D,
    pub shape: Vec2D,
    pub speed: f32,
    pub player_speed: f32,
    pub velocity: Vec2D,
    /// The ducky sprite-sheet animation that represents the player on screen.
    anim: Animation,
    /// `true` when the ducky should be mirrored to face left.
    facing_left: bool,
}

impl Player {
    pub fn new(sheet: SpriteSheet) -> Player {
        Self {
            pos: Vec2D::ZERO,
            shape: Vec2D::new(100.0, 100.0), // Player hit-box size
            speed: 500.0,                    // Speed used by the chain system
            player_speed: 500.0,             // Speed used by the movable box push
            velocity: Vec2D::ZERO,
            // Loop the idle row to start; `input_direction` switches to walking.
            anim: Animation::new(sheet, ANIM_IDLE, 10.0, true),
            facing_left: false,
        }
    }

    /// Centre of the player's hit-box in world space (portal entry test point).
    pub fn center(&self) -> Vec2D {
        self.pos + Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0)
    }

    /// Returns the normalized input direction based on WASD / arrow keys.
    /// Also updates the velocity field (used by the movable box).
    pub fn input_direction(&mut self, ctx: &Context) -> Vec2D {
        let mut dir = Vec2D::ZERO;
        self.velocity = Vec2D::ZERO;

        if ctx.is_key_down(Key::W) || ctx.is_key_down(Key::Up) {
            dir.y -= 1.0;
            self.velocity.y -= 1.0;
        }
        if ctx.is_key_down(Key::S) || ctx.is_key_down(Key::Down) {
            dir.y += 1.0;
            self.velocity.y += 1.0;
        }
        if ctx.is_key_down(Key::A) || ctx.is_key_down(Key::Left) {
            dir.x -= 1.0;
            self.velocity.x -= 1.0;
        }
        if ctx.is_key_down(Key::D) || ctx.is_key_down(Key::Right) {
            dir.x += 1.0;
            self.velocity.x += 1.0;
        }

        if dir != Vec2D::ZERO {
            dir = dir.normalize();
        }
        if self.velocity != Vec2D::ZERO {
            self.velocity = self.velocity.normalize();
        }

        // Drive the ducky animation from the movement input: walk while moving,
        // idle otherwise, and face the last horizontal direction travelled.
        self.anim
            .set_state(if dir == Vec2D::ZERO { ANIM_IDLE } else { ANIM_WALK });
        if dir.x < 0.0 {
            self.facing_left = true;
        } else if dir.x > 0.0 {
            self.facing_left = false;
        }
        self.anim.update(ctx.dt);

        dir
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        self.anim
            .draw(canvas, self.pos, SPRITE_SCALE, self.facing_left, WHITE);
    }
}
