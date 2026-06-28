//! A simple, serializable **level format** shared by the game and the editor.
//!
//! A [`Level`] stores two ordered lists of [`Shape`]s in **world coordinates** —
//! the space a [`Camera2D`](crate::Camera2D) looks at. The editor uses one layer
//! for sprite planning, one for collision-box planning, and one for classification
//! (tagging objects with semantic labels like "static", "movable", etc.). The game
//! reads the same file and renders the sprite-planning layer with [`Level::draw`].
//! The on-disk form is plain JSON, so levels are hand-editable and diff-friendly.

use crate::canvas::Canvas;
use crate::color::Color;
use crate::math::Vec2D;
use serde::{Deserialize, Serialize};

/// Default level file path, relative to the working directory.
pub const DEFAULT_LEVEL_PATH: &str = "level.json";

/// A single placed primitive. Coordinates are world-space pixels.
///
/// Each shape carries a free-form string `id`. IDs are not required to be
/// unique — multiple objects (e.g. a sprite and its collision box) can share
/// the same ID so they also share the same classification tag automatically.
/// Call [`Level::ensure_ids`] after loading to fill in any empty IDs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Shape {
    /// Axis-aligned rectangle with top-left at `(x, y)`.
    Rect {
        id: String,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        color: Color,
    },
    /// Circle centered at `(x, y)`.
    Circle {
        id: String,
        x: f32,
        y: f32,
        radius: f32,
        color: Color,
    },
}

impl Shape {
    /// The object ID of this shape (empty string = unassigned).
    pub fn id(&self) -> &str {
        match self {
            Shape::Rect { id, .. } => id.as_str(),
            Shape::Circle { id, .. } => id.as_str(),
        }
    }

    /// Replace this shape's ID.
    pub fn set_id(&mut self, new_id: String) {
        match self {
            Shape::Rect { id, .. } => *id = new_id,
            Shape::Circle { id, .. } => *id = new_id,
        }
    }

    /// The shape's authored fill colour (used by the editor's palette and, at
    /// load time, to derive a classification tag).
    pub fn color(&self) -> Color {
        match self {
            Shape::Rect { color, .. } | Shape::Circle { color, .. } => *color,
        }
    }

    /// Bounding rectangle as `(x, y, width, height)` in world-space pixels.
    pub fn bounding_rect(&self) -> (f32, f32, f32, f32) {
        match self {
            Shape::Rect {
                x,
                y,
                width,
                height,
                ..
            } => (*x, *y, *width, *height),
            Shape::Circle { x, y, radius, .. } => {
                (x - radius, y - radius, 2.0 * radius, 2.0 * radius)
            }
        }
    }

    /// Draw this shape into `canvas`.
    pub fn draw(&self, canvas: &mut Canvas) {
        match self {
            Shape::Rect {
                x,
                y,
                width,
                height,
                color,
                ..
            } => canvas.rectangle(*x, *y, *width, *height, *color),
            Shape::Circle {
                x,
                y,
                radius,
                color,
                ..
            } => canvas.circle(Vec2D::new(*x, *y), *radius, *color),
        }
    }

    /// Return a clone of this shape with its color alpha scaled to `alpha`.
    pub fn with_alpha(&self, alpha: f32) -> Self {
        match self {
            Shape::Rect {
                id,
                x,
                y,
                width,
                height,
                color,
            } => Shape::Rect {
                id: id.clone(),
                x: *x,
                y: *y,
                width: *width,
                height: *height,
                color: color.with_alpha(alpha),
            },
            Shape::Circle {
                id,
                x,
                y,
                radius,
                color,
            } => Shape::Circle {
                id: id.clone(),
                x: *x,
                y: *y,
                radius: *radius,
                color: color.with_alpha(alpha),
            },
        }
    }

    /// `true` if `p` (world-space pixels) lies inside the shape.
    pub fn contains(&self, p: Vec2D) -> bool {
        match self {
            Shape::Rect {
                x,
                y,
                width,
                height,
                ..
            } => p.x >= *x && p.x <= *x + *width && p.y >= *y && p.y <= *y + *height,
            Shape::Circle { x, y, radius, .. } => Vec2D::new(*x, *y).distance(p) <= *radius,
        }
    }
}

/// A placed sprite image in the level.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpriteInstance {
    /// Free-form object ID. May be shared with other objects (e.g. a collision
    /// box that belongs to the same logical entity). Empty = unassigned.
    pub id: String,
    /// Path to the PNG, relative to the workspace root.
    pub path: String,
    /// Top-left position in world-space pixels.
    pub x: f32,
    pub y: f32,
    /// Uniform scale factor (1.0 = native pixel size).
    pub scale: f32,
    /// When `true`, this sprite is part of the flat background and is always
    /// drawn behind every other sprite and the player (it sits out of any
    /// depth/Y-sort). Defaults to `false` for older level files.
    #[serde(default)]
    pub background: bool,
}

