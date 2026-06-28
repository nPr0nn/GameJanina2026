use juni::prelude::*;
use juni::Rect;
use crate::animation::{Animation, SpriteSheet};
use crate::loc::{Lang, Loc};

const RENDER_W: f32 = 1280.0;
const RENDER_H: f32 = 720.0;

// Color palette
const BG_TOP: Color = Color::new(8, 12, 28, 255);
const BG_BOTTOM: Color = Color::new(16, 22, 46, 255);
const BG_GLOW: Color = Color::new(28, 38, 72, 255);
const PANEL_BG: Color = Color::new(13, 17, 34, 245);
const GOLD: Color = Color::new(195, 150, 45, 255);
const GOLD_DIM: Color = Color::new(80, 60, 18, 255);
const GOLD_BRIGHT: Color = Color::new(235, 190, 65, 255);
const TEXT_BRIGHT: Color = Color::new(255, 255, 255, 255);
const TEXT_NORMAL: Color = Color::new(178, 180, 200, 255);
const TEXT_DIM: Color = Color::new(75, 78, 100, 255);
const BTN_SEL_BG: Color = Color::new(25, 32, 65, 255);
const STAR: Color = Color::new(180, 188, 230, 55);
const DUCK_TINT: Color = Color::new(255, 230, 180, 200);

// Layout
const PANEL_X: f32 = 390.0;
const PANEL_Y: f32 = 200.0;
const PANEL_W: f32 = 500.0;
const PANEL_H: f32 = 270.0;
const BTN_PAD_X: f32 = 28.0;
const BTN_H: f32 = 44.0;
const BTN_GAP: f32 = 12.0;
const BTN_START_Y: f32 = PANEL_Y + 30.0;
const FONT_SIZE: f32 = 26.0;

// Duck rain
const DUCK_COUNT: usize = 50;
const DUCK_MIN_SPEED: f32 = 90.0;
const DUCK_MAX_SPEED: f32 = 240.0;
const DUCK_MIN_SCALE: f32 = 0.7;
const DUCK_MAX_SCALE: f32 = 1.8;
const DUCK_ANIM_FPS_MIN: f32 = 8.0;
const DUCK_ANIM_FPS_MAX: f32 = 14.0;

pub enum MenuAction {
    Play,
    Instructions,
    Credits,
    Quit,
    SelectLanguage(Lang),
}

enum MenuState {
    Title,
    Main,
    Language,
}

struct MenuItem {
    label: String,
    label_width: f32,
    y: f32,
}

const LANGS: [Lang; 3] = [Lang::English, Lang::Portuguese, Lang::Arabic];

/// A tiny deterministic RNG so we can scatter ducks without adding a dependency.
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed.max(1) }
    }

    fn next(&mut self) -> u64 {
        // PCG-style mix: good enough for visual jitter.
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }

    fn gen_f32(&mut self) -> f32 {
        (self.next() as f32) / (u64::MAX as f32)
    }

    fn range_f32(&mut self, min: f32, max: f32) -> f32 {
        min + self.gen_f32() * (max - min)
    }

    fn gen_bool(&mut self) -> bool {
        self.next() & 1 == 0
    }
}

struct FallingDuck {
    anim: Animation,
    x: f32,
    y: f32,
    speed: f32,
    scale: f32,
    flip: bool,
    rot: f32,
}

impl FallingDuck {
    fn spawn(sheet: &SpriteSheet, rng: &mut Rng, above_screen: bool) -> Self {
        let x = rng.range_f32(-20.0, RENDER_W + 20.0);
        let y = if above_screen {
            rng.range_f32(-RENDER_H, -40.0)
        } else {
            rng.range_f32(0.0, RENDER_H)
        };
        let speed = rng.range_f32(DUCK_MIN_SPEED, DUCK_MAX_SPEED);
        let scale = rng.range_f32(DUCK_MIN_SCALE, DUCK_MAX_SCALE);
        let flip = rng.gen_bool();
        let rot = rng.range_f32(-12.0, 12.0);
        let fps = rng.range_f32(DUCK_ANIM_FPS_MIN, DUCK_ANIM_FPS_MAX);
        // Row 1 of the ducky sheet is the walk cycle.
        let anim = Animation::new(sheet.clone(), 1, fps, true);
        Self {
            anim,
            x,
            y,
            speed,
            scale,
            flip,
            rot,
        }
    }
}

