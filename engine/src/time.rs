//! Fixed-timestep accumulator and FPS tracking.
//!
//! Uses `web_time::Instant`, a drop-in replacement for `std::time::Instant`
//! that also works on `wasm32` (where `std::time::Instant` panics).

use web_time::{Instant, SystemTime, UNIX_EPOCH};

/// A time-based RNG seed (milliseconds since the Unix epoch) that works on every
/// platform — including `wasm32`, where `std::time::SystemTime` panics with
/// "time not implemented on this platform". Uses `web_time`'s portable clock.
pub fn time_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(42)
}

/// Print the per-second FPS readout. On the web we call `console.log` directly
/// rather than going through the `log` crate: that lets us keep the global
/// console level at `Error` so third-party crates (notably wgpu's WebGL
/// backend) can't spam the console every frame — synchronous `console.log`
/// calls in the render loop are slow enough to wreck the framerate.
/// `cpu_ms` is the worst main-thread frame cost this window (update + draw +
/// command submission). If it stays low while `fps` is also low, the frames are
/// being throttled by the GPU/compositor/present path, not by our code.
#[cfg(not(target_arch = "wasm32"))]
fn log_fps(fps: u32, cpu_ms: f32) {
    log::info!("juni fps={fps} cpu={cpu_ms:.2}ms");
}

#[cfg(target_arch = "wasm32")]
fn log_fps(fps: u32, cpu_ms: f32) {
    web_sys::console::log_1(&format!("juni fps={fps} cpu={cpu_ms:.2}ms").into());
}

pub struct TimeStep {
    /// Seconds per fixed update (1.0 / target_ups).
    fixed_dt: f32,
    /// Real time since the last frame that has not yet been consumed by updates.
    accumulator: f32,
    /// Maximum real time to consume per frame, to avoid the "spiral of death"
    /// where slow frames queue ever more updates.
    max_frame_time: f32,

    last: Instant,
    start: Instant,

    /// Total elapsed wall-clock time in seconds since startup.
    total: f64,

    // FPS tracking (render frames per second, sampled once a second).
    fps: u32,
    frame_counter: u32,
    fps_timer: f32,
    /// Worst main-thread frame cost (seconds) in the current 1s window.
    cpu_max: f32,
}

impl TimeStep {
    pub fn new(target_ups: u32) -> Self {
        let fixed_dt = 1.0 / target_ups.max(1) as f32;
        let now = Instant::now();
        Self {
            fixed_dt,
            accumulator: 0.0,
            max_frame_time: 0.25,
            last: now,
            start: now,
            total: 0.0,
            fps: 0,
            frame_counter: 0,
            fps_timer: 0.0,
            cpu_max: 0.0,
        }
    }

    /// Record the main-thread cost (seconds) of the frame just rendered. Call
    /// once per frame after submitting the GPU work.
    pub fn record_cpu(&mut self, cpu: f32) {
        if cpu > self.cpu_max {
            self.cpu_max = cpu;
        }
    }

    pub fn fixed_dt(&self) -> f32 {
        self.fixed_dt
    }

    /// Total elapsed time in seconds since startup.
    pub fn total(&self) -> f64 {
        self.total
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    /// Advance real time by one render frame, feeding the accumulator. Returns
    /// the frame's real delta time in seconds (capped at `max_frame_time`).
    pub fn frame(&mut self) -> f32 {
        let now = Instant::now();
        let frame_time_raw = now.duration_since(self.last).as_secs_f32();
        self.last = now;
        self.total = now.duration_since(self.start).as_secs_f64();

        let mut frame_time = frame_time_raw.min(self.max_frame_time);

        // Vsync snapping. Real frame times wobble by a fraction of a millisecond
        // around the refresh interval, so the accumulator slowly drifts and
        // every so often a frame consumes 0 fixed steps (object frozen) and the
        // next consumes 2 (object jumps) — the fixed-timestep "beat" judder. If
        // `frame_time` is within `snap_window` of a whole multiple of
        // `fixed_dt`, snap it exactly onto that multiple so a 60 Hz display
        // feeds the 60 Hz sim exactly one step per frame. Genuinely slow frames
        // (far from any multiple) pass through unchanged.
        let multiple = (frame_time / self.fixed_dt).round();
        if multiple >= 1.0 {
            let snapped = multiple * self.fixed_dt;
            if (frame_time - snapped).abs() < self.fixed_dt * 0.1 {
                frame_time = snapped;
            }
        }

        self.accumulator += frame_time;

        // FPS sampling, once per real second.
        self.frame_counter += 1;
        self.fps_timer += frame_time;
        if self.fps_timer >= 1.0 {
            self.fps = self.frame_counter;
            log_fps(self.fps, self.cpu_max * 1000.0);
            self.frame_counter = 0;
            self.fps_timer = 0.0;
            self.cpu_max = 0.0;
        }

        frame_time
    }

    /// Consume one fixed step from the accumulator if enough time is available.
    /// Call in a `while time.next_fixed_step() { game.update(...) }` loop.
    pub fn next_fixed_step(&mut self) -> bool {
        if self.accumulator >= self.fixed_dt {
            self.accumulator -= self.fixed_dt;
            true
        } else {
            false
        }
    }
}
