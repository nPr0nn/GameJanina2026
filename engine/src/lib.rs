//! `juni` — a small, cross-platform 2D game engine on top of wgpu, with a
//! Raylib-inspired API.
//!
//! The engine owns the main loop. You implement [`Game`] and call [`run`]:
//!
//! ```no_run
//! use juni::prelude::*;
//!
//! struct MyGame { x: f32 }
//!
//! impl Game for MyGame {
//!     fn init(_ctx: &mut Context) -> Self { MyGame { x: 0.0 } }
//!     fn update(&mut self, ctx: &mut Context) { self.x += 60.0 * ctx.dt; }
//!     fn draw(&mut self, canvas: &mut Canvas) {
//!         canvas.clear_background(WHITE);
//!         canvas.rectangle(self.x, 100.0, 80.0, 80.0, RED);
//!     }
//! }
//!
//! run::<MyGame>(Config::default());
//! ```
//!
//! Everything is rendered into a fixed virtual resolution and letterboxed to the
//! window, so your coordinates are resolution-independent.

mod app;
mod audio;
mod camera;
mod canvas;
mod color;
mod graphics;
mod input;
pub mod level;
mod math;
mod renderer;
mod text;
mod time;

pub use audio::Sound;
pub use camera::Camera2D;
pub use canvas::Canvas;
pub use color::*;
pub use input::{Key, MouseButton};
pub use level::{Level, Shape};
pub use math::{Rect, Vec2D};
pub use renderer::{Shader, Texture};

/// Common imports for using `juni`. `use juni::prelude::*;` brings the engine
/// entry points, core types, the [`Color`] palette, and input enums into scope.
pub mod prelude {
    pub use crate::color::*;
    pub use crate::{
        run, Camera2D, Canvas, Config, Context, Game, Key, Level, MouseButton, Rect, Shader, Shape,
        Sound, Texture, Vec2D,
    };
}

use app::App;
use audio::Audio;
use graphics::Graphics;
use input::Input;
use renderer::Renderer;
use winit::event_loop::{ControlFlow, EventLoop};

/// Engine configuration passed to [`run`].
#[derive(Debug, Clone)]
pub struct Config {
    /// Initial window size in physical pixels.
    pub width: u32,
    pub height: u32,
    /// Fixed virtual canvas resolution. All drawing happens here, then it is
    /// letterboxed to the window.
    pub render_width: u32,
    pub render_height: u32,
    pub title: String,
    /// Fixed updates per second (the rate at which [`Game::update`] runs).
    pub target_ups: u32,
    /// Whether the window can be resized by the user (native only). Set to
    /// `false` for a fixed-size window.
    pub resizable: bool,
    /// Center the window on the primary monitor at startup (native only).
    pub centered: bool,
    /// Start in borderless fullscreen (native only). The `F` key toggles
    /// fullscreen at runtime regardless of this initial value.
    pub fullscreen: bool,
    /// Multisample anti-aliasing sample count for the render texture. `1`
    /// disables MSAA; `4` is a common choice. Unsupported values fall back to
    /// the highest supported count at or below the request (down to `1`).
    pub msaa: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 800,
            height: 600,
            render_width: 1280,
            render_height: 720,
            title: "juni".to_string(),
            target_ups: 60,
            resizable: true,
            centered: false,
            fullscreen: false,
            msaa: 1,
        }
    }
}

/// Per-update state passed to [`Game::update`].
pub struct Context<'a> {
    /// Fixed delta time in seconds for this update step (`1.0 / target_ups`).
    pub dt: f32,
    /// Total elapsed time in seconds since the game started.
    pub time: f64,
    /// Render frames per second, sampled once per second.
    pub fps: u32,
    pub(crate) input: &'a Input,
    pub(crate) fullscreen: bool,
    pub(crate) should_exit: bool,
    pub(crate) toggle_fullscreen: bool,
    /// Physical window size, for mapping the cursor into virtual coordinates.
    pub(crate) window_size: (u32, u32),
    /// Virtual render resolution, for mapping the cursor into virtual coords.
    pub(crate) render_size: (u32, u32),
    /// GPU access for compiling custom shaders.
    pub(crate) gfx: &'a Graphics,
    pub(crate) renderer: &'a Renderer,
    /// Audio output, for loading and playing sounds.
    pub(crate) audio: &'a mut Audio,
}