pub struct Menu {
    state: MenuState,
    title: String,
    title_width: f32,
    title_prompt: String,
    title_prompt_width: f32,
    items: Vec<MenuItem>,
    selected: usize,
    hint: String,
    hint_width: f32,
    // language screen
    lang_selected: usize,
    lang_labels: Vec<(String, f32)>,
    lang_title_width: f32,
    lang_back_hint_width: f32,
    // icons: play, instructions, credits, quit
    icons: Vec<Texture>,
    // language button bottom-right
    lang_btn_rect: Rect,
    lang_code: String,
    // animated duck rain
    duck_sheet: SpriteSheet,
    ducks: Vec<FallingDuck>,
    duck_rng: Rng,
}

impl Menu {
    pub fn new(ctx: &Context, loc: Loc) -> Self {
        let labels = [
            loc.play(),
            loc.instructions(),
            loc.credits(),
            loc.quit(),
        ];

        let items = labels
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let y = BTN_START_Y + i as f32 * (BTN_H + BTN_GAP);
                MenuItem {
                    label_width: ctx.measure_text(label, FONT_SIZE).x,
                    label: label.to_string(),
                    y,
                }
            })
            .collect();

        let hint = loc.menu_hint().to_string();
        let hint_width = ctx.measure_text(&hint, 16.0).x;

        let lang_code = lang_code_str(loc.lang()).to_string();

        let lang_names = ["English", "Portugues", "Arabic"];
        let lang_labels = lang_names
            .iter()
            .map(|name| {
                let s = name.to_string();
                let w = ctx.measure_text(s.as_str(), 28.0).x;
                (s, w)
            })
            .collect();

        let lang_title_width = ctx.measure_text("SELECT LANGUAGE", 24.0).x;
        let lang_back_hint_width = ctx.measure_text("ESC to cancel", 16.0).x;

        let lang_selected = LANGS.iter().position(|&l| l == loc.lang()).unwrap_or(0);

        let icons = vec![
            ctx.load_texture_from_memory(include_bytes!(
                "../assets/icons/PNG/Flat/16/Play.png"
            )),
            ctx.load_texture_from_memory(include_bytes!(
                "../assets/icons/PNG/Flat/16/Info.png"
            )),
            ctx.load_texture_from_memory(include_bytes!(
                "../assets/icons/PNG/Flat/16/People.png"
            )),
            ctx.load_texture_from_memory(include_bytes!(
                "../assets/icons/PNG/Flat/16/Exit.png"
            )),
        ];

        let lang_btn_rect = Rect::new(RENDER_W - 108.0, RENDER_H - 58.0, 95.0, 40.0);

        let duck_sheet = SpriteSheet::from_memory(
            ctx,
            include_bytes!("../assets/ducky_spritesheet.png"),
            32,
            32,
        );

        // Portable clock: `std::time::SystemTime` panics on wasm ("time not
        // implemented on this platform"); `time_seed` uses the engine's
        // `web_time` shim instead.
        let mut rng = Rng::new(time_seed());
        let ducks = (0..DUCK_COUNT)
            .map(|_| FallingDuck::spawn(&duck_sheet, &mut rng, false))
            .collect();

        let title_prompt = loc.title_prompt().to_string();
        let title_prompt_width = ctx.measure_text(&title_prompt, 28.0).x;

        Self {
            state: MenuState::Title,
            title: loc.game_title().to_string(),
            title_width: ctx.measure_text(loc.game_title(), 72.0).x,
            title_prompt,
            title_prompt_width,
            items,
            selected: 0,
            hint,
            hint_width,
            lang_selected,
            lang_labels,
            lang_title_width,
            lang_back_hint_width,
            icons,
            lang_btn_rect,
            lang_code,
            duck_sheet,
            ducks,
            duck_rng: rng,
        }
    }

    pub fn is_in_submenu(&self) -> bool {
        matches!(self.state, MenuState::Language)
    }

    pub fn update(&mut self, ctx: &Context) -> Option<MenuAction> {
        self.update_ducks(ctx.dt);
        match self.state {
            MenuState::Title => self.update_title(ctx),
            MenuState::Main => self.update_main(ctx),
            MenuState::Language => self.update_language(ctx),
        }
    }

    fn update_title(&mut self, ctx: &Context) -> Option<MenuAction> {
        if ctx.is_key_pressed(Key::Enter)
            || ctx.is_key_pressed(Key::Space)
            || ctx.is_key_pressed(Key::Escape)
            || ctx.is_mouse_button_pressed(MouseButton::Left)
        {
            self.state = MenuState::Main;
        }
        None
    }

    fn update_ducks(&mut self, dt: f32) {
        for duck in &mut self.ducks {
            duck.anim.update(dt);
            duck.y += duck.speed * dt;
            if duck.y > RENDER_H + 40.0 {
                *duck = FallingDuck::spawn(&self.duck_sheet, &mut self.duck_rng, true);
            }
        }
    }

    fn update_main(&mut self, ctx: &Context) -> Option<MenuAction> {
        if ctx.is_key_pressed(Key::Up) || ctx.is_key_pressed(Key::W) {
            self.selected = self.selected.saturating_sub(1);
        }
        if ctx.is_key_pressed(Key::Down) || ctx.is_key_pressed(Key::S) {
            self.selected = (self.selected + 1).min(self.items.len() - 1);
        }
        if ctx.is_key_pressed(Key::Enter) || ctx.is_key_pressed(Key::Space) {
            return Some(self.action_for(self.selected));
        }

        let mouse = ctx.mouse_position();

        if point_in_rect(mouse, self.lang_btn_rect)
            && ctx.is_mouse_button_pressed(MouseButton::Left)
        {
            self.state = MenuState::Language;
            return None;
        }

        let btn_x = PANEL_X + BTN_PAD_X;
        let btn_w = PANEL_W - BTN_PAD_X * 2.0;
        for (i, item) in self.items.iter().enumerate() {
            let btn_rect = Rect::new(btn_x, item.y, btn_w, BTN_H);
            if point_in_rect(mouse, btn_rect) {
                self.selected = i;
                if ctx.is_mouse_button_pressed(MouseButton::Left) {
                    return Some(self.action_for(i));
                }
            }
        }

        None
    }

    fn update_language(&mut self, ctx: &Context) -> Option<MenuAction> {
        if ctx.is_key_pressed(Key::Escape) || ctx.is_key_pressed(Key::Backspace) {
            self.state = MenuState::Main;
            return None;
        }
        if ctx.is_key_pressed(Key::Up) || ctx.is_key_pressed(Key::W) {
            if self.lang_selected > 0 {
                self.lang_selected -= 1;
            }
        }
        if ctx.is_key_pressed(Key::Down) || ctx.is_key_pressed(Key::S) {
            if self.lang_selected < LANGS.len() - 1 {
                self.lang_selected += 1;
            }
        }
        if ctx.is_key_pressed(Key::Enter) || ctx.is_key_pressed(Key::Space) {
            self.state = MenuState::Main;
            return Some(MenuAction::SelectLanguage(LANGS[self.lang_selected]));
        }

        let mouse = ctx.mouse_position();
        let (lp_x, lp_y, lp_w, _) = lang_panel_dims();
        for (i, _) in LANGS.iter().enumerate() {
            let item_y = lp_y + 68.0 + i as f32 * 52.0;
            let item_rect = Rect::new(lp_x + 20.0, item_y, lp_w - 40.0, 42.0);
            if point_in_rect(mouse, item_rect) {
                self.lang_selected = i;
                if ctx.is_mouse_button_pressed(MouseButton::Left) {
                    self.state = MenuState::Main;
                    return Some(MenuAction::SelectLanguage(LANGS[i]));
                }
            }
        }

        None
    }

    fn action_for(&self, index: usize) -> MenuAction {
        match index {
            0 => MenuAction::Play,
            1 => MenuAction::Instructions,
            2 => MenuAction::Credits,
            _ => MenuAction::Quit,
        }
    }

    pub fn draw(&self, canvas: &mut Canvas, _loc: Loc) {
        self.draw_background(canvas);
        self.draw_ducks(canvas);

        match self.state {
            MenuState::Title => self.draw_title_screen(canvas),
            MenuState::Main => self.draw_main_menu(canvas),
            MenuState::Language => {
                self.draw_main_menu(canvas);
                self.draw_language_overlay(canvas);
            }
        }
    }

    fn draw_title_screen(&self, canvas: &mut Canvas) {
        // Large centered title with shadow and a gold sheen.
        let title_size = 96.0;
        let title_x = (RENDER_W - self.title_width) * 0.5;
        let title_y = 220.0;
        canvas.text(&self.title, title_x + 4.0, title_y + 4.0, title_size, Color::new(0, 0, 0, 160));
        canvas.text(&self.title, title_x, title_y, title_size, GOLD_BRIGHT);

        // Animated prompt below the title.
        let prompt_x = (RENDER_W - self.title_prompt_width) * 0.5;
        let prompt_y = 360.0;
        let pulse = ((self.ducks[0].y * 0.05).sin() * 0.5 + 0.5) * 155.0 + 100.0;
        canvas.text(
            &self.title_prompt,
            prompt_x,
            prompt_y,
            28.0,
            Color::new(255, 255, 255, pulse as u8),
        );

        // Bottom footer hint.
        let hint_x = (RENDER_W - self.hint_width) * 0.5;
        canvas.text(&self.hint, hint_x, RENDER_H - 40.0, 16.0, TEXT_DIM);
    }

    fn draw_main_menu(&self, canvas: &mut Canvas) {
        // Top border line
        canvas.rectangle(0.0, 10.0, RENDER_W, 2.0, GOLD_DIM);

        // Title shadow + title
        let title_x = (RENDER_W - self.title_width) * 0.5;
        canvas.text(&self.title, title_x + 3.0, 63.0, 72.0, Color::new(0, 0, 0, 130));
        canvas.text(&self.title, title_x, 60.0, 72.0, TEXT_BRIGHT);

        // Double separator below title
        canvas.rectangle(100.0, 150.0, RENDER_W - 200.0, 1.0, GOLD_DIM);
        canvas.rectangle(100.0, 153.0, RENDER_W - 200.0, 1.0, GOLD);

        // Panel drop shadow
        canvas.rectangle(PANEL_X + 5.0, PANEL_Y + 5.0, PANEL_W, PANEL_H, Color::new(0, 0, 0, 70));
        // Panel background
        canvas.rectangle(PANEL_X, PANEL_Y, PANEL_W, PANEL_H, PANEL_BG);
        // Panel border (dim outer, bright top)
        draw_border(canvas, PANEL_X, PANEL_Y, PANEL_W, PANEL_H, GOLD_DIM);
        canvas.rectangle(PANEL_X + 2.0, PANEL_Y, PANEL_W - 4.0, 2.0, GOLD);

        // Buttons — all share the same width/height.
        for (i, item) in self.items.iter().enumerate() {
            self.draw_button(canvas, item, i);
        }

        // Bottom border line
        canvas.rectangle(0.0, RENDER_H - 12.0, RENDER_W, 2.0, GOLD_DIM);

        // Footer hint
        let hint_x = (RENDER_W - self.hint_width) * 0.5;
        canvas.text(&self.hint, hint_x, RENDER_H - 40.0, 16.0, TEXT_DIM);

        // Language button (bottom-right)
        self.draw_lang_button(canvas);
    }

    fn draw_background(&self, canvas: &mut Canvas) {
        // Deep night-sky gradient with a soft horizontal glow band.
        canvas.quad_gradient(
            Vec2D::new(0.0, 0.0),
            Vec2D::new(RENDER_W, 0.0),
            Vec2D::new(RENDER_W, RENDER_H),
            Vec2D::new(0.0, RENDER_H),
            BG_TOP, BG_TOP, BG_BOTTOM, BG_BOTTOM,
        );

        // Soft glow band behind the title area.
        let band_y = 90.0;
        let band_h = 120.0;
        canvas.quad_gradient(
            Vec2D::new(0.0, band_y),
            Vec2D::new(RENDER_W, band_y),
            Vec2D::new(RENDER_W, band_y + band_h),
            Vec2D::new(0.0, band_y + band_h),
            Color::new(0, 0, 0, 0),
            Color::new(0, 0, 0, 0),
            BG_GLOW.with_alpha(0.25),
            BG_GLOW.with_alpha(0.25),
        );

        draw_stars(canvas);
    }

    fn draw_ducks(&self, canvas: &mut Canvas) {
        for duck in &self.ducks {
            // Duck silhouettes: dimmer and slightly blue-shifted so they read as background.
            let tint = DUCK_TINT;
            duck.anim.draw_rotated(canvas, Vec2D::new(duck.x, duck.y), duck.scale, duck.flip, duck.rot, tint);
        }
    }

    fn draw_button(&self, canvas: &mut Canvas, item: &MenuItem, index: usize) {
        let selected = index == self.selected;
        let btn_x = PANEL_X + BTN_PAD_X;
        let btn_w = PANEL_W - BTN_PAD_X * 2.0;
        let btn_y = item.y;

        if selected {
            canvas.rectangle(btn_x, btn_y, btn_w, BTN_H, BTN_SEL_BG);
            // Gold left accent bar
            canvas.rectangle(btn_x, btn_y, 3.0, BTN_H, GOLD);
        }

        // Icon + label, grouped and centered horizontally in the button
        let icon_size = 22.0;
        let icon_text_gap = 10.0;
        let group_w = icon_size + icon_text_gap + item.label_width;
        let group_x = btn_x + (btn_w - group_w) * 0.5;
        let icon_y = btn_y + (BTN_H - icon_size) * 0.5;
        let text_y = btn_y + (BTN_H - FONT_SIZE) * 0.5;

        let icon_tint = if selected {
            GOLD_BRIGHT
        } else {
            Color::new(255, 255, 255, 150)
        };
        let text_color = if selected { TEXT_BRIGHT } else { TEXT_NORMAL };

        if let Some(icon) = self.icons.get(index) {
            let scale = icon_size / 16.0;
            canvas.draw_texture_ex(icon, Vec2D::new(group_x, icon_y), 0.0, scale, icon_tint);
        }
        canvas.text(
            &item.label,
            group_x + icon_size + icon_text_gap,
            text_y,
            FONT_SIZE,
            text_color,
        );

        // Right-pointing arrow on selected
        if selected {
            let ax = btn_x + btn_w - 20.0;
            let ay = btn_y + BTN_H * 0.5;
            canvas.triangle(
                Vec2D::new(ax, ay - 6.0),
                Vec2D::new(ax + 9.0, ay),
                Vec2D::new(ax, ay + 6.0),
                GOLD,
            );
        }
    }

    fn draw_lang_button(&self, canvas: &mut Canvas) {
        let r = self.lang_btn_rect;
        canvas.rectangle(r.x, r.y, r.width, r.height, Color::new(16, 20, 42, 200));
        draw_border(canvas, r.x, r.y, r.width, r.height, GOLD_DIM);

        // Hand-drawn globe
        let center = Vec2D::new(r.x + 18.0, r.y + r.height * 0.5);
        let rad = 10.0;
        canvas.circle(center, rad, Color::new(28, 52, 95, 255));
        canvas.circle(Vec2D::new(center.x - 3.0, center.y - 3.0), 3.5, Color::new(50, 108, 55, 255));
        canvas.circle(Vec2D::new(center.x + 3.5, center.y + 2.5), 3.5, Color::new(50, 108, 55, 255));
        canvas.circle(center, rad + 2.0, Color::new(255, 255, 255, 30));

        // Language code
        canvas.text(&self.lang_code, r.x + 33.0, r.y + (r.height - 18.0) * 0.5, 18.0, TEXT_NORMAL);
    }

    fn draw_language_overlay(&self, canvas: &mut Canvas) {
        // Dim everything behind
        canvas.rectangle(0.0, 0.0, RENDER_W, RENDER_H, Color::new(0, 0, 10, 185));

        let (lp_x, lp_y, lp_w, lp_h) = lang_panel_dims();

        // Panel shadow + bg + border
        canvas.rectangle(lp_x + 5.0, lp_y + 5.0, lp_w, lp_h, Color::new(0, 0, 0, 80));
        canvas.rectangle(lp_x, lp_y, lp_w, lp_h, PANEL_BG);
        draw_border(canvas, lp_x, lp_y, lp_w, lp_h, GOLD);
        canvas.rectangle(lp_x + 2.0, lp_y, lp_w - 4.0, 2.0, GOLD_BRIGHT);

        // Title
        let title_x = lp_x + (lp_w - self.lang_title_width) * 0.5;
        canvas.text("SELECT LANGUAGE", title_x, lp_y + 18.0, 24.0, TEXT_NORMAL);
        canvas.rectangle(lp_x + 16.0, lp_y + 55.0, lp_w - 32.0, 1.0, GOLD_DIM);

        // Language options
        for (i, (label, width)) in self.lang_labels.iter().enumerate() {
            let item_y = lp_y + 68.0 + i as f32 * 52.0;
            let selected = i == self.lang_selected;

            if selected {
                canvas.rectangle(lp_x + 20.0, item_y, lp_w - 40.0, 42.0, BTN_SEL_BG);
                canvas.rectangle(lp_x + 20.0, item_y, 3.0, 42.0, GOLD);
            }

            let text_color = if selected { TEXT_BRIGHT } else { TEXT_NORMAL };
            let label_x = lp_x + (lp_w - width) * 0.5;
            canvas.text(label, label_x, item_y + 8.0, 28.0, text_color);

            if selected {
                let cx = lp_x + lp_w - 34.0;
                let cy = item_y + 21.0;
                canvas.triangle(
                    Vec2D::new(cx, cy - 6.0),
                    Vec2D::new(cx + 10.0, cy),
                    Vec2D::new(cx, cy + 6.0),
                    GOLD,
                );
            }
        }

        // Back hint
        let back_x = lp_x + (lp_w - self.lang_back_hint_width) * 0.5;
        canvas.text("ESC to cancel", back_x, lp_y + lp_h - 28.0, 16.0, TEXT_DIM);
    }
}

