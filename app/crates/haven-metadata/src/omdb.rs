//! OMDb (omdbapi.com): the scores TMDB and Cinemeta do not have (Rotten
//! Tomatoes, Metacritic) and the age rating, for a title with an IMDb id.
//! Needs an OMDb API key.

use serde_json::Value;

use crate::{ANSWER_BYTES, Error};

const API: &str = "https://www.omdbapi.com/";

/// What OMDb knows about a title. Empty fields are not known.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Scores {
    /// "8.4"
    pub imdb: Option<String>,
    /// "94%"
    pub rotten_tomatoes: Option<String>,
    /// "78" (out of 100)
    pub metacritic: Option<String>,
    /// "R", "PG-13", "TV-MA"…
    pub rated: Option<String>,
    /// "173 min"
    pub runtime: Option<String>,
}

fn known(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty() && *text != "N/A")
        .map(str::to_owned)
}

pub(crate) fn read_scores(answer: &Value) -> Option<Scores> {
    if answer.get("Response").and_then(Value::as_str) == Some("False") {
        return None;
    }
    let rating = |source: &str| {
        answer.get("Ratings")?.as_array()?.iter().find_map(|r| {
            (r.get("Source")?.as_str()? == source).then(|| known(r.get("Value"))).flatten()
        })
    };
    Some(Scores {
        imdb: known(answer.get("imdbRating")),
        rotten_tomatoes: rating("Rotten Tomatoes"),
        metacritic: known(answer.get("Metascore")),
        rated: known(answer.get("Rated")).filter(|r| r != "Not Rated" && r != "Unrated"),
        runtime: known(answer.get("Runtime")),
    })
}

/// The scores for the title with IMDb id `imdb`.
pub async fn scores(http: &reqwest::Client, key: &str, imdb: &str) -> Result<Option<Scores>, Error> {
    if !crate::is_imdb_id(imdb) {
        return Ok(None);
    }
    let url = format!("{API}?i={imdb}&apikey={key}");
    let (status, body) = haven_core::http::send(http, &url, &[], None, ANSWER_BYTES).await?;
    match status {
        200..=299 => Ok(read_scores(&serde_json::from_slice(&body).map_err(|_| Error::BadAnswer)?)),
        401 => Err(Error::BadKey),
        other => Err(Error::Status(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scores_are_read_and_unknowns_left_out() {
        let answer = serde_json::json!({
            "Response": "True", "imdbRating": "8.4", "Metascore": "N/A", "Rated": "R", "Runtime": "173 min",
            "Ratings": [{"Source": "Internet Movie Database", "Value": "8.4/10"}, {"Source": "Rotten Tomatoes", "Value": "94%"}]
        });
        let scores = read_scores(&answer).unwrap();
        assert_eq!(scores.rotten_tomatoes.as_deref(), Some("94%"));
        assert_eq!(scores.metacritic, None);
        assert_eq!(scores.rated.as_deref(), Some("R"));
        assert!(read_scores(&serde_json::json!({"Response": "False"})).is_none());
    }
}
