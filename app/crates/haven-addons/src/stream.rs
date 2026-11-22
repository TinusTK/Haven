use serde::Deserialize;

use crate::{Error, Result, lenient};

/// One way to watch a title, as offered by an add-on.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
pub struct Stream {
    /// Short label, often the quality ("1080p").
    #[serde(default, deserialize_with = "lenient::text_or_number")]
    pub name: Option<String>,
    /// Longer text; `description` in the current protocol, `title` before.
    #[serde(default, deserialize_with = "lenient::text_or_number")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "lenient::text_or_number")]
    pub description: Option<String>,
    /// A direct address a player can open.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, rename = "externalUrl")]
    pub external_url: Option<String>,
    #[serde(default, rename = "ytId")]
    pub yt_id: Option<String>,
    #[serde(default, rename = "infoHash")]
    pub info_hash: Option<String>,
    /// Hints from the add-on; Haven reads `videoSize` (bytes) from them.
    #[serde(default, rename = "behaviorHints")]
    pub behavior_hints: Option<serde_json::Value>,
    /// Who provided the stream, when whoever hands it on says so (a
    /// media server that gathers streams names the add-on each came
    /// from). Never read from an add-on's own answer.
    #[serde(skip)]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    /// A web address Haven's player can open.
    Direct,
    /// A page to open in a web browser.
    External,
    YouTube,
    Torrent,
}

impl Stream {
    /// How the stream is delivered; `None` if it names no source at all.
    pub fn kind(&self) -> Option<StreamKind> {
        let has = |field: &Option<String>| field.as_deref().is_some_and(|text| !text.trim().is_empty());
        if has(&self.url) {
            Some(StreamKind::Direct)
        } else if has(&self.external_url) {
            Some(StreamKind::External)
        } else if has(&self.yt_id) {
            Some(StreamKind::YouTube)
        } else if has(&self.info_hash) {
            Some(StreamKind::Torrent)
        } else {
            None
        }
    }

    /// The longer text, whichever field the add-on used.
    /// The file's size in bytes, when the add-on gives it as a hint.
    pub fn video_size(&self) -> Option<u64> {
        let size = self.behavior_hints.as_ref()?.get("videoSize")?;
        size.as_u64().or_else(|| size.as_str()?.trim().parse().ok()).filter(|bytes| *bytes > 0)
    }

    pub fn details(&self) -> Option<&str> {
        self.description.as_deref().or(self.title.as_deref())
    }
}

/// Reads a `stream` answer. Entries that are not objects, or that name no
/// source, are left out.
pub fn parse_streams(json: &str) -> Result<Vec<Stream>> {
    #[derive(Deserialize)]
    struct Answer {
        #[serde(default)]
        streams: Vec<serde_json::Value>,
    }
    let answer: Answer = serde_json::from_str(json).map_err(Error::BadResponse)?;
    Ok(answer
        .streams
        .into_iter()
        .filter_map(|stream| serde_json::from_value::<Stream>(stream).ok())
        .filter(|stream| stream.kind().is_some())
        .collect())
}
