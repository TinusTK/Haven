//! Library > Local: videos already on this device.
//!
//! - **Android:** every video on the phone, from Android's own media list.
//!   Android asks the user once for permission to see videos, the first
//!   time the tab is opened.
//! - **iPhone and iPad:** the videos in Files › On My iPhone › Haven › Videos
//!   (Haven makes that folder at start, so people can move videos into it).
//! - **Computers:** the Movies (Videos) and Downloads folders.

use std::{cell::Cell, rc::Rc};
#[cfg(not(target_os = "android"))]
use std::path::PathBuf;

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
    #[cfg_attr(target_os = "android", allow(dead_code))]
    pub modified: i64,
    /// A file path, or on Android a `content://` address.
    pub address: String,
}

#[derive(Default)]
pub(super) struct LocalState {
    found: std::cell::RefCell<Vec<Found>>,
    busy: Cell<bool>,
    /// Android: waiting for the user to answer the permission question.
    asking: Cell<u32>,
    timer: slint::Timer,
}

fn size_words(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    let bytes = bytes as f64;
    if bytes >= 1024.0 * MB { format!("{:.1} GB", bytes / (1024.0 * MB)) } else { format!("{:.0} MB", bytes / MB) }
}

#[cfg_attr(target_os = "android", allow(dead_code))]
fn is_video(path: &std::path::Path) -> bool {
    const KINDS: &[&str] = &["mp4", "m4v", "mkv", "mov", "avi", "webm", "ts", "m2ts", "wmv", "flv", "mpg", "mpeg", "3gp"];
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| KINDS.contains(&e.to_ascii_lowercase().as_str()))
}

/// Walks `root` a few folders deep for videos.
#[cfg_attr(target_os = "android", allow(dead_code))]
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
#[cfg(not(target_os = "android"))]
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
#[cfg(not(target_os = "android"))]
fn find_videos() -> Result<Vec<Found>, String> {
    let mut found = Vec::new();
    for (folder, place) in folders() {
        scan(&folder, &place, &mut found, 3);
    }
    found.sort_by(|a, b| b.modified.cmp(&a.modified));
    Ok(found)
}

#[cfg(target_os = "android")]
mod android {
    use jni::{
        JNIEnv,
        objects::{JObject, JObjectArray, JString, JValue},
    };

    use super::Found;

