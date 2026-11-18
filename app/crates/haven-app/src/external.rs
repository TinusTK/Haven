//! Playing with another program instead of Haven's own player: VLC,
//! mpv or any program the user names (Settings > Player). On Android
//! the video is handed to whichever player app the user picks; on iOS
//! to the player app chosen in Settings (VLC, Infuse or Outplayer).
//!
//! Another program does not tell Haven how far it got, so there is no
//! resuming, no Continue Watching and no next episode with it.

use std::{cell::RefCell, rc::Rc};

use serde::{Deserialize, Serialize};
use slint::ComponentHandle;

use haven_core::storage::Store;

use crate::{AppWindow, library::PlayRequest};

const KEY: &str = "external_player";

/// What plays videos: 0 Haven's own player, 1 VLC, 2 mpv, 3 a program
/// the user names.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Choice {
    #[serde(default)]
    kind: i32,
    /// The program to run when `kind` is 3.
    #[serde(default)]
    program: String,
}

pub struct External {
    ui: slint::Weak<AppWindow>,
    choice: RefCell<Choice>,
    /// Haven's own player works on this computer.
    has_own: bool,
}

/// Where a program is usually found on this kind of computer; the
/// first that exists is used, else the bare name (found on the PATH).
#[cfg_attr(any(target_os = "android", target_os = "ios"), allow(dead_code))]
fn usual(kind: i32) -> Vec<&'static str> {
    match (kind, std::env::consts::OS) {
        (1, "windows") => vec![
            r"C:\Program Files\VideoLAN\VLC\vlc.exe",
            r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe",
            "vlc.exe",
        ],
        (1, "macos") => vec!["/Applications/VLC.app/Contents/MacOS/VLC", "vlc"],
        (1, _) => vec!["vlc"],
        (2, "windows") => vec!["mpv.exe"],
        (2, "macos") => vec![
            "/Applications/mpv.app/Contents/MacOS/mpv",
            "/opt/homebrew/bin/mpv",
            "/usr/local/bin/mpv",
            "mpv",
        ],
        (2, _) => vec!["mpv"],
        _ => Vec::new(),
    }
}

/// The program and what it is told, for a choice and a video.
#[cfg_attr(any(target_os = "android", target_os = "ios"), allow(dead_code))]
fn command(choice: &Choice, address: &str, title: &str) -> Result<(String, Vec<String>), String> {
    // VLC stands in when Haven's own player is chosen but cannot run.
    let kind = if choice.kind == 0 { 1 } else { choice.kind };
    if kind == 3 {
        let program = choice.program.trim();
        if program.is_empty() {
            return Err("name the program to play with in Settings, under Player".to_owned());
        }
        return Ok((program.to_owned(), vec![address.to_owned()]));
    }
    let places = usual(kind);
    let program = places
        .iter()
        .find(|place| std::path::Path::new(place).is_file())
        .or(places.last())
        .ok_or("no such player")?
        .to_string();
    // The address goes last and alone: it is never read as an option
    // (add-on addresses are not to be trusted), and never through a shell.
    let arguments = match kind {
        1 => vec!["--meta-title".to_owned(), title.to_owned(), address.to_owned()],
        _ => vec![format!("--force-media-title={title}"), "--".to_owned(), address.to_owned()],
    };
    Ok((program, arguments))
}

impl External {
    pub fn start(ui: &AppWindow, store: Rc<Store>, has_own: bool) -> Rc<Self> {
        let choice: Choice = store.get(KEY).ok().flatten().unwrap_or_default();
        let external = Rc::new(Self { ui: ui.as_weak(), choice: RefCell::new(choice), has_own });
        ui.set_player_own_works(has_own);
        ui.set_player_choice(external.choice.borrow().kind);
        ui.set_player_program(external.choice.borrow().program.as_str().into());
        let save = {
            let (external, store) = (external.clone(), store.clone());
            move || {
                if let Err(err) = store.put(KEY, &*external.choice.borrow()) {
                    eprintln!("haven: could not save that: {err}");
                }
            }
        };
        ui.on_set_player_choice({
            let (external, save) = (external.clone(), save.clone());
            move |kind| {
                external.choice.borrow_mut().kind = kind.clamp(0, 3);
                save();
            }
        });
        ui.on_set_player_program({
            let external = external.clone();
            move |program| {
                external.choice.borrow_mut().program = program.trim().to_owned();
                save();
            }
        });
        external
    }

    /// Videos go to another program, not Haven's own player.
    pub fn wanted(&self) -> bool {
        !self.has_own || self.choice.borrow().kind != 0
    }

    /// Haven's own player is playing: what was said about another
    /// program no longer holds.
    pub fn clear(&self) {
        if let Some(ui) = self.ui.upgrade() {
            ui.set_play_note("".into());
        }
    }

    /// Hands the video to the other program and says so, or says why not.
    pub fn play(&self, request: &PlayRequest) {
        let label = [request.title.as_str(), request.detail.as_str()]
            .into_iter()
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        let done = self.open(&request.address, &label);
        if let Err(why) = &done {
            haven_core::diag::note(&format!("external player: {why}"));
        }
        if let Some(ui) = self.ui.upgrade() {
            ui.set_play_note(match done {
                Ok(()) => "Opened in your player.".into(),
                Err(why) => format!("Could not open your player: {why}.").into(),
            });
        }
    }

