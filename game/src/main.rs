// Run natively: cargo run
// Run on the web: trunk serve   (WebGPU, with WebGL2 fallback)

use juni::prelude::*;

// A custom fragment shader: an animated rainbow driven by world position and
// `globals.time`. Same vertex/uniform interface as the built-in shape shader,
// so it plugs straight into `begin_shader_mode`. Kept inline as a string here,
// but it could equally live in its own .wgsl file loaded with `include_str!`.
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

struct Demo {
    x: f32,
    dir: f32,
    player: Vec2D,
    mouse: Vec2D,
    block: Rect,
    rainbow: Shader,
    cow: Texture,
    pop: Sound,
    spin: f32,
    zoom: f32,
}

impl Game for Demo {
    fn init(ctx: &mut Context) -> Self {
        Demo {
            x: 100.0, dir: 1.0,
            player: Vec2D{ x: 0.0, y: 0.0 },
            mouse: Vec2D::ZERO,

            // A solid obstacle in the world. The player cannot walk through it.
            block: Rect::new(400.0, 400.0, 200.0, 60.0),

            // Compile the custom shader once, up front (raylib's LoadShader).
            rainbow: ctx.load_shader_from_memory(RAINBOW_SHADER),

            // Load the texture once. `include_bytes!` embeds the PNG (works for
            // `cargo run`, `--example`, and the web build); the engine decodes
            // and uploads it
            cow: ctx.load_texture_from_memory(include_bytes!("assets/vaca.png")),

            // Decode the WAV once up front (raylib's LoadSound). Played on right
            // click in update().
            pop: ctx.load_sound_from_memory(include_bytes!("assets/bolha.wav")),
            spin: 0.0,
            zoom: 1.0,
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        // Track the cursor in virtual-canvas coordinates.
        self.mouse = ctx.mouse_position();

        // Play a pop on each right-click.
        if ctx.is_mouse_button_pressed(MouseButton::Left) {
            ctx.play_sound(&self.pop);
        }

        // Press F to toggle fullscreen, Esc to quit.
        if ctx.is_key_pressed(Key::F) {
            ctx.toggle_fullscreen();
        }
        if ctx.is_key_pressed(Key::Escape) {
            ctx.exit();
        }

        // Player Movement with collision against the hard block.
        let mut velocity = Vec2D::ZERO;
        if ctx.is_key_down(Key::W) {
            velocity.y -= 5.0;
        }
        if ctx.is_key_down(Key::A) {
            velocity.x -= 5.0;
        }
        if ctx.is_key_down(Key::S) {
            velocity.y += 5.0;
        }
        if ctx.is_key_down(Key::D) {
            velocity.x += 5.0;
        }
        let player_rect = Rect::new(self.player.x, self.player.y, 100.0, 100.0);
        let resolved = move_rect(player_rect, velocity, &[self.block]);
        self.player = Vec2D::new(resolved.x, resolved.y);

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

    fn draw(&mut self, canvas: &mut Canvas) {
        canvas.clear_background(WHITE);

        // View the whole scene through a 2D camera that follows the player
        // (its center pinned to the screen center) and zooms with the wheel.
        // Everything until end_mode_2d is drawn in world space.
        let camera = Camera2D {
            target: self.player + Vec2D::new(50.0, 50.0),
            offset: Vec2D::new(640.0, 360.0),
            rotation: 0.0,
            zoom: self.zoom,
        };
        canvas.begin_mode_2d(camera);

        // Static rectangle.
        canvas.rectangle(60.0, 60.0, 300.0, 180.0, SKYBLUE);

        // Hard block: the player cannot pass through this.
        canvas.rectangle_from_rect(self.block, DARKGRAY);

        // Rectangle from a Rectangle struct.
        canvas.rectangle_from_rect(Rect::new(60.0, 300.0, 300.0, 180.0), GOLD);

        // A triangle.
        canvas.triangle(
            Vec2D::new(640.0, 120.0),
            Vec2D::new(540.0, 320.0),
            Vec2D::new(740.0, 320.0),
            MAROON,
        );

        // A free-form quad (parallelogram).
        canvas.quad(
            Vec2D::new(820.0, 120.0),
            Vec2D::new(1120.0, 120.0),
            Vec2D::new(1060.0, 320.0),
            Vec2D::new(760.0, 320.0),
            DARKGREEN,
        );

        // An animated rainbow quad, drawn with a custom shader. Everything
        // between begin/end_shader_mode uses `self.rainbow` instead of the
        // default shape shader (raylib's BeginShaderMode / EndShaderMode). The
        // quad's own vertex color is ignored — the fragment shader computes it.
        canvas.begin_shader_mode(&self.rainbow);
        canvas.quad(
            Vec2D::new(400.0, 300.0),
            Vec2D::new(500.0, 300.0),
            Vec2D::new(500.0, 480.0),
            Vec2D::new(400.0, 480.0),
            RED,
        );
        canvas.end_shader_mode();

        // A circle and a regular polygon (pentagon).
        canvas.circle(Vec2D::new(960.0, 480.0), 70.0, PURPLE);
        canvas.regular_polygon(Vec2D::new(1140.0, 480.0), 5, 70.0, -90.0, ORANGE);

        canvas.draw_texture_ex(&self.cow, Vec2D::new(520.0, 230.0), 180.0, 6.0, WHITE);

        // The same texture via DrawTexturePro: scaled 4x and spun about its
        // center, which is placed at (1180, 600). A red tint is applied.
        let size = self.cow.width() as f32 * 4.0;
        canvas.draw_texture_pro(
            &self.cow,
            Rect::new(0.0, 0.0, self.cow.width() as f32, self.cow.height() as f32),
            Rect::new(1180.0, 600.0, size, size),
            Vec2D::new(size / 2.0, size / 2.0),
            self.spin,
            RED,
        );

        // A thick line from the canvas center to the mouse cursor. We're inside
        // camera mode, so convert these screen-space points to world space —
        // the camera maps them back to the exact same screen positions, keeping
        // the line pinned to the center and the cursor regardless of zoom/pan.
        let center = camera.screen_to_world(Vec2D::new(640.0, 360.0));
        let cursor = camera.screen_to_world(self.mouse);
        canvas.line(center, cursor, 5.0, DARKBLUE);

        // The moving rectangle.
        canvas.rectangle(self.x, 520.0, 100.0, 100.0, RED);
        canvas.rectangle(self.player.x, self.player.y, 100.0, 100.0, BLACK);
        canvas.draw_texture_ex(&self.cow, self.player, 0.0, 6.0, WHITE);

        canvas.end_mode_2d();
    }
}

fn main() {
    run::<Demo>(Config {
        width: 1280 / 2,
        height: 720 / 2,
        render_width: 1280,
        render_height: 720,
        title: "juni — shapes".to_string(),
        target_ups: 60,
        centered: true,
        resizable: false,
        // 4x MSAA looks crisp on native but is expensive on the web (WebGL2
        // resolves are bandwidth-heavy), so disable it there.
        msaa: if cfg!(target_arch = "wasm32") { 1 } else { 4 },
        ..Config::default()
    });
}
