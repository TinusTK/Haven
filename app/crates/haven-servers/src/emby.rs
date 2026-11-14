//! Jellyfin and Emby. Jellyfin grew out of Emby and the two still share
//! the parts of the API used here; they differ in the sign-in header.

use haven_addons::{MetaDetail, MetaPreview, Stream, Video};
use serde_json::{Value, json};

use crate::{Client, Error, Kind, Library, Result, Server, list, number, query, text, title_id};

/// One tick is 100 nanoseconds.
const TICKS_PER_SECOND: f64 = 10_000_000.0;
const LIST_LENGTH: u32 = 300;

fn headers(client: &Client, kind: Kind, token: Option<&str>) -> Vec<(&'static str, String)> {
    let mut value = format!(
        "MediaBrowser Client=\"Haven\", Device=\"Haven\", DeviceId=\"{}\", Version=\"{}\"",
        client.device,
        env!("CARGO_PKG_VERSION"),
    );
    if let Some(token) = token {
        value.push_str(&format!(", Token=\"{token}\""));
    }
    match (kind, token) {
        (Kind::Emby, Some(token)) => {
            vec![("X-Emby-Authorization", value), ("X-Emby-Token", token.to_owned())]
        }
        (Kind::Emby, None) => vec![("X-Emby-Authorization", value)],
        _ => vec![("Authorization", value)],
    }
}

async fn get(client: &Client, server: &Server, path: &str) -> Result<Value> {
    let url = format!("{}{path}", server.address);
    client.call(&url, &headers(client, server.kind, Some(&server.token)), None).await
}

pub async fn sign_in(
    client: &Client,
    kind: Kind,
    address: &str,
    user: &str,
    password: &str,
) -> Result<Server> {
    let answer = client
        .call(
            &format!("{address}/Users/AuthenticateByName"),
            &headers(client, kind, None),
            Some(json!({ "Username": user, "Pw": password })),
        )
        .await?;
    let token = text(&answer, "AccessToken").ok_or(Error::BadAnswer)?;
    let user_id =
        answer.get("User").and_then(|user| text(user, "Id")).ok_or(Error::BadAnswer)?;
    // The server's own name, if it will say; otherwise its address.
    let name = client
        .call(&format!("{address}/System/Info/Public"), &headers(client, kind, None), None)
        .await
        .ok()
        .and_then(|info| text(&info, "ServerName"))
        .unwrap_or_else(|| address.to_owned());
    Ok(Server { id: 0, kind, name, address: address.to_owned(), user_id, token })
}

pub async fn libraries(client: &Client, server: &Server) -> Result<Vec<Library>> {
    let views = get(client, server, &format!("/Users/{}/Views", server.user_id)).await?;
    Ok(read_libraries(&views))
}

pub(crate) fn read_libraries(views: &Value) -> Vec<Library> {
    list(views, "Items")
        .iter()
        .filter(|view| {
            // No type means a mixed library; other types (music, books) are skipped.
            matches!(text(view, "CollectionType").as_deref(), Some("movies" | "tvshows") | None)
        })
        .filter_map(|view| Some(Library { id: text(view, "Id")?, name: text(view, "Name")? }))
        .collect()
}

pub async fn latest(client: &Client, server: &Server, library: &str) -> Result<Vec<MetaPreview>> {
    let path = format!(
        "/Users/{}/Items/Latest?ParentId={}&Limit=30&IncludeItemTypes=Movie,Series,Episode",
        server.user_id,
        query(library)
    );
    let items = get(client, server, &path).await?;
    Ok(read_previews(server, items.as_array().map_or(&[], Vec::as_slice)))
}

pub async fn all(client: &Client, server: &Server, library: &str) -> Result<Vec<MetaPreview>> {
    let path = format!(
        "/Users/{}/Items?ParentId={}&Recursive=true&IncludeItemTypes=Movie,Series\
         &SortBy=SortName&SortOrder=Ascending&Limit={LIST_LENGTH}",
        server.user_id,
        query(library)
    );
    let items = get(client, server, &path).await?;
    Ok(read_previews(server, list(&items, "Items")))
}

pub async fn search(client: &Client, server: &Server, words: &str) -> Result<Vec<MetaPreview>> {
    let path = format!(
        "/Users/{}/Items?SearchTerm={}&Recursive=true&IncludeItemTypes=Movie,Series&Limit=30",
        server.user_id,
        query(words)
    );
    let items = get(client, server, &path).await?;
    Ok(read_previews(server, list(&items, "Items")))
}

