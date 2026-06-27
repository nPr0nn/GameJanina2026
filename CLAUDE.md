# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Overview

`juni` is a small cross-platform **2D game engine** built on WGPU (27.0.1) +
Winit (0.30), with a **Raylib-inspired** API. It compiles to native
(Windows/Linux/macOS) and to WebAssembly (WebGPU, with WebGL2 fallback).

The repo is a **Cargo workspace** with two members:

- **`engine/`** — the `juni` **library** crate (`engine/src/lib.rs`). All
  graphics/audio/input/wasm plumbing lives here.
- **`game/`** — the binary crate (`game/src/main.rs`) that depends on `juni`,
  implements the `Game` trait, and ships its assets under `game/src/assets/`.

The root `Cargo.toml` sets `default-members = ["game"]`, so a bare `cargo run`
builds and runs the game.

The engine owns the main loop: users implement the `Game` trait
(`init`/`update`/`draw`) and call `juni::run::<G>(config)`. All drawing happens at
a fixed virtual resolution into an offscreen render texture, which is then
letterboxed onto the window.

## Commands

```sh
cargo run                            # native game (game/src/main.rs)
cargo test                           # unit tests (e.g. letterbox math) + doctests
cargo clippy --all-targets           # lints (kept clean)

# Web (WASM) — one-time: rustup target add wasm32-unknown-unknown; cargo install --locked trunk
trunk serve                          # then open http://127.0.0.1:8080 (no flags needed)

cargo build --bin game --target wasm32-unknown-unknown  # wasm build check
```

Run a single test: `cargo test taller_window_letterboxes`.

### Native run note (Wayland)
On some Wayland setups the **GLES backend segfaults** during context init
(a wgpu/driver issue, not engine code). Force Vulkan if you hit this:
`WGPU_BACKEND=vulkan cargo run`. Screenshots for verification: `grim out.png`.

## Architecture

The engine keeps WGPU's async init working inside winit's sync lifecycle by
sending the constructed graphics state back through the event loop as a custom
user event — `EventLoop<Graphics>` where `Graphics` *is* the user-event type
(`lib.rs` `run`, `graphics.rs` `create_graphics` → `proxy.send_event`).

**Frame flow per module:**

- **`lib.rs`** — Public API: `Config`, `Context`, the `Game` trait, `run::<G>()`,
  and the `cfg(wasm32)` split for `run_app` (panic/log hooks + `spawn_app` on web,
  `run_app` on native). Re-exports `Color`/`Vector2`/`Rectangle`/`Draw` and `glam`.

- **`app.rs`** — `App<G: Game>` implements winit's `ApplicationHandler`. Holds a
  `State` (`Init(proxy)` → `Ready(Graphics)`), plus `Option<Renderer>`,
  `Option<G>`, and `TimeStep`. On `RedrawRequested` it: advances time, runs the
  **fixed-timestep accumulator** loop (`while time.next_fixed_step() { game.update }`),
  then `renderer.begin()` → `game.draw(&mut Draw)` → `renderer.flush(gfx)`, and
  re-requests a redraw (`ControlFlow::Poll`). Resize reconfigures only the
  swapchain; the render texture stays at its fixed size.

- **`graphics.rs`** — Owns wgpu device/queue/surface (no pipelines). Picks an
  **sRGB surface format** so linear vertex colors are gamma-correct. Keeps the
  `Rc = Arc/rc::Rc` platform alias and `downlevel_webgl2_defaults()` limits so the
  same code runs under WebGL.

- **`renderer.rs`** — The 2D batch renderer. One `Texture` at the virtual
  resolution. **Two render passes per frame**: (1) all shapes (one
  colored-triangle pipeline, one growable vertex buffer, ortho projection
  uniform) drawn into the render texture with the frame's clear color; (2) a
  letterbox blit (`shaders/letterbox.wgsl`, fullscreen triangle) into the
  swapchain, using `set_viewport` with `compute_letterbox()` for aspect-fit black
  bars. `Batch` holds the CPU vertex `Vec` + clear color.

- **`shapes.rs`** — `Draw<'a>` wraps `&mut Batch`. Raylib-style methods
  (`clear_background`, `rectangle`, `rectangle_rec`, `triangle`, `quad`) that
  decompose everything into triangles via `Batch::push_triangle`.

- **`color.rs`** — `Color { r,g,b,a: u8 }` (`Pod`), `to_linear()` (sRGB→linear),
  and the Raylib palette constants. **Colors are authored in sRGB**; `to_linear`
  is applied when pushing vertices, and the sRGB render target converts back.

- **`math.rs`** — `Vector2 = glam::Vec2`; `Rectangle { x, y, width, height }`.

- **`time.rs`** — `TimeStep`: fixed-timestep accumulator (with a max-frame-time
  clamp to avoid the spiral of death) + FPS sampling. Uses `web_time::Instant`
  (a `std::time::Instant` shim that also works on wasm).

### Coordinates
Virtual-canvas pixels, origin top-left, +Y down (Raylib convention). The ortho
matrix in `renderer.rs::ortho` maps `(0,0)-(render_w,render_h)` into wgpu NDC.

### Web build
`index.html` (at the workspace root) is the Trunk entry
(`data-trunk rel="rust" data-bin="game"`) — Trunk reads the workspace
`Cargo.toml` and `data-bin` maps to `cargo build --bin game`, building the
game crate for wasm.

The web build compiles **both** backends: `wgpu`'s default features include
`webgpu`, and `engine/Cargo.toml` additionally enables the `webgl` feature only
for the `wasm32` target (in `[target.'cfg(target_arch = "wasm32")'.dependencies]`).
At runtime wgpu prefers **WebGPU** and falls back to **WebGL2**. The webgl
feature is essential — without it, browsers lacking WebGPU make `request_adapter`
trap with "Could not get an adapter". There is no `webgl` *cargo* feature to pass
to Trunk; `trunk serve` with no flags is the only command.

`Trunk.toml` pins the serve address to `127.0.0.1:8080` because Trunk otherwise
advertises `http://localhost.:8080/` (trailing dot), which some browsers refuse.

CSS in `index.html` sizes the canvas to the viewport; on web `app.rs` does **not**
call `with_inner_size` (that fights the stylesheet and causes scrollbars) — it's
native-only.
