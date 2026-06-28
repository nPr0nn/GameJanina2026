//! Spritesheet/tileset tile cropping.
//!
//! Panel geometry is handled by egui in [`crate::app`]; this module only owns
//! the pure "crop a sub-rectangle out of a PNG and save it" operation.

use std::path::Path;

use juni::prelude::*;

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
}
