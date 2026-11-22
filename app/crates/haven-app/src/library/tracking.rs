//! Keys for outside services (Settings), extra details from TMDB, and
//! tracking with Trakt and Simkl.

use std::{
    rc::Rc,
    sync::atomic::Ordering,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use haven_addons::MetaDetail;
use haven_core::settings::Keys;
use haven_tracking::{Account, Client, Service};
use slint::{ModelRc, VecModel};

use super::{BACKGROUND_SIZE, CAST_SHOWN, DetailImage, Library, POSTER_SIZE, on_ui, servers::open_in_browser};
use crate::{AppWindow, TrackingRow};

const ACCOUNTS: &str = "tracking_accounts";

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs() as i64)
}

fn service_of(index: i32) -> Service {
    if index == 1 { Service::Simkl } else { Service::Trakt }
}

impl Library {
    /// Hooks up the Keys and Tracking parts of Settings.
    pub(super) fn start_tracking(self: &Rc<Self>, ui: &AppWindow) {
        let keys = Keys::load(&self.store).unwrap_or_default();
        ui.set_keys_tmdb(keys.tmdb.as_str().into());
        ui.set_tmdb_built_in(option_env!("HAVEN_TMDB_API_KEY").is_some_and(|key| !key.trim().is_empty()));
        ui.set_keys_trakt_id(keys.trakt_id.as_str().into());
        ui.set_keys_trakt_secret(keys.trakt_secret.as_str().into());
        ui.set_keys_simkl_id(keys.simkl_id.as_str().into());
        *self.keys.borrow_mut() = keys.resolved(&self.secrets_file);
        *self.accounts.borrow_mut() =
            self.store.get::<Vec<Account>>(ACCOUNTS).ok().flatten().unwrap_or_default();

        ui.on_save_keys({
            let library = self.clone();
            move || library.save_keys()
        });
        ui.on_tracking_connect({
            let library = self.clone();
            move |service| library.tracking_connect(service_of(service))
        });
        ui.on_tracking_cancel({
            let library = self.clone();
            move || library.tracking_cancel()
        });
        ui.on_tracking_disconnect({
            let library = self.clone();
            move |service| library.tracking_disconnect(service_of(service))
        });
        ui.on_tracking_import({
            let library = self.clone();
            move |service| library.tracking_import(service_of(service))
        });
        self.show_tracking();
    }

    pub(super) fn tracker(&self) -> Client {
        Client::new(self.http.clone(), self.keys.borrow().clone())
    }

    fn account(&self, service: Service) -> Option<Account> {
        self.accounts.borrow().iter().find(|account| account.service == service).cloned()
    }

    pub(super) fn save_accounts(&self) {
        if let Err(err) = self.store.put(ACCOUNTS, &*self.accounts.borrow()) {
            eprintln!("haven: could not save the tracking sign-ins: {err}");
        }
    }

