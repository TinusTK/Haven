//! Media servers (Jellyfin, Emby, Plex): signing in, the Media Servers
//! page, and telling a server how far something was watched. Their titles
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
        self.load_servers();
    }

    /// Reads the servers from the database into memory and the page.
    fn load_servers(&self) {
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
        let rows: Vec<ServerRow> = servers
            .iter()
            .map(|server| ServerRow {
                id: server.id as i32,
                name: server.name.as_str().into(),
                kind: server.kind.label().into(),
                address: server.address.as_str().into(),
            })
            .collect();
        *self.servers.borrow_mut() = servers;
        if let Some(ui) = self.ui.upgrade() {
            ui.set_servers(ModelRc::new(VecModel::from(rows)));
        }
        self.reload_server_shelves();
    }

    /// The server a `srv:…` id belongs to and the item's id on it.
    pub(super) fn server_item(&self, id: &str) -> Option<(Server, String)> {
        let (number, item) = split_id(id)?;
        let server = self.servers.borrow().iter().find(|server| server.id == number)?.clone();
        Some((server, item.to_owned()))
    }

    fn server_status(&self, text: &str, is_error: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_server_status(text.into());
            ui.set_server_status_is_error(is_error);
        }
    }

    fn server_busy(&self, busy: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_server_busy(busy);
        }
    }

    /// Asks every server for its libraries; each library then gets two
    /// shelves, the newest titles and the titles by name.
    fn reload_server_shelves(&self) {
        let generation = self.server_generation.get() + 1;
        self.server_generation.set(generation);
        let servers = self.servers.borrow().clone();
        if servers.is_empty() {
            return self.fill(Board::Servers, Vec::new(), None);
        }
        let client = self.server_client.clone();
        self.tasks.spawn(async move {
            let mut plans = Vec::new();
            let mut failures = Vec::new();
            for server in servers {
                match client.libraries(&server).await {
                    Ok(libraries) => {
                        for library in libraries {
                            for latest in [true, false] {
                                plans.push(ShelfPlan {
                                    title: if latest {
                                        format!("New in {}", library.name)
                                    } else {
                                        library.name.clone()
                                    },
                                    source: server.name.clone(),
                                    from: ShelfFrom::Server {
                                        server: server.clone(),
                                        library: library.id.clone(),
                                        latest,
                                    },
                                });
                            }
                        }
                    }
                    Err(err) => failures.push(format!("{}: {err}", server.name)),
                }
            }
            on_ui(move |library| {
                if generation != library.server_generation.get() {
                    return;
                }
                if !failures.is_empty() {
                    library.server_status(
                        &format!("Could not load from {}.", failures.join("; ")),
                        true,
                    );
                }
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

    fn servers_added(&self, added: Result<Vec<Server>, String>) {
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

    fn remove_server(&self, id: i64) {
        if let Err(err) = self.store.remove_server(id) {
            return self.server_status(&format!("Could not remove the server: {err}"), true);
        }
        self.load_servers();
        self.server_status("Server removed. Haven has forgotten its sign-in.", false);
    }

    /// Tells the server a film or episode came from how far it was watched.
    pub(super) fn report_played(&self, played: &Played) {
        let Some((server, item)) = self.server_item(&played.video_id) else { return };
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
