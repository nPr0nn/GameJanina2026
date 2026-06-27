//! Batched 2D renderer.
//!
//! Everything is drawn into an offscreen render texture at a fixed virtual
//! resolution. Each frame, shape vertices are accumulated on the CPU, uploaded
//! in one buffer, and drawn with a single colored-triangle pipeline. The render
//! texture is then blitted onto the swapchain with an aspect-fit (letterboxed)
//! viewport so the game looks identical at any window size.

use crate::camera::Camera2D;
use crate::color::{Color, BLACK};
use crate::graphics::{Graphics, Rc};
use crate::math::Vec2D;
use crate::text::{TextDraw, TextEngine};
use bytemuck::{Pod, Zeroable};
use glam::Mat4;
use std::cell::RefCell;
use std::ops::Range;
use wgpu::util::DeviceExt;

/// Compile a WGSL shader that lives in `src/shaders/`, embedding its source at
/// build time with the `include_str!` macro. The stem names both the file
/// (`"shape"` → `shaders/shape.wgsl`) and the module's debug label, so adding a
/// shader is one line instead of a full `ShaderModuleDescriptor`.
macro_rules! shader {
    ($device:expr, $name:literal) => {
        $device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(concat!("juni ", $name, " shader")),
            source: wgpu::ShaderSource::Wgsl(
                include_str!(concat!("shaders/", $name, ".wgsl")).into(),
            ),
        })
    };
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 2],
    pub color: [f32; 4],
    /// Texture coordinates. Ignored by the shape/custom shaders (they only read
    /// `position` and `color`); used by the texture shader. UV is appended at
    /// `location(2)` so adding it didn't renumber the existing attributes.
    pub uv: [f32; 2],
}

impl Vertex {
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4, 2 => Float32x2],
    };
}

/// A GPU texture ready to draw with [`Canvas::draw_texture`](crate::Canvas::draw_texture)
/// and friends. Build one from PNG bytes with
/// [`Context::load_texture_from_memory`](crate::Context::load_texture_from_memory).
///
/// Holds an `Rc` to its bind group (texture view + sampler), so cloning is cheap
/// and the GPU resources live as long as any clone does.
#[derive(Clone)]
pub struct Texture {
    pub(crate) bind_group: Rc<wgpu::BindGroup>,
    width: u32,
    height: u32,
}

impl Texture {
    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }
    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }
}

/// Per-frame uniforms shared by every shape/custom pipeline (bind group 0).
/// Laid out to match the `Globals` struct in `shaders/shape.wgsl`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct Globals {
    /// Orthographic projection: virtual-canvas pixels -> NDC.
    proj: [f32; 16],
    /// Seconds since startup, for animated custom shaders.
    time: f32,
    _pad: [f32; 3],
}

/// A compiled custom shader the user can switch to with
/// [`Canvas::begin_shader_mode`](crate::Canvas::begin_shader_mode). It is a full
/// render pipeline built against the engine's standard interface (vertex
/// `position`+`color`, `Globals` at group 0), so it slots into the shape pass.
///
/// `Rc` so it is cheap to clone into the frame's draw commands and to hold in
/// game state. Build one with
/// [`Context::load_shader_from_memory`](crate::Context::load_shader_from_memory).
#[derive(Clone)]
pub struct Shader {
    pub(crate) pipeline: Rc<wgpu::RenderPipeline>,
}

/// Which pipeline (and resources) a run of vertices draws with.
#[derive(Clone)]
enum Pipeline {
    /// The built-in colored-triangle pipeline.
    Shape,
    /// A user [`Shader`] (still a shape-pass pipeline, group 0 only).
    Custom(Rc<wgpu::RenderPipeline>),
    /// The texture pipeline, sampling the given bind group (group 1).
    Texture(Rc<wgpu::BindGroup>),
}

/// A contiguous run of vertices drawn with one pipeline.
struct DrawCommand {
    pipeline: Pipeline,
    range: Range<u32>,
}