    fn tracking_status(&self, text: &str) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_tracking_status(text.into());
        }
    }

    /// Fills the Tracking rows in Settings.
    fn show_tracking(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let tracker = self.tracker();
        let rows: Vec<TrackingRow> = [Service::Trakt, Service::Simkl]
            .into_iter()
            .enumerate()
            .map(|(index, service)| {
                let connected = self.account(service).is_some();
                let has_keys = tracker.has_keys(service);
                TrackingRow {
                    service: index as i32,
                    name: service.label().into(),
                    state: if connected {
                        "Linked: what you finish is marked watched, and your watchlist is passed on."
                    } else if has_keys && service == Service::Trakt && cfg!(any(target_os = "android", target_os = "ios")) {
                        "Not linked. Link opens trakt.tv and shows a short code to approve there."
                    } else if has_keys && service == Service::Trakt {
                        "Not linked. Link opens trakt.tv to sign in, then comes back to Haven."
                    } else if has_keys {
                        "Not linked. Link opens simkl.com: sign in and allow Haven, and it links by itself."
                    } else if service == Service::Trakt {
                        "Not available in this copy of Haven yet (no Trakt app key)."
                    } else {
                        "Not available in this copy of Haven yet (no Simkl app key)."
                    }
                    .into(),
                    connected,
                    can_connect: has_keys,
                }
            })
            .collect();
        ui.set_tracking(ModelRc::new(VecModel::from(rows)));
        drop(ui);
        self.show_sync();
    }

    fn save_keys(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let keys = Keys {
            tmdb: ui.get_keys_tmdb().trim().to_owned(),
            trakt_id: ui.get_keys_trakt_id().trim().to_owned(),
            trakt_secret: ui.get_keys_trakt_secret().trim().to_owned(),
            simkl_id: ui.get_keys_simkl_id().trim().to_owned(),
        };
        match keys.save(&self.store) {
            Ok(()) => {
                *self.keys.borrow_mut() = keys.resolved(&self.secrets_file);
                ui.set_keys_status("Keys saved.".into());
                // A TMDB key turns the built-in catalogue on or off.
                self.reload_catalog();
                self.rebuild_explore();
            }
            Err(err) => ui.set_keys_status(format!("Could not save the keys: {err}").into()),
        }
        self.show_tracking();
    }

    /// Starts a sign-in: shows the code, opens the service's page and
    /// waits for the user to approve it there.
    fn tracking_connect(&self, service: Service) {
        // A computer: sign in on trakt.tv and come straight back. A phone
        // puts Haven to sleep behind the browser, so it cannot hear
        // trakt.tv's answer: there Trakt's activation page and a short
        // code are used, and Haven links by itself once it is approved.
        if service == Service::Trakt && !cfg!(any(target_os = "android", target_os = "ios")) {
            return self.trakt_link();
        }
        let attempt = self.tracking_attempt.fetch_add(1, Ordering::SeqCst) + 1;
        let attempts = self.tracking_attempt.clone();
        let tracker = self.tracker();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_tracking_busy(true);
        }
        self.tracking_status(&format!("Asking {} for a code…", service.label()));
        self.tasks.spawn(async move {
            let current = || attempts.load(Ordering::SeqCst) == attempt;
            let code = match tracker.begin(service).await {
                Ok(code) => code,
                Err(err) => {
                    let err = err.to_string();
                    return on_ui(move |library| library.tracking_connected(service, Err(err)));
                }
            };
            {
                let (user_code, link) = (code.user_code.clone(), code.link.clone());
                on_ui(move |library| {
                    if let Some(ui) = library.ui.upgrade() {
                        ui.set_tracking_code(user_code.into());
                        ui.set_tracking_link(link.as_str().into());
                    }
                    library.tracking_status(&if service == Service::Simkl {
                        "Sign in on the Simkl page that just opened in your web browser and allow Haven. Haven links by itself.".to_owned()
                    } else {
                        format!("Enter the code on the {} page that just opened in your web browser.", service.label())
                    });
                    open_in_browser(&link);
                });
            }
            let started = std::time::Instant::now();
            let result = loop {
                tokio::time::sleep(Duration::from_secs(code.interval)).await;
                if !current() {
                    return;
                }
                if started.elapsed().as_secs() > code.expires_in {
                    break Err(haven_tracking::Error::Expired.to_string());
                }
                match tracker.poll(&code, now()).await {
                    Ok(Some(account)) => break Ok(account),
                    Ok(None) => {}
                    Err(err) => break Err(err.to_string()),
                }
            };
            if current() {
                on_ui(move |library| library.tracking_connected(service, result));
            }
        });
    }

    /// Links Trakt through its website: opens trakt.tv's sign-in page in
    /// the web browser and waits for trakt.tv to send the browser back to
    /// Haven (a listener on this computer) with a code, which is traded
    /// for a sign-in. Nothing to type.
    fn trakt_link(&self) {
        let attempt = self.tracking_attempt.fetch_add(1, Ordering::SeqCst) + 1;
        let attempts = self.tracking_attempt.clone();
        let tracker = self.tracker();
        let state = random_state();
        // A new PKCE pair for each sign-in: only this run of Haven can
        // turn Trakt's code into a sign-in.
        let pkce = haven_tracking::Pkce::new();
        let link = match tracker.trakt_authorize_url(&state, &pkce.challenge) {
            Ok(link) => link,
            Err(_) => {
                return self.tracking_status(
                    "This copy of Haven has no Trakt app set up yet, so it cannot link to Trakt.",
                );
            }
        };
        if let Some(ui) = self.ui.upgrade() {
            ui.set_tracking_busy(true);
            ui.set_tracking_link(link.as_str().into());
        }
        self.tracking_status("Sign in to Trakt in your web browser and choose Yes to link Haven. Haven carries on by itself.");
        self.tasks.spawn(async move {
            let current = || attempts.load(Ordering::SeqCst) == attempt;
            // A link tried a moment ago may still hold the port: it lets
            // go within a second of being replaced.
            let mut tries = 0;
            let listener = loop {
                match tokio::net::TcpListener::bind(("127.0.0.1", haven_tracking::TRAKT_REDIRECT_PORT)).await {
                    Ok(listener) => break listener,
                    Err(_) if tries < 8 && current() => {
                        tries += 1;
                        tokio::time::sleep(Duration::from_millis(500)).await;
                    }
                    Err(err) => {
                        if !current() {
                            return;
                        }
                        let err = format!("Haven could not wait for Trakt's answer ({err})");
                        return on_ui(move |library| library.tracking_connected(Service::Trakt, Err(err)));
                    }
                }
            };
            on_ui(move |_| open_in_browser(&link));
            let started = std::time::Instant::now();
            let code = loop {
                if !current() {
                    return;
                }
                if started.elapsed() > Duration::from_secs(15 * 60) {
                    break Err(haven_tracking::Error::Expired.to_string());
                }
                let Ok(Ok((stream, _))) = tokio::time::timeout(Duration::from_secs(1), listener.accept()).await else {
                    continue;
                };
                match answer_redirect(stream, &state).await {
                    Some(Ok(code)) => break Ok(code),
                    Some(Err(why)) => break Err(why),
                    None => continue,
                }
            };
            drop(listener);
            let result = match code {
                Ok(code) => tracker.trakt_exchange(&code, &pkce.verifier, now()).await.map_err(|err| err.to_string()),
                Err(err) => Err(err),
            };
            if current() {
                on_ui(move |library| library.tracking_connected(Service::Trakt, result));
            }
        });
    }

    fn tracking_cancel(&self) {
        self.tracking_attempt.fetch_add(1, Ordering::SeqCst);
        self.tracking_idle();
        self.tracking_status("");
    }

    fn tracking_idle(&self) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_tracking_busy(false);
            ui.set_tracking_code("".into());
            ui.set_tracking_link("".into());
        }
    }

    fn tracking_connected(&self, service: Service, result: Result<Account, String>) {
        self.tracking_idle();
        match result {
            Ok(account) => {
                self.accounts.borrow_mut().retain(|known| known.service != service);
                self.accounts.borrow_mut().push(account);
                self.save_accounts();
                self.tracking_status(&format!("Linked to {}.", service.label()));
                // Linking turns its sync on: bring in what the new account
                // holds, and send what waits.
                self.set_account_sync(service, true);
            }
            Err(err) => {
                self.tracking_status(&format!("Could not link {}: {err}.", service.label()))
            }
        }
        self.show_tracking();
    }

    fn tracking_disconnect(&self, service: Service) {
        // Tell the service too, so the link is gone on its side as well.
        if let Some(account) = self.account(service) {
            let tracker = self.tracker();
            self.tasks.spawn(async move {
                let _ = match account.service {
                    Service::Trakt => tracker.trakt_revoke(&account).await,
                    Service::Simkl => tracker.simkl_revoke(&account).await,
                };
            });
        }
        self.accounts.borrow_mut().retain(|account| account.service != service);
        self.save_accounts();
        self.tracking_status(&format!(
            "Unlinked from {}. Haven has forgotten the sign-in.",
            service.label()
        ));
        self.show_tracking();
    }

    /// Notes a Watchlist change for the linked accounts (sent by sync).
    pub(super) fn track_watchlist(&self, kind: &str, title_id: &str, on: bool) {
        let what = if on { "watchlist_add" } else { "watchlist_remove" };
        self.queue_sync(what, kind, title_id, "");
    }

    /// Notes a finished film or episode for the linked accounts (sent by sync).
    pub(super) fn track_watched(&self, kind: &str, title_id: &str, video_id: &str) {
        self.queue_sync("watched", kind, title_id, video_id);
    }

    /// Copies a service's watchlist into Haven's.
    fn tracking_import(&self, service: Service) {
        let Some(account) = self.account(service) else { return };
        let tracker = self.tracker();
        self.tracking_status(&format!("Fetching your {} watchlist…", service.label()));
        self.tasks.spawn(async move {
            let listed = tracker.watchlist(&account).await.map_err(|err| err.to_string());
            on_ui(move |library| {
                let listed = match listed {
                    Ok(listed) => listed,
                    Err(err) => {
                        return library.tracking_status(&format!(
                            "Could not fetch your {} watchlist: {err}.",
                            service.label()
                        ));
                    }
                };
                let added = library.add_listed(&listed);
                library.reload_watchlist();
                library.tracking_status(&format!(
                    "{} of {} titles from {} were new and are now on your Watchlist.",
                    added,
                    listed.len(),
                    service.label()
                ));
            });
        });
    }

    /// Asks TMDB for what the add-on left out of a title's details.
    pub(super) fn enrich(&self, generation: u64, meta: &MetaDetail) {
        let Some(key) = self.keys.borrow().tmdb_api_key.clone() else { return };
        // A media server's title names its IMDb id beside its own.
        let Some(id) = Some(meta.id.clone())
            .filter(|id| haven_metadata::is_imdb_id(id))
            .or_else(|| meta.imdb_id.clone().filter(|id| haven_metadata::is_imdb_id(id)))
        else {
            return;
        };
        let (http, series) = (self.http.clone(), meta.kind == "series");
        self.tasks.spawn(async move {
            let details = match haven_metadata::details(&http, &key, series, &id).await {
                Ok(Some(details)) => details,
                Ok(None) => return,
                Err(err) => return eprintln!("haven: TMDB details: {err}"),
            };
            on_ui(move |library| {
                if generation != library.detail_generation.get() {
                    return;
                }
                let Some(ui) = library.ui.upgrade() else { return };
                let shown = ui.get_detail();
                library.update_detail(|detail| {
                    if detail.description.is_empty() {
                        detail.description = details.overview.clone().unwrap_or_default().into();
                    }
                    if detail.cast.is_empty() {
                        detail.cast =
                            details.cast.iter().take(CAST_SHOWN).cloned().collect::<Vec<_>>().join(", ").into();
                    }
                    if detail.director.is_empty() {
                        detail.director = details.directors.join(", ").into();
                    }
                });
                let pictures = [
                    (details.poster.filter(|_| !shown.has_poster), POSTER_SIZE, DetailImage::Poster),
                    (details.backdrop.filter(|_| !shown.has_background), BACKGROUND_SIZE, DetailImage::Background),
                ];
                for (url, size, which) in pictures {
                    if let Some(url) = url {
                        library.fetch_image(url, size, move |library, pixels| {
                            // An add-on's own picture that arrived meanwhile wins.
                            let has = library.ui.upgrade().map(|ui| ui.get_detail());
                            let taken = has.is_some_and(|detail| match which {
                                DetailImage::Poster => detail.has_poster,
                                DetailImage::Background => detail.has_background,
                            });
                            if !taken {
                                library.detail_image_loaded(generation, which, pixels);
                            }
                        });
                    }
                }
            });
        });
    }
}

