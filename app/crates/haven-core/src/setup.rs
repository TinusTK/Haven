//! A whole setup (sign-ins, sources, keys, watchlist and watch
//! progress, and what is switched off) as one JSON document, and reading
//! it into or out of a [`Store`]. Backups are made of it.
//!
//! Add-ons are part of the document only as raw records (address and
//! description). A build without add-ons saves none and leaves any it is
//! given alone.

use serde::{Deserialize, Serialize};

use crate::{
    Result,
    settings::{Keys, SwitchedOff},
    storage::{Progress, Saved, Store, StoredAddon, StoredServer, TvSource},
};

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
            .filter(|s| off.servers.contains(&s.id))
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
}

impl Setup {
    /// Everything in `store` worth handing over.
    pub fn read(store: &Store, include: Include) -> Result<Self> {
        Ok(Self {
            servers: store
                .servers()?
                .into_iter()
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
        })
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
            if have.iter().any(|h| h.address == s.address && h.kind == s.kind) {
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

        let all = Include { sources: true };
        let setup = Setup::read(&from, all).unwrap();
        let json = serde_json::to_string(&setup).unwrap();
        let back: Setup = serde_json::from_str(&json).unwrap();
        assert_eq!(back, setup);

        // Without add-ons: the source is neither sent nor taken.
        let none = Include { sources: false };
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
        let merged = setup.merge_into(&store, Include { sources: false }).unwrap();
        assert!(merged.tracking);
        assert_eq!(merged.watched, [("movie".to_owned(), "tt9".to_owned(), "tt9".to_owned())]);
        let kept: Vec<serde_json::Value> = store.get(TRACKING).unwrap().unwrap();
        assert_eq!(kept.len(), 2);
        assert!(kept.iter().any(|a| a["token"] == "new"));
        // An older one does not replace it.
        let older = Setup { tracking: vec![serde_json::json!({"service": "Trakt", "token": "old", "expires_at": 10})], ..Setup::default() };
        assert!(!older.merge_into(&store, Include { sources: false }).unwrap().tracking);
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
        setup.merge_into(&store, Include { sources: false }).unwrap();
        let off = SwitchedOff::load(&store).unwrap();
        assert_eq!(off.servers, [store.servers().unwrap()[0].id]);
        assert_eq!(off.tv_sources, ["http://tv"]);
    }
}
