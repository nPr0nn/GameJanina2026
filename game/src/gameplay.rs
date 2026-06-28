//! The gameplay screen: chain-lasso prototype with obstacles, squeezable
//! circles, a movable box, and portals. World state lives here; global keys
//! (pause, fullscreen, win/lose) are handled by the screen manager in `main.rs`.

use std::cell::Cell;
use std::rc::Rc;

use juni::prelude::*;

use crate::animation::SpriteSheet;
use crate::chain::Chain;
use crate::collision::{push_rect_out_of_aabb, push_rect_out_of_circle, resolve_swept, Collider};
use crate::loc::Loc;
use crate::player::Player;
use crate::squeezable::Squeezables;

// A custom fragment shader: an animated rainbow driven by world position and
// `globals.time`. Same vertex/uniform interface as the built-in shape shader,
// so it plugs straight into `begin_shader_mode`.
/// Seconds of player stillness before the chain is allowed to freeze.
const PLAYER_STILL_THRESHOLD: f32 = 0.01;
/// Maximum joint displacement (px/frame) considered "totally still".
const CHAIN_STILL_THRESHOLD: f32 = 0.01;

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
    player: Player,
    mouse: Vec2D,
    rainbow: Shader,
    cow: Texture,
    /// The ducky sprite sheet that backs the player's animation. Kept here so a
    /// `reset` can hand a fresh clone to a new `Player`.
    ducky: SpriteSheet,
    /// When `true`, the editor's collision layer is drawn on top of the level
    /// (toggle with F3). Off by default — normal play shows only the sprites.
    debug_collisions: bool,
    pop: Sound,
    spin: f32,
    zoom: f32,
    fps: u32,
    loc: Loc,
    /// The level authored in the `editor` crate, in world coordinates. Drawn
    /// through the game camera, the same way the editor authored it.
    level: Level,
    chains: Vec<Chain>,
    chain_anchor: Vec2D,
    prev_player_pos: Vec2D,
    player_still_for: f32,
    /// Static obstacles in world space. Chains and the player both collide
    /// with these; chain joints wrap around them naturally.
    obstacles: Vec<Rect>,
    /// Round objects that get crushed when a chain loops tight around them.
    squeezables: Squeezables,
    /// Running squeeze tally, shared with the squeeze listener so the HUD can
    /// display it. Demonstrates the event/listener wiring.
    squeeze_count: Rc<Cell<u32>>,
    /// Scratch buffer for the per-frame collider set (blocks + live objects),
    /// reused to avoid a heap allocation every update.
    colliders: Vec<Collider>,
    test_movable: crate::movable::MovableBox,
}

impl Gameplay {
    pub fn new(ctx: &mut Context, loc: Loc) -> Self {
        let cow_texture = ctx.load_texture_from_memory(include_bytes!("../assets/sprites/vaca.png"));
        // The ducky sheet is a 6×4 grid of 32×32 frames (row 0 idle, row 1 walk).
        let ducky = SpriteSheet::from_memory(
            ctx,
            include_bytes!("../assets/ducky_spritesheet.png"),
            32,
            32,
        );
        let test_movable = crate::movable::MovableBox::new(Rect::new(200.0, 400.0, 50.0, 50.0));
        let chain_anchor = Vec2D::new(640.0, 100.0);
        let mut player = Player::new(ducky.clone());
        // Start close to the anchor so all chains begin with visible slack.
        player.pos = Vec2D::new(640.0, 150.0);
        // Three chains sharing the same anchor but with different lengths and tints.
        let chains = vec![
            Chain::new(chain_anchor, player.pos, 1600.0, 6.0, RED),
            Chain::new(chain_anchor, player.pos, 2400.0, 6.0, LIME),
            Chain::new(chain_anchor, player.pos, 3200.0, 6.0, SKYBLUE),
        ];
        // Two solid blocks the player and chains can collide with.
        // Placed within reach of all three chains (max 2200 px from anchor).
        let obstacles = vec![
            Rect::new(450.0, 220.0, 90.0, 90.0),
            Rect::new(720.0, 310.0, 110.0, 70.0),
        ];

        let mut squeezables = Squeezables::new();
        squeezables.spawn(Vec2D::new(1000.0, 550.0), 45.0);
        let squeeze_count = Rc::new(Cell::new(0u32));
        let counter = squeeze_count.clone();
        squeezables.on_squeeze(move |_| {
            counter.set(counter.get() + 1);
        });

        let prev_player_pos = player.pos;

        Self {
            x: 100.0,
            dir: 1.0,
            test_movable,
            mouse: Vec2D::ZERO,
            rainbow: ctx.load_shader_from_memory(RAINBOW_SHADER),
            cow: cow_texture.clone(),
            ducky,
            debug_collisions: false,
            player,
            pop: ctx.load_sound_from_memory(include_bytes!("../assets/audio/bolha.wav")),
            spin: 0.0,
            zoom: 1.0,
            fps: 0,
            loc,
            level: load_level(),
            chains,
            chain_anchor,
            prev_player_pos,
            player_still_for: 0.0,
            obstacles,
            squeezables,
            squeeze_count,
            colliders: Vec::new(),
        }
    }

