//! Haven's built-in catalogue: films and series straight from TMDB, in the
//! same shapes add-ons use (posters, details, episodes), so shelves, the
//! Explore page and a title's page treat them like any other title.
//!
//! A TMDB title has the id `tmdb:movie:<number>` or `tmdb:tv:<number>`.
//!
//! This product uses the TMDB API but is not endorsed or certified by TMDB.

use haven_titles::{MetaDetail, MetaPreview, Video};
use serde_json::Value;

use crate::{ANSWER_BYTES, API, CAST_KEPT, Error, IMAGES, read_details};

/// TMDB takes at most this many extra parts in one request.
const SEASONS_AT_ONCE: usize = 20;

/// The ready-made lists TMDB keeps up to date.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum List {
    /// What people are watching this week.
    Trending,
    Popular,
    TopRated,
    /// Films in cinemas now; series with episodes airing now.
    Now,
    /// Films about to come out (films only).
    Upcoming,
}

/// A genre as TMDB numbers it. Films and series have separate lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genre {
    pub id: u64,
    pub name: String,
}

/// A service that carries a title, and the ways it offers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    /// "Subscription", "Free", "Free with ads", "Rent", "Buy".
    pub ways: Vec<&'static str>,
}

/// Where a title can be watched in one country. The data is JustWatch's,
/// passed on by TMDB, and must be credited to JustWatch where shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offers {
    /// The country's two-letter code, upper case.
    pub country: String,
    /// TMDB's page for the title in that country, with links to each service.
    pub link: Option<String>,
    pub services: Vec<Service>,
}

/// The id Haven gives a TMDB title.
pub fn title_id(series: bool, number: u64) -> String {
    format!("tmdb:{}:{number}", if series { "tv" } else { "movie" })
}

/// Splits a `tmdb:movie:<n>` or `tmdb:tv:<n>` id into "is a series" and
/// the number; `None` for any other id.
pub fn split_id(id: &str) -> Option<(bool, u64)> {
    let (kind, number) = id.strip_prefix("tmdb:")?.split_once(':')?;
    let series = match kind {
        "tv" => true,
        "movie" => false,
        _ => return None,
    };
    Some((series, number.parse().ok()?))
}

/// Talks to TMDB with the user's key.
#[derive(Clone)]
pub struct Tmdb {
    http: reqwest::Client,
    key: String,
    api: String,
    images: String,
}

impl Tmdb {
    /// `key` is a TMDB "API key" or the longer "API read access token";
    /// either works.
    pub fn new(http: reqwest::Client, key: &str) -> Self {
        // Both kinds of key are letters, digits and `-_.`; anything else
        // would not be a key.
        let key = key.chars().filter(|c| c.is_ascii_alphanumeric() || "-_.".contains(*c)).collect();
        Self { http, key, api: API.to_owned(), images: IMAGES.to_owned() }
    }

    /// Asks a stand-in for TMDB instead (for checking Haven without a key).
    pub fn at(mut self, api: &str, images: &str) -> Self {
        self.api = api.trim_end_matches('/').to_owned();
        self.images = images.trim_end_matches('/').to_owned();
        self
    }

    async fn get(&self, path: &str) -> Result<Value, Error> {
        let join = if path.contains('?') { '&' } else { '?' };
        // The long token is sent as a header, the short key in the address.
        let (url, headers) = if self.key.contains('.') {
            (format!("{}{path}", self.api), vec![("Authorization", format!("Bearer {}", self.key))])
        } else {
            (format!("{}{path}{join}api_key={}", self.api, self.key), Vec::new())
        };
        let (status, body) =
            haven_core::http::send(&self.http, &url, &headers, None, ANSWER_BYTES).await?;
        match status {
            200..=299 => serde_json::from_slice(&body).map_err(|_| Error::BadAnswer),
            401 => Err(Error::BadKey),
            other => Err(Error::Status(other)),
        }
    }

