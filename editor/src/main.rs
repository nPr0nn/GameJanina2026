// A tiny mouse-driven level editor for juni.
//
//   cargo run -p editor
//
// Left-drag to place the current shape, right-click to delete the shape under
// the cursor, and save with S. Middle-drag pans and the wheel zooms a
// `Camera2D`, so the canvas can be scrolled around a level larger than the
// screen. Shapes are authored and stored in *world* coordinates (what the
// camera looks at), so the game must view them through a camera too — see
// `juni::level`. The output (`level.json` in the working directory) is the
// exact file the game loads.

use juni::prelude::*;
use std::{
    io::{self, Write},
    path::Path,
    sync::OnceLock,
};

const RENDER_W: u32 = 1280;
const RENDER_H: u32 = 720;
const GRID_SIZE: f32 = 32.0;
const GRID_MAJOR_EVERY: i32 = 4;

/// The primitive the left mouse button currently places.
#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Rect,
    Circle,
}

/// Color choices, selectable with the number keys 1–6.
const PALETTE: [Color; 6] = [RED, ORANGE, GOLD, LIME, SKYBLUE, VIOLET];

#[derive(Debug)]
struct StartupConfig {
    path: String,
    level: Level,
    status: String,
}

static STARTUP: OnceLock<StartupConfig> = OnceLock::new();

struct Editor {
    current_path: String,
    level: Level,
    tool: Tool,
    color: Color,
    /// Where (in world space) the left button went down, while a drag is in
    /// progress.
    drag_start: Option<Vec2D>,
    /// Cursor in screen (canvas) space; the crosshair is drawn here.
    mouse: Vec2D,
    /// Where the camera looks (world point at the screen's top-left when zoom
    /// is 1). Panned with the middle mouse button.
    target: Vec2D,
    /// Camera zoom; the wheel scales it about the cursor.
    zoom: f32,
    /// Cursor position when the middle button was last seen down, for panning.
    pan_last: Option<Vec2D>,
    /// Transient status line (last action), shown in the HUD.
    status: String,
    /// Whether the multiline help overlay is visible.
    show_help: bool,
}

impl Editor {
    /// The current view. `offset` is the origin so that at `target = 0`,
    /// `zoom = 1` world coordinates map 1:1 to the screen.
    fn camera(&self) -> Camera2D {
        Camera2D {
            offset: Vec2D::ZERO,
            target: self.target,
            rotation: 0.0,
            zoom: self.zoom,
        }
    }

    /// The cursor in world space, through the current camera.
    fn mouse_world(&self) -> Vec2D {
        self.camera().screen_to_world(self.mouse)
    }

    /// Draw a world-space planning grid that follows the camera.
    fn draw_grid(&self, canvas: &mut Canvas) {
        let camera = self.camera();
        let top_left = camera.screen_to_world(Vec2D::ZERO);
        let bottom_right = camera.screen_to_world(Vec2D::new(RENDER_W as f32, RENDER_H as f32));

        let min_x = (top_left.x / GRID_SIZE).floor() as i32;
        let max_x = (bottom_right.x / GRID_SIZE).ceil() as i32;
        let min_y = (top_left.y / GRID_SIZE).floor() as i32;
        let max_y = (bottom_right.y / GRID_SIZE).ceil() as i32;

        for ix in min_x..=max_x {
            let x = ix as f32 * GRID_SIZE;
            let is_major = ix.rem_euclid(GRID_MAJOR_EVERY) == 0;
            let color = if is_major {
                LIGHTGRAY.with_alpha(0.30)
            } else {
                LIGHTGRAY.with_alpha(0.12)
            };

            canvas.line(
                Vec2D::new(x, top_left.y),
                Vec2D::new(x, bottom_right.y),
                1.0,
                color,
            );
        }

        for iy in min_y..=max_y {
            let y = iy as f32 * GRID_SIZE;
            let is_major = iy.rem_euclid(GRID_MAJOR_EVERY) == 0;
            let color = if is_major {
                LIGHTGRAY.with_alpha(0.30)
            } else {
                LIGHTGRAY.with_alpha(0.12)
            };

            canvas.line(
                Vec2D::new(top_left.x, y),
                Vec2D::new(bottom_right.x, y),
                1.0,
                color,
            );
        }
    }
}

