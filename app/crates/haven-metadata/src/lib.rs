//! TMDB (themoviedb.org): extra details for titles that carry an IMDb id,
//! and Haven's built-in catalogue of films and series ([`Tmdb`]). Both
//! need a TMDB API key.
//!
//! This product uses the TMDB API but is not endorsed or certified by TMDB.

use serde_json::Value;

mod catalog;
pub mod omdb;

pub use catalog::{Genre, List, Offers, Service, Tmdb, split_id, title_id};

const API: &str = "https://api.themoviedb.org/3";
const IMAGES: &str = "https://image.tmdb.org/t/p";
const ANSWER_BYTES: usize = 4 * 1024 * 1024;
const CAST_KEPT: usize = 10;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] haven_core::http::HttpError),
    #[error("TMDB did not accept the API key")]
    BadKey,
    #[error("TMDB answered with error {0}")]
    Status(u16),
    #[error("TMDB's answer could not be read")]
    BadAnswer,
}

/// What TMDB knows about a title. Empty fields are simply not known.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Details {
    pub overview: Option<String>,
    pub runtime_minutes: Option<u32>,
    /// Out of 10.
    pub rating: Option<f64>,
    pub genres: Vec<String>,
    pub cast: Vec<String>,
    /// A film's directors, or a series' creators.
    pub directors: Vec<String>,
    /// Address of a wide picture.
    pub backdrop: Option<String>,
    /// Address of the poster.
    pub poster: Option<String>,
}

/// Whether an id is an IMDb id (`tt` and digits), the only kind TMDB can
/// be asked about here.
pub fn is_imdb_id(id: &str) -> bool {
    id.strip_prefix("tt").is_some_and(|digits| {
        !digits.is_empty() && digits.len() <= 12 && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

async fn get(http: &reqwest::Client, url: &str) -> Result<Value, Error> {
    let (status, body) = haven_core::http::send(http, url, &[], None, ANSWER_BYTES).await?;
    match status {
        200..=299 => serde_json::from_slice(&body).map_err(|_| Error::BadAnswer),
        401 => Err(Error::BadKey),
        other => Err(Error::Status(other)),
    }
}

/// Looks a title up by its IMDb id. `series` chooses between TMDB's film
/// and television records. `Ok(None)` when TMDB does not know the id.
pub async fn details(
    http: &reqwest::Client,
    key: &str,
    series: bool,
    imdb_id: &str,
) -> Result<Option<Details>, Error> {
    if !is_imdb_id(imdb_id) {
        return Ok(None);
    }
    // The key is letters and digits; anything else would not be a key.
    let key: String = key.chars().filter(char::is_ascii_alphanumeric).collect();
    let found =
        get(http, &format!("{API}/find/{imdb_id}?external_source=imdb_id&api_key={key}")).await?;
    let Some(id) = found_id(&found, series) else { return Ok(None) };
    let kind = if series { "tv" } else { "movie" };
    let record =
        get(http, &format!("{API}/{kind}/{id}?append_to_response=credits&api_key={key}")).await?;
    Ok(Some(read_details(&record)))
}

fn found_id(found: &Value, series: bool) -> Option<u64> {
    let results = if series { "tv_results" } else { "movie_results" };
    found.get(results)?.as_array()?.first()?.get("id")?.as_u64()
}

fn read_details(record: &Value) -> Details {
    let text = |key: &str| {
        record.get(key).and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()).map(str::to_owned)
    };
    let names = |list: Option<&Value>| -> Vec<String> {
        list.and_then(Value::as_array)
            .map(|entries| {
                entries.iter().filter_map(|e| e.get("name")?.as_str().map(str::to_owned)).collect()
            })
            .unwrap_or_default()
    };
    let credits = record.get("credits");
    let mut directors: Vec<String> = credits
        .and_then(|credits| credits.get("crew"))
        .and_then(Value::as_array)
        .map(|crew| {
            crew.iter()
                .filter(|member| member.get("job").and_then(Value::as_str) == Some("Director"))
                .filter_map(|member| member.get("name")?.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    if directors.is_empty() {
        directors = names(record.get("created_by"));
    }
    let image = |key: &str, size: &str| text(key).map(|path| format!("{IMAGES}/{size}{path}"));
    Details {
        overview: text("overview"),
        runtime_minutes: record
            .get("runtime")
            .and_then(Value::as_u64)
            .or_else(|| record.get("episode_run_time")?.as_array()?.first()?.as_u64())
            .filter(|minutes| *minutes > 0)
            .map(|minutes| minutes as u32),
        rating: record.get("vote_average").and_then(Value::as_f64).filter(|rating| *rating > 0.0),
        genres: names(record.get("genres")),
        cast: names(credits.and_then(|credits| credits.get("cast"))).into_iter().take(CAST_KEPT).collect(),
        directors,
        backdrop: image("backdrop_path", "w1280"),
        poster: image("poster_path", "w500"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_imdb_ids_are_looked_up() {
        assert!(is_imdb_id("tt0903747"));
        for id in ["", "tt", "tt12a", "srv:1:abc", "hv1", "tt1234567890123"] {
            assert!(!is_imdb_id(id), "{id}");
        }
    }

    // Made up in the shape TMDB answers with.
    #[test]
    fn finds_the_film_or_the_series_record() {
        let found = json!({"movie_results": [{"id": 11}], "tv_results": [{"id": 22}]});
        assert_eq!(found_id(&found, false), Some(11));
        assert_eq!(found_id(&found, true), Some(22));
        assert_eq!(found_id(&json!({"movie_results": []}), false), None);
    }

    #[test]
    fn reads_a_film() {
        let record = json!({
            "overview": " About it. ", "runtime": 118, "vote_average": 7.8,
            "genres": [{"name": "Drama"}, {"name": "Crime"}],
            "backdrop_path": "/b.jpg", "poster_path": "/p.jpg",
            "credits": {
                "cast": [{"name": "An Actor"}, {"name": "Another"}],
                "crew": [{"name": "A Writer", "job": "Writer"}, {"name": "A Director", "job": "Director"}]
            }
        });
        let details = read_details(&record);
        assert_eq!(details.overview.as_deref(), Some("About it."));
        assert_eq!(details.runtime_minutes, Some(118));
        assert_eq!(details.genres, ["Drama", "Crime"]);
        assert_eq!(details.cast, ["An Actor", "Another"]);
        assert_eq!(details.directors, ["A Director"]);
        assert_eq!(details.backdrop.as_deref(), Some("https://image.tmdb.org/t/p/w1280/b.jpg"));
        assert_eq!(details.poster.as_deref(), Some("https://image.tmdb.org/t/p/w500/p.jpg"));
    }

    #[test]
    fn reads_a_series_and_copes_with_gaps() {
        let record = json!({
            "episode_run_time": [47], "vote_average": 0.0,
            "created_by": [{"name": "A Creator"}], "overview": ""
        });
        let details = read_details(&record);
        assert_eq!(details.runtime_minutes, Some(47));
        assert_eq!(details.rating, None);
        assert_eq!(details.overview, None);
        assert_eq!(details.directors, ["A Creator"]);
        assert_eq!(read_details(&json!({})), Details::default());
    }
}
