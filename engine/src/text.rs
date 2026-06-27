//! Text rendering via [glyphon](https://docs.rs/glyphon) — cosmic-text does the
//! shaping (Unicode, kerning, ligatures) and glyphon rasterizes into a GPU glyph
//! atlas.
//!
//! Strings are queued during `draw()` as [`TextDraw`] requests on the batch
//! (`renderer.rs`), then shaped and drawn in their own pass during
//! [`Renderer::flush`](crate::renderer::Renderer::flush). That pass loads the
//! already-rendered scene and draws text on top, so **text always layers above
//! shapes/textures** regardless of call order (a deliberate v1 simplification —
//! most game text is a HUD/overlay).
//!
//! A single sans-serif font (Liberation Sans, SIL OFL) is embedded so text looks
//! identical on native and on the web, where the browser exposes no system fonts.

use crate::color::Color;
use crate::graphics::Graphics;
use crate::math::Vec2D;
use glyphon::{
    Attrs, Buffer, Cache, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport,
};

/// The embedded default font (raylib ships a built-in font too). Liberation Sans
/// is metric-compatible with Arial and licensed under the SIL Open Font License,
/// so embedding and redistributing it is fine.
const DEFAULT_FONT: &[u8] = include_bytes!("assets/LiberationSans-Regular.ttf");
const DEFAULT_FAMILY: &str = "Liberation Sans";

/// Line height as a multiple of the font size. cosmic-text needs a line height
/// for vertical layout; raylib's default text spacing is comparable.
const LINE_HEIGHT_SCALE: f32 = 1.2;

/// One string queued for this frame (raylib's `DrawText` arguments, already
/// resolved to screen space by the active 2D camera, if any).
pub struct TextDraw {
    pub text: String,
    /// Screen-space top-left of the text (camera already applied).
    pub x: f32,
    pub y: f32,
    pub font_size: f32,
    /// Uniform scale from the active camera's zoom (1.0 with no camera). Applied
    /// by glyphon to the shaped buffer, so zooming doesn't re-shape the text.
    pub scale: f32,
    pub color: Color,
}

/// Owns every glyphon resource: the font database + shaping (`FontSystem`), the
/// glyph rasterization cache (`SwashCache`), the GPU glyph atlas, the screen
/// `Viewport`, and the `TextRenderer` pipeline. Lives inside the [`Renderer`].
pub struct TextEngine {
    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    renderer: TextRenderer,
    /// Fixed virtual resolution — the coordinate space text positions live in.
    render_width: u32,
    render_height: u32,
}

impl TextEngine {
    pub fn new(
        gfx: &Graphics,
        render_format: wgpu::TextureFormat,
        render_width: u32,
        render_height: u32,
    ) -> Self {
        // Start empty and load only our embedded font. On native `FontSystem::new`
        // would also scan system fonts, but loading just one keeps startup fast
        // and the result identical everywhere (notably on wasm, which has none).
        let mut font_system = FontSystem::new();
        font_system.db_mut().load_font_data(DEFAULT_FONT.to_vec());

        let cache = Cache::new(&gfx.device);
        let viewport = Viewport::new(&gfx.device, &cache);
        let atlas = TextAtlas::new(&gfx.device, &gfx.queue, &cache, render_format);
        // Text draws into the single-sample resolved render texture (after any
        // MSAA resolve), so its pipeline is single-sampled regardless of how many
        // samples the scene uses.
        let mut atlas = atlas;
        let renderer =
            TextRenderer::new(&mut atlas, &gfx.device, wgpu::MultisampleState::default(), None);

        Self {
            font_system,
            swash_cache: SwashCache::new(),
            viewport,
            atlas,
            renderer,
            render_width,
            render_height,
        }
    }

    /// Shape `text` into a cosmic-text buffer at `font_size`, with no wrapping
    /// (the text lays out at its natural width; explicit `\n`s still break lines).
    fn build_buffer(&mut self, text: &str, font_size: f32) -> Buffer {
        let metrics = Metrics::new(font_size, font_size * LINE_HEIGHT_SCALE);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        buffer.set_size(&mut self.font_system, None, None);
        let attrs = Attrs::new().family(Family::Name(DEFAULT_FAMILY));
        buffer.set_text(&mut self.font_system, text, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }

    /// Measure the laid-out size of `text` at `font_size`, in virtual pixels
    /// (raylib's `MeasureTextEx`). Width is the widest line; height covers all
    /// lines at the engine's line spacing.
    pub fn measure(&mut self, text: &str, font_size: f32) -> Vec2D {
        let buffer = self.build_buffer(text, font_size);
        let mut width = 0.0f32;
        let mut lines = 0u32;
        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            lines += 1;
        }
        Vec2D::new(width, lines as f32 * font_size * LINE_HEIGHT_SCALE)
    }

    /// Shape and upload this frame's queued text. Call before [`render`] (both
    /// inside `Renderer::flush`). `texts` may be empty.
    ///
    /// [`render`]: Self::render
    pub fn prepare(&mut self, gfx: &Graphics, texts: &[TextDraw]) {
        self.viewport.update(
            &gfx.queue,
            Resolution {
                width: self.render_width,
                height: self.render_height,
            },
        );

        // Shape every string first; the buffers must outlive `prepare` below
        // because the `TextArea`s borrow them.
        let mut buffers = Vec::with_capacity(texts.len());
        for t in texts {
            buffers.push(self.build_buffer(&t.text, t.font_size));
        }

        let areas = texts.iter().zip(&buffers).map(|(t, buffer)| TextArea {
            buffer,
            left: t.x,
            top: t.y,
            scale: t.scale,
            bounds: TextBounds::default(),
            // Colors are authored in sRGB; the atlas is built in `Accurate`
            // (sRGB) mode, so the bytes pass straight through.
            default_color: glyphon::Color::rgba(t.color.r, t.color.g, t.color.b, t.color.a),
            custom_glyphs: &[],
        });

        if let Err(e) = self.renderer.prepare(
            &gfx.device,
            &gfx.queue,
            &mut self.font_system,
            &mut self.atlas,
            &self.viewport,
            areas,
            &mut self.swash_cache,
        ) {
            log::error!("juni: text prepare failed: {e:?}");
        }
    }

    /// Draw the prepared text into `pass` (which must target the single-sample
    /// render texture with a *load* op so the scene underneath is preserved).
    pub fn render(&mut self, pass: &mut wgpu::RenderPass<'_>) {
        if let Err(e) = self.renderer.render(&self.atlas, &self.viewport, pass) {
            log::error!("juni: text render failed: {e:?}");
        }
        // Drop glyphs that weren't used this frame so the atlas doesn't grow
        // without bound as strings change.
        self.atlas.trim();
    }
}
