use serde::{Deserialize, Serialize};

use crate::{Result, storage::Store};

const KEY: &str = "settings";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    Dark,
    Light,
}

/// User preferences. Every field has a default so settings saved by an
/// older build still load after new fields are added.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: Theme,
    /// Index of the page shown when Haven was last closed.
    pub last_page: u32,
}

impl Settings {
    pub fn load(store: &Store) -> Result<Self> {
        Ok(store.get(KEY)?.unwrap_or_default())
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.put(KEY, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_nothing_saved() {
        let store = Store::in_memory().unwrap();
        assert_eq!(Settings::load(&store).unwrap(), Settings::default());
    }

    #[test]
    fn saves_and_loads() {
        let store = Store::in_memory().unwrap();
        let settings = Settings {
            theme: Theme::Light,
            last_page: 3,
        };
        settings.save(&store).unwrap();
        assert_eq!(Settings::load(&store).unwrap(), settings);
    }

    #[test]
    fn older_saved_settings_still_load() {
        let store = Store::in_memory().unwrap();
        store
            .put(KEY, &serde_json::json!({ "theme": "light" }))
            .unwrap();
        let loaded = Settings::load(&store).unwrap();
        assert_eq!(loaded.theme, Theme::Light);
        assert_eq!(loaded.last_page, 0);
    }
}