/// Build a shape from two drag points, or `None` if it's too small to keep.
fn make_shape(tool: Tool, a: Vec2D, b: Vec2D, color: Color) -> Option<Shape> {
    match tool {
        Tool::Rect => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let width = (a.x - b.x).abs();
            let height = (a.y - b.y).abs();
            (width >= 2.0 && height >= 2.0).then_some(Shape::Rect {
                x,
                y,
                width,
                height,
                color,
            })
        }
        Tool::Circle => {
            let radius = a.distance(b);
            (radius >= 2.0).then_some(Shape::Circle {
                x: a.x,
                y: a.y,
                radius,
                color,
            })
        }
    }
}

fn prompt_level_path() -> io::Result<String> {
    loop {
        print!("Level path: ");
        io::stdout().flush()?;

        let mut line = String::new();
        io::stdin().read_line(&mut line)?;

        let path = line.trim();
        if !path.is_empty() {
            return Ok(path.to_string());
        }

        eprintln!("A level path is required.");
    }
}

fn load_or_create_level(path: &str) -> io::Result<StartupConfig> {
    if Path::new(path).exists() {
        let level = Level::load(path)?;
        let n = level.shapes.len();
        Ok(StartupConfig {
            path: path.to_string(),
            level,
            status: format!("Loaded {path} ({n} shapes)"),
        })
    } else {
        if let Some(parent) = Path::new(path)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }

        let level = Level::new();
        level.save(path)?;
        Ok(StartupConfig {
            path: path.to_string(),
            level,
            status: format!("Created {path} (new level)"),
        })
    }
}

