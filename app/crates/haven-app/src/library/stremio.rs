//! The Stremio account (Settings › Accounts): a person signs
//! in with their own Stremio account and Haven uses what that account
//! uses, asking those services itself as Stremio does. Haven follows the
//! account: each time it starts it reads the account's list again, so what
//! is installed always matches it (new ones in, removed ones out). On
//! Android nothing else installs anything, so the whole installed list is
//! the account's. On a computer people install add-ons by hand too: there
//! only those the account brought in (kv `stremio_followed`) follow it.
//!
//! The account (email and Stremio's key, never the password) is kept in
//! the store under `stremio_account`; it is not part of backups.

use std::rc::Rc;

use haven_addons::stremio::{self, Account, Listed, StremioError};

use super::{Library, on_ui};
use crate::AppWindow;

const KEY: &str = "stremio_account";
/// The add-on addresses the account brought in.
const FOLLOWED: &str = "stremio_followed";

/// Whether this build offers the Stremio sign-in: every build with add-ons
/// (user 2026-11-25).
pub(crate) const SHOWN: bool = true;

impl Library {
    pub(super) fn start_stremio(self: &Rc<Self>, ui: &AppWindow) {
        ui.set_stremio_shown(SHOWN);
        if !SHOWN {
            return;
        }
        ui.on_stremio_sign_in({
            let library = self.clone();
            move |email, password| library.stremio_sign_in(email.as_str(), password.as_str())
        });
        ui.on_stremio_sign_out({
            let library = self.clone();
            move || library.stremio_sign_out()
        });
        self.show_stremio("");
        self.stremio_refresh();
    }

    fn stremio_account(&self) -> Option<Account> {
        self.store.get(KEY).ok().flatten()
    }

    fn show_stremio(&self, status: &str) {
        let email = self.stremio_account().map(|account| account.email).unwrap_or_default();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_stremio_email(email.into());
            ui.set_stremio_status(status.into());
            ui.set_stremio_busy(false);
        }
    }

    fn stremio_busy(&self, status: &str) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_stremio_busy(true);
            ui.set_stremio_status(status.into());
        }
    }

    fn stremio_sign_in(&self, email: &str, password: &str) {
        if email.trim().is_empty() || password.is_empty() {
            return self.show_stremio("Type your Stremio email address and password.");
        }
        self.stremio_busy("Signing in to Stremio…");
        let http = self.http.clone();
        let (email, password) = (email.to_owned(), password.to_owned());
        self.tasks.spawn(async move {
            let signed = async {
                let account = stremio::login(&http, &email, &password).await?;
                let listed = stremio::listed(&http, &account).await?;
                Ok::<_, StremioError>((account, listed))
            }
            .await;
            on_ui(move |library| match signed {
                Ok((account, listed)) => {
                    if let Err(err) = library.store.put(KEY, &account) {
                        return library.show_stremio(&format!("Could not keep the sign-in: {err}."));
                    }
                    if let Some(ui) = library.ui.upgrade() {
                        ui.set_stremio_entered_password("".into());
                    }
                    library.follow_stremio(&listed);
                    library.show_stremio("");
                }
                Err(err) => library.show_stremio(&format!("Could not sign in: {err}.")),
            });
        });
    }

    fn stremio_sign_out(&self) {
        let Some(account) = self.stremio_account() else { return self.show_stremio("") };
        let _ = self.store.remove(KEY);
        // What came from the account goes (on Android, everything).
        self.follow_stremio(&[]);
        self.show_stremio("Signed out of Stremio.");
        let http = self.http.clone();
        self.tasks.spawn(async move { stremio::logout(&http, &account).await });
    }

    /// At start: the account's list again. Offline, what is installed stays.
    fn stremio_refresh(&self) {
        let Some(account) = self.stremio_account() else { return };
        let http = self.http.clone();
        self.tasks.spawn(async move {
            let listed = stremio::listed(&http, &account).await;
            on_ui(move |library| match listed {
                Ok(listed) => library.follow_stremio(&listed),
                Err(StremioError::Refused(_)) => library.show_stremio("Stremio asks you to sign in again."),
                Err(err) => haven_core::diag::note(&format!("stremio: kept the last list: {err}")),
            });
        });
    }

    /// Makes what is installed match the account's list.
    fn follow_stremio(&self, listed: &[Listed]) {
        let have: Vec<String> = self.store.addons().unwrap_or_default().into_iter().map(|a| a.url).collect();
        let followed: Vec<String> = self.store.get(FOLLOWED).ok().flatten().unwrap_or_default();
        let (add, remove, kept) = stremio::followed_changes(&have, &followed, listed);
        let _ = if kept.is_empty() { self.store.remove(FOLLOWED) } else { self.store.put(FOLLOWED, &kept) };
        if add.is_empty() && remove.is_empty() {
            return;
        }
        for url in &remove {
            let _ = self.store.remove_addon(url);
        }
        for item in add {
            let _ = self.store.save_addon(&item.url, &item.manifest);
        }
        self.load_installed();
        self.reload_home();
    }
}