/// CPU-side accumulation of the current frame's geometry. Owned by the renderer
/// and handed to user code (wrapped in `Canvas`) during `draw()`.
///
/// Geometry goes into one shared vertex `Vec`; `commands` records which pipeline
/// draws which slice of it, so `begin/end_shader_mode` only costs a pipeline
/// switch (no extra buffers).
pub struct Batch {
    pub vertices: Vec<Vertex>,
    pub clear_color: Color,
    commands: Vec<DrawCommand>,
    /// First vertex of the run not yet committed to `commands`.
    run_start: u32,
    /// Pipeline the current shape/shader run draws with. Texture draws emit
    /// their own command and leave this untouched (so an active shader mode
    /// resumes after a texture).
    run_pipeline: Pipeline,
    /// Active 2D camera (between `begin/end_mode_2d`). Applied to every vertex
    /// position on push, so it transforms shapes and textures uniformly.
    camera: Option<Camera2D>,
    /// Strings queued for this frame. Drawn in a separate pass (on top of the
    /// shape/texture geometry) at flush time; see `text.rs`.
    texts: Vec<TextDraw>,
    /// Optional fullscreen overlay color, composited on top of *everything*
    /// (shapes, textures, and text) at the very end of the frame. Drives the
    /// fade-in/out screen transitions. `None` = no overlay.
    overlay: Option<Color>,
}

impl Batch {
    /// Push a triangle with a single flat color.
    pub fn push_triangle(&mut self, a: [f32; 2], b: [f32; 2], c: [f32; 2], color: Color) {
        self.push_triangle_gradient(a, b, c, color, color, color);
    }

    /// Push a triangle with a color per corner; the GPU interpolates between
    /// them across the face (the basis for gradients / rainbow fills).
    pub fn push_triangle_gradient(
        &mut self,
        a: [f32; 2],
        b: [f32; 2],
        c: [f32; 2],
        ca: Color,
        cb: Color,
        cc: Color,
    ) {
        // Shapes don't sample a texture, so UV is irrelevant (set to 0).
        self.vertices.push(Vertex { position: self.transform(a), color: ca.to_linear(), uv: [0.0; 2] });
        self.vertices.push(Vertex { position: self.transform(b), color: cb.to_linear(), uv: [0.0; 2] });
        self.vertices.push(Vertex { position: self.transform(c), color: cc.to_linear(), uv: [0.0; 2] });
    }

    /// Apply the active camera (if any) to a world-space position.
    fn transform(&self, p: [f32; 2]) -> [f32; 2] {
        match &self.camera {
            Some(cam) => cam.world_to_screen(Vec2D::from(p)).to_array(),
            None => p,
        }
    }

    /// Set the active 2D camera (`None` to clear). Used by `begin/end_mode_2d`.
    pub(crate) fn set_camera(&mut self, camera: Option<Camera2D>) {
        self.camera = camera;
    }

    /// Queue a string for this frame. The active camera (if any) is baked in now:
    /// the position is mapped to screen space and the camera's zoom becomes a
    /// uniform `scale` (rotation is not applied to text in this version).
    pub(crate) fn push_text(&mut self, text: &str, x: f32, y: f32, font_size: f32, color: Color) {
        let (pos, scale) = match &self.camera {
            Some(cam) => (cam.world_to_screen(Vec2D::new(x, y)), cam.zoom),
            None => (Vec2D::new(x, y), 1.0),
        };
        self.texts.push(TextDraw {
            text: text.to_string(),
            x: pos.x,
            y: pos.y,
            font_size,
            scale,
            color,
        });
    }

    /// Set the fullscreen overlay color for this frame (composited last, over
    /// text too). Used by `Canvas::fade`.
    pub(crate) fn set_overlay(&mut self, color: Color) {
        self.overlay = Some(color);
    }

    /// Emit a textured quad as its own draw command. `corners` and `uvs` are in
    /// TL, TR, BR, BL order; `tint` multiplies the sampled texels.
    pub(crate) fn push_textured_quad(
        &mut self,
        corners: [[f32; 2]; 4],
        uvs: [[f32; 2]; 4],
        tint: Color,
        texture: Rc<wgpu::BindGroup>,
    ) {
        // Close any pending shape/shader run first so draw order is preserved.
        self.close_run();
        let color = tint.to_linear();
        let start = self.vertices.len() as u32;
        for &i in &[0usize, 1, 2, 0, 2, 3] {
            self.vertices.push(Vertex {
                position: self.transform(corners[i]),
                color,
                uv: uvs[i],
            });
        }
        let end = self.vertices.len() as u32;
        self.commands.push(DrawCommand {
            pipeline: Pipeline::Texture(texture),
            range: start..end,
        });
        self.run_start = end;
    }

