//! Downloads: a film or episode saved to this device from one of its
//! places to watch, and the Library's list of them. Films go in
//! `Movies/`, episodes in `Series/<show>/Season <n>/`, under Haven's
//! folder in the user's videos folder. Removing one in Haven deletes its
//! file too.

use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use haven_core::storage::Download;
use slint::{ModelRc, VecModel};
use tokio::io::AsyncWriteExt;

use super::{Library, PlayRequest, on_ui};
use crate::DownloadRow;

/// How often a download's progress reaches the screen and the database.
const REPORT_EVERY: Duration = Duration::from_millis(700);
/// Downloads fetched at the same time; the rest wait their turn.
const AT_ONCE: usize = 2;

#[derive(Default)]
pub(super) struct Downloads {
    /// Running downloads, by number: set to stop one.
    running: RefCell<HashMap<i64, Arc<AtomicBool>>>,
    /// Bytes saved so far of each running download (newer than the database).
    live: RefCell<HashMap<i64, (i64, i64)>>,
    rows: Rc<VecModel<DownloadRow>>,
}

/// Haven's folder for downloads: `Movies/Haven` on a Mac, `Videos\Haven`
/// on Windows, `~/Videos/Haven` on Linux; on a phone, inside Haven's own
/// data folder.
pub(super) fn downloads_root(data_dir: &Path) -> PathBuf {
    // Unattended checks keep their files away from the user's own folder.
    if cfg!(debug_assertions) {
        if let Some(folder) = std::env::var_os("HAVEN_DOWNLOADS_DIR") {
            return PathBuf::from(folder);
        }
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    match home {
        Some(home) if cfg!(target_os = "macos") => home.join("Movies").join("Haven"),
        Some(home) if cfg!(any(windows, target_os = "linux")) => home.join("Videos").join("Haven"),
        _ => data_dir.join("Downloads"),
    }
}

/// A name that is safe as a file or folder name everywhere.
pub(super) fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_control() || "<>:\"/\\|?*".contains(c) { ' ' } else { c })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_matches(|c: char| c == '.' || c == ' ').to_owned();
    if cleaned.is_empty() { "Untitled".to_owned() } else { cleaned.chars().take(120).collect() }
}

/// The file type, from the address (`.mkv`, `.mp4`…), else `mkv`.
pub(super) fn extension_of(address: &str) -> &'static str {
    let path = address.split(['?', '#']).next().unwrap_or_default().to_ascii_lowercase();
    for known in ["mkv", "mp4", "m4v", "avi", "mov", "webm", "ts", "wmv", "mpg"] {
        if path.ends_with(&format!(".{known}")) {
            return known;
        }
    }
    "mkv"
}

/// Where a download is saved, relative to the downloads folder.
pub(super) fn relative_path(d: &Download, extension: &str) -> PathBuf {
    let show = safe_name(&d.title);
    if d.kind == "movie" || d.season.is_none() {
        return PathBuf::from("Movies").join(format!("{show}.{extension}"));
    }
    let season = d.season.unwrap_or(0);
    let number = match d.episode {
        Some(episode) => format!("S{season:02}E{episode:02}"),
        None => format!("S{season:02}"),
    };
    // "S1 E2 · Its name" gives the episode's name after the dot.
    let name = d.detail.split_once(" · ").map(|(_, name)| safe_name(name));
    let file = match name {
        Some(name) => format!("{show} - {number} - {name}.{extension}"),
        None => format!("{show} - {number}.{extension}"),
    };
    PathBuf::from("Series").join(&show).join(format!("Season {season}")).join(file)
}

fn size_words(bytes: i64) -> String {
    let bytes = bytes.max(0) as f64;
    if bytes >= 1e9 {
        format!("{:.1} GB", bytes / 1e9)
    } else if bytes >= 1e6 {
        format!("{:.0} MB", bytes / 1e6)
    } else {
        format!("{:.0} KB", (bytes / 1e3).max(1.0))
    }
}

impl Library {
    fn downloads_folder(&self) -> PathBuf {
        let data = self.picture_cache.parent().and_then(Path::parent).unwrap_or(Path::new("."));
        downloads_root(data)
    }