    /// One page (20 titles) of a ready-made list; pages count from 1.
    pub async fn list(&self, list: List, series: bool, page: u32) -> Result<Vec<MetaPreview>, Error> {
        let kind = if series { "tv" } else { "movie" };
        let path = match (list, series) {
            (List::Trending, _) => format!("/trending/{kind}/week"),
            (List::Popular, _) => format!("/{kind}/popular"),
            (List::TopRated, _) => format!("/{kind}/top_rated"),
            (List::Now, false) => "/movie/now_playing".to_owned(),
            (List::Now, true) => "/tv/on_the_air".to_owned(),
            (List::Upcoming, _) => "/movie/upcoming".to_owned(),
        };
        let found = self.get(&format!("{path}?page={}", page.max(1))).await?;
        Ok(read_page(&found, series, &self.images))
    }

    /// One page of every film or series, most popular first, narrowed to
    /// a genre if one is given.
    pub async fn discover(
        &self,
        series: bool,
        genre: Option<u64>,
        page: u32,
    ) -> Result<Vec<MetaPreview>, Error> {
        let kind = if series { "tv" } else { "movie" };
        let mut path = format!("/discover/{kind}?sort_by=popularity.desc&page={}", page.max(1));
        if let Some(genre) = genre {
            path.push_str(&format!("&with_genres={genre}"));
        }
        Ok(read_page(&self.get(&path).await?, series, &self.images))
    }

    pub async fn genres(&self, series: bool) -> Result<Vec<Genre>, Error> {
        let kind = if series { "tv" } else { "movie" };
        Ok(read_genres(&self.get(&format!("/genre/{kind}/list")).await?))
    }

    /// Films or series whose name matches `words`.
    pub async fn search(&self, series: bool, words: &str) -> Result<Vec<MetaPreview>, Error> {
        let kind = if series { "tv" } else { "movie" };
        let found = self.get(&format!("/search/{kind}?query={}", escaped(words))).await?;
        Ok(read_page(&found, series, &self.images))
    }

    /// The TMDB number of the title with that IMDb id, if TMDB knows it.
    pub async fn number_of(&self, series: bool, imdb_id: &str) -> Result<Option<u64>, Error> {
        if !crate::is_imdb_id(imdb_id) {
            return Ok(None);
        }
        let found = self.get(&format!("/find/{imdb_id}?external_source=imdb_id")).await?;
        Ok(crate::found_id(&found, series))
    }

    /// The services that carry a title, in the first of `countries`
    /// (two-letter codes, most wanted first) that has any. `None` when
    /// none of them has.
    pub async fn offers(
        &self,
        series: bool,
        number: u64,
        countries: &[String],
    ) -> Result<Option<Offers>, Error> {
        let kind = if series { "tv" } else { "movie" };
        let answer = self.get(&format!("/{kind}/{number}/watch/providers")).await?;
        Ok(read_offers(&answer, countries))
    }

    /// Everything about one title; for a series, with all its episodes.
    pub async fn title(&self, series: bool, number: u64) -> Result<MetaDetail, Error> {
        let kind = if series { "tv" } else { "movie" };
        let record = self
            .get(&format!("/{kind}/{number}?append_to_response=credits,external_ids"))
            .await?;
        let mut meta = read_title(&record, series, number, &self.images).ok_or(Error::BadAnswer)?;
        if series {
            let seasons = season_numbers(&record);
            for group in seasons.chunks(SEASONS_AT_ONCE) {
                let parts: Vec<String> = group.iter().map(|season| format!("season/{season}")).collect();
                let path = format!("/tv/{number}?append_to_response={}", parts.join(","));
                // A season that cannot be fetched is left out, not fatal.
                if let Ok(answer) = self.get(&path).await {
                    for season in group {
                        meta.videos.extend(read_season(&answer, &meta, number, *season));
                    }
                }
            }
        }
        Ok(meta)
    }
}

