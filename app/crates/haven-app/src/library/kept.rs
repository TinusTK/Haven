//! What Haven loaded last time, kept on disk so the next start shows it
//! at once: Home's shelves, Explore's first page, the media servers'
//! libraries and titles' details. Each is asked for again in the
//! background and replaced when the fresh answer comes.

use std::path::{Path, PathBuf};

use haven_titles::MetaDetail;
use serde::{Serialize, de::DeserializeOwned};

use super::Library;

/// The file a kept answer lives in, named after what it is.
fn kept_file(dir: &Path, name: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut hasher);
    dir.join(format!("{:016x}.json", hasher.finish()))
}

/// Reads a kept answer; anything missing or unreadable is `None`.
pub(super) fn read_kept<T: DeserializeOwned>(dir: &Path, name: &str) -> Option<T> {
    let text = std::fs::read(kept_file(dir, name)).ok()?;
    serde_json::from_slice(&text).ok()
}

/// Writes a kept answer (through a temporary file, so a half-written
/// one is never read).
pub(super) fn write_kept<T: Serialize>(dir: &Path, name: &str, value: &T) {
    let Ok(text) = serde_json::to_vec(value) else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let file = kept_file(dir, name);
    let part = file.with_extension("part");
    if std::fs::write(&part, text).is_ok() {
        let _ = std::fs::rename(&part, &file);
    }
}

/// The name a title's details are kept under.
pub(super) fn details_name(kind: &str, id: &str) -> String {
    format!("details|{kind}|{id}")
}

impl Library {
    /// Where kept answers live: beside the kept pictures.
    pub(super) fn kept_dir(&self) -> PathBuf {
        self.picture_cache.parent().map_or_else(|| self.picture_cache.join("kept"), |cache| cache.join("kept"))
    }

    pub(super) fn kept<T: DeserializeOwned>(&self, name: &str) -> Option<T> {
        read_kept(&self.kept_dir(), name)
    }

    /// Keeps an answer for the next start, written off the UI thread.
    pub(super) fn keep<T: Serialize + Send + 'static>(&self, name: String, value: T) {
        let dir = self.kept_dir();
        self.tasks.spawn(async move { write_kept(&dir, &name, &value) });
    }

    /// A title's details as last loaded, if kept.
    pub(super) fn kept_details(&self, kind: &str, id: &str) -> Option<MetaDetail> {
        self.kept(&details_name(kind, id))
    }

    /// Forgets everything kept (Settings' Clear, with the pictures).
    pub(super) fn clear_kept(&self) {
        let _ = std::fs::remove_dir_all(self.kept_dir());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_answers_round_trip() {
        let dir = std::env::temp_dir().join(format!("haven-kept-{}", std::process::id()));
        write_kept(&dir, "shelf|Home|Popular", &vec!["a".to_owned(), "b".to_owned()]);
        let back: Option<Vec<String>> = read_kept(&dir, "shelf|Home|Popular");
        assert_eq!(back, Some(vec!["a".to_owned(), "b".to_owned()]));
        let missing: Option<Vec<String>> = read_kept(&dir, "shelf|Home|Other");
        assert_eq!(missing, None);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn details_survive_being_kept() {
        let json = r#"{"id":"tt1","type":"series","name":"Show","releaseInfo":2020,
            "videos":[{"id":"tt1:1:1","name":"Pilot","season":"1","number":1,"released":"2020-01-01"}]}"#;
        let meta: MetaDetail = serde_json::from_str(json).unwrap();
        let dir = std::env::temp_dir().join(format!("haven-kept-d-{}", std::process::id()));
        write_kept(&dir, &details_name("series", "tt1"), &meta);
        let back: Option<MetaDetail> = read_kept(&dir, &details_name("series", "tt1"));
        assert_eq!(back, Some(meta));
        let _ = std::fs::remove_dir_all(dir);
    }
}
