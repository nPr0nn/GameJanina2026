// Standalone slots minigame prototype.
// Run with: cargo run -p slots
// Later you can move `Slots` (and its helpers) into the main game crate.

use juni::prelude::*;

const RENDER_W: f32 = 1280.0;
const RENDER_H: f32 = 720.0;

const SYMBOL_SIZE: f32 = 100.0;
const REEL_WIDTH: f32 = 120.0;
const REEL_HEIGHT: f32 = SYMBOL_SIZE * 3.0;
const GAP: f32 = 16.0;

/// One of the symbols that can appear on a reel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Symbol {
    Seven,
    Bar,
    Diamond,
    Cherry,
}

impl Symbol {
    fn color(self) -> Color {
        match self {
            Symbol::Seven => RED,
            Symbol::Bar => GRAY,
            Symbol::Diamond => SKYBLUE,
            Symbol::Cherry => MAGENTA,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Symbol::Seven => "7",
            Symbol::Bar => "BAR",
            Symbol::Diamond => "DIAM",
            Symbol::Cherry => "CHER",
        }
    }
}

/// Tiny LCG so we don't need the `rand` crate (keeps wasm builds simple).
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        self.0
    }

    fn range_f(&mut self, min: f32, max: f32) -> f32 {
        min + (self.next() as f32 / u32::MAX as f32) * (max - min)
    }
}

/// A single vertical reel of symbols.
struct Reel {
    symbols: &'static [Symbol],
    /// Vertical position in symbol-height units. A whole number means the
    /// symbol at that index is aligned with the top of the reel window; adding
    /// 0.5 centers it on the middle payline.
    offset: f32,
    /// Symbols per second while spinning.
    speed: f32,
    /// Counts down to the moment this reel snaps to a symbol.
    stop_timer: f32,
    stopped: bool,
}

impl Reel {
    fn new() -> Self {
        Self {
            symbols: &[
                Symbol::Seven,
                Symbol::Cherry,
                Symbol::Bar,
                Symbol::Diamond,
                Symbol::Cherry,
                Symbol::Bar,
                Symbol::Seven,
                Symbol::Diamond,
            ],
            offset: 0.5,
            speed: 0.0,
            stop_timer: 0.0,
            stopped: true,
        }
    }

    fn symbol_at(&self, index: isize) -> Symbol {
        let len = self.symbols.len() as isize;
        let idx = ((index % len) + len) % len;
        self.symbols[idx as usize]
    }

    /// The symbol currently centered on the middle payline.
    fn middle_symbol(&self) -> Symbol {
        self.symbol_at(self.offset.floor() as isize)
    }

    fn spin(&mut self, speed: f32, stop_delay: f32) {
        self.speed = speed;
        self.stop_timer = stop_delay;
        self.stopped = false;
    }

    fn update(&mut self, dt: f32) {
        if self.stopped {
            return;
        }

        self.offset += self.speed * dt;

        if self.stop_timer > 0.0 {
            self.stop_timer -= dt;
            if self.stop_timer <= 0.0 {
                // Snap to the nearest symbol centered on the payline.
                self.offset = self.offset.round() + 0.5;
                self.speed = 0.0;
                self.stopped = true;
            }
        }
    }
}

#[derive(PartialEq, Eq)]
enum State {
    Idle,
    Spinning,
    ShowingResult,
}

struct Slots {
    reels: [Reel; 3],
    state: State,
    rng: Rng,
    result_timer: f32,
    message: String,
    message_x: f32,
    title_x: f32,
    controls_x: f32,
}

impl Slots {
    fn new(ctx: &Context) -> Self {
        let title = "LUCKY SLOTS";
        let title_w = ctx.measure_text(title, 64.0).x;
        let controls = "SPACE = spin   ESC = quit";
        let controls_w = ctx.measure_text(controls, 24.0).x;

        Self {
            reels: [Reel::new(), Reel::new(), Reel::new()],
            state: State::Idle,
            rng: Rng(1),
            result_timer: 0.0,
            message: "Press SPACE to spin".to_string(),
            message_x: Self::centered_x(ctx, "Press SPACE to spin", 36.0),
            title_x: (RENDER_W - title_w) * 0.5,
            controls_x: (RENDER_W - controls_w) * 0.5,
        }
    }

    fn centered_x(ctx: &Context, text: &str, size: f32) -> f32 {
        (RENDER_W - ctx.measure_text(text, size).x) * 0.5
    }

    fn set_message(&mut self, ctx: &Context, text: &str) {
        self.message_x = Self::centered_x(ctx, text, 36.0);
        self.message = text.to_string();
    }

    fn start_spin(&mut self, ctx: &Context) {
        // Reseed from time so repeated plays don't loop the same sequence.
        self.rng = Rng((ctx.time * 1_000_000.0) as u32);

        for (i, reel) in self.reels.iter_mut().enumerate() {
            let speed = self.rng.range_f(10.0, 18.0);
            let delay = 0.8 + i as f32 * 0.45 + self.rng.range_f(0.0, 0.25);
            reel.spin(speed, delay);
        }

        self.state = State::Spinning;
        self.set_message(ctx, "Good luck...");
    }

