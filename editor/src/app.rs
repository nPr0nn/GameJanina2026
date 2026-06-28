//! The `eframe::App`: rendering, input, panels and the world<->screen camera.
//!
//! All egui lives here; [`crate::editor::Editor`] stays a pure model. The level
//! view is drawn with an [`egui::Painter`]; the UI chrome is real egui panels.

use std::path::PathBuf;

use egui_file_dialog::FileDialog;
use juni::prelude::*;

use crate::classification::{build_label_text, resolve_label_overlaps, LabelSpec};
use crate::constants::*;
use crate::editor::Editor;
use crate::geometry::{make_shape, translate_shape, update_shape_geometry};
use crate::id::random_id;
use crate::level_io::build_tag_colors;
use crate::sprite_sheet::{crop_and_save_sprite, snap_tile_selection};
use crate::types::*;

/// A loaded spritesheet plus the in-progress tile selection.
struct SheetState {
    path: String,
    texture: egui::TextureHandle,
    width: u32,
    height: u32,
    /// Tile grid cell size in sheet pixels. Selections snap to whole cells; the
    /// grid is drawn over the sheet. `1` (or less) disables snapping.
    grid: u32,
    /// Drag start in sheet-pixel coordinates while selecting.
    drag_start: Option<egui::Pos2>,
}

/// Pan/zoom camera. `target` is the world point shown at the canvas top-left.
struct View {
    target: Vec2D,
    zoom: f32,
}

impl View {
    fn world_to_screen(&self, rect: egui::Rect, w: Vec2D) -> egui::Pos2 {
        rect.min + egui::vec2((w.x - self.target.x) * self.zoom, (w.y - self.target.y) * self.zoom)
    }

    fn screen_to_world(&self, rect: egui::Rect, s: egui::Pos2) -> Vec2D {
        Vec2D::new(
            (s.x - rect.min.x) / self.zoom + self.target.x,
            (s.y - rect.min.y) / self.zoom + self.target.y,
        )
    }
}

pub struct EditorApp {
    ed: Editor,
    view: View,
    sheet: Option<SheetState>,
    show_help: bool,
    title_dirty: bool,
    /// When armed, the next canvas left-click sets the player spawn.
    arming_spawn: bool,
    // File dialogs.
    open_dialog: FileDialog,
    save_dialog: FileDialog,
    sheet_dialog: FileDialog,
    // Classification edit buffers for the focused object.
    tag_buf: String,
    id_buf: String,
    synced_focus: Option<ObjectRef>,
}

/// Convert a juni `Color` to an egui `Color32`.
fn col(c: Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(c.r, c.g, c.b, c.a)
}

/// Snap a world point to a grid of cell size `grid` (world pixels).
fn snap(w: Vec2D, grid: f32) -> Vec2D {
    let grid = grid.max(1.0);
    Vec2D::new((w.x / grid).round() * grid, (w.y / grid).round() * grid)
}

/// A labelled `f32` drag box clamped to `min..`. Returns `true` if edited.
fn num_field(ui: &mut egui::Ui, label: &str, v: &mut f32, min: f32) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.add(
            egui::DragValue::new(v)
                .speed(1.0)
                .range(min..=f32::MAX)
                .suffix(" px"),
        )
        .changed()
    })
    .inner
}

/// Load a PNG into an egui `ColorImage` (nearest-neighbour pixel art).
fn load_color_image(path: &str) -> Option<egui::ColorImage> {
    let img = image::open(path).ok()?.to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    Some(egui::ColorImage::from_rgba_unmultiplied([w, h], img.as_raw()))
}

impl EditorApp {
    pub fn new(cc: &eframe::CreationContext<'_>, ed: Editor) -> Self {
        let mut app = Self {
            ed,
            view: View {
                target: Vec2D::ZERO,
                zoom: 1.0,
            },
            sheet: None,
            show_help: false,
            title_dirty: true,
            arming_spawn: false,
            open_dialog: FileDialog::new(),
            save_dialog: FileDialog::new(),
            sheet_dialog: FileDialog::new(),
            tag_buf: String::new(),
            id_buf: String::new(),
            synced_focus: None,
        };
        // Pre-load textures for already-placed sprites and the selected sprite.
        let paths: Vec<String> = app
            .ed
            .level
            .sprite_instances
            .iter()
            .map(|s| s.path.clone())
            .chain(app.ed.selected_sprite.and_then(|i| app.ed.available_sprites.get(i).cloned()))
            .collect();
        for p in paths {
            app.ensure_texture(&cc.egui_ctx, &p);
        }
        app
    }

    /// Load `path` into the sprite texture cache if not already present.
    fn ensure_texture(&mut self, ctx: &egui::Context, path: &str) {
        if self.ed.sprite_cache.contains_key(path) {
            return;
        }
        if let Some(ci) = load_color_image(path) {
            let tex = ctx.load_texture(path, ci, egui::TextureOptions::NEAREST);
            self.ed.sprite_cache.insert(path.to_string(), tex);
        }
    }

    fn select_sprite_index(&mut self, ctx: &egui::Context, idx: usize) {
        if let Some(path) = self.ed.available_sprites.get(idx).cloned() {
            self.ed.selected_sprite = Some(idx);
            self.ensure_texture(ctx, &path);
            self.ed.status = format!("Sprite: {}", self.ed.selected_sprite_name());
        }
    }

    fn save(&mut self) {
        let path = self.ed.current_path.clone();
        self.ed.status = match self.ed.level.save(&path) {
            Ok(()) => {
                self.ed.is_dirty = false;
                self.title_dirty = true;
                format!(
                    "Saved {} ({} sprites, {} collision, {} tags)",
                    path,
                    self.ed.level.sprite_instances.len(),
                    self.ed.level.collision_shapes.len(),
                    self.ed.level.classifications.len(),
                )
            }
            Err(e) => format!("Save failed: {e}"),
        };
    }