    /// Reset world state for a fresh run (called when entering gameplay from the
    /// menu or after a win/defeat). Reuses the already-loaded GPU/audio assets.
    pub fn reset(&mut self) {
        self.x = 100.0;
        self.dir = 1.0;
        self.player = Player::new(self.ducky.clone());
        self.player.pos = Vec2D::new(640.0, 150.0);
        self.chains = vec![
            Chain::new(self.chain_anchor, self.player.pos, 1600.0, 6.0, RED),
            Chain::new(self.chain_anchor, self.player.pos, 2400.0, 6.0, LIME),
            Chain::new(self.chain_anchor, self.player.pos, 3200.0, 6.0, SKYBLUE),
        ];
        self.test_movable = crate::movable::MovableBox::new(Rect::new(200.0, 400.0, 50.0, 50.0));
        self.spin = 0.0;
        self.zoom = 1.0;
        self.obstacles = vec![
            Rect::new(450.0, 220.0, 90.0, 90.0),
            Rect::new(720.0, 310.0, 110.0, 70.0),
        ];
        self.squeezables.revive_all();
        self.squeeze_count.set(0);
        self.prev_player_pos = self.player.pos;
        self.player_still_for = 0.0;
    }

    /// Advance the world one fixed step. Only called while actually playing, so
    /// pausing freezes everything here automatically.
    pub fn update(&mut self, ctx: &mut Context) {
        self.mouse = ctx.mouse_position();
        self.fps = ctx.fps;

        // Toggle the collision-layer debug overlay.
        if ctx.is_key_pressed(Key::F3) {
            self.debug_collisions = !self.debug_collisions;
        }

        // Play a pop on each left-click.
        if ctx.is_mouse_button_pressed(MouseButton::Left) {
            ctx.play_sound(&self.pop);
        }

        // Track how long the player has been still.
        if self.player.pos.distance_squared(self.prev_player_pos) > 1e-4 {
            self.player_still_for = 0.0;
            self.prev_player_pos = self.player.pos;
        } else {
            self.player_still_for += ctx.dt;
        }

        // Build the collider set: blocks + living squeezables.
        self.colliders.clear();
        self.colliders
            .extend(self.obstacles.iter().map(|&r| Collider::Aabb(r)));
        self.squeezables.extend_colliders(&mut self.colliders);

        // Move the player against blocks, then push out of round objects.
        let move_dir = self.player.input_direction(ctx);
        let vel = move_dir * self.player.speed * ctx.dt;
        self.player.pos =
            resolve_swept(self.player.pos, self.player.shape, vel, &self.obstacles);
        for (center, radius) in self.squeezables.alive() {
            if let Some((new_pos, _)) =
                push_rect_out_of_circle(self.player.pos, self.player.shape, center, radius)
            {
                self.player.pos = new_pos;
            }
        }

        // Simulate chains only while the player is moving or the chain is still
        // settling. Once the player stops and the chains go totally still, freeze
        // them to avoid micro-oscillations and save work.
        let chains_frozen = self.player_still_for >= PLAYER_STILL_THRESHOLD
            && self.chains.iter().all(|c| c.is_still(CHAIN_STILL_THRESHOLD));
        for chain in &mut self.chains {
            chain.set_start(self.chain_anchor);
            chain.set_end(self.player.pos);
            if !chains_frozen {
                chain.update(ctx.dt, &self.colliders);
            }
        }

        // Clamp the player to each chain's remaining free length.
        for _ in 0..4 {
            let mut target = self.player.pos;
            for chain in &self.chains {
                let (tether, free_len) = chain.player_tether();
                let dist = tether.distance(target);
                if dist > free_len {
                    let dir = (target - tether).try_normalize().unwrap_or(-Vec2D::Y);
                    target = tether + dir * free_len;
                }
            }
            let delta = target - self.player.pos;
            if delta.length_squared() < 1e-4 {
                break; // converged
            }
            self.player.pos =
                resolve_swept(self.player.pos, self.player.shape, delta, &self.obstacles);
        }

        // Push the player out of any overlapping block or object.
        for &rect in &self.obstacles {
            if let Some((new_pos, _)) =
                push_rect_out_of_aabb(self.player.pos, self.player.shape, rect)
            {
                self.player.pos = new_pos;
            }
        }
        for (center, radius) in self.squeezables.alive() {
            if let Some((new_pos, _)) =
                push_rect_out_of_circle(self.player.pos, self.player.shape, center, radius)
            {
                self.player.pos = new_pos;
            }
        }

        for chain in &mut self.chains {
            chain.set_end(self.player.pos);
        }

        // Crush any object a chain has cinched tight.
        self.squeezables.update(&self.chains);

        self.spin += 90.0 * ctx.dt;

        self.zoom = (self.zoom + ctx.mouse_wheel_move() * 0.1).clamp(0.1, 4.0);

        self.x += self.dir * 240.0 * ctx.dt;
        if self.x > 1080.0 {
            self.x = 1080.0;
            self.dir = -1.0;
        } else if self.x < 100.0 {
            self.x = 100.0;
            self.dir = 1.0;
        }

        self.test_movable.update(ctx);

        // Push the movable box when the player walks into it.
        let player_rect = Rect::new(self.player.pos.x, self.player.pos.y, self.player.shape.x, self.player.shape.y);
        let box_rect = self.test_movable.rect;
        if player_rect.intersects(&box_rect) {
            let impulse = self.player.velocity * self.player.player_speed * ctx.dt;
            self.test_movable.push(impulse);
            self.player.player_speed = 200.0; // Slow down while pushing
        } else {
            self.player.player_speed = 500.0; // Restore normal speed
        }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.clear_background(WHITE);

        let camera = Camera2D {
            target: self.player.pos + Vec2D::new(50.0, 50.0),
            offset: Vec2D::new(640.0, 360.0),
            rotation: 0.0,
            zoom: self.zoom,
        };
        canvas.begin_mode_2d(camera);

        for &rect in &self.obstacles {
            canvas.rectangle_from_rect(rect, DARKGRAY);
        }

        canvas.begin_shader_mode(&self.rainbow);
        canvas.quad(
            Vec2D::new(400.0, 300.0),
            Vec2D::new(500.0, 300.0),
            Vec2D::new(500.0, 480.0),
            Vec2D::new(400.0, 480.0),
            RED,
        );
        canvas.end_shader_mode();

        for (pos, radius) in self.squeezables.alive() {
            canvas.circle(pos, radius, MAGENTA);
            canvas.circle(pos, radius - 6.0, PINK);
        }

        for chain in &self.chains {
            chain.draw(canvas);
        }
        canvas.circle(self.chain_anchor, 12.0, GOLD);

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

        self.player.draw(canvas);

        // The sprite-planning layer authored in the editor.
        self.level.draw(canvas);

        // Debug view: overlay the editor's collision layer (translucent so the
        // sprites underneath stay visible). Off during normal play.
        if self.debug_collisions {
            for shape in &self.level.collision_shapes {
                shape.with_alpha(0.4).draw(canvas);
            }
        }

        self.test_movable.draw(canvas);
        canvas.end_mode_2d();

        canvas.text(&self.loc.fps(self.fps), 20.0, 20.0, 28.0, LIME);
        canvas.text(
            &self.loc.squeezed(self.squeeze_count.get()),
            20.0,
            52.0,
            28.0,
            MAGENTA,
        );
        canvas.text(self.loc.hud_controls(), 20.0, 680.0, 24.0, WHITE);
    }
}

/// The level authored in the editor, embedded at build time. Embedding (rather
/// than reading a file at runtime) keeps the level available identically on
/// native and on the web, and independent of the working directory.
const EDITOR_LEVEL_JSON: &str = include_str!("new.json");

/// Parse the embedded editor level. A malformed file falls back to an empty
/// level rather than crashing the game.
fn load_level() -> Level {
    Level::from_json(EDITOR_LEVEL_JSON).unwrap_or_default()
}
