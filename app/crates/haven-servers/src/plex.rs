//! Plex. Signing in goes through plex.tv with a short code the user
//! approves in a web browser; the servers are then asked directly.

use haven_addons::{MetaDetail, MetaPreview, Stream, Video};
use serde_json::{Value, json};

use crate::{
    Candidate, Client, Error, Kind, Library, Result, Server, Wanted, best_match, list, number,
    query, text, title_id,
};

const PLEX_TV: &str = "https://plex.tv/api/v2";

/// A sign-in in progress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlexPin {
    pub id: String,
    /// The code plex.tv shows the user to confirm.
    pub code: String,
    /// The page to open in a web browser.
    pub link: String,
}

fn headers(client: &Client, token: Option<&str>) -> Vec<(&'static str, String)> {
    let mut headers = vec![
        ("X-Plex-Product", "Haven".to_owned()),
        ("X-Plex-Version", env!("CARGO_PKG_VERSION").to_owned()),
        ("X-Plex-Client-Identifier", client.device.clone()),
    ];
    if let Some(token) = token {
        headers.push(("X-Plex-Token", token.to_owned()));
    }
    headers
}

pub async fn pin(client: &Client) -> Result<PlexPin> {
    let answer = client
        .call(&format!("{PLEX_TV}/pins?strong=true"), &headers(client, None), Some(json!({})))
        .await?;
    let (id, code) =
        text(&answer, "id").zip(text(&answer, "code")).ok_or(Error::BadAnswer)?;
    let link = format!(
        "https://app.plex.tv/auth#?clientID={}&code={}&context%5Bdevice%5D%5Bproduct%5D=Haven",
        query(&client.device),
        query(&code)
    );
    Ok(PlexPin { id, code, link })
}

pub async fn token(client: &Client, pin: &PlexPin) -> Result<Option<String>> {
    let url = format!("{PLEX_TV}/pins/{}?code={}", query(&pin.id), query(&pin.code));
    let answer = client.call(&url, &headers(client, None), None).await?;
    Ok(text(&answer, "authToken"))
}

/// A server as plex.tv lists it, before an address has been tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Listed {
    pub name: String,
    pub token: String,
    /// Addresses to try, best first: local, then remote, then relayed.
    pub addresses: Vec<String>,
}

pub(crate) fn read_resources(resources: &Value) -> Vec<Listed> {
    resources
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .filter(|resource| {
            text(resource, "provides").is_some_and(|what| what.split(',').any(|w| w.trim() == "server"))
        })
        .filter_map(|resource| {
            let mut connections: Vec<(u8, String)> = list(resource, "connections")
                .iter()
                .filter_map(|connection| {
                    let flag = |key: &str| connection.get(key).and_then(Value::as_bool) == Some(true);
                    let rank = if flag("relay") { 2 } else if flag("local") { 0 } else { 1 };
                    Some((rank, text(connection, "uri")?.trim_end_matches('/').to_owned()))
                })
                .collect();
            connections.sort_by_key(|(rank, _)| *rank);
            Some(Listed {
                name: text(resource, "name")?,
                token: text(resource, "accessToken")?,
                addresses: connections.into_iter().map(|(_, uri)| uri).collect(),
            })
        })
        .collect()
}

pub async fn servers(client: &Client, account_token: &str) -> Result<Vec<Server>> {
    let url = format!("{PLEX_TV}/resources?includeHttps=1&includeRelay=1");
    let resources = client.call(&url, &headers(client, Some(account_token)), None).await?;
    let listed = read_resources(&resources);
    if listed.is_empty() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for server in listed {
        for address in &server.addresses {
            let probe = format!("{address}/identity");
            if client.call(&probe, &headers(client, Some(&server.token)), None).await.is_ok() {
                found.push(Server {
                    id: 0,
                    kind: Kind::Plex,
                    name: server.name.clone(),
                    address: address.clone(),
                    user_id: String::new(),
                    token: server.token.clone(),
                });
                break;
            }
        }
    }
    if found.is_empty() { Err(Error::Unreachable) } else { Ok(found) }
}

