//! What the Hub does while no one is looking:
//!
//! - **Trakt and Simkl:** every quarter of an hour (and straight after a
//!   device hands over something new) it sends what was watched and the
//!   Watchlist changes, renews sign-ins, and brings in what the accounts
//!   hold. Devices then take it from the Hub.
//! - **New episodes:** every few hours it fetches the details of the
//!   series on the Watchlist or being watched, so every device's calendar
//!   has fresh dates the moment it opens.
//! - **Add-ons:** their descriptions are fetched again now and then, so
//!   new catalogues show up.

use std::{sync::Mutex, time::Duration};

use haven_core::{
    hub::Merged,
    secrets::Secrets,
    settings::Keys,
    storage::Change,
};
use haven_titles::MetaDetail;
use haven_tracking::{Account, sync};
use tokio::sync::Notify;

use crate::hub::Hub;

const SYNC_EVERY: Duration = Duration::from_secs(15 * 60);
const EPISODES_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const ADDONS_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
/// The most series whose dates are kept.
const MOST_SERIES: usize = 300;
/// Where the fetched details are kept between runs.
const CALENDAR: &str = "hub_calendar";

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// How the background work stands, for the status page.
#[derive(Debug, Clone, Default)]
pub struct Report {
    /// Linked services ("Trakt", "Simkl").
    pub linked: Vec<String>,
    /// When the last round with them ended, in seconds since 1970.
    pub last_sync: i64,
    pub sync_problem: String,
    pub last_episodes: i64,
    pub series_checked: usize,
    /// Episodes airing from yesterday to a week ahead: date, series, episode.
    pub upcoming: Vec<(String, String, String)>,
}

#[derive(Default)]
pub struct Background {
    sync_now: Notify,
    episodes_now: Notify,
    pub report: Mutex<Report>,
}

