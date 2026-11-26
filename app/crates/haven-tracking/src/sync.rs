//! One round of keeping Haven and the linked accounts in step:
//!
//! 1. [`exchange`] (off the database, may take a while): renews tokens
//!    about to run out, sends the queued changes, and with `pull` brings
//!    in each account's Watchlist and what it says was watched.
//! 2. [`changes_since`] compares what each account holds with what it held
//!    at its last pull ([`Seen`]): what was added there comes in, and what
//!    was taken off there (off the Watchlist, or marked unwatched) goes
//!    here too. Only what changed on the account counts, so a title one
//!    account still lists does not come back after it was taken off here.
//! 3. [`add_listed`] and [`add_watched`] put what came in into a
//!    [`Store`], where it is new there.

use std::collections::{BTreeMap, BTreeSet};

use haven_core::storage::{Change, Progress, Saved, Store};
use serde::{Deserialize, Serialize};

use crate::{Account, Client, Listed, Service, Watched, subject};

/// The key the linked accounts are kept under in a store.
pub const ACCOUNTS: &str = "tracking_accounts";
/// The key what each account held at its last pull is kept under: a map
/// from the service's name to its [`Seen`].
pub const SEEN: &str = "sync_seen";

/// What one account held at its last pull, so the next pull can tell what
/// was added there and what was taken off. Keys from [`listed_key`] and
/// [`watched_key`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Seen {
    pub watchlist: BTreeSet<String>,
    pub watched: BTreeSet<String>,
}

/// Every account's [`Seen`], by the service's name.
pub type SeenByAccount = BTreeMap<String, Seen>;

/// "movie|tt1" or "series|tt1".
pub fn listed_key(title: &Listed) -> String {
    format!("{}|{}", if title.series { "series" } else { "movie" }, title.imdb)
}

/// "movie|tt1|tt1" for a film, "series|tt1|tt1:2:3" for an episode: the
/// kind, title and video ids Haven keeps them under.
pub fn watched_key(entry: &Watched) -> String {
    match entry.episode {
        Some((season, episode)) => format!("series|{0}|{0}:{season}:{episode}", entry.imdb),
        None => format!("movie|{0}|{0}", entry.imdb),
    }
}

/// The kind, title id and (for a watched key) video id in a key.
pub fn key_parts(key: &str) -> (&str, &str, &str) {
    let mut parts = key.splitn(3, '|');
    (parts.next().unwrap_or_default(), parts.next().unwrap_or_default(), parts.next().unwrap_or_default())
}

/// Whether a change for what `key` names is still waiting to be sent:
/// then this device's word stands and what the account says is left.
pub fn waiting(queued: &[Change], key: &str) -> bool {
    let (kind, title, video) = key_parts(key);
    queued.iter().any(|change| {
        change.kind == kind
            && change.title_id == title
            && (video.is_empty() || change.what.starts_with("watchlist") || change.video_id == video)
    })
}

/// What one account holds now. `None` where it could not be read: then
/// nothing is taken from that part.
#[derive(Debug, Clone)]
pub struct Pulled {
    pub service: Service,
    pub watchlist: Option<Vec<Listed>>,
    pub watched: Option<Vec<Watched>>,
}

/// What changed on one account since its last pull, and what to keep as
/// its new [`Seen`].
#[derive(Debug, Default)]
pub struct AccountChanges {
    pub added_listed: Vec<Listed>,
    /// Keys taken off the account's Watchlist.
    pub gone_listed: Vec<String>,
    pub added_watched: Vec<Watched>,
    /// Keys the account no longer has as watched.
    pub gone_watched: Vec<String>,
    pub seen: Seen,
}

/// Compares what an account holds with what it held at its last pull
/// (`None` the first time: then everything it holds is new and nothing
/// was taken off).
pub fn changes_since(previous: Option<&Seen>, pulled: &Pulled) -> AccountChanges {
    let mut changes = AccountChanges { seen: previous.cloned().unwrap_or_default(), ..AccountChanges::default() };
    if let Some(listed) = &pulled.watchlist {
        let now: BTreeSet<String> = listed.iter().map(listed_key).collect();
        changes.added_listed =
            listed.iter().filter(|title| previous.is_none_or(|p| !p.watchlist.contains(&listed_key(title)))).cloned().collect();
        if let Some(previous) = previous {
            changes.gone_listed = previous.watchlist.difference(&now).cloned().collect();
        }
        changes.seen.watchlist = now;
    }
    if let Some(watched) = &pulled.watched {
        let now: BTreeSet<String> = watched.iter().map(watched_key).collect();
        changes.added_watched =
            watched.iter().filter(|entry| previous.is_none_or(|p| !p.watched.contains(&watched_key(entry)))).cloned().collect();
        if let Some(previous) = previous {
            changes.gone_watched = previous.watched.difference(&now).cloned().collect();
        }
        changes.seen.watched = now;
    }
    changes
}

