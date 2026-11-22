//! A backup of Haven: the whole setup (media servers and their sign-ins,
//! Trakt and Simkl, keys, TV sources, Watchlist, what was watched) and the
//! settings, as one JSON file. Restoring adds what is missing and never
//! removes anything, so it is safe to run on a device already in use.

use std::{collections::BTreeMap, path::Path};

use serde::{Deserialize, Serialize};

use crate::{
    Result,
    hub::{Include, Merged, Setup},
    storage::Store,
};

/// The name the backup file goes by.
pub const FILE_NAME: &str = "Haven backup.json";

/// Settings that travel with a backup (others belong to one device).
const SETTINGS: &[&str] =
    &["settings", "sync_options", "external_player", "country", "tv_favourites", "player", "update_check_at_start"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    /// Always "Haven backup": tells a backup from any other JSON file.
    pub kind: String,
    pub version: u32,
    /// When it was made, in seconds since 1970.
    pub made_at: i64,
    pub setup: Setup,
    #[serde(default)]
    pub settings: BTreeMap<String, serde_json::Value>,
}

const KIND: &str = "Haven backup";

impl Backup {
    /// A backup of `store`, made at `now` (seconds since 1970).
    pub fn of(store: &Store, include: Include, now: i64) -> Result<Self> {
        let mut settings = BTreeMap::new();
        for key in SETTINGS {
            if let Some(value) = store.get::<serde_json::Value>(key)? {
                settings.insert((*key).to_owned(), value);
            }
        }
        Ok(Self { kind: KIND.to_owned(), version: 1, made_at: now, setup: Setup::read(store, include)?, settings })
    }

    /// Reads a backup file's text; `None` if it is not a Haven backup.
    pub fn parse(text: &str) -> Option<Self> {
        serde_json::from_str::<Self>(text).ok().filter(|backup| backup.kind == KIND)
    }

    /// Writes it to `path`, replacing the file only once the new one is
    /// complete (a crash halfway leaves the old backup whole).
    pub fn write(&self, path: &Path) -> std::io::Result<()> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)?;
        }
        let text = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let partial = path.with_extension("json.part");
        std::fs::write(&partial, text)?;
        std::fs::rename(&partial, path)
    }

    /// Adds this backup to `store`. Settings are put back only where the
    /// store has none of its own yet (a fresh install).
    pub fn restore_into(&self, store: &Store, include: Include) -> Result<Merged> {
        for (key, value) in &self.settings {
            if SETTINGS.contains(&key.as_str()) && store.get::<serde_json::Value>(key)?.is_none() {
                store.put(key, value)?;
            }
        }
        self.setup.merge_into(store, include)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredServer;

    #[test]
    fn a_backup_round_trips_through_a_file() {
        let from = Store::in_memory().unwrap();
        from.save_server(&StoredServer {
            id: 0,
            kind: "jellyfin".into(),
            name: "Den".into(),
            address: "http://192.168.1.9:8096".into(),
            user_id: "u".into(),
            token: "t".into(),
        })
        .unwrap();
        from.put("settings", &serde_json::json!({"theme": "light", "last_page": 2})).unwrap();
        from.put("device_id", &"abc").unwrap();
        let include = Include { sources: true, hubs: false };
        let backup = Backup::of(&from, include, 100).unwrap();
        assert!(!backup.settings.contains_key("device_id"));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub").join(FILE_NAME);
        backup.write(&path).unwrap();
        let read = Backup::parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(read, backup);
        assert!(Backup::parse(r#"{"something": "else"}"#).is_none());

        let to = Store::in_memory().unwrap();
        let merged = read.restore_into(&to, include).unwrap();
        assert_eq!(merged.added, 1);
        assert_eq!(to.servers().unwrap()[0].name, "Den");
        assert_eq!(to.get::<serde_json::Value>("settings").unwrap().unwrap()["theme"], "light");
    }
}
