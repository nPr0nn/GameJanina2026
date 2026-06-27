use juni::prelude::*;

pub struct Player {
    pub pos: Vec2D,
    shape: Vec2D,
    player_speed: f32,
    cow: Texture,
    portal_activated: bool,
    both_portal_activated: bool,
    portal_in: Circle,
    portal_out: Circle,
    last_portal_ativated_in: bool,
    can_teleportate: bool,
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
            portal_in: Circle::new(Vec2D::ZERO, 50.0),
            portal_out: Circle::new(Vec2D::ZERO, 50.0),
            can_teleportate: true,
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
                self.portal_out.center = self.pos.clone() + Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0); // Offset the second portal to the right
                self.both_portal_activated = true;
            } else {
                self.portal_in.center = self.pos.clone() + Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0); // Offset the first portal to the right
                self.last_portal_ativated_in = true;
            }
        }
        if dir != Vec2D::ZERO {
            dir = dir.normalize();
            self.pos += dir * self.player_speed * ctx.dt;
        }

        
        if let Some(new_pos) = self.detect_portal_collision() {
            if self.can_teleportate {
                self.can_teleportate = false; // Set the teleportation flag when colliding with portals
                self.pos = new_pos - Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0); // Offset the player to the left after teleportation
            }
        }
        else {
            self.can_teleportate = true; // Reset the teleportation flag when not colliding with portals
        }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        // Draw the player as a simple circle.
        if self.portal_activated {
            // circle
            canvas.circle(self.portal_in.center, self.portal_in.radius, PURPLE);
        }
        if self.both_portal_activated {
            // circle
            canvas.circle(self.portal_out.center, self.portal_out.radius, PURPLE);
        }
            
        // canvas.rectangle(self.pos.x, self.pos.y, self.shape.x, self.shape.y, BLACK);
        canvas.draw_texture_ex(&self.cow, self.pos, 0.0, 6.0, WHITE);
    }

    fn detect_portal_collision(&self) -> Option<Vec2D> {
        if self.portal_activated && self.both_portal_activated {
            if self.portal_in.intersects_point(self.pos+Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0))  {
                return Some(self.portal_out.center);
            } else if self.portal_out.intersects_point(self.pos+Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0)){
                return Some(self.portal_in.center);
            }
        }
        None
    }
}