/// Percent-encodes text for use inside a query string.
fn escaped(text: &str) -> String {
    let mut out = String::new();
    for byte in text.trim().bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn text(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().map(str::trim).filter(|text| !text.is_empty()).map(str::to_owned)
}

/// "2019-05-04" to "2019".
fn year(date: Option<String>) -> Option<String> {
    date.and_then(|date| date.get(..4).map(str::to_owned))
}

/// A list answer as posters. Entries with no id or name are left out.
pub(crate) fn read_page(found: &Value, series: bool, images: &str) -> Vec<MetaPreview> {
    found
        .get("results")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|item| {
            Some(MetaPreview {
                id: title_id(series, item.get("id")?.as_u64()?),
                kind: if series { "series" } else { "movie" }.to_owned(),
                name: text(item, "title").or_else(|| text(item, "name"))?,
                poster: text(item, "poster_path").map(|path| format!("{images}/w342{path}")),
                release_info: year(text(item, "release_date").or_else(|| text(item, "first_air_date"))),
                rating: item
                    .get("vote_average")
                    .and_then(Value::as_f64)
                    .filter(|r| *r > 0.0)
                    .map(|r| format!("{r:.1}")),
            })
        })
        .collect()
}

pub(crate) fn read_genres(found: &Value) -> Vec<Genre> {
    found
        .get("genres")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|genre| Some(Genre { id: genre.get("id")?.as_u64()?, name: text(genre, "name")? }))
        .collect()
}

pub(crate) fn read_offers(answer: &Value, countries: &[String]) -> Option<Offers> {
    let results = answer.get("results")?.as_object()?;
    const WAYS: [(&str, &str); 5] = [
        ("flatrate", "Subscription"),
        ("free", "Free"),
        ("ads", "Free with ads"),
        ("rent", "Rent"),
        ("buy", "Buy"),
    ];
    countries.iter().find_map(|country| {
        let country = country.trim().to_uppercase();
        let entry = results.get(&country)?;
        let mut services: Vec<Service> = Vec::new();
        for (key, way) in WAYS {
            let listed = entry.get(key).and_then(Value::as_array).map_or(&[][..], Vec::as_slice);
            for name in listed.iter().filter_map(|provider| text(provider, "provider_name")) {
                match services.iter_mut().find(|service| service.name == name) {
                    Some(service) => service.ways.push(way),
                    None => services.push(Service { name, ways: vec![way] }),
                }
            }
        }
        (!services.is_empty()).then(|| Offers { country, link: text(entry, "link"), services })
    })
}

/// The numbers of a series' seasons that have episodes, specials included.
pub(crate) fn season_numbers(record: &Value) -> Vec<u32> {
    record
        .get("seasons")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|season| season.get("episode_count").and_then(Value::as_u64).unwrap_or(0) > 0)
        .filter_map(|season| season.get("season_number")?.as_u64().map(|n| n as u32))
        .collect()
}

/// A title's own record as a page of details (episodes come separately).
pub(crate) fn read_title(record: &Value, series: bool, number: u64, images: &str) -> Option<MetaDetail> {
    let details = read_details(record);
    let image = |key: &str, size: &str| text(record, key).map(|path| format!("{images}/{size}{path}"));
    let imdb_id = text(record, "imdb_id")
        .or_else(|| record.get("external_ids").and_then(|ids| text(ids, "imdb_id")))
        .filter(|id| crate::is_imdb_id(id));
    Some(MetaDetail {
        id: title_id(series, number),
        kind: if series { "series" } else { "movie" }.to_owned(),
        name: text(record, "title").or_else(|| text(record, "name"))?,
        logo: None,
        poster: image("poster_path", "w500"),
        background: image("backdrop_path", "w1280"),
        description: details.overview,
        release_info: year(text(record, "release_date").or_else(|| text(record, "first_air_date"))),
        released: text(record, "release_date").or_else(|| text(record, "first_air_date")),
        runtime: details.runtime_minutes.map(|minutes| format!("{minutes} min")),
        rating: details.rating.map(|rating| format!("{rating:.1}")),
        genres: details.genres,
        cast: details.cast.into_iter().take(CAST_KEPT).collect(),
        director: details.directors,
        videos: Vec::new(),
        imdb_id,
        tmdb_id: Some(number.to_string()),
    })
}

