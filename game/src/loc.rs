//! Centralized localization loaded from `assets/translations.json`.
//!
//! All user-facing strings live in the JSON file, organized by language code
//! (`en`, `pt`, `ar`). Adding a new language only requires adding a new entry
//! there; no Rust code needs to change.

use std::collections::HashMap;
use std::sync::OnceLock;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Lang {
    #[default]
    English,
    Portuguese,
    Arabic,
}

impl Lang {
    pub const fn code(self) -> &'static str {
        match self {
            Lang::English => "en",
            Lang::Portuguese => "pt",
            Lang::Arabic => "ar",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Lang::English => Lang::Portuguese,
            Lang::Portuguese => Lang::Arabic,
            Lang::Arabic => Lang::English,
        }
    }
}

#[derive(Clone, Copy)]
pub struct Loc {
    lang: Lang,
}

static STRINGS: OnceLock<HashMap<String, HashMap<String, String>>> = OnceLock::new();

fn strings() -> &'static HashMap<String, HashMap<String, String>> {
    STRINGS.get_or_init(|| {
        let json = include_str!("../assets/translations.json");
        serde_json::from_str(json).expect("assets/strings.json must be valid JSON")
    })
}

impl Loc {
    pub fn new(lang: Lang) -> Self {
        Self { lang }
    }

    pub fn set(&mut self, lang: Lang) {
        self.lang = lang;
    }

    pub fn lang(&self) -> Lang {
        self.lang
    }

    fn get<'a>(&self, key: &'a str) -> &'a str {
        strings()
            .get(self.lang.code())
            .and_then(|map| map.get(key))
            .map(|s| s.as_str())
            .unwrap_or(key)
    }

    // Title screen
    pub fn game_title(&self) -> &str { self.get("game_title") }
    pub fn play(&self) -> &str { self.get("play") }
    pub fn config(&self) -> &str { self.get("config") }
    pub fn instructions(&self) -> &str { self.get("instructions") }
    pub fn credits(&self) -> &str { self.get("credits") }
    pub fn quit(&self) -> &str { self.get("quit") }
    pub fn menu_hint(&self) -> &str { self.get("menu_hint") }
    pub fn back_hint(&self) -> &str { self.get("back_hint") }

    // Sub-screen titles
    pub fn config_title(&self) -> &str { self.get("config_title") }
    pub fn instructions_title(&self) -> &str { self.get("instructions_title") }
    pub fn credits_title(&self) -> &str { self.get("credits_title") }
    pub fn fullscreen_key(&self) -> &str { self.get("fullscreen_key") }
    pub fn zoom_key(&self) -> &str { self.get("zoom_key") }
    pub fn config_placeholder(&self) -> &str { self.get("config_placeholder") }

    // Instructions
    pub fn inst_move(&self) -> &str { self.get("inst_move") }
    pub fn inst_portals(&self) -> &str { self.get("inst_portals") }
    pub fn inst_zoom(&self) -> &str { self.get("inst_zoom") }
    pub fn inst_pause(&self) -> &str { self.get("inst_pause") }
    pub fn inst_objective(&self) -> &str { self.get("inst_objective") }
    pub fn inst_box(&self) -> &str { self.get("inst_box") }

    // Pause / end screens
    pub fn paused(&self) -> &str { self.get("paused") }
    pub fn defeat(&self) -> &str { self.get("defeat") }
    pub fn win(&self) -> &str { self.get("win") }
    pub fn pause_prompt(&self) -> &str { self.get("pause_prompt") }
    pub fn retry_prompt(&self) -> &str { self.get("retry_prompt") }
    pub fn play_again_prompt(&self) -> &str { self.get("play_again_prompt") }

    // HUD
    pub fn fps(&self, value: u32) -> String {
        format!("{}: {}", self.get("fps_label"), value)
    }

    pub fn squeezed(&self, value: u32) -> String {
        format!("{}: {}", self.get("squeezed_label"), value)
    }

    pub fn hud_controls(&self) -> &str { self.get("hud_controls") }
}