impl Context<'_> {
    /// Request the engine to close the window and end the loop.
    pub fn exit(&mut self) {
        self.should_exit = true;
    }

    /// Set the native window title. On the web this is a no-op because the
    /// canvas is not a native desktop window.
    pub fn set_window_title(&mut self, title: &str) {
        #[cfg(not(target_arch = "wasm32"))]
        self.gfx.window.set_title(title);

        #[cfg(target_arch = "wasm32")]
        let _ = title;
    }

    /// Compile a custom [`Shader`] from WGSL source (raylib's
    /// `LoadShaderFromMemory`). The module must provide `vs_main`/`fs_main`
    /// against the engine's standard interface — vertex inputs
    /// `@location(0) position: vec2<f32>` and `@location(1) color: vec4<f32>`,
    /// and `Globals { proj: mat4x4<f32>, time: f32 }` at `@group(0) @binding(0)`
    /// (see `shaders/shape.wgsl`). Build it once (typically in [`Game::init`])
    /// and apply it with [`Canvas::begin_shader_mode`].
    ///
    /// Pair with the `include_str!` macro to keep WGSL in its own file:
    /// `ctx.load_shader_from_memory(include_str!("my.wgsl"))`.
    pub fn load_shader_from_memory(&self, source: &str) -> Shader {
        self.renderer.build_shader(self.gfx, source)
    }

    /// Decode PNG `bytes` and upload them as a [`Texture`] (raylib's
    /// `LoadTextureFromImage` over in-memory data). Pair with `include_bytes!`
    /// to embed an asset: `ctx.load_texture_from_memory(include_bytes!("a.png"))`.
    /// A decode failure logs and yields a 1×1 magenta placeholder.
    pub fn load_texture_from_memory(&self, bytes: &[u8]) -> Texture {
        self.renderer.build_texture(self.gfx, bytes)
    }

    /// Load a [`Texture`] from a PNG file on disk (raylib's `LoadTexture`).
    /// Native only — the web has no synchronous filesystem, so embed assets with
    /// [`load_texture_from_memory`](Self::load_texture_from_memory) there.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_texture(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<Texture> {
        let bytes = std::fs::read(path)?;
        Ok(self.renderer.build_texture(self.gfx, &bytes))
    }

    /// Measure the rendered size of `text` at `font_size` (in virtual pixels),
    /// using the engine's default font. Raylib's `MeasureTextEx`: the `x` of the
    /// result is the width of the widest line, `y` the total height. For just the
    /// width (raylib's `MeasureText`) take `.x`.
    pub fn measure_text(&self, text: &str, font_size: f32) -> Vec2D {
        self.renderer.measure_text(text, font_size)
    }

    /// Decode WAV `bytes` into a [`Sound`] (raylib's `LoadSound`, over in-memory
    /// data). Pair with `include_bytes!` to embed a clip:
    /// `ctx.load_sound_from_memory(include_bytes!("a.wav"))`.
    pub fn load_sound_from_memory(&self, bytes: &[u8]) -> Sound {
        self.audio.load(bytes)
    }

    /// Play a one-shot instance of `sound` (raylib's `PlaySound`). Overlapping
    /// plays mix.
    pub fn play_sound(&mut self, sound: &Sound) {
        self.audio.play(sound);
    }

    /// `true` on the frame `key` was pressed (edge-triggered, ignores OS repeat).
    pub fn is_key_pressed(&self, key: Key) -> bool {
        self.input.is_key_pressed(key)
    }

    /// `true` while `key` is held down.
    pub fn is_key_down(&self, key: Key) -> bool {
        self.input.is_key_down(key)
    }

    /// `true` on the frame `key` was released.
    pub fn is_key_released(&self, key: Key) -> bool {
        self.input.is_key_released(key)
    }

    /// `true` while `button` is held down.
    pub fn is_mouse_button_down(&self, button: MouseButton) -> bool {
        self.input.is_mouse_button_down(button)
    }

    /// `true` on the frame `button` was pressed.
    pub fn is_mouse_button_pressed(&self, button: MouseButton) -> bool {
        self.input.is_mouse_button_pressed(button)
    }

    /// `true` on the frame `button` was released.
    pub fn is_mouse_button_released(&self, button: MouseButton) -> bool {
        self.input.is_mouse_button_released(button)
    }

    /// Cursor position in virtual-canvas coordinates (the same space you draw
    /// in). The physical cursor is mapped through the letterbox transform;
    /// positions over the letterbox bars fall outside `[0, render_size]`.
    pub fn mouse_position(&self) -> Vec2D {
        let (ww, wh) = self.window_size;
        let (rw, rh) = self.render_size;
        let (vx, vy, vw, vh) = renderer::compute_letterbox(ww, wh, rw, rh);
        if vw <= 0.0 || vh <= 0.0 {
            return Vec2D::ZERO;
        }
        let p = self.input.mouse_pos();
        Vec2D::new((p.x - vx) / vw * rw as f32, (p.y - vy) / vh * rh as f32)
    }

    /// Cursor X in virtual-canvas coordinates. See [`mouse_position`](Self::mouse_position).
    pub fn mouse_x(&self) -> f32 {
        self.mouse_position().x
    }

    /// Cursor Y in virtual-canvas coordinates. See [`mouse_position`](Self::mouse_position).
    pub fn mouse_y(&self) -> f32 {
        self.mouse_position().y
    }

    /// Cursor movement this frame, scaled into virtual-canvas units.
    pub fn mouse_delta(&self) -> Vec2D {
        let (ww, wh) = self.window_size;
        let (rw, rh) = self.render_size;
        let (_, _, vw, vh) = renderer::compute_letterbox(ww, wh, rw, rh);
        if vw <= 0.0 || vh <= 0.0 {
            return Vec2D::ZERO;
        }
        let d = self.input.mouse_delta();
        Vec2D::new(d.x / vw * rw as f32, d.y / vh * rh as f32)
    }

    /// Mouse wheel movement this frame (positive is up/away from the user).
    pub fn mouse_wheel_move(&self) -> f32 {
        self.input.wheel()
    }

    /// `true` if the window is currently in (borderless) fullscreen.
    pub fn is_fullscreen(&self) -> bool {
        self.fullscreen
    }

    /// Toggle borderless fullscreen. Applied after this update step. Native
    /// only; on the web fullscreen is governed by the browser.
    pub fn toggle_fullscreen(&mut self) {
        self.toggle_fullscreen = true;
    }
}