/// A hard-to-guess word that trakt.tv sends back, so only the answer to
/// Haven's own request is taken.
fn random_state() -> String {
    use ring::rand::SecureRandom;
    let mut bytes = [0u8; 16];
    let _ = ring::rand::SystemRandom::new().fill(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads one browser request to the listener. `None` for anything that
/// is not trakt.tv's answer (a favicon, a stray visit); else the code, or
/// why there is none (the user said no).
async fn answer_redirect(mut stream: tokio::net::TcpStream, state: &str) -> Option<Result<String, String>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buffer = vec![0u8; 8192];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await.ok()?.ok()?;
    let request = String::from_utf8_lossy(&buffer[..read]);
    let target = request.lines().next()?.split_whitespace().nth(1)?.to_owned();
    let found = trakt_redirect_answer(&target, state);
    let (title, body) = match &found {
        Some(Ok(_)) => ("Haven is linked to Trakt", "You can close this tab and go back to Haven."),
        Some(Err(_)) => ("Haven was not linked", "Trakt did not allow it. Go back to Haven to try again."),
        None => ("Haven", "Nothing to see here."),
    };
    let page = format!(
        "<!doctype html><meta charset=utf-8><title>{title}</title>\
         <body style=\"font-family:system-ui;background:#0d0f14;color:#eef0f6;display:grid;place-items:center;height:90vh\">\
         <div style=\"text-align:center\"><h1 style=\"color:#f2c66d\">{title}</h1><p>{body}</p></div>"
    );
    let reply = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}",
        if found.is_some() { "200 OK" } else { "404 Not Found" },
        page.len()
    );
    let _ = stream.write_all(reply.as_bytes()).await;
    let _ = stream.shutdown().await;
    found
}

