//! Sync: Haven keeps everything on this device first (what was watched,
//! the Watchlist) and notes each change in a queue in its database. The
//! queue is sent to the linked accounts (Trakt, Simkl) in one go: a
//! little while after a change (so several changes travel together),
//! when Haven starts, and on "Sync now". A change that cannot be sent
//! stays queued for the next time. What the accounts hold (their
//! Watchlist and, for Trakt, what was watched) is brought in at start
//! at most every few hours, and on "Sync now".
//!
//! On a phone or tablet nothing runs while Haven is closed and there is
//! no timer ticking in the background: the network is only used when
//! there is something to send or it is time to bring things in, which
//! keeps the battery use to a few short requests. A computer has no
//! battery to save (user, 2026-11-19): it sends a change at once and
//! brings things in every quarter of an hour while Haven is open.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use haven_core::storage::{Change, Progress, Saved};
use haven_tracking::{Account, Listed, Watched, subject};
use serde::{Deserialize, Serialize};

use super::{Library, on_ui};

const OPTIONS: &str = "sync_options";
/// A phone or tablet, where sync is paced to save battery.
const PHONE: bool = cfg!(any(target_os = "android", target_os = "ios"));
/// How long after a change the queue is sent (on a phone, so changes
/// travel together).
const AFTER_A_CHANGE: Duration = Duration::from_secs(if PHONE { 20 } else { 2 });
/// How long after start the first sync runs (Home loads first).
const AFTER_START: Duration = Duration::from_secs(if PHONE { 8 } else { 3 });
/// On a phone, bring things in from the accounts at start at most this often.
const PULL_EVERY_SECONDS: i64 = 6 * 60 * 60;
/// On a computer, bring things in this often while Haven is open.
const COMPUTER_PULL: Duration = Duration::from_secs(15 * 60);

fn yes() -> bool {
    true
}

#[derive(Clone, Serialize, Deserialize)]
struct Options {
    #[serde(default = "yes")]
    watched: bool,
    #[serde(default = "yes")]
    watchlist: bool,
    /// Also bring in what the accounts hold.
    #[serde(default = "yes")]
    pull: bool,
    /// When the queue was last sent in full, in seconds since 1970.
    #[serde(default)]
    last_sync: i64,
    #[serde(default)]
    last_pull: i64,
}

impl Default for Options {
    fn default() -> Self {
        Self { watched: true, watchlist: true, pull: true, last_sync: 0, last_pull: 0 }
    }
}

#[derive(Default)]
pub(super) struct SyncState {
    timer: slint::Timer,
    /// A computer only: brings things in every quarter of an hour.
    pull_timer: slint::Timer,
    running: Cell<bool>,
    /// Asked to run again (or to bring things in) while it was running.
    again: Cell<Option<bool>>,
    last_problem: RefCell<String>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// What one run brought back, for the UI thread.
struct Outcome {
    sent: Vec<i64>,
    problems: Vec<String>,
    renewed: Vec<Account>,
    watchlists: Vec<Listed>,
    watched: Vec<Watched>,
    pulled: bool,
}

impl Library {
    fn sync_options(&self) -> Options {
        self.store.get::<Options>(OPTIONS).ok().flatten().unwrap_or_default()
    }

    fn save_sync_options(&self, options: &Options) {
        if let Err(err) = self.store.put(OPTIONS, options) {
            eprintln!("haven: could not save the sync options: {err}");
        }
    }

    pub(super) fn start_sync(self: &Rc<Self>, ui: &crate::AppWindow) {
        let options = self.sync_options();
        ui.set_sync_watched(options.watched);
        ui.set_sync_watchlist(options.watchlist);
        ui.set_sync_pull(options.pull);
        ui.on_sync_now({
            let library = self.clone();
            move || library.run_sync(true)
        });
        ui.on_sync_option({
            let library = self.clone();
            move |which, on| {
                let mut options = library.sync_options();
                match which {
                    0 => options.watched = on,
                    1 => options.watchlist = on,
                    _ => options.pull = on,
                }
                library.save_sync_options(&options);
                library.show_sync();
            }
        });
        self.show_sync();
        // The first run waits a little so Home loads first.
        let pull = options.pull && (!PHONE || now() - options.last_pull > PULL_EVERY_SECONDS);
        if !PHONE {
            let library = Rc::downgrade(self);
            self.sync.pull_timer.start(slint::TimerMode::Repeated, COMPUTER_PULL, move || {
                if let Some(library) = library.upgrade() {
                    library.run_sync(true);
                }
            });
        }
        let library = Rc::downgrade(self);
        self.sync.timer.start(slint::TimerMode::SingleShot, AFTER_START, move || {
            if let Some(library) = library.upgrade() {
                library.run_sync(pull);
            }
        });
    }

