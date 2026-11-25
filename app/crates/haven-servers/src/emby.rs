//! Jellyfin and Emby. Jellyfin grew out of Emby and the two still share
//! the parts of the API used here; they differ in the sign-in header.

use haven_titles::{MetaDetail, MetaPreview, Stream, Video};
use serde_json::{Value, json};

use crate::{
    Candidate, Client, Error, Kind, Library, Result, Server, Wanted, best_match, list,
    number, quality, query, size, text, title_id,
};

/// Collections listed per server (each becomes a shelf).
const COLLECTIONS: u32 = 60;

/// One tick is 100 nanoseconds.
const TICKS_PER_SECOND: f64 = 10_000_000.0;

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
    let mut libraries = read_libraries(&views);
    // The collections view holds lists, not titles: each list is offered
    // as a library of its own. On a server that gathers add-ons these
    // are the add-ons' catalogues.
    for view in collection_views(&views) {
        let path = format!(
            "/Users/{}/Items?ParentId={}&StartIndex=0&Limit={COLLECTIONS}",
            server.user_id,
            query(&view)
        );
        if let Ok(found) = get(client, server, &path).await {
            libraries.extend(read_collections(&found));
        }
    }
    Ok(libraries)
}

fn collection_views(views: &Value) -> Vec<String> {
    list(views, "Items")
        .iter()
        .filter(|view| text(view, "CollectionType").as_deref() == Some("boxsets"))
        .filter_map(|view| text(view, "Id"))
        .collect()
}

pub(crate) fn read_collections(found: &Value) -> Vec<Library> {
    list(found, "Items")
        .iter()
        .filter(|item| text(item, "Type").as_deref() == Some("BoxSet"))
        .filter_map(|item| {
            Some(Library { id: text(item, "Id")?, name: text(item, "Name")?, collection: true })
        })
        .collect()
}