async fn get(client: &Client, server: &Server, path: &str) -> Result<Value> {
    let url = format!("{}{path}", server.address);
    let answer = client.call(&url, &headers(client, Some(&server.token)), None).await?;
    Ok(answer.get("MediaContainer").cloned().unwrap_or(Value::Null))
}

pub async fn libraries(client: &Client, server: &Server) -> Result<Vec<Library>> {
    Ok(read_libraries(&get(client, server, "/library/sections").await?))
}

pub(crate) fn read_libraries(container: &Value) -> Vec<Library> {
    list(container, "Directory")
        .iter()
        .filter(|section| matches!(text(section, "type").as_deref(), Some("movie" | "show")))
        .filter_map(|section| Some(Library { id: text(section, "key")?, name: text(section, "title")? }))
        .collect()
}

pub async fn list_page(
    client: &Client,
    server: &Server,
    path: &str,
    start: u32,
    length: u32,
) -> Result<Vec<MetaPreview>> {
    let join = if path.contains('?') { '&' } else { '?' };
    let path =
        format!("{path}{join}X-Plex-Container-Start={start}&X-Plex-Container-Size={length}");
    Ok(read_previews(server, &get(client, server, &path).await?))
}

pub async fn titles(
    client: &Client,
    server: &Server,
    library: &str,
    latest: bool,
) -> Result<Vec<MetaPreview>> {
    if latest {
        let path = format!("/library/sections/{}/recentlyAdded", query(library));
        list_page(client, server, &path, 0, 30).await
    } else {
        page(client, server, library, 0, crate::LIST_LENGTH).await
    }
}

pub async fn page(
    client: &Client,
    server: &Server,
    library: &str,
    start: u32,
    length: u32,
) -> Result<Vec<MetaPreview>> {
    let path = format!("/library/sections/{}/all?sort=titleSort", query(library));
    list_page(client, server, &path, start, length).await
}

pub async fn search(client: &Client, server: &Server, words: &str) -> Result<Vec<MetaPreview>> {
    list_page(client, server, &format!("/search?query={}", query(words)), 0, 30).await
}

pub async fn online(client: &Client, server: &Server) -> bool {
    let url = format!("{}/identity", server.address);
    client.call(&url, &headers(client, None), None).await.is_ok()
}

/// A title's ids at IMDb and TMDB: from its `Guid` list, or from the
/// single `guid` older libraries carry.
fn provider_ids(item: &Value) -> (Option<String>, Option<String>) {
    let mut guids: Vec<String> = list(item, "Guid").iter().filter_map(|guid| text(guid, "id")).collect();
    guids.extend(text(item, "guid"));
    let id = |marks: &[&str]| {
        guids.iter().find_map(|guid| {
            let rest = marks.iter().find_map(|mark| guid.split_once(mark).map(|(_, rest)| rest))?;
            let id = rest.split(['?', '/']).next().unwrap_or(rest).trim();
            (!id.is_empty()).then(|| id.to_owned())
        })
    };
    (id(&["imdb://"]), id(&["tmdb://", "themoviedb://"]))
}

pub(crate) fn read_candidates(container: &Value) -> Vec<Candidate> {
    list(container, "Metadata")
        .iter()
        .filter_map(|item| {
            let (imdb, tmdb) = provider_ids(item);
            Some(Candidate {
                id: text(item, "ratingKey")?,
                name: text(item, "title")?,
                year: text(item, "year"),
                imdb,
                tmdb,
            })
        })
        .collect()
}

pub(crate) fn read_episode(leaves: &Value, season: u32, number_in_season: u32) -> Option<String> {
    list(leaves, "Metadata")
        .iter()
        .find(|episode| {
            number(episode, "parentIndex") == Some(f64::from(season))
                && number(episode, "index") == Some(f64::from(number_in_season))
        })
        .and_then(|episode| text(episode, "ratingKey"))
}

