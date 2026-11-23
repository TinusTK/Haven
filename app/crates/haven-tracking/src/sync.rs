//! One round of keeping Haven and the linked accounts in step:
//!
//! 1. [`exchange`] (off the database, may take a while): renews tokens
//!    about to run out, sends the queued changes, and with `pull` brings
//!    in each account's Watchlist and what it says was watched.
//! 2. [`add_listed`] and [`add_watched`] put what came back into a
//!    [`Store`], where it is new there.

use haven_core::storage::{Change, Progress, Saved, Store};

use crate::{Account, Client, Listed, Watched, subject};

/// The key the linked accounts are kept under in a store.
pub const ACCOUNTS: &str = "tracking_accounts";

/// What one round brought back.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Queued changes every account now has (to take off the queue).
    pub sent: Vec<i64>,
    pub problems: Vec<String>,
    /// Accounts whose tokens were renewed (to keep in place of the old).
    pub renewed: Vec<Account>,
    pub watchlists: Vec<Listed>,
    pub watched: Vec<Watched>,
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
            match tracker.watchlist(account).await {
                Ok(listed) => outcome.watchlists.extend(listed),
                Err(err) => outcome.problems.push(format!("{}: {err}", account.service.label())),
            }
            match tracker.watched(account).await {
                Ok(watched) => outcome.watched.extend(watched),
                Err(err) => outcome.problems.push(format!("{}: {err}", account.service.label())),
            }
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
