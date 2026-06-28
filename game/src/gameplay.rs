//! The gameplay screen. It renders the level authored in the editor (the
//! sprite-planning layer) and the player, plus the chain-lasso mechanic. The
//! player walks with WASD / arrows and collides with the level's collision
//! layer; press F3 to draw that collision layer on top for debugging.

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use juni::prelude::*;

use crate::animation::SpriteSheet;
use crate::chain::Chain;
use crate::collision::{depenetrate_aabb, resolve_aabb, Collider};
use crate::loc::Loc;
use crate::player::Player;
use crate::squeezable::Squeezables;

/// Where the player spawns, in world coordinates.
const PLAYER_START: Vec2D = Vec2D::new(0.0, 0.0);
/// Where the chains are anchored, in world coordinates.
const CHAIN_ANCHOR: Vec2D = Vec2D::new(0.0, 0.0);
/// Seconds of player stillness before the chain is allowed to freeze.
const PLAYER_STILL_THRESHOLD: f32 = 0.01;
/// Maximum joint displacement (px/frame) considered "totally still".
const CHAIN_STILL_THRESHOLD: f32 = 0.01;
/// Iterations of the chain-length constraint solved per frame.
const CHAIN_CLAMP_ITERS: usize = 4;

pub struct Gameplay {
    player: Player,
    /// The ducky sprite sheet that backs the player's animation. Kept here so a
    /// `reset` can hand a fresh clone to a new `Player`.
    ducky: SpriteSheet,
    /// When `true`, the level's collision layer is drawn on top of the level
    /// (toggle with F3). Off by default — normal play shows only the sprites.
    debug_collisions: bool,
    zoom: f32,
    fps: u32,
    loc: Loc,
    /// The level authored in the editor, in world coordinates. Drawn through the
    /// game camera, the same way the editor authored it.
    level: Level,
    /// The level's collision layer as colliders (world space), derived once at
    /// load. Static for the lifetime of the level.
    level_colliders: Vec<Collider>,
    /// Sprite-instance textures, keyed by their PNG path. Loaded once at startup
    /// from the paths the editor recorded. Empty on the web (no filesystem).
    sprite_textures: HashMap<String, Texture>,
    /// Player spawn in world coordinates: the level's authored point, or the
    /// `PLAYER_START` fallback. Reused by `reset`.
    player_start: Vec2D,
    chains: Vec<Chain>,
    prev_player_pos: Vec2D,
    player_still_for: f32,
    /// Round objects that get crushed when a chain loops tight around them.
    squeezables: Squeezables,
    /// Running squeeze tally, shared with the squeeze listener so the HUD can
    /// display it. Demonstrates the event/listener wiring.
    squeeze_count: Rc<Cell<u32>>,
    /// Scratch buffer for the per-frame chain collider set (level shapes + live
    /// squeezables), reused to avoid a heap allocation every update.
    colliders: Vec<Collider>,
}

impl Gameplay {
    pub fn new(ctx: &mut Context, loc: Loc) -> Self {
        // The ducky sheet is a 6×4 grid of 32×32 frames (row 0 idle, row 1 walk).
        let ducky = SpriteSheet::from_memory(
            ctx,
            include_bytes!("../assets/ducky_spritesheet.png"),
            32,
            32,
        );
        let level = load_level();
        let level_colliders = level_colliders_from(&level);
        let sprite_textures = load_sprite_textures(ctx, &level);
        // Spawn where the editor authored it, else the built-in default.
        let player_start = level.player_start_world().unwrap_or(PLAYER_START);

        let mut player = Player::new(ducky.clone());
        player.pos = player_start;

        let mut squeezables = Squeezables::new();
        squeezables.spawn(Vec2D::new(1000.0, 550.0), 18.0);
        let squeeze_count = Rc::new(Cell::new(0u32));
        let counter = squeeze_count.clone();
        squeezables.on_squeeze(move |_| {
            counter.set(counter.get() + 1);
        });

        Self {
            chains: new_chains(player.pos),
            prev_player_pos: player.pos,
            player_still_for: 0.0,
            player,
            ducky,
            debug_collisions: true,
            zoom: 3.0,
            fps: 0,
            loc,
            level,
            level_colliders,
            sprite_textures,
            player_start,
            squeezables,
            squeeze_count,
            colliders: Vec::new(),
        }
    }

