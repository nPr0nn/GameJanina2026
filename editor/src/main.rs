// A mouse-driven level editor for juni, built with egui/eframe.
//
//   cargo run -p editor [level.json]
//
// Opens `level.json` (or the path given as the first argument), creating it if
// it does not exist. Use File ▸ Open / Save / Save As, or the keyboard:
//
//   Tab cycle layer · F reset view · H help
//   L-drag place shape · click shape to select/redraw · R-click delete
//   M-drag pan · wheel zoom · S save · O open
//
// Layers: Sprite → Collision → Classification. On the Sprite layer, "Load
// spritesheet…" opens a cutter window: drag a tile, release to cut it into
// sprites/. On the Classification layer, click an object and edit its ID/tag in
// the side panel.

mod app;
mod classification;
mod constants;
mod editor;
mod geometry;
mod id;
mod level_io;
mod sprite_sheet;
mod types;

use app::{default_level_path, EditorApp};
use constants::WINDOW_TITLE;
use editor::Editor;
use level_io::load_or_create_level;

fn main() -> eframe::Result<()> {
    let path = default_level_path(std::env::args().nth(1));
    let loaded = match load_or_create_level(&path.to_string_lossy()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Failed to open or create level at {}: {e}", path.display());
            std::process::exit(1);
        }
    };
    let editor = Editor::new(loaded.path, loaded.level, loaded.status);

    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(WINDOW_TITLE)
            .with_inner_size([1600.0, 900.0])
            .with_fullscreen(true),
        ..Default::default()
    };
    eframe::run_native(
        WINDOW_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(EditorApp::new(cc, editor)))),
    )
}