    fn open_path(&mut self, ctx: &egui::Context, path: &str) {
        match crate::level_io::load_or_create_level(path) {
            Ok(loaded) => {
                self.ed.tag_colors = build_tag_colors(&loaded.level);
                self.ed.level = loaded.level;
                self.ed.current_path = loaded.path;
                self.ed.is_dirty = false;
                self.ed.selected_shape = None;
                self.ed.focused_object = None;
                self.title_dirty = true;
                self.ed.status = loaded.status;
                let paths: Vec<String> =
                    self.ed.level.sprite_instances.iter().map(|s| s.path.clone()).collect();
                for p in paths {
                    self.ensure_texture(ctx, &p);
                }
            }
            Err(e) => self.ed.status = format!("Open failed: {e}"),
        }
    }

    fn load_sheet(&mut self, ctx: &egui::Context, path: &str) {
        match load_color_image(path) {
            Some(ci) => {
                let [w, h] = ci.size;
                let texture = ctx.load_texture(path, ci, egui::TextureOptions::NEAREST);
                self.sheet = Some(SheetState {
                    path: path.to_string(),
                    texture,
                    width: w as u32,
                    height: h as u32,
                    grid: 16,
                    drag_start: None,
                });
                self.ed.status = format!("Loaded sheet {w}x{h}; drag to cut a tile");
            }
            None => self.ed.status = format!("Failed to load sheet: {path}"),
        }
    }

    /// Apply the focused object's edited id/tag from the side-panel buffers.
    fn apply_classification(&mut self) {
        let Some(obj) = self.ed.focused_object.clone() else {
            return;
        };
        // ID first (tag lookup is by id).
        let new_id = {
            let t = self.id_buf.trim();
            if t.is_empty() {
                random_id()
            } else {
                t.to_string()
            }
        };
        let old_id = self.ed.object_id(&obj).to_string();
        if new_id != old_id {
            // Re-point any classification rows that referenced the old id.
            for e in self.ed.level.classifications.iter_mut() {
                if e.object_id == old_id {
                    e.object_id = new_id.clone();
                }
            }
            self.ed.object_set_id(&obj, new_id.clone());
        }
        let tag = {
            let t = self.tag_buf.trim();
            if t.is_empty() {
                "static".to_string()
            } else {
                t.to_string()
            }
        };
        if let Some(entry) = self
            .ed
            .level
            .classifications
            .iter_mut()
            .find(|e| e.object_id == new_id)
        {
            entry.tag = tag.clone();
        } else {
            self.ed.level.classifications.push(ClassificationEntry {
                object_id: new_id.clone(),
                tag: tag.clone(),
            });
        }
        if !self.ed.tag_colors.contains_key(&tag) {
            let i = self.ed.tag_colors.len() % TAG_PALETTE.len();
            self.ed.tag_colors.insert(tag.clone(), TAG_PALETTE[i]);
        }
        self.ed.is_dirty = true;
        self.title_dirty = true;
        self.id_buf = new_id;
        self.synced_focus = None; // re-sync buffers next frame
        self.ed.status = format!("Tagged '{}' as '{tag}'", self.ed.object_id(&obj));
    }

    fn clear_focused_tag(&mut self) {
        if let Some(obj) = self.ed.focused_object.clone() {
            let id = self.ed.object_id(&obj).to_string();
            self.ed.level.classifications.retain(|e| e.object_id != id);
            self.ed.is_dirty = true;
            self.title_dirty = true;
            self.synced_focus = None;
            self.ed.status = format!("Cleared tag for '{id}'");
        }
    }

    fn focus_step(&mut self, forward: bool) {
        let objs = self.ed.ordered_objects();
        if objs.is_empty() {
            return;
        }
        let cur = self
            .ed
            .focused_object
            .as_ref()
            .and_then(|fo| objs.iter().position(|o| o == fo));
        let next = if forward {
            cur.map_or(0, |i| (i + 1) % objs.len())
        } else {
            match cur {
                None | Some(0) => objs.len() - 1,
                Some(i) => i - 1,
            }
        };
        self.ed.focused_object = Some(objs[next].clone());
        self.synced_focus = None;
    }
}

impl eframe::App for EditorApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_dialogs(&ctx);

        if self.title_dirty {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(self.ed.window_title()));
            self.title_dirty = false;
        }

        // Global keyboard shortcuts — only when no text field is focused, so
        // typing in a TextEdit never triggers a tool/command.
        if !ctx.egui_wants_keyboard_input() {
            self.handle_shortcuts(&ctx);
        }

        egui::Panel::top("menu").show(ui, |ui| self.ui_menu_bar(&ctx, ui));
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.colored_label(col(GOLD), &self.ed.status);
            });
        });
        egui::Panel::right("tools")
            .default_size(240.0)
            .show(ui, |ui| self.ui_side_panel(&ctx, ui));
        egui::CentralPanel::default().show(ui, |ui| self.ui_canvas(&ctx, ui));

        self.ui_sheet_window(&ctx);
        self.ui_help_window(&ctx);
    }
}