    pub(super) fn start_downloads(self: &Rc<Self>, ui: &crate::AppWindow) {
        ui.set_download_rows(ModelRc::from(self.downloads.rows.clone()));
        ui.set_downloads_folder(self.downloads_folder().display().to_string().into());
        ui.on_download_stream({
            let library = self.clone();
            move |row| library.download_stream(row)
        });
        ui.on_download_remove({
            let library = self.clone();
            move |id| library.remove_download(id as i64)
        });
        ui.on_download_toggle({
            let library = self.clone();
            move |id| library.toggle_download(id as i64)
        });
        ui.on_download_play({
            let library = self.clone();
            move |id| library.play_download(id as i64)
        });
        ui.on_open_downloads_folder({
            let library = self.clone();
            move || {
                let folder = library.downloads_folder();
                let _ = std::fs::create_dir_all(&folder);
                crate::external::open_path(&folder);
            }
        });
        // Carry on with downloads a quit left half done.
        for d in self.store.downloads().unwrap_or_default() {
            if d.state == "loading" || d.state == "waiting" {
                let _ = self.store.update_download(d.id, d.size, d.received, "waiting");
            }
        }
        self.next_downloads();
        self.show_downloads();
    }

    /// Saves the place to watch in row `row` of the open title's list.
    fn download_stream(&self, row: i32) {
        let Some((request, _)) = self.request_for(row) else { return };
        let (season, episode) = self.listed_numbers();
        let mut d = Download {
            id: 0,
            kind: if season.is_some() { "series".into() } else { request.kind.clone() },
            title_id: request.title_id.clone(),
            video_id: request.video_id.clone(),
            title: request.title.clone(),
            detail: request.detail.clone(),
            season,
            episode,
            poster: request.poster.clone(),
            url: request.address.clone(),
            path: String::new(),
            size: 0,
            received: 0,
            state: "waiting".into(),
            added_at: chrono::Utc::now().timestamp_millis(),
        };
        // The same film or episode already saved or on its way.
        let existing = self.store.downloads().unwrap_or_default();
        if existing.iter().any(|old| old.kind == d.kind && old.video_id == d.video_id && old.state != "failed") {
            if let Some(ui) = self.ui.upgrade() {
                ui.set_play_note("That one is already in your downloads (Library).".into());
            }
            return;
        }
        let path = self.downloads_folder().join(relative_path(&d, extension_of(&d.url)));
        d.path = path.display().to_string();
        match self.store.add_download(&d) {
            Ok(_) => {
                if let Some(ui) = self.ui.upgrade() {
                    ui.set_play_note(format!("Downloading to {}. It is in your Library under Downloads.", d.path).into());
                }
                self.next_downloads();
                self.show_downloads();
            }
            Err(err) => eprintln!("haven: could not add a download: {err}"),
        }
    }

    /// Starts waiting downloads while fewer than `AT_ONCE` run.
    fn next_downloads(&self) {
        let waiting: Vec<Download> = self
            .store
            .downloads()
            .unwrap_or_default()
            .into_iter()
            .filter(|d| d.state == "waiting")
            .collect();
        for d in waiting {
            if self.downloads.running.borrow().len() >= AT_ONCE {
                break;
            }
            self.run_download(d);
        }
    }

    fn run_download(&self, d: Download) {
        let stop = Arc::new(AtomicBool::new(false));
        self.downloads.running.borrow_mut().insert(d.id, stop.clone());
        let _ = self.store.update_download(d.id, d.size, d.received, "loading");
        let http = self.http.clone();
        self.tasks.spawn(async move {
            let id = d.id;
            let result = fetch(http, d, stop.clone()).await;
            on_ui(move |library| {
                library.downloads.running.borrow_mut().remove(&id);
                library.downloads.live.borrow_mut().remove(&id);
                match result {
                    Ok((size, received, done)) => {
                        let state = if done { "done" } else if stop.load(Ordering::Relaxed) { "paused" } else { "failed" };
                        // A removed download has no row to update.
                        let _ = library.store.update_download(id, size, received, state);
                    }
                    Err((size, received, err)) => {
                        haven_core::diag::note(&format!("download failed: {err}"));
                        let state = if stop.load(Ordering::Relaxed) { "paused" } else { "failed" };
                        let _ = library.store.update_download(id, size, received, state);
                    }
                }
                library.next_downloads();
                library.show_downloads();
            });
        });
    }

