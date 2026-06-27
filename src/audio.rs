//! Audio playback via the [`kira`](https://docs.rs/kira) crate.
//!
//! [`Audio`] owns the mixer/output stream; [`Sound`] is a decoded clip. Load a
//! clip once with [`Context::load_sound_from_memory`](crate::Context::load_sound_from_memory)
//! and play it with [`Context::play_sound`](crate::Context::play_sound)
//! (raylib's `LoadSound` / `PlaySound`).
//!
//! kira's `cpal` backend works on every target: native uses the OS audio API,
//! wasm drives the Web Audio API. On the web the browser only lets an
//! `AudioContext` start once the page has had a user gesture, and it will *not*
//! resume a context that was created earlier. So we create the manager lazily on
//! the first [`play`](Audio::play) — by then a click/keypress has happened and
//! the freshly created context starts in `running` rather than `suspended`.

use kira::sound::static_sound::StaticSoundData;
use kira::{AudioManager, AudioManagerSettings, DefaultBackend};
use std::io::Cursor;

/// Owns the audio output stream and mixer. Created lazily on the first
/// [`play`](Audio::play) (see the module docs for why).
pub struct Audio {
    /// The output stream/mixer. `None` until the first play; stays `None` if
    /// `failed` is set.
    manager: Option<AudioManager>,
    /// Set once if opening the output device errored, so we don't retry (and
    /// re-log) on every subsequent play.
    failed: bool,
}

/// A decoded sound clip. Cheap to clone (kira stores samples behind an `Arc`);
/// `None` if decoding failed.
#[derive(Clone)]
pub struct Sound {
    data: Option<StaticSoundData>,
}

impl Audio {
    pub(crate) fn new() -> Self {
        // Deliberately does NOT open the device yet — see the module docs: on
        // the web the AudioContext must be created after a user gesture or it
        // stays muted. `ensure_manager` opens it on first play.
        Self {
            manager: None,
            failed: false,
        }
    }

    /// Open the output device on first use, returning the manager (or `None` if
    /// it already failed once).
    fn ensure_manager(&mut self) -> Option<&mut AudioManager> {
        if self.manager.is_none() && !self.failed {
            match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
                Ok(manager) => self.manager = Some(manager),
                Err(e) => {
                    log::error!("juni: failed to initialize audio: {e}");
                    self.failed = true;
                }
            }
        }
        self.manager.as_mut()
    }

    /// Decode WAV `bytes` into a [`Sound`]. Errors are logged and yield a silent
    /// clip rather than failing the caller.
    pub(crate) fn load(&self, bytes: &[u8]) -> Sound {
        // Copy into an owned buffer so the cursor satisfies kira's `'static`
        // bound regardless of the input lifetime.
        let data = match StaticSoundData::from_cursor(Cursor::new(bytes.to_vec())) {
            Ok(data) => Some(data),
            Err(e) => {
                log::error!("juni: failed to decode sound: {e}");
                None
            }
        };
        Sound { data }
    }

    /// Play a one-shot instance of `sound`. Overlapping plays mix.
    pub(crate) fn play(&mut self, sound: &Sound) {
        let Some(data) = sound.data.clone() else {
            return;
        };
        if let Some(manager) = self.ensure_manager() {
            if let Err(e) = manager.play(data) {
                log::error!("juni: failed to play sound: {e}");
            }
        }
    }
}
