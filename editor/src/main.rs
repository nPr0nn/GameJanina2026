// A tiny mouse-driven level editor for juni.
//
//   cargo run -p editor
//
// Left-drag to place shapes, click a shape to select/redraw, right-click or Del/Bksp to delete, S to save.
// Middle-drag pans; wheel zooms. Tab cycles between three layers:
//   Sprite → Collision → Classification
//
// Sprite layer: Shift+L loads/replaces a spritesheet/tileset PNG. L toggles the sheet
// panel. Drag to select a tile, release to cut it into sprites/, then hide the panel
// with L and click in the world to place.
//
// In the Classification layer:
//   Arrows  navigate objects
//   Enter   edit the focused object's tag
//   I       edit the focused object's ID
//   Tab     (while editing a tag) cycle through existing tags
//   Delete  clear the tag on the focused object

use image::GenericImageView;
use juni::prelude::*;
use std::collections::HashMap;
use std::{
    io::{self, Write},
    path::Path,
    sync::OnceLock,
};

const RENDER_W: u32 = 1280;
const RENDER_H: u32 = 720;
const GRID_SIZE: f32 = 32.0;
const GRID_MAJOR_EVERY: i32 = 4;
const WINDOW_TITLE: &str = "juni — level editor";

/// Color palette for classification tags. Colors are assigned in order as new
/// tags are introduced; the first entry is used for the built-in `"static"`.
const TAG_PALETTE: [Color; 10] = [
    LIGHTGRAY, SKYBLUE, GREEN, ORANGE, PINK, PURPLE, GOLD, RED, BEIGE, BLUE,
];

/// Height of a classification label rectangle in world pixels.
const LABEL_H: f32 = 15.0;
/// Minimum vertical gap between two resolved labels.
const LABEL_GAP: f32 = 2.0;

/// A deferred label for the classification layer. All labels are collected,
/// overlap-resolved, then drawn in a single second pass so no two labels
/// whose x-ranges overlap are rendered on top of each other.
struct LabelSpec {
    x: f32,
    /// Resolved y (top of label rect). Starts as the preferred position
    /// and is pushed downward by [`resolve_label_overlaps`] as needed.
    y: f32,
    w: f32,
    text: String,
    text_color: Color,
    bg_color: Color,
}

// ---------------------------------------------------------------------------
// Helper types
// ---------------------------------------------------------------------------

/// Identifies one specific classifiable object by its position in the level.
/// Indices are into `Level::sprite_instances` / `Level::collision_shapes`.
/// Using a positional reference (not the string ID) lets two objects that
/// happen to share an ID still be navigated / edited independently.
#[derive(Clone, PartialEq, Debug)]
enum ObjectRef {
    Sprite(usize),
    CollisionShape(usize),
}

/// Which field is currently being edited on the focused object.
#[derive(Clone, PartialEq, Debug)]
enum EditMode {
    Tag,
    ObjectId,
}

/// The primitive the left mouse button currently places.
#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Rect,
    Circle,
}

/// What a left-drag is currently doing in a shape layer.
#[derive(Clone, Copy, PartialEq, Debug)]
enum DragAction {
    /// Dragging on empty space to create a brand-new shape.
    NewShape,
    /// Dragging to redefine the geometry of an already-selected shape.
    RedrawShape(usize),
}

/// A loaded spritesheet/tileset image used to cut new sprite tiles.
struct SpriteSheet {
    /// Path the sheet was loaded from.
    path: String,
    /// GPU texture for rendering the sheet in the UI panel.
    texture: Texture,
    /// Original image width in pixels.
    width: u32,
    /// Original image height in pixels.
    height: u32,
}

/// Screen-space geometry of the sheet preview panel.
struct SheetPanel {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Scale from sheet pixels to screen pixels.
    scale: f32,
    /// Offset of the scaled image inside the panel, centered.
    offset_x: f32,
    offset_y: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum Layer {
    SpritePlanning,
    CollisionPlanning,
    ClassificationPlanning,
}

/// Color choices for shapes, selectable with the number keys 1–6.
const PALETTE: [Color; 6] = [RED, ORANGE, GOLD, LIME, SKYBLUE, VIOLET];

// ---------------------------------------------------------------------------
// Startup config (written before `run::<Editor>` then read in `init`)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct StartupConfig {
    path: String,
    level: Level,
    status: String,
}

static STARTUP: OnceLock<StartupConfig> = OnceLock::new();

// ---------------------------------------------------------------------------
// Random ID generation
// ---------------------------------------------------------------------------

/// Generate a short random-looking 6-character lowercase alphanumeric ID.
/// Uses a global atomic counter mixed with sub-second time so IDs remain
/// unique even when called many times in rapid succession.
fn random_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    // Mix with a multiplicative hash so closely-timed calls look different.
    let mut h = nanos.wrapping_add(seq.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1));
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^= h >> 33;
    let charset: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut id = String::with_capacity(6);
    let mut val = h;
    for _ in 0..6 {
        id.push(charset[(val % 36) as usize] as char);
        val /= 36;
    }
    id
}

// ---------------------------------------------------------------------------
// Editor struct
// ---------------------------------------------------------------------------

struct Editor {
    current_path: String,
    level: Level,
    active_layer: Layer,
    tool: Tool,
    color: Color,
    drag_start: Option<Vec2D>,
    drag_action: Option<DragAction>,
    selected_shape: Option<usize>,
    mouse: Vec2D,
    target: Vec2D,
    zoom: f32,
    pan_last: Option<Vec2D>,
    status: String,
    show_help: bool,
    is_dirty: bool,
    // --- Sprite support ---
    available_sprites: Vec<String>,
    selected_sprite: Option<usize>,
    sprite_scale: f32,
    sprite_cache: HashMap<String, Texture>,
    // --- Spritesheet/tileset cutter ---
    /// A loaded spritesheet for cutting tiles.
    sprite_sheet: Option<SpriteSheet>,
    /// Whether the sheet cutter panel is currently visible.
    sheet_panel_open: bool,
    /// Pixel selection inside the current sheet (`None` = no confirmed selection).
    sheet_selection: Option<Rect>,
    /// Drag start in sheet pixel coordinates while the user is selecting a region.
    sheet_drag_start: Option<Vec2D>,
    /// Buffer for typing a spritesheet path.
    sheet_path_input: String,
    /// Whether the user is currently typing a sheet path.
    entering_sheet_path: bool,
    // --- Classification layer ---
    /// tag string → display color
    tag_colors: HashMap<String, Color>,
    /// Which object is keyboard-focused (arrow key selection).
    focused_object: Option<ObjectRef>,
    /// Active text edit: (target object, what we're editing, current buffer).
    /// While `Some`, all printable keys feed the buffer.
    editing_object: Option<(ObjectRef, EditMode, String)>,
}

// ---------------------------------------------------------------------------
// Editor helpers
// ---------------------------------------------------------------------------

impl Editor {
    fn selected_sprite_name(&self) -> &str {
        self.selected_sprite
            .and_then(|i| self.available_sprites.get(i))
            .map(|p| {
                Path::new(p)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or(p.as_str())
            })
            .unwrap_or("none")
    }

    /// Ensure `path` is in `available_sprites` and `sprite_cache`, then select it.
    fn select_sprite_by_path(&mut self, ctx: &mut Context, path: String) {
        if let std::collections::hash_map::Entry::Vacant(e) = self.sprite_cache.entry(path.clone())
        {
            if let Ok(tex) = ctx.load_texture(&path) {
                e.insert(tex);
            }
        }
        if let Some(i) = self.available_sprites.iter().position(|p| p == &path) {
            self.selected_sprite = Some(i);
        } else {
            self.available_sprites.push(path.clone());
            self.available_sprites.sort();
            self.selected_sprite = self.available_sprites.iter().position(|p| p == &path);
        }
    }

    /// Bounding rect `(x, y, w, h)` for a sprite instance.
    fn sprite_bounding_rect(&self, inst: &SpriteInstance) -> (f32, f32, f32, f32) {
        if let Some(tex) = self.sprite_cache.get(&inst.path) {
            (
                inst.x,
                inst.y,
                tex.width() as f32 * inst.scale,
                tex.height() as f32 * inst.scale,
            )
        } else {
            (
                inst.x,
                inst.y,
                GRID_SIZE * inst.scale,
                GRID_SIZE * inst.scale,
            )
        }
    }

