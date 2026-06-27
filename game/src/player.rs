use juni::prelude::*;

pub struct Player {
    pub pos: Vec2D,
    pub shape: Vec2D,
    pub speed: f32,
    pub player_speed: f32,
    pub velocity: Vec2D,
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
            speed: 500.0,                    // Velocidade usada pelo sistema de correntes
            player_speed: 500.0,             // Velocidade usada pela caixa móvel
            velocity: Vec2D::ZERO,
            cow,
            portal_activated: false,
            both_portal_activated: false,
            last_portal_ativated_in: false,
            portal_in: Circle::new(Vec2D::ZERO, 50.0),
            portal_out: Circle::new(Vec2D::ZERO, 50.0),
            can_teleportate: true,
        }
    }

    /// Returns the normalized input direction based on WASD / arrow keys.
    /// Also updates the velocity field (used by the movable box) and handles
    /// portal activation / teleportation.
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

        if dir != Vec2D::ZERO {
            dir = dir.normalize();
        }
        if self.velocity != Vec2D::ZERO {
            self.velocity = self.velocity.normalize();
        }

        if let Some(new_pos) = self.detect_portal_collision() {
            if self.can_teleportate {
                self.can_teleportate = false;
                self.pos = new_pos - Vec2D::new(self.shape.x / 2.0, self.shape.y / 2.0);
            }
        } else {
            self.can_teleportate = true;
        }

        dir
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        if self.portal_activated {
            canvas.circle(self.portal_in.center, self.portal_in.radius, PURPLE);
        }
        if self.both_portal_activated {
            canvas.circle(self.portal_out.center, self.portal_out.radius, PURPLE);
        }

        canvas.draw_texture_ex(&self.cow, self.pos, 0.0, 6.0, WHITE);
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
