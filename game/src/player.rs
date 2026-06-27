use juni::prelude::*;

pub struct Player {
    pub pos: Vec2D,
    shape: Vec2D,
    PLAYER_SPEED: f32,
    cow: Texture,
}

impl Player {
    pub fn new(cow: Texture) -> Player {
        Self {
            pos: Vec2D::ZERO,
            shape: Vec2D::new(100.0, 100.0), // Define o tamanho do jogador
            PLAYER_SPEED: 500.0, // Define a velocidade do jogador
            cow: cow
        }
    }

    pub fn update(&mut self, ctx: &mut Context) {
        // Move the player with WASD or arrow keys.
        let mut dir = Vec2D::ZERO;
        if ctx.is_key_down(Key::W) || ctx.is_key_down(Key::Up) {
            dir.y -= 5.0;
        }
        if ctx.is_key_down(Key::S) || ctx.is_key_down(Key::Down) {
            dir.y += 5.0;
        }
        if ctx.is_key_down(Key::A) || ctx.is_key_down(Key::Left) {
            dir.x -= 5.0;
        }
        if ctx.is_key_down(Key::D) || ctx.is_key_down(Key::Right) {
            dir.x += 5.0;
        }
        if dir != Vec2D::ZERO {
            dir = dir.normalize();
            self.pos += dir * self.PLAYER_SPEED * ctx.dt;
        }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        // Draw the player as a simple circle.
        canvas.rectangle(self.pos.x, self.pos.y, self.shape.x, self.shape.y, BLACK);
        canvas.draw_texture_ex(&self.cow, self.pos, 0.0, 6.0, WHITE);
    }
}