pub async fn genres(client: &Client, server: &Server, library: &str) -> Vec<String> {
    let path = format!("/Items/Filters?UserId={}&ParentId={}", server.user_id, query(library));
    match get(client, server, &path).await {
        Ok(filters) => list(&filters, "Genres")
            .iter()
            .filter_map(|genre| genre.as_str().map(str::trim).filter(|g| !g.is_empty()).map(str::to_owned))
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Trellis tells its owner which add-ons it uses, at `/api/config`.
/// Any other server has no such page, which reads as no add-ons.
#[cfg(feature = "shared-addons")]
pub async fn shared_addons(client: &Client, server: &Server) -> Result<Vec<crate::SharedAddon>> {
    match get(client, server, "/api/config").await {
        Ok(config) => Ok(read_shared_addons(&config)),
        Err(Error::Status(_) | Error::BadAnswer) => Ok(Vec::new()),
        Err(err) => Err(err),
    }
}

#[cfg(feature = "shared-addons")]
pub(crate) fn read_shared_addons(config: &Value) -> Vec<crate::SharedAddon> {
    list(config, "addons")
        .iter()
        .filter(|addon| addon.get("disabled").and_then(Value::as_bool) != Some(true))
        .filter_map(|addon| {
            let manifest_url = text(addon, "manifestUrl")
                .filter(|url| url.starts_with("http://") || url.starts_with("https://"))?;
            Some(crate::SharedAddon {
                name: text(addon, "nameOverride").or_else(|| text(addon, "name"))?,
                manifest_url,
            })
        })
        .collect()
}

pub(crate) fn read_libraries(views: &Value) -> Vec<Library> {
    list(views, "Items")
        .iter()
        .filter(|view| {
            // No type means a mixed library; other types (music, books) are skipped.
            matches!(text(view, "CollectionType").as_deref(), Some("movies" | "tvshows") | None)
        })
        .filter_map(|view| {
            Some(Library { id: text(view, "Id")?, name: text(view, "Name")?, collection: false })
        })
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

pub async fn page(
    client: &Client,
    server: &Server,
    library: &str,
    genre: Option<&str>,
    start: u32,
    length: u32,
) -> Result<Vec<MetaPreview>> {
    let mut path = format!(
        "/Users/{}/Items?ParentId={}&Recursive=true&IncludeItemTypes=Movie,Series\
         &SortBy=SortName&SortOrder=Ascending&StartIndex={start}&Limit={length}",
        server.user_id,
        query(library)
    );
    if let Some(genre) = genre {
        path.push_str(&format!("&Genres={}", query(genre)));
    }
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

pub async fn online(client: &Client, server: &Server) -> bool {
    let url = format!("{}/System/Info/Public", server.address);
    client.call(&url, &headers(client, server.kind, None), None).await.is_ok()
}

/// A title's ids at IMDb and TMDB, as the server recorded them.
fn provider_ids(item: &Value) -> (Option<String>, Option<String>) {
    let ids = item.get("ProviderIds");
    // The keys' case differs between servers and versions.
    let id = |name: &str| {
        ids.and_then(Value::as_object)?
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .and_then(|(_, value)| value.as_str())
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
    };
    (id("Imdb"), id("Tmdb"))
}

pub(crate) fn read_candidates(items: &Value) -> Vec<Candidate> {
    list(items, "Items")
        .iter()
        .filter_map(|item| {
            let (imdb, tmdb) = provider_ids(item);
            Some(Candidate {
                id: text(item, "Id")?,
                name: text(item, "Name")?,
                year: text(item, "ProductionYear"),
                imdb,
                tmdb,
            })
        })
        .collect()
}

/// The id of the episode with that season and number, from a series'
/// episode list.
pub(crate) fn read_episode(episodes: &Value, season: u32, number_in_season: u32) -> Option<String> {
    list(episodes, "Items")
        .iter()
        .find(|episode| {
            number(episode, "ParentIndexNumber") == Some(f64::from(season))
                && number(episode, "IndexNumber") == Some(f64::from(number_in_season))
        })
        .and_then(|episode| text(episode, "Id"))
}

pub async fn find(client: &Client, server: &Server, wanted: &Wanted) -> Result<Option<String>> {
    if wanted.name.trim().is_empty() {
        return Ok(None);
    }
    let path = format!(
        "/Users/{}/Items?SearchTerm={}&Recursive=true&IncludeItemTypes={}&Fields=ProviderIds&Limit=25",
        server.user_id,
        query(wanted.name.trim()),
        if wanted.series { "Series" } else { "Movie" },
    );
    let candidates = read_candidates(&get(client, server, &path).await?);
    let Some(found) = best_match(wanted, &candidates) else { return Ok(None) };
    // A gathered title listed under another number (TMDB, say) is asked
    // for under its IMDb id, which is what finds its streams.
    let found = wanted
        .imdb
        .as_deref()
        .and_then(|imdb| crate::by_imdb(&found.id, imdb))
        .unwrap_or_else(|| found.id.clone());
    let Some((season, episode)) = wanted.episode else {
        return Ok((!wanted.series).then_some(found));
    };
    let path = format!("/Shows/{}/Episodes?UserId={}", query(&found), server.user_id);
    Ok(read_episode(&get(client, server, &path).await?, season, episode))
}

/// The address of one of an item's pictures. `tag` is the picture's
/// version as the server names it, when known. The token is included
/// because some servers that speak this API only hand pictures to a
/// signed-in app.
fn image(server: &Server, item: &str, which: &str, height: u32, tag: Option<&str>) -> String {
    let mut address = format!(
        "{}/Items/{}/Images/{which}?maxHeight={height}&quality=90",
        server.address,
        query(item)
    );
    if let Some(tag) = tag {
        address.push_str(&format!("&tag={}", query(tag)));
    }
    address.push_str(&format!("&api_key={}", query(&server.token)));
    address
}

/// The version tag of an item's poster, if it says it has one.
fn poster_tag(item: &Value) -> Option<String> {
    item.get("ImageTags").and_then(|tags| text(tags, "Primary"))
}

/// A picture the item names by its full web address, as servers that
/// gather titles from elsewhere sometimes do instead of holding it.
fn remote_image(item: &Value, kind: &str) -> Option<String> {
    list(item, "RemoteImages")
        .iter()
        .find(|image| text(image, "Type").as_deref() == Some(kind))
        .and_then(|image| text(image, "Url"))
        .filter(|url| url.starts_with("http://") || url.starts_with("https://"))
}

/// Films and series as posters. An episode stands for its series; a
/// series shows once however many of its episodes are listed.
pub(crate) fn read_previews(server: &Server, items: &[Value]) -> Vec<MetaPreview> {
    let mut previews: Vec<MetaPreview> = Vec::new();
    for item in items {
        let (kind, id, name, tag) = match text(item, "Type").as_deref() {
            Some("Movie") => ("movie", text(item, "Id"), text(item, "Name"), poster_tag(item)),
            Some("Series") => ("series", text(item, "Id"), text(item, "Name"), poster_tag(item)),
            Some("Episode") => (
                "series",
                text(item, "SeriesId"),
                text(item, "SeriesName"),
                text(item, "SeriesPrimaryImageTag"),
            ),
            _ => continue,
        };
        let (Some(id), Some(name)) = (id, name) else { continue };
        if is_error_entry(&id) {
            continue;
        }
        let full = title_id(server.id, &id);
        if previews.iter().any(|known| known.id == full) {
            continue;
        }
        previews.push(MetaPreview {
            id: full,
            kind: kind.to_owned(),
            name,
            poster: Some(image(server, &id, "Primary", 450, tag.as_deref())),
            release_info: text(item, "ProductionYear"),
            rating: number(item, "CommunityRating").filter(|r| *r > 0.0).map(|r| format!("{r:.1}")),
        });
    }
    previews
}

/// A server that gathers add-on catalogues passes on an add-on's error
/// (such as "Rate Limit Exceeded") as if it were a film. Its id spells
/// `<kind>|<add-on id>|<type>` in hex, and the add-on id says it is an error.
pub(crate) fn is_error_entry(id: &str) -> bool {
    if id.len() % 2 != 0 || !id.is_ascii() {
        return false;
    }
    let bytes: Option<Vec<u8>> =
        (0..id.len()).step_by(2).map(|at| u8::from_str_radix(&id[at..at + 2], 16).ok()).collect();
    bytes
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|spelt| spelt.split('|').nth(1).map(|addon_id| addon_id.contains("streamserror.")))
        .unwrap_or(false)
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
    let (imdb_id, tmdb_id) = provider_ids(detail);
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
                // The episode's own still, if the server has one.
                thumbnail: remote_image(episode, "Primary").or_else(|| {
                    let tag = poster_tag(episode)?;
                    Some(image(server, &text(episode, "Id")?, "Primary", 360, Some(&tag)))
                }),
            })
        })
        .collect();
    Some(MetaDetail {
        id: title_id(server.id, &id),
        kind: if series { "series" } else { "movie" }.to_owned(),
        name: text(detail, "Name")?,
        logo: None,
        poster: remote_image(detail, "Primary")
            .or_else(|| Some(image(server, &id, "Primary", 450, poster_tag(detail).as_deref()))),
        // Asked for even when the item lists no backdrop: a server that
        // gathers its titles from elsewhere may still have one to give.
        background: remote_image(detail, "Backdrop").or_else(|| {
            let tag = list(detail, "BackdropImageTags").first().and_then(|tag| tag.as_str());
            Some(image(server, &id, "Backdrop", 900, tag))
        }),
        description: text(detail, "Overview"),
        release_info: text(detail, "ProductionYear"),
        released: text(detail, "PremiereDate"),
        runtime: number(detail, "RunTimeTicks")
            .map(|ticks| (ticks / TICKS_PER_SECOND / 60.0).round() as u64)
            .filter(|minutes| *minutes > 0)
            .map(|minutes| format!("{minutes} min")),
        rating: number(detail, "CommunityRating").map(|rating| format!("{rating:.1}")),
        genres: list(detail, "Genres").iter().filter_map(|g| g.as_str().map(str::to_owned)).collect(),
        cast: people("Actor"),
        director: people("Director"),
        videos,
        imdb_id,
        tmdb_id,
    })
}

