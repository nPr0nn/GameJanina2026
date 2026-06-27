// A tiny mouse-driven level editor for juni.
//
//   cargo run -p editor
//
// Left-drag to place the current shape, right-click to delete the shape under
// the cursor, and save with S. The output (`level.json` in the working
// directory) is the exact file the game loads — see `juni::level`.

use juni::level::DEFAULT_LEVEL_PATH;
use juni::prelude::*;

const RENDER_W: u32 = 1280;
const RENDER_H: u32 = 720;

/// The primitive the left mouse button currently places.
#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Rect,
    Circle,
}

/// Color choices, selectable with the number keys 1–6.
const PALETTE: [Color; 6] = [RED, ORANGE, GOLD, LIME, SKYBLUE, VIOLET];

struct Editor {
    level: Level,
    tool: Tool,
    color: Color,
    /// Where the left button went down, while a drag is in progress.
    drag_start: Option<Vec2D>,
    mouse: Vec2D,
    /// Transient status line (last action), shown in the HUD.
    status: String,
}

/// Build a shape from two drag points, or `None` if it's too small to keep.
fn make_shape(tool: Tool, a: Vec2D, b: Vec2D, color: Color) -> Option<Shape> {
    match tool {
        Tool::Rect => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let width = (a.x - b.x).abs();
            let height = (a.y - b.y).abs();
            (width >= 2.0 && height >= 2.0).then_some(Shape::Rect { x, y, width, height, color })
        }
        Tool::Circle => {
            let radius = a.distance(b);
            (radius >= 2.0).then_some(Shape::Circle { x: a.x, y: a.y, radius, color })
        }
    }
}

impl Game for Editor {
    fn init(_ctx: &mut Context) -> Self {
        // Pick up an existing level if one is already on disk, so editing is
        // iterative rather than starting blank every launch.
        let (level, status) = match Level::load(DEFAULT_LEVEL_PATH) {
            Ok(level) => {
                let n = level.shapes.len();
                (level, format!("Loaded {DEFAULT_LEVEL_PATH} ({n} shapes)"))
            }
            Err(_) => (Level::new(), format!("New level (no {DEFAULT_LEVEL_PATH} yet)")),
        };
        Self {
            level,
            tool: Tool::Rect,
            color: PALETTE[0],
            drag_start: None,
            mouse: Vec2D::ZERO,
            status,
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        self.mouse = ctx.mouse_position();

        if ctx.is_key_pressed(Key::Escape) {
            ctx.exit();
        }

        // Tool selection.
        if ctx.is_key_pressed(Key::R) {
            self.tool = Tool::Rect;
        }
        if ctx.is_key_pressed(Key::C) {
            self.tool = Tool::Circle;
        }

        // Color selection (number keys 1–6).
        for (i, key) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6]
            .iter()
            .enumerate()
        {
            if ctx.is_key_pressed(*key) {
                self.color = PALETTE[i];
            }
        }

        // Left button: drag to place a shape.
        if ctx.is_mouse_button_pressed(MouseButton::Left) {
            self.drag_start = Some(self.mouse);
        }
        if ctx.is_mouse_button_released(MouseButton::Left) {
            if let Some(start) = self.drag_start.take() {
                if let Some(shape) = make_shape(self.tool, start, self.mouse, self.color) {
                    self.level.shapes.push(shape);
                    self.status = format!("Placed shape ({} total)", self.level.shapes.len());
                }
            }
        }

        // Right click: delete the topmost shape under the cursor.
        if ctx.is_mouse_button_pressed(MouseButton::Right) {
            if let Some(i) = self.level.shapes.iter().rposition(|s| s.contains(self.mouse)) {
                self.level.shapes.remove(i);
                self.status = format!("Deleted shape ({} left)", self.level.shapes.len());
            }
        }

        // Z undo last, X clear all.
        if ctx.is_key_pressed(Key::Z) && self.level.shapes.pop().is_some() {
            self.status = format!("Undid last ({} left)", self.level.shapes.len());
        }
        if ctx.is_key_pressed(Key::X) {
            self.level.shapes.clear();
            self.status = "Cleared all shapes".to_string();
        }

        // S save, O reload from disk.
        if ctx.is_key_pressed(Key::S) {
            self.status = match self.level.save(DEFAULT_LEVEL_PATH) {
                Ok(()) => format!("Saved {DEFAULT_LEVEL_PATH} ({} shapes)", self.level.shapes.len()),
                Err(e) => format!("Save failed: {e}"),
            };
        }
        if ctx.is_key_pressed(Key::O) {
            self.status = match Level::load(DEFAULT_LEVEL_PATH) {
                Ok(level) => {
                    self.level = level;
                    format!("Reloaded {DEFAULT_LEVEL_PATH} ({} shapes)", self.level.shapes.len())
                }
                Err(e) => format!("Load failed: {e}"),
            };
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        canvas.clear_background(DARKGRAY);

        // Placed shapes.
        self.level.draw(canvas);

        // Live preview of the shape currently being dragged out, drawn a little
        // translucent so it reads as "not yet placed".
        if let Some(start) = self.drag_start {
            if let Some(shape) = make_shape(self.tool, start, self.mouse, self.color.with_alpha(0.5)) {
                shape.draw(canvas);
            }
        }

        // Crosshair at the cursor.
        canvas.line(self.mouse - Vec2D::new(10.0, 0.0), self.mouse + Vec2D::new(10.0, 0.0), 1.0, WHITE);
        canvas.line(self.mouse - Vec2D::new(0.0, 10.0), self.mouse + Vec2D::new(0.0, 10.0), 1.0, WHITE);

        // --- HUD ---
        let tool_name = match self.tool {
            Tool::Rect => "Rect",
            Tool::Circle => "Circle",
        };
        // Current color swatch + label.
        canvas.rectangle(20.0, 20.0, 28.0, 28.0, self.color);
        canvas.text(&format!("Tool: {tool_name}   (R rect · C circle)"), 60.0, 22.0, 26.0, WHITE);
        canvas.text("Color: 1-6", 60.0, 52.0, 22.0, LIGHTGRAY);

        canvas.text(
            "L-drag place · R-click delete · Z undo · X clear · S save · O reload · Esc quit",
            20.0,
            RENDER_H as f32 - 60.0,
            22.0,
            WHITE,
        );
        canvas.text(&self.status, 20.0, RENDER_H as f32 - 32.0, 22.0, GOLD);
    }
}

fn main() {
    run::<Editor>(Config {
        width: 1280,
        height: 720,
        render_width: RENDER_W,
        render_height: RENDER_H,
        title: "juni — level editor".to_string(),
        target_ups: 60,
        centered: true,
        resizable: false,
        msaa: 4,
        ..Config::default()
    });
}