/// The player's spawn position in world-space pixels (top-left of the player
/// box, matching the editor's and `draw_texture`'s top-left origin convention).
///
/// Stored as a plain `{ x, y }` struct rather than a [`Vec2D`] because the
/// engine's `glam` dependency is built without its `serde` feature.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpawnPoint {
    pub x: f32,
    pub y: f32,
}

/// Maps an object ID to a semantic classification tag such as `"static"`,
/// `"movable"`, or `"cuttable"`. Authored in the editor's Classification layer.
///
/// The `object_id` is a string and may match multiple objects — this is
/// intentional: setting `id = "player"` on both a sprite and its collision box
/// means they automatically share the same tag.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ClassificationEntry {
    /// The ID string that identifies the target object(s).
    pub object_id: String,
    /// Free-form tag (e.g. `"static"`, `"movable"`, `"wall"`).
    pub tag: String,
}

/// Default authoring grid size (world pixels) for a level that doesn't record
/// one — e.g. a level file written before the grid became configurable.
pub const DEFAULT_GRID_SIZE: f32 = 32.0;

fn default_grid_size() -> f32 {
    DEFAULT_GRID_SIZE
}

/// An ordered collection of planning layers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Level {
    #[serde(default, alias = "shapes")]
    pub sprite_shapes: Vec<Shape>,
    #[serde(default)]
    pub collision_shapes: Vec<Shape>,
    /// Sprite images placed in the sprite-planning layer.
    #[serde(default)]
    pub sprite_instances: Vec<SpriteInstance>,
    /// Classification tags authored in the Classification layer.
    #[serde(default)]
    pub classifications: Vec<ClassificationEntry>,
    /// Where the player spawns, authored in the editor. `None` lets the game
    /// fall back to its own default spawn.
    #[serde(default)]
    pub player_start: Option<SpawnPoint>,
    /// The authoring grid the editor snaps to, in world pixels. Defaults to
    /// [`DEFAULT_GRID_SIZE`] for older files that predate this field.
    #[serde(default = "default_grid_size")]
    pub grid_size: f32,
}

impl Default for Level {
    fn default() -> Self {
        Self {
            sprite_shapes: Vec::new(),
            collision_shapes: Vec::new(),
            sprite_instances: Vec::new(),
            classifications: Vec::new(),
            player_start: None,
            grid_size: DEFAULT_GRID_SIZE,
        }
    }
}

impl Level {
    /// An empty level.
    pub fn new() -> Self {
        Self::default()
    }

    /// Call `id_gen()` for every shape or sprite whose ID is currently empty,
    /// assigning the returned string as a stable identifier.
    ///
    /// Pass any `FnMut() -> String` — the editor supplies [`random_id`] from
    /// its own module so the engine itself stays dependency-free.
    pub fn ensure_ids<F: FnMut() -> String>(&mut self, mut id_gen: F) {
        for shape in &mut self.sprite_shapes {
            if shape.id().is_empty() {
                shape.set_id(id_gen());
            }
        }
        for shape in &mut self.collision_shapes {
            if shape.id().is_empty() {
                shape.set_id(id_gen());
            }
        }
        for inst in &mut self.sprite_instances {
            if inst.id.is_empty() {
                inst.id = id_gen();
            }
        }
    }

    /// The authored player spawn as a [`Vec2D`], if set.
    pub fn player_start_world(&self) -> Option<Vec2D> {
        self.player_start.map(|p| Vec2D::new(p.x, p.y))
    }

    /// Return the classification tag for objects with the given `id`, if any.
    pub fn get_tag(&self, id: &str) -> Option<&str> {
        self.classifications
            .iter()
            .find(|e| e.object_id == id)
            .map(|e| e.tag.as_str())
    }

    /// Draw the sprite-planning layer, in order.
    pub fn draw(&self, canvas: &mut Canvas) {
        for shape in &self.sprite_shapes {
            shape.draw(canvas);
        }
    }

    /// Serialize to pretty JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self)
            .unwrap_or_else(|_| "{\"sprite_shapes\":[],\"collision_shapes\":[]}".to_string())
    }

    /// Parse from JSON text.
    pub fn from_json(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn load(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Self::from_json(&text).map_err(std::io::Error::other)
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, self.to_json())
    }
}
