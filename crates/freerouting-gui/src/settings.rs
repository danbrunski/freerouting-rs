//! Settings kept between runs. Window size and position come from eframe's
//! own persistence; this is the rest.
//!
//! Restoring is defensive: unknown fields and values this version does not
//! understand fall back to defaults, never to a panic. A removed control
//! once crashed KRT's plugin on close and lost the user's settings.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The key under which [`PersistedSettings`] is stored.
pub const STORAGE_KEY: &str = "freerouting_settings";
/// The version of the stored layout. A stored value with another version is
/// ignored as a whole.
pub const SCHEMA: u32 = 1;
/// Recent files kept.
pub const MAX_RECENT: usize = 10;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedSettings {
    pub schema: u32,
    /// Layer visibility by layer name, for every layer seen so far.
    pub layer_visible_by_name: BTreeMap<String, bool>,
    /// Item kind visibility by kind label.
    pub kind_visible: BTreeMap<String, bool>,
    pub show_labels: bool,
    pub last_dir: Option<PathBuf>,
    /// At most [`MAX_RECENT`], newest first.
    pub recent_files: Vec<PathBuf>,
    pub side_panel_width: Option<f32>,
    // G04: display options. New fields are `#[serde(default)]` through the
    // struct, so settings stored before them still load (the schema stays 1).
    /// A copper layer's visibility level in percent, by layer name.
    pub layer_level_by_name: BTreeMap<String, u8>,
    /// A kind of object's visibility level in percent, by its label.
    pub object_levels: BTreeMap<String, u8>,
    /// The automatic layer dimming slider; `None` is the default.
    pub dim: Option<u8>,
    /// The six virtual layers, by name.
    pub virtual_layers: BTreeMap<String, bool>,
    /// Colours that differ from the defaults, `key -> [r, g, b]` (see
    /// `look::Look::changed_colors`).
    pub colors: BTreeMap<String, [u8; 3]>,
    /// The unit of the status bar and the lists ("mil", "inch", "mm", "um").
    pub unit: Option<String>,
    /// The large crosshair.
    pub crosshair: bool,
}

impl Default for PersistedSettings {
    fn default() -> Self {
        PersistedSettings {
            schema: SCHEMA,
            layer_visible_by_name: BTreeMap::new(),
            kind_visible: BTreeMap::new(),
            show_labels: true,
            last_dir: None,
            recent_files: Vec::new(),
            side_panel_width: None,
            layer_level_by_name: BTreeMap::new(),
            object_levels: BTreeMap::new(),
            dim: None,
            virtual_layers: BTreeMap::new(),
            colors: BTreeMap::new(),
            unit: None,
            crosshair: false,
        }
    }
}

impl PersistedSettings {
    /// The stored settings, or the defaults if there are none or this version
    /// cannot use them.
    pub fn restore(storage: Option<&dyn eframe::Storage>) -> Self {
        let stored: Option<Self> = storage.and_then(|s| eframe::get_value(s, STORAGE_KEY));
        match stored {
            Some(s) if s.schema == SCHEMA => s.sanitized(),
            _ => Self::default(),
        }
    }

    pub fn save(&self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, self);
    }

    fn sanitized(mut self) -> Self {
        self.recent_files.truncate(MAX_RECENT);
        // A level is a percentage; anything else in a hand-edited file is
        // clamped rather than trusted.
        for level in self
            .layer_level_by_name
            .values_mut()
            .chain(self.object_levels.values_mut())
        {
            *level = (*level).min(100);
        }
        self.dim = self.dim.map(|d| d.min(100));
        if self
            .side_panel_width
            .is_some_and(|w| !w.is_finite() || w < 0.0)
        {
            self.side_panel_width = None;
        }
        self
    }

    /// Puts `path` at the front of the recent files.
    pub fn remember_file(&mut self, path: PathBuf) {
        self.recent_files.retain(|p| *p != path);
        self.recent_files.insert(0, path);
        self.recent_files.truncate(MAX_RECENT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recent_files_are_unique_newest_first_and_capped() {
        let mut s = PersistedSettings::default();
        for i in 0..12 {
            s.remember_file(PathBuf::from(format!("{i}.dsn")));
        }
        s.remember_file(PathBuf::from("5.dsn"));
        assert_eq!(s.recent_files.len(), MAX_RECENT);
        assert_eq!(s.recent_files[0], PathBuf::from("5.dsn"));
        assert_eq!(
            s.recent_files
                .iter()
                .filter(|p| p.ends_with("5.dsn"))
                .count(),
            1
        );
    }
}