impl EditorApp {
    fn poll_dialogs(&mut self, ctx: &egui::Context) {
        self.open_dialog.update(ctx);
        if let Some(p) = self.open_dialog.take_picked() {
            self.open_path(ctx, &p.to_string_lossy());
        }
        self.save_dialog.update(ctx);
        if let Some(p) = self.save_dialog.take_picked() {
            self.ed.current_path = p.to_string_lossy().to_string();
            self.save();
        }
        self.sheet_dialog.update(ctx);
        if let Some(p) = self.sheet_dialog.take_picked() {
            self.load_sheet(ctx, &p.to_string_lossy());
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let pressed = |k: egui::Key| ctx.input(|i| i.key_pressed(k));

        if pressed(egui::Key::S) {
            self.save();
        }
        if pressed(egui::Key::O) {
            self.open_dialog.pick_file();
        }
        if pressed(egui::Key::H) {
            self.show_help = !self.show_help;
        }
        if pressed(egui::Key::F) {
            self.view.target = Vec2D::ZERO;
            self.view.zoom = 1.0;
            self.ed.status = "View reset".into();
        }
        if pressed(egui::Key::Tab) {
            self.cycle_layer();
        }

        match self.ed.active_layer {
            Layer::ClassificationPlanning => {
                if pressed(egui::Key::ArrowRight) || pressed(egui::Key::ArrowDown) {
                    self.focus_step(true);
                }
                if pressed(egui::Key::ArrowLeft) || pressed(egui::Key::ArrowUp) {
                    self.focus_step(false);
                }
                if pressed(egui::Key::Delete) {
                    self.clear_focused_tag();
                }
            }
            _ => {
                let placing = self.ed.active_layer == Layer::SpritePlanning
                    && self.ed.selected_sprite.is_some();
                if !placing {
                    if pressed(egui::Key::R) {
                        self.ed.tool = Tool::Rect;
                    }
                    if pressed(egui::Key::C) {
                        self.ed.tool = Tool::Circle;
                    }
                    for (i, k) in [
                        egui::Key::Num1,
                        egui::Key::Num2,
                        egui::Key::Num3,
                        egui::Key::Num4,
                        egui::Key::Num5,
                        egui::Key::Num6,
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        if pressed(k) {
                            self.ed.color = PALETTE[i];
                        }
                    }
                }
                if self.ed.active_layer == Layer::SpritePlanning
                    && !self.ed.available_sprites.is_empty()
                {
                    let n = self.ed.available_sprites.len();
                    let cur = self.ed.selected_sprite.unwrap_or(0);
                    if pressed(egui::Key::OpenBracket) {
                        self.select_sprite_index(ctx, if cur == 0 { n - 1 } else { cur - 1 });
                    }
                    if pressed(egui::Key::CloseBracket) {
                        self.select_sprite_index(ctx, (cur + 1) % n);
                    }
                    if pressed(egui::Key::Comma) {
                        self.ed.sprite_scale = (self.ed.sprite_scale / 2.0).max(0.25);
                    }
                    if pressed(egui::Key::Period) {
                        self.ed.sprite_scale = (self.ed.sprite_scale * 2.0).min(8.0);
                    }
                }
                if pressed(egui::Key::Z) {
                    self.undo();
                }
                if pressed(egui::Key::X) {
                    self.clear_layer();
                }
                if pressed(egui::Key::Delete) || pressed(egui::Key::Backspace) {
                    self.delete_selected();
                }
            }
        }
    }

    fn cycle_layer(&mut self) {
        self.ed.active_layer = match self.ed.active_layer {
            Layer::SpritePlanning => Layer::CollisionPlanning,
            Layer::CollisionPlanning => Layer::ClassificationPlanning,
            Layer::ClassificationPlanning => Layer::SpritePlanning,
        };
        self.ed.color = Editor::default_layer_color(self.ed.active_layer);
        self.ed.drag_start = None;
        self.ed.drag_action = None;
        self.ed.selected_shape = None;
        self.ed.focused_object = None;
        self.title_dirty = true;
        self.ed.status = format!("Active layer: {}", self.ed.active_layer_name());
    }

    fn undo(&mut self) {
        if self.ed.active_layer == Layer::SpritePlanning
            && self.ed.level.sprite_instances.pop().is_some()
        {
            let c = self.ed.level.sprite_instances.len();
            self.ed.status = format!("Undid last sprite ({c} left)");
        } else {
            let shapes = self.ed.active_shapes_mut();
            if shapes.pop().is_some() {
                let c = shapes.len();
                self.ed.selected_shape = None;
                self.ed.status = format!("Undid last shape ({c} left)");
            } else {
                return;
            }
        }
        self.ed.is_dirty = true;
        self.title_dirty = true;
    }

    fn clear_layer(&mut self) {
        let name = self.ed.active_layer_name();
        if self.ed.active_layer == Layer::ClassificationPlanning {
            if !self.ed.level.classifications.is_empty() {
                self.ed.level.classifications.clear();
                self.ed.is_dirty = true;
                self.title_dirty = true;
                self.ed.status = "Cleared all classifications".into();
            }
            return;
        }
        let shapes = self.ed.active_shapes_mut();
        if !shapes.is_empty() {
            shapes.clear();
            self.ed.selected_shape = None;
            self.ed.is_dirty = true;
            self.title_dirty = true;
            self.ed.status = format!("Cleared {name} layer");
        }
    }

    fn delete_selected(&mut self) {
        if let Some(i) = self.ed.selected_shape.take() {
            let shapes = self.ed.active_shapes_mut();
            if i < shapes.len() {
                shapes.remove(i);
                let c = shapes.len();
                self.ed.is_dirty = true;
                self.title_dirty = true;
                self.ed.status = format!("Deleted selected shape ({c} left)");
            }
        }
    }

    fn set_layer(&mut self, layer: Layer) {
        if self.ed.active_layer == layer {
            return;
        }
        self.ed.active_layer = layer;
        self.ed.color = Editor::default_layer_color(layer);
        self.ed.drag_start = None;
        self.ed.drag_action = None;
        self.ed.selected_shape = None;
        self.ed.focused_object = None;
        self.title_dirty = true;
        self.ed.status = format!("Active layer: {}", self.ed.active_layer_name());
    }

    // ------------------------------------------------------------------ menus
    fn ui_menu_bar(&mut self, _ctx: &egui::Context, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Open…").clicked() {
                    self.open_dialog.pick_file();
                    ui.close();
                }
                if ui.button("Save").clicked() {
                    self.save();
                    ui.close();
                }
                if ui.button("Save As…").clicked() {
                    self.save_dialog.save_file();
                    ui.close();
                }
            });
            ui.separator();
            ui.label("Layer:");
            for (layer, name) in [
                (Layer::SpritePlanning, "Sprite"),
                (Layer::CollisionPlanning, "Collision"),
                (Layer::ClassificationPlanning, "Classification"),
            ] {
                if ui
                    .selectable_label(self.ed.active_layer == layer, name)
                    .clicked()
                {
                    self.set_layer(layer);
                }
            }
            ui.separator();
            if ui
                .selectable_label(self.arming_spawn, "Set player spawn")
                .clicked()
            {
                self.arming_spawn = !self.arming_spawn;
                self.ed.status = if self.arming_spawn {
                    "Click the canvas to set the player spawn".into()
                } else {
                    "Spawn placement cancelled".into()
                };
            }
            match self.ed.level.player_start {
                Some(p) => ui.label(format!("spawn {:.0},{:.0}", p.x, p.y)),
                None => ui.label("spawn unset"),
            };
            ui.separator();
            ui.label("Grid");
            if ui
                .add(
                    egui::DragValue::new(&mut self.ed.level.grid_size)
                        .range(1.0..=512.0)
                        .speed(1.0)
                        .suffix(" px"),
                )
                .changed()
            {
                self.ed.is_dirty = true;
            }
            ui.separator();
            ui.checkbox(&mut self.show_help, "Help");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.ed.is_dirty {
                    ui.colored_label(col(ORANGE), "● Unsaved");
                } else {
                    ui.colored_label(col(LIME), "Saved");
                }
                ui.label(self.ed.current_file_label());
            });
        });
    }

    // ------------------------------------------------------------- side panel
    fn ui_side_panel(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.heading(self.ed.active_layer_name());
        ui.separator();
        match self.ed.active_layer {
            Layer::SpritePlanning => {
                self.ui_sprite_controls(ctx, ui);
                ui.separator();
                self.ui_shape_controls(ui);
            }
            Layer::CollisionPlanning => self.ui_shape_controls(ui),
            Layer::ClassificationPlanning => self.ui_classification_controls(ui),
        }
    }

    fn ui_shape_controls(&mut self, ui: &mut egui::Ui) {
        ui.label("Tool");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.ed.tool, Tool::Rect, "Rect");
            ui.selectable_value(&mut self.ed.tool, Tool::Circle, "Circle");
        });
        ui.label("Color");
        ui.horizontal_wrapped(|ui| {
            for c in PALETTE {
                let selected = self.ed.color.r == c.r && self.ed.color.g == c.g && self.ed.color.b == c.b;
                let (rect_resp, painter) =
                    ui.allocate_painter(egui::vec2(26.0, 22.0), egui::Sense::click());
                painter.rect_filled(rect_resp.rect, egui::CornerRadius::same(3), col(c));
                if selected {
                    painter.rect_stroke(
                        rect_resp.rect,
                        egui::CornerRadius::same(3),
                        egui::Stroke::new(2.0, col(WHITE)),
                        egui::StrokeKind::Inside,
                    );
                }
                if rect_resp.clicked() {
                    self.ed.color = c;
                }
            }
        });
        ui.add_space(6.0);
        ui.label(
            "Drag empty space to draw · click a shape to select · drag it to move · \
             Shift-drag to resize · right-click to delete",
        );
        self.ui_selected_shape(ui);
    }

    /// Numeric editor for the currently-selected shape on the active shape
    /// layer: precise position/size fields plus a delete button. Edits are
    /// snapped to nothing (exact), complementing the grid-snapped mouse tools.
    fn ui_selected_shape(&mut self, ui: &mut egui::Ui) {
        let Some(i) = self.ed.selected_shape else {
            return;
        };
        // Selection can dangle after a delete/clear; drop it cleanly.
        if self.ed.active_shapes().get(i).is_none() {
            self.ed.selected_shape = None;
            return;
        }
        ui.separator();
        ui.label(format!("Selected shape #{i}"));

        let mut changed = false;
        if let Some(shape) = self.ed.active_shapes_mut().get_mut(i) {
            match shape {
                Shape::Rect {
                    x, y, width, height, ..
                } => {
                    changed |= num_field(ui, "x", x, f32::MIN);
                    changed |= num_field(ui, "y", y, f32::MIN);
                    changed |= num_field(ui, "w", width, 1.0);
                    changed |= num_field(ui, "h", height, 1.0);
                }
                Shape::Circle { x, y, radius, .. } => {
                    changed |= num_field(ui, "x", x, f32::MIN);
                    changed |= num_field(ui, "y", y, f32::MIN);
                    changed |= num_field(ui, "radius", radius, 1.0);
                }
            }
        }
        if ui.button("Delete shape").clicked() {
            self.ed.active_shapes_mut().remove(i);
            self.ed.selected_shape = None;
            changed = true;
        }
        if changed {
            self.ed.is_dirty = true;
            self.title_dirty = true;
        }
    }

    fn ui_sprite_controls(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        if ui.button("Load spritesheet…").clicked() {
            self.sheet_dialog.pick_file();
        }
        ui.add(
            egui::Slider::new(&mut self.ed.sprite_scale, 0.25..=8.0)
                .text("scale")
                .logarithmic(true),
        );
        ui.checkbox(&mut self.ed.place_background, "Place behind everything")
            .on_hover_text("Sprites placed while this is on are flagged as background and always drawn behind every other sprite and the player.");
        ui.separator();
        if self.ed.available_sprites.is_empty() {
            ui.label("No sprites in sprites/");
            return;
        }
        ui.label("Sprites (click to select, click canvas to place)");
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for i in 0..self.ed.available_sprites.len() {
                let name = std::path::Path::new(&self.ed.available_sprites[i])
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?")
                    .to_string();
                if ui
                    .selectable_label(self.ed.selected_sprite == Some(i), name)
                    .clicked()
                {
                    self.select_sprite_index(ctx, i);
                }
            }
        });
    }

    fn ui_classification_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("◀ Prev").clicked() {
                self.focus_step(false);
            }
            if ui.button("Next ▶").clicked() {
                self.focus_step(true);
            }
        });
        ui.separator();

        if let Some(obj) = self.ed.focused_object.clone() {
            // Re-sync buffers when focus changes.
            if self.synced_focus.as_ref() != Some(&obj) {
                self.id_buf = self.ed.object_id(&obj).to_string();
                self.tag_buf = self.ed.object_tag(&obj).to_string();
                self.synced_focus = Some(obj.clone());
            }
            ui.label("ID");
            ui.text_edit_singleline(&mut self.id_buf);
            ui.label("Tag");
            ui.text_edit_singleline(&mut self.tag_buf);
            ui.horizontal_wrapped(|ui| {
                let mut tags: Vec<String> = self.ed.tag_colors.keys().cloned().collect();
                tags.sort();
                for t in tags {
                    if ui.small_button(&t).clicked() {
                        self.tag_buf = t;
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button("Apply").clicked() {
                    self.apply_classification();
                }
                if ui.button("Clear tag").clicked() {
                    self.clear_focused_tag();
                }
            });
        } else {
            ui.label("Click an object in the canvas to edit its tag.");
        }

        ui.separator();
        ui.label("Tags");
        let mut tags: Vec<(String, Color)> =
            self.ed.tag_colors.iter().map(|(k, v)| (k.clone(), *v)).collect();
        tags.sort_by(|a, b| a.0.cmp(&b.0));
        for (tag, color) in tags {
            ui.horizontal(|ui| {
                let (r, p) = ui.allocate_painter(egui::vec2(14.0, 14.0), egui::Sense::hover());
                p.rect_filled(r.rect, egui::CornerRadius::same(2), col(color));
                ui.colored_label(col(color), tag);
            });
        }
    }

    // ----------------------------------------------------------------- canvas
    fn ui_canvas(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let (response, painter) =
            ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
        let rect = response.rect;
        painter.rect_filled(rect, egui::CornerRadius::ZERO, col(DARKGRAY));

        // Pan with the middle button.
        if response.dragged_by(egui::PointerButton::Middle) {
            let d = response.drag_delta();
            self.view.target -= Vec2D::new(d.x, d.y) / self.view.zoom;
        }
        // Zoom toward the cursor with the wheel.
        if response.hovered() {
            let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if let Some(p) = response.hover_pos() {
                    let before = self.view.screen_to_world(rect, p);
                    self.view.zoom = (self.view.zoom * (1.0 + scroll.signum() * 0.1)).clamp(0.1, 10.0);
                    let after = self.view.screen_to_world(rect, p);
                    self.view.target += before - after;
                }
            }
        }

        let pointer = response.interact_pointer_pos().or(response.hover_pos());
        let world = pointer.map(|p| self.view.screen_to_world(rect, p));
        let snapped = world.map(|w| snap(w, self.ed.level.grid_size));

        self.canvas_interact(ctx, &response, world, snapped);
        self.paint_world(&painter, rect, world);
    }

    fn canvas_interact(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        world: Option<Vec2D>,
        snapped: Option<Vec2D>,
    ) {
        let (Some(world), Some(snapped)) = (world, snapped) else {
            return;
        };

        // Player-spawn placement takes precedence over any layer interaction so
        // the same click doesn't also place a shape/sprite.
        if self.arming_spawn {
            if response.clicked_by(egui::PointerButton::Primary) {
                self.ed.level.player_start = Some(SpawnPoint {
                    x: snapped.x,
                    y: snapped.y,
                });
                self.ed.is_dirty = true;
                self.title_dirty = true;
                self.arming_spawn = false;
                self.ed.status = format!("Player spawn set to {:.0},{:.0}", snapped.x, snapped.y);
            }
            return;
        }

        match self.ed.active_layer {
            Layer::ClassificationPlanning => {
                if response.clicked_by(egui::PointerButton::Primary) {
                    if let Some(obj) = self.ed.classify_object_at(world) {
                        self.ed.focused_object = Some(obj);
                        self.synced_focus = None;
                        self.ed.status = "Focused object — edit its tag in the panel".into();
                    }
                }
                if response.clicked_by(egui::PointerButton::Secondary) {
                    if let Some(obj) = self.ed.classify_object_at(world) {
                        let id = self.ed.object_id(&obj).to_string();
                        self.ed.focused_object = Some(obj);
                        self.synced_focus = None;
                        self.ed.level.classifications.retain(|e| e.object_id != id);
                        self.ed.is_dirty = true;
                        self.title_dirty = true;
                        self.ed.status = format!("Cleared tag for '{id}'");
                    }
                }
            }
            _ => {
                let placing = self.ed.active_layer == Layer::SpritePlanning
                    && self.ed.selected_sprite.is_some();
                if placing {
                    if response.clicked_by(egui::PointerButton::Primary) {
                        if let Some(idx) = self.ed.selected_sprite {
                            let path = self.ed.available_sprites[idx].clone();
                            self.ensure_texture(ctx, &path);
                            self.ed.level.sprite_instances.push(SpriteInstance {
                                id: random_id(),
                                path,
                                x: snapped.x,
                                y: snapped.y,
                                scale: self.ed.sprite_scale,
                                background: self.ed.place_background,
                            });
                            self.ed.is_dirty = true;
                            self.title_dirty = true;
                            let n = self.ed.level.sprite_instances.len();
                            self.ed.status = format!("Placed sprite ({n} total)");
                        }
                    }
                    if response.clicked_by(egui::PointerButton::Secondary) {
                        if let Some(i) = self.ed.sprite_at(world) {
                            self.ed.level.sprite_instances.remove(i);
                            self.ed.is_dirty = true;
                            self.title_dirty = true;
                            let n = self.ed.level.sprite_instances.len();
                            self.ed.status = format!("Deleted sprite ({n} left)");
                        }
                    }
                } else {
                    // Plain click selects (or clears) the shape under the cursor
                    // without changing its geometry.
                    if response.clicked_by(egui::PointerButton::Primary) {
                        self.ed.selected_shape =
                            self.ed.active_shapes().iter().rposition(|s| s.contains(world));
                    }
                    if response.drag_started_by(egui::PointerButton::Primary) {
                        let hit = self.ed.active_shapes().iter().rposition(|s| s.contains(world));
                        self.ed.selected_shape = hit;
                        self.ed.drag_start = Some(snapped);
                        // Dragging a shape moves it; hold Shift to resize/redraw
                        // it instead. Dragging empty space draws a new shape.
                        let resize = ctx.input(|i| i.modifiers.shift);
                        self.ed.drag_action = Some(match hit {
                            Some(i) if resize => DragAction::RedrawShape(i),
                            Some(i) => DragAction::MoveShape(i),
                            None => DragAction::NewShape,
                        });
                    }
                    if response.drag_stopped_by(egui::PointerButton::Primary) {
                        if let (Some(start), Some(action)) =
                            (self.ed.drag_start.take(), self.ed.drag_action.take())
                        {
                            match action {
                                DragAction::NewShape => {
                                    if let Some(shape) =
                                        make_shape(self.ed.tool, start, snapped, self.ed.color)
                                    {
                                        let shapes = self.ed.active_shapes_mut();
                                        shapes.push(shape);
                                        let n = shapes.len();
                                        self.ed.is_dirty = true;
                                        self.title_dirty = true;
                                        self.ed.status = format!("Placed shape ({n} total)");
                                    }
                                }
                                DragAction::MoveShape(i) => {
                                    let delta = snapped - start;
                                    if delta != Vec2D::ZERO {
                                        if let Some(s) = self.ed.active_shapes_mut().get_mut(i) {
                                            translate_shape(s, delta);
                                            self.ed.is_dirty = true;
                                            self.title_dirty = true;
                                            self.ed.status = format!("Moved shape {i}");
                                        }
                                    }
                                    self.ed.selected_shape = Some(i);
                                }
                                DragAction::RedrawShape(i) => {
                                    let ok = {
                                        let shapes = self.ed.active_shapes_mut();
                                        shapes
                                            .get_mut(i)
                                            .is_some_and(|s| update_shape_geometry(s, start, snapped))
                                    };
                                    if ok {
                                        self.ed.is_dirty = true;
                                        self.title_dirty = true;
                                        self.ed.status = format!("Redrawn shape {i}");
                                    }
                                    self.ed.selected_shape = Some(i);
                                }
                            }
                        }
                    }
                    if response.clicked_by(egui::PointerButton::Secondary) {
                        let hit = self.ed.active_shapes().iter().rposition(|s| s.contains(world));
                        if let Some(i) = hit {
                            let n = {
                                let shapes = self.ed.active_shapes_mut();
                                shapes.remove(i);
                                shapes.len()
                            };
                            match self.ed.selected_shape {
                                Some(s) if s == i => self.ed.selected_shape = None,
                                Some(s) if s > i => self.ed.selected_shape = Some(s - 1),
                                _ => {}
                            }
                            self.ed.is_dirty = true;
                            self.title_dirty = true;
                            self.ed.status = format!("Deleted shape ({n} left)");
                        }
                    }
                }
            }
        }
    }

    // --------------------------------------------------------------- painting
    fn paint_world(&self, painter: &egui::Painter, rect: egui::Rect, world: Option<Vec2D>) {
        self.paint_grid(painter, rect);

        let sprite_alpha = match self.ed.active_layer {
            Layer::SpritePlanning => 1.0,
            Layer::ClassificationPlanning => 0.6,
            Layer::CollisionPlanning => 0.30,
        };
        // Background sprites first (behind everything), then the rest — matching
        // the game's draw order.
        for background in [true, false] {
            for inst in self
                .ed
                .level
                .sprite_instances
                .iter()
                .filter(|s| s.background == background)
            {
                if let Some(tex) = self.ed.sprite_cache.get(&inst.path) {
                    let [tw, th] = tex.size();
                    let p0 = self.view.world_to_screen(rect, Vec2D::new(inst.x, inst.y));
                    let p1 = self.view.world_to_screen(
                        rect,
                        Vec2D::new(
                            inst.x + tw as f32 * inst.scale,
                            inst.y + th as f32 * inst.scale,
                        ),
                    );
                    painter.image(
                        tex.id(),
                        egui::Rect::from_min_max(p0, p1),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        col(WHITE.with_alpha(sprite_alpha)),
                    );
                }
            }
        }

        if self.ed.active_layer == Layer::ClassificationPlanning {
            self.paint_classification(painter, rect);
        } else {
            self.paint_shapes(painter, rect, self.ed.inactive_shapes(), 0.30);
            self.paint_shapes(painter, rect, self.ed.active_shapes(), 1.0);

            if let Some(i) = self.ed.selected_shape {
                if let Some(s) = self.ed.active_shapes().get(i) {
                    let (x, y, w, h) = s.bounding_rect();
                    self.outline(painter, rect, x, y, w, h, 2.0, WHITE);
                }
            }

            // Live preview.
            if let Some(world) = world {
                let end = snap(world, self.ed.level.grid_size);
                if self.ed.active_layer == Layer::SpritePlanning {
                    if let Some(idx) = self.ed.selected_sprite {
                        if let Some(path) = self.ed.available_sprites.get(idx) {
                            if let Some(tex) = self.ed.sprite_cache.get(path) {
                                let [tw, th] = tex.size();
                                let p0 = self.view.world_to_screen(rect, end);
                                let p1 = self.view.world_to_screen(
                                    rect,
                                    end + Vec2D::new(
                                        tw as f32 * self.ed.sprite_scale,
                                        th as f32 * self.ed.sprite_scale,
                                    ),
                                );
                                painter.image(
                                    tex.id(),
                                    egui::Rect::from_min_max(p0, p1),
                                    egui::Rect::from_min_max(
                                        egui::pos2(0.0, 0.0),
                                        egui::pos2(1.0, 1.0),
                                    ),
                                    col(WHITE.with_alpha(0.45)),
                                );
                            }
                        }
                    }
                } else if let Some(start) = self.ed.drag_start {
                    match self.ed.drag_action {
                        Some(DragAction::MoveShape(i)) => {
                            if let Some(mut s) = self.ed.active_shapes().get(i).cloned() {
                                translate_shape(&mut s, end - start);
                                self.paint_shapes(painter, rect, std::slice::from_ref(&s), 0.5);
                            }
                        }
                        Some(DragAction::RedrawShape(i)) => {
                            if let Some(mut s) = self.ed.active_shapes().get(i).cloned() {
                                if update_shape_geometry(&mut s, start, end) {
                                    self.paint_shapes(painter, rect, std::slice::from_ref(&s), 0.5);
                                }
                            }
                        }
                        _ => {
                            if let Some(s) = make_shape(self.ed.tool, start, end, self.ed.color) {
                                self.paint_shapes(painter, rect, std::slice::from_ref(&s), 0.5);
                            }
                        }
                    }
                }
            }
        }

        // Player spawn marker (shown on every layer): a box the size of the
        // game's player hit-box, anchored at the spawn's top-left, plus a label.
        if let Some(p) = self.ed.level.player_start {
            self.outline(
                painter,
                rect,
                p.x,
                p.y,
                PLAYER_MARKER_SIZE,
                PLAYER_MARKER_SIZE,
                2.0,
                MAGENTA,
            );
            let tl = self.view.world_to_screen(rect, Vec2D::new(p.x, p.y));
            painter.text(
                tl + egui::vec2(3.0, 2.0),
                egui::Align2::LEFT_TOP,
                "Player",
                egui::FontId::proportional(13.0 * self.view.zoom),
                col(MAGENTA),
            );
        }

        // Crosshair at the snapped cursor.
        if let Some(world) = world {
            let sp = self.view.world_to_screen(rect, snap(world, self.ed.level.grid_size));
            let st = egui::Stroke::new(1.0, col(WHITE));
            painter.line_segment([sp - egui::vec2(10.0, 0.0), sp + egui::vec2(10.0, 0.0)], st);
            painter.line_segment([sp - egui::vec2(0.0, 10.0), sp + egui::vec2(0.0, 10.0)], st);
        }
    }

    fn paint_grid(&self, painter: &egui::Painter, rect: egui::Rect) {
        let grid = self.ed.level.grid_size.max(1.0);
        let tl = self.view.screen_to_world(rect, rect.min);
        let br = self.view.screen_to_world(rect, rect.max);
        let min_x = (tl.x / grid).floor() as i32;
        let max_x = (br.x / grid).ceil() as i32;
        let min_y = (tl.y / grid).floor() as i32;
        let max_y = (br.y / grid).ceil() as i32;
        let line = |major: bool| {
            egui::Stroke::new(1.0, col(LIGHTGRAY.with_alpha(if major { 0.30 } else { 0.12 })))
        };
        for ix in min_x..=max_x {
            let x = ix as f32 * grid;
            let a = self.view.world_to_screen(rect, Vec2D::new(x, tl.y));
            let b = self.view.world_to_screen(rect, Vec2D::new(x, br.y));
            painter.line_segment([a, b], line(ix.rem_euclid(GRID_MAJOR_EVERY) == 0));
        }
        for iy in min_y..=max_y {
            let y = iy as f32 * grid;
            let a = self.view.world_to_screen(rect, Vec2D::new(tl.x, y));
            let b = self.view.world_to_screen(rect, Vec2D::new(br.x, y));
            painter.line_segment([a, b], line(iy.rem_euclid(GRID_MAJOR_EVERY) == 0));
        }
    }

    fn paint_shapes(&self, painter: &egui::Painter, rect: egui::Rect, shapes: &[Shape], alpha: f32) {
        for s in shapes {
            match s {
                Shape::Rect {
                    x,
                    y,
                    width,
                    height,
                    color,
                    ..
                } => {
                    let p0 = self.view.world_to_screen(rect, Vec2D::new(*x, *y));
                    let p1 = self.view.world_to_screen(rect, Vec2D::new(x + width, y + height));
                    painter.rect_filled(
                        egui::Rect::from_min_max(p0, p1),
                        egui::CornerRadius::ZERO,
                        col(color.with_alpha(alpha)),
                    );
                }
                Shape::Circle {
                    x,
                    y,
                    radius,
                    color,
                    ..
                } => {
                    let c = self.view.world_to_screen(rect, Vec2D::new(*x, *y));
                    painter.circle_filled(c, radius * self.view.zoom, col(color.with_alpha(alpha)));
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn outline(
        &self,
        painter: &egui::Painter,
        rect: egui::Rect,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        thick: f32,
        color: Color,
    ) {
        let p0 = self.view.world_to_screen(rect, Vec2D::new(x, y));
        let p1 = self.view.world_to_screen(rect, Vec2D::new(x + w, y + h));
        painter.rect_stroke(
            egui::Rect::from_min_max(p0, p1),
            egui::CornerRadius::ZERO,
            egui::Stroke::new(thick, col(color)),
            egui::StrokeKind::Inside,
        );
    }

    fn paint_classification(&self, painter: &egui::Painter, rect: egui::Rect) {
        self.paint_shapes(painter, rect, &self.ed.level.sprite_shapes, 0.20);
        self.paint_shapes(painter, rect, &self.ed.level.collision_shapes, 0.25);

        let mut labels: Vec<LabelSpec> = Vec::new();
        let mut push = |obj: ObjectRef, id: &str, bx: f32, by: f32, bw: f32, bh: f32, prefer_above: bool| {
            let tag = self
                .ed
                .level
                .classifications
                .iter()
                .find(|e| e.object_id == id)
                .map(|e| e.tag.as_str())
                .unwrap_or("static");
            let tag_color = self.ed.tag_colors.get(tag).copied().unwrap_or(LIGHTGRAY);
            let focused = self.ed.focused_object.as_ref() == Some(&obj);
            let (thick, box_color) = if focused {
                (2.5, GOLD)
            } else {
                (1.5, tag_color.with_alpha(0.9))
            };
            self.outline(painter, rect, bx, by, bw, bh, thick, box_color);
            let text = build_label_text(id, tag);
            labels.push(LabelSpec {
                x: bx,
                y: if prefer_above { by - 17.0 } else { by + bh + 3.0 },
                w: (text.len() as f32 * 7.5 + 8.0).max(40.0),
                text,
                text_color: if focused { GOLD } else { tag_color },
                bg_color: BLACK.with_alpha(if focused { 0.88 } else { 0.70 }),
            });
        };

        for i in 0..self.ed.level.sprite_instances.len() {
            let inst = &self.ed.level.sprite_instances[i];
            let (bx, by, bw, bh) = self.ed.sprite_bounding_rect(inst);
            push(ObjectRef::Sprite(i), &inst.id.clone(), bx, by, bw, bh, true);
        }
        for i in 0..self.ed.level.collision_shapes.len() {
            let id = self.ed.level.collision_shapes[i].id().to_string();
            let (bx, by, bw, bh) = self.ed.level.collision_shapes[i].bounding_rect();
            push(ObjectRef::CollisionShape(i), &id, bx, by, bw, bh, false);
        }

        resolve_label_overlaps(&mut labels);
        let z = self.view.zoom;
        for spec in &labels {
            let p0 = self.view.world_to_screen(rect, Vec2D::new(spec.x, spec.y));
            let p1 = self.view.world_to_screen(rect, Vec2D::new(spec.x + spec.w, spec.y + LABEL_H));
            painter.rect_filled(
                egui::Rect::from_min_max(p0, p1),
                egui::CornerRadius::ZERO,
                col(spec.bg_color),
            );
            painter.text(
                p0 + egui::vec2(3.0, 1.0),
                egui::Align2::LEFT_TOP,
                &spec.text,
                egui::FontId::proportional(12.0 * z),
                col(spec.text_color),
            );
        }
    }

    // ---------------------------------------------------------------- windows
    fn ui_sheet_window(&mut self, ctx: &egui::Context) {
        let mut open = self.sheet.is_some();
        if !open {
            return;
        }
        egui::Window::new("Spritesheet / tileset")
            .open(&mut open)
            .default_size([700.0, 520.0])
            .show(ctx, |ui| {
                let Some(sheet) = self.sheet.as_mut() else {
                    return;
                };
                ui.horizontal(|ui| {
                    ui.label("Grid");
                    ui.add(
                        egui::DragValue::new(&mut sheet.grid)
                            .range(1..=512)
                            .speed(1.0)
                            .suffix(" px"),
                    );
                    if ui.small_button("½").clicked() {
                        sheet.grid = (sheet.grid / 2).max(1);
                    }
                    if ui.small_button("2×").clicked() {
                        sheet.grid = (sheet.grid * 2).min(512);
                    }
                    ui.label("· drag to select whole cells, release to cut");
                });

                // Fit the sheet into the available area.
                let avail = ui.available_size();
                let scale = (avail.x / sheet.width as f32)
                    .min(avail.y / sheet.height as f32)
                    .max(0.05);
                let size = egui::vec2(sheet.width as f32 * scale, sheet.height as f32 * scale);
                let (response, painter) = ui.allocate_painter(size, egui::Sense::click_and_drag());
                let r = response.rect;
                painter.image(
                    sheet.texture.id(),
                    r,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );

                // Grid overlay (sheet-pixel cells, drawn in screen space).
                let g = sheet.grid.max(1) as f32;
                if g > 1.0 {
                    let grid_stroke = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(40));
                    let mut x = 0.0;
                    while x <= sheet.width as f32 {
                        let sx = r.min.x + x * scale;
                        painter.line_segment(
                            [egui::pos2(sx, r.min.y), egui::pos2(sx, r.max.y)],
                            grid_stroke,
                        );
                        x += g;
                    }
                    let mut y = 0.0;
                    while y <= sheet.height as f32 {
                        let sy = r.min.y + y * scale;
                        painter.line_segment(
                            [egui::pos2(r.min.x, sy), egui::pos2(r.max.x, sy)],
                            grid_stroke,
                        );
                        y += g;
                    }
                }

                // Screen -> sheet pixel.
                let to_px = |p: egui::Pos2| {
                    egui::pos2(
                        ((p.x - r.min.x) / scale).clamp(0.0, sheet.width as f32),
                        ((p.y - r.min.y) / scale).clamp(0.0, sheet.height as f32),
                    )
                };
                let snap_selection = |a: egui::Pos2, b: egui::Pos2| -> Rect {
                    snap_tile_selection(
                        Vec2D::new(a.x, a.y),
                        Vec2D::new(b.x, b.y),
                        sheet.grid,
                        sheet.width,
                        sheet.height,
                    )
                };

                if response.drag_started() {
                    sheet.drag_start = response.hover_pos().map(to_px);
                }
                let cur = response.hover_pos().map(to_px);

                // Live snapped preview.
                if let (Some(s), Some(c)) = (sheet.drag_start, cur) {
                    let sel = snap_selection(s, c);
                    let scr = egui::Rect::from_min_size(
                        r.min + egui::vec2(sel.x, sel.y) * scale,
                        egui::vec2(sel.width, sel.height) * scale,
                    );
                    painter.rect_stroke(
                        scr,
                        egui::CornerRadius::ZERO,
                        egui::Stroke::new(2.0, egui::Color32::YELLOW),
                        egui::StrokeKind::Inside,
                    );
                }

                if response.drag_stopped() {
                    if let (Some(s), Some(c)) = (sheet.drag_start.take(), cur) {
                        let selection = snap_selection(s, c);
                        if selection.width >= 1.0 && selection.height >= 1.0 {
                            match crop_and_save_sprite(&sheet.path, selection, "sprites") {
                                Ok(out) => {
                                    self.ed.add_and_select_sprite_path(out.clone());
                                    self.ensure_texture(ctx, &out);
                                    self.ed.is_dirty = true;
                                    self.title_dirty = true;
                                    self.ed.status = format!(
                                        "Cut '{}' ({}×{})",
                                        self.ed.selected_sprite_name(),
                                        selection.width as u32,
                                        selection.height as u32,
                                    );
                                }
                                Err(e) => self.ed.status = format!("Cut failed: {e}"),
                            }
                        }
                    }
                }
            });
        if !open {
            self.sheet = None;
        }
    }

    fn ui_help_window(&mut self, ctx: &egui::Context) {
        let mut open = self.show_help;
        egui::Window::new("Editor controls")
            .open(&mut open)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label("Mouse");
                ui.label("  L-drag        place new shape (shape layers)");
                ui.label("  L-click shape select / drag to redraw");
                ui.label("  Click         place sprite (sprite selected)");
                ui.label("  R-click       delete under cursor");
                ui.label("  M-drag        pan · Wheel zoom");
                ui.separator();
                ui.label("Keys (canvas focused)");
                ui.label("  Tab cycle layer · F reset view · H help");
                ui.label("  R/C tool · 1-6 color");
                ui.label("  [ ] cycle sprite · , . scale");
                ui.label("  Z undo · X clear layer · Del delete");
                ui.label("  S save · O open");
                ui.separator();
                ui.label("Classification: click an object, edit ID/tag in the panel.");
            });
        self.show_help = open;
    }
}

/// Convenience for `main` to compute a default level path.
pub(crate) fn default_level_path(arg: Option<String>) -> PathBuf {
    PathBuf::from(arg.unwrap_or_else(|| "level.json".to_string()))
}

