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

mod classification;
mod constants;
mod editor;
mod geometry;
mod id;
mod level_io;
mod render;
mod sprite_sheet;
mod text_input;
mod types;

use juni::prelude::*;

use constants::{RENDER_H, RENDER_W, WINDOW_TITLE};
use editor::Editor;
use level_io::{load_or_create_level, prompt_level_path, STARTUP};

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
