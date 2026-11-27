//! Library > Local: videos already on this computer, in the Movies
//! (Videos) and Downloads folders.

use std::{cell::Cell, path::PathBuf, rc::Rc};

use slint::{ModelRc, VecModel};

use super::{Library, PlayRequest, on_ui};
use crate::{AppWindow, LocalRow};

/// The most videos listed.
const MOST: usize = 1000;

/// A video found on the device.
#[derive(Debug, Clone)]
pub(super) struct Found {
    pub name: String,
    /// Where it is, in words ("Movies", "Download"…).
    pub place: String,
    pub size: u64,
    pub modified: i64,
    /// The file's path.
    pub address: String,
}

#[derive(Default)]
pub(super) struct LocalState {
    found: std::cell::RefCell<Vec<Found>>,
    busy: Cell<bool>,
}

fn size_words(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let bytes = bytes as f64;
    if bytes >= 1024.0 * MB { format!("{:.1} GB", bytes / (1024.0 * MB)) } else { format!("{:.0} MB", bytes / MB) }
}

fn is_video(path: &std::path::Path) -> bool {
    const KINDS: &[&str] = &["mp4", "m4v", "mkv", "mov", "avi", "webm", "ts", "m2ts", "wmv", "flv", "mpg", "mpeg", "3gp"];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| KINDS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Walks `root` a few folders deep for videos.
fn scan(root: &std::path::Path, place: &str, found: &mut Vec<Found>, depth: u32) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        if found.len() >= MOST {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            if depth > 0 {
                scan(&path, place, found, depth - 1);
            }
        } else if is_video(&path) {
            found.push(Found {
                name: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name),
                place: place.to_owned(),
                size: meta.len(),
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs() as i64),
                address: path.display().to_string(),
            });
        }
    }
}

/// The folders looked in, with how to name them.
fn folders() -> Vec<(PathBuf, String)> {
    {
        let mut list = Vec::new();
        if let Some(dirs) = directories::UserDirs::new() {
            if let Some(videos) = dirs.video_dir() {
                list.push((videos.to_path_buf(), videos.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()));
            }
            if let Some(downloads) = dirs.download_dir() {
                list.push((downloads.to_path_buf(), "Downloads".to_owned()));
            }
        }
        list
    }
}

/// Every video found, newest first. Off the UI thread.
fn find_videos() -> Result<Vec<Found>, String> {
    let mut found = Vec::new();
    for (folder, place) in folders() {
        scan(&folder, &place, &mut found, 3);
    }
    found.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(found)
}

impl Library {
    pub(super) fn start_local(self: &Rc<Self>, ui: &AppWindow) {
        ui.on_local_open({
            let library = self.clone();
            move || library.open_local()
        });
        ui.on_local_play({
            let library = self.clone();
            move |row| library.play_local(row as usize)
        });
        ui.set_local_place("Videos in your Movies and Downloads folders".into());
    }

    fn local_status(&self, text: &str) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_local_status(text.into());
            ui.set_local_busy(self.local.busy.get());
        }
    }

    /// The Local tab was opened (or "Look again"): finds the videos.
    fn open_local(self: &Rc<Self>) {
        if self.local.busy.get() {
            return;
        }
        self.local.busy.set(true);
        self.local_status("Looking for videos…");
        self.tasks.spawn(async move {
                        let found = tokio::task::spawn_blocking(find_videos).await.unwrap_or_else(|e| Err(e.to_string()));
            on_ui(move |library| {
                library.local.busy.set(false);
                match found {
                    Ok(found) => library.show_local(found),
                    Err(err) => library.local_status(&format!("Could not look for videos: {err}.")),
                }
            });
        });
    }

    fn show_local(&self, found: Vec<Found>) {
        let rows: Vec<LocalRow> = found
            .iter()
            .map(|f| LocalRow {
                title: f.name.as_str().into(),
                detail: [f.place.clone(), if f.size > 0 { size_words(f.size) } else { String::new() }]
                    .into_iter()
                    .filter(|part| !part.is_empty())
                    .collect::<Vec<_>>()
                    .join("  ·  ")
                    .into(),
            })
            .collect();
        let status = match rows.len() {
            0 => "No videos found.".to_owned(),
            1 => "1 video.".to_owned(),
            n => format!("{n} videos."),
        };
        *self.local.found.borrow_mut() = found;
        if let Some(ui) = self.ui.upgrade() {
            ui.set_local_rows(ModelRc::new(VecModel::from(rows)));
        }
        self.local_status(&status);
    }

    fn play_local(&self, row: usize) {
        let Some(found) = self.local.found.borrow().get(row).cloned() else { return };
        let request = PlayRequest {
            address: found.address.clone(),
            title: found.name.clone(),
            detail: String::new(),
            kind: "movie".into(),
            title_id: format!("local:{}", found.address),
            video_id: format!("local:{}", found.address),
            poster: None,
            remember: true,
            source: "On this device".into(),
            next: None,
        };
        if let Some(play) = self.player.borrow().as_ref() {
            *self.playing_item.borrow_mut() = None;
            play(request);
        }
    }
}