pub async fn find(client: &Client, server: &Server, wanted: &Wanted) -> Result<Option<String>> {
    if wanted.name.trim().is_empty() {
        return Ok(None);
    }
    // Type 1 is films, 2 is shows.
    let path = format!(
        "/library/all?type={}&title={}&includeGuids=1&X-Plex-Container-Start=0&X-Plex-Container-Size=25",
        if wanted.series { 2 } else { 1 },
        query(wanted.name.trim()),
    );
    let candidates = read_candidates(&get(client, server, &path).await?);
    let Some(found) = best_match(wanted, &candidates) else { return Ok(None) };
    let Some((season, episode)) = wanted.episode else {
        return Ok((!wanted.series).then(|| found.id.clone()));
    };
    let leaves =
        get(client, server, &format!("/library/metadata/{}/allLeaves", query(&found.id))).await?;
    Ok(read_episode(&leaves, season, episode))
}

/// A picture on the server; the token goes in the address.
fn image(server: &Server, path: &str) -> String {
    format!("{}{path}?X-Plex-Token={}", server.address, query(&server.token))
}

/// Films and shows as posters. A season or episode stands for its show;
/// a show appears once.
pub(crate) fn read_previews(server: &Server, container: &Value) -> Vec<MetaPreview> {
    let mut previews: Vec<MetaPreview> = Vec::new();
    for item in list(container, "Metadata") {
        let (kind, id, name, thumb) = match text(item, "type").as_deref() {
            Some("movie") => ("movie", text(item, "ratingKey"), text(item, "title"), text(item, "thumb")),
            Some("show") => ("series", text(item, "ratingKey"), text(item, "title"), text(item, "thumb")),
            Some("season") => (
                "series",
                text(item, "parentRatingKey"),
                text(item, "parentTitle"),
                text(item, "parentThumb"),
            ),
            Some("episode") => (
                "series",
                text(item, "grandparentRatingKey"),
                text(item, "grandparentTitle"),
                text(item, "grandparentThumb"),
            ),
            _ => continue,
        };
        let (Some(id), Some(name)) = (id, name) else { continue };
        let full = title_id(server.id, &id);
        if previews.iter().any(|known| known.id == full) {
            continue;
        }
        previews.push(MetaPreview {
            id: full,
            kind: kind.to_owned(),
            name,
            poster: thumb.map(|thumb| image(server, &thumb)),
            release_info: text(item, "year"),
        });
    }
    previews
}

pub async fn meta(client: &Client, server: &Server, item: &str) -> Result<MetaDetail> {
    let item = query(item);
    let detail = get(client, server, &format!("/library/metadata/{item}")).await?;
    let show = list(&detail, "Metadata")
        .first()
        .is_some_and(|first| text(first, "type").as_deref() == Some("show"));
    let episodes = if show {
        get(client, server, &format!("/library/metadata/{item}/allLeaves")).await?
    } else {
        Value::Null
    };
    read_detail(server, &detail, &episodes).ok_or(Error::BadAnswer)
}

pub(crate) fn read_detail(server: &Server, detail: &Value, episodes: &Value) -> Option<MetaDetail> {
    let item = list(detail, "Metadata").first()?;
    let id = text(item, "ratingKey")?;
    let (imdb_id, tmdb_id) = provider_ids(item);
    let tags = |key: &str| -> Vec<String> {
        list(item, key).iter().filter_map(|entry| text(entry, "tag")).collect()
    };
    let videos = list(episodes, "Metadata")
        .iter()
        .filter_map(|episode| {
            Some(Video {
                id: title_id(server.id, &text(episode, "ratingKey")?),
                title: text(episode, "title"),
                season: number(episode, "parentIndex").map(|n| n as u32),
                episode: number(episode, "index").map(|n| n as u32),
                released: text(episode, "originallyAvailableAt"),
            })
        })
        .collect();
    Some(MetaDetail {
        id: title_id(server.id, &id),
        kind: if text(item, "type").as_deref() == Some("show") { "series" } else { "movie" }.to_owned(),
        name: text(item, "title")?,
        poster: text(item, "thumb").map(|thumb| image(server, &thumb)),
        background: text(item, "art").map(|art| image(server, &art)),
        description: text(item, "summary"),
        release_info: text(item, "year"),
        // Plex counts in milliseconds.
        runtime: number(item, "duration")
            .map(|ms| (ms / 60_000.0).round() as u64)
            .filter(|minutes| *minutes > 0)
            .map(|minutes| format!("{minutes} min")),
        rating: number(item, "rating").or_else(|| number(item, "audienceRating")).map(|r| format!("{r:.1}")),
        genres: tags("Genre"),
        cast: tags("Role"),
        director: tags("Director"),
        videos,
        imdb_id,
        tmdb_id,
    })
}