    /// Switch the pipeline used for subsequent shape geometry, closing the
    /// current run. Used by `begin/end_shader_mode`.
    pub(crate) fn set_pipeline(&mut self, pipeline: Option<Rc<wgpu::RenderPipeline>>) {
        self.close_run();
        self.run_pipeline = match pipeline {
            Some(p) => Pipeline::Custom(p),
            None => Pipeline::Shape,
        };
    }

    /// Commit the in-progress vertex run as a draw command (if non-empty).
    fn close_run(&mut self) {
        let end = self.vertices.len() as u32;
        if end > self.run_start {
            self.commands.push(DrawCommand {
                pipeline: self.run_pipeline.clone(),
                range: self.run_start..end,
            });
        }
        self.run_start = end;
    }

    /// Reset for a new frame.
    fn reset(&mut self) {
        self.vertices.clear();
        self.commands.clear();
        self.clear_color = BLACK;
        self.run_start = 0;
        self.run_pipeline = Pipeline::Shape;
        self.camera = None;
        self.texts.clear();
        self.overlay = None;
    }
}

const INITIAL_VERTEX_CAPACITY: u64 = 4096;

pub struct Renderer {
    render_width: u32,
    render_height: u32,

    /// Single-sample texture the letterbox pass samples. When MSAA is on it is
    /// the resolve target; otherwise shapes render straight into it.
    sampled_view: wgpu::TextureView,
    /// Multisampled color attachment, present only when MSAA is enabled.
    msaa_view: Option<wgpu::TextureView>,

    // Shape pass.
    shape_pipeline: wgpu::RenderPipeline,
    globals_buffer: wgpu::Buffer,
    globals_bind_group: wgpu::BindGroup,
    vertex_buffer: wgpu::Buffer,
    vertex_capacity: u64,

    // Texture pass (shares the vertex buffer + globals; group 1 = texture).
    texture_pipeline: wgpu::RenderPipeline,
    texture_layout: wgpu::BindGroupLayout,
    texture_sampler: wgpu::Sampler,

    // Kept so `build_shader` can compile user pipelines matching the shape pass.
    globals_layout: wgpu::BindGroupLayout,
    render_format: wgpu::TextureFormat,
    sample_count: u32,

    // Letterbox pass.
    letterbox_pipeline: wgpu::RenderPipeline,
    letterbox_bind_group: wgpu::BindGroup,

    // Text pass. `RefCell` so `Context::measure_text` can shape through a shared
    // `&Renderer` while `flush` still mutates it directly.
    text: RefCell<TextEngine>,

    // Fullscreen fade overlay, composited after text. Single-sample (it shares
    // the post pass with text, which targets the resolved texture), so it needs
    // its own pipeline rather than reusing the (MSAA) shape pipeline.
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_buffer: wgpu::Buffer,

    pub batch: Batch,
}

