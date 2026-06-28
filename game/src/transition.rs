//! A fade-to-black transition between screens.
//!
//! A transition runs in two halves: **fade out** (the current screen darkens to
//! black), then **fade in** (the new screen brightens from black). The screen
//! swap happens at the midpoint — the instant the view is fully black — so the
//! change is never visible. The owner drives it by calling [`Transition::update`]
//! each frame and drawing the overlay with [`Transition::alpha`].

/// Seconds each half of the fade takes. The full transition is twice this.
const FADE_SECONDS: f32 = 0.30;

/// The screens the game can be on. `Copy` so it's trivially passed around.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Menu,
    Gameplay,
    Pause,
    Defeat,
    Win,
    Instructions,
    Credits,
    Config,
}

enum Phase {
    /// No transition running.
    Idle,
    /// Darkening to black; `t` ramps 0→1. At 1 we switch to `target`.
    Out { target: Screen, t: f32 },
    /// Brightening from black; `t` ramps 0→1.
    In { t: f32 },
}

pub struct Transition {
    phase: Phase,
}

impl Transition {
    pub fn new() -> Self {
        Self { phase: Phase::Idle }
    }

    /// `true` while a fade is in progress. Callers should suppress normal input
    /// during this window so a transition can't be interrupted or stacked.
    pub fn is_active(&self) -> bool {
        !matches!(self.phase, Phase::Idle)
    }

    /// Begin fading out toward `target`. Ignored if a transition is already
    /// running, so a key mashed twice can't queue two switches.
    pub fn start(&mut self, target: Screen) {
        if !self.is_active() {
            self.phase = Phase::Out { target, t: 0.0 };
        }
    }

    /// Advance the fade by `dt` seconds. Returns `Some(target)` on the single
    /// frame the screen should switch (fully black, midpoint); otherwise `None`.
    pub fn update(&mut self, dt: f32) -> Option<Screen> {
        let step = dt / FADE_SECONDS;
        match &mut self.phase {
            Phase::Idle => None,
            Phase::Out { target, t } => {
                *t += step;
                if *t >= 1.0 {
                    let target = *target;
                    self.phase = Phase::In { t: 0.0 };
                    Some(target)
                } else {
                    None
                }
            }
            Phase::In { t } => {
                *t += step;
                if *t >= 1.0 {
                    self.phase = Phase::Idle;
                }
                None
            }
        }
    }

    /// The fade overlay's opacity this frame: `0.0` clear … `1.0` fully black.
    pub fn alpha(&self) -> f32 {
        match &self.phase {
            Phase::Idle => 0.0,
            Phase::Out { t, .. } => t.clamp(0.0, 1.0),
            Phase::In { t } => (1.0 - t).clamp(0.0, 1.0),
        }
    }
}