/// What one round brought back.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Queued changes every account now has (to take off the queue).
    pub sent: Vec<i64>,
    pub problems: Vec<String>,
    /// Accounts whose tokens were renewed (to keep in place of the old).
    pub renewed: Vec<Account>,
    /// What each account holds, when brought in.
    pub pulls: Vec<Pulled>,
    pub pulled: bool,
}

/// Renews, sends `queued` to every account, and with `pull` brings in what
/// the accounts hold.
pub async fn exchange(tracker: &Client, accounts: Vec<Account>, queued: Vec<Change>, pull: bool, now: i64) -> Outcome {
    let mut outcome = Outcome { pulled: pull, ..Outcome::default() };
    // A token about to run out is renewed first.
    let mut ready = Vec::new();
    for account in accounts {
        match tracker.renewed(&account, now).await {
            Ok(Some(renewed)) => {
                outcome.renewed.push(renewed.clone());
                ready.push(renewed);
            }
            Ok(None) => ready.push(account),
            Err(err) => outcome.problems.push(format!("{}: {err}", account.service.label())),
        }
    }
    // Each change goes to every account; it leaves the queue once all of
    // them have it (sending the same again does no harm).
    for change in queued {
        let video = (change.what == "watched" || change.what == "unwatched").then_some(change.video_id.as_str());
        let Some(subject) = subject(&change.kind, &change.title_id, video) else {
            outcome.sent.push(change.id);
            continue;
        };
        let mut all = true;
        for account in &ready {
            let sent = match change.what.as_str() {
                "watched" | "unwatched" if subject.series && subject.episode.is_none() => Ok(()),
                "watched" => tracker.mark_watched(account, &subject).await,
                "unwatched" => tracker.mark_unwatched(account, &subject).await,
                "watchlist_add" => tracker.set_watchlist(account, &subject, true).await,
                _ => tracker.set_watchlist(account, &subject, false).await,
            };
            if let Err(err) = sent {
                all = false;
                outcome.problems.push(format!("{}: {err}", account.service.label()));
            }
        }
        if all && !ready.is_empty() {
            outcome.sent.push(change.id);
        }
    }
    if pull {
        for account in &ready {
            let mut pulled = Pulled { service: account.service, watchlist: None, watched: None };
            match tracker.watchlist(account).await {
                Ok(listed) => pulled.watchlist = Some(listed),
                Err(err) => outcome.problems.push(format!("{}: {err}", account.service.label())),
            }
            match tracker.watched(account).await {
                Ok(watched) => pulled.watched = Some(watched),
                Err(err) => outcome.problems.push(format!("{}: {err}", account.service.label())),
            }
            outcome.pulls.push(pulled);
        }
    }
    outcome
}

/// Keeps renewed accounts in place of the old ones in `accounts`.
pub fn keep_renewed(accounts: &mut Vec<Account>, renewed: &[Account]) {
    for account in renewed {
        accounts.retain(|known| known.service != account.service);
        accounts.push(account.clone());
    }
}

/// Puts titles from an account's Watchlist on the store's, if new there.
/// `now` in seconds since 1970.
pub fn add_listed(store: &Store, listed: &[Listed], now: i64) -> usize {
    let mut added = 0;
    // Oldest first, so the list keeps the service's order.
    for (place, title) in listed.iter().rev().enumerate() {
        let kind = if title.series { "series" } else { "movie" };
        if store.in_watchlist(kind, &title.imdb).unwrap_or(true) {
            continue;
        }
        let saved = store.add_to_watchlist(&Saved {
            kind: kind.to_owned(),
            title_id: title.imdb.clone(),
            title: title.title.clone(),
            year: title.year.clone(),
            poster: None,
            added_at: now * 1000 + place as i64,
        });
        if saved.is_ok() {
            added += 1;
        }
    }
    added
}