pub async fn streams(client: &Client, server: &Server, item: &str) -> Result<Vec<Stream>> {
    let detail = get(client, server, &format!("/library/metadata/{}", query(item))).await?;
    Ok(read_streams(server, &detail))
}

/// One stream per version of the file the server holds.
pub(crate) fn read_streams(server: &Server, detail: &Value) -> Vec<Stream> {
    let Some(item) = list(detail, "Metadata").first() else { return Vec::new() };
    list(item, "Media")
        .iter()
        .filter_map(|media| {
            let part = list(media, "Part").first()?;
            let key = text(part, "key")?;
            let quality = text(media, "videoResolution")
                .map(|resolution| match resolution.as_str() {
                    "4k" => "4K".to_owned(),
                    "sd" => "SD".to_owned(),
                    lines => format!("{lines}p"),
                })
                .unwrap_or_else(|| "Direct".to_owned());
            let mut facts: Vec<String> = [text(media, "container"), text(media, "videoCodec"), text(media, "audioCodec")]
                .into_iter()
                .flatten()
                .map(|fact| fact.to_uppercase())
                .collect();
            facts.extend(number(part, "size").and_then(crate::size));
            Some(Stream {
                name: Some(quality),
                description: Some(if facts.is_empty() {
                    format!("The file as stored on {}", server.name)
                } else {
                    facts.join(" · ")
                }),
                url: Some(image(server, &key)),
                ..Stream::default()
            })
        })
        .collect()
}