    /// Pauses a running download, or carries on with a paused or failed one.
    fn toggle_download(&self, id: i64) {
        if let Some(stop) = self.downloads.running.borrow().get(&id) {
            stop.store(true, Ordering::Relaxed);
            return;
        }
        if let Some(d) = self.store.downloads().unwrap_or_default().into_iter().find(|d| d.id == id) {
            if d.state != "done" {
                let _ = self.store.update_download(id, d.size, d.received, "waiting");
                self.next_downloads();
                self.show_downloads();
            }
        }
    }

    /// Stops it, deletes its file (and the folders left empty) and forgets it.
    fn remove_download(&self, id: i64) {
        if let Some(stop) = self.downloads.running.borrow_mut().remove(&id) {
            stop.store(true, Ordering::Relaxed);
        }
        let Some(d) = self.store.downloads().unwrap_or_default().into_iter().find(|d| d.id == id) else { return };
        let path = PathBuf::from(&d.path);
        let root = self.downloads_folder();
        // Only ever delete inside Haven's downloads folder.
        if path.starts_with(&root) {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(part_path(&path));
            let mut folder = path.parent().map(Path::to_path_buf);
            while let Some(dir) = folder {
                if dir == root || !dir.starts_with(&root) || std::fs::remove_dir(&dir).is_err() {
                    break;
                }
                folder = dir.parent().map(Path::to_path_buf);
            }
        }
        let _ = self.store.remove_download(id);
        self.show_downloads();
    }

    fn play_download(&self, id: i64) {
        let Some(d) = self.store.downloads().unwrap_or_default().into_iter().find(|d| d.id == id) else { return };
        if d.state != "done" || !Path::new(&d.path).exists() {
            return;
        }
        let request = PlayRequest {
            address: d.path.clone(),
            title: d.title.clone(),
            detail: d.detail.clone(),
            kind: d.kind.clone(),
            title_id: d.title_id.clone(),
            video_id: d.video_id.clone(),
            poster: d.poster.clone(),
            remember: true,
            source: "Downloaded".into(),
            next: None,
        };
        if let Some(play) = self.player.borrow().as_ref() {
            *self.playing_item.borrow_mut() = None;
            play(request);
        }
    }

    /// Fills the Library's Downloads list: films, then series, each
    /// series under its name with its episodes in order.
    pub(super) fn show_downloads(&self) {
        let all = self.store.downloads().unwrap_or_default();
        let live = self.downloads.live.borrow();
        let mut rows = Vec::new();
        let mut last_section = "";
        let mut last_show = String::new();
        for d in &all {
            let film = d.kind == "movie" || d.season.is_none();
            let section = if film { "Films" } else { "Series" };
            let (size, received) = live.get(&d.id).copied().unwrap_or((d.size, d.received));
            let progress = if d.state == "done" { 1.0 } else if size > 0 { received as f32 / size as f32 } else { 0.0 };
            let status = match d.state.as_str() {
                "done" => size_words(size.max(received)),
                "paused" => format!("Paused · {:.0}%", progress * 100.0),
                "failed" => "Stopped: could not fetch it. Try again.".to_owned(),
                "waiting" => "Waiting…".to_owned(),
                _ if size > 0 => format!("{:.0}% · {} of {}", progress * 100.0, size_words(received), size_words(size)),
                _ => format!("{} so far", size_words(received)),
            };
            let show_heading = !film && d.title != last_show;
            rows.push(DownloadRow {
                id: d.id as i32,
                section: if section != last_section { section.into() } else { "".into() },
                group: if show_heading { d.title.as_str().into() } else { "".into() },
                title: if film { d.title.as_str().into() } else if d.detail.is_empty() { d.title.as_str().into() } else { d.detail.as_str().into() },
                status: status.into(),
                progress,
                done: d.state == "done",
                running: d.state == "loading",
                failed: d.state == "failed",
            });
            last_section = section;
            if !film {
                last_show = d.title.clone();
            }
        }
        self.downloads.rows.set_vec(rows);
        if let Some(ui) = self.ui.upgrade() {
            ui.set_downloads_count(all.len() as i32);
        }
    }
}