impl Renderer {
    pub fn new(gfx: &Graphics, render_width: u32, render_height: u32, msaa: u32) -> Self {
        let device = &gfx.device;
        // The swapchain is linear (non-sRGB) and we encode sRGB ourselves in the
        // letterbox shader; the offscreen render texture, however, is a genuine
        // sRGB texture so shape colors are gamma-correct and MSAA resolve and
        // filtering happen in linear space. See `graphics.rs` for the why.
        let surface_format = gfx.surface_format();
        let render_format = wgpu::TextureFormat::Rgba8UnormSrgb;
        let render_width = render_width.max(1);
        let render_height = render_height.max(1);
        let sample_count = resolve_sample_count(gfx, render_format, msaa);

        let size = wgpu::Extent3d {
            width: render_width,
            height: render_height,
            depth_or_array_layers: 1,
        };

        // --- Render texture (single-sample; the letterbox pass samples this) ---
        let render_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("juni render texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: render_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let sampled_view = render_texture.create_view(&wgpu::TextureViewDescriptor::default());

        // --- Multisampled attachment, only when MSAA is enabled. Shapes render
        // here and are resolved into the single-sample texture above. ---
        let msaa_view = (sample_count > 1).then(|| {
            let msaa_texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("juni msaa texture"),
                size,
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format: render_format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            msaa_texture.create_view(&wgpu::TextureViewDescriptor::default())
        });

        // --- Shape pipeline ---
        let proj = ortho(render_width, render_height);
        let globals = Globals {
            proj: proj.to_cols_array(),
            time: 0.0,
            _pad: [0.0; 3],
        };
        let globals_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("juni globals"),
            contents: bytemuck::bytes_of(&globals),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let globals_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("juni globals layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                // Custom shaders (e.g. an animated rainbow) read `time` in the
                // fragment stage, so the uniform must be visible there too.
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let globals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("juni globals bind group"),
            layout: &globals_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: globals_buffer.as_entire_binding(),
            }],
        });

        let shape_shader = shader!(device, "shape");
        let shape_pipeline = create_shape_pipeline(
            device,
            &globals_layout,
            &shape_shader,
            render_format,
            sample_count,
        );

        // --- Texture pipeline (group 0 = globals, group 1 = texture+sampler) ---
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("juni texture layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        // Nearest filtering keeps pixel-art crisp when scaled up (raylib's
        // default texture filter is also point/nearest).
        let texture_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("juni texture sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let texture_shader = shader!(device, "texture");
        let texture_pipeline = create_textured_pipeline(
            device,
            &globals_layout,
            &texture_layout,
            &texture_shader,
            render_format,
            sample_count,
        );

        let vertex_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("juni vertex buffer"),
            size: INITIAL_VERTEX_CAPACITY * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Letterbox pipeline ---
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("juni letterbox sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let letterbox_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("juni letterbox layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let letterbox_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("juni letterbox bind group"),
            layout: &letterbox_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&sampled_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let letterbox_shader = shader!(device, "letterbox");
        let letterbox_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("juni letterbox pipeline layout"),
                bind_group_layouts: &[Some(&letterbox_layout)],
                immediate_size: 0,
            });
        let letterbox_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("juni letterbox pipeline"),
            layout: Some(&letterbox_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &letterbox_shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &letterbox_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(surface_format.into())],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });

        let text = RefCell::new(TextEngine::new(
            gfx,
            render_format,
            render_width,
            render_height,
        ));

        // Overlay: a single-sample shape pipeline plus a 6-vertex buffer holding
        // the fullscreen quad (rewritten with the fade color each frame).
        let overlay_pipeline =
            create_shape_pipeline(device, &globals_layout, &shape_shader, render_format, 1);
        let overlay_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("juni overlay buffer"),
            size: 6 * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self {
            render_width,
            render_height,
            sampled_view,
            msaa_view,
            shape_pipeline,
            globals_buffer,
            globals_bind_group,
            vertex_buffer,
            vertex_capacity: INITIAL_VERTEX_CAPACITY,
            texture_pipeline,
            texture_layout,
            texture_sampler,
            globals_layout,
            render_format,
            sample_count,
            letterbox_pipeline,
            letterbox_bind_group,
            text,
            overlay_pipeline,
            overlay_buffer,
            batch: Batch {
                vertices: Vec::new(),
                clear_color: BLACK,
                commands: Vec::new(),
                run_start: 0,
                run_pipeline: Pipeline::Shape,
                camera: None,
                texts: Vec::new(),
                overlay: None,
            },
        }
    }

    /// Measure `text` at `font_size` in virtual pixels (raylib's `MeasureTextEx`).
    /// Shapes through the shared text engine via interior mutability.
    pub fn measure_text(&self, text: &str, font_size: f32) -> Vec2D {
        self.text.borrow_mut().measure(text, font_size)
    }

    /// Compile a custom [`Shader`] from WGSL source. The module must expose
    /// `vs_main`/`fs_main` against the engine's standard interface (see
    /// `shaders/shape.wgsl`): vertex inputs `@location(0) position: vec2` and
    /// `@location(1) color: vec4`, plus `Globals` at `@group(0) @binding(0)`.
    pub fn build_shader(&self, gfx: &Graphics, source: &str) -> Shader {
        let module = gfx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("juni custom shader"),
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = create_shape_pipeline(
            &gfx.device,
            &self.globals_layout,
            &module,
            self.render_format,
            self.sample_count,
        );
        Shader {
            pipeline: Rc::new(pipeline),
        }
    }

    /// Decode PNG `bytes` and upload them as a [`Texture`]. On a decode error a
    /// 1×1 magenta placeholder is returned (and the error logged), matching
    /// raylib's "never fail the caller" loading ergonomics.
    pub fn build_texture(&self, gfx: &Graphics, bytes: &[u8]) -> Texture {
        let (rgba, width, height) = match image::load_from_memory(bytes) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = rgba.dimensions();
                (rgba.into_raw(), w, h)
            }
            Err(e) => {
                log::error!("juni: failed to decode texture: {e}");
                (vec![255, 0, 255, 255], 1, 1)
            }
        };

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = gfx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("juni texture"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            // sRGB so sampling decodes to linear, matching the shape colors.
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        gfx.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let bind_group = gfx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("juni texture bind group"),
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.texture_sampler),
                },
            ],
        });

        Texture {
            bind_group: Rc::new(bind_group),
            width,
            height,
        }
    }

    /// Reset the batch for a new frame.
    pub fn begin(&mut self) {
        self.batch.reset();
    }

    /// Upload the batch, draw it into the render texture, then letterbox-blit
    /// onto the swapchain and present. `time` is the elapsed seconds exposed to
    /// custom shaders via `Globals.time`.
    pub fn flush(&mut self, gfx: &Graphics, time: f32) {
        let frame = match gfx.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            // Surface lost/outdated/occluded (e.g. mid-resize) — skip this frame.
            _ => return,
        };
        let surface_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Update the per-frame uniform (only `time` changes; `proj` is fixed).
        // `time` lives right after the 64-byte projection matrix.
        gfx.queue
            .write_buffer(&self.globals_buffer, 64, bytemuck::bytes_of(&time));

        self.upload_vertices(gfx);
        // Commit the trailing vertex run so every shape is in a draw command.
        self.batch.close_run();

        let mut encoder = gfx
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("juni encoder"),
            });

        // Pass 1: shapes -> render texture. With MSAA, draw into the
        // multisampled attachment and resolve into the sampled texture.
        {
            let (view, resolve_target) = match &self.msaa_view {
                Some(msaa) => (msaa, Some(&self.sampled_view)),
                None => (&self.sampled_view, None),
            };
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("juni shape pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(self.batch.clear_color.to_wgpu()),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if !self.batch.commands.is_empty() {
                pass.set_bind_group(0, &self.globals_bind_group, &[]);
                pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                // One draw per run, in submission order so layering is correct.
                // The pipeline (and, for textures, the group-1 binding) switches
                // at each begin/end_shader_mode or texture boundary.
                for cmd in &self.batch.commands {
                    match &cmd.pipeline {
                        Pipeline::Shape => pass.set_pipeline(&self.shape_pipeline),
                        Pipeline::Custom(p) => pass.set_pipeline(p),
                        Pipeline::Texture(tex) => {
                            pass.set_pipeline(&self.texture_pipeline);
                            pass.set_bind_group(1, tex.as_ref(), &[]);
                        }
                    }
                    pass.draw(cmd.range.clone(), 0..1);
                }
            }
        }

        // Pass 1.5: text, drawn on top of the scene. It targets the single-sample
        // resolved texture with a *load* op, so it composites over the shapes
        // regardless of MSAA. Skipped entirely when no text was queued.
        if !self.batch.texts.is_empty() {
            self.text.borrow_mut().prepare(gfx, &self.batch.texts);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("juni text pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.sampled_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.text.borrow_mut().render(&mut pass);
        }

        // Pass 2: render texture -> swapchain (letterboxed).
        {
            let window = gfx.window_size();
            let (vx, vy, vw, vh) = compute_letterbox(
                window.width,
                window.height,
                self.render_width,
                self.render_height,
            );

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("juni letterbox pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &surface_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if vw > 0.0 && vh > 0.0 {
                pass.set_viewport(vx, vy, vw, vh, 0.0, 1.0);
                pass.set_pipeline(&self.letterbox_pipeline);
                pass.set_bind_group(0, &self.letterbox_bind_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }

        gfx.queue.submit(Some(encoder.finish()));
        frame.present();
    }

    fn upload_vertices(&mut self, gfx: &Graphics) {
        let needed = self.batch.vertices.len() as u64;
        if needed == 0 {
            return;
        }
        if needed > self.vertex_capacity {
            // Grow to the next power of two that fits.
            let mut cap = self.vertex_capacity.max(1);
            while cap < needed {
                cap *= 2;
            }
            self.vertex_capacity = cap;
            self.vertex_buffer = gfx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("juni vertex buffer"),
                size: cap * std::mem::size_of::<Vertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        gfx.queue
            .write_buffer(&self.vertex_buffer, 0, bytemuck::cast_slice(&self.batch.vertices));
    }
}

/// Clamp a requested MSAA sample count to what the adapter supports for
/// `format`, falling back to the highest supported count at or below the
/// request (down to `1` = no MSAA).
fn resolve_sample_count(gfx: &Graphics, format: wgpu::TextureFormat, requested: u32) -> u32 {
    if requested <= 1 {
        return 1;
    }
    let flags = gfx.adapter.get_texture_format_features(format).flags;
    [requested, 8, 4, 2]
        .into_iter()
        .find(|&c| c <= requested && flags.sample_count_supported(c))
        .unwrap_or(1)
}

/// Build a pipeline for the shape pass: the `Vertex` layout, `Globals` at bind
/// group 0, alpha blending into `render_format`, at `sample_count` MSAA. Both
/// the built-in shape pipeline and user [`Shader`]s share this so they are
/// interchangeable within the shape pass.
fn create_shape_pipeline(
    device: &wgpu::Device,
    globals_layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    render_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("juni shape pipeline layout"),
        bind_group_layouts: &[Some(globals_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("juni shape pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Vertex::LAYOUT],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: render_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}

/// Like [`create_shape_pipeline`] but with a second bind group for the texture
/// (group 1). Shares the same `Vertex` layout — the texture shader additionally
/// reads `uv` (location 2).
fn create_textured_pipeline(
    device: &wgpu::Device,
    globals_layout: &wgpu::BindGroupLayout,
    texture_layout: &wgpu::BindGroupLayout,
    shader: &wgpu::ShaderModule,
    render_format: wgpu::TextureFormat,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("juni texture pipeline layout"),
        bind_group_layouts: &[Some(globals_layout), Some(texture_layout)],
        immediate_size: 0,
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("juni texture pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_main"),
            buffers: &[Vertex::LAYOUT],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: render_format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview_mask: None,
        cache: None,
    })
}

/// Orthographic projection mapping (0,0)-(w,h) with a top-left origin and +Y
/// down into wgpu NDC.
fn ortho(w: u32, h: u32) -> Mat4 {
    Mat4::orthographic_rh(0.0, w as f32, h as f32, 0.0, -1.0, 1.0)
}

/// Aspect-fit the render texture inside the window, returning the centered
/// viewport `(x, y, width, height)` in physical pixels. The remaining area is
/// the letterbox/pillarbox bars.
pub fn compute_letterbox(
    window_w: u32,
    window_h: u32,
    render_w: u32,
    render_h: u32,
) -> (f32, f32, f32, f32) {
    let window_w = window_w as f32;
    let window_h = window_h as f32;
    let target_aspect = render_w as f32 / render_h as f32;
    let window_aspect = window_w / window_h;

    if window_aspect > target_aspect {
        // Window is wider than the canvas: pillarbox (bars left/right).
        let h = window_h;
        let w = h * target_aspect;
        ((window_w - w) * 0.5, 0.0, w, h)
    } else {
        // Window is taller: letterbox (bars top/bottom).
        let w = window_w;
        let h = w / target_aspect;
        (0.0, (window_h - h) * 0.5, w, h)
    }
}

#[cfg(test)]
mod tests {
    use super::compute_letterbox;

    #[test]
    fn equal_aspect_fills_window() {
        let (x, y, w, h) = compute_letterbox(960, 540, 1280, 720);
        assert_eq!((x, y, w, h), (0.0, 0.0, 960.0, 540.0));
    }

    #[test]
    fn wider_window_pillarboxes() {
        // 2:1 window, 16:9 canvas -> bars on left/right, full height.
        let (x, y, w, h) = compute_letterbox(1440, 720, 1280, 720);
        assert_eq!(h, 720.0);
        assert_eq!(y, 0.0);
        assert!((w - 1280.0).abs() < 0.001);
        assert!((x - 80.0).abs() < 0.001); // (1440 - 1280) / 2
    }

    #[test]
    fn taller_window_letterboxes() {
        // 1:1 window, 16:9 canvas -> bars on top/bottom, full width.
        let (x, y, w, h) = compute_letterbox(720, 720, 1280, 720);
        assert_eq!(w, 720.0);
        assert_eq!(x, 0.0);
        let expected_h = 720.0 / (1280.0 / 720.0);
        assert!((h - expected_h).abs() < 0.001);
        assert!((y - (720.0 - expected_h) * 0.5).abs() < 0.001);
    }
}
