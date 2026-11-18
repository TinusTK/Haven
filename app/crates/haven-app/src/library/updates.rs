//! Looks for a newer Haven: quietly when Haven starts (unless switched
//! off in Settings) and when asked for in Settings > About. Nothing is
//! downloaded or installed by itself: Haven says a newer one is out
//! and opens its download in the web browser when asked.

use haven_core::{http, update};

use super::{Library, on_ui};

/// The setting: look for a newer Haven at start. On unless set off.
const AT_START: &str = "update_check_at_start";

impl Library {
    pub(super) fn start_updates(self: &std::rc::Rc<Self>, ui: &crate::AppWindow) {
        ui.on_check_updates({
            let library = self.clone();
            move || library.check_updates(true)
        });
        ui.on_set_update_check({
            let library = self.clone();
            move |on| {
                if let Err(err) = library.store.put(AT_START, &on) {
                    eprintln!("haven: could not save that: {err}");
                }
            }
        });
        let at_start = self.store.get::<bool>(AT_START).ok().flatten() != Some(false);
        ui.set_update_check_at_start(at_start);
        if at_start {
            self.check_updates(false);
        }
    }

    /// Asks whether a newer Haven is out. `asked` says the user pressed
    /// the button: then "you have the newest" and failures are said too.
    fn check_updates(&self, asked: bool) {
        let Some(ui) = self.ui.upgrade() else { return };
        if asked {
            ui.set_update_status("Looking…".into());
        }
        let address = update::LATEST.to_owned();
        // A stand-in for GitHub, to check Haven itself.
        #[cfg(debug_assertions)]
        let address = std::env::var("HAVEN_UPDATES_AT").unwrap_or(address);
        let http = self.http.clone();
        let current = env!("CARGO_PKG_VERSION");
        self.tasks.spawn(async move {
            let headers = [("Accept", "application/vnd.github+json".to_owned())];
            let found = match http::send(&http, &address, &headers, None, 2 * 1024 * 1024).await {
                Ok((200, body)) => serde_json::from_slice::<serde_json::Value>(&body)
                    .ok()
                    .and_then(|answer| update::parse_latest(&answer, std::env::consts::OS))
                    .ok_or_else(|| "the answer could not be read".to_owned()),
                // GitHub answers 404 while the releases are private.
                Ok((404, _)) => Err("Haven's releases are not public yet".to_owned()),
                Ok((status, _)) => Err(format!("the server answered with error {status}")),
                Err(err) => Err(err.to_string()),
            };
            on_ui(move |library| {
                let Some(ui) = library.ui.upgrade() else { return };
                match found {
                    Ok(latest) if update::newer(current, &latest.version) => {
                        ui.set_update_version(latest.version.as_str().into());
                        ui.set_update_link(latest.download.unwrap_or(latest.page).into());
                        ui.set_update_status(
                            format!("Haven {} is out. You have {current}.", latest.version).into(),
                        );
                    }
                    Ok(_) => {
                        ui.set_update_version("".into());
                        if asked {
                            ui.set_update_status(format!("You have the newest Haven ({current}).").into());
                        }
                    }
                    Err(why) => {
                        haven_core::diag::note(&format!("update check: {why}"));
                        if asked {
                            ui.set_update_status(format!("Could not look for a newer Haven: {why}.").into());
                        }
                    }
                }
            });
        });
    }
}
