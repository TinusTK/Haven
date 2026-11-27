//! Looks for a newer Haven and installs it: quietly looks when Haven
//! starts (unless switched off in Settings) and when asked for in
//! Settings > About. When the user says so, Haven downloads the newer
//! one from its own releases, checks the copy against the checksum
//! GitHub states, and installs it.

use std::{cell::RefCell, path::PathBuf, time::Duration};

use haven_core::{http, update};

use super::{Library, on_ui};

/// The setting: look for a newer Haven at start. On unless set off.
const AT_START: &str = "update_check_at_start";
/// Where an issue is reported: GitHub's form for a new one.
const NEW_ISSUE: &str = "https://github.com/TinusTK/Haven/issues/new";
/// The largest download taken for an update.
const MOST_BYTES: u64 = 600 * 1024 * 1024;

#[derive(Default)]
pub(super) struct Updates {
    /// The newer Haven found by the last look, if any.
    found: RefCell<Option<update::Latest>>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Fetches the download into `folder`, saying how far it is through
/// `progress` (0 to 100), and checks it. Returns where it was put.
async fn fetch(
    http: &reqwest::Client,
    download: &update::Download,
    folder: PathBuf,
    progress: impl Fn(u8),
) -> Result<PathBuf, String> {
    let failed = |err: reqwest::Error| {
        if err.is_timeout() { "the download took too long".to_owned() } else { "the download failed".to_owned() }
    };
    let url = http::web_url(&download.address).map_err(|err| err.to_string())?;
    let mut response =
        http.get(url).timeout(Duration::from_secs(3600)).send().await.map_err(failed)?;
    if !response.status().is_success() {
        return Err(format!("the server answered with error {}", response.status().as_u16()));
    }
    let total = response.content_length().unwrap_or(download.size);
    let mut bytes: Vec<u8> = Vec::new();
    let mut said = 0;
    while let Some(part) = response.chunk().await.map_err(failed)? {
        if bytes.len() as u64 + part.len() as u64 > MOST_BYTES {
            return Err("the download is larger than expected".to_owned());
        }
        bytes.extend_from_slice(&part);
        if total > 0 {
            let now = (bytes.len() as u64 * 100 / total).min(100) as u8;
            if now != said {
                said = now;
                progress(now);
            }
        }
    }
    // The copy must be the file GitHub says the release holds.
    match &download.sha256 {
        Some(expected) if *expected == sha256_hex(&bytes) => {}
        Some(_) => return Err("the download did not match its checksum, so it was not used".to_owned()),
        None => return Err("the release gives no checksum to check the download by".to_owned()),
    }
    std::fs::create_dir_all(&folder).map_err(|err| err.to_string())?;
    // Older downloads are not kept.
    if let Ok(old) = std::fs::read_dir(&folder) {
        for entry in old.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let file = folder.join(&download.name);
    std::fs::write(&file, &bytes).map_err(|err| err.to_string())?;
    Ok(file)
}

/// A release's notes (as `update::plain_notes` gives them) as lines
/// for Settings: bullets and the ones under them told apart.
fn note_lines(notes: &str) -> slint::ModelRc<crate::NoteLine> {
    let lines: Vec<crate::NoteLine> = notes
        .lines()
        .map(|line| {
            let (deep, line) = match line.strip_prefix("    ") {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            let (bullet, text) = match line.strip_prefix("•  ") {
                Some(rest) => (true, rest),
                None => (false, line),
            };
            crate::NoteLine { bullet, deep, text: text.into() }
        })
        .collect();
    std::rc::Rc::new(slint::VecModel::from(lines)).into()
}

/// What became of the downloaded installer.
enum Installed {
    /// The installer has taken over: Haven closes so it can be replaced.
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    Closing,
    /// Done or handed on; the words say what the user does next.
    #[cfg_attr(not(any(target_os = "linux", target_os = "macos")), allow(dead_code))]
    Say(String),
}

/// Starts the installer for this kind of computer.
#[allow(unused_variables)]
fn install(file: &std::path::Path, version: &str) -> Result<Installed, String> {
    #[cfg(target_os = "windows")]
    {
        // Haven's own installer, without its questions; it starts the
        // new Haven when it is done.
        std::process::Command::new(file)
            .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
            .spawn()
            .map_err(|err| format!("the installer could not be started: {err}"))?;
        return Ok(Installed::Closing);
    }
    #[cfg(target_os = "linux")]
    {
        // Installing a package needs the system's say-so: pkexec asks
        // for the password in a window of its own.
        let has = |tool: &str| {
            std::env::var_os("PATH").is_some_and(|path| {
                std::env::split_paths(&path).any(|folder| folder.join(tool).is_file())
            })
        };
        if has("pkexec") && (has("apt-get") || has("dpkg")) {
            let mut command = std::process::Command::new("pkexec");
            if has("apt-get") {
                command.args(["apt-get", "install", "-y"]).arg(file);
            } else {
                command.args(["dpkg", "-i"]).arg(file);
            }
            let done = command.status().map_err(|err| format!("the installer could not be started: {err}"))?;
            return if done.success() {
                Ok(Installed::Say(format!("Haven {version} is installed. Close Haven and open it again to use it.")))
            } else {
                Err("the installation was cancelled or failed".to_owned())
            };
        }
        std::process::Command::new("xdg-open")
            .arg(file)
            .spawn()
            .map_err(|err| format!("the package could not be opened: {err}"))?;
        return Ok(Installed::Say(format!("Haven {version} is downloaded and opened in your package installer.")));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(file)
            .spawn()
            .map_err(|err| format!("the download could not be opened: {err}"))?;
        return Ok(Installed::Say(format!(
            "Haven {version} is downloaded and open: drag it to Applications, then open it again."
        )));
    }
    #[allow(unreachable_code)]
    Err("Haven cannot install updates on this kind of computer".to_owned())
}

impl Library {
    pub(super) fn start_updates(self: &std::rc::Rc<Self>, ui: &crate::AppWindow) {
        ui.on_check_updates({
            let library = self.clone();
            move || library.check_updates(true)
        });
        ui.on_install_update({
            let library = self.clone();
            move || library.install_update()
        });
        ui.on_report_issue(|| super::servers::open_in_browser(NEW_ISSUE));
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
        if ui.get_update_busy() {
            return;
        }
        if asked {
            ui.set_update_status("Looking…".into());
        }
        let address = update::LATEST.to_owned();
        // A stand-in for GitHub, to check Haven itself.
        #[cfg(debug_assertions)]
        let address = std::env::var("HAVEN_UPDATES_AT").unwrap_or(address);
        let http = self.http.clone();
        // This build, as releases are tagged ("0.2.6-build3").
        let current = concat!(env!("CARGO_PKG_VERSION"), "-build", env!("HAVEN_BUILD"));
        let shown_current = update::shown(current);
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
                if let Ok(latest) = &found {
                    ui.set_update_notes(note_lines(&latest.notes));
                    ui.set_update_notes_version(update::shown(&latest.version).into());
                }
                match found {
                    Ok(latest) if update::newer(current, &latest.version) => {
                        ui.set_update_version(update::shown(&latest.version).into());
                        ui.set_update_status(
                            format!("Haven {} is out. You have {shown_current}.", update::shown(&latest.version)).into(),
                        );
                        *library.updates.found.borrow_mut() = Some(latest);
                    }
                    Ok(_) => {
                        ui.set_update_version("".into());
                        *library.updates.found.borrow_mut() = None;
                        if asked {
                            ui.set_update_status(format!("You have the newest Haven ({shown_current}).").into());
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

    /// Downloads the newer Haven found, checks it and installs it. A
    /// release with nothing for this kind of computer opens its page.
    fn install_update(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let Some(latest) = self.updates.found.borrow().clone() else { return };
        let Some(download) = latest.download.clone() else {
            return super::servers::open_in_browser(&latest.page);
        };
        // A stand-in for the file's address, to check Haven itself.
        #[cfg(debug_assertions)]
        let download = match std::env::var("HAVEN_UPDATE_FILE_AT") {
            Ok(address) => update::Download { address, ..download },
            Err(_) => download,
        };
        if ui.get_update_busy() {
            return;
        }
        ui.set_update_busy(true);
        ui.set_update_status(format!("Downloading Haven {}…", update::shown(&latest.version)).into());
        // Beside the picture cache, in Haven's own data folder.
        let folder = self
            .picture_cache
            .parent()
            .map(|cache| cache.join("updates"))
            .unwrap_or_else(|| self.picture_cache.join("updates"));
        let http = self.http.clone();
        let version = update::shown(&latest.version);
        self.tasks.spawn(async move {
            let said = version.clone();
            let fetched = fetch(&http, &download, folder, move |percent| {
                let words = format!("Downloading Haven {said}… {percent}%");
                on_ui(move |library| {
                    if let Some(ui) = library.ui.upgrade() {
                        ui.set_update_status(words.into());
                    }
                });
            })
            .await;
            let done = match fetched {
                Ok(file) => {
                    let installing = format!("Installing Haven {version}…");
                    on_ui(move |library| {
                        if let Some(ui) = library.ui.upgrade() {
                            ui.set_update_status(installing.into());
                        }
                    });
                    install(&file, &version)
                }
                Err(why) => Err(why),
            };
            on_ui(move |library| {
                let Some(ui) = library.ui.upgrade() else { return };
                ui.set_update_busy(false);
                match done {
                    Ok(Installed::Closing) => {
                        ui.set_update_status("Haven closes to finish the update and opens again by itself.".into());
                        let _ = slint::quit_event_loop();
                    }
                    Ok(Installed::Say(words)) => ui.set_update_status(words.into()),
                    Err(why) => {
                        haven_core::diag::note(&format!("update: {why}"));
                        ui.set_update_status(format!("Could not update Haven: {why}.").into());
                    }
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_checksum_is_worked_out_in_hex() {
        assert_eq!(
            super::sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
