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
use crate::collision::{depenetrate_aabb, push_rect_out_of_aabb, resolve_aabb, Collider};
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
/// Classification tag (authored in the editor) marking a collision box the
/// player can push around. Anything not tagged `mov` is treated as static.
const TAG_MOVABLE: &str = "mov";
/// Side length (world pixels) of the snap grid the movable boxes settle onto.
/// Boxes move freely while being pushed, then snap to the nearest cell once the
/// player lets go — so accumulated float drift is erased every time one rests.
const GRID_SIZE: f32 = 4.0;

/// Snap a single world coordinate to the nearest [`GRID_SIZE`] line.
fn snap_to_grid(v: f32) -> f32 {
    (v / GRID_SIZE).round() * GRID_SIZE
}

/// A pushable box derived from a `mov`-tagged collision rectangle. Its `rect`
/// is the live world-space AABB; the player both collides with it and shoves it.
/// `sprites` links the box to every sprite instance that shares its object ID
/// (via the editor's classification), so the artwork slides along with the box.
struct MovableBox {
    /// Current world-space AABB (top-left + size).
    rect: Rect,
    /// Initial top-left, restored on [`Gameplay::reset`].
    origin: Vec2D,
    /// `(sprite_instance index, offset from box top-left to sprite top-left)`
    /// for each sprite that moves rigidly with this box.
    sprites: Vec<(usize, Vec2D)>,
}

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
    /// The level's *static* collision layer as colliders (world space), derived
    /// once at load from every collision shape not tagged `mov`.
    static_colliders: Vec<Collider>,
    /// Pushable boxes derived from the `mov`-tagged collision shapes. Their
    /// positions change at runtime as the player shoves them.
    boxes: Vec<MovableBox>,
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
    /// Scratch buffer for the per-frame chain collider set (static shapes +
    /// movable boxes + live squeezables), reused to avoid a heap allocation
    /// every update.
    colliders: Vec<Collider>,
    /// Scratch buffer for the player's movement set: the static world plus live
    /// squeezables, but *not* the movable boxes (those are pushed, not slid on).
    move_colliders: Vec<Collider>,
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
        let static_colliders = static_colliders_from(&level);
        let boxes = movable_boxes_from(&level);
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
            static_colliders,
            boxes,
            sprite_textures,
            player_start,
            squeezables,
            squeeze_count,
            colliders: Vec::new(),
            move_colliders: Vec::new(),
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
        // Slide every movable box (and its linked sprites) back to where the
        // editor authored it.
        for i in 0..self.boxes.len() {
            let delta = self.boxes[i].origin - self.boxes[i].rect.position();
            self.move_box(i, delta);
        }
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

        // Integrate the player's velocity from input (acceleration + friction).
        self.player.input_direction(ctx);

        // ── Collision phase ─────────────────────────────────────────────────
        // Move the player continuously by its velocity (sliding on the static
        // world) and shove any movable box it runs into. Then rebuild the full
        // collider set — now including the boxes at their new positions — let
        // the chains follow and simulate, constrain the player to the
        // (now-current) chain lengths, and finish with a depenetration pass.
        self.rebuild_move_colliders();
        self.move_player_and_push(ctx);
        self.rebuild_colliders();
        self.step_chains(ctx);
        self.constrain_player_to_chains();
        self.player.pos = depenetrate_aabb(self.player.pos, self.player.shape, &self.colliders);

        // Once the player has fully stopped, settle it onto the grid too, so it
        // comes to rest aligned with the boxes it pushes. Only when idle, so this
        // never tugs at the player mid-move.
        self.snap_player();

        // Sync the chain ends to the player's final attachment point.
        let end = self.player.chain_point();
        for chain in &mut self.chains {
            chain.set_end(end);
        }

        // Crush any object a chain has cinched tight.
        self.squeezables.update(&self.chains);
    }

    /// Rebuild the full per-frame collider set (used by the chains and the final
    /// depenetration): static colliders, then the movable boxes at their current
    /// positions, then the currently-alive squeezables.
    fn rebuild_colliders(&mut self) {
        self.colliders.clear();
        self.colliders.extend(self.static_colliders.iter().copied());
        for b in &self.boxes {
            self.colliders.push(Collider::Aabb(b.rect));
        }
        self.squeezables.extend_colliders(&mut self.colliders);
    }

    /// Rebuild the player's *movement* collider set: the static world plus the
    /// live squeezables, deliberately excluding the movable boxes so the player
    /// pushes them instead of sliding off them.
    fn rebuild_move_colliders(&mut self) {
        self.move_colliders.clear();
        self.move_colliders.extend(self.static_colliders.iter().copied());
        self.squeezables.extend_colliders(&mut self.move_colliders);
    }

    /// Move the player by its velocity for this frame, resolved continuously
    /// against the static world, then shove any movable box it ends up
    /// overlapping. Each box is pushed only as far as the static world and the
    /// other boxes permit; whatever it can't absorb (e.g. it's wedged against a
    /// wall) is fed back onto the player so the player never ends up inside it.
    fn move_player_and_push(&mut self, ctx: &Context) {
        let displacement = self.player.velocity * ctx.dt;
        self.player.pos =
            resolve_aabb(self.player.pos, self.player.shape, displacement, &self.move_colliders);

        // Which boxes the player is in contact with this frame. A box that's
        // being touched moves freely (smooth pushing); the rest are settled onto
        // the grid below.
        let mut touched = vec![false; self.boxes.len()];

        for i in 0..self.boxes.len() {
            let player_rect = Rect::new(
                self.player.pos.x,
                self.player.pos.y,
                self.player.shape.x,
                self.player.shape.y,
            );
            let box_rect = self.boxes[i].rect;
            // Minimum translation that separates the box from the player; `want`
            // points away from the player (roughly along the player's travel).
            let Some((new_box_pos, _)) =
                push_rect_out_of_aabb(box_rect.position(), box_rect.size(), player_rect)
            else {
                continue;
            };
            touched[i] = true;
            let want = new_box_pos - box_rect.position();

            // The box may only move where the static world and the other boxes
            // allow it to.
            let obstacles = self.box_obstacles(i);
            let resolved = resolve_aabb(box_rect.position(), box_rect.size(), want, &obstacles);
            let moved = resolved - box_rect.position();
            self.move_box(i, moved);

            // Push the player back by whatever separation the box couldn't take.
            let residual = want - moved;
            if residual.length_squared() > 1e-6 {
                self.player.pos = resolve_aabb(
                    self.player.pos,
                    self.player.shape,
                    -residual,
                    &self.move_colliders,
                );
            }
        }

        // Settle every box the player isn't touching onto the grid, so resting
        // boxes always sit on exact 4-pixel coordinates and never accumulate the
        // float drift of repeated free-coordinate pushes. Boxes in contact are
        // left alone so snapping never fights the player mid-push.
        for i in 0..self.boxes.len() {
            if !touched[i] {
                self.snap_box(i);
            }
        }
    }

    /// Nudge box `i` to the nearest grid cell, clamped by the static world and
    /// the other boxes so settling can never shove it into a wall. The move is
    /// at most half a cell, so it's an imperceptible correction in practice.
    fn snap_box(&mut self, i: usize) {
        let rect = self.boxes[i].rect;
        let target = Vec2D::new(snap_to_grid(rect.x), snap_to_grid(rect.y));
        let delta = target - rect.position();
        if delta.length_squared() < 1e-12 {
            return;
        }
        let obstacles = self.box_obstacles(i);
        let resolved = resolve_aabb(rect.position(), rect.size(), delta, &obstacles);
        self.move_box(i, resolved - rect.position());
    }

    /// Snap the player to the nearest grid cell, but only when it has come to a
    /// complete stop (`velocity == 0`, which friction only reaches with no input
    /// held). The move is clamped against the world so it can't snap into a wall,
    /// and at most half a cell, so it reads as a clean "click into place".
    fn snap_player(&mut self) {
        if self.player.velocity != Vec2D::ZERO {
            return;
        }
        let pos = self.player.pos;
        let target = Vec2D::new(snap_to_grid(pos.x), snap_to_grid(pos.y));
        let delta = target - pos;
        if delta.length_squared() < 1e-12 {
            return;
        }
        self.player.pos = resolve_aabb(pos, self.player.shape, delta, &self.colliders);
    }

    /// Draw the snap grid as thin lines in a small neighbourhood around every
    /// movable box. Bounded per box (not across the whole world) so a 4-pixel
    /// grid stays cheap and legible — it's only ever shown in the F3 debug view.
    fn draw_debug_grid(&self, canvas: &mut Canvas) {
        /// Cells of margin drawn around each box.
        const MARGIN_CELLS: f32 = 4.0;
        /// Line thickness in world pixels.
        const LINE_W: f32 = 0.5;
        let color = LIGHTGRAY.with_alpha(0.2);

        for b in &self.boxes {
            let x0 = snap_to_grid(b.rect.x) - MARGIN_CELLS * GRID_SIZE;
            let y0 = snap_to_grid(b.rect.y) - MARGIN_CELLS * GRID_SIZE;
            let x1 = snap_to_grid(b.rect.x + b.rect.width) + MARGIN_CELLS * GRID_SIZE;
            let y1 = snap_to_grid(b.rect.y + b.rect.height) + MARGIN_CELLS * GRID_SIZE;

            let mut x = x0;
            while x <= x1 {
                canvas.rectangle(x, y0, LINE_W, y1 - y0, color);
                x += GRID_SIZE;
            }
            let mut y = y0;
            while y <= y1 {
                canvas.rectangle(x0, y, x1 - x0, LINE_W, color);
                y += GRID_SIZE;
            }
        }
    }

    /// The collider set a single box is allowed to move against: the static
    /// world plus every *other* movable box (so boxes can't be shoved through
    /// each other).
    fn box_obstacles(&self, skip: usize) -> Vec<Collider> {
        let mut obstacles = self.static_colliders.clone();
        for (j, b) in self.boxes.iter().enumerate() {
            if j != skip {
                obstacles.push(Collider::Aabb(b.rect));
            }
        }
        obstacles
    }

    /// Translate box `i` by `delta`, dragging every sprite linked to it (those
    /// sharing its object ID) along rigidly.
    fn move_box(&mut self, i: usize, delta: Vec2D) {
        if delta == Vec2D::ZERO {
            return;
        }
        self.boxes[i].rect.x += delta.x;
        self.boxes[i].rect.y += delta.y;
        let pos = self.boxes[i].rect.position();
        for k in 0..self.boxes[i].sprites.len() {
            let (idx, offset) = self.boxes[i].sprites[k];
            let inst = &mut self.level.sprite_instances[idx];
            inst.x = pos.x + offset.x;
            inst.y = pos.y + offset.y;
        }
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
            // The snap grid, drawn faintly around each movable box so the cells
            // the boxes settle onto are visible while debugging.
            self.draw_debug_grid(canvas);
            // Static collision shapes (translucent) drawn from the level...
            for shape in &self.level.collision_shapes {
                if self.level.get_tag(shape.id()) == Some(TAG_MOVABLE) {
                    continue;
                }
                shape.with_alpha(0.4).draw(canvas);
            }
            // ...and the movable boxes at their *current* positions, tinted
            // differently so the two kinds are easy to tell apart.
            for b in &self.boxes {
                canvas.rectangle(
                    b.rect.x,
                    b.rect.y,
                    b.rect.width,
                    b.rect.height,
                    ORANGE.with_alpha(0.4),
                );
            }
            self.player.draw_collider(canvas);
        }

        for (pos, radius) in self.squeezables.alive() {
            canvas.circle(pos, radius, MAGENTA);
            canvas.circle(pos, radius * 0.7, PINK);
        }

        for chain in &self.chains {
            chain.draw(canvas);
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

/// Convert the level's *static* collision shapes into colliders, in world
/// coordinates. Done once at load — every shape not tagged `mov` is static and
/// never changes at runtime. The `mov`-tagged shapes become [`MovableBox`]es
/// instead (see [`movable_boxes_from`]).
fn static_colliders_from(level: &Level) -> Vec<Collider> {
    level
        .collision_shapes
        .iter()
        .filter(|shape| level.get_tag(shape.id()) != Some(TAG_MOVABLE))
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

/// Build the pushable boxes from the `mov`-tagged collision rectangles.
///
/// Each box is linked to every sprite instance that shares its object ID — the
/// correlation the editor's classification layer is for — so the artwork is
/// dragged along when the box is shoved. Only rectangles become boxes; a `mov`
/// tag on a circle is ignored (boxes are axis-aligned).
fn movable_boxes_from(level: &Level) -> Vec<MovableBox> {
    let mut boxes = Vec::new();
    for shape in &level.collision_shapes {
        if level.get_tag(shape.id()) != Some(TAG_MOVABLE) {
            continue;
        }
        let Shape::Rect {
            id,
            x,
            y,
            width,
            height,
            ..
        } = shape
        else {
            continue;
        };
        // Start on the grid: snap the collision box, but keep each sprite at its
        // authored position by baking the (sub-cell) difference into its offset.
        // The sprites then track the box exactly as it's pushed and re-snapped.
        let rect = Rect::new(snap_to_grid(*x), snap_to_grid(*y), *width, *height);
        let sprites = level
            .sprite_instances
            .iter()
            .enumerate()
            .filter(|(_, inst)| inst.id == *id)
            .map(|(idx, inst)| (idx, Vec2D::new(inst.x - rect.x, inst.y - rect.y)))
            .collect();
        boxes.push(MovableBox {
            rect,
            origin: rect.position(),
            sprites,
        });
    }
    boxes
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
