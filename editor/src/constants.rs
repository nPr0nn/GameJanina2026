//! Editor constants: canvas size, grid, palettes.

use juni::prelude::*;

pub(crate) const RENDER_W: u32 = 1280;
pub(crate) const RENDER_H: u32 = 720;
pub(crate) const GRID_SIZE: f32 = 32.0;
pub(crate) const GRID_MAJOR_EVERY: i32 = 4;
pub(crate) const WINDOW_TITLE: &str = "juni — level editor";

/// Color palette for classification tags. Colors are assigned in order as new
/// tags are introduced; the first entry is used for the built-in `"static"`.
pub(crate) const TAG_PALETTE: [Color; 10] = [
    LIGHTGRAY, SKYBLUE, GREEN, ORANGE, PINK, PURPLE, GOLD, RED, BEIGE, BLUE,
];

/// Height of a classification label rectangle in world pixels.
pub(crate) const LABEL_H: f32 = 15.0;
/// Minimum vertical gap between two resolved labels.
pub(crate) const LABEL_GAP: f32 = 2.0;

/// Color choices for shapes, selectable with the number keys 1–6.
pub(crate) const PALETTE: [Color; 6] = [RED, ORANGE, GOLD, LIME, SKYBLUE, VIOLET];
