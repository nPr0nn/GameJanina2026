//! The `Editor` model: level data + pure queries/mutations.
//!
//! Rendering, input, the camera transform and all UI live in [`crate::app`]
//! (the `eframe::App`). This module is deliberately free of egui drawing code —
//! it only answers questions about the level and edits it.

use std::collections::HashMap;
use std::path::Path;

use juni::prelude::*;

use crate::constants::*;
use crate::level_io::build_tag_colors;
use crate::types::*;

/// Editor model state. View state (pan/zoom), the file dialogs and the
/// spritesheet cutter live in [`crate::app::EditorApp`].
pub(crate) struct Editor {
    pub(crate) current_path: String,
    pub(crate) level: Level,
    pub(crate) active_layer: Layer,
    pub(crate) tool: Tool,
    pub(crate) color: Color,
    /// Drag-in-progress start point (world space), if any.
    pub(crate) drag_start: Option<Vec2D>,
    pub(crate) drag_action: Option<DragAction>,
    pub(crate) selected_shape: Option<usize>,
    pub(crate) status: String,
    pub(crate) is_dirty: bool,
    // --- Sprite support ---
    pub(crate) available_sprites: Vec<String>,
    pub(crate) selected_sprite: Option<usize>,
    pub(crate) sprite_scale: f32,
    /// PNG path -> loaded egui texture (with its pixel dimensions).
    pub(crate) sprite_cache: HashMap<String, egui::TextureHandle>,
    // --- Classification layer ---
    /// tag string -> display color
    pub(crate) tag_colors: HashMap<String, Color>,
    /// Which object is selected/focused in the classification layer.
    pub(crate) focused_object: Option<ObjectRef>,
}

impl Editor {
    /// Build the model from a loaded level and its source path.
    pub(crate) fn new(current_path: String, level: Level, status: String) -> Self {
        let available_sprites = crate::level_io::scan_sprites("sprites");
        let selected_sprite = (!available_sprites.is_empty()).then_some(0);
        let tag_colors = build_tag_colors(&level);
        Self {
            current_path,
            level,
            active_layer: Layer::SpritePlanning,
            tool: Tool::Rect,
            color: Self::default_layer_color(Layer::SpritePlanning),
            drag_start: None,
            drag_action: None,
            selected_shape: None,
            status,
            is_dirty: false,
            available_sprites,
            selected_sprite,
            sprite_scale: 1.0,
            sprite_cache: HashMap::new(),
            tag_colors,
            focused_object: None,
        }
    }

    /// Display name (file stem) of the currently-selected sprite.
    pub(crate) fn selected_sprite_name(&self) -> &str {
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

    /// Pixel size of a cached sprite texture, or `None` if not loaded yet.
    pub(crate) fn sprite_dims(&self, path: &str) -> Option<(f32, f32)> {
        self.sprite_cache.get(path).map(|t| {
            let [w, h] = t.size();
            (w as f32, h as f32)
        })
    }

    /// Add `path` to the available list (if new) and select it. Texture loading
    /// is the app's responsibility (see `EditorApp::ensure_texture`).
    pub(crate) fn add_and_select_sprite_path(&mut self, path: String) {
        if let Some(i) = self.available_sprites.iter().position(|p| p == &path) {
            self.selected_sprite = Some(i);
        } else {
            self.available_sprites.push(path.clone());
            self.available_sprites.sort();
            self.selected_sprite = self.available_sprites.iter().position(|p| p == &path);
        }
    }

    /// Bounding rect `(x, y, w, h)` for a sprite instance.
    pub(crate) fn sprite_bounding_rect(&self, inst: &SpriteInstance) -> (f32, f32, f32, f32) {
        let (w, h) = self.sprite_dims(&inst.path).unwrap_or((GRID_SIZE, GRID_SIZE));
        (inst.x, inst.y, w * inst.scale, h * inst.scale)
    }

    /// String ID of the given object (borrows from the level).
    pub(crate) fn object_id<'a>(&'a self, obj: &ObjectRef) -> &'a str {
        match obj {
            ObjectRef::Sprite(i) => &self.level.sprite_instances[*i].id,
            ObjectRef::CollisionShape(i) => self.level.collision_shapes[*i].id(),
        }
    }

    /// Overwrite the string ID of the given object.
    pub(crate) fn object_set_id(&mut self, obj: &ObjectRef, new_id: String) {
        match obj {
            ObjectRef::Sprite(i) => self.level.sprite_instances[*i].id = new_id,
            ObjectRef::CollisionShape(i) => self.level.collision_shapes[*i].set_id(new_id),
        }
    }

    /// Current tag for the given object (falls back to `"static"`).
    pub(crate) fn object_tag<'a>(&'a self, obj: &ObjectRef) -> &'a str {
        self.level.get_tag(self.object_id(obj)).unwrap_or("static")
    }

    /// All classifiable objects in display order (sprites first, then shapes).
    pub(crate) fn ordered_objects(&self) -> Vec<ObjectRef> {
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
    pub(crate) fn classify_object_at(&self, world: Vec2D) -> Option<ObjectRef> {
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
    pub(crate) fn sprite_at(&self, p: Vec2D) -> Option<usize> {
        self.level.sprite_instances.iter().rposition(|s| {
            if let Some((tw, th)) = self.sprite_dims(&s.path) {
                let w = tw * s.scale;
                let h = th * s.scale;
                p.x >= s.x && p.x <= s.x + w && p.y >= s.y && p.y <= s.y + h
            } else {
                false
            }
        })
    }

    pub(crate) fn active_layer_name(&self) -> &'static str {
        match self.active_layer {
            Layer::SpritePlanning => "Sprite",
            Layer::CollisionPlanning => "Collision",
            Layer::ClassificationPlanning => "Classification",
        }
    }

    pub(crate) fn default_layer_color(layer: Layer) -> Color {
        match layer {
            Layer::SpritePlanning => BLUE,
            Layer::CollisionPlanning => RED,
            Layer::ClassificationPlanning => PURPLE,
        }
    }

    pub(crate) fn active_shapes(&self) -> &[Shape] {
        match self.active_layer {
            Layer::SpritePlanning => &self.level.sprite_shapes,
            Layer::CollisionPlanning => &self.level.collision_shapes,
            Layer::ClassificationPlanning => &[],
        }
    }

    pub(crate) fn active_shapes_mut(&mut self) -> &mut Vec<Shape> {
        match self.active_layer {
            Layer::SpritePlanning => &mut self.level.sprite_shapes,
            Layer::CollisionPlanning => &mut self.level.collision_shapes,
            Layer::ClassificationPlanning => &mut self.level.sprite_shapes,
        }
    }

    pub(crate) fn inactive_shapes(&self) -> &[Shape] {
        match self.active_layer {
            Layer::SpritePlanning => &self.level.collision_shapes,
            Layer::CollisionPlanning => &self.level.sprite_shapes,
            Layer::ClassificationPlanning => &[],
        }
    }

    pub(crate) fn current_file_label(&self) -> &str {
        Path::new(&self.current_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&self.current_path)
    }

    /// Window title for the native window (set via egui's viewport command).
    pub(crate) fn window_title(&self) -> String {
        let dirty = if self.is_dirty { " *" } else { "" };
        format!(
            "{WINDOW_TITLE} — {} — {}{}",
            self.current_path,
            self.active_layer_name(),
            dirty
        )
    }
}
