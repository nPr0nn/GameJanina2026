use juni::prelude::*;

pub struct Player {
    pub pos: Vec2D,
    shape: Vec2D,
    player_speed: f32,
    cow: Texture,
    portal_activated: bool,
    both_portal_activated: bool,
    portal_in: Vec2D,
    portal_out: Vec2D,
    last_portal_ativated_in: bool,
    portal_shape: Vec2D,
}

impl Player {
    pub fn new(cow: Texture) -> Player {
        Self {
            pos: Vec2D::ZERO,
            shape: Vec2D::new(100.0, 100.0), // Define o tamanho do jogador
            player_speed: 500.0, // Define a velocidade do jogador
            cow: cow,
            portal_activated: false,
            both_portal_activated: false,
            last_portal_ativated_in: false,
            portal_in: Vec2D::ZERO,
            portal_out: Vec2D::ZERO,
            portal_shape: Vec2D::new(100.0, 100.0), // Define o tamanho do portal

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
        if ctx.is_key_pressed(Key::Space) {
            self.portal_activated = true;
            if self.last_portal_ativated_in {
                self.last_portal_ativated_in = false;
                self.portal_out = self.pos.clone();
                self.both_portal_activated = true;
                println!("Portal deactivated at position: {:?}", self.portal_out);
            } else {
                self.portal_in = self.pos.clone();
                println!("Portal activated at position: {:?}", self.portal_in);
                self.last_portal_ativated_in = true;
            }
        }
        if dir != Vec2D::ZERO {
            dir = dir.normalize();
            self.pos += dir * self.player_speed * ctx.dt;
        }

        if let Some(new_pos) = self.detect_portal_collision() {
            self.pos = new_pos;
        }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        // Draw the player as a simple circle.
        if self.portal_activated {
            // circle
            canvas.circle(Vec2D::new(self.portal_in.x + self.portal_shape.x / 2.0, self.portal_in.y + self.portal_shape.y / 2.0), self.portal_shape.x / 2.0, BLUE);
            canvas.circle(Vec2D::new(self.portal_out.x + self.portal_shape.x / 2.0, self.portal_out.y + self.portal_shape.y / 2.0), self.portal_shape.x / 2.0, ORANGE);
        }
            
        // canvas.rectangle(self.pos.x, self.pos.y, self.shape.x, self.shape.y, BLACK);
        canvas.draw_texture_ex(&self.cow, self.pos, 0.0, 6.0, WHITE);
    }

    fn detect_portal_collision(&self) -> Option<Vec2D> {
        if self.portal_activated && self.both_portal_activated {
            let player_rect = Rect::new(self.pos.x, self.pos.y, self.shape.x, self.shape.y);
            
            let portal_in_rect = Rect::new(self.portal_in.x, self.portal_in.y, self.portal_shape.x, self.portal_shape.y);
            let portal_out_rect = Rect::new(self.portal_out.x, self.portal_out.y, self.portal_shape.x, self.portal_shape.y);

            if player_rect.intersects(&portal_in_rect) {
                return Some(self.portal_out);
            } else if player_rect.intersects(&portal_out_rect) {
                return Some(self.portal_in);
            }
        }
        None
    }
}