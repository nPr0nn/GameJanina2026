// Run natively: cargo run
// Run on the web: trunk serve   (WebGPU, with WebGL2 fallback)
//
// This binary is a small screen-managed game shell: a Menu, the Gameplay demo,
// a Pause overlay, and Defeat / Win end screens, with fade-to-black transitions
// between them. Everything is driven by the keyboard for now (see the on-screen
// hints and the key handling in `App::update`).

mod player;
mod chain;
mod gameplay;
mod transition;
mod movable;

use gameplay::Gameplay; 
use juni::prelude::*;
use transition::{Screen, Transition};

// Virtual canvas size (matches `Config::render_*`), used to center UI text.
const RENDER_W: f32 = 1280.0;
const RENDER_H: f32 = 720.0;

/// A pre-measured, horizontally centered line of UI text. Width is measured once
/// (at construction, where a `Context` is available) so `draw` needs no measuring.
struct Centered {
    text: &'static str,
    size: f32,
    x: f32,
    y: f32,
}

impl Centered {
    fn new(ctx: &Context, text: &'static str, size: f32, y: f32) -> Self {
        let width = ctx.measure_text(text, size).x;
        Self { text, size, x: (RENDER_W - width) * 0.5, y }
    }

    fn draw(&self, canvas: &mut Canvas, color: Color) {
        canvas.text(self.text, self.x, self.y, self.size, color);
    }
}

/// Pre-built UI labels for the non-gameplay screens (all static text).
struct Ui {
    menu_title: Centered,
    menu_prompt: Centered,
    pause_title: Centered,
    pause_prompt: Centered,
    defeat_title: Centered,
    defeat_prompt: Centered,
    win_title: Centered,
    win_prompt: Centered,
}

impl Ui {
    fn new(ctx: &Context) -> Self {
        Self {
            menu_title: Centered::new(ctx, "JUNI", 120.0, 200.0),
            menu_prompt: Centered::new(ctx, "Press ENTER to play    ·    ESC to quit", 32.0, 400.0),
            pause_title: Centered::new(ctx, "PAUSED", 90.0, 250.0),
            pause_prompt: Centered::new(ctx, "ESC / P resume    ·    M menu", 32.0, 380.0),
            defeat_title: Centered::new(ctx, "DEFEAT", 100.0, 230.0),
            defeat_prompt: Centered::new(ctx, "ENTER retry    ·    M menu", 32.0, 380.0),
            win_title: Centered::new(ctx, "YOU WIN!", 100.0, 230.0),
            win_prompt: Centered::new(ctx, "ENTER play again    ·    M menu", 32.0, 380.0),
        }
    }
}

struct App {
    screen: Screen,
    transition: Transition,
    gameplay: Gameplay,
    ui: Ui,
}

impl App {
    /// Request a fade transition to `target` (no-op if one is already running).
    fn go(&mut self, target: Screen) {
        self.transition.start(target);
    }
}

impl Game for App {
    fn init(ctx: &mut Context) -> Self {
        Self {
            screen: Screen::Menu,
            transition: Transition::new(),
            gameplay: Gameplay::new(ctx),
            ui: Ui::new(ctx),
        }
    }

    fn update(&mut self, ctx: &mut Context) {
        // Advance any running fade. When it reports the midpoint, swap screens.
        if let Some(target) = self.transition.update(ctx.dt) {
            // Start a fresh run whenever we enter gameplay other than by
            // resuming from pause (i.e. from the menu or after a win/defeat).
            if target == Screen::Gameplay && self.screen != Screen::Pause {
                self.gameplay.reset();
            }
            self.screen = target;
        }

        // Input is frozen while a transition plays so it can't be interrupted.
        if self.transition.is_active() {
            return;
        }

        // Fullscreen toggle works on every screen.
        if ctx.is_key_pressed(Key::F) {
            ctx.toggle_fullscreen();
        }

        match self.screen {
            Screen::Menu => {
                if ctx.is_key_pressed(Key::Enter) {
                    self.go(Screen::Gameplay);
                }
                if ctx.is_key_pressed(Key::Escape) {
                    ctx.exit();
                }
            }
            Screen::Gameplay => {
                self.gameplay.update(ctx);
                if ctx.is_key_pressed(Key::P) || ctx.is_key_pressed(Key::Escape) {
                    self.go(Screen::Pause);
                } else if ctx.is_key_pressed(Key::K) {
                    self.go(Screen::Defeat);
                } else if ctx.is_key_pressed(Key::L) {
                    self.go(Screen::Win);
                }
            }
            Screen::Pause => {
                if ctx.is_key_pressed(Key::P) || ctx.is_key_pressed(Key::Escape) {
                    self.go(Screen::Gameplay);
                } else if ctx.is_key_pressed(Key::M) {
                    self.go(Screen::Menu);
                }
            }
            Screen::Defeat | Screen::Win => {
                if ctx.is_key_pressed(Key::Enter) {
                    self.go(Screen::Gameplay);
                } else if ctx.is_key_pressed(Key::M) {
                    self.go(Screen::Menu);
                }
            }
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        match self.screen {
            Screen::Menu => {
                canvas.clear_background(DARKBLUE);
                self.ui.menu_title.draw(canvas, WHITE);
                self.ui.menu_prompt.draw(canvas, SKYBLUE);
            }
            Screen::Gameplay => {
                self.gameplay.draw(canvas);
            }
            Screen::Pause => {
                // Keep the (frozen) game visible behind a translucent dim, with
                // the pause text on top.
                self.gameplay.draw(canvas);
                canvas.rectangle(0.0, 0.0, RENDER_W, RENDER_H, BLACK.with_alpha(0.55));
                self.ui.pause_title.draw(canvas, WHITE);
                self.ui.pause_prompt.draw(canvas, LIGHTGRAY);
            }
            Screen::Defeat => {
                canvas.clear_background(MAROON);
                self.ui.defeat_title.draw(canvas, WHITE);
                self.ui.defeat_prompt.draw(canvas, LIGHTGRAY);
            }
            Screen::Win => {
                canvas.clear_background(DARKGREEN);
                self.ui.win_title.draw(canvas, WHITE);
                self.ui.win_prompt.draw(canvas, LIGHTGRAY);
            }
        }

        // The fade overlay, composited over everything (text included). Zero
        // alpha when idle, so this is a no-op outside transitions.
        let alpha = self.transition.alpha();
        if alpha > 0.0 {
            canvas.fade(BLACK.with_alpha(alpha));
        }
    }
}

fn main() {
    run::<App>(Config {
        width: 1280,
        height: 720,
        render_width: RENDER_W as u32,
        render_height: RENDER_H as u32,
        title: "juni — screens".to_string(),
        target_ups: 60,
        centered: true,
        resizable: false,
        // 4x MSAA looks crisp on native but is expensive on the web (WebGL2
        // resolves are bandwidth-heavy), so disable it there.
        msaa: if cfg!(target_arch = "wasm32") { 1 } else { 4 },
        ..Config::default()
    });
}
