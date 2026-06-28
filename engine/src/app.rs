//! The winit `ApplicationHandler` that owns the loop and drives the user's
//! [`Game`](crate::Game) through its fixed-timestep lifecycle.

use crate::audio::Audio;
use crate::canvas::Canvas;
use crate::graphics::{create_graphics, Graphics, Rc};
use crate::input::{Input, Key, MouseButton};
use crate::renderer::Renderer;
use crate::time::TimeStep;
use crate::{Config, Context, Game};
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    keyboard::PhysicalKey,
    window::{Fullscreen, Window, WindowId},
};

enum State {
    /// Waiting for async graphics init. Holds the proxy used to send `Graphics`
    /// back into the loop once ready.
    Init(Option<EventLoopProxy<Graphics>>),
    Ready(Graphics),
}

pub struct App<G: Game> {
    state: State,
    config: Config,
    renderer: Option<Renderer>,
    game: Option<G>,
    time: TimeStep,
    input: Input,
    audio: Audio,
}

impl<G: Game> App<G> {
    pub fn new(event_loop: &EventLoop<Graphics>, config: Config) -> Self {
        let time = TimeStep::new(config.target_ups);
        Self {
            state: State::Init(Some(event_loop.create_proxy())),
            config,
            renderer: None,
            game: None,
            time,
            input: Input::default(),
            audio: Audio::new(),
        }
    }
}

impl<G: Game> ApplicationHandler<Graphics> for App<G> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if let State::Init(proxy) = &mut self.state {
            if let Some(proxy) = proxy.take() {
                #[allow(unused_mut)]
                let mut win_attr = Window::default_attributes();

                // On native we set the initial window size and title.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    win_attr = win_attr
                        .with_title(self.config.title.clone())
                        .with_inner_size(winit::dpi::LogicalSize::new(
                            self.config.width,
                            self.config.height,
                        ))
                        .with_resizable(self.config.resizable);

                    if self.config.fullscreen {
                        win_attr = win_attr.with_fullscreen(Some(Fullscreen::Borderless(None)));
                    }

                    // Center on the primary monitor by positioning the window's
                    // top-left at half the leftover space (accounting for the
                    // monitor's own offset on multi-monitor setups).
                    if self.config.centered {
                        if let Some(monitor) = event_loop.primary_monitor() {
                            let screen = monitor.size();
                            let origin = monitor.position();
                            let scale = monitor.scale_factor();
                            let phys_w = (self.config.width as f64 * scale) as u32;
                            let phys_h = (self.config.height as f64 * scale) as u32;
                            let x = origin.x + (screen.width.saturating_sub(phys_w) / 2) as i32;
                            let y = origin.y + (screen.height.saturating_sub(phys_h) / 2) as i32;
                            win_attr =
                                win_attr.with_position(winit::dpi::PhysicalPosition::new(x, y));
                        }
                    }
                }

                #[cfg(target_arch = "wasm32")]
                {
                    use winit::platform::web::WindowAttributesExtWebSys;
                    // Pin the canvas *backing buffer* (the resolution wgpu
                    // renders into) to the configured size. Without this, winit
                    // tracks the canvas's CSS box × devicePixelRatio, so on a
                    // large/HiDPI display the surface balloons to millions of
                    // pixels: the per-frame letterbox blit then shades the whole
                    // thing, and every viewport change reconfigures the
                    // swapchain. A fixed backing buffer keeps the surface small
                    // and stops the resize churn; `index.html` stretches this
                    // canvas to the viewport with CSS (`object-fit: contain`), so
                    // the browser does the final upscale for free. The CSS uses
                    // `!important` to beat the inline size winit sets here, which
                    // is what previously caused scrollbars.
                    //
                    // `with_prevent_default(false)` stops winit from calling
                    // `event.preventDefault()` on canvas events, which otherwise
                    // swallows the right-click context menu (so you can't
                    // "Inspect").
                    win_attr = win_attr
                        .with_inner_size(winit::dpi::PhysicalSize::new(
                            self.config.width,
                            self.config.height,
                        ))
                        .with_append(true)
                        .with_prevent_default(false);
                }

                let window = Rc::new(
                    event_loop
                        .create_window(win_attr)
                        .expect("create window err."),
                );

                #[cfg(target_arch = "wasm32")]
                wasm_bindgen_futures::spawn_local(create_graphics(window, proxy));

