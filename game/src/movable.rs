use juni::prelude::*;


pub struct MovableBox {
    pub rect: Rect,
}

impl MovableBox {
    pub fn new(rect: Rect) -> Self {
        Self { rect }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.rectangle(self.rect.x, self.rect.y, self.rect.width, self.rect.height, BROWN);
    }

    pub fn update(&mut self, ctx: &mut Context) {

    }

    pub fn empurrar(&mut self, impulso: Vec2D) {
        let pos = self.rect.position() + impulso;
        self.rect.x = pos.x;
        self.rect.y = pos.y;
    }
        
}