fn file_address(server: &Server, item: &str, version: Option<&str>) -> String {
    let mut address =
        format!("{}/Videos/{}/stream?static=true", server.address, query(item));
    if let Some(version) = version {
        address.push_str(&format!("&MediaSourceId={}", query(version)));
    }
    address.push_str(&format!("&api_key={}", query(&server.token)));
    address
}

/// Sources handed on at a time while a long list is still arriving.
const BATCH: usize = 10;
/// The most sources read from one answer.
const MOST_SOURCES: usize = 3000;
/// How long a list of sources may take in all, and between two parts.
const LIST_WAIT: std::time::Duration = std::time::Duration::from_secs(900);
const PART_WAIT: std::time::Duration = std::time::Duration::from_secs(45);
/// How often an empty list of sources is asked for again, and how
/// long is waited before each.
const EMPTY_RETRIES: u32 = 2;
const EMPTY_WAIT: std::time::Duration = std::time::Duration::from_secs(5);

/// Every way the server offers to play the film or episode: each version
/// of the file it holds, and, on servers that gather streams from
/// elsewhere, each of those streams. The token goes in the address
/// because the player, not Haven's own HTTP client, opens it.
pub async fn streams(client: &Client, server: &Server, item: &str) -> Result<Vec<Stream>> {
    let mut all = Vec::new();
    streams_live(client, server, item, |some| {
        all.extend(some);
        true
    })
    .await?;
    Ok(all)
}