fn lang_panel_dims() -> (f32, f32, f32, f32) {
    let w = 320.0;
    let h = 268.0;
    let x = (RENDER_W - w) * 0.5;
    let y = (RENDER_H - h) * 0.5;
    (x, y, w, h)
}

fn lang_code_str(lang: Lang) -> &'static str {
    match lang {
        Lang::English => "EN",
        Lang::Portuguese => "PT",
        Lang::Arabic => "AR",
    }
}

fn draw_stars(canvas: &mut Canvas) {
    let stars: &[(f32, f32, f32)] = &[
        (45.0, 28.0, 2.0), (118.0, 75.0, 1.0), (198.0, 18.0, 2.0), (305.0, 95.0, 1.0),
        (448.0, 42.0, 1.0), (602.0, 12.0, 2.0), (718.0, 66.0, 1.0), (852.0, 28.0, 2.0),
        (948.0, 88.0, 1.0), (1052.0, 18.0, 2.0), (1148.0, 52.0, 1.0), (1218.0, 78.0, 2.0),
        (78.0, 148.0, 1.0), (182.0, 168.0, 2.0), (342.0, 138.0, 1.0), (505.0, 158.0, 1.0),
        (698.0, 128.0, 2.0), (902.0, 152.0, 1.0), (1102.0, 142.0, 2.0),
        (58.0, 598.0, 1.0), (202.0, 638.0, 2.0), (402.0, 608.0, 1.0), (602.0, 658.0, 1.0),
        (798.0, 598.0, 2.0), (1002.0, 638.0, 1.0), (1152.0, 602.0, 2.0),
        (18.0, 340.0, 1.0), (1262.0, 282.0, 1.0), (1238.0, 402.0, 2.0),
    ];
    for &(x, y, s) in stars {
        canvas.rectangle(x, y, s, s, STAR);
    }
}

fn draw_border(canvas: &mut Canvas, x: f32, y: f32, w: f32, h: f32, color: Color) {
    canvas.rectangle(x, y, w, 2.0, color);
    canvas.rectangle(x, y + h - 2.0, w, 2.0, color);
    canvas.rectangle(x, y, 2.0, h, color);
    canvas.rectangle(x + w - 2.0, y, 2.0, h, color);
}

fn point_in_rect(point: Vec2D, rect: Rect) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}