                #[cfg(not(target_arch = "wasm32"))]
                pollster::block_on(create_graphics(window, proxy));
            }
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, graphics: Graphics) {
        // Graphics are ready: build the renderer, initialize the game, and
        // start a fresh clock so init time isn't counted as a huge first frame.
        let renderer = Renderer::new(
            &graphics,
            self.config.render_width,
            self.config.render_height,
            self.config.msaa,
            self.config.font_bytes,
        );

        let window = graphics.window_size();
        let mut ctx = Context {
            dt: self.time.fixed_dt(),
            time: 0.0,
            fps: 0,
            input: &self.input,
            fullscreen: graphics.window.fullscreen().is_some(),
            should_exit: false,
            toggle_fullscreen: false,
            window_size: (window.width, window.height),
            render_size: (self.config.render_width, self.config.render_height),
            gfx: &graphics,
            renderer: &renderer,
            audio: &mut self.audio,
        };
        let game = G::init(&mut ctx);

        self.renderer = Some(renderer);
        self.game = Some(game);
        self.time = TimeStep::new(self.config.target_ups);

        graphics.request_redraw();
        self.state = State::Ready(graphics);
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::Resized(size) => {
                if let State::Ready(gfx) = &mut self.state {
                    gfx.resize(size);
                }
            }
            WindowEvent::RedrawRequested => {
                let (State::Ready(gfx), Some(renderer), Some(game)) =
                    (&mut self.state, &mut self.renderer, &mut self.game)
                else {
                    return;
                };

                // Advance time and run fixed updates.
                self.time.frame();
                let cpu_start = web_time::Instant::now();
                let window = gfx.window_size();
                let mut ctx = Context {
                    dt: self.time.fixed_dt(),
                    time: self.time.total(),
                    fps: self.time.fps(),
                    input: &self.input,
                    fullscreen: gfx.window.fullscreen().is_some(),
                    should_exit: false,
                    toggle_fullscreen: false,
                    window_size: (window.width, window.height),
                    render_size: (self.config.render_width, self.config.render_height),
                    gfx: &*gfx,
                    renderer: &*renderer,
                    audio: &mut self.audio,
                };
                let mut steps_ran = 0u32;
                while self.time.next_fixed_step() {
                    game.update(&mut ctx);
                    steps_ran += 1;
                }
                // Take the frame's requests before releasing the borrow on
                // `self.input` (held immutably by `ctx`).
                let should_exit = ctx.should_exit;
                let toggle_fullscreen = ctx.toggle_fullscreen;

                // Edge-triggered input (`pressed`/`released`) and per-frame
                // deltas are only valid until an update observes them. The
                // renderer runs every vsync tick but updates run at the fixed
                // rate, so on displays faster than `target_ups` most render
                // frames consume zero fixed steps. Clearing unconditionally
                // here would wipe a press whose edge landed on such a frame
                // before any `update` ever saw it — a dropped click/keypress.
                // Only clear once at least one update has consumed the edges.
                if steps_ran > 0 {
                    self.input.new_frame();
                }

                if toggle_fullscreen {
                    let fullscreen = if gfx.window.fullscreen().is_some() {
                        None
                    } else {
                        Some(Fullscreen::Borderless(None))
                    };
                    gfx.window.set_fullscreen(fullscreen);
                }
                if should_exit {
                    event_loop.exit();
                    return;
                }

                // Render one frame.
                renderer.begin();
                {
                    let mut canvas = Canvas::new(&mut renderer.batch);
                    game.draw(&mut canvas);
                }
                renderer.flush(gfx, self.time.total() as f32);
                self.time.record_cpu(cpu_start.elapsed().as_secs_f32());

                // Keep the loop pumping.
                gfx.request_redraw();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        physical_key: PhysicalKey::Code(code),
                        state,
                        ..
                    },
                ..
            } => {
                if let Some(key) = Key::from_code(code) {
                    self.input.process(key, state == ElementState::Pressed);
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input
                    .set_mouse_pos(position.x as f32, position.y as f32);
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(button) = MouseButton::from_winit(button) {
                    self.input
                        .process_mouse_button(button, state == ElementState::Pressed);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                use winit::event::MouseScrollDelta;
                let amount = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 100.0,
                };
                self.input.add_wheel(amount);
            }
            WindowEvent::CloseRequested => event_loop.exit(),
            _ => {}
        }
    }
}
