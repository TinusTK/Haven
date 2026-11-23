//! What Haven and Haven Hub hand each other: a whole setup (sign-ins,
//! sources, keys, watchlist and watch progress) as one JSON document, and
//! reading it into or out of a [`Store`].
//!
//! Add-ons are part of the document only as raw records (address and
//! description). A build without add-ons sends none and leaves any it is
//! given alone.
//!
//! A setup is either added to a store ([`Setup::merge_into`], nothing is
//! removed) or, when `replace` is set ("Send everything to the Hub"), the
//! store is made to match it ([`Setup::mirror_into`]): servers, add-ons and
//! TV sources it lacks are removed, and its on/off switches are taken.

use serde::{Deserialize, Serialize};

use crate::{
    Result,
    settings::{Keys, SwitchedOff},
    storage::{Progress, Saved, Store, StoredAddon, StoredServer, TvSource},
};

/// The port Haven Hub listens on unless told otherwise.
pub const DEFAULT_PORT: u16 = 8590;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Setup {
    #[serde(default)]
    pub servers: Vec<ServerRecord>,
    #[serde(default)]
    pub sources: Vec<SourceRecord>,
    #[serde(default)]
    pub tv_sources: Vec<TvRecord>,
    #[serde(default)]
    pub keys: Option<Keys>,
    #[serde(default)]
    pub watchlist: Vec<SavedRecord>,
    #[serde(default)]
    pub progress: Vec<ProgressRecord>,
    /// The Trakt and Simkl sign-ins, as kept (a list of accounts, each
    /// with a `service` and, for Trakt, `expires_at`).
    #[serde(default)]
    pub tracking: Vec<serde_json::Value>,
    /// What is switched off on the device that wrote it (None from older
    /// versions, which did not say).
    #[serde(default)]
    pub off: Option<OffRecord>,
    /// The receiver should match this setup, not only add to it.
    #[serde(default)]
    pub replace: bool,
    /// The sender has add-ons: a build without them leaves the receiver's
    /// add-ons alone, even when matching.
    #[serde(default)]
    pub has_sources: bool,
}

/// Switched-off things, named the same way on every device (a server's
/// number in the database differs from device to device).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OffRecord {
    pub servers: Vec<ServerKey>,
    pub addons: Vec<String>,
    pub tv_sources: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerKey {
    pub kind: String,
    pub address: String,
}

/// What is switched off in `store`, by address.
fn off_record(store: &Store, include: Include) -> Result<OffRecord> {
    let off = SwitchedOff::load(store)?;
    Ok(OffRecord {
        servers: store
            .servers()?
            .into_iter()
            .filter(|s| off.servers.contains(&s.id) && (include.hubs || s.kind != "hub"))
            .map(|s| ServerKey { kind: s.kind, address: s.address })
            .collect(),
        addons: if include.sources { off.addons } else { Vec::new() },
        tv_sources: off.tv_sources,
    })
}

/// The key tracking sign-ins are kept under in a store.
const TRACKING: &str = "tracking_accounts";

/// What [`Setup::merge_into`] added.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Merged {
    /// How many things were new.
    pub added: usize,
    /// Films and episodes newly marked watched: kind, title, video.
    pub watched: Vec<(String, String, String)>,
    /// Titles newly on the Watchlist: kind, title.
    pub watchlist: Vec<(String, String)>,
    /// A tracking sign-in was added or replaced by a newer one.
    pub tracking: bool,
    /// Servers, add-ons and TV sources removed to match ([`Setup::mirror_into`]).
    pub removed: usize,
}

/// When a kept sign-in runs out (0 when it does not say).
fn expires(account: &serde_json::Value) -> i64 {
    account.get("expires_at").and_then(serde_json::Value::as_i64).unwrap_or(0)
}

fn service(account: &serde_json::Value) -> Option<&str> {
    account.get("service").and_then(serde_json::Value::as_str)
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerRecord {
    pub kind: String,
    pub name: String,
    pub address: String,
    pub user_id: String,
    pub token: String,
}

/// Never print the token.
impl std::fmt::Debug for ServerRecord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerRecord").field("kind", &self.kind).field("name", &self.name).finish_non_exhaustive()
    }
}

