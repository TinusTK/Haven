//! Sync: Haven keeps everything on this device first (what was watched,
//! the Watchlist) and notes each change in a queue in its database. The
//! queue is sent to the linked accounts (Trakt, Simkl) in one go: a
//! little while after a change (so several changes travel together),
//! when Haven starts, and on "Sync now". A change that cannot be sent
//! stays queued for the next time.
//!
//! What the accounts hold (their Watchlist and what was watched) is
//! brought in when Haven starts, a little after something was watched,
//! on "Sync now", and while Haven is open every quarter of an hour on a
//! computer, every half hour on a phone. Both ways: what was added on an
//! account comes in, and what was taken off there (off the Watchlist, or
//! marked unwatched) is taken off here too (`haven_tracking::sync::Seen`),
//! so every device ends up the same.
//!
//! On a phone nothing runs while Haven is closed: the network is only
//! used when there is something to send or it is time to bring things in,
//! which keeps the battery use to a few short requests.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

use haven_core::storage::Change;
use haven_tracking::sync::{Pulled, SEEN, SeenByAccount, changes_since, key_parts, listed_key, waiting, watched_key};
use haven_tracking::{Account, Listed, Watched, subject};
use serde::{Deserialize, Serialize};

use super::{Library, on_ui};

const OPTIONS: &str = "sync_options";
/// How long after a change the queue is sent.
const AFTER_A_CHANGE: Duration = Duration::from_secs(2);
/// How long after start the first sync runs (Home loads first).
const AFTER_START: Duration = Duration::from_secs(3);
/// Bring things in this often while Haven is open: a quarter of an hour.
const PULL_WHILE_OPEN: Duration = Duration::from_secs(15 * 60);
/// How long after a video closes what the accounts hold is brought in
/// (the watched mark goes out first, with it).
const AFTER_WATCHING: Duration = Duration::from_secs(5);

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
    /// Linked accounts whose sync is switched off (by service name).
    /// Everything else syncs: watched, progress and the Watchlist, both
    /// ways (user, 2026-11-22: one switch per account, not three).
    #[serde(default)]
    off: Vec<String>,
}

impl Default for Options {
    fn default() -> Self {
        Self { watched: true, watchlist: true, pull: true, last_sync: 0, last_pull: 0, off: Vec::new() }
    }
}

#[derive(Default)]
pub(super) struct SyncState {
    timer: slint::Timer,
    /// The next run of `timer` also brings things in.
    pull_next: Cell<bool>,
    /// Brings things in while Haven is open.
    pull_timer: slint::Timer,
    running: Cell<bool>,
    /// Asked to run again (or to bring things in) while it was running.
    again: Cell<Option<bool>>,
    last_problem: RefCell<String>,
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

use haven_tracking::sync::Outcome;

impl Library {
    fn sync_options(&self) -> Options {
        let mut options = self.store.get::<Options>(OPTIONS).ok().flatten().unwrap_or_default();
        // The three old switches are gone: an account syncs everything or nothing.
        options.watched = true;
        options.watchlist = true;
        options.pull = true;
        options
    }

    /// The linked accounts whose sync is on.
    fn syncing_accounts(&self) -> Vec<Account> {
        let off = self.sync_options().off;
        self.accounts.borrow().iter().filter(|a| !off.iter().any(|o| o == a.service.label())).cloned().collect()
    }

    /// Switches one account's sync on or off; linking one turns it on.
    pub(super) fn set_account_sync(&self, service: haven_tracking::Service, on: bool) {
        let mut options = self.sync_options();
        options.off.retain(|name| name != service.label());
        if !on {
            options.off.push(service.label().to_owned());
        }
        self.save_sync_options(&options);
        if on {
            self.run_sync(true);
        }
        self.show_sync();
    }

    fn save_sync_options(&self, options: &Options) {
        if let Err(err) = self.store.put(OPTIONS, options) {
            eprintln!("haven: could not save the sync options: {err}");
        }
    }

    pub(super) fn start_sync(self: &Rc<Self>, ui: &crate::AppWindow) {
        let options = self.sync_options();
        ui.on_sync_account({
            let library = self.clone();
            move |service, on| {
                let service = if service == 0 { haven_tracking::Service::Trakt } else { haven_tracking::Service::Simkl };
                library.set_account_sync(service, on);
            }
        });
        ui.on_sync_now({
            let library = self.clone();
            move || library.run_sync(true)
        });
        self.show_sync();
        // The first run waits a little so Home loads first.
        let pull = options.pull;
        let library = Rc::downgrade(self);
        self.sync.pull_timer.start(slint::TimerMode::Repeated, PULL_WHILE_OPEN, move || {
            if let Some(library) = library.upgrade() {
                library.run_sync(true);
            }
        });
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
        let wanted = if what == "watched" || what == "unwatched" { options.watched } else { options.watchlist };
        if !wanted || self.syncing_accounts().is_empty() {
            return;
        }
        // The services go by IMDb id: a title known by another (a media
        // server's) is sent under its IMDb id; one with none is not sent.
        let Some((title_id, video_id)) = self.service_ids(kind, title_id, video_id) else { return };
        if subject(kind, &title_id, None).is_none() {
            return;
        }
        let change = Change {
            id: 0,
            what: what.to_owned(),
            kind: kind.to_owned(),
            title_id,
            video_id,
            created_at: now(),
        };
        if let Err(err) = self.store.queue_change(&change) {
            return eprintln!("haven: could not note a change to sync: {err}");
        }
        self.show_sync();
        self.sync_soon(AFTER_A_CHANGE, false);
    }

