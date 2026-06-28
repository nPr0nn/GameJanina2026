//! Level loading/creation, sprite scanning, tag colors.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use juni::prelude::*;

use crate::constants::TAG_PALETTE;
use crate::id::random_id;

/// Result of loading (or creating) a level from disk.
#[derive(Debug)]
pub(crate) struct LoadedLevel {
    pub(crate) path: String,
    pub(crate) level: Level,
    pub(crate) status: String,
}

/// Build `tag_colors` from level data, ensuring `"static"` always gets the
/// first palette slot.
pub(crate) fn build_tag_colors(level: &Level) -> HashMap<String, Color> {
    let mut map = HashMap::new();
    map.insert("static".to_string(), TAG_PALETTE[0]);
    for entry in &level.classifications {
        if !map.contains_key(&entry.tag) {
            let idx = map.len() % TAG_PALETTE.len();
            map.insert(entry.tag.clone(), TAG_PALETTE[idx]);
        }
    }
    map
}

pub(crate) fn scan_sprites(dir: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<String> = entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
        })
        .map(|e| e.path().to_string_lossy().replace('\\', "/"))
        .collect();
    paths.sort();
    paths
}

pub(crate) fn load_or_create_level(path: &str) -> io::Result<LoadedLevel> {
    if Path::new(path).exists() {
        let mut level = Level::load(path)?;
        level.ensure_ids(random_id);
        let sprite_n = level.sprite_instances.len();
        let collision_n = level.collision_shapes.len();
        let class_n = level.classifications.len();
        Ok(LoadedLevel {
            path: path.to_string(),
            level,
            status: format!(
                "Loaded {path} ({sprite_n} sprites, {collision_n} collision, {class_n} tags)"
            ),
        })
    } else {
        if let Some(parent) = Path::new(path)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let level = Level::new();
        level.save(path)?;
        Ok(LoadedLevel {
            path: path.to_string(),
            level,
            status: format!("Created {path} (new level)"),
        })
    }
}
