//! Media servers (Jellyfin, Emby, Plex): signing in (in Settings), whether
//! each is online, their shelves on Home, and telling a server how far
//! something was watched. Their titles
//! then flow through the same shelves, title page and stream list as
//! add-on titles.

use std::{
    rc::Rc,
    sync::atomic::Ordering,
    time::Duration,
};

use haven_core::storage::StoredServer;
use haven_servers::{Kind, Server, split_id};
use slint::{ModelRc, VecModel};

use super::{Board, Library, Played, ShelfFrom, ShelfPlan, on_ui};
use crate::{AppWindow, ServerRow};

/// How long a Plex code stays worth waiting for.
const PLEX_WAIT: Duration = Duration::from_secs(5 * 60);
const PLEX_ASK_EVERY: Duration = Duration::from_secs(2);
/// How often the servers are asked whether they still answer.
const CHECK_EVERY: Duration = Duration::from_secs(60);
/// A server that takes longer than this to say so counts as offline.
const CHECK_WAIT: Duration = Duration::from_secs(8);
const UNKNOWN: i32 = 0;
const ONLINE: i32 = 1;
const OFFLINE: i32 = 2;

fn stored(server: &Server) -> StoredServer {
    StoredServer {
        id: server.id,
        kind: server.kind.as_str().to_owned(),
        name: server.name.clone(),
        address: server.address.clone(),
        user_id: server.user_id.clone(),
        token: server.token.clone(),
    }
}

impl Library {
    /// Hooks up the Media Servers page and shows the stored servers.
    pub(super) fn start_servers(self: &Rc<Self>, ui: &AppWindow) {
        ui.set_server_shelves(ModelRc::from(self.server_shelves.shelves.clone()));
        ui.on_server_sign_in({
            let library = self.clone();
            move |kind, address, user, password| {
                if kind == 3 {
                    return library.pair_hub(address.as_str(), user.as_str());
                }
                let kind = if kind == 1 { Kind::Emby } else { Kind::Jellyfin };
                library.sign_in(kind, address.as_str(), user.as_str(), password.as_str());
            }
        });
        ui.on_plex_sign_in({
            let library = self.clone();
            move || library.plex_sign_in()
        });
        ui.on_plex_cancel({
            let library = self.clone();
            move || library.plex_cancel()
        });
        ui.on_remove_server({
            let library = self.clone();
            move |id| library.remove_server(id as i64)
        });
        ui.on_open_link(|link| open_in_browser(link.as_str()));
        ui.on_set_server_enabled({
            let library = self.clone();
            move |id, on| library.set_server_enabled(id as i64, on)
        });
        ui.on_check_servers({
            let library = self.clone();
            move || library.check_servers(true)
        });
        let library = Rc::downgrade(self);
        self.server_timer.start(slint::TimerMode::Repeated, CHECK_EVERY, move || {
            if let Some(library) = library.upgrade() {
                library.check_servers(false);
            }
        });
        self.load_servers();
    }

