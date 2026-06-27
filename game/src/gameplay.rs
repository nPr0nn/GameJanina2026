//! The gameplay screen: the original shapes/texture/audio demo, wrapped so the
//! screen manager can create, reset, update, and draw it. It owns only world
//! state — global keys (pause, fullscreen, win/lose) are handled by the screen
//! manager in `main.rs`.

use juni::prelude::*;

// A custom fragment shader: an animated rainbow driven by world position and
// `globals.time`. Same vertex/uniform interface as the built-in shape shader,
// so it plugs straight into `begin_shader_mode`.
const RAINBOW_SHADER: &str = r#"
struct Globals {
    proj: mat4x4<f32>,
    time: f32,
};
@group(0) @binding(0) var<uniform> globals: Globals;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) world: vec2<f32>,
};

@vertex
fn vs_main(@location(0) position: vec2<f32>, @location(1) color: vec4<f32>) -> VsOut {
    var out: VsOut;
    out.clip = globals.proj * vec4<f32>(position, 0.0, 1.0);
    out.world = position;
    return out;
}

// Hue (0..1) -> RGB.
fn hue(h: f32) -> vec3<f32> {
    let r = abs(h * 6.0 - 3.0) - 1.0;
    let g = 2.0 - abs(h * 6.0 - 2.0);
    let b = 2.0 - abs(h * 6.0 - 4.0);
    return clamp(vec3<f32>(r, g, b), vec3<f32>(0.0), vec3<f32>(1.0));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let h = fract((in.world.x + in.world.y) * 0.0015 + globals.time * 0.2);
    return vec4<f32>(hue(h), 1.0);
}
"#;

pub struct Gameplay {
    x: f32,
    dir: f32,
    player: Vec2D,
    mouse: Vec2D,
    rainbow: Shader,
    cow: Texture,
    pop: Sound,
    spin: f32,
    zoom: f32,
    fps: u32,
}

impl Gameplay {
    pub fn new(ctx: &mut Context) -> Self {
        Self {
            x: 100.0,
            dir: 1.0,
            player: Vec2D::ZERO,
            mouse: Vec2D::ZERO,
            // Compile the custom shader once, up front (raylib's LoadShader).
            rainbow: ctx.load_shader_from_memory(RAINBOW_SHADER),
            // Embed + decode the texture and sound once.
            cow: ctx.load_texture_from_memory(include_bytes!("assets/vaca.png")),
            pop: ctx.load_sound_from_memory(include_bytes!("assets/bolha.wav")),
            spin: 0.0,
            zoom: 1.0,
            fps: 0,
        }
    }

    /// Reset world state for a fresh run (called when entering gameplay from the
    /// menu or after a win/defeat). Reuses the already-loaded GPU/audio assets.
    pub fn reset(&mut self) {
        self.x = 100.0;
        self.dir = 1.0;
        self.player = Vec2D::ZERO;
        self.spin = 0.0;
        self.zoom = 1.0;
    }

    /// Advance the world one fixed step. Only called while actually playing, so
    /// pausing freezes everything here automatically.
    pub fn update(&mut self, ctx: &mut Context) {
        self.mouse = ctx.mouse_position();
        self.fps = ctx.fps;

        // Play a pop on each left-click.
        if ctx.is_mouse_button_pressed(MouseButton::Left) {
            ctx.play_sound(&self.pop);
        }

        // Player movement.
        if ctx.is_key_down(Key::W) {
            self.player.y -= 5.0;
        }
        if ctx.is_key_down(Key::A) {
            self.player.x -= 5.0;
        }
        if ctx.is_key_down(Key::S) {
            self.player.y += 5.0;
        }
        if ctx.is_key_down(Key::D) {
            self.player.x += 5.0;
        }

        // Spin the rotating cow at 90 deg/sec.
        self.spin += 90.0 * ctx.dt;

        // Mouse wheel zooms the camera in/out (clamped).
        self.zoom = (self.zoom + ctx.mouse_wheel_move() * 0.1).clamp(0.1, 4.0);

        // Fixed-timestep movement: 240 virtual px/sec, bouncing in [100, 1080].
        self.x += self.dir * 240.0 * ctx.dt;
        if self.x > 1080.0 {
            self.x = 1080.0;
            self.dir = -1.0;
        } else if self.x < 100.0 {
            self.x = 100.0;
            self.dir = 1.0;
        }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.clear_background(RED);

        // A 2D camera following the player and zooming with the wheel.
        let camera = Camera2D {
            target: self.player + Vec2D::new(50.0, 50.0),
            offset: Vec2D::new(640.0, 360.0),
            rotation: 0.0,
            zoom: self.zoom,
        };
        canvas.begin_mode_2d(camera);

        canvas.rectangle(60.0, 60.0, 300.0, 180.0, SKYBLUE);
        canvas.rectangle_from_rect(Rect::new(60.0, 300.0, 300.0, 180.0), GOLD);
        canvas.triangle(
            Vec2D::new(640.0, 120.0),
            Vec2D::new(540.0, 320.0),
            Vec2D::new(740.0, 320.0),
            MAROON,
        );
        canvas.quad(
            Vec2D::new(820.0, 120.0),
            Vec2D::new(1120.0, 120.0),
            Vec2D::new(1060.0, 320.0),
            Vec2D::new(760.0, 320.0),
            DARKGREEN,
        );

        // Animated rainbow quad via the custom shader.
        canvas.begin_shader_mode(&self.rainbow);
        canvas.quad(
            Vec2D::new(400.0, 300.0),
            Vec2D::new(500.0, 300.0),
            Vec2D::new(500.0, 480.0),
            Vec2D::new(400.0, 480.0),
            RED,
        );
        canvas.end_shader_mode();

        canvas.regular_polygon(Vec2D::new(1140.0, 480.0), 5, 70.0, -90.0, ORANGE);
        canvas.draw_texture_ex(&self.cow, Vec2D::new(520.0, 230.0), 180.0, 6.0, WHITE);

        let size = self.cow.width() as f32 * 4.0;
        canvas.draw_texture_pro(
            &self.cow,
            Rect::new(0.0, 0.0, self.cow.width() as f32, self.cow.height() as f32),
            Rect::new(1180.0, 600.0, size, size),
            Vec2D::new(size / 2.0, size / 2.0),
            self.spin,
            RED,
        );

        let center = camera.screen_to_world(Vec2D::new(640.0, 360.0));
        let cursor = camera.screen_to_world(self.mouse);
        canvas.line(center, cursor, 5.0, DARKBLUE);

        canvas.rectangle(self.x, 520.0, 100.0, 100.0, RED);
        canvas.rectangle(self.player.x, self.player.y, 100.0, 100.0, BLACK);
        canvas.draw_texture_ex(&self.cow, self.player, 0.0, 6.0, WHITE);

        canvas.end_mode_2d();

        // HUD (screen space).
        canvas.text(&format!("FPS: {}", self.fps), 20.0, 20.0, 28.0, LIME);
        canvas.text(
            "WASD move · wheel zoom · P pause · K defeat · L win",
            20.0,
            680.0,
            24.0,
            WHITE,
        );
    }
}
