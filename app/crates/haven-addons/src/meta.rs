use serde::Deserialize;

use crate::{Error, Result, lenient};

/// The short form of a title that catalogues list: enough for a poster.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MetaPreview {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    /// Usually a year or a year range; some add-ons send a number.
    #[serde(default, rename = "releaseInfo", deserialize_with = "lenient::text_or_number")]
    pub release_info: Option<String>,
}

/// Everything an add-on knows about one title.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct MetaDetail {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub poster: Option<String>,
    /// A wide picture for the top of the page.
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default, rename = "releaseInfo", deserialize_with = "lenient::text_or_number")]
    pub release_info: Option<String>,
    #[serde(default, deserialize_with = "lenient::text_or_number")]
    pub runtime: Option<String>,
    #[serde(default, rename = "imdbRating", deserialize_with = "lenient::text_or_number")]
    pub rating: Option<String>,
    /// `genres` in the current protocol, `genre` in older add-ons.
    #[serde(default, alias = "genre", deserialize_with = "lenient::texts")]
    pub genres: Vec<String>,
    #[serde(default, deserialize_with = "lenient::texts")]
    pub cast: Vec<String>,
    #[serde(default, deserialize_with = "lenient::texts")]
    pub director: Vec<String>,
    /// Episodes of a series (or the videos of a channel). Empty for films.
    #[serde(default)]
    pub videos: Vec<Video>,
}

/// One episode. Its `id` is what streams are asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Video {
    pub id: String,
    /// `title` in the current protocol, `name` in older add-ons.
    #[serde(default, alias = "name", deserialize_with = "lenient::text_or_number")]
    pub title: Option<String>,
    #[serde(default, deserialize_with = "lenient::number")]
    pub season: Option<u32>,
    /// `episode` in the current protocol, `number` in older add-ons.
    #[serde(default, alias = "number", deserialize_with = "lenient::number")]
    pub episode: Option<u32>,
    /// When it first aired, as an ISO date.
    #[serde(default, deserialize_with = "lenient::text_or_number")]
    pub released: Option<String>,
}

impl MetaDetail {
    /// The seasons that have episodes, in order, specials (season 0) last.
    pub fn seasons(&self) -> Vec<u32> {
        let mut seasons: Vec<u32> = self.videos.iter().filter_map(|video| video.season).collect();
        seasons.sort_unstable_by_key(|&season| (season == 0, season));
        seasons.dedup();
        seasons
    }

    /// The episodes of one season, in episode order.
    pub fn episodes(&self, season: u32) -> Vec<&Video> {
        let mut episodes: Vec<&Video> =
            self.videos.iter().filter(|video| video.season == Some(season)).collect();
        episodes.sort_by_key(|video| video.episode);
        episodes
    }
}

/// Reads a catalogue answer. Entries that are missing an id, type or name
/// are left out instead of failing the whole catalogue.
pub fn parse_catalog(json: &str) -> Result<Vec<MetaPreview>> {
    #[derive(Deserialize)]
    struct Page {
        #[serde(default)]
        metas: Vec<serde_json::Value>,
    }
    let page: Page = serde_json::from_str(json).map_err(Error::BadResponse)?;
    Ok(page
        .metas
        .into_iter()
        .filter_map(|meta| serde_json::from_value(meta).ok())
        .collect())
}

/// Reads a `meta` answer. Episodes with no id are left out.
pub fn parse_meta(json: &str) -> Result<MetaDetail> {
    #[derive(Deserialize)]
    struct Answer {
        #[serde(default)]
        meta: Option<serde_json::Value>,
    }
    let answer: Answer = serde_json::from_str(json).map_err(Error::BadResponse)?;
    let mut meta = match answer.meta {
        Some(serde_json::Value::Object(meta)) if !meta.is_empty() => meta,
        _ => return Err(Error::UnknownTitle),
    };
    // Both spellings together would be a duplicate field: keep the current one.
    if meta.contains_key("genres") {
        meta.remove("genre");
    }
    if let Some(serde_json::Value::Array(videos)) = meta.get_mut("videos") {
        videos.retain_mut(|video| {
            let Some(video) = video.as_object_mut() else { return false };
            if video.contains_key("title") {
                video.remove("name");
            }
            if video.contains_key("episode") {
                video.remove("number");
            }
            video.get("id").is_some_and(|id| id.is_string())
        });
    }
    serde_json::from_value(serde_json::Value::Object(meta)).map_err(Error::BadResponse)
}