    fn check_result(&mut self, ctx: &Context) {
        let a = self.reels[0].middle_symbol();
        let b = self.reels[1].middle_symbol();
        let c = self.reels[2].middle_symbol();

        if a == b && b == c {
            self.set_message(ctx, &format!("JACKPOT! Three {}s!", a.label()));
        } else if a == b || b == c || a == c {
            self.set_message(ctx, "Small win! Two match.");
        } else {
            self.set_message(ctx, "No match. Try again!");
        }
    }
}

impl Game for Slots {
    fn init(ctx: &mut Context) -> Self {
        Self::new(ctx)
    }

    fn update(&mut self, ctx: &mut Context) {
        for reel in &mut self.reels {
            reel.update(ctx.dt);
        }

        match self.state {
            State::Idle => {
                if ctx.is_key_pressed(Key::Space) {
                    self.start_spin(ctx);
                }
            }
            State::Spinning => {
                if self.reels.iter().all(|r| r.stopped) {
                    self.check_result(ctx);
                    self.state = State::ShowingResult;
                    self.result_timer = 1.2;
                }
            }
            State::ShowingResult => {
                self.result_timer -= ctx.dt;
                if ctx.is_key_pressed(Key::Space) {
                    self.start_spin(ctx);
                } else if self.result_timer <= 0.0 {
                    self.state = State::Idle;
                    self.set_message(ctx, "Press SPACE to spin");
                }
            }
        }

        if ctx.is_key_pressed(Key::Escape) {
            ctx.exit();
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        canvas.clear_background(DARKGREEN);

        // Title
        canvas.text("LUCKY SLOTS", self.title_x, 60.0, 64.0, GOLD);

        // Layout.
        let total_w = REEL_WIDTH * 3.0 + GAP * 4.0;
        let start_x = (RENDER_W - total_w) * 0.5;
        let start_y = 180.0;
        let machine_x = start_x - GAP;
        let machine_y = start_y - GAP;
        let machine_w = total_w;
        let machine_h = REEL_HEIGHT + GAP * 2.0;

        // Reel backgrounds.
        for i in 0..3 {
            let x = start_x + GAP + i as f32 * (REEL_WIDTH + GAP);
            canvas.rectangle(x, start_y, REEL_WIDTH, REEL_HEIGHT, WHITE);
        }

        // Symbols.
        for i in 0..3 {
            let reel = &self.reels[i];
            let x = start_x + GAP + i as f32 * (REEL_WIDTH + GAP);
            let base_idx = reel.offset.floor() as isize - 1;
            let frac = reel.offset.fract();

            for row in 0..3 {
                let idx = base_idx + row;
                let symbol = reel.symbol_at(idx);
                let y = start_y + row as f32 * SYMBOL_SIZE - frac * SYMBOL_SIZE;
                let cy = y + SYMBOL_SIZE * 0.5;
                let cx = x + REEL_WIDTH * 0.5;

                canvas.circle(Vec2D::new(cx, cy), SYMBOL_SIZE * 0.38, symbol.color());

                let label = symbol.label();
                let label_w = label.len() as f32 * 24.0 * 0.58;
                canvas.text(label, cx - label_w * 0.5, cy - 11.0, 24.0, BLACK);
            }
        }

        // Payline highlight (middle row).
        canvas.rectangle(
            start_x,
            start_y + SYMBOL_SIZE,
            total_w - GAP * 2.0,
            SYMBOL_SIZE,
            YELLOW.with_alpha(0.25),
        );

        // Machine frame drawn last to hide any symbol overflow.
        // Top bar
        canvas.rectangle(machine_x, machine_y, machine_w, GAP, DARKGRAY);
        // Bottom bar
        canvas.rectangle(
            machine_x,
            machine_y + machine_h - GAP,
            machine_w,
            GAP,
            DARKGRAY,
        );
        // Left bar
        canvas.rectangle(machine_x, machine_y, GAP, machine_h, DARKGRAY);
        // Right bar
        canvas.rectangle(
            machine_x + machine_w - GAP,
            machine_y,
            GAP,
            machine_h,
            DARKGRAY,
        );

        // Message.
        canvas.text(&self.message, self.message_x, 580.0, 36.0, WHITE);

        // Controls.
        canvas.text(
            "SPACE = spin   ESC = quit",
            self.controls_x,
            650.0,
            24.0,
            LIGHTGRAY,
        );
    }
}

fn main() {
    run::<Slots>(Config {
        width: 1280,
        height: 720,
        render_width: RENDER_W as u32,
        render_height: RENDER_H as u32,
        title: "Slots Minigame".to_string(),
        target_ups: 60,
        centered: true,
        resizable: false,
        fullscreen: false,
        msaa: if cfg!(target_arch = "wasm32") { 1 } else { 4 },
    });
}