    fn with_env<T>(work: impl FnOnce(&mut JNIEnv, &JObject) -> Result<T, jni::errors::Error>) -> Result<T, String> {
        let app = crate::ANDROID.get().ok_or("Android is not ready")?;
        let failed = |err: jni::errors::Error| format!("Android refused ({err})");
        // SAFETY: the VM and activity pointers come from the running app.
        let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(failed)?;
        let mut env = vm.attach_current_thread().map_err(failed)?;
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let done = work(&mut env, &activity);
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
        }
        done.map_err(failed)
    }

    /// The permission that lets Haven see the phone's videos.
    fn permission(env: &mut JNIEnv) -> Result<&'static str, jni::errors::Error> {
        let sdk = env.get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?.i()?;
        Ok(if sdk >= 33 { "android.permission.READ_MEDIA_VIDEO" } else { "android.permission.READ_EXTERNAL_STORAGE" })
    }

    /// Whether Haven may see the phone's videos.
    pub fn allowed() -> bool {
        with_env(|env, activity| {
            let wanted = permission(env)?;
            let name = env.new_string(wanted)?;
            let state = env
                .call_method(activity, "checkSelfPermission", "(Ljava/lang/String;)I", &[JValue::Object(&name)])?
                .i()?;
            Ok(state == 0)
        })
        .unwrap_or(false)
    }

    /// Asks the user for it (Android shows its own question).
    pub fn ask() {
        let _ = with_env(|env, activity| {
            let wanted = permission(env)?;
            let name = env.new_string(wanted)?;
            let list: JObjectArray = env.new_object_array(1, "java/lang/String", &name)?;
            env.call_method(activity, "requestPermissions", "([Ljava/lang/String;I)V", &[JValue::Object(&list), JValue::Int(7)])?;
            Ok(())
        });
    }

    /// The phone's videos from Android's media list, newest first.
    pub fn videos() -> Result<Vec<Found>, String> {
        with_env(|env, activity| {
            let resolver =
                env.call_method(activity, "getContentResolver", "()Landroid/content/ContentResolver;", &[])?.l()?;
            let uri = env
                .get_static_field("android/provider/MediaStore$Video$Media", "EXTERNAL_CONTENT_URI", "Landroid/net/Uri;")?
                .l()?;
            let base: JString = env.call_method(&uri, "toString", "()Ljava/lang/String;", &[])?.l()?.into();
            let base: String = env.get_string(&base)?.into();
            let columns = ["_id", "_display_name", "_size", "date_modified", "bucket_display_name"];
            let first = env.new_string(columns[0])?;
            let projection = env.new_object_array(columns.len() as i32, "java/lang/String", &first)?;
            for (at, column) in columns.iter().enumerate().skip(1) {
                let text = env.new_string(column)?;
                env.set_object_array_element(&projection, at as i32, &text)?;
            }
            let order = env.new_string("date_modified DESC")?;
            let null = JObject::null();
            let cursor = env
                .call_method(
                    &resolver,
                    "query",
                    "(Landroid/net/Uri;[Ljava/lang/String;Ljava/lang/String;[Ljava/lang/String;Ljava/lang/String;)Landroid/database/Cursor;",
                    &[JValue::Object(&uri), JValue::Object(&projection), JValue::Object(&null), JValue::Object(&null), JValue::Object(&order)],
                )?
                .l()?;
            let mut found = Vec::new();
            if cursor.is_null() {
                return Ok(found);
            }
            let text = |env: &mut JNIEnv, at: i32| -> Result<String, jni::errors::Error> {
                let value = env.call_method(&cursor, "getString", "(I)Ljava/lang/String;", &[JValue::Int(at)])?.l()?;
                if value.is_null() {
                    return Ok(String::new());
                }
                Ok(env.get_string(&JString::from(value))?.into())
            };
            while found.len() < super::MOST && env.call_method(&cursor, "moveToNext", "()Z", &[])?.z()? {
                let id = env.call_method(&cursor, "getLong", "(I)J", &[JValue::Int(0)])?.j()?;
                let name = text(env, 1)?;
                let size = env.call_method(&cursor, "getLong", "(I)J", &[JValue::Int(2)])?.j()?;
                let modified = env.call_method(&cursor, "getLong", "(I)J", &[JValue::Int(3)])?.j()?;
                let place = text(env, 4)?;
                let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem).to_owned();
                found.push(Found { name: stem, place, size: size.max(0) as u64, modified, address: format!("{base}/{id}") });
            }
            env.call_method(&cursor, "close", "()V", &[])?;
            Ok(found)
        })
    }
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
        ui.set_local_place(
            if cfg!(target_os = "android") {
                "Videos on this phone"
            } else {
                "Videos in your Movies and Downloads folders"
            }
            .into(),
        );
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
        #[cfg(target_os = "android")]
        if !android::allowed() {
            // Android asks; Haven looks again once it is allowed.
            android::ask();
            self.local_status("Allow Haven to see the videos on this phone to list them here.");
            self.local.asking.set(0);
            let library = Rc::downgrade(self);
            self.local.timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(1), move || {
                let Some(library) = library.upgrade() else { return };
                let tries = library.local.asking.get() + 1;
                library.local.asking.set(tries);
                if android::allowed() {
                    library.local.timer.stop();
                    library.open_local();
                } else if tries > 120 {
                    library.local.timer.stop();
                }
            });
            return;
        }
        let _ = &self.local.asking;
        let _ = &self.local.timer;
        self.local.busy.set(true);
        self.local_status("Looking for videos…");
        self.tasks.spawn(async move {
            #[cfg(target_os = "android")]
            let found = tokio::task::spawn_blocking(android::videos).await.unwrap_or_else(|e| Err(e.to_string()));
            #[cfg(not(target_os = "android"))]
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
