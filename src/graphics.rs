//! Ownership of the wgpu device, queue and surface.
//!
//! Construction is async (adapter/device acquisition), but winit's lifecycle
//! callbacks are sync. We bridge that by sending the finished `Graphics` back
//! into the event loop as a user event (see `app.rs`).

use winit::{dpi::PhysicalSize, event_loop::EventLoopProxy, window::Window};

#[cfg(target_arch = "wasm32")]
pub type Rc<T> = std::rc::Rc<T>;

#[cfg(not(target_arch = "wasm32"))]
pub type Rc<T> = std::sync::Arc<T>;

pub async fn create_graphics(window: Rc<Window>, proxy: EventLoopProxy<Graphics>) {
    // On the web, force the WebGL2 backend. Browser WebGPU is still experimental
    // on several platforms (notably Linux), where its swapchain/present path
    // stutters: frames are ready in ~1ms but presentation throttles to an
    // erratic ~30fps. WebGL2 is mature everywhere and presents at full rate. We
    // compile both backends (see Cargo.toml); wgpu would otherwise prefer
    // WebGPU, so we narrow the instance to GL here.
    #[cfg(target_arch = "wasm32")]
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::GL,
        ..Default::default()
    });
    #[cfg(not(target_arch = "wasm32"))]
    let instance = wgpu::Instance::default();
    let surface = instance.create_surface(Rc::clone(&window)).unwrap();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            // Ask for the discrete/high-performance GPU where there's a choice.
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
            compatible_surface: Some(&surface),
        })
        .await
        .expect(
            "Could not get a GPU adapter. On the web this means the browser \
             exposed neither WebGPU nor WebGL2; on native, no compatible GPU \
             backend was found.",
        );

    // Report the adapter so we can tell WebGPU from WebGL2 and, crucially,
    // hardware from software rendering: a name like `llvmpipe`, `SwiftShader`
    // or `WARP` means the browser/OS is NOT GPU-accelerating us, which caps the
    // framerate regardless of how cheap our drawing is.
    let info = adapter.get_info();
    log_adapter(&format!(
        "juni adapter: {:?} / {} / {:?} / driver: {}",
        info.backend, info.name, info.device_type, info.driver
    ));

    let (device, queue) = adapter
        .request_device(&wgpu::DeviceDescriptor {
            label: None,
            required_features: wgpu::Features::empty(),
            // Stay within WebGL2 limits so the same code runs under the `webgl`
            // feature in browsers without WebGPU.
            required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                .using_resolution(adapter.limits()),
            memory_hints: wgpu::MemoryHints::Performance,
            trace: Default::default(),
            experimental_features: Default::default(),
        })
        .await
        .expect("Failed to get device");

    let size = window.inner_size();
    let width = size.width.max(1);
    let height = size.height.max(1);

    let mut surface_config = surface.get_default_config(&adapter, width, height).unwrap();

    // Use a NON-sRGB (linear) swapchain and do the sRGB encode ourselves in the
    // letterbox shader. Relying on an sRGB swapchain is not portable: WebGPU's
    // preferred canvas format is non-sRGB and WebGL2's default framebuffer does
    // not reliably apply sRGB encoding, so colors came out darker on the web
    // than on native. The offscreen render texture is still a real sRGB texture
    // (see `renderer.rs`), which is reliable on every backend.
    let caps = surface.get_capabilities(&adapter);
    let linear = surface_config.format.remove_srgb_suffix();
    if caps.formats.contains(&linear) {
        surface_config.format = linear;
    }

    // Opaque compositing: an alpha-capable canvas makes the browser blend the
    // whole surface over the page every frame. We never need page see-through,
    // so prefer Opaque to skip that per-frame compositing cost on the web.
    if caps
        .alpha_modes
        .contains(&wgpu::CompositeAlphaMode::Opaque)
    {
        surface_config.alpha_mode = wgpu::CompositeAlphaMode::Opaque;
    }

    surface.configure(&device, &surface_config);

    let gfx = Graphics {
        window,
        instance,
        surface,
        surface_config,
        adapter,
        device,
        queue,
    };

    let _ = proxy.send_event(gfx);
}

/// Print a one-time startup diagnostic. On the web we call `console.log`
/// directly so it shows even though the global `log` level is kept at `Error`
/// (see `lib.rs` — info/warn would let wgpu spam the console per frame).
#[cfg(not(target_arch = "wasm32"))]
fn log_adapter(msg: &str) {
    log::info!("{msg}");
}

#[cfg(target_arch = "wasm32")]
fn log_adapter(msg: &str) {
    web_sys::console::log_1(&msg.into());
}

/// All wgpu state. Drawing pipelines live in [`crate::renderer::Renderer`].
#[derive(Debug)]
pub struct Graphics {
    pub window: Rc<Window>,
    #[allow(dead_code)]
    pub instance: wgpu::Instance,
    pub surface: wgpu::Surface<'static>,
    pub surface_config: wgpu::SurfaceConfiguration,
    #[allow(dead_code)]
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
}

impl Graphics {
    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn window_size(&self) -> PhysicalSize<u32> {
        self.window.inner_size()
    }

    pub fn surface_format(&self) -> wgpu::TextureFormat {
        self.surface_config.format
    }

    /// Reconfigure the swapchain after a window resize.
    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        self.surface_config.width = new_size.width.max(1);
        self.surface_config.height = new_size.height.max(1);
        self.surface.configure(&self.device, &self.surface_config);
    }
}
