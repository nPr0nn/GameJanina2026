//! The `Editor` model + its juni `Game` implementation.
//! (The `impl Game` block is rewritten as an `eframe::App` in PR2.)

use std::collections::HashMap;
use std::path::Path;

use image::GenericImageView;

use juni::prelude::*;

use crate::classification::*;
use crate::constants::*;
use crate::geometry::*;
use crate::id::*;
use crate::level_io::*;
use crate::render::*;
use crate::sprite_sheet::*;
use crate::text_input::*;
use crate::types::*;

pub(crate) struct Editor {
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