/// The episodes of one season from an answer that carries `season/<n>`.
/// An episode's id is `<imdb id>:<season>:<episode>`, the form add-ons
/// know, or `tmdb:tv:<number>:<season>:<episode>` without an IMDb id.
pub(crate) fn read_season(answer: &Value, meta: &MetaDetail, number: u64, season: u32) -> Vec<Video> {
    let stem = meta.imdb_id.clone().unwrap_or_else(|| title_id(true, number));
    answer
        .get(format!("season/{season}"))
        .and_then(|season| season.get("episodes"))
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter_map(|episode| {
            let episode_number = episode.get("episode_number")?.as_u64()? as u32;
            Some(Video {
                id: format!("{stem}:{season}:{episode_number}"),
                title: text(episode, "name"),
                season: Some(season),
                episode: Some(episode_number),
                released: text(episode, "air_date"),
                thumbnail: text(episode, "still_path").map(|still| format!("{IMAGES}/w300{still}")),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const IMAGES: &str = "https://img.example/t/p";

    #[test]
    fn ids_round_trip() {
        assert_eq!(title_id(false, 603), "tmdb:movie:603");
        assert_eq!(split_id("tmdb:movie:603"), Some((false, 603)));
        assert_eq!(split_id(&title_id(true, 1396)), Some((true, 1396)));
        for id in ["tt0903747", "tmdb:person:5", "tmdb:tv:x", "tmdb:tv", "srv:1:a"] {
            assert_eq!(split_id(id), None, "{id}");
        }
    }

    // Made up in the shape TMDB answers with.
    #[test]
    fn a_page_becomes_posters() {
        let found = json!({"results": [
            {"id": 11, "title": "A Film", "poster_path": "/a.jpg", "release_date": "2021-03-04"},
            {"id": 12, "name": "A Show", "first_air_date": "2019-01-01"},
            {"id": 13},
            {"title": "No id"}
        ]});
        let films = read_page(&found, false, IMAGES);
        assert_eq!(films.len(), 2);
        assert_eq!((films[0].id.as_str(), films[0].kind.as_str()), ("tmdb:movie:11", "movie"));
        assert_eq!(films[0].poster.as_deref(), Some("https://img.example/t/p/w342/a.jpg"));
        assert_eq!(films[0].release_info.as_deref(), Some("2021"));
        assert_eq!((films[1].name.as_str(), films[1].poster.as_deref()), ("A Show", None));
        assert_eq!(read_page(&found, true, IMAGES)[1].id, "tmdb:tv:12");
        assert!(read_page(&json!({}), false, IMAGES).is_empty());
    }

    #[test]
    fn genres_are_read() {
        let found = json!({"genres": [{"id": 18, "name": "Drama"}, {"id": 35}, {"name": "No id"}]});
        assert_eq!(read_genres(&found), [Genre { id: 18, name: "Drama".into() }]);
    }

    #[test]
    fn a_film_record_becomes_a_page_of_details() {
        let record = json!({
            "title": "A Film", "overview": "About it.", "runtime": 118, "vote_average": 7.84,
            "release_date": "2021-03-04", "imdb_id": "tt0000011",
            "genres": [{"name": "Drama"}], "poster_path": "/p.jpg", "backdrop_path": "/b.jpg",
            "credits": {"cast": [{"name": "An Actor"}], "crew": [{"name": "A Director", "job": "Director"}]}
        });
        let meta = read_title(&record, false, 11, IMAGES).unwrap();
        assert_eq!((meta.id.as_str(), meta.kind.as_str(), meta.name.as_str()), ("tmdb:movie:11", "movie", "A Film"));
        assert_eq!(meta.imdb_id.as_deref(), Some("tt0000011"));
        assert_eq!(meta.tmdb_id.as_deref(), Some("11"));
        assert_eq!(meta.runtime.as_deref(), Some("118 min"));
        assert_eq!(meta.rating.as_deref(), Some("7.8"));
        assert_eq!(meta.release_info.as_deref(), Some("2021"));
        assert_eq!(meta.poster.as_deref(), Some("https://img.example/t/p/w500/p.jpg"));
        assert_eq!(meta.background.as_deref(), Some("https://img.example/t/p/w1280/b.jpg"));
        assert_eq!((meta.cast, meta.director), (vec!["An Actor".to_owned()], vec!["A Director".to_owned()]));
        assert!(read_title(&json!({}), false, 1, IMAGES).is_none());
    }

    #[test]
    fn a_series_lists_its_seasons_and_episodes() {
        let record = json!({
            "name": "A Show", "first_air_date": "2019-01-01",
            "external_ids": {"imdb_id": "tt0000022"},
            "seasons": [
                {"season_number": 0, "episode_count": 2},
                {"season_number": 1, "episode_count": 8},
                {"season_number": 2, "episode_count": 0}
            ]
        });
        assert_eq!(season_numbers(&record), [0, 1]);
        let meta = read_title(&record, true, 22, IMAGES).unwrap();
        assert_eq!(meta.imdb_id.as_deref(), Some("tt0000022"));

        let answer = json!({"season/1": {"episodes": [
            {"episode_number": 1, "name": "Pilot", "air_date": "2019-01-01"},
            {"episode_number": 2},
            {"name": "No number"}
        ]}});
        let episodes = read_season(&answer, &meta, 22, 1);
        assert_eq!(episodes.len(), 2);
        assert_eq!(episodes[0].id, "tt0000022:1:1");
        assert_eq!((episodes[0].season, episodes[0].episode), (Some(1), Some(1)));
        assert_eq!(episodes[0].title.as_deref(), Some("Pilot"));
        assert!(read_season(&answer, &meta, 22, 3).is_empty());

        // Without an IMDb id the episode ids stay Haven's own.
        let bare = read_title(&json!({"name": "Bare"}), true, 23, IMAGES).unwrap();
        assert_eq!(read_season(&answer, &bare, 23, 1)[0].id, "tmdb:tv:23:1:1");
    }

    #[test]
    fn offers_come_from_the_first_country_that_has_any() {
        let answer = json!({"results": {
            "AU": {"link": "https://www.themoviedb.org/movie/11/watch?locale=AU"},
            "US": {"link": "https://www.themoviedb.org/movie/11/watch?locale=US",
                   "flatrate": [{"provider_name": "Service One"}],
                   "rent": [{"provider_name": "Shop Two"}, {"provider_name": "Service One"}],
                   "buy": [{"provider_name": "Shop Two"}]}
        }});
        let wanted = ["au".to_owned(), "US".to_owned()];
        let offers = read_offers(&answer, &wanted).unwrap();
        assert_eq!(offers.country, "US");
        assert_eq!(offers.link.as_deref(), Some("https://www.themoviedb.org/movie/11/watch?locale=US"));
        assert_eq!(
            offers.services,
            [
                Service { name: "Service One".into(), ways: vec!["Subscription", "Rent"] },
                Service { name: "Shop Two".into(), ways: vec!["Rent", "Buy"] },
            ]
        );
        assert_eq!(read_offers(&answer, &["GB".to_owned()]), None);
        assert_eq!(read_offers(&json!({}), &wanted), None);
    }

    #[test]
    fn search_words_are_escaped() {
        assert_eq!(escaped(" the matrix & more "), "the%20matrix%20%26%20more");
    }
}
