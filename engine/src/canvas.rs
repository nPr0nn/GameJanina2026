//! A `Canvas` is handed to `Game::draw`. Its methods push geometry into the
//! current frame's batch; all shapes are decomposed into triangles. Coordinates
//! are in virtual-canvas pixels (origin top-left, +Y down).

use crate::camera::Camera2D;
use crate::color::Color;
use crate::math::{Rect, Vec2D};
use crate::renderer::Batch;
use crate::{Shader, Texture};

pub struct Canvas<'a> {
    batch: &'a mut Batch,
}

impl<'a> Canvas<'a> {
    pub(crate) fn new(batch: &'a mut Batch) -> Self {
        Self { batch }
    }

    /// Set the background (render-texture clear) color for this frame.
    pub fn clear_background(&mut self, color: Color) {
        self.batch.clear_color = color;
    }

    /// Draw subsequent shapes with a custom [`Shader`] until
    /// [`end_shader_mode`](Self::end_shader_mode). Raylib-style: build the shader
    /// once (e.g. in `Game::init` via
    /// [`Context::load_shader_from_memory`](crate::Context::load_shader_from_memory))
    /// and wrap the draw calls you want it applied to.
    pub fn begin_shader_mode(&mut self, shader: &Shader) {
        self.batch.set_pipeline(Some(shader.pipeline.clone()));
    }

    /// Revert to the default shape shader. Pairs with
    /// [`begin_shader_mode`](Self::begin_shader_mode).
    pub fn end_shader_mode(&mut self) {
        self.batch.set_pipeline(None);
    }

    /// Draw subsequent shapes/textures through `camera` (world space) until
    /// [`end_mode_2d`](Self::end_mode_2d). Raylib's `BeginMode2D`.
    pub fn begin_mode_2d(&mut self, camera: Camera2D) {
        self.batch.set_camera(Some(camera));
    }

    /// Stop applying the 2D camera; subsequent drawing is back in screen space.
    /// Raylib's `EndMode2D`.
    pub fn end_mode_2d(&mut self) {
        self.batch.set_camera(None);
    }

    /// Draw a filled, axis-aligned rectangle.
    pub fn rectangle(&mut self, x: f32, y: f32, width: f32, height: f32, color: Color) {
        let tl = [x, y];
        let tr = [x + width, y];
        let br = [x + width, y + height];
        let bl = [x, y + height];
        self.batch.push_triangle(tl, tr, br, color);
        self.batch.push_triangle(tl, br, bl, color);
    }

    /// Draw a filled rectangle from a [`Rect`].
    pub fn rectangle_from_rect(&mut self, rec: Rect, color: Color) {
        self.rectangle(rec.x, rec.y, rec.width, rec.height, color);
    }

    /// Draw a filled triangle. Any winding works (culling is disabled).
    pub fn triangle(&mut self, v1: Vec2D, v2: Vec2D, v3: Vec2D, color: Color) {
        self.batch
            .push_triangle(v1.to_array(), v2.to_array(), v3.to_array(), color);
    }

    /// Draw a filled quad from four corners (in order, e.g. clockwise).
    pub fn quad(&mut self, v1: Vec2D, v2: Vec2D, v3: Vec2D, v4: Vec2D, color: Color) {
        self.quad_gradient(v1, v2, v3, v4, color, color, color, color);
    }

    /// Draw a quad with one color per corner (in the same order as the
    /// vertices). The colors interpolate smoothly across the surface — give the
    /// four corners distinct hues for a rainbow fill.
    #[allow(clippy::too_many_arguments)]
    pub fn quad_gradient(
        &mut self,
        v1: Vec2D,
        v2: Vec2D,
        v3: Vec2D,
        v4: Vec2D,
        c1: Color,
        c2: Color,
        c3: Color,
        c4: Color,
    ) {
        self.batch
            .push_triangle_gradient(v1.to_array(), v2.to_array(), v3.to_array(), c1, c2, c3);
        self.batch
            .push_triangle_gradient(v1.to_array(), v3.to_array(), v4.to_array(), c1, c3, c4);
    }

    /// Draw a line segment of the given thickness (a rotated rectangle). A
    /// zero-length segment draws nothing.
    pub fn line(&mut self, start: Vec2D, end: Vec2D, thickness: f32, color: Color) {
        let dir = end - start;
        let len = dir.length();
        if len <= f32::EPSILON {
            return;
        }
        // Perpendicular offset of half the thickness on each side.
        let normal = Vec2D::new(-dir.y, dir.x) / len * (thickness * 0.5);
        self.quad(start + normal, end + normal, end - normal, start - normal, color);
    }

