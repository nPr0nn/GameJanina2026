//! A small sprite-sheet animation system.
//!
//! The sheet is a regular grid of equal cells (e.g. the 32×32 frames in
//! `assets/ducky_spritesheet.png`). **Each row is a state** (row 0 = idle,
//! row 1 = mogin, …) and **each column is a step in time**. A row need not be
//! full: trailing (or interior) empty cells are detected at load time and
//! skipped, so a 3-frame state in a 6-wide sheet just plays its 3 frames.
//!
//! An [`Animation`] plays one state, advancing through that row's non-empty
//! frames; [`set_state`](Animation::set_state) switches rows.

use juni::prelude::*;

/// A texture cut into a uniform grid of `frame_w × frame_h` cells, plus a record
/// of which cells actually contain pixels. Cloning is cheap (the [`Texture`] is
/// reference-counted and the per-row frame lists are small).
#[derive(Clone)]
pub struct SpriteSheet {
    texture: Texture,
    frame_w: f32,
    frame_h: f32,
    /// For each row (state), the column indices that contain visible pixels, in
    /// left-to-right order. Empty cells are absent, so iterating a row's `Vec`
    /// visits only the frames worth drawing.
    rows: Vec<Vec<u32>>,
}

impl SpriteSheet {
    /// Decode PNG `bytes`, upload them as a [`Texture`], and scan the grid to
    /// record the non-empty cell of each row. A cell counts as non-empty if any
    /// of its pixels has a non-zero alpha.
    pub fn from_memory(ctx: &Context, bytes: &[u8], frame_w: u32, frame_h: u32) -> Self {
        let texture = ctx.load_texture_from_memory(bytes);
        let rows = scan_non_empty_cells(bytes, frame_w.max(1), frame_h.max(1));
        Self {
            texture,
            frame_w: frame_w as f32,
            frame_h: frame_h as f32,
            rows,
        }
    }

    /// The non-empty frame columns of `row` (empty slice if the row is blank or
    /// out of range).
    fn frames_in_row(&self, row: u32) -> &[u32] {
        self.rows.get(row as usize).map_or(&[], Vec::as_slice)
    }

    /// The source rectangle (in texture pixels) for the cell at `(col, row)`.
    fn frame_rect(&self, col: u32, row: u32) -> Rect {
        Rect::new(
            col as f32 * self.frame_w,
            row as f32 * self.frame_h,
            self.frame_w,
            self.frame_h,
        )
    }

    /// Number of non-empty frames in `row`.
    pub fn frame_count(&self, row: u32) -> usize {
        self.frames_in_row(row).len()
    }

    /// Width of one frame cell, in texture pixels.
    pub fn frame_width(&self) -> f32 {
        self.frame_w
    }

    /// Height of one frame cell, in texture pixels.
    pub fn frame_height(&self) -> f32 {
        self.frame_h
    }

    /// Draw the frame at `(col, row)` with its top-left at `pos`, scaled by
    /// `scale` and rotated by `rotation` radians around its centre.
    pub fn draw_frame_rotated(
        &self,
        canvas: &mut Canvas,
        col: u32,
        row: u32,
        pos: Vec2D,
        scale: f32,
        rotation: f32,
        tint: Color,
    ) {
        let src = self.frame_rect(col, row);
        let dest = Rect::new(
            pos.x,
            pos.y,
            self.frame_w * scale,
            self.frame_h * scale,
        );
        let origin = Vec2D::new(dest.width * 0.5, dest.height * 0.5);
        canvas.draw_texture_pro(&self.texture, src, dest, origin, rotation, tint);
    }
}

/// A timed playback of one state (row) of a [`SpriteSheet`].
///
/// Advance it each fixed step with [`update`](Self::update), switch states with
/// [`set_state`](Self::set_state), and render with [`draw`](Self::draw). An
/// empty row draws nothing.
#[derive(Clone)]
pub struct Animation {
    sheet: SpriteSheet,
    /// The current state (row index).
    row: u32,
    /// Seconds each frame is shown.
    frame_time: f32,
    /// Time accumulated toward the next frame.
    timer: f32,
    /// Index into the current row's frame list of the frame shown.
    current: usize,
    /// Loop back to the first frame at the end (else hold the last frame).
    looping: bool,
    /// `true` once a non-looping animation has shown its last frame.
    finished: bool,
}

impl Animation {
    /// Play `row` of `sheet` at `fps` frames per second. `looping` restarts from
    /// the first frame at the end; otherwise it holds the last frame and reports
    /// [`is_finished`](Self::is_finished).
    pub fn new(sheet: SpriteSheet, row: u32, fps: f32, looping: bool) -> Self {
        Self {
            sheet,
            row,
            frame_time: if fps > 0.0 { 1.0 / fps } else { f32::INFINITY },
            timer: 0.0,
            current: 0,
            looping,
            finished: false,
        }
    }