    /// String ID of the given object (borrows from the level).
    fn object_id<'a>(&'a self, obj: &ObjectRef) -> &'a str {
        match obj {
            ObjectRef::Sprite(i) => &self.level.sprite_instances[*i].id,
            ObjectRef::CollisionShape(i) => self.level.collision_shapes[*i].id(),
        }
    }

    /// Overwrite the string ID of the given object.
    fn object_set_id(&mut self, obj: &ObjectRef, new_id: String) {
        match obj {
            ObjectRef::Sprite(i) => self.level.sprite_instances[*i].id = new_id,
            ObjectRef::CollisionShape(i) => self.level.collision_shapes[*i].set_id(new_id),
        }
    }

    /// Current tag for the given object (falls back to `"static"`).
    fn object_tag<'a>(&'a self, obj: &ObjectRef) -> &'a str {
        self.level.get_tag(self.object_id(obj)).unwrap_or("static")
    }

    /// All classifiable objects in display order (sprites first, then shapes).
    fn ordered_objects(&self) -> Vec<ObjectRef> {
        let mut refs = Vec::new();
        for i in 0..self.level.sprite_instances.len() {
            refs.push(ObjectRef::Sprite(i));
        }
        for i in 0..self.level.collision_shapes.len() {
            refs.push(ObjectRef::CollisionShape(i));
        }
        refs
    }

    /// Object under the cursor in world space (`None` if nothing there).
    fn classify_object_at(&self, world: Vec2D) -> Option<ObjectRef> {
        for i in (0..self.level.sprite_instances.len()).rev() {
            let inst = &self.level.sprite_instances[i];
            let (bx, by, bw, bh) = self.sprite_bounding_rect(inst);
            if world.x >= bx && world.x <= bx + bw && world.y >= by && world.y <= by + bh {
                return Some(ObjectRef::Sprite(i));
            }
        }
        for i in (0..self.level.collision_shapes.len()).rev() {
            if self.level.collision_shapes[i].contains(world) {
                return Some(ObjectRef::CollisionShape(i));
            }
        }
        None
    }

    /// Topmost sprite instance under `p` in world space (returns list index).
    fn sprite_at(&self, p: Vec2D) -> Option<usize> {
        self.level.sprite_instances.iter().rposition(|s| {
            if let Some(tex) = self.sprite_cache.get(&s.path) {
                let w = tex.width() as f32 * s.scale;
                let h = tex.height() as f32 * s.scale;
                p.x >= s.x && p.x <= s.x + w && p.y >= s.y && p.y <= s.y + h
            } else {
                false
            }
        })
    }

    fn active_layer_name(&self) -> &'static str {
        match self.active_layer {
            Layer::SpritePlanning => "Sprite",
            Layer::CollisionPlanning => "Collision",
            Layer::ClassificationPlanning => "Classification",
        }
    }

    fn default_layer_color(layer: Layer) -> Color {
        match layer {
            Layer::SpritePlanning => BLUE,
            Layer::CollisionPlanning => RED,
            Layer::ClassificationPlanning => PURPLE,
        }
    }

    fn active_shapes(&self) -> &[Shape] {
        match self.active_layer {
            Layer::SpritePlanning => &self.level.sprite_shapes,
            Layer::CollisionPlanning => &self.level.collision_shapes,
            Layer::ClassificationPlanning => &[],
        }
    }

    fn active_shapes_mut(&mut self) -> &mut Vec<Shape> {
        match self.active_layer {
            Layer::SpritePlanning => &mut self.level.sprite_shapes,
            Layer::CollisionPlanning => &mut self.level.collision_shapes,
            Layer::ClassificationPlanning => &mut self.level.sprite_shapes,
        }
    }

    fn inactive_shapes(&self) -> &[Shape] {
        match self.active_layer {
            Layer::SpritePlanning => &self.level.collision_shapes,
            Layer::CollisionPlanning => &self.level.sprite_shapes,
            Layer::ClassificationPlanning => &[],
        }
    }

    fn draw_shape_layer(&self, canvas: &mut Canvas, shapes: &[Shape], alpha: f32) {
        for shape in shapes {
            shape.with_alpha(alpha).draw(canvas);
        }
    }

    fn camera(&self) -> Camera2D {
        Camera2D {
            offset: Vec2D::ZERO,
            target: self.target,
            rotation: 0.0,
            zoom: self.zoom,
        }
    }

    fn mouse_world(&self) -> Vec2D {
        self.camera().screen_to_world(self.mouse)
    }

    fn snap_world(&self, world: Vec2D) -> Vec2D {
        Vec2D::new(
            (world.x / GRID_SIZE).round() * GRID_SIZE,
            (world.y / GRID_SIZE).round() * GRID_SIZE,
        )
    }

    fn current_file_label(&self) -> &str {
        Path::new(&self.current_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.current_path)
    }

    fn window_title(&self) -> String {
        let dirty = if self.is_dirty { " *" } else { "" };
        format!(
            "{WINDOW_TITLE} — {} — {}{}",
            self.current_path,
            self.active_layer_name(),
            dirty
        )
    }

    fn refresh_window_title(&self, ctx: &mut Context) {
        ctx.set_window_title(&self.window_title());
    }

    fn draw_grid(&self, canvas: &mut Canvas) {
        let camera = self.camera();
        let top_left = camera.screen_to_world(Vec2D::ZERO);
        let bottom_right = camera.screen_to_world(Vec2D::new(RENDER_W as f32, RENDER_H as f32));

        let min_x = (top_left.x / GRID_SIZE).floor() as i32;
        let max_x = (bottom_right.x / GRID_SIZE).ceil() as i32;
        let min_y = (top_left.y / GRID_SIZE).floor() as i32;
        let max_y = (bottom_right.y / GRID_SIZE).ceil() as i32;

        for ix in min_x..=max_x {
            let x = ix as f32 * GRID_SIZE;
            let c = if ix.rem_euclid(GRID_MAJOR_EVERY) == 0 {
                LIGHTGRAY.with_alpha(0.30)
            } else {
                LIGHTGRAY.with_alpha(0.12)
            };
            canvas.line(
                Vec2D::new(x, top_left.y),
                Vec2D::new(x, bottom_right.y),
                1.0,
                c,
            );
        }
        for iy in min_y..=max_y {
            let y = iy as f32 * GRID_SIZE;
            let c = if iy.rem_euclid(GRID_MAJOR_EVERY) == 0 {
                LIGHTGRAY.with_alpha(0.30)
            } else {
                LIGHTGRAY.with_alpha(0.12)
            };
            canvas.line(
                Vec2D::new(top_left.x, y),
                Vec2D::new(bottom_right.x, y),
                1.0,
                c,
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Free functions
// ---------------------------------------------------------------------------

/// Update an existing shape's geometry from two drag endpoints while
/// preserving its ID and color. The shape keeps its original type.
fn update_shape_geometry(shape: &mut Shape, a: Vec2D, b: Vec2D) -> bool {
    match shape {
        Shape::Rect {
            x,
            y,
            width,
            height,
            ..
        } => {
            let nx = a.x.min(b.x);
            let ny = a.y.min(b.y);
            let nw = (a.x - b.x).abs();
            let nh = (a.y - b.y).abs();
            if nw >= 2.0 && nh >= 2.0 {
                *x = nx;
                *y = ny;
                *width = nw;
                *height = nh;
                true
            } else {
                false
            }
        }
        Shape::Circle { x, y, radius, .. } => {
            let nr = a.distance(b);
            if nr >= 2.0 {
                *x = a.x;
                *y = a.y;
                *radius = nr;
                true
            } else {
                false
            }
        }
    }
}

/// Build a shape from two drag endpoints, assigning a fresh random ID.
fn make_shape(tool: Tool, a: Vec2D, b: Vec2D, color: Color) -> Option<Shape> {
    match tool {
        Tool::Rect => {
            let x = a.x.min(b.x);
            let y = a.y.min(b.y);
            let width = (a.x - b.x).abs();
            let height = (a.y - b.y).abs();
            (width >= 2.0 && height >= 2.0).then_some(Shape::Rect {
                id: random_id(),
                x,
                y,
                width,
                height,
                color,
            })
        }
        Tool::Circle => {
            let radius = a.distance(b);
            (radius >= 2.0).then_some(Shape::Circle {
                id: random_id(),
                x: a.x,
                y: a.y,
                radius,
                color,
            })
        }
    }
}

/// Draw a rectangle outline using four line segments.
fn draw_rect_outline(canvas: &mut Canvas, x: f32, y: f32, w: f32, h: f32, t: f32, c: Color) {
    canvas.line(Vec2D::new(x, y), Vec2D::new(x + w, y), t, c);
    canvas.line(Vec2D::new(x + w, y), Vec2D::new(x + w, y + h), t, c);
    canvas.line(Vec2D::new(x + w, y + h), Vec2D::new(x, y + h), t, c);
    canvas.line(Vec2D::new(x, y + h), Vec2D::new(x, y), t, c);
}

/// Collect characters suitable for typing a file path: letters, digits, and
/// common path separators/punctuation. Letters are lowercase unless `shift`.
fn collect_path_chars(ctx: &Context, shift: bool) -> String {
    let mut s = String::new();
    for &(key, lo, hi) in &[
        (Key::A, 'a', 'A'),
        (Key::B, 'b', 'B'),
        (Key::C, 'c', 'C'),
        (Key::D, 'd', 'D'),
        (Key::E, 'e', 'E'),
        (Key::F, 'f', 'F'),
        (Key::G, 'g', 'G'),
        (Key::H, 'h', 'H'),
        (Key::I, 'i', 'I'),
        (Key::J, 'j', 'J'),
        (Key::K, 'k', 'K'),
        (Key::L, 'l', 'L'),
        (Key::M, 'm', 'M'),
        (Key::N, 'n', 'N'),
        (Key::O, 'o', 'O'),
        (Key::P, 'p', 'P'),
        (Key::Q, 'q', 'Q'),
        (Key::R, 'r', 'R'),
        (Key::S, 's', 'S'),
        (Key::T, 't', 'T'),
        (Key::U, 'u', 'U'),
        (Key::V, 'v', 'V'),
        (Key::W, 'w', 'W'),
        (Key::X, 'x', 'X'),
        (Key::Y, 'y', 'Y'),
        (Key::Z, 'z', 'Z'),
        (Key::Num0, '0', ')'),
        (Key::Num1, '1', '!'),
        (Key::Num2, '2', '@'),
        (Key::Num3, '3', '#'),
        (Key::Num4, '4', '$'),
        (Key::Num5, '5', '%'),
        (Key::Num6, '6', '^'),
        (Key::Num7, '7', '&'),
        (Key::Num8, '8', '*'),
        (Key::Num9, '9', '('),
        (Key::Space, ' ', ' '),
        (Key::Minus, '-', '_'),
        (Key::Period, '.', '>'),
        (Key::Comma, ',', '<'),
        (Key::Slash, '/', '?'),
        (Key::Backslash, '\\', '|'),
        (Key::Semicolon, ';', ':'),
        (Key::Apostrophe, '\'', '"'),
        (Key::Equal, '=', '+'),
        (Key::BracketLeft, '[', '{'),
        (Key::BracketRight, ']', '}'),
        (Key::Grave, '`', '~'),
    ] {
        if ctx.is_key_pressed(key) {
            s.push(if shift { hi } else { lo });
        }
    }
    s
}

/// Compute the screen-space panel for a loaded spritesheet preview.
/// Uses a large centered modal so big spritesheets/tilesets remain readable.
fn sheet_panel_rect(sheet_w: u32, sheet_h: u32) -> SheetPanel {
    const MAX_W: f32 = 1000.0;
    const MAX_H: f32 = 600.0;
    const PADDING: f32 = 40.0;
    let x = (RENDER_W as f32 - MAX_W) * 0.5;
    let y = (RENDER_H as f32 - MAX_H) * 0.5;
    let avail_w = MAX_W - 2.0 * PADDING;
    let avail_h = MAX_H - 2.0 * PADDING;
    let scale = (avail_w / sheet_w as f32).min(avail_h / sheet_h as f32);
    let img_w = sheet_w as f32 * scale;
    let img_h = sheet_h as f32 * scale;
    let offset_x = (MAX_W - img_w) * 0.5;
    let offset_y = (MAX_H - img_h) * 0.5;
    SheetPanel {
        x,
        y,
        w: MAX_W,
        h: MAX_H,
        scale,
        offset_x,
        offset_y,
    }
}

/// Convert a screen-space mouse position into sheet pixel coordinates relative
/// to the panel. Returns `None` if the mouse is outside the scaled image.
fn sheet_mouse_pos(panel: &SheetPanel, mouse: Vec2D) -> Option<Vec2D> {
    let local_x = mouse.x - panel.x - panel.offset_x;
    let local_y = mouse.y - panel.y - panel.offset_y;
    if local_x < 0.0 || local_y < 0.0 {
        return None;
    }
    let sx = local_x / panel.scale;
    let sy = local_y / panel.scale;
    Some(Vec2D::new(sx, sy))
}

/// Convert a sheet-pixel rectangle to screen-space coordinates inside the panel.
fn sheet_rect_to_panel(panel: &SheetPanel, rect: Rect) -> Rect {
    Rect::new(
        panel.x + panel.offset_x + rect.x * panel.scale,
        panel.y + panel.offset_y + rect.y * panel.scale,
        rect.width * panel.scale,
        rect.height * panel.scale,
    )
}

/// Build a normalized rectangle from two corner points.
fn rect_from_points(a: Vec2D, b: Vec2D) -> Rect {
    let x = a.x.min(b.x);
    let y = a.y.min(b.y);
    let width = (a.x - b.x).abs();
    let height = (a.y - b.y).abs();
    Rect::new(x, y, width, height)
}

/// Clamp a rectangle to lie within `(0,0..max_w,max_h)` and have positive size.
fn clamp_rect(rect: Rect, max_w: u32, max_h: u32) -> Rect {
    let x = rect.x.clamp(0.0, max_w as f32);
    let y = rect.y.clamp(0.0, max_h as f32);
    let width = (rect.width.min(max_w as f32 - x)).max(1.0);
    let height = (rect.height.min(max_h as f32 - y)).max(1.0);
    Rect::new(x, y, width, height)
}

/// Crop the selected region from `sheet_path` and save it as a new PNG in
/// `sprites_dir`. Returns the path of the newly-created sprite file.
fn crop_and_save_sprite(
    sheet_path: &str,
    selection: Rect,
    sprites_dir: &str,
) -> std::io::Result<String> {
    use image::GenericImageView;

    let img = image::open(sheet_path)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let x = selection.x as u32;
    let y = selection.y as u32;
    let w = selection.width as u32;
    let h = selection.height as u32;
    let cropped = img.view(x, y, w, h).to_image();

    let base = Path::new(sheet_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("sheet");
    std::fs::create_dir_all(sprites_dir)?;

    let mut counter = 0;
    let out_path = loop {
        let name = if counter == 0 {
            format!("{base}_{x}_{y}_{w}_{h}.png")
        } else {
            format!("{base}_{x}_{y}_{w}_{h}_{counter}.png")
        };
        let full = Path::new(sprites_dir).join(&name);
        if !full.exists() {
            break full;
        }
        counter += 1;
    };

    cropped.save(&out_path).map_err(std::io::Error::other)?;

    Ok(out_path.to_string_lossy().replace('\\', "/"))
}

/// Collect all printable characters pressed this frame (letters, digits, space,
/// comma, period). Letters are lowercase unless `shift` is true.
fn collect_typed_chars(ctx: &Context, shift: bool) -> String {
    let mut s = String::new();
    for &(key, lo, hi) in &[
        (Key::A, 'a', 'A'),
        (Key::B, 'b', 'B'),
        (Key::C, 'c', 'C'),
        (Key::D, 'd', 'D'),
        (Key::E, 'e', 'E'),
        (Key::F, 'f', 'F'),
        (Key::G, 'g', 'G'),
        (Key::H, 'h', 'H'),
        (Key::I, 'i', 'I'),
        (Key::J, 'j', 'J'),
        (Key::K, 'k', 'K'),
        (Key::L, 'l', 'L'),
        (Key::M, 'm', 'M'),
        (Key::N, 'n', 'N'),
        (Key::O, 'o', 'O'),
        (Key::P, 'p', 'P'),
        (Key::Q, 'q', 'Q'),
        (Key::R, 'r', 'R'),
        (Key::S, 's', 'S'),
        (Key::T, 't', 'T'),
        (Key::U, 'u', 'U'),
        (Key::V, 'v', 'V'),
        (Key::W, 'w', 'W'),
        (Key::X, 'x', 'X'),
        (Key::Y, 'y', 'Y'),
        (Key::Z, 'z', 'Z'),
        (Key::Num0, '0', '0'),
        (Key::Num1, '1', '1'),
        (Key::Num2, '2', '2'),
        (Key::Num3, '3', '3'),
        (Key::Num4, '4', '4'),
        (Key::Num5, '5', '5'),
        (Key::Num6, '6', '6'),
        (Key::Num7, '7', '7'),
        (Key::Num8, '8', '8'),
        (Key::Num9, '9', '9'),
        (Key::Space, ' ', ' '),
        (Key::Comma, ',', '<'),
        (Key::Period, '.', '>'),
    ] {
        if ctx.is_key_pressed(key) {
            s.push(if shift { hi } else { lo });
        }
    }
    s
}

/// Build `tag_colors` from level data, ensuring `"static"` always gets the
/// first palette slot.
fn build_tag_colors(level: &Level) -> HashMap<String, Color> {
    let mut map = HashMap::new();
    map.insert("static".to_string(), TAG_PALETTE[0]);
    for entry in &level.classifications {
        if !map.contains_key(&entry.tag) {
            let idx = map.len() % TAG_PALETTE.len();
            map.insert(entry.tag.clone(), TAG_PALETTE[idx]);
        }
    }
    map
}

// ---------------------------------------------------------------------------
// Startup helpers
// ---------------------------------------------------------------------------

fn prompt_level_path() -> io::Result<String> {
    loop {
        print!("Level path: ");
        io::stdout().flush()?;
        let mut line = String::new();
        io::stdin().read_line(&mut line)?;
        let path = line.trim();
        if !path.is_empty() {
            return Ok(path.to_string());
        }
        eprintln!("A level path is required.");
    }
}

fn scan_sprites(dir: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<String> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        })
        .map(|e| e.path().to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    paths
}

fn load_or_create_level(path: &str) -> io::Result<StartupConfig> {
    if Path::new(path).exists() {
        let mut level = Level::load(path)?;
        level.ensure_ids(random_id);
        let sprite_n = level.sprite_instances.len();
        let collision_n = level.collision_shapes.len();
        let class_n = level.classifications.len();
        Ok(StartupConfig {
            path: path.to_string(),
            level,
            status: format!(
                "Loaded {path} ({sprite_n} sprites, {collision_n} collision, {class_n} tags)"
            ),
        })
    } else {
        if let Some(parent) = Path::new(path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let level = Level::new();
        level.save(path)?;
        Ok(StartupConfig {
            path: path.to_string(),
            level,
            status: format!("Created {path} (new level)"),
        })
    }
}

// ---------------------------------------------------------------------------
// Game trait impl
// ---------------------------------------------------------------------------

impl Game for Editor {
    fn init(ctx: &mut Context) -> Self {
        let startup = STARTUP
            .get()
            .expect("startup config must be set before run()");

        let available_sprites = scan_sprites("sprites");
        let selected_sprite = if available_sprites.is_empty() {
            None
        } else {
            Some(0)
        };

        let mut sprite_cache = HashMap::new();
        for inst in &startup.level.sprite_instances {
            if !sprite_cache.contains_key(&inst.path) {
                if let Ok(tex) = ctx.load_texture(&inst.path) {
                    sprite_cache.insert(inst.path.clone(), tex);
                }
            }
        }
        if let Some(path) = selected_sprite.and_then(|i| available_sprites.get(i)) {
            if !sprite_cache.contains_key(path) {
                if let Ok(tex) = ctx.load_texture(path) {
                    sprite_cache.insert(path.clone(), tex);
                }
            }
        }

        let tag_colors = build_tag_colors(&startup.level);

        let editor = Self {
            current_path: startup.path.clone(),
            level: startup.level.clone(),
            active_layer: Layer::SpritePlanning,
            tool: Tool::Rect,
            color: Self::default_layer_color(Layer::SpritePlanning),
            drag_start: None,
            drag_action: None,
            selected_shape: None,
            mouse: Vec2D::ZERO,
            target: Vec2D::ZERO,
            zoom: 1.0,
            pan_last: None,
            status: startup.status.clone(),
            show_help: false,
            is_dirty: false,
            available_sprites,
            selected_sprite,
            sprite_scale: 1.0,
            sprite_cache,
            sprite_sheet: None,
            sheet_panel_open: false,
            sheet_selection: None,
            sheet_drag_start: None,
            sheet_path_input: String::new(),
            entering_sheet_path: false,
            tag_colors,
            focused_object: None,
            editing_object: None,
        };
        editor.refresh_window_title(ctx);
        editor
    }

    fn update(&mut self, ctx: &mut Context) {
        self.mouse = ctx.mouse_position();

        // ----------------------------------------------------------------
        // ESC: cancel active edit OR quit
        // ----------------------------------------------------------------
        if ctx.is_key_pressed(Key::Escape) {
            if self.editing_object.is_some() {
                self.editing_object = None;
                self.status = "Edit cancelled".to_string();
                return;
            } else if self.selected_shape.take().is_some() {
                self.drag_start = None;
                self.drag_action = None;
                self.status = "Deselected shape".to_string();
                return;
            } else {
                ctx.exit();
            }
        }

        let is_editing = self.editing_object.is_some();

        // ----------------------------------------------------------------
        // Global shortcuts (suppressed while editing a tag/ID)
        // ----------------------------------------------------------------
        if !is_editing {
            if ctx.is_key_pressed(Key::H) {
                self.show_help = !self.show_help;
                self.status = if self.show_help {
                    "Help shown".into()
                } else {
                    "Help hidden".into()
                };
            }
            if ctx.is_key_pressed(Key::Tab) {
                self.active_layer = match self.active_layer {
                    Layer::SpritePlanning => Layer::CollisionPlanning,
                    Layer::CollisionPlanning => Layer::ClassificationPlanning,
                    Layer::ClassificationPlanning => Layer::SpritePlanning,
                };
                self.color = Self::default_layer_color(self.active_layer);
                self.drag_start = None;
                self.drag_action = None;
                self.selected_shape = None;
                self.focused_object = None;
                self.refresh_window_title(ctx);
                self.status = format!("Active layer: {}", self.active_layer_name());
            }
        }

        // ----------------------------------------------------------------
        // Camera (always active)
        // ----------------------------------------------------------------
        if ctx.is_mouse_button_down(MouseButton::Middle) {
            if let Some(prev) = self.pan_last {
                self.target -= (self.mouse - prev) / self.zoom;
            }
            self.pan_last = Some(self.mouse);
        } else {
            self.pan_last = None;
        }
        let wheel = ctx.mouse_wheel_move();
        if wheel != 0.0 {
            let before = self.mouse_world();
            self.zoom = (self.zoom * (1.0 + wheel * 0.1)).clamp(0.1, 10.0);
            self.target += before - self.mouse_world();
        }
        if !is_editing && ctx.is_key_pressed(Key::F) {
            self.target = Vec2D::ZERO;
            self.zoom = 1.0;
            self.status = "View reset".to_string();
        }

        let world = self.mouse_world();
        let snapped_world = self.snap_world(world);

        // ================================================================
        // CLASSIFICATION LAYER: text editing
        // ================================================================
        if self.active_layer == Layer::ClassificationPlanning && is_editing {
            // Consume the current edit state so we can work with owned values,
            // then put it back if the edit is still in progress.
            if let Some((obj, mode, mut buf)) = self.editing_object.take() {
                let shift = ctx.is_key_down(Key::LeftShift) || ctx.is_key_down(Key::RightShift);
                let should_confirm = ctx.is_key_pressed(Key::Enter);
                let should_backspace = ctx.is_key_pressed(Key::Backspace);
                let should_tab = ctx.is_key_pressed(Key::Tab);
                let typed = collect_typed_chars(ctx, shift);

                if should_confirm {
                    // ---- Confirm the edit ----
                    match mode {
                        EditMode::Tag => {
                            let tag = {
                                let t = buf.trim().to_string();
                                if t.is_empty() {
                                    "static".to_string()
                                } else {
                                    t
                                }
                            };
                            let obj_id = self.object_id(&obj).to_string();
                            if let Some(entry) = self
                                .level
                                .classifications
                                .iter_mut()
                                .find(|e| e.object_id == obj_id)
                            {
                                entry.tag = tag.clone();
                            } else {
                                self.level.classifications.push(ClassificationEntry {
                                    object_id: obj_id,
                                    tag: tag.clone(),
                                });
                            }
                            if !self.tag_colors.contains_key(&tag) {
                                let idx = self.tag_colors.len() % TAG_PALETTE.len();
                                self.tag_colors.insert(tag.clone(), TAG_PALETTE[idx]);
                            }
                            self.is_dirty = true;
                            self.refresh_window_title(ctx);
                            self.status = format!("Tagged as '{tag}'");
                        }
                        EditMode::ObjectId => {
                            let new_id = {
                                let t = buf.trim().to_string();
                                if t.is_empty() {
                                    random_id()
                                } else {
                                    t
                                }
                            };
                            self.object_set_id(&obj, new_id.clone());
                            self.is_dirty = true;
                            self.refresh_window_title(ctx);
                            self.status = format!("ID set to '{new_id}'");
                        }
                    }
                    // editing_object stays None (taken above)
                    return;
                }

                // ---- Still editing: mutate the buffer ----
                if should_backspace {
                    buf.pop();
                }

                // Tab cycles through known tags (only in Tag mode)
                if should_tab && mode == EditMode::Tag {
                    let mut sorted_tags: Vec<String> = self.tag_colors.keys().cloned().collect();
                    sorted_tags.sort();
                    if !sorted_tags.is_empty() {
                        let cur = sorted_tags.iter().position(|t| t == &buf);
                        let next = if shift {
                            match cur {
                                None | Some(0) => sorted_tags.len() - 1,
                                Some(i) => i - 1,
                            }
                        } else {
                            match cur {
                                None => 0,
                                Some(i) => (i + 1) % sorted_tags.len(),
                            }
                        };
                        buf = sorted_tags[next].clone();
                    }
                }

                if !typed.is_empty() {
                    buf.push_str(&typed);
                }

                // Put the (mutated) edit state back.
                self.editing_object = Some((obj, mode, buf));
            }
            return; // Swallow all other keys while editing.
        }

        // ================================================================
        // CLASSIFICATION LAYER: navigation and selection
        // ================================================================
        if self.active_layer == Layer::ClassificationPlanning {
            let objs = self.ordered_objects();

            // ---- Arrow key navigation ----
            if !objs.is_empty() {
                let cur_idx = self
                    .focused_object
                    .as_ref()
                    .and_then(|fo| objs.iter().position(|o| o == fo));
                let nav_fwd = ctx.is_key_pressed(Key::Right) || ctx.is_key_pressed(Key::Down);
                let nav_bwd = ctx.is_key_pressed(Key::Left) || ctx.is_key_pressed(Key::Up);

                if nav_fwd || nav_bwd {
                    let next_idx = if nav_fwd {
                        match cur_idx {
                            None => 0,
                            Some(i) => (i + 1) % objs.len(),
                        }
                    } else {
                        match cur_idx {
                            None | Some(0) => objs.len() - 1,
                            Some(i) => i - 1,
                        }
                    };
                    let next_obj = objs[next_idx].clone();
                    let id = self.object_id(&next_obj).to_string();
                    let tag = self.level.get_tag(&id).unwrap_or("static").to_string();
                    self.focused_object = Some(next_obj);
                    self.status = format!("{} | {} ({}/{})", id, tag, next_idx + 1, objs.len());
                }
            }

            // ---- Enter: start editing the tag ----
            if ctx.is_key_pressed(Key::Enter) {
                if let Some(obj) = self.focused_object.clone() {
                    let tag = self.object_tag(&obj).to_string();
                    self.editing_object = Some((obj, EditMode::Tag, tag));
                    self.status =
                        "Editing tag — type, Tab=cycle existing, Enter=confirm, Esc=cancel"
                            .to_string();
                } else {
                    self.status = "Use arrows to select an object first".to_string();
                }
            }

            // ---- I: start editing the ID ----
            if ctx.is_key_pressed(Key::I) {
                if let Some(obj) = self.focused_object.clone() {
                    let id = self.object_id(&obj).to_string();
                    self.editing_object = Some((obj, EditMode::ObjectId, id));
                    self.status = "Editing ID — type new ID, Enter=confirm, Esc=cancel".to_string();
                } else {
                    self.status = "Use arrows to select an object first".to_string();
                }
            }

            // ---- Delete: clear the tag ----
            if ctx.is_key_pressed(Key::Delete) {
                if let Some(obj) = self.focused_object.as_ref() {
                    let id = self.object_id(obj).to_string();
                    self.level.classifications.retain(|e| e.object_id != id);
                    self.is_dirty = true;
                    self.refresh_window_title(ctx);
                    self.status = format!("Cleared tag for '{id}'");
                }
            }

            // ---- Mouse: click to focus + immediately start tag edit ----
            if ctx.is_mouse_button_pressed(MouseButton::Left) {
                if let Some(obj) = self.classify_object_at(world) {
                    let tag = self.object_tag(&obj).to_string();
                    self.focused_object = Some(obj.clone());
                    self.editing_object = Some((obj, EditMode::Tag, tag));
                    self.status = "Editing tag — Tab=cycle, Enter=confirm, Esc=cancel".to_string();
                } else {
                    self.status = "No object here — use arrows or click an object".to_string();
                }
            }

            // ---- R-click: clear tag of object under cursor ----
            if ctx.is_mouse_button_pressed(MouseButton::Right) {
                if let Some(obj) = self.classify_object_at(world) {
                    let id = self.object_id(&obj).to_string();
                    self.focused_object = Some(obj);
                    self.level.classifications.retain(|e| e.object_id != id);
                    self.is_dirty = true;
                    self.refresh_window_title(ctx);
                    self.status = format!("Cleared tag for '{id}'");
                }
            }

            // Z / X work on classifications in this layer
            if ctx.is_key_pressed(Key::Z) && self.level.classifications.pop().is_some() {
                let n = self.level.classifications.len();
                self.is_dirty = true;
                self.refresh_window_title(ctx);
                self.status = format!("Undid last tag ({n} left)");
            }
            if ctx.is_key_pressed(Key::X) && !self.level.classifications.is_empty() {
                self.level.classifications.clear();
                self.is_dirty = true;
                self.refresh_window_title(ctx);
                self.status = "Cleared all classifications".to_string();
            }
        }

        // ================================================================
        // SPRITE / COLLISION LAYERS: tools, picker, placement
        // ================================================================

        // --- Spritesheet / tileset cutter (sprite layer only) ---
        if self.active_layer == Layer::SpritePlanning {
            // Path input mode: type a PNG path, Enter loads it, Esc cancels.
            if self.entering_sheet_path {
                let shift = ctx.is_key_down(Key::LeftShift) || ctx.is_key_down(Key::RightShift);
                let typed = collect_path_chars(ctx, shift);
                if !typed.is_empty() {
                    self.sheet_path_input.push_str(&typed);
                }
                if ctx.is_key_pressed(Key::Backspace) {
                    self.sheet_path_input.pop();
                }
                if ctx.is_key_pressed(Key::Escape) {
                    self.entering_sheet_path = false;
                    self.sheet_path_input.clear();
                    self.status = "Sheet load cancelled".to_string();
                }
                if ctx.is_key_pressed(Key::Enter) {
                    let path = self.sheet_path_input.trim().to_string();
                    self.entering_sheet_path = false;
                    self.sheet_path_input.clear();
                    if path.is_empty() {
                        self.status = "Sheet load cancelled".to_string();
                    } else {
                        match std::fs::read(&path) {
                            Ok(bytes) => {
                                let texture = ctx.load_texture_from_memory(&bytes);
                                match image::load_from_memory(&bytes) {
                                    Ok(img) => {
                                        let (w, h) = img.dimensions();
                                        self.sprite_sheet = Some(SpriteSheet {
                                            path,
                                            texture,
                                            width: w,
                                            height: h,
                                        });
                                        self.sheet_panel_open = true;
                                        self.sheet_selection = None;
                                        self.sheet_drag_start = None;
                                        self.status =
                                            format!("Loaded sheet {w}x{h}; drag to cut a tile");
                                    }
                                    Err(e) => {
                                        self.status = format!("Failed to decode sheet: {e}");
                                    }
                                }
                            }
                            Err(e) => {
                                self.status = format!("Failed to read sheet: {e}");
                            }
                        }
                    }
                }
                // Swallow all other input while typing a sheet path.
                return;
            }

            // L toggles the loaded sheet panel. Shift+L always loads/replaces.
            let shift = ctx.is_key_down(Key::LeftShift) || ctx.is_key_down(Key::RightShift);
            if ctx.is_key_pressed(Key::L) {
                if shift {
                    self.entering_sheet_path = true;
                    self.sheet_path_input.clear();
                    self.status = "Type sheet path, Enter=load, Esc=cancel".to_string();
                } else if let Some(sheet) = &self.sprite_sheet {
                    self.sheet_panel_open = !self.sheet_panel_open;
                    self.status = if self.sheet_panel_open {
                        format!(
                            "Sheet shown ({}x{}); drag to cut a tile",
                            sheet.width, sheet.height
                        )
                    } else {
                        "Sheet hidden".to_string()
                    };
                } else {
                    self.entering_sheet_path = true;
                    self.sheet_path_input.clear();
                    self.status = "Type sheet path, Enter=load, Esc=cancel".to_string();
                }
            }

            // Sheet panel interaction (only when panel is visible).
            if self.sheet_panel_open {
                let mut unload_sheet = false;
                if let Some(sheet) = &self.sprite_sheet {
                    let panel = sheet_panel_rect(sheet.width, sheet.height);

                    // Cancel selection/drag with Esc, or hide panel on second Esc.
                    if ctx.is_key_pressed(Key::Escape) {
                        if self.sheet_drag_start.take().is_some()
                            || self.sheet_selection.take().is_some()
                        {
                            self.status = "Sheet selection cancelled".to_string();
                        } else {
                            self.sheet_panel_open = false;
                            self.status = "Sheet hidden".to_string();
                        }
                    }

                    // Delete unloads the current sheet from the editor.
                    if ctx.is_key_pressed(Key::Delete) {
                        unload_sheet = true;
                        self.status = "Sheet unloaded".to_string();
                    }

                    // Mouse interaction only when cursor is inside the scaled image.
                    if let Some(sheet_pos) = sheet_mouse_pos(&panel, self.mouse) {
                        let current = Vec2D::new(
                            sheet_pos.x.clamp(0.0, sheet.width as f32),
                            sheet_pos.y.clamp(0.0, sheet.height as f32),
                        );

                        if ctx.is_mouse_button_pressed(MouseButton::Left) {
                            self.sheet_drag_start = Some(current);
                            self.sheet_selection = None;
                        }

                        if ctx.is_mouse_button_released(MouseButton::Left) {
                            if let Some(start) = self.sheet_drag_start.take() {
                                let selection = clamp_rect(
                                    rect_from_points(start, current),
                                    sheet.width,
                                    sheet.height,
                                );
                                if selection.width >= 1.0 && selection.height >= 1.0 {
                                    match crop_and_save_sprite(&sheet.path, selection, "sprites") {
                                        Ok(out_path) => {
                                            self.select_sprite_by_path(ctx, out_path);
                                            self.sheet_selection = None;
                                            self.status = format!(
                                                "Cut '{}'; hide sheet (L) to place",
                                                self.selected_sprite_name()
                                            );
                                            self.is_dirty = true;
                                            self.refresh_window_title(ctx);
                                        }
                                        Err(e) => {
                                            self.status = format!("Cut failed: {e}");
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                if unload_sheet {
                    self.sprite_sheet = None;
                    self.sheet_panel_open = false;
                    self.sheet_selection = None;
                    self.sheet_drag_start = None;
                }
            }
        }

        let sheet_open = self.active_layer == Layer::SpritePlanning && self.sheet_panel_open;
        let placing_sprite = self.active_layer == Layer::SpritePlanning
            && self.selected_sprite.is_some()
            && !sheet_open;

        if !placing_sprite && self.active_layer != Layer::ClassificationPlanning {
            if ctx.is_key_pressed(Key::R) {
                self.tool = Tool::Rect;
            }
            if ctx.is_key_pressed(Key::C) {
                self.tool = Tool::Circle;
            }
            for (i, key) in [
                Key::Num1,
                Key::Num2,
                Key::Num3,
                Key::Num4,
                Key::Num5,
                Key::Num6,
            ]
            .iter()
            .enumerate()
            {
                if ctx.is_key_pressed(*key) {
                    self.color = PALETTE[i];
                }
            }
        }

        if self.active_layer == Layer::SpritePlanning && !self.available_sprites.is_empty() {
            let n = self.available_sprites.len();
            let cur = self.selected_sprite.unwrap_or(0);
            if ctx.is_key_pressed(Key::BracketLeft) {
                let next = if cur == 0 { n - 1 } else { cur - 1 };
                self.selected_sprite = Some(next);
                let path = self.available_sprites[next].clone();
                if !self.sprite_cache.contains_key(&path) {
                    if let Ok(tex) = ctx.load_texture(&path) {
                        self.sprite_cache.insert(path.clone(), tex);
                    }
                }
                self.status = format!("Sprite: {}", self.selected_sprite_name());
            }
            if ctx.is_key_pressed(Key::BracketRight) {
                let next = (cur + 1) % n;
                self.selected_sprite = Some(next);
                let path = self.available_sprites[next].clone();
                if !self.sprite_cache.contains_key(&path) {
                    if let Ok(tex) = ctx.load_texture(&path) {
                        self.sprite_cache.insert(path.clone(), tex);
                    }
                }
                self.status = format!("Sprite: {}", self.selected_sprite_name());
            }
            if ctx.is_key_pressed(Key::Comma) {
                self.sprite_scale = (self.sprite_scale / 2.0).max(0.25);
                self.status = format!("Scale: {:.2}x", self.sprite_scale);
            }
            if ctx.is_key_pressed(Key::Period) {
                self.sprite_scale = (self.sprite_scale * 2.0).min(8.0);
                self.status = format!("Scale: {:.2}x", self.sprite_scale);
            }
        }

        // Left button: place
        if placing_sprite {
            if ctx.is_mouse_button_pressed(MouseButton::Left) {
                if let Some(idx) = self.selected_sprite {
                    let path = self.available_sprites[idx].clone();
                    if !self.sprite_cache.contains_key(&path) {
                        match ctx.load_texture(&path) {
                            Ok(tex) => {
                                self.sprite_cache.insert(path.clone(), tex);
                            }
                            Err(e) => {
                                self.status = format!("Failed to load sprite: {e}");
                                return;
                            }
                        }
                    }
                    self.level.sprite_instances.push(SpriteInstance {
                        id: random_id(),
                        path,
                        x: snapped_world.x,
                        y: snapped_world.y,
                        scale: self.sprite_scale,
                    });
                    let count = self.level.sprite_instances.len();
                    self.is_dirty = true;
                    self.refresh_window_title(ctx);
                    self.status = format!("Placed sprite ({count} total)");
                }
            }
        } else if self.active_layer != Layer::ClassificationPlanning && !sheet_open {
            if ctx.is_mouse_button_pressed(MouseButton::Left) {
                let active_shapes = self.active_shapes();
                if let Some(i) = active_shapes.iter().rposition(|s| s.contains(world)) {
                    self.selected_shape = Some(i);
                    self.drag_start = Some(snapped_world);
                    self.drag_action = Some(DragAction::RedrawShape(i));
                    self.status = format!("Selected shape {i}; drag to redraw, Del to delete");
                } else {
                    self.selected_shape = None;
                    self.drag_start = Some(snapped_world);
                    self.drag_action = Some(DragAction::NewShape);
                }
            }
            if ctx.is_mouse_button_released(MouseButton::Left) {
                if let (Some(start), Some(action)) =
                    (self.drag_start.take(), self.drag_action.take())
                {
                    match action {
                        DragAction::NewShape => {
                            if let Some(shape) =
                                make_shape(self.tool, start, snapped_world, self.color)
                            {
                                let active_shapes = self.active_shapes_mut();
                                active_shapes.push(shape);
                                let count = active_shapes.len();
                                self.is_dirty = true;
                                self.refresh_window_title(ctx);
                                self.status = format!(
                                    "Placed shape on {} ({} total)",
                                    self.active_layer_name(),
                                    count
                                );
                            }
                        }
                        DragAction::RedrawShape(i) => {
                            let redrawn = {
                                let active_shapes = self.active_shapes_mut();
                                active_shapes.get_mut(i).is_some_and(|shape| {
                                    update_shape_geometry(shape, start, snapped_world)
                                })
                            };
                            if redrawn {
                                self.is_dirty = true;
                                self.refresh_window_title(ctx);
                                self.status = format!("Redrawn shape {i}");
                            } else {
                                self.status =
                                    format!("Selected shape {i}; drag to redraw, Del to delete");
                            }
                            self.selected_shape = Some(i);
                        }
                    }
                }
            }
        }

        // Right click: delete
        if ctx.is_mouse_button_pressed(MouseButton::Right)
            && self.active_layer != Layer::ClassificationPlanning
            && !sheet_open
        {
            let mut deleted = false;
            if self.active_layer == Layer::SpritePlanning {
                if let Some(i) = self.sprite_at(world) {
                    self.level.sprite_instances.remove(i);
                    let count = self.level.sprite_instances.len();
                    self.is_dirty = true;
                    self.refresh_window_title(ctx);
                    self.status = format!("Deleted sprite ({count} left)");
                    deleted = true;
                }
            }
            if !deleted {
                let maybe_i = {
                    let active_shapes = self.active_shapes_mut();
                    active_shapes.iter().rposition(|s| s.contains(world))
                };
                if let Some(i) = maybe_i {
                    let count = {
                        let active_shapes = self.active_shapes_mut();
                        active_shapes.remove(i);
                        active_shapes.len()
                    };
                    match self.selected_shape {
                        Some(s) if s == i => self.selected_shape = None,
                        Some(s) if s > i => self.selected_shape = Some(s - 1),
                        _ => {}
                    }
                    self.is_dirty = true;
                    self.refresh_window_title(ctx);
                    self.status = format!(
                        "Deleted shape from {} ({} left)",
                        self.active_layer_name(),
                        count
                    );
                }
            }
        }

        // Delete / Backspace: remove sprite under cursor (sprite layer) or selected shape.
        // Backspace is included because on macOS the key labelled Delete sends
        // Backspace; the true Delete key is Fn+Delete.
        if self.active_layer != Layer::ClassificationPlanning
            && !sheet_open
            && (ctx.is_key_pressed(Key::Delete) || ctx.is_key_pressed(Key::Backspace))
        {
            let mut handled = false;
            if self.active_layer == Layer::SpritePlanning {
                if let Some(i) = self.sprite_at(world) {
                    self.level.sprite_instances.remove(i);
                    let count = self.level.sprite_instances.len();
                    self.is_dirty = true;
                    self.refresh_window_title(ctx);
                    self.status = format!("Deleted sprite ({count} left)");
                    handled = true;
                }
            }
            if !handled {
                if let Some(i) = self.selected_shape.take() {
                    let active_shapes = self.active_shapes_mut();
                    if i < active_shapes.len() {
                        active_shapes.remove(i);
                        let count = active_shapes.len();
                        self.is_dirty = true;
                        self.refresh_window_title(ctx);
                        self.status = format!("Deleted selected shape ({count} left)");
                    }
                }
            }
        }

        // Z/X undo / clear (sprite + collision only)
        if self.active_layer != Layer::ClassificationPlanning && !sheet_open {
            if ctx.is_key_pressed(Key::Z) {
                let undone = if self.active_layer == Layer::SpritePlanning
                    && !self.level.sprite_instances.is_empty()
                {
                    self.level.sprite_instances.pop();
                    let c = self.level.sprite_instances.len();
                    self.status = format!("Undid last sprite ({c} left)");
                    true
                } else {
                    let count = {
                        let shapes = self.active_shapes_mut();
                        if shapes.pop().is_some() {
                            let c = shapes.len();
                            self.status = format!(
                                "Undid last shape on {} ({c} left)",
                                self.active_layer_name()
                            );
                            Some(c)
                        } else {
                            None
                        }
                    };
                    if let Some(c) = count {
                        if self.selected_shape == Some(c) {
                            self.selected_shape = None;
                        }
                    }
                    count.is_some()
                };
                if undone {
                    self.is_dirty = true;
                    self.refresh_window_title(ctx);
                }
            }
            if ctx.is_key_pressed(Key::X) {
                let shapes = self.active_shapes_mut();
                if !shapes.is_empty() {
                    shapes.clear();
                    self.selected_shape = None;
                    self.is_dirty = true;
                    self.refresh_window_title(ctx);
                    self.status = format!("Cleared {} layer", self.active_layer_name());
                }
            }
        }

        // S save / O reload (all layers)
        if ctx.is_key_pressed(Key::S) {
            self.status = match self.level.save(&self.current_path) {
                Ok(()) => {
                    self.is_dirty = false;
                    self.refresh_window_title(ctx);
                    format!(
                        "Saved {} ({} sprites, {} collision, {} tags)",
                        self.current_path,
                        self.level.sprite_instances.len(),
                        self.level.collision_shapes.len(),
                        self.level.classifications.len(),
                    )
                }
                Err(e) => format!("Save failed: {e}"),
            };
        }
        if ctx.is_key_pressed(Key::O) {
            self.status = match Level::load(&self.current_path) {
                Ok(mut level) => {
                    level.ensure_ids(random_id);
                    self.tag_colors = build_tag_colors(&level);
                    let sn = level.sprite_instances.len();
                    let cn = level.collision_shapes.len();
                    let tn = level.classifications.len();
                    self.level = level;
                    self.is_dirty = false;
                    self.editing_object = None;
                    self.focused_object = None;
                    self.refresh_window_title(ctx);
                    format!(
                        "Reloaded {} ({sn} sprites, {cn} collision, {tn} tags)",
                        self.current_path
                    )
                }
                Err(e) => format!("Load failed: {e}"),
            };
        }
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        canvas.clear_background(DARKGRAY);
        canvas.begin_mode_2d(self.camera());
        self.draw_grid(canvas);

        // Sprites
        let sprite_alpha = match self.active_layer {
            Layer::SpritePlanning => 1.0,
            Layer::ClassificationPlanning => 0.6,
            Layer::CollisionPlanning => 0.30,
        };
        for inst in &self.level.sprite_instances {
            if let Some(tex) = self.sprite_cache.get(&inst.path) {
                canvas.draw_texture_ex(
                    tex,
                    Vec2D::new(inst.x, inst.y),
                    0.0,
                    inst.scale,
                    WHITE.with_alpha(sprite_alpha),
                );
            }
        }

        match self.active_layer {
            // --------------------------------------------------------
            // Classification layer: bounding boxes + labels
            // --------------------------------------------------------
            Layer::ClassificationPlanning => {
                self.draw_shape_layer(canvas, &self.level.sprite_shapes, 0.20);
                self.draw_shape_layer(canvas, &self.level.collision_shapes, 0.25);

                // Collect outlines and deferred labels in two separate loops.
                // Outlines are drawn immediately; labels are push-apart resolved
                // then drawn together so no two labels overlap regardless of how
                // the bounding boxes are spatially arranged.
                let mut labels: Vec<LabelSpec> = Vec::new();

                // --- Sprite instance outlines + label collection ---
                for i in 0..self.level.sprite_instances.len() {
                    let obj = ObjectRef::Sprite(i);
                    let id_str = self.level.sprite_instances[i].id.clone();
                    let path = self.level.sprite_instances[i].path.clone();
                    let x = self.level.sprite_instances[i].x;
                    let y = self.level.sprite_instances[i].y;
                    let scale = self.level.sprite_instances[i].scale;
                    let (bw, bh) = if let Some(tex) = self.sprite_cache.get(&path) {
                        (tex.width() as f32 * scale, tex.height() as f32 * scale)
                    } else {
                        (GRID_SIZE * scale, GRID_SIZE * scale)
                    };

                    let tag = self
                        .level
                        .classifications
                        .iter()
                        .find(|e| e.object_id == id_str)
                        .map(|e| e.tag.as_str())
                        .unwrap_or("static");
                    let tag_color = self.tag_colors.get(tag).copied().unwrap_or(LIGHTGRAY);
                    let is_editing_this = self
                        .editing_object
                        .as_ref()
                        .is_some_and(|(o, _, _)| o == &obj);
                    let is_focused = !is_editing_this && self.focused_object.as_ref() == Some(&obj);

                    draw_classification_outline(
                        canvas,
                        x,
                        y,
                        bw,
                        bh,
                        tag_color,
                        is_focused,
                        is_editing_this,
                    );

                    let eref = self.editing_object.as_ref().filter(|(o, _, _)| o == &obj);
                    let text = build_label_text(&id_str, tag, eref);
                    labels.push(LabelSpec {
                        x,
                        y: y - 17.0, // prefer above the sprite
                        w: (text.len() as f32 * 7.5 + 8.0).max(40.0),
                        text,
                        text_color: if is_editing_this {
                            WHITE
                        } else if is_focused {
                            GOLD
                        } else {
                            tag_color
                        },
                        bg_color: if is_focused || is_editing_this {
                            BLACK.with_alpha(0.88)
                        } else {
                            BLACK.with_alpha(0.70)
                        },
                    });
                }

                // --- Collision shape outlines + label collection ---
                for i in 0..self.level.collision_shapes.len() {
                    let obj = ObjectRef::CollisionShape(i);
                    let id_str = self.level.collision_shapes[i].id().to_string();
                    let (bx, by, bw, bh) = self.level.collision_shapes[i].bounding_rect();

                    let tag = self
                        .level
                        .classifications
                        .iter()
                        .find(|e| e.object_id == id_str)
                        .map(|e| e.tag.as_str())
                        .unwrap_or("static");
                    let tag_color = self.tag_colors.get(tag).copied().unwrap_or(LIGHTGRAY);
                    let is_editing_this = self
                        .editing_object
                        .as_ref()
                        .is_some_and(|(o, _, _)| o == &obj);
                    let is_focused = !is_editing_this && self.focused_object.as_ref() == Some(&obj);

                    draw_classification_outline(
                        canvas,
                        bx,
                        by,
                        bw,
                        bh,
                        tag_color,
                        is_focused,
                        is_editing_this,
                    );

                    let eref = self.editing_object.as_ref().filter(|(o, _, _)| o == &obj);
                    let text = build_label_text(&id_str, tag, eref);
                    labels.push(LabelSpec {
                        x: bx,
                        y: by + bh + 3.0, // prefer below the collision shape
                        w: (text.len() as f32 * 7.5 + 8.0).max(40.0),
                        text,
                        text_color: if is_editing_this {
                            WHITE
                        } else if is_focused {
                            GOLD
                        } else {
                            tag_color
                        },
                        bg_color: if is_focused || is_editing_this {
                            BLACK.with_alpha(0.88)
                        } else {
                            BLACK.with_alpha(0.70)
                        },
                    });
                }

                // Resolve any remaining overlaps, then draw all labels.
                resolve_label_overlaps(&mut labels);
                for spec in &labels {
                    canvas.rectangle(spec.x, spec.y, spec.w, LABEL_H, spec.bg_color);
                    canvas.text(
                        &spec.text,
                        spec.x + 3.0,
                        spec.y + 1.0,
                        12.0,
                        spec.text_color,
                    );
                }
            }

            // --------------------------------------------------------
            // Sprite / collision layers: normal editing view
            // --------------------------------------------------------
            _ => {
                self.draw_shape_layer(canvas, self.inactive_shapes(), 0.30);
                self.draw_shape_layer(canvas, self.active_shapes(), 1.0);

                // Highlight the selected shape.
                if let Some(i) = self.selected_shape {
                    if let Some(shape) = self.active_shapes().get(i) {
                        let (x, y, w, h) = shape.bounding_rect();
                        draw_rect_outline(canvas, x, y, w, h, 2.0, WHITE);
                    }
                }

                if self.active_layer == Layer::SpritePlanning {
                    if let Some(idx) = self.selected_sprite {
                        if let Some(path) = self.available_sprites.get(idx) {
                            if let Some(tex) = self.sprite_cache.get(path) {
                                let pos = self.snap_world(self.mouse_world());
                                canvas.draw_texture_ex(
                                    tex,
                                    pos,
                                    0.0,
                                    self.sprite_scale,
                                    WHITE.with_alpha(0.45),
                                );
                            }
                        }
                    }
                } else if let Some(start) = self.drag_start {
                    let end = self.snap_world(self.mouse_world());
                    match self.drag_action {
                        Some(DragAction::RedrawShape(i)) => {
                            if let Some(mut shape) = self.active_shapes().get(i).cloned() {
                                if update_shape_geometry(&mut shape, start, end) {
                                    shape.with_alpha(0.5).draw(canvas);
                                }
                            }
                        }
                        _ => {
                            if let Some(shape) =
                                make_shape(self.tool, start, end, self.color.with_alpha(0.5))
                            {
                                shape.draw(canvas);
                            }
                        }
                    }
                }
            }
        }

        canvas.end_mode_2d();

        // Crosshair
        let sm = self
            .camera()
            .world_to_screen(self.snap_world(self.mouse_world()));
        canvas.line(
            sm - Vec2D::new(10.0, 0.0),
            sm + Vec2D::new(10.0, 0.0),
            1.0,
            WHITE,
        );
        canvas.line(
            sm - Vec2D::new(0.0, 10.0),
            sm + Vec2D::new(0.0, 10.0),
            1.0,
            WHITE,
        );

        // ---- HUD: info panel ----
        let save_state = if self.is_dirty { "Unsaved" } else { "Saved" };
        let save_color = if self.is_dirty { ORANGE } else { LIME };
        canvas.rectangle(20.0, 20.0, 340.0, 136.0, BLACK.with_alpha(0.8));
        canvas.text("Layer", 40.0, 34.0, 24.0, GOLD);
        canvas.text(self.active_layer_name(), 110.0, 34.0, 24.0, WHITE);
        canvas.text("Tab", 260.0, 34.0, 24.0, GOLD);
        canvas.text("cycle", 300.0, 38.0, 20.0, LIGHTGRAY);
        canvas.text("State", 40.0, 76.0, 24.0, GOLD);
        canvas.text(save_state, 110.0, 76.0, 24.0, save_color);
        canvas.text("File", 40.0, 118.0, 24.0, GOLD);
        canvas.text(self.current_file_label(), 96.0, 118.0, 20.0, LIGHTGRAY);

        // ---- HUD: sprite picker ----
        if self.active_layer == Layer::SpritePlanning {
            canvas.rectangle(20.0, 168.0, 340.0, 132.0, BLACK.with_alpha(0.8));
            if self.available_sprites.is_empty() {
                canvas.text("No sprites in sprites/", 40.0, 200.0, 20.0, LIGHTGRAY);
            } else {
                canvas.text("Sprite", 40.0, 184.0, 24.0, GOLD);
                canvas.text(self.selected_sprite_name(), 110.0, 184.0, 22.0, WHITE);
                canvas.text("[ ]", 240.0, 184.0, 20.0, GOLD);
                canvas.text("cycle", 275.0, 188.0, 18.0, LIGHTGRAY);
                canvas.text("Scale", 40.0, 218.0, 24.0, GOLD);
                canvas.text(
                    &format!("{:.2}x", self.sprite_scale),
                    110.0,
                    218.0,
                    22.0,
                    WHITE,
                );
                canvas.text(", .", 190.0, 218.0, 20.0, GOLD);
                canvas.text("half/double", 220.0, 222.0, 18.0, LIGHTGRAY);
                canvas.text("Click to place", 40.0, 252.0, 20.0, LIGHTGRAY);
            }
            canvas.text("L", 40.0, 282.0, 20.0, GOLD);
            canvas.text("show/hide", 68.0, 282.0, 20.0, LIGHTGRAY);
            canvas.text("Shift+L", 150.0, 282.0, 20.0, GOLD);
            canvas.text("load/replace", 238.0, 282.0, 20.0, LIGHTGRAY);
        }

        // ---- HUD: classification tag legend + hints ----
        if self.active_layer == Layer::ClassificationPlanning {
            let mut sorted: Vec<(&String, &Color)> = self.tag_colors.iter().collect();
            sorted.sort_by_key(|(k, _)| k.as_str());
            let panel_h = 34.0 + sorted.len() as f32 * 22.0 + 12.0;
            canvas.rectangle(20.0, 168.0, 340.0, panel_h, BLACK.with_alpha(0.8));
            canvas.text("Tags", 40.0, 178.0, 22.0, GOLD);
            for (i, (tag, &color)) in sorted.iter().enumerate() {
                let py = 204.0 + i as f32 * 22.0;
                canvas.rectangle(40.0, py, 14.0, 14.0, color);
                canvas.text(tag, 60.0, py, 18.0, color);
            }

            let hint_y = 168.0 + panel_h + 6.0;
            if let Some((_, mode, _)) = &self.editing_object {
                canvas.rectangle(20.0, hint_y, 340.0, 26.0, BLACK.with_alpha(0.8));
                let hint = match mode {
                    EditMode::Tag => "Tab=cycle tags  Enter=confirm  Esc=cancel  Bksp",
                    EditMode::ObjectId => "Enter=confirm  Esc=cancel  Backspace=delete",
                };
                canvas.text(hint, 28.0, hint_y + 5.0, 16.0, GOLD);
            } else {
                canvas.rectangle(20.0, hint_y, 340.0, 52.0, BLACK.with_alpha(0.8));
                canvas.text(
                    "Arrows=navigate  Enter=edit tag  I=edit ID",
                    28.0,
                    hint_y + 5.0,
                    16.0,
                    LIGHTGRAY,
                );
                canvas.text(
                    "Del=clear tag  L-click=edit  R-click=clear",
                    28.0,
                    hint_y + 27.0,
                    16.0,
                    LIGHTGRAY,
                );
            }
        }

        // ---- HUD: help overlay ----
        if self.show_help {
            canvas.rectangle(20.0, 250.0, 560.0, 460.0, BLACK.with_alpha(0.8));
            canvas.text("Editor controls", 40.0, 308.0, 28.0, GOLD);

            canvas.text("Mouse", 40.0, 348.0, 22.0, WHITE);
            canvas.text(
                "L-drag        Place new shape",
                60.0,
                372.0,
                19.0,
                LIGHTGRAY,
            );
            canvas.text(
                "L-click shape Select / drag to redraw",
                60.0,
                394.0,
                19.0,
                LIGHTGRAY,
            );
            canvas.text(
                "R-click       Delete shape under cursor",
                60.0,
                416.0,
                19.0,
                LIGHTGRAY,
            );
            canvas.text(
                "Del / Bksp    Delete sprite under cursor / selected shape",
                60.0,
                438.0,
                19.0,
                LIGHTGRAY,
            );
            canvas.text("M-drag        Pan camera", 60.0, 460.0, 19.0, LIGHTGRAY);
            canvas.text("Wheel         Zoom", 60.0, 482.0, 19.0, LIGHTGRAY);

            canvas.text("Shapes", 40.0, 510.0, 22.0, WHITE);
            canvas.text("R / C  Tool     1-6  Color", 60.0, 532.0, 19.0, LIGHTGRAY);

            canvas.text("Sprites (sprite layer)", 40.0, 560.0, 22.0, WHITE);
            canvas.text("[ ]  cycle    , .  scale", 60.0, 582.0, 19.0, LIGHTGRAY);
            canvas.text(
                "L show/hide  Shift+L load/replace  Del unload  drag=cut",
                60.0,
                604.0,
                19.0,
                LIGHTGRAY,
            );

            canvas.text("Classification layer", 40.0, 632.0, 22.0, WHITE);
            canvas.text(
                "Arrows  navigate    Enter  edit tag",
                60.0,
                654.0,
                19.0,
                LIGHTGRAY,
            );
            canvas.text("I  edit ID    Del  clear tag", 60.0, 674.0, 19.0, LIGHTGRAY);
            canvas.text(
                "Tab (in edit)  cycle existing tags",
                60.0,
                694.0,
                19.0,
                LIGHTGRAY,
            );

            canvas.text("Level", 310.0, 348.0, 22.0, WHITE);
            canvas.text("S    Save", 330.0, 372.0, 19.0, LIGHTGRAY);
            canvas.text("O    Reload", 330.0, 394.0, 19.0, LIGHTGRAY);
            canvas.text("Tab  Cycle layer", 330.0, 416.0, 19.0, LIGHTGRAY);
            canvas.text("Z    Undo last", 330.0, 438.0, 19.0, LIGHTGRAY);
            canvas.text("X    Clear layer", 330.0, 460.0, 19.0, LIGHTGRAY);

            canvas.text("View", 310.0, 488.0, 22.0, WHITE);
            canvas.text("F    Reset view", 330.0, 510.0, 19.0, LIGHTGRAY);
            canvas.text("H    Toggle help", 330.0, 532.0, 19.0, LIGHTGRAY);
            canvas.text("Esc  Quit / cancel", 330.0, 554.0, 19.0, LIGHTGRAY);
        }

        // ---- Sheet cutter panel (modal, on top of HUD) ----
        if self.sheet_panel_open {
            if let Some(sheet) = &self.sprite_sheet {
                let panel = sheet_panel_rect(sheet.width, sheet.height);

                // Dim the rest of the UI to focus on the sheet.
                canvas.rectangle(
                    0.0,
                    0.0,
                    RENDER_W as f32,
                    RENDER_H as f32,
                    BLACK.with_alpha(0.45),
                );

                // Panel background
                canvas.rectangle(panel.x, panel.y, panel.w, panel.h, BLACK.with_alpha(0.92));
                draw_rect_outline(canvas, panel.x, panel.y, panel.w, panel.h, 2.0, GOLD);

                // Title
                canvas.text(
                    "Spritesheet / tileset — drag to select a tile",
                    panel.x + 16.0,
                    panel.y + 24.0,
                    22.0,
                    GOLD,
                );

                // Scaled sheet image
                let dest = Rect::new(
                    panel.x + panel.offset_x,
                    panel.y + panel.offset_y,
                    sheet.width as f32 * panel.scale,
                    sheet.height as f32 * panel.scale,
                );
                canvas.draw_texture_pro(
                    &sheet.texture,
                    Rect::new(0.0, 0.0, sheet.width as f32, sheet.height as f32),
                    dest,
                    Vec2D::ZERO,
                    0.0,
                    WHITE,
                );

                // Confirmed selection outline
                if let Some(sel) = self.sheet_selection {
                    let r = sheet_rect_to_panel(&panel, sel);
                    draw_rect_outline(canvas, r.x, r.y, r.width, r.height, 2.0, GREEN);
                }

                // In-progress drag preview
                if let (Some(start), Some(sheet_pos)) =
                    (self.sheet_drag_start, sheet_mouse_pos(&panel, self.mouse))
                {
                    let current = Vec2D::new(
                        sheet_pos.x.clamp(0.0, sheet.width as f32),
                        sheet_pos.y.clamp(0.0, sheet.height as f32),
                    );
                    let preview =
                        clamp_rect(rect_from_points(start, current), sheet.width, sheet.height);
                    let r = sheet_rect_to_panel(&panel, preview);
                    draw_rect_outline(canvas, r.x, r.y, r.width, r.height, 1.0, YELLOW);
                }

                // Hint label inside the panel footer
                canvas.text(
                    "Drag=select  Release=cut  Esc=close  Del=unload",
                    panel.x + 16.0,
                    panel.y + panel.h - 18.0,
                    16.0,
                    LIGHTGRAY,
                );
            }
        }

        // Path input overlay
        if self.entering_sheet_path {
            canvas.rectangle(
                0.0,
                0.0,
                RENDER_W as f32,
                RENDER_H as f32,
                BLACK.with_alpha(0.6),
            );
            let box_w = 760.0;
            let box_h = 120.0;
            let box_x = (RENDER_W as f32 - box_w) * 0.5;
            let box_y = (RENDER_H as f32 - box_h) * 0.5;
            canvas.rectangle(box_x, box_y, box_w, box_h, DARKGRAY);
            draw_rect_outline(canvas, box_x, box_y, box_w, box_h, 2.0, GOLD);
            canvas.text(
                "Load spritesheet / tileset",
                box_x + 20.0,
                box_y + 16.0,
                24.0,
                GOLD,
            );
            let prompt = format!("{}|", self.sheet_path_input);
            canvas.text(&prompt, box_x + 20.0, box_y + 60.0, 22.0, WHITE);
            canvas.text(
                "Enter=load  Esc=cancel",
                box_x + 20.0,
                box_y + 94.0,
                18.0,
                LIGHTGRAY,
            );
        }

        canvas.text(&self.status, 20.0, RENDER_H as f32 - 32.0, 22.0, GOLD);
    }
}

/// Draw the bounding-box outline for one classifiable object.
/// Labels are NOT drawn here — they are collected into [`LabelSpec`]s,
/// overlap-resolved by [`resolve_label_overlaps`], and rendered afterwards.
fn draw_classification_outline(
    canvas: &mut Canvas,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    tag_color: Color,
    is_focused: bool,
    is_editing: bool,
) {
    let (thickness, box_color) = if is_editing {
        (3.0, WHITE)
    } else if is_focused {
        (2.5, GOLD)
    } else {
        (1.5, tag_color.with_alpha(0.9))
    };
    draw_rect_outline(canvas, x, y, w, h, thickness, box_color);
}

/// Build the label string for a classifiable object.
///
/// Format:
/// - Normal:      `"id | tag"`
/// - Editing tag: `"id | buffer|"`
/// - Editing ID:  `"buffer| | tag"`
fn build_label_text(
    id: &str,
    tag: &str,
    edit_state: Option<&(ObjectRef, EditMode, String)>,
) -> String {
    if let Some((_, mode, buf)) = edit_state {
        match mode {
            EditMode::Tag => format!("{id} | {buf}|"),
            EditMode::ObjectId => format!("{buf}| | {tag}"),
        }
    } else {
        format!("{id} | {tag}")
    }
}

/// Push [`LabelSpec`]s apart so that no two labels whose x-ranges overlap
/// are rendered on top of each other.
///
/// Algorithm: sort by preferred y, then do an O(n²) forward pass — each
/// label can push every subsequent label downward if they share an x-range
/// and are too close vertically. Because pushed labels are only ever moved
/// downward, and j > i in the inner loop sees the already-updated y of i,
/// cascades propagate correctly in a single pass.
fn resolve_label_overlaps(labels: &mut Vec<LabelSpec>) {
    labels.sort_by(|a, b| a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal));
    let n = labels.len();
    for i in 0..n {
        for j in (i + 1)..n {
            // Only push if the two labels share a horizontal band.
            let x_overlap =
                labels[j].x < labels[i].x + labels[i].w && labels[j].x + labels[j].w > labels[i].x;
            if x_overlap {
                let needed = labels[i].y + LABEL_H + LABEL_GAP;
                if labels[j].y < needed {
                    labels[j].y = needed;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let path = match prompt_level_path() {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Failed to read level path: {e}");
            std::process::exit(1);
        }
    };
    let startup = match load_or_create_level(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Failed to open or create level at {path}: {e}");
            std::process::exit(1);
        }
    };
    STARTUP
        .set(startup)
        .expect("startup config should only be set once");
    run::<Editor>(Config {
        width: 1920,
        height: 1080,
        render_width: RENDER_W,
        render_height: RENDER_H,
        title: WINDOW_TITLE.to_string(),
        target_ups: 60,
        centered: true,
        resizable: false,
        msaa: 4,
        ..Config::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn crop_and_save_sprite_cuts_a_region() {
        let tmp = std::env::temp_dir().join("juni_editor_sheet_test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();

        let sheet_path = tmp.join("sheet.png");
        let sprites_dir = tmp.join("sprites");
        std::fs::create_dir_all(&sprites_dir).unwrap();

        // 4x4 checkerboard: red, green, blue, yellow quadrants.
        let mut img = image::RgbaImage::new(4, 4);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = match (x / 2, y / 2) {
                (0, 0) => Rgba([255, 0, 0, 255]),
                (1, 0) => Rgba([0, 255, 0, 255]),
                (0, 1) => Rgba([0, 0, 255, 255]),
                _ => Rgba([255, 255, 0, 255]),
            };
        }
        img.save(&sheet_path).unwrap();

        let selection = Rect::new(0.0, 0.0, 2.0, 2.0);
        let out_path = crop_and_save_sprite(
            sheet_path.to_str().unwrap(),
            selection,
            sprites_dir.to_str().unwrap(),
        )
        .unwrap();

        assert!(std::path::Path::new(&out_path).exists());
        let cropped = image::open(&out_path).unwrap();
        let (w, h) = cropped.dimensions();
        assert_eq!(w, 2);
        assert_eq!(h, 2);

        // Top-left pixel of the cropped region should be red.
        assert_eq!(cropped.get_pixel(0, 0), Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn sheet_panel_rect_fits_large_sheet_inside_max_size() {
        let panel = sheet_panel_rect(2048, 1024);
        assert!(panel.w <= 1000.0);
        assert!(panel.h <= 600.0);
        let img_w = 2048.0 * panel.scale;
        let img_h = 1024.0 * panel.scale;
        assert!(img_w <= 1000.0 - 80.0);
        assert!(img_h <= 600.0 - 80.0);
    }

    #[test]
    fn sheet_panel_rect_upscales_small_sheet() {
        let panel = sheet_panel_rect(4, 4);
        assert!(panel.scale >= 1.0);
    }

    #[test]
    fn clamp_rect_keeps_selection_inside_image() {
        let rect = Rect::new(-5.0, -5.0, 100.0, 100.0);
        let clamped = clamp_rect(rect, 16, 16);
        assert_eq!(clamped.x, 0.0);
        assert_eq!(clamped.y, 0.0);
        assert_eq!(clamped.width, 16.0);
        assert_eq!(clamped.height, 16.0);
    }
}