    /// Draw a filled regular polygon with `sides` sides (>= 3), `rotation` in
    /// degrees. Built as a fan of triangles from the center.
    pub fn regular_polygon(
        &mut self,
        center: Vec2D,
        sides: u32,
        radius: f32,
        rotation: f32,
        color: Color,
    ) {
        if sides < 3 {
            return;
        }
        let step = std::f32::consts::TAU / sides as f32;
        let start = rotation.to_radians();
        let point = |i: u32| {
            let a = start + step * i as f32;
            center + Vec2D::new(a.cos(), a.sin()) * radius
        };
        for i in 0..sides {
            self.triangle(center, point(i), point(i + 1), color);
        }
    }

    /// Draw a filled circle. The number of segments is chosen from the radius;
    /// it is a [`regular_polygon`](Self::regular_polygon) with enough sides to
    /// look round.
    pub fn circle(&mut self, center: Vec2D, radius: f32, color: Color) {
        let segments = ((radius * 0.6) as u32).clamp(12, 64);
        self.regular_polygon(center, segments, radius, 0.0, color);
    }

    /// Draw `texture` at native size with its top-left at `(x, y)`. `tint`
    /// multiplies the texels (use [`WHITE`](crate::WHITE) for no tint).
    /// Raylib's `DrawTexture`.
    pub fn draw_texture(&mut self, texture: &Texture, x: f32, y: f32, tint: Color) {
        self.draw_texture_v(texture, Vec2D::new(x, y), tint);
    }

    /// Draw `texture` at native size with its top-left at `position`. Raylib's
    /// `DrawTextureV`.
    pub fn draw_texture_v(&mut self, texture: &Texture, position: Vec2D, tint: Color) {
        let (w, h) = (texture.width() as f32, texture.height() as f32);
        let src = Rect::new(0.0, 0.0, w, h);
        let dest = Rect::new(position.x, position.y, w, h);
        self.draw_texture_pro(texture, src, dest, Vec2D::ZERO, 0.0, tint);
    }

    /// Draw `texture` with a uniform `scale` and `rotation` (degrees, around
    /// `position`). Raylib's `DrawTextureEx`.
    pub fn draw_texture_ex(
        &mut self,
        texture: &Texture,
        position: Vec2D,
        rotation: f32,
        scale: f32,
        tint: Color,
    ) {
        let (w, h) = (texture.width() as f32, texture.height() as f32);
        let src = Rect::new(0.0, 0.0, w, h);
        let dest = Rect::new(position.x, position.y, w * scale, h * scale);
        self.draw_texture_pro(texture, src, dest, Vec2D::ZERO, rotation, tint);
    }

    /// The general texture draw (raylib's `DrawTexturePro`): sample the `source`
    /// sub-rectangle (in texture pixels) into the `dest` rectangle (in canvas
    /// pixels), pivoting/rotating about `origin` (an offset within `dest`, also
    /// the point placed at `dest`'s position). `rotation` is in degrees.
    pub fn draw_texture_pro(
        &mut self,
        texture: &Texture,
        source: Rect,
        dest: Rect,
        origin: Vec2D,
        rotation: f32,
        tint: Color,
    ) {
        let (tw, th) = (texture.width() as f32, texture.height() as f32);
        // Source pixels -> UVs, in TL, TR, BR, BL order.
        let (u0, v0) = (source.x / tw, source.y / th);
        let (u1, v1) = ((source.x + source.width) / tw, (source.y + source.height) / th);
        let uvs = [[u0, v0], [u1, v0], [u1, v1], [u0, v1]];

        // Corners relative to the origin pivot, rotated, then placed at dest.
        let (sin, cos) = rotation.to_radians().sin_cos();
        let (dx, dy) = (-origin.x, -origin.y);
        let local = [
            [dx, dy],
            [dx + dest.width, dy],
            [dx + dest.width, dy + dest.height],
            [dx, dy + dest.height],
        ];
        let corners = local.map(|[lx, ly]| {
            [dest.x + lx * cos - ly * sin, dest.y + lx * sin + ly * cos]
        });

        self.batch
            .push_textured_quad(corners, uvs, tint, texture.bind_group.clone());
    }

    /// Draw `text` with its top-left at `(x, y)` at the given `font_size` (in
    /// virtual pixels), using the engine's embedded default font. Raylib's
    /// `DrawText`. Embedded `\n`s start new lines.
    ///
    /// Text is composited on top of all shapes/textures drawn this frame,
    /// regardless of call order. The active 2D camera's pan/zoom apply (rotation
    /// does not).
    pub fn text(&mut self, text: &str, x: f32, y: f32, font_size: f32, color: Color) {
        self.batch.push_text(text, x, y, font_size, color);
    }

    /// Like [`text`](Self::text) but taking a [`Vec2D`] position. Raylib's
    /// `DrawTextEx` (without per-glyph spacing; the default font's metrics are
    /// used directly).
    pub fn text_v(&mut self, text: &str, position: Vec2D, font_size: f32, color: Color) {
        self.batch
            .push_text(text, position.x, position.y, font_size, color);
    }
}
