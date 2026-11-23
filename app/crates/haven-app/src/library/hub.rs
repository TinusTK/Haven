//! Haven Hub from the app's side: pairing, handing it this device's
//! setup, taking the Hub's, and keeping watch progress in step.
//!
//! The Hub is kept like a media server (kind `hub`): its shelves, details
//! and ways to watch come through `haven_servers`, as a server's do.

use std::rc::Rc;

use haven_core::hub::{Include, Setup};
use haven_servers::{Kind, Server};

use super::{Library, NO_ADDONS, on_ui};
use crate::AppWindow;

/// Add-ons travel with the setup only in builds that have them.
const INCLUDE: Include = Include { sources: !NO_ADDONS, hubs: false };

impl Library {
    pub(super) fn connect_hub(self: &Rc<Self>, ui: &AppWindow) {
        ui.on_hub_send({
            let library = self.clone();
            move || library.hub_send(false)
        });
        ui.on_hub_get({
            let library = self.clone();
            move || library.hub_get(false)
        });
    }

    /// The paired Hub, if there is one and it is switched on.
    pub(super) fn hub(&self) -> Option<Server> {
        self.servers.borrow().iter().find(|server| server.kind == Kind::Hub).cloned()
    }

    fn hub_status(&self, text: &str, busy: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_hub_status(text.into());
            ui.set_hub_busy(busy);
        }
    }

    /// Pairs with the Hub at `address` with the code from its page.
    pub(super) fn pair_hub(&self, address: &str, code: &str) {
        self.server_busy(true);
        self.server_status("Pairing with the Hub…", false);
        let client = self.server_client.clone();
        let (address, code) = (address.to_owned(), code.to_owned());
        self.tasks.spawn(async move {
            let paired = client.pair_hub(&address, &code).await.map(|server| vec![server]).map_err(|err| err.to_string());
            on_ui(move |library| {
                let ok = paired.is_ok();
                library.servers_added(paired);
                if ok {
                    // A fresh pairing takes the Hub's setup straight away.
                    library.hub_get(true);
                }
            });
        });
    }

    /// Hands the Hub this device's whole setup.
    pub(super) fn hub_send(&self, quiet: bool) {
        let Some(hub) = self.hub() else { return };
        let mut setup = match Setup::read(&self.store, INCLUDE) {
            Ok(setup) => setup,
            Err(err) => return self.hub_status(&format!("Could not read this device's setup: {err}"), false),
        };
        if quiet {
            // In the background only what was watched goes: servers,
            // add-ons, lists and keys go when "Send everything" is pressed,
            // so a device never puts back what was removed on the Hub.
            setup.servers.clear();
            setup.sources.clear();
            setup.tv_sources.clear();
            setup.keys = None;
            setup.off = None;
        } else {
            // "Send everything": the Hub is made to match this device.
            setup.replace = true;
        }
        if !quiet {
            self.hub_status("Sending to the Hub…", true);
        }
        let client = self.server_client.clone();
        self.tasks.spawn(async move {
            let sent = client.send_setup(&hub, &setup).await;
            on_ui(move |library| match sent {
                Ok((added, removed)) if !quiet => library.hub_status(&sent_words(added, removed), false),
                Ok(_) => {}
                Err(err) if !quiet => library.hub_status(&format!("Could not send to the Hub: {err}."), false),
                Err(err) => eprintln!("haven: could not send to the Hub: {err}"),
            });
        });
    }

    /// Takes the Hub's setup into this device (nothing is removed).
    pub(super) fn hub_get(&self, quiet: bool) {
        let Some(hub) = self.hub() else { return };
        if !quiet {
            self.hub_status("Asking the Hub…", true);
        }
        let client = self.server_client.clone();
        self.tasks.spawn(async move {
            let fetched = client.fetch_setup(&hub).await.map_err(|err| err.to_string());
            // The series' dates the Hub keeps fresh, for the calendar.
            let calendar = client.hub_calendar(&hub).await.unwrap_or_default();
            on_ui(move |library| {
                library.hub_calendar(calendar);
                library.hub_got(fetched, quiet);
            });
        });
    }

    fn hub_got(&self, fetched: Result<Setup, String>, quiet: bool) {
        let merged = fetched.and_then(|setup| setup.merge_into(&self.store, INCLUDE).map_err(|err| err.to_string()));
        if merged.as_ref().is_ok_and(|m| m.tracking) {
            // A Trakt or Simkl sign-in came from the Hub.
            *self.accounts.borrow_mut() =
                self.store.get(haven_tracking::sync::ACCOUNTS).ok().flatten().unwrap_or_default();
            self.show_tracking();
            self.show_sync();
        }
        match merged.map(|m| m.added) {
            Ok(0) => {
                if !quiet {
                    self.hub_status("This device already had everything the Hub has.", false);
                }
            }
            Ok(added) => {
                self.load_servers();
                self.reload_watchlist();
                self.reload_continue();
                self.refresh_ticks();
                self.recheck_calendar();
                #[cfg(feature = "addons")]
                {
                    self.load_installed();
                    self.reload_home();
                    self.rebuild_explore();
                }
                self.hub_status(
                    &format!(
                        "Took {added} new {} from the Hub. New TV sources show the next time Haven starts.",
                        if added == 1 { "thing" } else { "things" }
                    ),
                    false,
                );
            }
            Err(err) if !quiet => self.hub_status(&format!("Could not get the Hub's setup: {err}."), false),
            Err(err) => eprintln!("haven: could not get the Hub's setup: {err}"),
        }
    }

    /// Keeps the series' details the Hub fetched, as if fetched here: the
    /// calendar shows their dates at once.
    fn hub_calendar(&self, series: Vec<haven_titles::MetaDetail>) {
        if series.is_empty() {
            return;
        }
        let dir = self.kept_dir();
        for meta in &series {
            super::kept::write_kept(&dir, &super::kept::details_name(&meta.kind, &meta.id), meta);
        }
        self.recheck_calendar();
    }

    /// Two ways at once, quietly: this device's progress to the Hub, and
    /// the Hub's (other devices') back. At start and after watching.
    pub(super) fn hub_sync(&self) {
        if self.hub().is_some() {
            self.hub_send(true);
            self.hub_get(true);
        }
    }
}

/// What "Send everything to the Hub" did, in words.
fn sent_words(added: u64, removed: Option<u64>) -> String {
    let Some(removed) = removed else {
        // A Hub from before 0.2.4 only adds.
        return match added {
            0 => "The Hub already had everything. Update the Hub to have removals and switches sent too.".to_owned(),
            n => format!("Sent: {n} new on the Hub. Update the Hub to have removals and switches sent too."),
        };
    };
    let things = |n: u64| if n == 1 { "1 thing".to_owned() } else { format!("{n} things") };
    match (added, removed) {
        (0, 0) => "Sent. The Hub matches this device.".to_owned(),
        (a, 0) => format!("Sent. The Hub matches this device ({} added).", things(a)),
        (0, r) => format!("Sent. The Hub matches this device ({} removed).", things(r)),
        (a, r) => format!("Sent. The Hub matches this device ({} added, {} removed).", things(a), things(r)),
    }
}