    /// iOS: hands the video to a player app through that app's own
    /// kind of address.
    #[cfg(target_os = "ios")]
    fn open(&self, address: &str, _title: &str) -> Result<(), String> {
        let link = ios_link(self.choice.borrow().kind, address);
        webbrowser::open(&link).map_err(|_| {
            "the player app could not be opened: install VLC, Infuse or Outplayer, and pick it in Settings"
                .to_owned()
        })
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    fn open(&self, address: &str, title: &str) -> Result<(), String> {
        let (program, arguments) = command(&self.choice.borrow(), address, title)?;
        std::process::Command::new(&program)
            .args(arguments)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map(|_| ())
            .map_err(|err| match err.kind() {
                std::io::ErrorKind::NotFound => format!("\"{program}\" was not found"),
                _ => format!("\"{program}\" did not start ({err})"),
            })
    }

    /// Android: asks the system to show the video; it offers the player
    /// apps that are installed (VLC, mpv and so on).
    #[cfg(target_os = "android")]
    fn open(&self, address: &str, title: &str) -> Result<(), String> {
        view(address, Some(title))
    }
}

/// Android: asks the system to show an address. With a `title` it is a
/// video for a player app; without, a page for the web browser.
#[cfg(target_os = "android")]
pub(crate) fn view(address: &str, title: Option<&str>) -> Result<(), String> {
    {
        use jni::objects::{JObject, JValue};
        let app = crate::ANDROID.get().ok_or("Android is not ready")?;
        let failed = |err: jni::errors::Error| format!("Android refused ({err})");
        let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) }.map_err(failed)?;
        let mut env = vm.attach_current_thread().map_err(failed)?;
        let activity = unsafe { JObject::from_raw(app.activity_as_ptr().cast()) };
        let sent = (|| -> Result<(), jni::errors::Error> {
            let text = env.new_string(address)?;
            let uri = env
                .call_static_method(
                    "android/net/Uri",
                    "parse",
                    "(Ljava/lang/String;)Landroid/net/Uri;",
                    &[JValue::Object(&text)],
                )?
                .l()?;
            let action = env.new_string("android.intent.action.VIEW")?;
            let intent =
                env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[JValue::Object(&action)])?;
            match title {
                Some(title) => {
                    let kind = env.new_string("video/*")?;
                    env.call_method(
                        &intent,
                        "setDataAndType",
                        "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;",
                        &[JValue::Object(&uri), JValue::Object(&kind)],
                    )?;
                    let (key, value) = (env.new_string("title")?, env.new_string(title)?);
                    env.call_method(
                        &intent,
                        "putExtra",
                        "(Ljava/lang/String;Ljava/lang/String;)Landroid/content/Intent;",
                        &[JValue::Object(&key), JValue::Object(&value)],
                    )?;
                }
                None => {
                    env.call_method(
                        &intent,
                        "setData",
                        "(Landroid/net/Uri;)Landroid/content/Intent;",
                        &[JValue::Object(&uri)],
                    )?;
                }
            }
            env.call_method(
                &activity,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[JValue::Object(&intent)],
            )?;
            Ok(())
        })();
        // No app that takes it is the usual reason.
        if env.exception_check().unwrap_or(false) {
            let _ = env.exception_clear();
            return Err(if title.is_some() {
                "no app on this device plays videos: install VLC or mpv".to_owned()
            } else {
                "no app on this device opens web pages".to_owned()
            });
        }
        sent.map_err(failed)
    }
}

/// The address that opens a video in an iOS player app: 2 Infuse,
/// 3 Outplayer, anything else VLC.
#[cfg_attr(not(target_os = "ios"), allow(dead_code))]
fn ios_link(kind: i32, address: &str) -> String {
    // The video's own address travels inside another address, so
    // everything but plain letters and digits is spelt out.
    let inside: String = address
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect();
    match kind {
        2 => format!("infuse://x-callback-url/play?url={inside}"),
        3 => format!("outplayer://{address}"),
        _ => format!("vlc-x-callback://x-callback-url/stream?url={inside}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_address_is_the_last_thing_a_player_is_told() {
        let address = "https://example.com/film.mkv?x=1&y=2";
        for kind in [0, 1, 2] {
            let (_, arguments) = command(&Choice { kind, program: String::new() }, address, "A Film").unwrap();
            assert_eq!(arguments.last().map(String::as_str), Some(address));
        }
        let (program, arguments) =
            command(&Choice { kind: 3, program: " /opt/player ".into() }, address, "A Film").unwrap();
        assert_eq!((program.as_str(), arguments), ("/opt/player", vec![address.to_owned()]));
        assert!(command(&Choice { kind: 3, program: "  ".into() }, address, "A Film").is_err());
    }

    #[test]
    fn an_ios_player_gets_the_address_spelt_out() {
        let address = "https://example.com/a film.mkv?x=1&y=2";
        assert_eq!(
            ios_link(1, address),
            "vlc-x-callback://x-callback-url/stream?url=https%3A%2F%2Fexample.com%2Fa%20film.mkv%3Fx%3D1%26y%3D2"
        );
        assert!(ios_link(2, address).starts_with("infuse://x-callback-url/play?url=https%3A%2F%2F"));
        assert_eq!(ios_link(3, address), format!("outplayer://{address}"));
    }
}
