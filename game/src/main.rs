// Run natively: cargo run
// Run on the web: trunk serve   (WebGPU, with WebGL2 fallback)
//
// This binary is a small screen-managed game shell: a Menu, the Gameplay demo,
// a Pause overlay, and Defeat / Win end screens, with fade-to-black transitions
// between them. Everything is driven by the keyboard for now (see the on-screen
// hints and the key handling in `App::update`).

mod animation;
mod collision;
mod player;
mod portals;
mod chain;
mod squeezable;
mod gameplay;
mod transition;
mod movable;
mod menu;
mod loc;

use gameplay::Gameplay;
use juni::prelude::*;
use loc::{Lang, Loc};
use menu::{Menu, MenuAction};
use transition::{Screen, Transition};

// Virtual canvas size (matches `Config::render_*`), used to center UI text.
const RENDER_W: f32 = 1280.0;
const RENDER_H: f32 = 720.0;

/// A pre-measured, horizontally centered line of UI text. Width is measured once
/// (at construction, where a `Context` is available) so `draw` needs no measuring.
struct Centered {
    text: String,
    size: f32,
    x: f32,
    y: f32,
}

impl Centered {
    fn new(ctx: &Context, text: &str, size: f32, y: f32) -> Self {
        let width = ctx.measure_text(text, size).x;
        Self {
            text: text.to_string(),
            size,
            x: (RENDER_W - width) * 0.5,
            y,
        }
    }

    fn draw(&self, canvas: &mut Canvas, color: Color) {
        canvas.text(&self.text, self.x, self.y, self.size, color);
    }
}

/// Pre-built UI labels for the non-gameplay screens (all static text).
struct Ui {
    pause_title: Centered,
    pause_prompt: Centered,
    defeat_title: Centered,
    defeat_prompt: Centered,
    win_title: Centered,
    win_prompt: Centered,
    sub_back: Centered,
    config_title: Centered,
    instructions_title: Centered,
    credits_title: Centered,
    config_options: Vec<Centered>,
    instructions_lines: Vec<Centered>,
    credits_lines: Vec<Centered>,
}

impl Ui {
    fn new(ctx: &Context, loc: Loc) -> Self {
        let instructions_texts = [
            loc.inst_move(),
            loc.inst_portals(),
            loc.inst_zoom(),
            loc.inst_pause(),
            "",
            loc.inst_objective(),
            loc.inst_box(),
        ];
        let credits_texts = [
            "",
            "Lucas Miranda",
            "Lucas Nogueira",
            "Nícolas Hecker",
            "Ana Clara Zoppi",
        ];
        let config_texts = [loc.fullscreen_key(), loc.zoom_key(), loc.config_placeholder()];

        Self {
            pause_title: Centered::new(ctx, loc.paused(), 90.0, 250.0),
            pause_prompt: Centered::new(ctx, loc.pause_prompt(), 32.0, 380.0),
            defeat_title: Centered::new(ctx, loc.defeat(), 100.0, 230.0),
            defeat_prompt: Centered::new(ctx, loc.retry_prompt(), 32.0, 380.0),
            win_title: Centered::new(ctx, loc.win(), 100.0, 230.0),
            win_prompt: Centered::new(ctx, loc.play_again_prompt(), 32.0, 380.0),
            sub_back: Centered::new(ctx, loc.back_hint(), 24.0, RENDER_H - 70.0),
            config_title: Centered::new(ctx, loc.config_title(), 72.0, 100.0),
            instructions_title: Centered::new(ctx, loc.instructions_title(), 72.0, 100.0),
            credits_title: Centered::new(ctx, loc.credits_title(), 72.0, 100.0),
            config_options: config_texts
                .iter()
                .enumerate()
                .map(|(i, text)| Centered::new(ctx, text, if i == 2 { 26.0 } else { 32.0 }, 200.0 + i as f32 * 60.0))
                .collect(),
            instructions_lines: instructions_texts
                .iter()
                .enumerate()
                .map(|(i, text)| Centered::new(ctx, text, if text.is_empty() { 1.0 } else { 30.0 }, 210.0 + i as f32 * 44.0))
                .collect(),
            credits_lines: credits_texts
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    let size = if text.is_empty() { 1.0 } else { 30.0 };
                    let y = 240.0 + i as f32 * 52.0;
                    Centered::new(ctx, text, size, y)
                })
                .collect(),
        }
    }

    fn draw_sub_screen(&self, canvas: &mut Canvas, title: &Centered) {
        draw_gradient_background(canvas);
        draw_gold_bars(canvas);

        title.draw(canvas, WHITE);
        self.sub_back.draw(canvas, GRAY);
    }
}

struct App {
    screen: Screen,
    transition: Transition,
    gameplay: Gameplay,
    menu: Menu,
    ui: Ui,
    loc: Loc,
}

