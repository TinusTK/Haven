use serde::{Deserialize, Serialize};

use crate::{Error, Result, lenient};

/// The short form of a title that catalogues list: enough for a poster.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
    /// Out of 10, as text ("8.4"), when the catalogue gives one.
    #[serde(default, rename = "imdbRating", deserialize_with = "lenient::text_or_number", skip_serializing_if = "Option::is_none")]
    pub rating: Option<String>,
}

/// Everything an add-on knows about one title.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
    /// The title's name drawn as a picture (a logo), when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
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
    /// When the title itself first came out, as an ISO date.
    #[serde(default, deserialize_with = "lenient::text_or_number")]
    pub released: Option<String>,
    /// Episodes of a series (or the videos of a channel). Empty for films.
    #[serde(default)]
    pub videos: Vec<Video>,
    /// The title's IMDb id (`tt…`) when its own id is something else.
    #[serde(default, deserialize_with = "lenient::text_or_number")]
    pub imdb_id: Option<String>,
    /// The title's number at TMDB, if known.
    #[serde(default, rename = "moviedb_id", deserialize_with = "lenient::text_or_number")]
    pub tmdb_id: Option<String>,
}

/// One episode. Its `id` is what streams are asked for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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

/// Whether a name reads like a file's name ("Film (2024) [1080p] [WEBRip]")
/// and not like a title.
pub fn is_release_name(name: &str) -> bool {
    name.contains('[') || name.contains('(')
}

/// What two listings of the same film or series have in common: the
/// kind, the name without what follows a bracket and without marks or
/// case, and the year. The year comes from `year` (its first four
/// digits) or, failing that, from a "(2024)" in the name.
pub fn title_key(kind: &str, name: &str, year: Option<&str>) -> String {
    let head = name.split(['(', '[']).next().map(str::trim).filter(|head| !head.is_empty()).unwrap_or(name);
    let plain: String =
        head.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect();
    let four = |text: &str| {
        text.get(..4).filter(|year| year.bytes().all(|b| b.is_ascii_digit())).map(str::to_owned)
    };
    let year = year.and_then(four).or_else(|| {
        name.split(['(', '[']).skip(1).find_map(|part| {
            four(part).filter(|year| year.starts_with("19") || year.starts_with("20"))
        })
    });
    format!("{kind}|{plain}|{}", year.unwrap_or_default())
}

/// Drops repeats of the same film or series from a list, keeping each
/// one's first place. Of two listings, the one named like a title wins
/// over the one named like a file.
pub fn without_repeats(titles: Vec<MetaPreview>) -> Vec<MetaPreview> {
    let mut kept: Vec<(String, MetaPreview)> = Vec::new();
    for title in titles {
        let key = title_key(&title.kind, &title.name, title.release_info.as_deref());
        match kept.iter_mut().find(|(known, _)| *known == key) {
            Some((_, first)) => {
                if is_release_name(&first.name) && !is_release_name(&title.name) {
                    *first = title;
                }
            }
            None => kept.push((key, title)),
        }
    }
    kept.into_iter().map(|(_, title)| title).collect()
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