fn part_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    path.with_file_name(name)
}

/// Fetches into `<file>.part`, carrying on from what is there, then
/// renames it. Answers (size, received, finished).
async fn fetch(
    http: reqwest::Client,
    d: Download,
    stop: Arc<AtomicBool>,
) -> Result<(i64, i64, bool), (i64, i64, String)> {
    let path = PathBuf::from(&d.path);
    let part = part_path(&path);
    if let Some(folder) = path.parent() {
        tokio::fs::create_dir_all(folder).await.map_err(|err| (d.size, d.received, err.to_string()))?;
    }
    let mut have = tokio::fs::metadata(&part).await.map(|m| m.len() as i64).unwrap_or(0);
    let mut request = http.get(&d.url).timeout(Duration::from_secs(60 * 60 * 24));
    if have > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={have}-"));
    }
    let mut response = request.send().await.map_err(|err| (d.size, have, err.without_url().to_string()))?;
    let status = response.status();
    if !status.is_success() {
        return Err((d.size, have, format!("the server answered {status}")));
    }
    // A server that ignores the range sends it all again from the start.
    let resumed = status == reqwest::StatusCode::PARTIAL_CONTENT;
    if !resumed {
        have = 0;
    }
    let size = response.content_length().map(|len| len as i64 + have).unwrap_or(d.size);
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(resumed)
        .truncate(!resumed)
        .open(&part)
        .await
        .map_err(|err| (size, have, err.to_string()))?;
    let mut reported = Instant::now();
    let id = d.id;
    loop {
        if stop.load(Ordering::Relaxed) {
            let _ = file.flush().await;
            return Ok((size, have, false));
        }
        match response.chunk().await {
            Ok(Some(chunk)) => {
                file.write_all(&chunk).await.map_err(|err| (size, have, err.to_string()))?;
                have += chunk.len() as i64;
                if reported.elapsed() >= REPORT_EVERY {
                    reported = Instant::now();
                    on_ui(move |library| {
                        library.downloads.live.borrow_mut().insert(id, (size, have));
                        let _ = library.store.update_download(id, size, have, "loading");
                        library.show_downloads();
                    });
                }
            }
            Ok(None) => break,
            Err(err) => return Err((size, have, err.without_url().to_string())),
        }
    }
    file.flush().await.map_err(|err| (size, have, err.to_string()))?;
    drop(file);
    if size > 0 && have < size {
        return Err((size, have, "the connection ended early".to_owned()));
    }
    tokio::fs::rename(&part, &path).await.map_err(|err| (size, have, err.to_string()))?;
    Ok((size.max(have), have, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(kind: &str, season: Option<u32>, episode: Option<u32>, detail: &str) -> Download {
        Download {
            id: 1, kind: kind.into(), title_id: "tt1".into(), video_id: "v".into(),
            title: "Dune: Part Two".into(), detail: detail.into(), season, episode, poster: None,
            url: String::new(), path: String::new(), size: 0, received: 0, state: "waiting".into(), added_at: 0,
        }
    }

    #[test]
    fn films_and_episodes_go_in_their_own_folders() {
        assert_eq!(relative_path(&sample("movie", None, None, ""), "mkv"), PathBuf::from("Movies/Dune Part Two.mkv"));
        assert_eq!(
            relative_path(&sample("series", Some(1), Some(2), "S1 E2 · The Gift / of Men"), "mp4"),
            PathBuf::from("Series/Dune Part Two/Season 1/Dune Part Two - S01E02 - The Gift of Men.mp4")
        );
    }

    #[test]
    fn the_file_type_comes_from_the_address() {
        assert_eq!(extension_of("https://x/a/film.MP4?token=1"), "mp4");
        assert_eq!(extension_of("https://x/stream/123"), "mkv");
        assert_eq!(safe_name("  ..  "), "Untitled");
    }
}