    /// Reset world state for a fresh run (called when entering gameplay from the
    /// menu or after a win/defeat). Reuses the already-loaded assets and the
    /// parsed level (its collision layer never changes at runtime).
    pub fn reset(&mut self) {
        self.player = Player::new(self.ducky.clone());
        self.player.pos = self.player_start;
        self.chains = new_chains(self.player.pos);
        self.zoom = 1.0;
        self.squeezables.revive_all();
        self.squeeze_count.set(0);
        self.prev_player_pos = self.player.pos;
        self.player_still_for = 0.0;
    }

    /// Advance the world one fixed step. Only called while actually playing, so
    /// pausing freezes everything here automatically.
    pub fn update(&mut self, ctx: &mut Context) {
        self.fps = ctx.fps;

        // Toggle the collision-layer debug overlay.
        if ctx.is_key_pressed(Key::F3) {
            self.debug_collisions = !self.debug_collisions;
        }

        self.zoom = (self.zoom + ctx.mouse_wheel_move() * 0.1).clamp(0.1, 4.0);

        // Track how long the player has been still.
        if self.player.pos.distance_squared(self.prev_player_pos) > 1e-4 {
            self.player_still_for = 0.0;
            self.prev_player_pos = self.player.pos;
        } else {
            self.player_still_for += ctx.dt;
        }

        // Rebuild the world collider set: the level's static shapes plus the
        // live squeezables. Both the player and the chains resolve against it.
        self.rebuild_colliders();

        // Integrate the player's velocity from input (acceleration + friction).
        self.player.input_direction(ctx);

        // ── Collision phase ─────────────────────────────────────────────────
        // Move the player continuously by its velocity, let the chains follow
        // and simulate, then constrain the player to the (now-current) chain
        // lengths. A final depenetration pass un-sticks any residual overlap.
        self.move_player(ctx);
        self.step_chains(ctx);
        self.constrain_player_to_chains();
        self.player.pos = depenetrate_aabb(self.player.pos, self.player.shape, &self.colliders);

        // Sync the chain ends to the player's final attachment point.
        let end = self.player.chain_point();
        for chain in &mut self.chains {
            chain.set_end(end);
        }

        // Crush any object a chain has cinched tight.
        self.squeezables.update(&self.chains);
    }

    /// Rebuild the per-frame collider set into the reused `colliders` buffer:
    /// the level's static colliders followed by the currently-alive squeezables.
    fn rebuild_colliders(&mut self) {
        self.colliders.clear();
        self.colliders.extend(self.level_colliders.iter().copied());
        self.squeezables.extend_colliders(&mut self.colliders);
    }

    /// Move the player by its velocity for this frame, resolved continuously so
    /// it slides along the first surface it meets and never tunnels.
    fn move_player(&mut self, ctx: &Context) {
        let displacement = self.player.velocity * ctx.dt;
        self.player.pos =
            resolve_aabb(self.player.pos, self.player.shape, displacement, &self.colliders);
    }

    /// Drive the chains to follow the player's attachment point and simulate
    /// them. They freeze once the player and the chains have all gone still, to
    /// avoid micro-oscillations and save work.
    fn step_chains(&mut self, ctx: &Context) {
        let chains_frozen = self.player_still_for >= PLAYER_STILL_THRESHOLD
            && self.chains.iter().all(|c| c.is_still(CHAIN_STILL_THRESHOLD));
        let end = self.player.chain_point();
        for chain in &mut self.chains {
            chain.set_start(CHAIN_ANCHOR);
            chain.set_end(end);
            if !chains_frozen {
                chain.update(ctx.dt, &self.colliders);
            }
        }
    }

    /// Constrain the player so each chain's attachment point stays within its
    /// remaining free length. The pull is resolved continuously (so a taut chain
    /// can't drag the player through a wall); since the attachment point moves
    /// rigidly with `pos`, a correction on it applies one-for-one to `pos`.
    fn constrain_player_to_chains(&mut self) {
        for _ in 0..CHAIN_CLAMP_ITERS {
            let mut target = self.player.chain_point();
            for chain in &self.chains {
                let (tether, free_len) = chain.player_tether();
                let dist = tether.distance(target);
                if dist > free_len {
                    let dir = (target - tether).try_normalize().unwrap_or(-Vec2D::Y);
                    target = tether + dir * free_len;
                }
            }
            let delta = target - self.player.chain_point();
            if delta.length_squared() < 1e-4 {
                break; // converged
            }
            self.player.pos =
                resolve_aabb(self.player.pos, self.player.shape, delta, &self.colliders);
        }
    }