fn image(server: &Server, item: &str, which: &str, height: u32) -> String {
    format!("{}/Items/{}/Images/{which}?maxHeight={height}&quality=90", server.address, query(item))
}

/// Films and series as posters. An episode stands for its series; a
/// series shows once however many of its episodes are listed.
pub(crate) fn read_previews(server: &Server, items: &[Value]) -> Vec<MetaPreview> {
    let mut previews: Vec<MetaPreview> = Vec::new();
    for item in items {
        let (kind, id, name) = match text(item, "Type").as_deref() {
            Some("Movie") => ("movie", text(item, "Id"), text(item, "Name")),
            Some("Series") => ("series", text(item, "Id"), text(item, "Name")),
            Some("Episode") => ("series", text(item, "SeriesId"), text(item, "SeriesName")),
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
            poster: Some(image(server, &id, "Primary", 450)),
            release_info: text(item, "ProductionYear"),
        });
    }
    previews
}

pub async fn meta(client: &Client, server: &Server, item: &str) -> Result<MetaDetail> {
    let detail =
        get(client, server, &format!("/Users/{}/Items/{}", server.user_id, query(item))).await?;
    let episodes = if text(&detail, "Type").as_deref() == Some("Series") {
        let path = format!("/Shows/{}/Episodes?UserId={}", query(item), server.user_id);
        get(client, server, &path).await?
    } else {
        Value::Null
    };
    read_detail(server, &detail, &episodes).ok_or(Error::BadAnswer)
}

pub(crate) fn read_detail(server: &Server, detail: &Value, episodes: &Value) -> Option<MetaDetail> {
    let id = text(detail, "Id")?;
    let series = text(detail, "Type").as_deref() == Some("Series");
    let people = |role: &str| -> Vec<String> {
        list(detail, "People")
            .iter()
            .filter(|person| text(person, "Type").as_deref() == Some(role))
            .filter_map(|person| text(person, "Name"))
            .collect()
    };
    let videos = list(episodes, "Items")
        .iter()
        .filter_map(|episode| {
            Some(Video {
                id: title_id(server.id, &text(episode, "Id")?),
                title: text(episode, "Name"),
                season: number(episode, "ParentIndexNumber").map(|n| n as u32),
                episode: number(episode, "IndexNumber").map(|n| n as u32),
                released: text(episode, "PremiereDate"),
            })
        })
        .collect();
    Some(MetaDetail {
        id: title_id(server.id, &id),
        kind: if series { "series" } else { "movie" }.to_owned(),
        name: text(detail, "Name")?,
        poster: Some(image(server, &id, "Primary", 450)),
        background: (!list(detail, "BackdropImageTags").is_empty())
            .then(|| image(server, &id, "Backdrop", 900)),
        description: text(detail, "Overview"),
        release_info: text(detail, "ProductionYear"),
        runtime: number(detail, "RunTimeTicks")
            .map(|ticks| (ticks / TICKS_PER_SECOND / 60.0).round() as u64)
            .filter(|minutes| *minutes > 0)
            .map(|minutes| format!("{minutes} min")),
        rating: number(detail, "CommunityRating").map(|rating| format!("{rating:.1}")),
        genres: list(detail, "Genres").iter().filter_map(|g| g.as_str().map(str::to_owned)).collect(),
        cast: people("Actor"),
        director: people("Director"),
        videos,
    })
}

/// The file as it is on the server. The token goes in the address because
/// the player, not Haven's own HTTP client, opens it.
pub fn streams(server: &Server, item: &str) -> Vec<Stream> {
    vec![Stream {
        name: Some("Direct".to_owned()),
        description: Some(format!("The file as stored on {}", server.name)),
        url: Some(format!(
            "{}/Videos/{}/stream?static=true&api_key={}",
            server.address,
            query(item),
            query(&server.token)
        )),
        ..Stream::default()
    }]
}