/// What the address trakt.tv sent the browser to says: `/trakt?code=…&state=…`.
fn trakt_redirect_answer(target: &str, state: &str) -> Option<Result<String, String>> {
    let url = reqwest::Url::parse(&format!("http://127.0.0.1{target}")).ok()?;
    if url.path() != "/trakt" {
        return None;
    }
    let pairs: std::collections::HashMap<String, String> = url.query_pairs().into_owned().collect();
    if pairs.get("state").map(String::as_str) != Some(state) {
        return None;
    }
    match pairs.get("code") {
        Some(code) if !code.is_empty() => Some(Ok(code.clone())),
        _ => Some(Err("Trakt did not allow it".to_owned())),
    }
}

#[cfg(test)]
mod link_tests {
    use super::trakt_redirect_answer;

    #[test]
    fn only_trakts_answer_to_this_request_is_taken() {
        assert_eq!(trakt_redirect_answer("/trakt?code=abc&state=s1", "s1"), Some(Ok("abc".into())));
        assert_eq!(trakt_redirect_answer("/trakt?code=abc&state=other", "s1"), None);
        assert_eq!(trakt_redirect_answer("/favicon.ico", "s1"), None);
        assert!(matches!(trakt_redirect_answer("/trakt?error=access_denied&state=s1", "s1"), Some(Err(_))));
    }
}