/// As [`streams`], but hands the sources to `found` a few at a time as
/// the server's answer arrives. A server that gathers streams can list
/// hundreds in an answer of many megabytes; waiting for the whole of it
/// would show nothing for minutes on a slow line. `found` answers
/// whether more are wanted; on no, the rest is not read.
pub async fn streams_live(
    client: &Client,
    server: &Server,
    item: &str,
    mut found: impl FnMut(Vec<Stream>) -> bool + Send,
) -> Result<()> {
    // The playback information is what the server's own apps list their
    // sources from; some servers only work them out when asked this way.
    // The request is shaped like those apps': a profile that says this
    // player takes any file as it is.
    let url = format!(
        "{}/Items/{}/PlaybackInfo?UserId={}&StartTimeTicks=0&IsPlayback=false&AutoOpenLiveStream=false&MaxStreamingBitrate=1000000000",
        server.address,
        query(item),
        query(&server.user_id)
    );
    let body = json!({
        "UserId": server.user_id,
        "StartTimeTicks": 0,
        "IsPlayback": false,
        "AutoOpenLiveStream": false,
        "MaxStreamingBitrate": 1_000_000_000,
        "EnableDirectPlay": true,
        "EnableDirectStream": true,
        "EnableTranscoding": false,
        "DeviceProfile": {
            "Name": "Haven",
            "MaxStreamingBitrate": 1_000_000_000,
            "MaxStaticBitrate": 1_000_000_000,
            "DirectPlayProfiles": [{ "Type": "Video" }, { "Type": "Audio" }],
            "TranscodingProfiles": [],
            "ContainerProfiles": [],
            "CodecProfiles": [],
            "SubtitleProfiles": []
        }
    });
    let mut listed = 0usize;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut asked: Result<()> = Ok(());
    // A server that gathers streams from elsewhere answers with what it
    // has so far, which can be nothing; asked again a little later it
    // has more. So an empty list is asked for again, twice at most.
    for attempt in 0..=EMPTY_RETRIES {
        if attempt > 0 {
            tokio::time::sleep(EMPTY_WAIT).await;
        }
        asked = async {
        let mut request = client
            .http
            .post(haven_core::http::web_url(&url).map_err(|_| Error::InvalidAddress(url.clone()))?)
            .timeout(LIST_WAIT)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .body(body.to_string());
        for (name, value) in headers(client, server.kind, Some(&server.token)) {
            request = request.header(name, value);
        }
        let mut response = request.send().await.map_err(reqwest::Error::without_url)?;
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err(Error::SignInRefused),
            other => return Err(Error::Status(other)),
        }
        let mut scanner = SourceScanner::default();
        let mut waiting: Vec<Value> = Vec::new();
        loop {
            let part = match tokio::time::timeout(PART_WAIT, response.chunk()).await {
                Ok(part) => part.map_err(reqwest::Error::without_url)?,
                Err(_) => break,
            };
            let Some(part) = part else { break };
            for source in scanner.feed(&part) {
                if text(&source, "Id").is_some_and(|id| !seen.insert(id)) {
                    continue;
                }
                listed += 1;
                waiting.push(source);
            }
            if waiting.len() >= BATCH {
                let wanted = found(read_sources(server, item, &waiting, listed > 1));
                waiting.clear();
                if !wanted {
                    break;
                }
            }
            if scanner.done || listed >= MOST_SOURCES {
                break;
            }
        }
        if !waiting.is_empty() {
            let _ = found(read_sources(server, item, &waiting, listed > 1));
        }
        Ok(())
        }
        .await;
        haven_core::diag::note(&format!(
            "{} sources: playback information {}",
            server.name,
            match &asked {
                Ok(()) => format!("lists {listed}"),
                Err(err) => format!("lists {listed}, then failed ({err})"),
            }
        ));
        if listed > 0 || asked.is_err() {
            break;
        }
    }
    if listed > 0 {
        return Ok(());
    }
    // Older servers, or one that listed nothing: the item itself may
    // list its files.
    let detail = get(
        client,
        server,
        &format!("/Users/{}/Items/{}?Fields=MediaSources,MediaStreams", server.user_id, query(item)),
    )
    .await;
    match (asked, detail) {
        (_, Ok(detail)) => {
            haven_core::diag::note(&format!(
                "{} sources: the item {}",
                server.name,
                describe_sources(&detail)
            ));
            // An item that says outright it has no sources has none
            // (a gathering server found nothing): only a server that
            // does not speak of sources at all gets its file offered.
            if detail.get("MediaSources").is_some() && list(&detail, "MediaSources").is_empty() {
                return Ok(());
            }
            let _ = found(read_streams(server, item, &detail));
            Ok(())
        }
        (Err(_), Err(err)) => Err(err),
        (Ok(()), Err(_)) => {
            let _ = found(read_streams(server, item, &json!({})));
            Ok(())
        }
    }
}

/// Picks the entries of `MediaSources` out of an answer while it is
/// still arriving, each as soon as its last byte is in.
#[derive(Default)]
pub(crate) struct SourceScanner {
    depth: usize,
    in_string: bool,
    escape: bool,
    /// The last string seen at the top level: the key before a value.
    key: Vec<u8>,
    in_list: bool,
    /// The entry being gathered, if inside one.
    entry: Option<Vec<u8>>,
    /// The list has ended.
    pub(crate) done: bool,
}

impl SourceScanner {
    /// The most one entry may take (one with more is skipped).
    const ENTRY_BYTES: usize = 4 * 1024 * 1024;