pub async fn report(
    client: &Client,
    server: &Server,
    item: &str,
    position: f64,
    duration: f64,
    finished: bool,
) -> Result<()> {
    let item = query(item);
    let path = if finished {
        format!("/:/scrobble?identifier=com.plexapp.plugins.library&key={item}")
    } else {
        format!(
            "/:/timeline?ratingKey={item}&key=%2Flibrary%2Fmetadata%2F{item}&state=stopped\
             &time={}&duration={}&identifier=com.plexapp.plugins.library",
            (position.max(0.0) * 1000.0) as i64,
            (duration.max(0.0) * 1000.0) as i64,
        )
    };
    get(client, server, &path).await.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server {
            id: 5,
            kind: Kind::Plex,
            name: "Den".into(),
            address: "https://plex.example:32400".into(),
            user_id: String::new(),
            token: "tk".into(),
        }
    }

    // Made up in the shape Plex answers with.
    #[test]
    fn resources_keep_servers_and_order_their_addresses() {
        let resources = json!([
            {"name": "Den", "provides": "server", "accessToken": "a", "connections": [
                {"uri": "https://relay.example/", "relay": true, "local": false},
                {"uri": "https://remote.example", "relay": false, "local": false},
                {"uri": "http://192.168.1.9:32400", "relay": false, "local": true}
            ]},
            {"name": "Phone", "provides": "client,player", "accessToken": "b", "connections": []},
            {"name": "No token", "provides": "server"}
        ]);
        let listed = read_resources(&resources);
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].addresses,
            ["http://192.168.1.9:32400", "https://remote.example", "https://relay.example"]
        );
    }

    #[test]
    fn libraries_and_previews() {
        let sections = json!({"Directory": [
            {"key": "1", "title": "Films", "type": "movie"},
            {"key": "2", "title": "TV", "type": "show"},
            {"key": "3", "title": "Music", "type": "artist"}
        ]});
        let names: Vec<String> = read_libraries(&sections).into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["Films", "TV"]);

        let recent = json!({"Metadata": [
            {"type": "movie", "ratingKey": "10", "title": "A Film", "year": 2021, "thumb": "/library/metadata/10/thumb/1"},
            {"type": "season", "ratingKey": "21", "parentRatingKey": "20", "parentTitle": "A Show", "parentThumb": "/t/20"},
            {"type": "episode", "ratingKey": "22", "grandparentRatingKey": "20", "grandparentTitle": "A Show"},
            {"type": "track", "ratingKey": "30", "title": "A Song"}
        ]});
        let previews = read_previews(&server(), &recent);
        let seen: Vec<(&str, &str)> = previews.iter().map(|p| (p.id.as_str(), p.name.as_str())).collect();
        assert_eq!(seen, [("srv:5:10", "A Film"), ("srv:5:20", "A Show")]);
        assert_eq!(
            previews[0].poster.as_deref(),
            Some("https://plex.example:32400/library/metadata/10/thumb/1?X-Plex-Token=tk")
        );
    }

    #[test]
    fn detail_and_streams() {
        let detail = json!({"Metadata": [{
            "type": "movie", "ratingKey": "10", "title": "A Film", "summary": "About it.",
            "year": 2021, "duration": 5400000, "rating": 7.44,
            "Genre": [{"tag": "Drama"}], "Role": [{"tag": "An Actor"}], "Director": [{"tag": "A Director"}],
            "Media": [
                {"videoResolution": "1080", "container": "mkv", "videoCodec": "h264", "audioCodec": "aac",
                 "Part": [{"key": "/library/parts/7/file.mkv"}]},
                {"videoResolution": "4k", "Part": [{"key": "/library/parts/8/file.mkv"}]},
                {"videoResolution": "sd", "Part": []}
            ]
        }]});
        let meta = read_detail(&server(), &detail, &Value::Null).unwrap();
        assert_eq!((meta.kind.as_str(), meta.runtime.as_deref(), meta.rating.as_deref()), ("movie", Some("90 min"), Some("7.4")));
        assert_eq!(meta.cast, ["An Actor"]);

        let streams = read_streams(&server(), &detail);
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].name.as_deref(), Some("1080p"));
        assert_eq!(streams[0].description.as_deref(), Some("MKV · H264 · AAC"));
        assert_eq!(streams[1].name.as_deref(), Some("4K"));
        assert_eq!(
            streams[0].url.as_deref(),
            Some("https://plex.example:32400/library/parts/7/file.mkv?X-Plex-Token=tk")
        );
    }

    #[test]
    fn candidates_read_ids_from_both_kinds_of_guid() {
        let found = json!({"Metadata": [
            {"ratingKey": "10", "title": "A Film", "year": 2021,
             "Guid": [{"id": "imdb://tt0000010"}, {"id": "tmdb://77"}]},
            {"ratingKey": "11", "title": "Old Agent", "guid": "com.plexapp.agents.imdb://tt0000011?lang=en"},
            {"ratingKey": "12", "title": "Bare", "guid": "plex://movie/abc"}
        ]});
        let candidates = read_candidates(&found);
        assert_eq!(candidates[0].imdb.as_deref(), Some("tt0000010"));
        assert_eq!(candidates[0].tmdb.as_deref(), Some("77"));
        assert_eq!(candidates[1].imdb.as_deref(), Some("tt0000011"));
        assert_eq!((candidates[2].imdb.as_deref(), candidates[2].tmdb.as_deref()), (None, None));

        let leaves = json!({"Metadata": [
            {"ratingKey": "22", "parentIndex": 1, "index": 1},
            {"ratingKey": "23", "parentIndex": 1, "index": 2}
        ]});
        assert_eq!(read_episode(&leaves, 1, 2).as_deref(), Some("23"));
        assert_eq!(read_episode(&leaves, 2, 2), None);
    }

    #[test]
    fn a_show_lists_its_episodes() {
        let detail = json!({"Metadata": [{"type": "show", "ratingKey": "20", "title": "A Show"}]});
        let leaves = json!({"Metadata": [
            {"ratingKey": "22", "title": "Pilot", "parentIndex": 1, "index": 1, "originallyAvailableAt": "2020-02-02"}
        ]});
        let meta = read_detail(&server(), &detail, &leaves).unwrap();
        assert_eq!(meta.kind, "series");
        assert_eq!(meta.videos[0].id, "srv:5:22");
        assert_eq!(meta.videos[0].released.as_deref(), Some("2020-02-02"));
    }
}
