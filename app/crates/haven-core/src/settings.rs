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

const KEYS: &str = "keys";

/// The keys the user typed into Settings for the outside services Haven
/// can use. Empty text means "not given".
#[derive(Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Keys {
    pub tmdb: String,
    pub trakt_id: String,
    pub trakt_secret: String,
    pub simkl_id: String,
    /// OMDb (omdbapi.com): Rotten Tomatoes and Metacritic scores, the age rating.
    pub omdb: String,
}

/// Never print the values.
impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Keys { .. }")
    }
}

impl Keys {
    pub fn load(store: &Store) -> Result<Self> {
        Ok(store.get(KEYS)?.unwrap_or_default())
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.put(KEYS, self)
    }

    /// The keys to use: what was typed into Settings first, then the
    /// secrets file, then keys built into this copy of Haven.
    pub fn resolved(&self, file: &crate::secrets::Secrets) -> crate::secrets::Secrets {
        let pick = |typed: &str, filed: &Option<String>, built_in: Option<&str>| {
            Some(typed.trim())
                .filter(|typed| !typed.is_empty())
                .map(str::to_owned)
                .or_else(|| filed.clone())
                .or_else(|| built_in.filter(|key| !key.is_empty()).map(str::to_owned))
        };
        crate::secrets::Secrets {
            tmdb_api_key: pick(&self.tmdb, &file.tmdb_api_key, option_env!("HAVEN_TMDB_API_KEY")),
            trakt_client_id: pick(
                &self.trakt_id,
                &file.trakt_client_id,
                option_env!("HAVEN_TRAKT_CLIENT_ID"),
            ),
            trakt_client_secret: pick(
                &self.trakt_secret,
                &file.trakt_client_secret,
                option_env!("HAVEN_TRAKT_CLIENT_SECRET"),
            ),
            simkl_client_id: pick(
                &self.simkl_id,
                &file.simkl_client_id,
                option_env!("HAVEN_SIMKL_CLIENT_ID"),
            ),
            omdb_api_key: pick(&self.omdb, &file.omdb_api_key, option_env!("HAVEN_OMDB_API_KEY")),
        }
    }
}

const SWITCHED_OFF: &str = "switched_off";

/// The add-ons, media servers and television sources the user has
/// switched off. They stay installed and signed in; Haven just leaves
/// them alone until they are switched on again.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SwitchedOff {
    /// Add-ons, by manifest address.
    pub addons: Vec<String>,
    /// Media servers, by their number in the database.
    pub servers: Vec<i64>,
    /// Playlists and Xtream Codes accounts, by the address they are kept under.
    pub tv_sources: Vec<String>,
}

impl SwitchedOff {
    pub fn load(store: &Store) -> Result<Self> {
        Ok(store.get(SWITCHED_OFF)?.unwrap_or_default())
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.put(SWITCHED_OFF, self)
    }
}

/// Puts `item` on a list of switched-off things, or takes it off.
pub fn switch<T: PartialEq>(off: &mut Vec<T>, item: T, on: bool) {
    off.retain(|known| *known != item);
    if !on {
        off.push(item);
    }
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
    fn typed_keys_win_over_the_secrets_file() {
        let store = Store::in_memory().unwrap();
        assert_eq!(Keys::load(&store).unwrap(), Keys::default());
        let keys = Keys { tmdb: " typed ".into(), ..Keys::default() };
        keys.save(&store).unwrap();
        let file = crate::secrets::Secrets::parse("TMDB_API_KEY=filed\nSIMKL_CLIENT_ID=simkl");
        let resolved = Keys::load(&store).unwrap().resolved(&file);
        assert_eq!(resolved.tmdb_api_key.as_deref(), Some("typed"));
        assert_eq!(resolved.simkl_client_id.as_deref(), Some("simkl"));
        assert!(!format!("{keys:?}").contains("typed"));
    }

    #[test]
    fn switching_off_is_remembered_and_undone() {
        let store = Store::in_memory().unwrap();
        let mut off = SwitchedOff::load(&store).unwrap();
        assert_eq!(off, SwitchedOff::default());
        switch(&mut off.servers, 3, false);
        switch(&mut off.servers, 3, false);
        switch(&mut off.addons, "https://a.example/manifest.json".to_owned(), false);
        off.save(&store).unwrap();
        let mut off = SwitchedOff::load(&store).unwrap();
        assert_eq!(off.servers, [3]);
        switch(&mut off.servers, 3, true);
        assert!(off.servers.is_empty());
        assert_eq!(off.addons.len(), 1);
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