    pub fn draw(&self, canvas: &mut Canvas) {
        canvas.clear_background(BLACK);

        let camera = Camera2D {
            // Centre the view on the duck (offset = half the hit-box).
            target: self.player.pos + Vec2D::new(14.0, 14.0),
            offset: Vec2D::new(640.0, 360.0),
            rotation: 0.0,
            zoom: self.zoom,
        };
        canvas.begin_mode_2d(camera);

        // Sprite instances authored in the editor (the actual PNGs), drawn at
        // their top-left world position and scale — the same call the editor
        // uses, so they land pixel-identically. Drawn first, as the background.
        for inst in &self.level.sprite_instances {
            if let Some(tex) = self.sprite_textures.get(&inst.path) {
                canvas.draw_texture_ex(tex, Vec2D::new(inst.x, inst.y), 0.0, inst.scale, WHITE);
            }
        }

        // The sprite-planning layer's placeholder shapes authored in the editor.
        self.level.draw(canvas);

        // Debug view: overlay the level's collision layer (translucent so the
        // sprites underneath stay visible). Off during normal play.
        if self.debug_collisions {
            for shape in &self.level.collision_shapes {
                shape.with_alpha(0.4).draw(canvas);
            }
            self.player.draw_collider(canvas);
        }

        for (pos, radius) in self.squeezables.alive() {
            canvas.circle(pos, radius, MAGENTA);
            canvas.circle(pos, radius * 0.7, PINK);
        }

        // Draw the chains largest-first so the shortest ends up stacked on top.
        // Opacity ramps from full on the topmost (shortest) chain down toward
        // zero on the longest, so the deeper layers fade out.
        let mut ordered: Vec<&Chain> = self.chains.iter().collect();
        ordered.sort_by(|a, b| b.max_length().total_cmp(&a.max_length()));
        let n = ordered.len();
        for (i, chain) in ordered.iter().enumerate() {
            // i == 0 is the longest (bottom); i == n - 1 is the shortest (top).
            let alpha = ((i + 1) as f32 / n as f32).powi(4);
            chain.draw(canvas, alpha);
        }
        canvas.circle(CHAIN_ANCHOR, 6.0, GOLD);

        self.player.draw(canvas);
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

/// Build the three chains that tether the player to `CHAIN_ANCHOR`, each with a
/// different length and tint, all starting at the player's spawn.
fn new_chains(player_pos: Vec2D) -> Vec<Chain> {
    vec![
        // Chain::new(CHAIN_ANCHOR, player_pos, 1600.0, 3.0, RED),
        // Chain::new(CHAIN_ANCHOR, player_pos, 2400.0, 3.0, LIME),
        // Chain::new(CHAIN_ANCHOR, player_pos, 3200.0, 3.0, SKYBLUE),
    ]
}

/// Convert the level's collision layer into colliders, in world coordinates.
/// Done once at load — the collision layer is static at runtime.
fn level_colliders_from(level: &Level) -> Vec<Collider> {
    level
        .collision_shapes
        .iter()
        .map(|shape| match shape {
            Shape::Rect {
                x, y, width, height, ..
            } => Collider::Aabb(Rect::new(*x, *y, *width, *height)),
            Shape::Circle { x, y, radius, .. } => Collider::Circle {
                center: Vec2D::new(*x, *y),
                radius: *radius,
            },
        })
        .collect()
}

/// Load a texture for each unique sprite-instance path the editor recorded.
///
/// Native only: `ctx.load_texture` reads PNGs from disk (paths are relative to
/// the working directory, where the editor saved them under `sprites/`). On the
/// web there is no synchronous filesystem, so this returns an empty map and the
/// sprite instances simply don't draw (the embedded level JSON still loads).
#[cfg(not(target_arch = "wasm32"))]
fn load_sprite_textures(ctx: &mut Context, level: &Level) -> HashMap<String, Texture> {
    let mut cache = HashMap::new();
    for inst in &level.sprite_instances {
        if cache.contains_key(&inst.path) {
            continue;
        }
        if let Ok(tex) = ctx.load_texture(&inst.path) {
            cache.insert(inst.path.clone(), tex);
        }
    }
    cache
}

#[cfg(target_arch = "wasm32")]
fn load_sprite_textures(_ctx: &mut Context, _level: &Level) -> HashMap<String, Texture> {
    HashMap::new()
}

/// The level authored in the editor, embedded at build time. Embedding (rather
/// than reading a file at runtime) keeps the level available identically on
/// native and on the web, and independent of the working directory.
const EDITOR_LEVEL_JSON: &str = include_str!("../../level.json");

/// Parse the embedded editor level. A malformed file falls back to an empty
/// level rather than crashing the game.
fn load_level() -> Level {
    Level::from_json(EDITOR_LEVEL_JSON).unwrap_or_default()
}