impl Game for Editor {
    fn init(_ctx: &mut Context) -> Self {
        let startup = STARTUP
            .get()
            .expect("editor startup config should be set before run()");

        Self {
            current_path: startup.path.clone(),
            level: startup.level.clone(),
            tool: Tool::Rect,
            color: PALETTE[0],
            drag_start: None,
            mouse: Vec2D::ZERO,
            target: Vec2D::ZERO,
            zoom: 1.0,
            pan_last: None,
            status: startup.status.clone(),
            show_help: false,
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        self.mouse = ctx.mouse_position();

        if ctx.is_key_pressed(Key::Escape) {
            ctx.exit();
        }
        if ctx.is_key_pressed(Key::H) {
            self.show_help = !self.show_help;
            self.status = if self.show_help {
                "Help shown".to_string()
            } else {
                "Help hidden".to_string()
            };
        }

        // --- Camera: middle-drag pans, wheel zooms about the cursor. ---
        if ctx.is_mouse_button_down(MouseButton::Middle) {
            if let Some(prev) = self.pan_last {
                // Move the world under the cursor by the screen delta.
                self.target -= (self.mouse - prev) / self.zoom;
            }
            self.pan_last = Some(self.mouse);
        } else {
            self.pan_last = None;
        }

        let wheel = ctx.mouse_wheel_move();
        if wheel != 0.0 {
            // Keep the world point under the cursor fixed while zooming.
            let before = self.mouse_world();
            self.zoom = (self.zoom * (1.0 + wheel * 0.1)).clamp(0.1, 10.0);
            let after = self.mouse_world();
            self.target += before - after;
        }
        // F resets the view to the origin at 1:1.
        if ctx.is_key_pressed(Key::F) {
            self.target = Vec2D::ZERO;
            self.zoom = 1.0;
            self.status = "View reset".to_string();
        }

        // The cursor in world space — what shapes are placed and hit-tested in.
        let world = self.mouse_world();

        // Tool selection.
        if ctx.is_key_pressed(Key::R) {
            self.tool = Tool::Rect;
        }
        if ctx.is_key_pressed(Key::C) {
            self.tool = Tool::Circle;
        }

        // Color selection (number keys 1–6).
        for (i, key) in [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
        ]
        .iter()
        .enumerate()
        {
            if ctx.is_key_pressed(*key) {
                self.color = PALETTE[i];
            }
        }

        // Left button: drag to place a shape (in world space).
        if ctx.is_mouse_button_pressed(MouseButton::Left) {
            self.drag_start = Some(world);
        }
        if ctx.is_mouse_button_released(MouseButton::Left) {
            if let Some(start) = self.drag_start.take() {
                if let Some(shape) = make_shape(self.tool, start, world, self.color) {
                    self.level.shapes.push(shape);
                    self.status = format!("Placed shape ({} total)", self.level.shapes.len());
                }
            }
        }

        // Right click: delete the topmost shape under the cursor.
        if ctx.is_mouse_button_pressed(MouseButton::Right) {
            if let Some(i) = self.level.shapes.iter().rposition(|s| s.contains(world)) {
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
            self.status = match self.level.save(&self.current_path) {
                Ok(()) => format!(
                    "Saved {} ({} shapes)",
                    self.current_path,
                    self.level.shapes.len()
                ),
                Err(e) => format!("Save failed: {e}"),
            };
        }
        if ctx.is_key_pressed(Key::O) {
            self.status = match Level::load(&self.current_path) {
                Ok(level) => {
                    self.level = level;
                    format!(
                        "Reloaded {} ({} shapes)",
                        self.current_path,
                        self.level.shapes.len()
                    )
                }
                Err(e) => format!("Load failed: {e}"),
            };
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        canvas.clear_background(DARKGRAY);

        // Everything world-space (the level and the drag preview) goes through
        // the camera, so panning/zooming move them together.
        canvas.begin_mode_2d(self.camera());

        // Planning grid behind the level content.
        self.draw_grid(canvas);

        // Placed shapes.
        self.level.draw(canvas);

        // Live preview of the shape currently being dragged out, drawn a little
        // translucent so it reads as "not yet placed".
        if let Some(start) = self.drag_start {
            if let Some(shape) = make_shape(
                self.tool,
                start,
                self.mouse_world(),
                self.color.with_alpha(0.5),
            ) {
                shape.draw(canvas);
            }
        }

        canvas.end_mode_2d();

        // Crosshair at the cursor (screen space).
        canvas.line(
            self.mouse - Vec2D::new(10.0, 0.0),
            self.mouse + Vec2D::new(10.0, 0.0),
            1.0,
            WHITE,
        );
        canvas.line(
            self.mouse - Vec2D::new(0.0, 10.0),
            self.mouse + Vec2D::new(0.0, 10.0),
            1.0,
            WHITE,
        );

        // --- HUD ---

        if self.show_help {
            canvas.rectangle(20.0, 120.0, 520.0, 330.0, BLACK.with_alpha(0.8));
            canvas.text("Editor controls", 40.0, 140.0, 28.0, GOLD);

            canvas.text("Mouse", 40.0, 182.0, 24.0, WHITE);
            canvas.text("L-drag      Place shape", 60.0, 210.0, 22.0, LIGHTGRAY);
            canvas.text("R-click     Delete shape", 60.0, 236.0, 22.0, LIGHTGRAY);
            canvas.text("M-drag      Pan camera", 60.0, 262.0, 22.0, LIGHTGRAY);
            canvas.text("Wheel       Zoom", 60.0, 288.0, 22.0, LIGHTGRAY);

            canvas.text("Tools", 40.0, 330.0, 24.0, WHITE);
            canvas.text("R / C       Select tool", 60.0, 358.0, 22.0, LIGHTGRAY);
            canvas.text("1-6         Select color", 60.0, 384.0, 22.0, LIGHTGRAY);

            canvas.text("File", 290.0, 182.0, 24.0, WHITE);
            canvas.text("S           Save level", 310.0, 210.0, 22.0, LIGHTGRAY);
            canvas.text("O           Reload level", 310.0, 236.0, 22.0, LIGHTGRAY);

            canvas.text("Other", 290.0, 288.0, 24.0, WHITE);
            canvas.text("Z           Undo last", 310.0, 316.0, 22.0, LIGHTGRAY);
            canvas.text(
                "X           Clear all shapes",
                310.0,
                342.0,
                22.0,
                LIGHTGRAY,
            );
            canvas.text("F           Reset view", 310.0, 368.0, 22.0, LIGHTGRAY);
            canvas.text(
                "H           Toggle this help",
                310.0,
                394.0,
                22.0,
                LIGHTGRAY,
            );
            canvas.text("Esc         Quit", 310.0, 420.0, 22.0, LIGHTGRAY);
        }

        canvas.text(&self.status, 20.0, RENDER_H as f32 - 32.0, 22.0, GOLD);
    }
}

fn main() {
    let path = match prompt_level_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Failed to read level path: {e}");
            std::process::exit(1);
        }
    };

    let startup = match load_or_create_level(&path) {
        Ok(startup) => startup,
        Err(e) => {
            eprintln!("Failed to open or create level at {path}: {e}");
            std::process::exit(1);
        }
    };

    STARTUP
        .set(startup)
        .expect("editor startup config should only be set once");

    run::<Editor>(Config {
        width: 1920,
        height: 1080,
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