/// A catalogue source the user chose, as its address and its own
/// description (raw JSON).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRecord {
    pub url: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TvRecord {
    pub url: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedRecord {
    pub kind: String,
    pub title_id: String,
    pub title: String,
    pub year: String,
    pub poster: Option<String>,
    pub added_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProgressRecord {
    pub kind: String,
    pub title_id: String,
    pub video_id: String,
    pub title: String,
    pub detail: String,
    pub poster: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub finished: bool,
    pub updated_at: i64,
}

/// What to put in a setup read from a store.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Include {
    /// Catalogue sources (add-ons): only builds that have them.
    pub sources: bool,
    /// Haven Hub pairings (for a backup; a Hub is never handed to a Hub).
    pub hubs: bool,
}

impl Setup {
    /// Everything in `store` worth handing over.
    pub fn read(store: &Store, include: Include) -> Result<Self> {
        Ok(Self {
            servers: store
                .servers()?
                .into_iter()
                // A Hub is not handed to a Hub.
                .filter(|s| include.hubs || s.kind != "hub")
                .map(|s| ServerRecord {
                    kind: s.kind,
                    name: s.name,
                    address: s.address,
                    user_id: s.user_id,
                    token: s.token,
                })
                .collect(),
            sources: if include.sources {
                store
                    .addons()?
                    .into_iter()
                    .map(|a| SourceRecord { url: a.url, description: a.manifest })
                    .collect()
            } else {
                Vec::new()
            },
            tv_sources: store
                .tv_sources()?
                .into_iter()
                .map(|t| TvRecord { url: t.url, name: t.name })
                .collect(),
            keys: Some(Keys::load(store)?),
            watchlist: store
                .watchlist()?
                .into_iter()
                .map(|w| SavedRecord {
                    kind: w.kind,
                    title_id: w.title_id,
                    title: w.title,
                    year: w.year,
                    poster: w.poster,
                    added_at: w.added_at,
                })
                .collect(),
            progress: store
                .all_progress()?
                .into_iter()
                .map(|p| ProgressRecord {
                    kind: p.kind,
                    title_id: p.title_id,
                    video_id: p.video_id,
                    title: p.title,
                    detail: p.detail,
                    poster: p.poster,
                    position: p.position,
                    duration: p.duration,
                    finished: p.finished,
                    updated_at: p.updated_at,
                })
                .collect(),
            tracking: store.get::<Vec<serde_json::Value>>(TRACKING)?.unwrap_or_default(),
            off: Some(off_record(store, include)?),
            replace: false,
            has_sources: include.sources,
        })
    }

    /// Makes `store` match this setup: its servers, add-ons (when included)
    /// and TV sources become this setup's, the keys it gives replace the
    /// store's, and its on/off switches are taken. The watchlist, watch
    /// progress and tracking sign-ins are added as in [`Self::merge_into`],
    /// never removed.
    pub fn mirror_into(&self, store: &Store, include: Include) -> Result<Merged> {
        let include = Include { sources: include.sources && self.has_sources, ..include };
        let mut removed = 0;
        // Servers taken out only to be put back with a new sign-in.
        let mut renewed = 0;
        for h in store.servers()? {
            if h.kind == "hub" && !include.hubs {
                continue;
            }
            // Gone from the device, or signed in again (a new token).
            let same = self.servers.iter().any(|s| {
                s.kind == h.kind && s.address == h.address && s.name == h.name && s.user_id == h.user_id && s.token == h.token
            });
            if !same {
                store.remove_server(h.id)?;
                if self.servers.iter().any(|s| s.kind == h.kind && s.address == h.address) {
                    renewed += 1;
                } else {
                    removed += 1;
                }
            }
        }
        if include.sources {
            for h in store.addons()? {
                if !self.sources.iter().any(|a| a.url == h.url) {
                    store.remove_addon(&h.url)?;
                    removed += 1;
                }
            }
        }
        for h in store.tv_sources()? {
            match self.tv_sources.iter().find(|t| t.url == h.url) {
                Some(t) if t.name == h.name => {}
                Some(_) => store.remove_tv_source(&h.url)?,
                None => {
                    store.remove_tv_source(&h.url)?;
                    removed += 1;
                }
            }
        }
        if let Some(keys) = &self.keys {
            let mut mine = Keys::load(store)?;
            let take = |mine: &mut String, theirs: &String| {
                if !theirs.trim().is_empty() {
                    *mine = theirs.clone();
                }
            };
            take(&mut mine.tmdb, &keys.tmdb);
            take(&mut mine.trakt_id, &keys.trakt_id);
            take(&mut mine.trakt_secret, &keys.trakt_secret);
            take(&mut mine.simkl_id, &keys.simkl_id);
            take(&mut mine.omdb, &keys.omdb);
            mine.save(store)?;
        }
        let mut merged = self.merge_into(store, include)?;
        // Put back after a new sign-in: not new to the user.
        merged.added = merged.added.saturating_sub(renewed);
        if let Some(off) = &self.off {
            let mut mine = SwitchedOff::load(store)?;
            mine.servers = store
                .servers()?
                .into_iter()
                .filter(|h| off.servers.iter().any(|k| k.kind == h.kind && k.address == h.address))
                .map(|h| h.id)
                .collect();
            if include.sources {
                mine.addons = off.addons.clone();
            }
            mine.tv_sources = off.tv_sources.clone();
            mine.save(store)?;
        }
        merged.removed = removed;
        Ok(merged)
    }

    /// Adds this setup to `store`; what it adds comes switched on or off as
    /// the setup has it. Nothing is removed: servers, sources
    /// and TV sources already there by address are kept; watchlist
    /// entries are added; for progress the newer record of the two wins.
    /// Keys only fill in what is blank. A tracking sign-in is taken where
    /// the store has none for that service, or has an older one.
    pub fn merge_into(&self, store: &Store, include: Include) -> Result<Merged> {
        let mut merged = Merged::default();
        let mut added = 0;
        // What arrives switched off stays off here; this store's own
        // switches are not touched.
        let off_record = self.off.clone().unwrap_or_default();
        let mut off = SwitchedOff::load(store)?;
        let mut off_changed = false;
        let have: Vec<StoredServer> = store.servers()?;
        for s in &self.servers {
            if (s.kind == "hub" && !include.hubs) || have.iter().any(|h| h.address == s.address && h.kind == s.kind) {
                continue;
            }
            let id = store.save_server(&StoredServer {
                id: 0,
                kind: s.kind.clone(),
                name: s.name.clone(),
                address: s.address.clone(),
                user_id: s.user_id.clone(),
                token: s.token.clone(),
            })?;
            if off_record.servers.iter().any(|k| k.kind == s.kind && k.address == s.address) {
                off.servers.push(id);
                off_changed = true;
            }
            added += 1;
        }
        if include.sources {
            let have: Vec<StoredAddon> = store.addons()?;
            for a in &self.sources {
                if !have.iter().any(|h| h.url == a.url) {
                    store.save_addon(&a.url, &a.description)?;
                    if off_record.addons.contains(&a.url) && !off.addons.contains(&a.url) {
                        off.addons.push(a.url.clone());
                        off_changed = true;
                    }
                    added += 1;
                }
            }
        }
        let have: Vec<TvSource> = store.tv_sources()?;
        for t in &self.tv_sources {
            if !have.iter().any(|h| h.url == t.url) {
                store.save_tv_source(&TvSource { url: t.url.clone(), name: t.name.clone(), playlist: String::new() })?;
                if off_record.tv_sources.contains(&t.url) && !off.tv_sources.contains(&t.url) {
                    off.tv_sources.push(t.url.clone());
                    off_changed = true;
                }
                added += 1;
            }
        }
        if off_changed {
            off.save(store)?;
        }
        if let Some(keys) = &self.keys {
            let mut mine = Keys::load(store)?;
            let fill = |mine: &mut String, theirs: &String| {
                if mine.trim().is_empty() && !theirs.trim().is_empty() {
                    *mine = theirs.clone();
                }
            };
            fill(&mut mine.tmdb, &keys.tmdb);
            fill(&mut mine.trakt_id, &keys.trakt_id);
            fill(&mut mine.trakt_secret, &keys.trakt_secret);
            fill(&mut mine.simkl_id, &keys.simkl_id);
            fill(&mut mine.omdb, &keys.omdb);
            mine.save(store)?;
        }
        for w in &self.watchlist {
            if !store.in_watchlist(&w.kind, &w.title_id)? {
                store.add_to_watchlist(&Saved {
                    kind: w.kind.clone(),
                    title_id: w.title_id.clone(),
                    title: w.title.clone(),
                    year: w.year.clone(),
                    poster: w.poster.clone(),
                    added_at: w.added_at,
                })?;
                merged.watchlist.push((w.kind.clone(), w.title_id.clone()));
                added += 1;
            }
        }
        for p in &self.progress {
            let mine = store.progress(&p.kind, &p.title_id, &p.video_id)?;
            let was_finished = mine.as_ref().is_some_and(|m| m.finished);
            let newer = match &mine {
                Some(mine) => p.updated_at > mine.updated_at,
                None => true,
            };
            if newer {
                store.save_progress(&Progress {
                    kind: p.kind.clone(),
                    title_id: p.title_id.clone(),
                    video_id: p.video_id.clone(),
                    title: p.title.clone(),
                    detail: p.detail.clone(),
                    poster: p.poster.clone(),
                    position: p.position,
                    duration: p.duration,
                    finished: p.finished,
                    updated_at: p.updated_at,
                })?;
                if p.finished && !was_finished {
                    merged.watched.push((p.kind.clone(), p.title_id.clone(), p.video_id.clone()));
                }
                added += 1;
            }
        }
        if !self.tracking.is_empty() {
            let mut mine: Vec<serde_json::Value> = store.get(TRACKING)?.unwrap_or_default();
            for theirs in &self.tracking {
                let Some(name) = service(theirs) else { continue };
                match mine.iter().position(|m| service(m) == Some(name)) {
                    Some(at) if expires(theirs) > expires(&mine[at]) => {
                        mine[at] = theirs.clone();
                        merged.tracking = true;
                    }
                    Some(_) => {}
                    None => {
                        mine.push(theirs.clone());
                        merged.tracking = true;
                        added += 1;
                    }
                }
            }
            if merged.tracking {
                store.put(TRACKING, &mine)?;
            }
        }
        merged.added = added;
        Ok(merged)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(video: &str, position: f64, at: i64) -> Progress {
        Progress {
            kind: "series".into(),
            title_id: "tt1".into(),
            video_id: video.into(),
            title: "A Show".into(),
            detail: String::new(),
            poster: None,
            position,
            duration: 100.0,
            finished: false,
            updated_at: at,
        }
    }

    #[test]
    fn a_setup_goes_from_one_store_to_another_and_newer_progress_wins() {
        let from = Store::in_memory().unwrap();
        from.save_server(&StoredServer {
            id: 0,
            kind: "jellyfin".into(),
            name: "Den".into(),
            address: "http://192.168.1.9:8096".into(),
            user_id: "u".into(),
            token: "secret".into(),
        })
        .unwrap();
        from.save_addon("https://one.example/manifest.json", r#"{"id":"x","name":"One"}"#).unwrap();
        from.save_progress(&progress("tt1:1:1", 50.0, 20)).unwrap();
        from.save_progress(&progress("tt1:1:2", 10.0, 5)).unwrap();

        let to = Store::in_memory().unwrap();
        to.save_progress(&progress("tt1:1:2", 70.0, 30)).unwrap();

        let all = Include { sources: true, hubs: false };
        let setup = Setup::read(&from, all).unwrap();
        let json = serde_json::to_string(&setup).unwrap();
        let back: Setup = serde_json::from_str(&json).unwrap();
        assert_eq!(back, setup);

        // Without add-ons: the source is neither sent nor taken.
        let none = Include { sources: false, hubs: false };
        assert!(Setup::read(&from, none).unwrap().sources.is_empty());
        back.merge_into(&to, none).unwrap();
        assert!(to.addons().unwrap().is_empty());
        assert_eq!(to.servers().unwrap().len(), 1);
        assert_eq!(to.progress("series", "tt1", "tt1:1:1").unwrap().unwrap().position, 50.0);
        // Its own newer position stays.
        assert_eq!(to.progress("series", "tt1", "tt1:1:2").unwrap().unwrap().position, 70.0);
        // A second time adds nothing.
        assert_eq!(back.merge_into(&to, none).unwrap().added, 0);
        back.merge_into(&to, all).unwrap();
        assert_eq!(to.addons().unwrap().len(), 1);
    }

    #[test]
    fn the_newer_tracking_sign_in_wins() {
        let store = Store::in_memory().unwrap();
        store
            .put(TRACKING, &vec![serde_json::json!({"service": "Trakt", "token": "old", "expires_at": 10})])
            .unwrap();
        let setup = Setup {
            tracking: vec![
                serde_json::json!({"service": "Trakt", "token": "new", "expires_at": 20}),
                serde_json::json!({"service": "Simkl", "token": "s"}),
            ],
            progress: vec![ProgressRecord {
                kind: "movie".into(),
                title_id: "tt9".into(),
                video_id: "tt9".into(),
                title: "A Film".into(),
                detail: String::new(),
                poster: None,
                position: 0.0,
                duration: 0.0,
                finished: true,
                updated_at: 1,
            }],
            ..Setup::default()
        };
        let merged = setup.merge_into(&store, Include { sources: false, hubs: false }).unwrap();
        assert!(merged.tracking);
        assert_eq!(merged.watched, [("movie".to_owned(), "tt9".to_owned(), "tt9".to_owned())]);
        let kept: Vec<serde_json::Value> = store.get(TRACKING).unwrap().unwrap();
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|a| a["token"] == "new"));
        // An older one does not replace it.
        let older = Setup { tracking: vec![serde_json::json!({"service": "Trakt", "token": "old", "expires_at": 10})], ..Setup::default() };
        assert!(!older.merge_into(&store, Include { sources: false, hubs: false }).unwrap().tracking);
    }

    fn server(name: &str, address: &str, token: &str) -> StoredServer {
        StoredServer {
            id: 0,
            kind: "jellyfin".into(),
            name: name.into(),
            address: address.into(),
            user_id: "u".into(),
            token: token.into(),
        }
    }

    #[test]
    fn sending_everything_makes_the_receiver_match() {
        let all = Include { sources: true, hubs: false };
        let device = Store::in_memory().unwrap();
        let kept = device.save_server(&server("Den", "http://den", "new")).unwrap();
        device.save_server(&server("Attic", "http://attic", "a")).unwrap();
        device.save_addon("https://one/manifest.json", "{}").unwrap();
        device.save_tv_source(&TvSource { url: "http://tv".into(), name: "TV".into(), playlist: String::new() }).unwrap();
        let mut off = SwitchedOff::load(&device).unwrap();
        off.servers.push(kept);
        off.addons.push("https://one/manifest.json".into());
        off.save(&device).unwrap();

        let hub = Store::in_memory().unwrap();
        hub.save_server(&server("Den", "http://den", "old")).unwrap();
        hub.save_server(&server("Gone", "http://gone", "g")).unwrap();
        hub.save_addon("https://one/manifest.json", "{}").unwrap();
        hub.save_addon("https://two/manifest.json", "{}").unwrap();
        hub.save_tv_source(&TvSource { url: "http://old-tv".into(), name: "Old".into(), playlist: String::new() }).unwrap();

        let mut setup = Setup::read(&device, all).unwrap();
        setup.replace = true;
        let merged = setup.mirror_into(&hub, all).unwrap();
        // Gone, two and old-tv removed; Attic and TV added; Den only renewed.
        assert_eq!(merged.removed, 3);
        assert_eq!(merged.added, 2);
        let servers = hub.servers().unwrap();
        assert_eq!(servers.len(), 2);
        let den = servers.iter().find(|s| s.address == "http://den").unwrap();
        assert_eq!(den.token, "new");
        assert_eq!(hub.addons().unwrap().len(), 1);
        assert_eq!(hub.tv_sources().unwrap()[0].url, "http://tv");
        // The switches came along, by the Hub's own server number.
        let hub_off = SwitchedOff::load(&hub).unwrap();
        assert_eq!(hub_off.servers, [den.id]);
        assert_eq!(hub_off.addons, ["https://one/manifest.json"]);
        // Again: nothing changes.
        let again = setup.mirror_into(&hub, all).unwrap();
        assert_eq!((again.added, again.removed), (0, 0));
    }

    #[test]
    fn a_device_without_add_ons_leaves_the_receivers_alone() {
        let device = Store::in_memory().unwrap();
        let hub = Store::in_memory().unwrap();
        hub.save_addon("https://one/manifest.json", "{}").unwrap();
        let mut off = SwitchedOff::load(&hub).unwrap();
        off.addons.push("https://one/manifest.json".into());
        off.save(&hub).unwrap();
        let setup = Setup::read(&device, Include { sources: false, hubs: false }).unwrap();
        setup.mirror_into(&hub, Include { sources: true, hubs: false }).unwrap();
        assert_eq!(hub.addons().unwrap().len(), 1);
        assert_eq!(SwitchedOff::load(&hub).unwrap().addons.len(), 1);
    }

    #[test]
    fn what_arrives_switched_off_stays_off() {
        let setup = Setup {
            servers: vec![ServerRecord {
                kind: "plex".into(),
                name: "Den".into(),
                address: "http://den".into(),
                user_id: "u".into(),
                token: "t".into(),
            }],
            tv_sources: vec![TvRecord { url: "http://tv".into(), name: "TV".into() }],
            off: Some(OffRecord {
                servers: vec![ServerKey { kind: "plex".into(), address: "http://den".into() }],
                addons: Vec::new(),
                tv_sources: vec!["http://tv".into()],
            }),
            ..Setup::default()
        };
        let store = Store::in_memory().unwrap();
        setup.merge_into(&store, Include { sources: false, hubs: false }).unwrap();
        let off = SwitchedOff::load(&store).unwrap();
        assert_eq!(off.servers, [store.servers().unwrap()[0].id]);
        assert_eq!(off.tv_sources, ["http://tv"]);
    }
}