    /// Runs the sync after `wait`; with `pull` it also brings in what the
    /// accounts hold (kept if a change asks for a run meanwhile).
    pub(super) fn sync_soon(&self, wait: Duration, pull: bool) {
        self.sync.pull_next.set(self.sync.pull_next.get() || pull);
        let library = self.ui.clone();
        self.sync.timer.start(slint::TimerMode::SingleShot, wait, move || {
            if library.upgrade().is_some() {
                on_ui(|library| library.run_sync(library.sync.pull_next.take()));
            }
        });
    }

    /// After a video: the watched mark goes out and what the accounts hold
    /// comes in, so another device's changes show here too.
    pub(super) fn sync_after_watching(&self) {
        if !self.syncing_accounts().is_empty() {
            self.sync_soon(AFTER_WATCHING, true);
        }
    }

    /// Sends the queue, and with `pull` brings in what the accounts hold.
    pub(super) fn run_sync(&self, pull: bool) {
        if self.sync.running.get() {
            let earlier = self.sync.again.get().unwrap_or(false);
            self.sync.again.set(Some(earlier || pull));
            return;
        }
        let accounts = self.syncing_accounts();
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
            let outcome = haven_tracking::sync::exchange(&tracker, accounts, queued, pull, now()).await;
            on_ui(move |library| library.sync_finished(outcome));
        });
    }

    fn sync_finished(&self, outcome: Outcome) {
        if !outcome.renewed.is_empty() {
            haven_tracking::sync::keep_renewed(&mut self.accounts.borrow_mut(), &outcome.renewed);
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
            let (watchlist_changed, watched_changed) = self.take_in(&outcome.pulls);
            if watchlist_changed {
                self.reload_watchlist();
            }
            if watched_changed {
                self.reload_continue();
                self.refresh_ticks();
            }
            if outcome.problems.is_empty() {
                options.last_pull = now();
                if options.watched {
                    self.send_missing_linked();
                }
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

    /// Queues what was watched on titles known by a media server's id
    /// that the accounts do not have yet (after a full pull, everything
    /// they have is kept under the IMDb id). Catches up ticks made
    /// before such titles were sent, and ones made with no details kept.
    fn send_missing_linked(&self) {
        let rows = self.store.all_progress().unwrap_or_default();
        let queued = self.store.queued_changes().unwrap_or_default();
        let mut links = std::collections::HashMap::new();
        let mut missing = Vec::new();
        for row in rows.iter().filter(|row| row.finished && !haven_metadata::is_imdb_id(&row.title_id)) {
            let link = links
                .entry((row.kind.clone(), row.title_id.clone()))
                .or_insert_with(|| self.imdb_link(&row.kind, &row.title_id));
            let Some(link) = link else { continue };
            let video = if row.kind == "series" {
                match link.episodes.get(&row.video_id) {
                    Some(numbers) => super::links::imdb_episode(&link.imdb, *numbers),
                    None => continue,
                }
            } else {
                link.imdb.clone()
            };
            let known = self.store.progress(&row.kind, &link.imdb, &video).ok().flatten().is_some_and(|known| known.finished);
            let waiting = queued.iter().any(|change| change.title_id == link.imdb && change.video_id == video);
            if !known && !waiting {
                missing.push(row);
            }
        }
        for row in missing {
            self.queue_sync("watched", &row.kind, &row.title_id, &row.video_id);
        }
    }

    /// Brings in what changed on each account since its last pull: what
    /// was added there, and what was taken off there (off the Watchlist,
    /// or marked unwatched) goes here too, unless a change of this
    /// device's for it is still waiting to be sent. With more than one
    /// account, what changed on one is passed on to the others (not on an
    /// account's first pull, which would send its whole history). Says
    /// whether the Watchlist and the watched marks changed.
    fn take_in(&self, pulls: &[Pulled]) -> (bool, bool) {
        let mut seen: SeenByAccount = self.store.get(SEEN).ok().flatten().unwrap_or_default();
        let queued = self.store.queued_changes().unwrap_or_default();
        let others = self.syncing_accounts().len() > 1;
        let (mut watchlist_changed, mut watched_changed) = (false, false);
        let mut pass_on = Vec::new();
        for pulled in pulls {
            let label = pulled.service.label().to_owned();
            let first = !seen.contains_key(&label);
            let changes = changes_since(seen.get(&label), pulled);
            let before = pass_on.len();
            for title in &changes.added_listed {
                if self.add_listed(std::slice::from_ref(title)) > 0 {
                    watchlist_changed = true;
                    pass_on.push(("watchlist_add", listed_key(title)));
                }
            }
            for key in changes.gone_listed.iter().filter(|key| !waiting(&queued, key)) {
                let (kind, title, _) = key_parts(key);
                if self.store.in_watchlist(kind, title).unwrap_or(false) && self.store.remove_from_watchlist(kind, title).is_ok() {
                    watchlist_changed = true;
                    pass_on.push(("watchlist_remove", key.clone()));
                }
            }
            for entry in &changes.added_watched {
                if self.add_watched(std::slice::from_ref(entry)) > 0 {
                    watched_changed = true;
                    pass_on.push(("watched", watched_key(entry)));
                }
            }
            let gone: Vec<&String> = changes.gone_watched.iter().filter(|key| !waiting(&queued, key)).collect();
            let parts: Vec<(String, String, String)> = gone
                .iter()
                .map(|key| {
                    let (kind, title, video) = key_parts(key);
                    (kind.to_owned(), title.to_owned(), video.to_owned())
                })
                .collect();
            for (key, removed) in gone.iter().zip(self.forget_watched(&parts)) {
                if removed {
                    watched_changed = true;
                    pass_on.push(("unwatched", (*key).clone()));
                }
            }
            if first {
                pass_on.truncate(before);
            }
            seen.insert(label, changes.seen);
        }
        if let Err(err) = self.store.put(SEEN, &seen) {
            eprintln!("haven: could not keep what the accounts hold: {err}");
        }
        if others {
            for (what, key) in pass_on {
                let (kind, title, video) = key_parts(&key);
                self.queue_sync(what, kind, title, video);
            }
        }
        (watchlist_changed, watched_changed)
    }

    /// Puts titles from an account's Watchlist on Haven's, if new there.
    pub(super) fn add_listed(&self, listed: &[Listed]) -> usize {
        haven_tracking::sync::add_listed(&self.store, listed, now())
    }

    /// Records what an account says was watched, where Haven has nothing yet.
    fn add_watched(&self, watched: &[Watched]) -> usize {
        haven_tracking::sync::add_watched(&self.store, watched)
    }

    /// Fills Settings > Sync options: which accounts, and how it stands.
    pub(super) fn show_sync(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let options = self.sync_options();
        let syncing: Vec<&str> = self.syncing_accounts().iter().map(|a| a.service.label()).collect();
        let waiting = if syncing.is_empty() { 0 } else { self.store.queued_changes().map(|q| q.len()).unwrap_or(0) };
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
        let problem = self.sync.last_problem.borrow().clone();
        let status = if syncing.is_empty() {
            "Link Trakt or Simkl (or switch one on below) and Haven keeps it up to date from this device.".to_owned()
        } else if self.sync.running.get() {
            format!("Syncing with {}…", syncing.join(" and "))
        } else if !problem.is_empty() {
            format!("{when}. Last try: {problem}. Haven tries again later.")
        } else if waiting == 0 {
            format!("{when}. Everything is in step with {}.", syncing.join(" and "))
        } else {
            format!("{when}. {waiting} change{} still to send to {}.", if waiting == 1 { "" } else { "s" }, syncing.join(" and "))
        };
        let linked: Vec<haven_tracking::Service> = self.accounts.borrow().iter().map(|a| a.service).collect();
        let rows: Vec<crate::SyncAccount> = [haven_tracking::Service::Trakt, haven_tracking::Service::Simkl]
            .into_iter()
            .enumerate()
            .map(|(index, service)| {
                let is_linked = linked.contains(&service);
                let on = is_linked && !options.off.iter().any(|o| o == service.label());
                crate::SyncAccount {
                    service: index as i32,
                    name: service.label().into(),
                    linked: is_linked,
                    on,
                    detail: if !is_linked {
                        "Not linked.".into()
                    } else if on {
                        "Watched, progress and Watchlist, kept in step both ways.".into()
                    } else {
                        "Linked, but not syncing.".into()
                    },
                }
            })
            .collect();
        ui.set_sync_accounts(slint::ModelRc::new(slint::VecModel::from(rows)));
        ui.set_sync_status(status.into());
        ui.set_sync_waiting(waiting as i32);
        ui.set_sync_problem(!problem.is_empty() && !syncing.is_empty());
        ui.set_sync_busy(self.sync.running.get());
        ui.set_sync_linked(!syncing.is_empty());
        ui.set_sync_note(
            "Changes go out straight away. Haven brings in your accounts' Watchlist and what you watched when it starts, after you watch something, and every 15 minutes while it is open. What you take off or mark unwatched on one device goes on the others too."
                .into(),
        );
    }
}
