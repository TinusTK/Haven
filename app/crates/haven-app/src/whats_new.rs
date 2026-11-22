//! "What's new": the notes of this version, over Haven right after the
//! loading screen. It shows at every start after an update until "Don't
//! show again" is ticked; a fresh install never sees it. The notes are
//! built in (`whats-new.json`), so every build shows the same ones.

use haven_core::storage::Store;
use slint::{ModelRc, VecModel};

use crate::{AppWindow, WhatsNewSection};

/// Which version's notes this device has put away.
const SEEN: &str = "whats_new_seen";
const NOTES: &str = include_str!("../whats-new.json");

struct Notes {
    version: String,
    sections: Vec<(String, Vec<String>)>,
}

fn notes() -> Option<Notes> {
    let value: serde_json::Value = serde_json::from_str(NOTES).ok()?;
    let version = value.get("version")?.as_str()?.to_owned();
    let sections = value
        .get("sections")?
        .as_array()?
        .iter()
        .filter_map(|section| {
            let title = section.get("title")?.as_str()?.to_owned();
            let lines = section
                .get("lines")?
                .as_array()?
                .iter()
                .filter_map(|line| line.as_str().map(str::to_owned))
                .collect();
            Some((title, lines))
        })
        .collect();
    Some(Notes { version, sections })
}

/// Fills the screen's notes. Returns whether it should show at this start.
/// `existing` is false on a fresh install: those notes count as seen.
pub fn prepare(ui: &AppWindow, store: &Store, existing: bool) -> bool {
    let Some(notes) = notes() else { return false };
    let rows: Vec<WhatsNewSection> = notes
        .sections
        .iter()
        .map(|(title, lines)| WhatsNewSection {
            title: title.as_str().into(),
            lines: ModelRc::new(VecModel::from(lines.iter().map(|l| l.as_str().into()).collect::<Vec<slint::SharedString>>())),
        })
        .collect();
    ui.set_whats_new_sections(ModelRc::new(VecModel::from(rows)));
    ui.set_whats_new_version(format!("Haven {}", notes.version).into());
    if cfg!(debug_assertions) && std::env::var_os("HAVEN_WHATS_NEW").is_some() {
        return true;
    }
    let seen: Option<String> = store.get(SEEN).ok().flatten();
    if !existing {
        let _ = store.put(SEEN, &notes.version);
        return false;
    }
    seen.as_deref() != Some(notes.version.as_str())
}

/// Done was pressed: with "Don't show again" ticked, these notes are put away.
pub fn done(store: &Store, dont_show_again: bool) {
    if dont_show_again {
        if let Some(notes) = notes() {
            let _ = store.put(SEEN, &notes.version);
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_built_in_notes_read() {
        let notes = super::notes().expect("whats-new.json is readable");
        assert!(!notes.version.is_empty());
        assert!(!notes.sections.is_empty());
        assert!(notes.sections.iter().all(|(title, lines)| !title.is_empty() && !lines.is_empty()));
        // Store builds must never mention add-ons.
        assert!(!super::NOTES.to_lowercase().contains("add-on"));
    }
}