    /// Fills the list of servers in Settings, each with its state.
    fn show_servers(&self) {
        let states = self.server_states.borrow();
        let on = self.servers.borrow();
        let rows: Vec<ServerRow> = self
            .all_servers
            .borrow()
            .iter()
            .map(|server| ServerRow {
                id: server.id as i32,
                name: server.name.as_str().into(),
                kind: server.kind.label().into(),
                address: server.address.as_str().into(),
                state: states.get(&server.id).copied().unwrap_or(UNKNOWN),
                enabled: on.iter().any(|known| known.id == server.id),
            })
            .collect();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_servers(ModelRc::new(VecModel::from(rows)));
            ui.set_has_hub(self.hub().is_some());
        }
    }

    /// Notes whether a server answers. One that comes back online gets
    /// its shelves loaded again.
    fn set_server_state(&self, id: i64, state: i32) {
        let before = self.server_states.borrow_mut().insert(id, state);
        if before == Some(state) {
            return;
        }
        self.show_servers();
        if before == Some(OFFLINE) && state == ONLINE {
            self.reload_server_shelves();
        }
    }

    /// Asks every server whether it answers. `show` marks them as being
    /// checked meanwhile (the user asked); the quiet check does not.
    fn check_servers(&self, show: bool) {
        let servers = self.servers.borrow().clone();
        if show {
            self.server_states.borrow_mut().clear();
            self.show_servers();
        }
        for server in servers {
            let client = self.server_client.clone();
            self.tasks.spawn(async move {
                let online = tokio::time::timeout(CHECK_WAIT, client.online(&server))
                    .await
                    .unwrap_or(false);
                on_ui(move |library| {
                    // A server removed meanwhile has no state to keep.
                    if library.servers.borrow().iter().any(|known| known.id == server.id) {
                        library.set_server_state(server.id, if online { ONLINE } else { OFFLINE });
                    }
                });
            });
        }
    }

    /// Reads the servers from the database into memory and the page.
    pub(super) fn load_servers(&self) {
        let servers: Vec<Server> = self
            .store
            .servers()
            .unwrap_or_else(|err| {
                eprintln!("haven: could not read the media servers: {err}");
                Vec::new()
            })
            .into_iter()
            .filter_map(|server| {
                Some(Server {
                    id: server.id,
                    kind: Kind::parse(&server.kind)?,
                    name: server.name,
                    address: server.address,
                    user_id: server.user_id,
                    token: server.token,
                })
            })
            .collect();
        // A server that is switched off is listed but never asked.
        let off = haven_core::settings::SwitchedOff::load(&self.store).unwrap_or_default().servers;
        *self.all_servers.borrow_mut() = servers.clone();
        let servers: Vec<Server> =
            servers.into_iter().filter(|server| !off.contains(&server.id)).collect();
        self.server_states
            .borrow_mut()
            .retain(|id, _| servers.iter().any(|server| server.id == *id));
        *self.servers.borrow_mut() = servers;
        self.show_servers();
        self.reload_server_shelves();
        self.check_servers(false);
    }

    /// The server a `srv:…` id belongs to and the item's id on it.
    pub(super) fn server_item(&self, id: &str) -> Option<(Server, String)> {
        let (number, item) = split_id(id)?;
        let server = self.servers.borrow().iter().find(|server| server.id == number)?.clone();
        Some((server, item.to_owned()))
    }

    pub(super) fn server_status(&self, text: &str, is_error: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_server_status(text.into());
            ui.set_server_status_is_error(is_error);
        }
    }

    pub(super) fn server_busy(&self, busy: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_server_busy(busy);
        }
    }

    /// Asks every server for its libraries; each library then gets two
    /// shelves, the newest titles and the titles by name.
    pub(super) fn reload_server_shelves(&self) {
        let generation = self.server_generation.get() + 1;
        self.server_generation.set(generation);
        let servers = self.servers.borrow().clone();
        if servers.is_empty() {
            self.server_libraries.borrow_mut().clear();
            #[cfg(feature = "addons")]
            {
                self.shared_addons.borrow_mut().clear();
                self.show_shared_addons();
            }
            self.rebuild_explore();
            return self.fill(Board::Servers, Vec::new(), None);
        }
        // The libraries found last time: their shelves show at once.
        let kept: Option<Vec<(haven_servers::Server, Vec<haven_servers::Library>)>> = servers
            .iter()
            .map(|server| {
                self.kept::<Vec<(String, String, bool)>>(&libraries_name(server)).map(|list| {
                    let libraries = list
                        .into_iter()
                        .map(|(id, name, collection)| haven_servers::Library { id, name, collection })
                        .collect();
                    (server.clone(), libraries)
                })
            })
            .collect();
        let kept_plans = kept.as_ref().map(|kept| {
            let (plans, found) = library_plans(kept);
            *self.server_libraries.borrow_mut() = found;
            self.rebuild_explore();
            let names: Vec<(String, String)> = plans.iter().map(|plan| (plan.title.clone(), plan.source.clone())).collect();
            self.fill(Board::Servers, plans, None);
            names
        });
        let client = self.server_client.clone();
        let dir = self.kept_dir();
        self.tasks.spawn(async move {
            let mut failures = Vec::new();
            let mut answered = Vec::new();
            let mut listed = Vec::new();
            for server in servers {
                let libraries = match tokio::time::timeout(CHECK_WAIT, client.libraries(&server)).await {
                    Ok(libraries) => libraries.map_err(|err| err.to_string()),
                    Err(_) => Err("it did not answer".to_owned()),
                };
                answered.push((server.id, libraries.is_ok()));
                match libraries {
                    Ok(libraries) => {
                        let keep: Vec<(String, String, bool)> = libraries
                            .iter()
                            .map(|library| (library.id.clone(), library.name.clone(), library.collection))
                            .collect();
                        super::kept::write_kept(&dir, &libraries_name(&server), &keep);
                        listed.push((server.clone(), libraries));
                    }
                    Err(err) => failures.push(format!("{}: {err}", server.name)),
                }
            }
            let (plans, found) = library_plans(&listed);
            on_ui(move |library| {
                if generation != library.server_generation.get() {
                    return;
                }
                // The list in Settings shows these servers as offline.
                for failure in &failures {
                    eprintln!("haven: could not load from {failure}");
                }
                for (id, fine) in answered {
                    library.server_states.borrow_mut().insert(id, if fine { ONLINE } else { OFFLINE });
                }
                library.show_servers();
                #[cfg(feature = "addons")]
                library.load_shared_addons();
                // The same shelves as the kept ones on screen: keep those
                // (each is already being fetched afresh).
                let names: Vec<(String, String)> = plans.iter().map(|plan| (plan.title.clone(), plan.source.clone())).collect();
                if kept_plans.as_ref() == Some(&names) {
                    return;
                }
                *library.server_libraries.borrow_mut() = found;
                library.rebuild_explore();
                library.fill(Board::Servers, plans, None);
            });
        });
    }

    /// Signs in to a Jellyfin or Emby server. The password is passed on
    /// once and never stored.
    fn sign_in(&self, kind: Kind, address: &str, user: &str, password: &str) {
        self.server_busy(true);
        self.server_status("Signing in…", false);
        let client = self.server_client.clone();
        let (address, user, password) = (address.to_owned(), user.to_owned(), password.to_owned());
        self.tasks.spawn(async move {
            let signed_in = client
                .sign_in(kind, &address, &user, &password)
                .await
                .map(|server| vec![server])
                .map_err(|err| err.to_string());
            on_ui(move |library| library.servers_added(signed_in));
        });
    }

    /// Starts signing in to Plex: shows a code, opens plex.tv in the web
    /// browser and waits for the user to approve it there.
    fn plex_sign_in(&self) {
        let attempt = self.plex_attempt.fetch_add(1, Ordering::SeqCst) + 1;
        let attempts = self.plex_attempt.clone();
        self.server_busy(true);
        self.server_status("Asking Plex for a code…", false);
        let client = self.server_client.clone();
        self.tasks.spawn(async move {
            let current = || attempts.load(Ordering::SeqCst) == attempt;
            let pin = match client.plex_pin().await {
                Ok(pin) => pin,
                Err(err) => {
                    let err = err.to_string();
                    return on_ui(move |library| library.servers_added(Err(err)));
                }
            };
            {
                let (code, link) = (pin.code.clone(), pin.link.clone());
                on_ui(move |library| {
                    if let Some(ui) = library.ui.upgrade() {
                        ui.set_plex_code(code.into());
                        ui.set_plex_link(link.as_str().into());
                    }
                    library.server_status(
                        "Approve Haven on the Plex page that just opened in your web browser.",
                        false,
                    );
                    open_in_browser(&link);
                });
            }
            let started = std::time::Instant::now();
            let found = loop {
                tokio::time::sleep(PLEX_ASK_EVERY).await;
                if !current() {
                    return; // cancelled, or another attempt took over
                }
                if started.elapsed() > PLEX_WAIT {
                    break Err("The Plex code was not approved in time.".to_owned());
                }
                match client.plex_token(&pin).await {
                    Ok(Some(token)) => {
                        break match client.plex_servers(&token).await {
                            Ok(servers) if servers.is_empty() => {
                                Err("This Plex account has no servers.".to_owned())
                            }
                            Ok(servers) => Ok(servers),
                            Err(err) => Err(err.to_string()),
                        };
                    }
                    Ok(None) => {}
                    Err(err) => break Err(err.to_string()),
                }
            };
            if current() {
                on_ui(move |library| library.servers_added(found));
            }
        });
    }

    fn plex_cancel(&self) {
        self.plex_attempt.fetch_add(1, Ordering::SeqCst);
        self.server_busy(false);
        self.server_status("", false);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_plex_code("".into());
            ui.set_plex_link("".into());
        }
    }

    pub(super) fn servers_added(&self, added: Result<Vec<Server>, String>) {
        self.server_busy(false);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_plex_code("".into());
            ui.set_plex_link("".into());
            // Whatever happened, the password does not stay on screen.
            ui.set_server_password("".into());
        }
        let servers = match added {
            Ok(servers) => servers,
            Err(err) => return self.server_status(&format!("Could not sign in: {err}."), true),
        };
        let mut names = Vec::new();
        for server in &servers {
            match self.store.save_server(&stored(server)) {
                Ok(_) => names.push(server.name.clone()),
                Err(err) => {
                    return self.server_status(&format!("Could not save the server: {err}"), true);
                }
            }
        }
        self.load_servers();
        self.server_status(&format!("Signed in to {}.", names.join(", ")), false);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_server_user("".into());
            ui.set_server_address("".into());
        }
    }

    /// Switches a media server on or off. Off, Haven stays signed in to
    /// it but does not ask it for anything.
    fn set_server_enabled(&self, id: i64, on: bool) {
        let mut off = haven_core::settings::SwitchedOff::load(&self.store).unwrap_or_default();
        haven_core::settings::switch(&mut off.servers, id, on);
        if let Err(err) = off.save(&self.store) {
            return self.server_status(&format!("Could not save that: {err}"), true);
        }
        self.load_servers();
        self.server_status(if on { "Server switched on." } else { "Server switched off." }, false);
    }

    fn remove_server(&self, id: i64) {
        if let Err(err) = self.store.remove_server(id) {
            return self.server_status(&format!("Could not remove the server: {err}"), true);
        }
        self.load_servers();
        self.server_status("Server removed. Haven has forgotten its sign-in.", false);
    }

    /// Tells the server a film or episode came from how far it was watched.
    pub(super) fn report_played(&self, played: &Played) {
        // A Hub hears what was watched, and passes it on to other devices.
        self.hub_send(true);
        // What played may be another server's copy of the open title.
        let played_item = self.playing_item.borrow_mut().take();
        let Some((server, item)) =
            self.server_item(played_item.as_deref().unwrap_or(&played.video_id))
        else {
            return;
        };
        if played.duration <= 0.0 {
            return;
        }
        let client = self.server_client.clone();
        let (position, duration, finished) = (played.position, played.duration, played.finished);
        self.tasks.spawn(async move {
            if let Err(err) = client.report(&server, &item, position, duration, finished).await {
                eprintln!("haven: could not tell {} what was watched: {err}", server.name);
            }
        });
    }
}

