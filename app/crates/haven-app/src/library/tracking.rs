//! Keys for outside services (Settings), extra details from TMDB, and
//! tracking with Trakt and Simkl.

use std::{
    rc::Rc,
    sync::atomic::Ordering,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use haven_addons::MetaDetail;
use haven_core::{settings::Keys, storage::Saved};
use haven_tracking::{Account, Client, Service, Subject, subject};
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

    fn tracker(&self) -> Client {
        Client::new(self.http.clone(), self.keys.borrow().clone())
    }

    fn account(&self, service: Service) -> Option<Account> {
        self.accounts.borrow().iter().find(|account| account.service == service).cloned()
    }

    fn save_accounts(&self) {
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
                        "Connected: what you finish is marked watched, and your watchlist is passed on."
                    } else if has_keys {
                        "Not connected."
                    } else if service == Service::Trakt {
                        "Add a Trakt client ID and secret below to connect."
                    } else {
                        "Add a Simkl client ID below to connect."
                    }
                    .into(),
                    connected,
                    can_connect: has_keys,
                }
            })
            .collect();
        ui.set_tracking(ModelRc::new(VecModel::from(rows)));
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
                    library.tracking_status(&format!(
                        "Enter the code on the {} page that just opened in your web browser.",
                        service.label()
                    ));
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
                self.tracking_status(&format!("Connected to {}.", service.label()));
            }
            Err(err) => {
                self.tracking_status(&format!("Could not connect to {}: {err}.", service.label()))
            }
        }
        self.show_tracking();
    }

    fn tracking_disconnect(&self, service: Service) {
        self.accounts.borrow_mut().retain(|account| account.service != service);
        self.save_accounts();
        self.tracking_status(&format!(
            "Disconnected from {}. Haven has forgotten the sign-in.",
            service.label()
        ));
        self.show_tracking();
    }

    /// Runs `work` for every connected service, off the UI thread, with a
    /// token that is still good.
    fn with_accounts<Work, Fut>(&self, what: &'static str, work: Work)
    where
        Work: Fn(Client, Account) -> Fut + Send + 'static,
        Fut: Future<Output = haven_tracking::Result<()>> + Send,
    {
        let accounts = self.accounts.borrow().clone();
        if accounts.is_empty() {
            return;
        }
        let tracker = self.tracker();
        self.tasks.spawn(async move {
            for account in accounts {
                let account = match tracker.renewed(&account, now()).await {
                    Ok(Some(renewed)) => {
                        let keep = renewed.clone();
                        on_ui(move |library| {
                            let mut accounts = library.accounts.borrow_mut();
                            accounts.retain(|known| known.service != keep.service);
                            accounts.push(keep);
                            drop(accounts);
                            library.save_accounts();
                        });
                        renewed
                    }
                    Ok(None) => account,
                    Err(err) => {
                        eprintln!("haven: {} sign-in could not be renewed: {err}", account.service.label());
                        continue;
                    }
                };
                let service = account.service;
                if let Err(err) = work(tracker.clone(), account).await {
                    eprintln!("haven: could not {what} on {}: {err}", service.label());
                }
            }
        });
    }

    /// Passes a watchlist change on to the connected services.
    pub(super) fn track_watchlist(&self, kind: &str, title_id: &str, on: bool) {
        let Some(subject) = subject(kind, title_id, None) else { return };
        self.with_accounts("change the watchlist", move |tracker, account| {
            let subject: Subject = subject.clone();
            async move { tracker.set_watchlist(&account, &subject, on).await }
        });
    }

    /// Marks a finished film or episode as watched on the connected services.
    pub(super) fn track_watched(&self, kind: &str, title_id: &str, video_id: &str) {
        let Some(subject) = subject(kind, title_id, Some(video_id)) else { return };
        // A series without a known episode cannot be marked.
        if subject.series && subject.episode.is_none() {
            return;
        }
        self.with_accounts("mark this watched", move |tracker, account| {
            let subject: Subject = subject.clone();
            async move { tracker.mark_watched(&account, &subject).await }
        });
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
                let mut added = 0;
                // Oldest first, so the list keeps the service's order.
                for (place, title) in listed.iter().rev().enumerate() {
                    let kind = if title.series { "series" } else { "movie" };
                    if library.store.in_watchlist(kind, &title.imdb).unwrap_or(true) {
                        continue;
                    }
                    let saved = library.store.add_to_watchlist(&Saved {
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
        if !haven_metadata::is_imdb_id(&meta.id) {
            return;
        }
        let (http, id, series) = (self.http.clone(), meta.id.clone(), meta.kind == "series");
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
