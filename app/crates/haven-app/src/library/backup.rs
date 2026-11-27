//! Backups (Settings > Sync options): the whole setup and the settings in
//! one file, kept up to date while Haven runs, and put back with Restore.
//!
//! Where the file goes:
//! - **App Store build on iPhone and iPad:** iCloud Drive (Haven's own
//!   folder there), so a new or reset device gets it back. Falls back to
//!   the Files app when iCloud is off.
//! - **Other iPhone builds:** the Files app, On My iPhone › Haven (which
//!   the user can move to iCloud Drive themselves).
//! - **Android:** Documents/Haven on the phone. Android's own backup to
//!   the user's Google account also keeps Haven's data, which a reinstall
//!   puts back by itself.
//! - **Computers:** Documents/Haven.

use std::{
    cell::{Cell, RefCell},
    path::PathBuf,
    rc::Rc,
    time::Duration,
};

use haven_core::{
    backup::{Backup, FILE_NAME},
    setup::Include,
};

use super::{Library, NO_ADDONS, on_ui};
use crate::AppWindow;

/// What a backup holds (add-ons only in builds that have them).
const INCLUDE: Include = Include { sources: !NO_ADDONS };
/// How often the backup is brought up to date while Haven is open.
const EVERY: Duration = Duration::from_secs(10 * 60);
/// The first backup waits for start-up to settle.
const AFTER_START: Duration = Duration::from_secs(30);
/// When the last backup was made, in seconds since 1970.
const LAST: &str = "backup_last";

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Default)]
pub(super) struct BackupState {
    timer: slint::Timer,
    /// The backup's contents last written (without its time), so an
    /// unchanged setup is not written again.
    last_written: RefCell<String>,
    busy: Cell<bool>,
    /// Where it goes, in words, once known.
    place: RefCell<String>,
}

/// The folder and how to say where it is. Looking up iCloud can take a
/// moment, so this runs off the UI thread.
fn backup_folder() -> (PathBuf, String) {
    // Checks of Haven itself keep their backups beside their own data.
    if cfg!(debug_assertions) {
        if let Some(folder) = std::env::var_os("HAVEN_DATA_DIR") {
            return (PathBuf::from(folder).join("backup"), "the test data folder".to_owned());
        }
    }
    let documents = directories::UserDirs::new()
        .and_then(|dirs| dirs.document_dir().map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let folder = documents.join("Haven");
    let words = folder.display().to_string();
    (folder, words)
}

fn when(at: i64) -> String {
    chrono::DateTime::from_timestamp(at, 0)
        .map(|at| at.with_timezone(&chrono::Local))
        .map(|at| {
            if at.date_naive() == chrono::Local::now().date_naive() {
                format!("today at {}", at.format("%H:%M"))
            } else {
                at.format("%-d %b at %H:%M").to_string()
            }
        })
        .unwrap_or_default()
}

impl Library {
    pub(super) fn start_backup(self: &Rc<Self>, ui: &AppWindow) {
        ui.on_backup_now({
            let library = self.clone();
            move || library.back_up(false)
        });
        ui.on_backup_restore({
            let library = self.clone();
            move || library.restore_backup()
        });
        self.show_backup("");
        let library = Rc::downgrade(self);
        self.backup.timer.start(slint::TimerMode::SingleShot, AFTER_START, move || {
            let Some(library) = library.upgrade() else { return };
            library.back_up(true);
            let later = Rc::downgrade(&library);
            library.backup.timer.start(slint::TimerMode::Repeated, EVERY, move || {
                if let Some(library) = later.upgrade() {
                    library.back_up(true);
                }
            });
        });
    }

    fn show_backup(&self, note: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let last: i64 = self.store.get(LAST).ok().flatten().unwrap_or(0);
        let place = self.backup.place.borrow().clone();
        let status = if !note.is_empty() {
            note.to_owned()
        } else if last == 0 {
            "Haven backs up your media servers, accounts, Trakt and Simkl, keys, Watchlist, what you watched and your settings while it is open.".to_owned()
        } else if place.is_empty() {
            format!("Last backed up {}.", when(last))
        } else {
            format!("Last backed up {} to {place}.", when(last))
        };
        ui.set_backup_status(status.into());
        ui.set_backup_where("Documents folder".into());
        ui.set_backup_busy(self.backup.busy.get());
    }

    /// Writes the backup if anything changed since the last one (or, not
    /// `quiet`, always).
    pub(super) fn back_up(&self, quiet: bool) {
        if self.backup.busy.get() {
            return;
        }
        let backup = match Backup::of(&self.store, INCLUDE, now()) {
            Ok(backup) => backup,
            Err(err) => return self.show_backup(&format!("Could not read what to back up: {err}")),
        };
        let contents = serde_json::to_string(&(&backup.setup, &backup.settings)).unwrap_or_default();
        if quiet && *self.backup.last_written.borrow() == contents {
            return;
        }
        self.backup.busy.set(true);
        if !quiet {
            self.show_backup("Backing up…");
        }
        self.tasks.spawn(async move {
            let written = tokio::task::spawn_blocking(move || {
                let (folder, place) = backup_folder();
                backup.write(&folder.join(FILE_NAME)).map(|()| place).map_err(|err| err.to_string())
            })
            .await
            .unwrap_or_else(|err| Err(err.to_string()));
            on_ui(move |library| {
                library.backup.busy.set(false);
                match written {
                    Ok(place) => {
                        *library.backup.last_written.borrow_mut() = contents;
                        *library.backup.place.borrow_mut() = place;
                        let _ = library.store.put(LAST, &now());
                        library.show_backup("");
                    }
                    Err(err) => library.show_backup(&format!("Could not back up: {err}.")),
                }
            });
        });
    }

    /// Reads the backup and adds what is missing here.
    fn restore_backup(&self) {
        if self.backup.busy.get() {
            return;
        }
        self.backup.busy.set(true);
        self.show_backup("Looking for the backup…");
        self.tasks.spawn(async move {
            let read = tokio::task::spawn_blocking(|| {
                let (folder, place) = backup_folder();
                let path = folder.join(FILE_NAME);
                let text = std::fs::read_to_string(&path)
                    .map_err(|_| format!("There is no backup in {place} yet"))?;
                Backup::parse(&text).ok_or_else(|| "That file is not a Haven backup".to_owned())
            })
            .await
            .unwrap_or_else(|err| Err(err.to_string()));
            on_ui(move |library| {
                library.backup.busy.set(false);
                let backup = match read {
                    Ok(backup) => backup,
                    Err(err) => return library.show_backup(&format!("{err}.")),
                };
                match backup.restore_into(&library.store, INCLUDE) {
                    Ok(merged) => {
                        if merged.tracking {
                            *library.accounts.borrow_mut() =
                                library.store.get(haven_tracking::sync::ACCOUNTS).ok().flatten().unwrap_or_default();
                            library.show_tracking();
                            library.show_sync();
                        }
                        library.load_servers();
                        library.reload_watchlist();
                        library.reload_continue();
                        library.refresh_ticks();
                        #[cfg(feature = "addons")]
                        {
                            library.load_installed();
                            library.reload_home();
                            library.rebuild_explore();
                        }
                        library.show_backup(&format!(
                            "Restored the backup from {}: {} new {}. New TV sources and settings show the next time Haven starts.",
                            when(backup.made_at),
                            merged.added,
                            if merged.added == 1 { "thing" } else { "things" }
                        ));
                    }
                    Err(err) => library.show_backup(&format!("Could not restore the backup: {err}")),
                }
            });
        });
    }
}
