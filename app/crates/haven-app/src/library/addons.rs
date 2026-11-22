//! Add-ons: installing, switching off and removing them, the add-ons a
//! media server shares, and the streams add-ons offer. Only in builds with
//! the `addons` feature (the builds on GitHub); store builds have none of
//! this.

use std::rc::Rc;

use haven_addons::{manifest_url, parse_manifest};
use haven_titles::Stream;
use slint::{ModelRc, VecModel};

use super::{Library, on_ui};
use crate::{AddonRow, AppWindow};

impl Library {
    pub(super) fn connect_addons(self: &Rc<Self>, ui: &AppWindow) {
        ui.on_install_addon({
            let library = self.clone();
            move |address| library.install(address.as_str())
        });
        ui.on_remove_addon({
            let library = self.clone();
            move |url| library.remove(url.as_str())
        });
        ui.on_set_addon_enabled({
            let library = self.clone();
            move |url, on| library.set_addon_enabled(url.as_str(), on)
        });
        ui.on_install_shared({
            let library = self.clone();
            move |index| library.install_shared(index)
        });
    }


    /// Reads the installed add-ons from the database into memory and the
    /// Add-ons page, then rebuilds the list of sources.
    pub(super) fn load_installed(&self) {
        let stored = self.store.addons().unwrap_or_else(|err| {
            eprintln!("haven: could not read the installed add-ons: {err}");
            Vec::new()
        });
        let off = haven_core::settings::SwitchedOff::load(&self.store).unwrap_or_default().addons;
        let mut rows = Vec::new();
        let mut urls = Vec::new();
        let mut sources = Vec::new();
        for addon in stored {
            // Cinemeta is built in now; the copy installed as an add-on by
            // earlier versions goes.
            if addon.url == haven_titles::CINEMETA_URL {
                let _ = self.store.remove_addon(&addon.url);
                continue;
            }
            let manifest = match parse_manifest(&addon.manifest) {
                Ok(manifest) => manifest,
                Err(err) => {
                    eprintln!("haven: skipping add-on {}: {err}", addon.url);
                    continue;
                }
            };
            let enabled = !off.contains(&addon.url);
            rows.push(AddonRow {
                url: addon.url.as_str().into(),
                name: manifest.name.as_str().into(),
                description: manifest.description.as_deref().unwrap_or_default().into(),
                version: manifest.version.as_str().into(),
                catalogs: manifest.catalogs.iter().filter(|c| !c.needs_input()).count() as i32,
                enabled,
            });
            urls.push(addon.url.clone());
            // An add-on that is switched off is listed but never asked.
            if enabled {
                sources.push(manifest.source(&addon.url));
            }
        }
        *self.addon_urls.borrow_mut() = urls;
        *self.addon_sources.borrow_mut() = sources;
        self.load_sources();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_addons(ModelRc::new(VecModel::from(rows)));
        }
        self.show_shared_addons();
    }

    fn set_status(&self, text: &str, is_error: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_addon_status(text.into());
            ui.set_addon_status_is_error(is_error);
        }
    }

    fn set_busy(&self, busy: bool) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_addon_busy(busy);
        }
    }

    /// Fetches the manifest at the address the user gave and installs it.
    fn install(&self, address: &str) {
        let url = match manifest_url(address) {
            Ok(url) => url,
            Err(err) => {
                return self.set_status(&format!("Could not install the add-on: {err}."), true);
            }
        };
        if url == haven_titles::CINEMETA_URL {
            return self.set_status("Cinemeta is built into Haven: there is no need to install it.", false);
        }
        self.set_busy(true);
        self.set_status("Contacting the add-on…", false);
        let addons = self.addons.clone();
        self.tasks.spawn(async move {
            let fetched = async {
                let json = addons.manifest_json(&url).await?;
                let manifest = parse_manifest(&json)?;
                Ok::<_, haven_addons::Error>((json, manifest.name))
            }
            .await
            .map_err(|err| err.to_string());
            on_ui(move |library| library.install_finished(&url, fetched));
        });
    }

    fn install_finished(&self, url: &str, fetched: Result<(String, String), String>) {
        self.set_busy(false);
        let (json, name) = match fetched {
            Ok(fetched) => fetched,
            Err(err) => {
                return self.set_status(&format!("Could not install the add-on: {err}."), true);
            }
        };
        if let Err(err) = self.store.save_addon(url, &json) {
            return self.set_status(&format!("Could not save the add-on: {err}"), true);
        }
        self.load_installed();
        self.reload_home();
        self.clear_search();
        self.rebuild_explore();
        self.set_status(&format!("Installed {name}."), false);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_addon_address("".into());
        }
    }

    /// Switches an add-on on or off. Off, it stays installed but is not
    /// asked for shelves, details or streams.
    fn set_addon_enabled(&self, url: &str, on: bool) {
        let mut off = haven_core::settings::SwitchedOff::load(&self.store).unwrap_or_default();
        haven_core::settings::switch(&mut off.addons, url.to_owned(), on);
        if let Err(err) = off.save(&self.store) {
            return self.set_status(&format!("Could not save that: {err}"), true);
        }
        self.load_installed();
        self.reload_home();
        self.clear_search();
        self.rebuild_explore();
        self.set_status(if on { "Add-on switched on." } else { "Add-on switched off." }, false);
    }

    fn remove(&self, url: &str) {
        if let Err(err) = self.store.remove_addon(url) {
            return self.set_status(&format!("Could not remove the add-on: {err}"), true);
        }
        self.load_installed();
        self.reload_home();
        self.clear_search();
        self.rebuild_explore();
        self.set_status("Add-on removed.", false);
    }

    /// Asks each server for the add-ons it uses and shares (Trellis does,
    /// with its owner), so they can be installed in Haven with a click.
    pub(super) fn load_shared_addons(&self) {
        let servers = self.servers.borrow().clone();
        let client = self.server_client.clone();
        self.tasks.spawn(async move {
            let mut shared = Vec::new();
            for server in servers {
                match client.shared_addons(&server).await {
                    Ok(addons) => {
                        shared.extend(addons.into_iter().map(|addon| (server.name.clone(), addon)))
                    }
                    // Only its owner may read a server's add-on list.
                    Err(err) => haven_core::diag::note(&format!(
                        "{} did not share its add-on list: {err}",
                        server.name
                    )),
                }
            }
            on_ui(move |library| {
                *library.shared_addons.borrow_mut() = shared;
                library.show_shared_addons();
            });
        });
    }

    /// Fills the list of servers' add-ons on the Add-ons page.
    pub(super) fn show_shared_addons(&self) {
        let installed = self.addon_urls.borrow();
        let rows: Vec<crate::SharedAddonRow> = self
            .shared_addons
            .borrow()
            .iter()
            .enumerate()
            .map(|(index, (server, addon))| crate::SharedAddonRow {
                index: index as i32,
                name: addon.name.as_str().into(),
                server: server.as_str().into(),
                installed: haven_addons::manifest_url(&addon.manifest_url)
                    .is_ok_and(|url| installed.contains(&url)),
            })
            .collect();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_shared_addons(ModelRc::new(VecModel::from(rows)));
        }
    }

    /// Installs one of the servers' add-ons in Haven; `index` -1 installs
    /// every one that is not installed yet.
    fn install_shared(&self, index: i32) {
        let wanted: Vec<String> = self
            .shared_addons
            .borrow()
            .iter()
            .enumerate()
            .filter(|(at, _)| index < 0 || *at as i32 == index)
            .map(|(_, (_, addon))| addon.manifest_url.clone())
            .collect();
        let installed = self.addon_urls.borrow().clone();
        for address in wanted {
            if !haven_addons::manifest_url(&address).is_ok_and(|url| installed.contains(&url)) {
                self.install(&address);
            }
        }
    }


    /// Asks every add-on that offers streams for this film or episode;
    /// returns how many were asked.
    pub(super) fn ask_addons_for_streams(&self, generation: u64, kind: &str, addon_id: &str) -> usize {
        let sources: Vec<(String, String)> = self
            .addon_sources
            .borrow()
            .iter()
            .filter(|addon| addon.provides("stream", kind, addon_id))
            .map(|addon| (addon.url.clone(), addon.name.clone()))
            .collect();
        let asked = sources.len();
        for (url, name) in sources {
            let (addons, kind, addon_id) = (self.addons.clone(), kind.to_owned(), addon_id.to_owned());
            self.tasks.spawn(async move {
                let streams: Result<Vec<Stream>, String> =
                    addons.streams(&url, &kind, &addon_id).await.map_err(|err| err.to_string());
                on_ui(move |library| library.streams_loaded(generation, &name, None, streams));
            });
        }
        asked
    }
}
