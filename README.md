# juni

A small, cross-platform **2D game engine** built on
**[wgpu](https://wgpu.rs/) (27.0.1)** + **[winit](https://github.com/rust-windowing/winit) (0.30)**,
with a **[Raylib](https://www.raylib.com/)** inspired API.

Runs on **Windows / Linux / macOS** natively and in the browser via
**WebAssembly** (WebGPU, with automatic WebGL2 fallback) with
[trunk](https://trunkrs.dev/).

## Features

- Trait-based `Game` lifecycle — the engine owns the loop and calls your
  `init` / `update` / `draw`.
- **Fixed-timestep** updates (deterministic `update`), rendered every frame.
- A fixed **virtual resolution** drawn into an offscreen render texture, then
  **letterboxed** (aspect-preserving) onto any window size.
- Raylib-style shape drawing: rectangles, triangles, quads.
- `glam` math (`Vector2` is `glam::Vec2`), Raylib `Color` palette.

## Quickstart

```rust
use juni::*;

struct MyGame { x: f32 }

impl Game for MyGame {
    fn init(_ctx: &mut Context) -> Self { MyGame { x: 0.0 } }

    fn update(&mut self, ctx: &mut Context) {
        self.x += 240.0 * ctx.dt; // ctx.dt is the fixed timestep
    }

    fn draw(&mut self, d: &mut Draw) {
        d.clear_background(WHITE);
        d.rectangle(self.x, 100.0, 80.0, 80.0, RED);
        d.triangle(
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

## Running

```sh
# Native (Windows/macOS/Linux) — runs the game (game/src/main.rs)
cargo run

# Web (WASM) — one-time setup
rustup target add wasm32-unknown-unknown
cargo install --locked trunk

trunk serve --open           # opens http://127.0.0.1:8080

# Tests
cargo test
```

The web build compiles **both** the WebGPU and WebGL2 backends (for `wasm32` in
`Cargo.toml`); wgpu uses **WebGPU when the browser supports it and falls back to
WebGL2 otherwise** — no flags needed, just `trunk serve`. (Don't pass
`--features webgl`; it isn't a valid flag here.) Open
**http://127.0.0.1:8080** — the address is pinned in `Trunk.toml`; avoid the
`localhost.` variant some browsers refuse.

> If the page shows an old build, the browser cached it — hard-refresh
> (Ctrl/Cmd+Shift+R) or clear the site cache. `trunk serve` rebuilds `dist/`
> fresh on each run.

### Project layout

The repo is a Cargo workspace: `engine/` is the `juni` library crate and
`game/` is the binary that depends on it (`game/src/main.rs`, with assets under
`game/src/assets/`). The same `game` binary runs natively via `cargo run` and on
the web via Trunk (`data-bin="game"` in `index.html`).

## Configuration

`Config` controls the window and the virtual canvas:

```rust
Config {
    width: 960, height: 540,        // initial window size (physical px)
    render_width: 1280,             // virtual canvas — all drawing
    render_height: 720,             // happens here, then letterboxed
    title: "my game".to_string(),
    target_ups: 60,                 // fixed updates per second
}
```

Coordinates are virtual-canvas pixels: origin top-left, +Y down (Raylib-style).

## Resources

References that helped create the original wgpu+winit template this is built on:

- [learn-wgpu](https://sotrh.github.io/learn-wgpu/)
- [raylib](https://www.raylib.com/) — API inspiration
- [wgpu_winit_example](https://github.com/w4ngzhen/wgpu_winit_example)