    /// Notes a change to send to the linked accounts, and sends the
    /// queue a little later.
    pub(super) fn queue_sync(&self, what: &str, kind: &str, title_id: &str, video_id: &str) {
        let options = self.sync_options();
        let wanted = if what == "watched" { options.watched } else { options.watchlist };
        // Nothing linked, switched off, or a title the services cannot
        // know (they go by IMDb id): nothing to send.
        if !wanted || self.accounts.borrow().is_empty() || subject(kind, title_id, None).is_none() {
            return;
        }
        let change = Change {
            id: 0,
            what: what.to_owned(),
            kind: kind.to_owned(),
            title_id: title_id.to_owned(),
            video_id: video_id.to_owned(),
            created_at: now(),
        };
        if let Err(err) = self.store.queue_change(&change) {
            return eprintln!("haven: could not note a change to sync: {err}");
        }
        self.show_sync();
        let library = self.ui.clone();
        self.sync.timer.start(slint::TimerMode::SingleShot, AFTER_A_CHANGE, move || {
            if library.upgrade().is_some() {
                on_ui(|library| library.run_sync(false));
            }
        });
    }

    /// Sends the queue, and with `pull` brings in what the accounts hold.
    pub(super) fn run_sync(&self, pull: bool) {
        if self.sync.running.get() {
            let earlier = self.sync.again.get().unwrap_or(false);
            self.sync.again.set(Some(earlier || pull));
            return;
        }
        let accounts = self.accounts.borrow().clone();
        let queued = self.store.queued_changes().unwrap_or_default();
        if accounts.is_empty() || (queued.is_empty() && !pull) {
            return self.show_sync();
        }
        let options = self.sync_options();
        self.sync.running.set(true);
        self.show_sync();
        let tracker = self.tracker();
        let pull = pull && options.pull;
        self.tasks.spawn(async move {
            let mut outcome = Outcome {
                sent: Vec::new(),
                problems: Vec::new(),
                renewed: Vec::new(),
                watchlists: Vec::new(),
                watched: Vec::new(),
                pulled: pull,
            };
            // A token about to run out is renewed first.
            let mut ready = Vec::new();
            for account in accounts {
                match tracker.renewed(&account, now()).await {
                    Ok(Some(renewed)) => {
                        outcome.renewed.push(renewed.clone());
                        ready.push(renewed);
                    }
                    Ok(None) => ready.push(account),
                    Err(err) => outcome.problems.push(format!("{}: {err}", account.service.label())),
                }
            }
            // Each change goes to every account; it leaves the queue once
            // all of them have it (sending the same again does no harm).
            for change in queued {
                let video = (change.what == "watched").then_some(change.video_id.as_str());
                let Some(subject) = subject(&change.kind, &change.title_id, video) else {
                    outcome.sent.push(change.id);
                    continue;
                };
                let mut all = true;
                for account in &ready {
                    let sent = match change.what.as_str() {
                        "watched" if subject.series && subject.episode.is_none() => Ok(()),
                        "watched" => tracker.mark_watched(account, &subject).await,
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
            on_ui(move |library| library.sync_finished(outcome));
        });
    }

    fn sync_finished(&self, outcome: Outcome) {
        for renewed in outcome.renewed {
            let mut accounts = self.accounts.borrow_mut();
            accounts.retain(|known| known.service != renewed.service);
            accounts.push(renewed);
            drop(accounts);
            self.save_accounts();
        }
        for id in &outcome.sent {
            let _ = self.store.remove_change(*id);
        }
        let mut options = self.sync_options();
        if outcome.problems.is_empty() {
            options.last_sync = now();
        }
        if outcome.pulled {
            let options_now = &options;
            let watchlist_changed = options_now.watchlist && self.add_listed(&outcome.watchlists) > 0;
            let watched_changed = options_now.watched && self.add_watched(&outcome.watched) > 0;
            if watchlist_changed {
                self.reload_watchlist();
            }
            if watched_changed {
                self.reload_continue();
            }
            if outcome.problems.is_empty() {
                options.last_pull = now();
            }
        }
        self.save_sync_options(&options);
        *self.sync.last_problem.borrow_mut() = outcome.problems.first().cloned().unwrap_or_default();
        for problem in &outcome.problems {
            haven_core::diag::note(&format!("sync: {problem}"));
        }
        self.sync.running.set(false);
        self.show_sync();
        if let Some(pull) = self.sync.again.take() {
            self.run_sync(pull);
        }
    }

    /// Puts titles from an account's Watchlist on Haven's, if new there.
    pub(super) fn add_listed(&self, listed: &[Listed]) -> usize {
        let mut added = 0;
        // Oldest first, so the list keeps the service's order.
        for (place, title) in listed.iter().rev().enumerate() {
            let kind = if title.series { "series" } else { "movie" };
            if self.store.in_watchlist(kind, &title.imdb).unwrap_or(true) {
                continue;
            }
            let saved = self.store.add_to_watchlist(&Saved {
                kind: kind.to_owned(),
                title_id: title.imdb.clone(),
                title: title.title.clone(),
                year: title.year.clone(),
                poster: None,
                added_at: now() * 1000 + place as i64,
            });
            if saved.is_ok() {
                added += 1;
            }
        }
        added
    }

    /// Records what an account says was watched, where Haven has nothing yet.
    fn add_watched(&self, watched: &[Watched]) -> usize {
        watched
            .iter()
            .filter(|entry| {
                let (kind, video_id, detail) = match entry.episode {
                    Some((season, episode)) => (
                        "series",
                        format!("{}:{season}:{episode}", entry.imdb),
                        format!("S{season} E{episode}"),
                    ),
                    None => ("movie", entry.imdb.clone(), String::new()),
                };
                self.store
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

    /// Fills Settings > Sync options: which accounts, and how it stands.
    pub(super) fn show_sync(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let accounts: Vec<&str> = self.accounts.borrow().iter().map(|a| a.service.label()).collect();
        let waiting = self.store.queued_changes().map(|q| q.len()).unwrap_or(0);
        let options = self.sync_options();
        let linked = if accounts.is_empty() {
            "No account is linked yet. Link Trakt or Simkl under Accounts and Haven keeps them up to date.".to_owned()
        } else {
            format!("Sending to {}.", accounts.join(" and "))
        };
        let when = if options.last_sync > 0 {
            chrono::DateTime::from_timestamp(options.last_sync, 0)
                .map(|at| at.with_timezone(&chrono::Local))
                .map(|at| {
                    if at.date_naive() == chrono::Local::now().date_naive() {
                        format!("Last synced today at {}", at.format("%H:%M"))
                    } else {
                        format!("Last synced {}", at.format("%-d %b at %H:%M"))
                    }
                })
                .unwrap_or_default()
        } else {
            "Not synced yet".to_owned()
        };
        let queue = match waiting {
            0 => "nothing waiting".to_owned(),
            1 => "1 change waiting".to_owned(),
            n => format!("{n} changes waiting"),
        };
        let problem = self.sync.last_problem.borrow().clone();
        let status = if self.sync.running.get() {
            "Syncing…".to_owned()
        } else if !problem.is_empty() {
            format!("{when} · {queue}. Last try: {problem}. Haven tries again later.")
        } else {
            format!("{when} · {queue}.")
        };
        ui.set_sync_accounts(linked.into());
        ui.set_sync_status(status.into());
        ui.set_sync_busy(self.sync.running.get());
        ui.set_sync_linked(!accounts.is_empty());
        ui.set_sync_note(
            if PHONE {
                "Haven keeps everything on this device first and sends your changes in one go, shortly after they happen and when Haven opens. Nothing runs while Haven is closed, so it barely touches the battery."
            } else {
                "Haven keeps everything on this computer first and sends each change straight away. While Haven is open it also brings in your accounts' Watchlist and what you watched every 15 minutes."
            }
            .into(),
        );
    }
}
