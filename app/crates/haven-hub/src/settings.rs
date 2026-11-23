//! The settings the Hub's own page changes: media servers, add-ons and
//! Live TV lists (switch on or off, remove), the keys, and the Trakt and
//! Simkl links. "Send everything to the Hub" in Haven sets all of these
//! at once; the page changes one thing at a time.

use haven_core::{
    settings::{Keys, SwitchedOff, switch},
    storage::{StoredServer, TvSource},
};

use crate::hub::Hub;

/// A media server, Live TV list or add-on as the page lists it.
pub struct Row<Id> {
    pub id: Id,
    pub name: String,
    pub detail: String,
    pub on: bool,
}

/// Which keys are set (never their values).
pub struct KeysSet {
    pub tmdb: bool,
    pub omdb: bool,
    pub trakt_id: bool,
    pub trakt_secret: bool,
    pub simkl_id: bool,
}

impl Hub {
    fn off(&self) -> SwitchedOff {
        SwitchedOff::load(&self.store.lock().unwrap()).unwrap_or_default()
    }

    fn change_off(&self, change: impl FnOnce(&mut SwitchedOff)) {
        let store = self.store.lock().unwrap();
        let mut off = SwitchedOff::load(&store).unwrap_or_default();
        change(&mut off);
        let _ = off.save(&store);
    }

    /// Whether an add-on is switched on (the Hub uses only those).
    pub fn addon_on(&self, url: &str) -> bool {
        !self.off().addons.iter().any(|off| off == url)
    }

    pub fn server_rows(&self) -> Vec<Row<i64>> {
        let off = self.off();
        let servers: Vec<StoredServer> = self.store.lock().unwrap().servers().unwrap_or_default();
        servers
            .into_iter()
            .filter(|s| s.kind != "hub")
            .map(|s| Row { id: s.id, on: !off.servers.contains(&s.id), name: s.name, detail: format!("{} · {}", kind_words(&s.kind), s.address) })
            .collect()
    }

    pub fn switch_server(&self, id: i64, on: bool) {
        self.change_off(|off| switch(&mut off.servers, id, on));
    }

    pub fn remove_server(&self, id: i64) {
        let _ = self.store.lock().unwrap().remove_server(id);
        self.change_off(|off| off.servers.retain(|known| *known != id));
    }

    pub fn switch_addon(&self, url: &str, on: bool) {
        self.change_off(|off| switch(&mut off.addons, url.to_owned(), on));
    }

    pub fn tv_rows(&self) -> Vec<Row<String>> {
        let off = self.off();
        let lists: Vec<TvSource> = self.store.lock().unwrap().tv_sources().unwrap_or_default();
        lists
            .into_iter()
            .map(|t| Row { on: !off.tv_sources.contains(&t.url), name: t.name, detail: String::new(), id: t.url })
            .collect()
    }

    pub fn add_tv(&self, name: &str, url: &str) {
        let (name, url) = (name.trim(), url.trim());
        if url.is_empty() {
            return;
        }
        let name = if name.is_empty() { "Live TV" } else { name };
        let _ = self.store.lock().unwrap().save_tv_source(&TvSource { url: url.into(), name: name.into(), playlist: String::new() });
    }

    pub fn switch_tv(&self, url: &str, on: bool) {
        self.change_off(|off| switch(&mut off.tv_sources, url.to_owned(), on));
    }

    pub fn remove_tv(&self, url: &str) {
        let _ = self.store.lock().unwrap().remove_tv_source(url);
        self.change_off(|off| off.tv_sources.retain(|known| known != url));
    }

    pub fn keys_set(&self) -> KeysSet {
        let keys = Keys::load(&self.store.lock().unwrap()).unwrap_or_default();
        let set = |value: &str| !value.trim().is_empty();
        KeysSet {
            tmdb: set(&keys.tmdb),
            omdb: set(&keys.omdb),
            trakt_id: set(&keys.trakt_id),
            trakt_secret: set(&keys.trakt_secret),
            simkl_id: set(&keys.simkl_id),
        }
    }

    /// Saves the keys typed on the page; a box left empty keeps its key.
    pub fn save_keys(&self, typed: &Keys) {
        let store = self.store.lock().unwrap();
        let mut keys = Keys::load(&store).unwrap_or_default();
        let take = |mine: &mut String, typed: &str| {
            if !typed.trim().is_empty() {
                *mine = typed.trim().to_owned();
            }
        };
        take(&mut keys.tmdb, &typed.tmdb);
        take(&mut keys.omdb, &typed.omdb);
        take(&mut keys.trakt_id, &typed.trakt_id);
        take(&mut keys.trakt_secret, &typed.trakt_secret);
        take(&mut keys.simkl_id, &typed.simkl_id);
        let _ = keys.save(&store);
    }

    /// Forgets the Hub's Trakt or Simkl sign-in ("Trakt", "Simkl").
    pub fn unlink(&self, service: &str) {
        let store = self.store.lock().unwrap();
        let mut accounts: Vec<serde_json::Value> =
            store.get(haven_tracking::sync::ACCOUNTS).ok().flatten().unwrap_or_default();
        accounts.retain(|a| a.get("service").and_then(serde_json::Value::as_str) != Some(service));
        let _ = store.put(haven_tracking::sync::ACCOUNTS, &accounts);
    }
}

fn kind_words(kind: &str) -> &str {
    match kind {
        "jellyfin" => "Jellyfin",
        "plex" => "Plex",
        "emby" => "Emby",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use haven_core::storage::Store;

    use super::*;

    fn hub() -> Hub {
        Hub::new(Store::in_memory().unwrap(), haven_core::http::client().unwrap(), "Den".into())
    }

    #[test]
    fn servers_and_lists_switch_and_go() {
        let hub = hub();
        let id = hub
            .store
            .lock()
            .unwrap()
            .save_server(&StoredServer {
                id: 0,
                kind: "plex".into(),
                name: "Den".into(),
                address: "http://den".into(),
                user_id: "u".into(),
                token: "t".into(),
            })
            .unwrap();
        hub.switch_server(id, false);
        assert!(!hub.server_rows()[0].on);
        hub.remove_server(id);
        assert!(hub.server_rows().is_empty());
        assert!(hub.off().servers.is_empty());

        hub.add_tv("", "http://tv/list.m3u");
        hub.switch_tv("http://tv/list.m3u", false);
        assert_eq!(hub.tv_rows()[0].name, "Live TV");
        assert!(!hub.tv_rows()[0].on);
        hub.remove_tv("http://tv/list.m3u");
        assert!(hub.tv_rows().is_empty());

        hub.switch_addon("https://one/manifest.json", false);
        assert!(!hub.addon_on("https://one/manifest.json"));
    }

    #[test]
    fn an_empty_key_box_keeps_the_key() {
        let hub = hub();
        hub.save_keys(&Keys { tmdb: "abc".into(), ..Keys::default() });
        hub.save_keys(&Keys { omdb: "x".into(), ..Keys::default() });
        let keys = Keys::load(&hub.store.lock().unwrap()).unwrap();
        assert_eq!((keys.tmdb.as_str(), keys.omdb.as_str()), ("abc", "x"));
        assert!(hub.keys_set().tmdb && !hub.keys_set().simkl_id);
    }
}
