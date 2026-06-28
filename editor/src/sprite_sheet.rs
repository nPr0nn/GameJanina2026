//! Spritesheet/tileset loading, panel geometry, and tile cropping.

use std::path::Path;

use juni::prelude::*;

use crate::constants::{RENDER_H, RENDER_W};

/// A loaded spritesheet/tileset image used to cut new sprite tiles.
pub(crate) struct SpriteSheet {
    /// Path the sheet was loaded from.
    pub(crate) path: String,
    /// GPU texture for rendering the sheet in the UI panel.
    pub(crate) texture: Texture,
    /// Original image width in pixels.
    pub(crate) width: u32,
    /// Original image height in pixels.
    pub(crate) height: u32,
}

/// Screen-space geometry of the sheet preview panel.
pub(crate) struct SheetPanel {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) w: f32,
    pub(crate) h: f32,
    /// Scale from sheet pixels to screen pixels.
    pub(crate) scale: f32,
    /// Offset of the scaled image inside the panel, centered.
    pub(crate) offset_x: f32,
    pub(crate) offset_y: f32,
}

/// Compute the screen-space panel for a loaded spritesheet preview.
/// Uses a large centered modal so big spritesheets/tilesets remain readable.
pub(crate) fn sheet_panel_rect(sheet_w: u32, sheet_h: u32) -> SheetPanel {
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
pub(crate) fn sheet_mouse_pos(panel: &SheetPanel, mouse: Vec2D) -> Option<Vec2D> {
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
pub(crate) fn sheet_rect_to_panel(panel: &SheetPanel, rect: Rect) -> Rect {
    Rect::new(
        panel.x + panel.offset_x + rect.x * panel.scale,
        panel.y + panel.offset_y + rect.y * panel.scale,
        rect.width * panel.scale,
        rect.height * panel.scale,
    )
}

/// Crop the selected region from `sheet_path` and save it as a new PNG in
/// `sprites_dir`. Returns the path of the newly-created sprite file.
pub(crate) fn crop_and_save_sprite(
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


#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;
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
}
