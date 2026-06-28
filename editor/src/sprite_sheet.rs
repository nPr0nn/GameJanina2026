//! Spritesheet/tileset tile cropping.
//!
//! Panel geometry is handled by egui in [`crate::app`]; this module only owns
//! the pure "crop a sub-rectangle out of a PNG and save it" operation.

use std::path::Path;

use juni::prelude::*;

/// Snap a raw drag (two corners `a`/`b`, in sheet pixels) to whole `grid` cells.
///
/// The low corner floors to a grid line and the high corner ceils, so the result
/// covers every cell the drag touched and always at least one cell. `grid <= 1`
/// falls back to pixel-exact selection. The result is clamped to the sheet and
/// has strictly positive size.
pub(crate) fn snap_tile_selection(a: Vec2D, b: Vec2D, grid: u32, sheet_w: u32, sheet_h: u32) -> Rect {
    let (w, h) = (sheet_w as f32, sheet_h as f32);
    let (lo_x, hi_x) = (a.x.min(b.x), a.x.max(b.x));
    let (lo_y, hi_y) = (a.y.min(b.y), a.y.max(b.y));

    let (mut x0, mut y0, mut x1, mut y1) = if grid <= 1 {
        (lo_x.floor(), lo_y.floor(), hi_x.ceil(), hi_y.ceil())
    } else {
        let g = grid as f32;
        (
            (lo_x / g).floor() * g,
            (lo_y / g).floor() * g,
            (hi_x / g).ceil() * g,
            (hi_y / g).ceil() * g,
        )
    };

    // Guarantee at least one cell (or one pixel when ungridded).
    let step = grid.max(1) as f32;
    if x1 <= x0 {
        x1 = x0 + step;
    }
    if y1 <= y0 {
        y1 = y0 + step;
    }

    x0 = x0.clamp(0.0, w);
    y0 = y0.clamp(0.0, h);
    x1 = x1.clamp(0.0, w);
    y1 = y1.clamp(0.0, h);
    Rect::new(x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0))
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
    fn snap_expands_a_partial_drag_to_whole_cells() {
        // Drag from (20,20) to (50,40) with a 16px grid → cells [16,64)x[16,48).
        let sel = snap_tile_selection(Vec2D::new(20.0, 20.0), Vec2D::new(50.0, 40.0), 16, 256, 256);
        assert_eq!((sel.x, sel.y, sel.width, sel.height), (16.0, 16.0, 48.0, 32.0));
    }

    #[test]
    fn snap_a_click_inside_one_cell_selects_that_cell() {
        // Tiny drag within a single 32px cell still yields a full 32×32 tile.
        let sel = snap_tile_selection(Vec2D::new(70.0, 70.0), Vec2D::new(72.0, 71.0), 32, 256, 256);
        assert_eq!((sel.x, sel.y, sel.width, sel.height), (64.0, 64.0, 32.0, 32.0));
    }

    #[test]
    fn snap_handles_reversed_drag_and_clamps_to_sheet() {
        // Drag up-left past the edge; corners reversed; 16px grid; 96×96 sheet.
        let sel = snap_tile_selection(Vec2D::new(90.0, 90.0), Vec2D::new(40.0, 40.0), 16, 96, 96);
        // low=40→32, high=90→96 (clamped to 96).
        assert_eq!((sel.x, sel.y, sel.width, sel.height), (32.0, 32.0, 64.0, 64.0));
    }

    #[test]
    fn snap_grid_one_is_pixel_exact() {
        let sel = snap_tile_selection(Vec2D::new(3.2, 4.8), Vec2D::new(10.0, 9.0), 1, 64, 64);
        assert_eq!((sel.x, sel.y, sel.width, sel.height), (3.0, 4.0, 7.0, 5.0));
    }

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
}
