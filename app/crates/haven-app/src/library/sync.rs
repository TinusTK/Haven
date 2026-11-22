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

use haven_core::storage::Change;
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
        let wanted = if what == "watched" || what == "unwatched" { options.watched } else { options.watchlist };
        // Nothing linked, switched off, or a title the services cannot
        // know (they go by IMDb id): nothing to send.
        if !wanted || self.syncing_accounts().is_empty() || subject(kind, title_id, None).is_none() {
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
            let options_now = &options;
            let watchlist_changed = options_now.watchlist && self.add_listed(&outcome.watchlists) > 0;
            let watched_changed = options_now.watched && self.add_watched(&outcome.watched) > 0;
            if watchlist_changed {
                self.reload_watchlist();
            }
            if watched_changed {
                self.reload_continue();
                self.refresh_ticks();
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
            if PHONE {
                "Changes go out in one go shortly after they happen and when Haven opens. Nothing runs while Haven is closed, so it barely touches the battery."
            } else {
                "Changes go out straight away. While Haven is open it also brings in your accounts' Watchlist and what you watched every 15 minutes."
            }
            .into(),
        );
    }
}