/// Records what an account says was watched, where the store has nothing yet.
pub fn add_watched(store: &Store, watched: &[Watched]) -> usize {
    watched
        .iter()
        .filter(|entry| {
            let (kind, video_id, detail) = match entry.episode {
                Some((season, episode)) => {
                    ("series", format!("{}:{season}:{episode}", entry.imdb), format!("S{season} E{episode}"))
                }
                None => ("movie", entry.imdb.clone(), String::new()),
            };
            store
                .add_watched_elsewhere(&Progress {
                    kind: kind.to_owned(),
                    title_id: entry.imdb.clone(),
                    video_id,
                    title: entry.title.clone(),
                    detail,
                    poster: None,
                    position: 0.0,
                    duration: 0.0,
                    finished: true,
                    updated_at: entry.at.max(0) * 1000,
                })
                .unwrap_or(false)
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(imdb: &str) -> Listed {
        Listed { imdb: imdb.into(), series: false, title: imdb.into(), year: String::new() }
    }

    fn episode(imdb: &str, season: u32, episode: u32) -> Watched {
        Watched { imdb: imdb.into(), title: imdb.into(), episode: Some((season, episode)), at: 0 }
    }

    #[test]
    fn the_first_pull_brings_everything_in_and_takes_nothing_off() {
        let pulled = Pulled { service: Service::Trakt, watchlist: Some(vec![listed("tt1")]), watched: Some(vec![episode("tt2", 1, 1)]) };
        let changes = changes_since(None, &pulled);
        assert_eq!(changes.added_listed.len(), 1);
        assert_eq!(changes.added_watched.len(), 1);
        assert!(changes.gone_listed.is_empty() && changes.gone_watched.is_empty());
        assert!(changes.seen.watchlist.contains("movie|tt1"));
        assert!(changes.seen.watched.contains("series|tt2|tt2:1:1"));
    }

    #[test]
    fn only_what_changed_on_the_account_counts() {
        let before = Seen {
            watchlist: ["movie|tt1".to_owned(), "movie|tt2".to_owned()].into(),
            watched: ["series|tt9|tt9:1:1".to_owned(), "series|tt9|tt9:1:2".to_owned()].into(),
        };
        // tt2 was taken off there, tt3 added; episode 2 was unmarked there.
        let pulled = Pulled {
            service: Service::Trakt,
            watchlist: Some(vec![listed("tt1"), listed("tt3")]),
            watched: Some(vec![episode("tt9", 1, 1)]),
        };
        let changes = changes_since(Some(&before), &pulled);
        assert_eq!(changes.added_listed.iter().map(|t| t.imdb.as_str()).collect::<Vec<_>>(), ["tt3"]);
        assert_eq!(changes.gone_listed, ["movie|tt2"]);
        assert!(changes.added_watched.is_empty());
        assert_eq!(changes.gone_watched, ["series|tt9|tt9:1:2"]);
    }

    #[test]
    fn a_part_that_could_not_be_read_changes_nothing() {
        let before = Seen { watchlist: ["movie|tt1".to_owned()].into(), watched: ["movie|tt5|tt5".to_owned()].into() };
        let changes = changes_since(Some(&before), &Pulled { service: Service::Simkl, watchlist: None, watched: None });
        assert!(changes.gone_listed.is_empty() && changes.gone_watched.is_empty());
        assert_eq!(changes.seen, before);
    }

    #[test]
    fn a_change_still_waiting_here_wins() {
        let change = |what: &str, video: &str| Change {
            id: 1, what: what.into(), kind: "series".into(), title_id: "tt9".into(), video_id: video.into(), created_at: 0,
        };
        assert!(waiting(&[change("watched", "tt9:1:2")], "series|tt9|tt9:1:2"));
        assert!(!waiting(&[change("watched", "tt9:1:3")], "series|tt9|tt9:1:2"));
        assert!(waiting(&[change("watchlist_add", "")], "series|tt9"));
        assert_eq!(key_parts("series|tt9|tt9:1:2"), ("series", "tt9", "tt9:1:2"));
    }

    #[test]
    fn what_accounts_hold_is_added_once() {
        let store = Store::in_memory().unwrap();
        let listed = vec![Listed { imdb: "tt1".into(), series: true, title: "A Show".into(), year: "2020".into() }];
        assert_eq!(add_listed(&store, &listed, 100), 1);
        assert_eq!(add_listed(&store, &listed, 100), 0);
        let watched = vec![Watched { imdb: "tt1".into(), title: "A Show".into(), episode: Some((1, 2)), at: 5 }];
        assert_eq!(add_watched(&store, &watched), 1);
        assert_eq!(add_watched(&store, &watched), 0);
        assert!(store.progress("series", "tt1", "tt1:1:2").unwrap().unwrap().finished);
    }
}