    /// Switch to a different state (row). A no-op if already in that state, so it
    /// is safe to call every frame; changing rows restarts playback.
    pub fn set_state(&mut self, row: u32) {
        if row != self.row {
            self.row = row;
            self.reset();
        }
    }

    /// Advance playback by `dt` seconds (pass `ctx.dt` from a fixed update).
    pub fn update(&mut self, dt: f32) {
        let frame_count = self.sheet.frames_in_row(self.row).len();
        if self.finished || frame_count <= 1 {
            return;
        }
        self.timer += dt;
        while self.timer >= self.frame_time {
            self.timer -= self.frame_time;
            if self.current + 1 < frame_count {
                self.current += 1;
            } else if self.looping {
                self.current = 0;
            } else {
                self.finished = true;
                break;
            }
        }
    }

    /// Restart the current state from its first frame.
    pub fn reset(&mut self) {
        self.current = 0;
        self.timer = 0.0;
        self.finished = false;
    }

    /// `true` once a non-looping animation has reached its last frame.
    #[allow(dead_code)] // Part of the API; used by one-shot animations.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Index of the frame currently shown in the active row.
    pub fn current_frame(&self) -> usize {
        self.current
    }

    /// Size of one frame in source pixels (before any draw `scale`).
    pub fn frame_size(&self) -> Vec2D {
        Vec2D::new(self.sheet.frame_w, self.sheet.frame_h)
    }

    /// Draw the current frame with its top-left at `pos`, scaled by `scale`.
    /// `flip_x` mirrors it horizontally (e.g. to face left), and `tint`
    /// multiplies the texels ([`WHITE`] for none). Draws nothing if the current
    /// row is empty.
    pub fn draw(&self, canvas: &mut Canvas, pos: Vec2D, scale: f32, flip_x: bool, tint: Color) {
        self.draw_rotated(canvas, pos, scale, flip_x, 0.0, tint);
    }

    /// Like [`draw`](Self::draw) but rotated by `rotation` degrees around the
    /// frame's centre.
    pub fn draw_rotated(
        &self,
        canvas: &mut Canvas,
        pos: Vec2D,
        scale: f32,
        flip_x: bool,
        rotation: f32,
        tint: Color,
    ) {
        let frames = self.sheet.frames_in_row(self.row);
        let Some(&col) = frames.get(self.current) else {
            return; // Empty row: nothing to draw.
        };
        let mut src = self.sheet.frame_rect(col, self.row);
        // A negative source width samples the cell right-to-left, mirroring it
        // (the engine derives UVs straight from the source rect).
        if flip_x {
            src.x += src.width;
            src.width = -src.width;
        }
        let dest = Rect::new(
            pos.x,
            pos.y,
            self.sheet.frame_w * scale,
            self.sheet.frame_h * scale,
        );
        let origin = Vec2D::new(dest.width * 0.5, dest.height * 0.5);
        canvas.draw_texture_pro(&self.sheet.texture, src, dest, origin, rotation, tint);
    }
}

/// Decode `bytes` and return, per grid row, the list of column indices whose
/// cell contains at least one non-transparent pixel. A decode failure (or a
/// size not divisible by the cell size) yields an empty result, so animations
/// simply draw nothing rather than panicking.
fn scan_non_empty_cells(bytes: &[u8], frame_w: u32, frame_h: u32) -> Vec<Vec<u32>> {
    let img = match image::load_from_memory(bytes) {
        Ok(img) => img.to_rgba8(),
        Err(e) => {
            eprintln!("animation: failed to decode sprite sheet: {e}");
            return Vec::new();
        }
    };
    let (w, h) = img.dimensions();
    let cols = w / frame_w;
    let rows = h / frame_h;

    (0..rows)
        .map(|row| {
            (0..cols)
                .filter(|&col| cell_has_pixels(&img, col * frame_w, row * frame_h, frame_w, frame_h))
                .collect()
        })
        .collect()
}

/// `true` if any pixel in the `frame_w × frame_h` cell at `(x0, y0)` is opaque
/// enough to matter (alpha > 0).
fn cell_has_pixels(
    img: &image::RgbaImage,
    x0: u32,
    y0: u32,
    frame_w: u32,
    frame_h: u32,
) -> bool {
    (y0..y0 + frame_h).any(|y| (x0..x0 + frame_w).any(|x| img.get_pixel(x, y)[3] != 0))
}