pub async fn report(
    client: &Client,
    server: &Server,
    item: &str,
    position: f64,
    finished: bool,
) -> Result<()> {
    let headers = headers(client, server.kind, Some(&server.token));
    let stopped = json!({
        "ItemId": item,
        "PositionTicks": (position.max(0.0) * TICKS_PER_SECOND) as i64,
    });
    client
        .call(&format!("{}/Sessions/Playing/Stopped", server.address), &headers, Some(stopped))
        .await?;
    if finished {
        let url = format!("{}/Users/{}/PlayedItems/{}", server.address, server.user_id, query(item));
        client.call(&url, &headers, Some(json!({}))).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server() -> Server {
        Server {
            id: 2,
            kind: Kind::Jellyfin,
            name: "Home".into(),
            address: "http://media.example".into(),
            user_id: "u1".into(),
            token: "tok en".into(),
        }
    }

    // Made up in the shape these servers answer with.
    #[test]
    fn libraries_keep_films_and_series_only() {
        let views = json!({"Items": [
            {"Id": "a", "Name": "Films", "CollectionType": "movies"},
            {"Id": "b", "Name": "Shows", "CollectionType": "tvshows"},
            {"Id": "c", "Name": "Music", "CollectionType": "music"},
            {"Id": "d", "Name": "Mixed"},
            {"Name": "No id", "CollectionType": "movies"}
        ]});
        let names: Vec<String> = read_libraries(&views).into_iter().map(|l| l.name).collect();
        assert_eq!(names, ["Films", "Shows", "Mixed"]);
    }

    #[test]
    fn previews_fold_episodes_into_their_series() {
        let items = json!([
            {"Type": "Movie", "Id": "m1", "Name": "A Film", "ProductionYear": 2020},
            {"Type": "Episode", "Id": "e1", "Name": "Pilot", "SeriesId": "s1", "SeriesName": "A Show"},
            {"Type": "Episode", "Id": "e2", "Name": "Two", "SeriesId": "s1", "SeriesName": "A Show"},
            {"Type": "Series", "Id": "s2", "Name": "Another Show"},
            {"Type": "Audio", "Id": "x", "Name": "A Song"}
        ]);
        let previews = read_previews(&server(), items.as_array().unwrap());
        let seen: Vec<(&str, &str, &str)> =
            previews.iter().map(|p| (p.id.as_str(), p.kind.as_str(), p.name.as_str())).collect();
        assert_eq!(
            seen,
            [("srv:2:m1", "movie", "A Film"), ("srv:2:s1", "series", "A Show"), ("srv:2:s2", "series", "Another Show")]
        );
        assert_eq!(previews[0].release_info.as_deref(), Some("2020"));
        assert_eq!(
            previews[0].poster.as_deref(),
            Some("http://media.example/Items/m1/Images/Primary?maxHeight=450&quality=90")
        );
    }

    #[test]
    fn detail_of_a_series_lists_its_episodes() {
        let detail = json!({
            "Id": "s1", "Type": "Series", "Name": "A Show", "Overview": "About it.",
            "ProductionYear": 2019, "CommunityRating": 8.25, "Genres": ["Drama", "Crime"],
            "RunTimeTicks": 27000000000u64, "BackdropImageTags": ["x"],
            "People": [{"Name": "An Actor", "Type": "Actor"}, {"Name": "A Director", "Type": "Director"}]
        });
        let episodes = json!({"Items": [
            {"Id": "e1", "Name": "Pilot", "ParentIndexNumber": 1, "IndexNumber": 1, "PremiereDate": "2019-01-01T00:00:00Z"},
            {"Name": "No id"}
        ]});
        let meta = read_detail(&server(), &detail, &episodes).unwrap();
        assert_eq!(meta.id, "srv:2:s1");
        assert_eq!(meta.kind, "series");
        assert_eq!(meta.runtime.as_deref(), Some("45 min"));
        assert_eq!(meta.rating.as_deref(), Some("8.2"));
        assert_eq!(meta.genres, ["Drama", "Crime"]);
        assert_eq!(meta.cast, ["An Actor"]);
        assert_eq!(meta.director, ["A Director"]);
        assert!(meta.background.is_some());
        assert_eq!(meta.videos.len(), 1);
        assert_eq!(meta.videos[0].id, "srv:2:e1");
        assert_eq!((meta.videos[0].season, meta.videos[0].episode), (Some(1), Some(1)));
    }

    #[test]
    fn the_stream_address_carries_the_escaped_token() {
        let streams = streams(&server(), "m1");
        assert_eq!(
            streams[0].url.as_deref(),
            Some("http://media.example/Videos/m1/stream?static=true&api_key=tok%20en")
        );
    }

    #[test]
    fn emby_and_jellyfin_sign_in_headers_differ() {
        let client = Client::new(haven_core::http::client().unwrap(), "dev-1".into());
        let jellyfin = headers(&client, Kind::Jellyfin, Some("t"));
        assert_eq!(jellyfin.len(), 1);
        assert_eq!(jellyfin[0].0, "Authorization");
        assert!(jellyfin[0].1.contains("DeviceId=\"dev-1\"") && jellyfin[0].1.contains("Token=\"t\""));
        let emby = headers(&client, Kind::Emby, Some("t"));
        assert_eq!(emby[0].0, "X-Emby-Authorization");
        assert_eq!(emby[1], ("X-Emby-Token", "t".to_owned()));
    }
}