/// A day number (days since 1970-01-01) as `YYYY-MM-DD`.
fn date(days: i64) -> String {
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// The episodes of `series` airing between `from` and `to` (dates as text).
fn airing(series: &[MetaDetail], from: &str, to: &str) -> Vec<(String, String, String)> {
    let mut found: Vec<(String, String, String)> = series
        .iter()
        .flat_map(|meta| {
            meta.videos.iter().filter_map(move |video| {
                let day = video.released.as_deref()?.get(..10)?;
                (day >= from && day <= to).then(|| {
                    let which = match (video.season, video.episode) {
                        (Some(s), Some(e)) => format!("S{s} E{e}"),
                        _ => String::new(),
                    };
                    let which = match &video.title {
                        Some(title) if !which.is_empty() => format!("{which} · {title}"),
                        Some(title) => title.clone(),
                        None => which,
                    };
                    (day.to_owned(), meta.name.clone(), which)
                })
            })
        })
        .collect();
    found.sort();
    found
}

impl Hub {
    fn tracker(&self) -> haven_tracking::Client {
        let keys = Keys::load(&self.store.lock().unwrap()).unwrap_or_default();
        haven_tracking::Client::new(self.http.clone(), keys.resolved(&Secrets::default()))
    }

    fn accounts(&self) -> Vec<Account> {
        self.store.lock().unwrap().get(sync::ACCOUNTS).ok().flatten().unwrap_or_default()
    }

    /// What a device handed over: anything newly watched or put on the
    /// Watchlist goes to the linked accounts soon.
    pub fn after_merge(&self, merged: Merged) {
        let linked = !self.accounts().is_empty();
        if linked {
            let store = self.store.lock().unwrap();
            let stamp = now();
            for (kind, title_id, video_id) in &merged.watched {
                let _ = store.queue_change(&Change {
                    id: 0,
                    what: "watched".into(),
                    kind: kind.clone(),
                    title_id: title_id.clone(),
                    video_id: video_id.clone(),
                    created_at: stamp,
                });
            }
            for (kind, title_id) in &merged.watchlist {
                let _ = store.queue_change(&Change {
                    id: 0,
                    what: "watchlist_add".into(),
                    kind: kind.clone(),
                    title_id: title_id.clone(),
                    video_id: String::new(),
                    created_at: stamp,
                });
            }
        }
        if linked && (!merged.watched.is_empty() || !merged.watchlist.is_empty() || merged.tracking) {
            self.background.sync_now.notify_one();
        }
        if !merged.watchlist.is_empty() || !merged.watched.is_empty() {
            self.background.episodes_now.notify_one();
        }
    }

    /// One round with Trakt and Simkl.
    pub async fn sync_round(&self) {
        let accounts = self.accounts();
        self.background.report.lock().unwrap().linked =
            accounts.iter().map(|a| a.service.label().to_owned()).collect();
        if accounts.is_empty() {
            return;
        }
        let queued = self.store.lock().unwrap().queued_changes().unwrap_or_default();
        let outcome = sync::exchange(&self.tracker(), accounts, queued, true, now()).await;
        let store = self.store.lock().unwrap();
        if !outcome.renewed.is_empty() {
            let mut accounts: Vec<Account> = store.get(sync::ACCOUNTS).ok().flatten().unwrap_or_default();
            sync::keep_renewed(&mut accounts, &outcome.renewed);
            let _ = store.put(sync::ACCOUNTS, &accounts);
        }
        for id in &outcome.sent {
            let _ = store.remove_change(*id);
        }
        let listed = sync::add_listed(&store, &outcome.watchlists, now());
        let watched = sync::add_watched(&store, &outcome.watched);
        drop(store);
        if listed > 0 {
            self.background.episodes_now.notify_one();
        }
        for problem in &outcome.problems {
            eprintln!("haven-hub: sync: {problem}");
        }
        if listed + watched > 0 {
            eprintln!("haven-hub: sync brought in {listed} Watchlist titles and {watched} watched");
        }
        let mut report = self.background.report.lock().unwrap();
        report.last_sync = now();
        report.sync_problem = outcome.problems.first().cloned().unwrap_or_default();
    }

    /// The series whose dates matter: on the Watchlist, or being watched.
    fn followed_series(&self) -> Vec<String> {
        let store = self.store.lock().unwrap();
        let mut ids: Vec<String> = store
            .watchlist()
            .unwrap_or_default()
            .into_iter()
            .filter(|saved| saved.kind == "series")
            .map(|saved| saved.title_id)
            .collect();
        for progress in store.history(MOST_SERIES as u32).unwrap_or_default() {
            if progress.kind == "series" && !ids.contains(&progress.title_id) {
                ids.push(progress.title_id);
            }
        }
        ids.truncate(MOST_SERIES);
        ids
    }

    /// Fetches the followed series' details and keeps them.
    pub async fn episodes_round(&self) {
        let mut details = Vec::new();
        for id in self.followed_series() {
            if let Ok(meta) = self.meta("series", &id).await {
                details.push(meta);
            }
        }
        if let Err(err) = self.store.lock().unwrap().put(CALENDAR, &details) {
            eprintln!("haven-hub: could not keep the calendar: {err}");
        }
        let today = now().div_euclid(86_400);
        let upcoming = airing(&details, &date(today - 1), &date(today + 7));
        let mut report = self.background.report.lock().unwrap();
        report.last_episodes = now();
        report.series_checked = details.len();
        report.upcoming = upcoming;
    }

    /// The followed series' details as last fetched (for apps' calendars).
    pub fn calendar(&self) -> Vec<MetaDetail> {
        self.store.lock().unwrap().get(CALENDAR).ok().flatten().unwrap_or_default()
    }
}

/// Starts the background work; it runs for as long as the Hub does.
pub fn start(hub: std::sync::Arc<Hub>) {
    let worker = hub.clone();
    tokio::spawn(async move {
        loop {
            worker.sync_round().await;
            let _ = tokio::time::timeout(SYNC_EVERY, worker.background.sync_now.notified()).await;
            // Several changes arriving together go in one round.
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    });
    let worker = hub.clone();
    tokio::spawn(async move {
        // Sync gets the first minute to bring in the Watchlist.
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            worker.episodes_round().await;
            let _ = tokio::time::timeout(EPISODES_EVERY, worker.background.episodes_now.notified()).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
    tokio::spawn(async move {
        loop {
            hub.refresh_addons().await;
            tokio::time::sleep(ADDONS_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_become_dates() {
        assert_eq!(date(0), "1970-01-01");
        assert_eq!(date(20_454), "2026-01-01");
        assert_eq!(date(20_513), "2026-03-01");
    }

    #[test]
    fn episodes_in_the_window_are_listed_in_date_order() {
        let meta: MetaDetail = serde_json::from_value(serde_json::json!({
            "id": "tt1", "type": "series", "name": "A Show",
            "videos": [
                {"id": "tt1:1:1", "title": "Pilot", "season": 1, "episode": 1, "released": "2026-11-20T00:00:00.000Z"},
                {"id": "tt1:1:2", "title": "Two", "season": 1, "episode": 2, "released": "2026-11-27T00:00:00.000Z"},
                {"id": "tt1:1:3", "season": 1, "episode": 3, "released": "2026-12-30T00:00:00.000Z"}
            ]
        }))
        .unwrap();
        let found = airing(&[meta], "2026-11-21", "2026-11-29");
        assert_eq!(found, [("2026-11-27".to_owned(), "A Show".to_owned(), "S1 E2 · Two".to_owned())]);
    }
}