    pub(crate) fn feed(&mut self, part: &[u8]) -> Vec<Value> {
        let mut entries = Vec::new();
        for &byte in part {
            if self.done {
                break;
            }
            if let Some(entry) = &mut self.entry {
                if entry.len() < Self::ENTRY_BYTES {
                    entry.push(byte);
                }
            }
            let top = self.depth == 1 && !self.in_list;
            if self.in_string {
                if self.escape {
                    self.escape = false;
                } else if byte == b'\\' {
                    self.escape = true;
                } else if byte == b'"' {
                    self.in_string = false;
                } else if top {
                    self.key.push(byte);
                }
                continue;
            }
            match byte {
                b'"' => {
                    self.in_string = true;
                    if top {
                        self.key.clear();
                    }
                }
                b'{' | b'[' => {
                    if byte == b'[' && top && self.key == b"MediaSources" {
                        self.in_list = true;
                    } else if byte == b'{' && self.in_list && self.depth == 2 {
                        self.entry = Some(vec![byte]);
                    }
                    self.depth += 1;
                }
                b'}' | b']' => {
                    self.depth = self.depth.saturating_sub(1);
                    if self.in_list && self.depth == 2 {
                        if let Some(entry) = self.entry.take() {
                            entries.extend(serde_json::from_slice::<Value>(&entry).ok());
                        }
                    } else if self.in_list && self.depth == 1 {
                        self.in_list = false;
                        self.done = true;
                    }
                }
                _ => {}
            }
        }
        entries
    }
}

/// What an answer's `MediaSources` look like, for the log: how many,
/// and for each which fields it has. No ids, names or addresses.
pub(crate) fn describe_sources(answer: &Value) -> String {
    let sources = list(answer, "MediaSources");
    let each: Vec<String> = sources
        .iter()
        .take(5)
        .map(|source| {
            let has = |key: &str| if text(source, key).is_some() { "yes" } else { "no" };
            let web_path = text(source, "Path")
                .is_some_and(|path| path.starts_with("http://") || path.starts_with("https://"));
            format!(
                "[protocol {} id {} name {} path {} url {} streams {}]",
                text(source, "Protocol").unwrap_or_else(|| "none".to_owned()),
                has("Id"),
                has("Name"),
                if web_path { "web" } else { has("Path") },
                has("DirectStreamUrl"),
                list(source, "MediaStreams").len(),
            )
        })
        .collect();
    let keys: Vec<&str> = answer
        .as_object()
        .map(|fields| fields.keys().map(String::as_str).filter(|key| key.contains("Source") || key.contains("Error")).collect())
        .unwrap_or_default();
    format!("lists {} {} (fields: {})", sources.len(), each.join(" "), keys.join(", "))
}

/// One stream per entry of `MediaSources` in a playback-information or
/// item answer.
pub(crate) fn read_streams(server: &Server, item: &str, answer: &Value) -> Vec<Stream> {
    let versions = list(answer, "MediaSources");
    let streams = read_sources(server, item, versions, versions.len() > 1);
    if !streams.is_empty() {
        return streams;
    }
    // The server listed no versions: the file itself still plays.
    vec![Stream {
        name: Some("Direct".to_owned()),
        description: Some(format!("The file as stored on {}", server.name)),
        url: Some(file_address(server, item, None)),
        ..Stream::default()
    }]
}