impl App {
    /// Request a fade transition to `target` (no-op if one is already running).
    fn go(&mut self, target: Screen) {
        self.transition.start(target);
    }

    /// Re-create UI and menu after a language change.
    fn rebuild_ui(&mut self, ctx: &Context) {
        self.ui = Ui::new(ctx, self.loc);
        self.menu = Menu::new(ctx, self.loc);
    }
}

impl Game for App {
    fn init(ctx: &mut Context) -> Self {
        let loc = Loc::new(Lang::English);
        Self {
            screen: Screen::Menu,
            transition: Transition::new(),
            gameplay: Gameplay::new(ctx, loc),
            menu: Menu::new(ctx, loc),
            ui: Ui::new(ctx, loc),
            loc,
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
                if ctx.is_key_pressed(Key::Escape) && !self.menu.is_in_submenu() {
                    ctx.exit();
                }
                if let Some(action) = self.menu.update(ctx) {
                    match action {
                        MenuAction::Play => self.go(Screen::Gameplay),
                        MenuAction::Config => self.go(Screen::Config),
                        MenuAction::Instructions => self.go(Screen::Instructions),
                        MenuAction::Credits => self.go(Screen::Credits),
                        MenuAction::Quit => ctx.exit(),
                        MenuAction::SelectLanguage(lang) => {
                            self.loc.set(lang);
                            self.rebuild_ui(ctx);
                        }
                    }
                }
            }
            Screen::Config => {
                if ctx.is_key_pressed(Key::Escape) || ctx.is_key_pressed(Key::Backspace) {
                    self.go(Screen::Menu);
                }
            }
            Screen::Instructions => {
                if ctx.is_key_pressed(Key::Escape) || ctx.is_key_pressed(Key::Backspace) {
                    self.go(Screen::Menu);
                }
            }
            Screen::Credits => {
                if ctx.is_key_pressed(Key::Escape) || ctx.is_key_pressed(Key::Backspace) {
                    self.go(Screen::Menu);
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
                self.menu.draw(canvas, self.loc);
            }
            Screen::Config => {
                self.ui.draw_sub_screen(canvas, &self.ui.config_title);
                for option in &self.ui.config_options {
                    option.draw(canvas, LIGHTGRAY);
                }
            }
            Screen::Instructions => {
                self.ui.draw_sub_screen(canvas, &self.ui.instructions_title);
                for (i, line) in self.ui.instructions_lines.iter().enumerate() {
                    line.draw(canvas, if i == 4 { Color::new(0, 0, 0, 0) } else { LIGHTGRAY });
                }
            }
            Screen::Credits => {
                self.ui.draw_sub_screen(canvas, &self.ui.credits_title);
                for line in &self.ui.credits_lines {
                    let color = if line.text.is_empty() {
                        Color::new(0, 0, 0, 0)
                    } else {
                        SKYBLUE
                    };
                    line.draw(canvas, color);
                }
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

fn draw_gradient_background(canvas: &mut Canvas) {
    const STRIPS: i32 = 24;
    const BG_TOP: Color = Color::new(16, 22, 42, 255);
    const BG_BOTTOM: Color = Color::new(10, 14, 28, 255);
    let strip_h = RENDER_H / STRIPS as f32;
    for i in 0..STRIPS {
        let t = i as f32 / (STRIPS - 1) as f32;
        let r = (BG_TOP.r as f32 + (BG_BOTTOM.r as f32 - BG_TOP.r as f32) * t) as u8;
        let g = (BG_TOP.g as f32 + (BG_BOTTOM.g as f32 - BG_TOP.g as f32) * t) as u8;
        let b = (BG_TOP.b as f32 + (BG_BOTTOM.b as f32 - BG_TOP.b as f32) * t) as u8;
        canvas.rectangle(0.0, i as f32 * strip_h, RENDER_W, strip_h + 1.0, Color::new(r, g, b, 255));
    }
}

fn draw_gold_bars(canvas: &mut Canvas) {
    canvas.rectangle(0.0, 0.0, RENDER_W, 6.0, GOLD);
    canvas.rectangle(0.0, RENDER_H - 6.0, RENDER_W, 6.0, GOLD);
}

fn main() {
    run::<App>(Config {
        width: 1280,
        height: 720,
        render_width: RENDER_W as u32,
        render_height: RENDER_H as u32,
        title: "Duck in Boots".to_string(),
        target_ups: 60,
        centered: true,
        resizable: false,
        // 4x MSAA looks crisp on native but is expensive on the web (WebGL2
        // resolves are bandwidth-heavy), so disable it there.
        msaa: if cfg!(target_arch = "wasm32") { 1 } else { 4 },
        font_bytes: Some(include_bytes!("../assets/fonts/BlockyPixel.ttf")),
        ..Config::default()
    });
}