/// Opens a web address in the user's browser. Only http and https.
pub fn open_in_browser(link: &str) {
    if haven_core::http::web_url(link).is_err() {
        return;
    }
    // Unattended checks: say it instead of opening a browser.
    if cfg!(debug_assertions) && std::env::var_os("HAVEN_NO_BROWSER").is_some() {
        return eprintln!("haven: would open {link}");
    }
    // A phone has no programs to start: the system opens the address.
    #[cfg(target_os = "ios")]
    {
        if let Err(err) = webbrowser::open(link) {
            eprintln!("haven: could not open the web browser: {err}");
        }
        return;
    }
    #[cfg(target_os = "android")]
    {
        if let Err(why) = crate::external::view(link, None) {
            eprintln!("haven: could not open the web browser: {why}");
        }
        return;
    }
    #[allow(unreachable_code)]
    let mut command = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut command = std::process::Command::new("rundll32");
        command.arg("url.dll,FileProtocolHandler");
        command
    } else {
        std::process::Command::new("xdg-open")
    };
    if let Err(err) = command.arg(link).spawn() {
        eprintln!("haven: could not open the web browser: {err}");
    }
}

/// The name a server's list of libraries is kept under.
fn libraries_name(server: &haven_servers::Server) -> String {
    format!("libraries|{}|{}", server.id, server.address)
}

/// The shelves for servers' libraries: a collection is one list, one
/// shelf; a library also gets a shelf of what is new.
fn library_plans(
    listed: &[(haven_servers::Server, Vec<haven_servers::Library>)],
) -> (Vec<ShelfPlan>, Vec<(haven_servers::Server, haven_servers::Library)>) {
    let mut plans = Vec::new();
    let mut found = Vec::new();
    for (server, libraries) in listed {
        for library in libraries {
            found.push((server.clone(), library.clone()));
            let shelves: &[bool] = if library.collection { &[false] } else { &[true, false] };
            for &latest in shelves {
                plans.push(ShelfPlan {
                    title: if latest { format!("New in {}", library.name) } else { library.name.clone() },
                    source: server.name.clone(),
                    from: ShelfFrom::Server { server: server.clone(), library: library.id.clone(), latest },
                });
            }
        }
    }
    (plans, found)
}
