//! Haven Hub: Haven without a screen on a Raspberry Pi or a small server.
//! To the app it is one more media server. Signing in is pairing: the
//! Hub's page shows a code, and the key it hands back is the token.
//!
//! An item on the Hub is named `<kind>.<id>`, such as `movie.tt0111161`
//! or, for an episode, `series.tt0903747:1:2`.

use haven_titles::{MetaDetail, MetaPreview, Stream};
use serde_json::{Value, json};

use crate::{Client, Error, Kind, Library, Result, Server, Wanted, title_id};

fn headers(server: &Server) -> Vec<(&'static str, String)> {
    vec![("Authorization", format!("Bearer {}", server.token))]
}

fn query(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn item(kind: &str, id: &str) -> String {
    format!("{kind}.{id}")
}

/// The kind and id an item names.
fn parts(item: &str) -> Result<(&str, &str)> {
    item.split_once('.').filter(|(kind, id)| !kind.is_empty() && !id.is_empty()).ok_or(Error::BadAnswer)
}

async fn get(client: &Client, server: &Server, path: &str) -> Result<Value> {
    client.call(&format!("{}{path}", server.address), &headers(server), None).await
}

/// The Hub's titles, renamed so the app knows they are the Hub's.
fn previews(server: &Server, answer: &Value) -> Result<Vec<MetaPreview>> {
    let mut titles = haven_titles::parse_catalog(&answer.to_string()).map_err(|_| Error::BadAnswer)?;
    for title in &mut titles {
        title.id = title_id(server.id, &item(&title.kind, &title.id));
    }
    Ok(titles)
}

/// Pairs with the Hub at `address` using the code on its page.
pub async fn pair(client: &Client, address: &str, code: &str) -> Result<Server> {
    let hello = client.call(&format!("{address}/api/hello"), &[], None).await?;
    if hello.get("hub").and_then(Value::as_str) != Some("Haven Hub") {
        return Err(Error::BadAnswer);
    }
    let device = format!("Haven on {}", std::env::consts::OS);
    let answer = client
        .call(&format!("{address}/api/pair"), &[], Some(json!({"code": code.trim(), "device": device})))
        .await?;
    let key = answer.get("key").and_then(Value::as_str).ok_or(Error::BadAnswer)?.to_owned();
    let name = answer.get("name").and_then(Value::as_str).unwrap_or("Haven Hub").to_owned();
    Ok(Server { id: 0, kind: Kind::Hub, name, address: address.to_owned(), user_id: String::new(), token: key })
}

pub async fn libraries(client: &Client, server: &Server) -> Result<Vec<Library>> {
    let answer = get(client, server, "/api/libraries").await?;
    Ok(answer
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    Some(Library {
                        id: row.get("id")?.as_str()?.to_owned(),
                        name: row.get("name")?.as_str()?.to_owned(),
                        // A list, not a library: one shelf, no "New in".
                        collection: true,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}

pub async fn genres(client: &Client, server: &Server, library: &str) -> Vec<String> {
    let Ok(answer) = get(client, server, "/api/libraries").await else { return Vec::new() };
    answer
        .as_array()
        .and_then(|rows| rows.iter().find(|row| row.get("id").and_then(Value::as_str) == Some(library)))
        .and_then(|row| row.get("genres")?.as_array().cloned())
        .map(|genres| genres.iter().filter_map(|g| g.as_str().map(str::to_owned)).collect())
        .unwrap_or_default()
}

pub async fn page(
    client: &Client,
    server: &Server,
    library: &str,
    genre: Option<&str>,
    start: u32,
) -> Result<Vec<MetaPreview>> {
    let mut path = format!("/api/catalog?library={}&skip={start}", query(library));
    if let Some(genre) = genre {
        path.push_str(&format!("&genre={}", query(genre)));
    }
    previews(server, &get(client, server, &path).await?)
}

pub async fn search(client: &Client, server: &Server, words: &str) -> Result<Vec<MetaPreview>> {
    previews(server, &get(client, server, &format!("/api/search?q={}", query(words))).await?)
}

pub async fn online(client: &Client, server: &Server) -> bool {
    client.call(&format!("{}/api/hello", server.address), &[], None).await.is_ok()
}

/// The Hub asks its add-ons by IMDb id, so a title with one is found.
pub fn find(wanted: &Wanted) -> Option<String> {
    let imdb = wanted.imdb.as_deref().filter(|id| id.starts_with("tt"))?;
    Some(match (wanted.series, wanted.episode) {
        (true, Some((season, episode))) => item("series", &format!("{imdb}:{season}:{episode}")),
        (true, None) => item("series", imdb),
        (false, _) => item("movie", imdb),
    })
}

pub async fn meta(client: &Client, server: &Server, wanted: &str) -> Result<MetaDetail> {
    let (kind, id) = parts(wanted)?;
    let answer = get(client, server, &format!("/api/meta/{}/{}", query(kind), query(id))).await?;
    let mut meta = haven_titles::parse_meta(&answer.to_string()).map_err(|_| Error::BadAnswer)?;
    if meta.imdb_id.is_none() && id.starts_with("tt") {
        meta.imdb_id = Some(id.to_owned());
    }
    meta.id = title_id(server.id, wanted);
    for video in &mut meta.videos {
        video.id = title_id(server.id, &item(kind, &video.id));
    }
    Ok(meta)
}

pub async fn streams(client: &Client, server: &Server, wanted: &str) -> Result<Vec<Stream>> {
    let (kind, id) = parts(wanted)?;
    let answer = get(client, server, &format!("/api/streams/{}/{}", query(kind), query(id))).await?;
    haven_titles::parse_streams(&answer.to_string()).map_err(|_| Error::BadAnswer)
}

/// Hands the Hub a whole setup to add to its own; how many things were new.
/// Answers how many things were added and removed; removed is None from
/// a Hub too old to match a setup (it only adds).
pub async fn send_setup(
    client: &Client,
    server: &Server,
    setup: &haven_core::hub::Setup,
) -> Result<(u64, Option<u64>)> {
    let body = serde_json::to_value(setup).map_err(|_| Error::BadAnswer)?;
    let answer = client.call(&format!("{}/api/setup", server.address), &headers(server), Some(body)).await?;
    Ok((
        answer.get("added").and_then(Value::as_u64).unwrap_or(0),
        answer.get("removed").and_then(Value::as_u64),
    ))
}

/// The Hub's whole setup.
pub async fn fetch_setup(client: &Client, server: &Server) -> Result<haven_core::hub::Setup> {
    let answer = get(client, server, "/api/setup").await?;
    serde_json::from_value(answer).map_err(|_| Error::BadAnswer)
}

/// The details of the series the Hub follows, as fetched in the
/// background (their own ids, not the Hub's), for a calendar.
pub async fn calendar(client: &Client, server: &Server) -> Result<Vec<MetaDetail>> {
    let answer = get(client, server, "/api/calendar").await?;
    serde_json::from_value(answer.get("series").cloned().unwrap_or(Value::Null)).map_err(|_| Error::BadAnswer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_found_by_imdb_id() {
        let film = Wanted { imdb: Some("tt0111161".into()), ..Wanted::default() };
        assert_eq!(find(&film).as_deref(), Some("movie.tt0111161"));
        let episode =
            Wanted { series: true, imdb: Some("tt0903747".into()), episode: Some((1, 2)), ..Wanted::default() };
        assert_eq!(find(&episode).as_deref(), Some("series.tt0903747:1:2"));
        assert_eq!(find(&Wanted::default()), None);
        assert_eq!(parts("series.tt1:1:2").unwrap(), ("series", "tt1:1:2"));
        assert!(parts("nodot").is_err());
    }

    #[test]
    fn titles_are_renamed_as_the_hubs() {
        let server = Server {
            id: 7,
            kind: Kind::Hub,
            name: "Hub".into(),
            address: "http://192.168.1.2:8590".into(),
            user_id: String::new(),
            token: "k".into(),
        };
        let answer = json!({"metas": [{"id": "tt1", "type": "movie", "name": "A Film"}]});
        let titles = previews(&server, &answer).unwrap();
        assert_eq!(titles[0].id, "srv:7:movie.tt1");
    }
}