/// Implement this trait for your game and pass it to [`run`].
pub trait Game: 'static {
    fn init(ctx: &mut Context) -> Self
    where
        Self: Sized;

    /// Called at the fixed update rate (`Config::target_ups`). May be called
    /// zero or more times per rendered frame.
    fn update(&mut self, ctx: &mut Context);

    /// Called once per rendered frame.
    fn draw(&mut self, canvas: &mut Canvas);
}

/// Start the engine with the given configuration and run game `G`.
pub fn run<G: Game>(config: Config) {
    let event_loop = EventLoop::<Graphics>::with_user_event().build().unwrap();
    // The loop is redraw-driven: every `RedrawRequested` re-requests the next
    // redraw to keep pumping.
    //
    // Use `Poll` on every platform. On the web, winit's event loop already runs
    // on `requestAnimationFrame`; a `request_redraw()` issued from *inside* the
    // `RedrawRequested` handler is serviced a full rAF cycle late under
    // `ControlFlow::Wait`, so a frame renders only every *other* animation frame
    // — a hard 60→30 fps cap regardless of how little we draw (winit's
    // double-rAF latency). `Poll` services the redraw on the next vblank and
    // restores full-rate rendering.
    event_loop.set_control_flow(ControlFlow::Poll);

    let app = App::<G>::new(&event_loop, config);
    run_app(event_loop, app);
}

#[cfg(target_arch = "wasm32")]
fn run_app<G: Game>(event_loop: EventLoop<Graphics>, app: App<G>) {
    std::panic::set_hook(Box::new(console_error_panic_hook::hook));
    // Keep the console at `Error`: `console.log` is synchronous and slow, so
    // letting third-party crates (e.g. wgpu's WebGL backend) log per-frame at
    // info/warn tanks the framerate. juni's own FPS readout bypasses this and
    // calls `console.log` directly once per second (see time.rs).
    let _ = console_log::init_with_level(log::Level::Error);

    use winit::platform::web::EventLoopExtWebSys;
    event_loop.spawn_app(app);
}

#[cfg(not(target_arch = "wasm32"))]
fn run_app<G: Game>(event_loop: EventLoop<Graphics>, mut app: App<G>) {
    // Default other crates to `warn` but show juni's own `info` logs (the
    // per-second FPS readout). `RUST_LOG` still overrides this.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,juni=info"))
        .init();
    let _ = event_loop.run_app(&mut app);
}