/// One stream per source; `several` says the film has more than one.
pub(crate) fn read_sources(
    server: &Server,
    item: &str,
    versions: &[Value],
    several: bool,
) -> Vec<Stream> {
    versions
        .iter()
        .filter_map(|version| {
            let id = text(version, "Id");
            let track = |kind: &str| {
                list(version, "MediaStreams")
                    .iter()
                    .find(|stream| text(stream, "Type").as_deref() == Some(kind))
            };
            let video = track("Video");
            let codec = |stream: Option<&Value>| {
                stream.and_then(|stream| text(stream, "Codec")).map(|codec| codec.to_uppercase())
            };
            let quality = quality(
                video.and_then(|video| number(video, "Width")),
                video.and_then(|video| number(video, "Height")),
            );
            // A source that is itself a web address (a stream gathered
            // from elsewhere) plays from there; a file plays from the server.
            let remote = text(version, "Path").filter(|path| {
                text(version, "Protocol").is_some_and(|protocol| protocol.eq_ignore_ascii_case("http"))
                    && (path.starts_with("http://") || path.starts_with("https://"))
            });
            // Or the server names the address to play it from itself.
            let given = text(version, "DirectStreamUrl").map(|url| {
                if url.starts_with("http://") || url.starts_with("https://") {
                    url
                } else {
                    let join = if url.contains('?') { '&' } else { '?' };
                    let slash = if url.starts_with('/') { "" } else { "/" };
                    format!("{}{slash}{url}{join}api_key={}", server.address, query(&server.token))
                }
            });
            let url = match (remote, given, &id) {
                (Some(path), _, _) => path,
                (None, Some(url), _) => url,
                (None, None, Some(id)) => file_address(server, item, Some(id)),
                (None, None, None) => return None,
            };
            let mut facts: Vec<String> = Vec::new();
            // The server's name for the source tells several apart. When
            // the server says nothing of the picture (a gathered stream),
            // the name's first line heads the row and the rest describes it.
            let mut heading = quality.clone();
            if let Some(name) = text(version, "Name") {
                if quality.is_some() {
                    if several {
                        facts.push(name);
                    }
                } else {
                    let mut lines = name.lines().map(str::trim).filter(|line| !line.is_empty());
                    heading = lines.next().map(str::to_owned);
                    facts.extend(lines.map(str::to_owned));
                }
            }
            // Containers can come as a list ("mov,mp4,m4a"): the first will do.
            facts.extend(
                text(version, "Container")
                    .and_then(|container| container.split(',').next().map(str::to_uppercase)),
            );
            facts.extend(codec(video));
            facts.extend(codec(track("Audio")));
            facts.extend(number(version, "Size").and_then(size));
            Some(Stream {
                name: Some(heading.unwrap_or_else(|| "Direct".to_owned())),
                description: Some(if facts.is_empty() {
                    format!("The file as stored on {}", server.name)
                } else {
                    facts.join(" · ")
                }),
                url: Some(url),
                // A server that gathers streams names where each came from.
                provider: version
                    .get("Remux")
                    .and_then(|extra| extra.get("ProviderInfo"))
                    .and_then(|info| text(info, "source")),
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

    // Made up in the shape Trellis answers with.
    #[test]
    fn collections_become_libraries_and_shared_addons_are_read() {
        let views = json!({"Items": [
            {"Id": "a", "Name": "Movies", "CollectionType": "movies"},
            {"Id": "636f6c", "Name": "Collections", "CollectionType": "boxsets"}
        ]});
        assert_eq!(collection_views(&views), ["636f6c"]);
        assert_eq!(read_libraries(&views).len(), 1);
        let found = json!({"Items": [
            {"Id": "c1", "Name": "Popular films", "Type": "BoxSet"},
            {"Id": "m1", "Name": "A Film", "Type": "Movie"},
            {"Name": "No id", "Type": "BoxSet"}
        ]});
        assert_eq!(
            read_collections(&found),
            [Library { id: "c1".into(), name: "Popular films".into(), collection: true }]
        );
    }

    #[cfg(feature = "shared-addons")]
    #[test]
    fn shared_addons_are_read() {
        let config = json!({"addons": [
            {"name": "One", "manifestUrl": "https://one.example/key123/manifest.json"},
            {"name": "Two", "nameOverride": "Second", "manifestUrl": "https://two.example/manifest.json"},
            {"name": "Off", "manifestUrl": "https://off.example/manifest.json", "disabled": true},
            {"name": "No address"},
            {"name": "Odd", "manifestUrl": "file:///x"}
        ]});
        let addons = read_shared_addons(&config);
        assert_eq!(addons.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["One", "Second"]);
        assert!(!format!("{:?}", addons[0]).contains("key123"));
        assert!(read_shared_addons(&json!({})).is_empty());
    }

    #[test]
    fn sources_are_picked_out_as_the_answer_arrives() {
        let answer = json!({
            "Note": "MediaSources",
            "Other": [{"Id": "no"}],
            "MediaSources": [
                {"Id": "a", "Name": "One \"quoted\" } ]", "MediaStreams": [{"Type": "Subtitle"}]},
                {"Id": "b", "Nested": {"MediaSources": [{"Id": "inner"}]}}
            ],
            "PlaySessionId": "x"
        })
        .to_string();
        let ids = |entries: &[Value]| entries.iter().filter_map(|e| text(e, "Id")).collect::<Vec<_>>();
        // Whole, and a few bytes at a time: the same two entries.
        let mut whole = SourceScanner::default();
        assert_eq!(ids(&whole.feed(answer.as_bytes())), ["a", "b"]);
        assert!(whole.done);
        let mut slow = SourceScanner::default();
        let mut entries = Vec::new();
        for part in answer.as_bytes().chunks(7) {
            entries.extend(slow.feed(part));
        }
        assert_eq!(ids(&entries), ["a", "b"]);
        assert_eq!(text(&entries[0], "Name").as_deref(), Some("One \"quoted\" } ]"));
        // The first is there before the answer is complete.
        let mut early = SourceScanner::default();
        let cut = answer.find("\"Id\":\"b\"").unwrap();
        assert_eq!(ids(&early.feed(&answer.as_bytes()[..cut])), ["a"]);
        assert!(!early.done);
    }

    #[test]
    fn a_gathered_title_can_be_named_by_its_imdb_id() {
        let hex = |text: &str| text.bytes().map(|b| format!("{b:02x}")).collect::<String>();
        let by_tmdb = hex("m|tmdb:603|movie");
        assert!(crate::wants_imdb(&by_tmdb));
        assert_eq!(crate::by_imdb(&by_tmdb, "tt0133093"), Some(hex("m|tt0133093|movie")));
        assert_eq!(crate::by_imdb(&hex("s|kitsu:12|series"), "tt0944947"), Some(hex("s|tt0944947|series")));
        // Already by its IMDb id, or not a gathered title at all.
        assert!(!crate::wants_imdb(&hex("m|tt0133093|movie")));
        assert_eq!(crate::by_imdb(&hex("m|tt0133093|movie"), "tt0133093"), None);
        assert!(!crate::wants_imdb("f27caa37e5142225cceded48f6553502"));
        assert_eq!(crate::by_imdb("f27caa37e5142225cceded48f6553502", "tt0133093"), None);
        assert_eq!(crate::by_imdb(&by_tmdb, "603"), None);
    }

    #[test]
    fn an_addons_error_is_not_a_film() {
        let hex = |text: &str| text.bytes().map(|b| format!("{b:02x}")).collect::<String>();
        let error = hex("m|aiostreamserror.%7B%22errorDescription%22%3A%22Rate%20Limit%22%7D|movie");
        assert!(is_error_entry(&error));
        assert!(!is_error_entry(&hex("m|tt0111161|movie")));
        assert!(!is_error_entry("f27caa37e5142225cceded48f6553502"));
        let items = json!([
            {"Type": "Movie", "Id": error, "Name": "Rate Limit Exceeded"},
            {"Type": "Movie", "Id": hex("m|tt0111161|movie"), "Name": "A Film"}
        ]);
        assert_eq!(read_previews(&server(), items.as_array().unwrap()).len(), 1);
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
            Some("http://media.example/Items/m1/Images/Primary?maxHeight=450&quality=90&api_key=tok%20en")
        );
    }

    #[test]
    fn detail_of_a_series_lists_its_episodes() {
        let detail = json!({
            "Id": "s1", "Type": "Series", "Name": "A Show", "Overview": "About it.",
            "ProductionYear": 2019, "CommunityRating": 8.25, "Genres": ["Drama", "Crime"],
            "RunTimeTicks": 27000000000u64, "BackdropImageTags": ["x"],
            "People": [{"Name": "An Actor", "Type": "Actor"}, {"Name": "A Director", "Type": "Director"}],
            "ProviderIds": {"Imdb": "tt123", "Tmdb": "456"}
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
        assert_eq!((meta.imdb_id.as_deref(), meta.tmdb_id.as_deref()), (Some("tt123"), Some("456")));
        assert_eq!((meta.videos[0].season, meta.videos[0].episode), (Some(1), Some(1)));
    }

    #[test]
    fn the_stream_address_carries_the_escaped_token() {
        let streams = read_streams(&server(), "m1", &json!({}));
        assert_eq!(streams.len(), 1);
        assert_eq!(
            streams[0].url.as_deref(),
            Some("http://media.example/Videos/m1/stream?static=true&api_key=tok%20en")
        );
    }

    #[test]
    fn every_version_of_a_file_is_a_stream() {
        let detail = json!({"MediaSources": [
            {"Id": "v1", "Name": "4K HDR", "Container": "mkv", "Size": 21474836480u64, "MediaStreams": [
                {"Type": "Video", "Codec": "hevc", "Width": 3840, "Height": 2160},
                {"Type": "Audio", "Codec": "eac3"}
            ]},
            {"Id": "v2", "Name": "HD", "Container": "mp4", "MediaStreams": [
                {"Type": "Video", "Codec": "h264", "Width": 1920, "Height": 1080}
            ]},
            {"Name": "No id"}
        ]});
        let streams = read_streams(&server(), "m1", &detail);
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0].name.as_deref(), Some("4K"));
        assert_eq!(streams[0].description.as_deref(), Some("4K HDR · MKV · HEVC · EAC3 · 20.0 GB"));
        assert_eq!(
            streams[0].url.as_deref(),
            Some("http://media.example/Videos/m1/stream?static=true&MediaSourceId=v1&api_key=tok%20en")
        );
        assert_eq!(streams[1].name.as_deref(), Some("1080p"));
        assert_eq!(streams[1].description.as_deref(), Some("HD · MP4 · H264"));
    }

    #[test]
    fn gathered_streams_play_from_their_own_address() {
        let info = json!({"MediaSources": [
            {"Id": "a", "Name": "Provider One 1080p WEB", "Protocol": "Http", "IsRemote": true,
             "Path": "https://streams.example/one.mkv", "Container": "mkv",
             "Remux": {"ProviderInfo": {"source": "Provider One"}}},
            {"Name": "Provider Two 4K", "Protocol": "Http", "Path": "https://streams.example/two.mp4",
             "MediaStreams": [{"Type": "Video", "Codec": "hevc", "Width": 3840, "Height": 2160}]},
            {"Id": "c", "Name": "A file", "Protocol": "File", "Path": "/media/file.mkv"},
            {"Name": "Nothing to play", "Protocol": "File", "Path": "/media/x.mkv"}
        ]});
        let streams = read_streams(&server(), "m1", &info);
        assert_eq!(streams.len(), 3);
        assert_eq!(streams[0].url.as_deref(), Some("https://streams.example/one.mkv"));
        assert_eq!(streams[0].name.as_deref(), Some("Provider One 1080p WEB"));
        assert_eq!(streams[0].description.as_deref(), Some("MKV"));
        assert_eq!(streams[1].name.as_deref(), Some("4K"));
        assert_eq!(streams[1].url.as_deref(), Some("https://streams.example/two.mp4"));
        assert_eq!(streams[0].provider.as_deref(), Some("Provider One"));
        assert_eq!(streams[1].provider, None);
        assert_eq!(
            streams[2].url.as_deref(),
            Some("http://media.example/Videos/m1/stream?static=true&MediaSourceId=c&api_key=tok%20en")
        );
    }

    #[test]
    fn a_source_can_name_its_own_address_and_the_log_names_no_secrets() {
        let info = json!({"MediaSources": [
            {"Id": "a", "Name": "Secret name", "Protocol": "File", "Path": "/media/secret.mkv",
             "DirectStreamUrl": "/Videos/m1/stream.mkv?MediaSourceId=a"}
        ], "ErrorCode": null});
        let streams = read_streams(&server(), "m1", &info);
        assert_eq!(
            streams[0].url.as_deref(),
            Some("http://media.example/Videos/m1/stream.mkv?MediaSourceId=a&api_key=tok%20en")
        );
        let shape = describe_sources(&info);
        assert!(shape.starts_with("lists 1 [protocol File id yes name yes path yes url yes streams 0]"), "{shape}");
        assert!(!shape.contains("secret") && !shape.contains("Secret"));
        assert!(describe_sources(&json!({})).starts_with("lists 0"));
    }

    #[test]
    fn pictures_carry_their_tag_and_remote_ones_are_used_as_given() {
        let detail = json!({
            "Id": "m1", "Type": "Movie", "Name": "A Film", "ImageTags": {"Primary": "p1"},
            "BackdropImageTags": ["b1"]
        });
        let meta = read_detail(&server(), &detail, &Value::Null).unwrap();
        assert_eq!(
            meta.poster.as_deref(),
            Some("http://media.example/Items/m1/Images/Primary?maxHeight=450&quality=90&tag=p1&api_key=tok%20en")
        );
        assert_eq!(
            meta.background.as_deref(),
            Some("http://media.example/Items/m1/Images/Backdrop?maxHeight=900&quality=90&tag=b1&api_key=tok%20en")
        );
        let gathered = json!({
            "Id": "m2", "Type": "Movie", "Name": "Gathered",
            "RemoteImages": [{"Type": "Primary", "Url": "https://pictures.example/p.jpg"}, {"Type": "Backdrop", "Url": "file:///x"}]
        });
        let meta = read_detail(&server(), &gathered, &Value::Null).unwrap();
        assert_eq!(meta.poster.as_deref(), Some("https://pictures.example/p.jpg"));
        assert!(meta.background.as_deref().unwrap().contains("/Items/m2/Images/Backdrop"));
    }

    #[test]
    fn candidates_carry_their_ids_and_episodes_are_found_by_number() {
        let items = json!({"Items": [
            {"Id": "m1", "Name": "A Film", "ProductionYear": 2020, "ProviderIds": {"Imdb": "tt7", "Tmdb": "42"}},
            {"Id": "m2", "Name": "Bare", "ProviderIds": {"imdb": " "}},
            {"Name": "No id"}
        ]});
        let candidates = read_candidates(&items);
        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].imdb.as_deref(), Some("tt7"));
        assert_eq!(candidates[0].tmdb.as_deref(), Some("42"));
        assert_eq!(candidates[1].imdb, None);

        let episodes = json!({"Items": [
            {"Id": "e1", "ParentIndexNumber": 1, "IndexNumber": 1},
            {"Id": "e2", "ParentIndexNumber": 1, "IndexNumber": 2},
            {"Id": "e3", "ParentIndexNumber": 2, "IndexNumber": 1}
        ]});
        assert_eq!(read_episode(&episodes, 2, 1).as_deref(), Some("e3"));
        assert_eq!(read_episode(&episodes, 3, 1), None);
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
