use juni::prelude::*;


pub struct MovableBox {
    pub pos: Vec2D,
    pub shape: Vec2D,
}

impl MovableBox {
    pub fn new(pos: Vec2D, shape: Vec2D) -> Self {
        Self { pos, shape }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.rectangle(self.pos.x, self.pos.y, self.shape.x, self.shape.y, BROWN);
    }

    pub fn update(&mut self, ctx: &mut Context) {

    }

    pub fn empurrar(&mut self, impulso: Vec2D) {
        self.pos += impulso;
    }
        
}

