# juni

A small, cross-platform **2D game engine** built on
**[wgpu](https://wgpu.rs/) (27.0.1)** + **[winit](https://github.com/rust-windowing/winit) (0.30)**,
with a **[Raylib](https://www.raylib.com/)**-inspired API.

Runs natively on **Windows / Linux / macOS** and in the browser via
**WebAssembly** (WebGPU, with automatic WebGL2 fallback) using
[Trunk](https://trunkrs.dev/).

## Features

- Trait-based `Game` lifecycle — the engine owns the loop and calls your
  `init` / `update` / `draw`.
- **Fixed-timestep** updates (deterministic `update`), rendered every frame.
- A fixed **virtual resolution** drawn into an offscreen render texture, then
  **letterboxed** (aspect-preserving) onto any window size.
- Raylib-style shape drawing: rectangles, triangles, quads, circles, polygons,
  lines.
- **Textures** (PNG) and **custom WGSL shaders**.
- A **2D camera** (`Camera2D`) with pan / zoom / rotation and
  screen↔world helpers.
- **Audio** playback (WAV) via [kira](https://docs.rs/kira).
- Keyboard and mouse **input**.
- `glam` math (`Vec2D` is `glam::Vec2`), the Raylib `Color` palette.

## Project layout

The repo is a Cargo **workspace** with two members:

| Crate     | Path             | What it is                                             |
|-----------|------------------|--------------------------------------------------------|
| `juni`    | `engine/`        | The engine **library** (`engine/src/lib.rs`).          |
| `game`    | `game/`          | The **binary** that depends on `juni` (`game/src/main.rs`), with assets under `game/src/assets/`. |

The root `Cargo.toml` sets `default-members = ["game"]`, so a bare `cargo run`
builds and runs the game. The same `game` binary runs natively and on the web
(`data-bin="game"` in `index.html`).

## Quickstart

```rust
use juni::prelude::*;

struct MyGame { x: f32 }

impl Game for MyGame {
    fn init(_ctx: &mut Context) -> Self { MyGame { x: 0.0 } }

    fn update(&mut self, ctx: &mut Context) {
        self.x += 240.0 * ctx.dt; // ctx.dt is the fixed timestep
    }

    fn draw(&mut self, canvas: &mut Canvas) {
        canvas.clear_background(WHITE);
        canvas.rectangle(self.x, 100.0, 80.0, 80.0, RED);
        canvas.triangle(
            Vec2D::new(400.0, 100.0),
            Vec2D::new(350.0, 200.0),
            Vec2D::new(450.0, 200.0),
            BLUE,
        );
    }
}

fn main() {
    run::<MyGame>(Config::default());
}
```

See `game/src/main.rs` for a fuller demo (textures, a custom shader, a 2D
camera, audio, and input).

## Running

```sh
# Native (Windows/macOS/Linux) — runs game/src/main.rs
cargo run

# Tests (letterbox math + doctests)
cargo test

# Lints
cargo clippy --all-targets
```

### Web (WASM)

```sh
# One-time setup
rustup target add wasm32-unknown-unknown
cargo install --locked trunk

trunk serve   # then open http://127.0.0.1:8080
```

The web build compiles **both** the WebGPU and WebGL2 backends (enabled for
`wasm32` in `engine/Cargo.toml`); wgpu uses **WebGPU when the browser supports
it and falls back to WebGL2 otherwise** — no flags needed, just `trunk serve`.
Open **http://127.0.0.1:8080**; the address is pinned in `Trunk.toml` to avoid
the `localhost.` variant some browsers refuse.

> If the page shows an old build, the browser cached it — hard-refresh
> (Ctrl/Cmd+Shift+R) or clear the site cache. `trunk serve` rebuilds `dist/`
> fresh on each run.

#### Native run note (Wayland)

On some Wayland setups the GLES backend segfaults during context init (a
wgpu/driver issue, not engine code). Force Vulkan if you hit this:
`WGPU_BACKEND=vulkan cargo run`.

## Configuration

`Config` controls the window and the virtual canvas (all fields have defaults
via `Config::default()`):

```rust
Config {
    width: 960, height: 540,        // initial window size (physical px)
    render_width: 1280,             // virtual canvas — all drawing
    render_height: 720,             // happens here, then letterboxed
    title: "my game".to_string(),
    target_ups: 60,                 // fixed updates per second
    resizable: false,               // native only
    centered: true,                 // native only
    fullscreen: false,              // native only; F toggles at runtime
    msaa: 4,                        // 1 disables; unsupported counts fall back
}
```

Coordinates are virtual-canvas pixels: origin top-left, +Y down (Raylib-style).

## Resources

References that helped create the original wgpu+winit template this is built on:

- [learn-wgpu](https://sotrh.github.io/learn-wgpu/)
- [raylib](https://www.raylib.com/) — API inspiration
- [wgpu_winit_example](https://github.com/w4ngzhen/wgpu_winit_example)
