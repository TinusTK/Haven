//! Media servers: the user's own Jellyfin, Emby and Plex servers, shown
//! with the same shapes add-ons use (posters, details, streams) so the
//! rest of Haven does not care where a title comes from.
//!
//! A title on a server has an id of the form `srv:<server>:<item>`, where
//! `<server>` is the server's number in Haven's database.

mod emby;
mod plex;

use haven_addons::{MetaDetail, MetaPreview, Stream};

pub use plex::PlexPin;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("\"{0}\" is not a server address")]
    InvalidAddress(String),
    #[error("could not reach the server: {0}")]
    Request(#[from] reqwest::Error),
    #[error("the user name or password was not accepted")]
    SignInRefused,
    #[error("the server answered with error {0}")]
    Status(u16),
    #[error("the server's answer could not be read")]
    BadAnswer,
    #[error("the server's answer was too large")]
    TooLarge,
    #[error("none of this Plex server's addresses could be reached")]
    Unreachable,
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Jellyfin,
    Emby,
    Plex,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Jellyfin => "jellyfin",
            Kind::Emby => "emby",
            Kind::Plex => "plex",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "jellyfin" => Some(Kind::Jellyfin),
            "emby" => Some(Kind::Emby),
            "plex" => Some(Kind::Plex),
            _ => None,
        }
    }

    /// The name people know it by.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Jellyfin => "Jellyfin",
            Kind::Emby => "Emby",
            Kind::Plex => "Plex",
        }
    }
}

/// A server the user signed in to.
#[derive(Clone, PartialEq, Eq)]
pub struct Server {
    /// Its number in Haven's database; 0 until it is saved.
    pub id: i64,
    pub kind: Kind,
    pub name: String,
    /// Where it answers, without a trailing slash.
    pub address: String,
    /// The signed-in user's id on the server (unused for Plex).
    pub user_id: String,
    /// What the server gave in exchange for signing in.
    pub token: String,
}

/// Never print the token.
impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("id", &self.id)
            .field("kind", &self.kind)
            .field("name", &self.name)
            .field("address", &self.address)
            .finish_non_exhaustive()
    }
}

/// One of a server's libraries that holds films or series.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Library {
    pub id: String,
    pub name: String,
    /// A collection (a list someone put together, such as an add-on's
    /// catalogue on a server that gathers them) and not a whole library.
    pub collection: bool,
}

/// An add-on a server uses and is willing to tell its owner about.
#[derive(Clone, PartialEq, Eq)]
pub struct SharedAddon {
    pub name: String,
    /// The add-on's manifest address. It can hold the user's keys for
    /// other services, so it is never printed.
    pub manifest_url: String,
}

impl std::fmt::Debug for SharedAddon {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedAddon").field("name", &self.name).finish_non_exhaustive()
    }
}

/// A film or episode to look for on a server: the one Haven has open
/// from somewhere else (another server, an add-on, the TMDB catalogue).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Wanted {
    pub series: bool,
    pub name: String,
    /// The year it came out; for a series the year it began.
    pub year: Option<String>,
    pub imdb: Option<String>,
    pub tmdb: Option<String>,
    /// For an episode: its season and its number in the season.
    pub episode: Option<(u32, u32)>,
}

/// A title on a server that might be the wanted one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Candidate {
    pub id: String,
    pub name: String,
    pub year: Option<String>,
    pub imdb: Option<String>,
    pub tmdb: Option<String>,
}

/// Letters and digits only, lower case: "The Film!" and "the film" agree.
fn plain(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// The first four digits: "2008-2013" and "2008" agree.
fn year_of(text: &Option<String>) -> Option<&str> {
    text.as_deref().and_then(|text| text.get(..4)).filter(|y| y.bytes().all(|b| b.is_ascii_digit()))
}

/// Picks the candidate that is the wanted title. Shared ids decide; if
/// the two sides share no kind of id, the name and the year must agree.
pub(crate) fn best_match<'a>(wanted: &Wanted, candidates: &'a [Candidate]) -> Option<&'a Candidate> {
    let same = |a: &Option<String>, b: &Option<String>| match (a, b) {
        (Some(a), Some(b)) => Some(a.eq_ignore_ascii_case(b)),
        _ => None,
    };
    let by_id = candidates.iter().find(|candidate| {
        same(&wanted.imdb, &candidate.imdb) == Some(true)
            || same(&wanted.tmdb, &candidate.tmdb) == Some(true)
    });
    by_id.or_else(|| {
        candidates.iter().find(|candidate| {
            // An id of the same kind that differs means another title.
            same(&wanted.imdb, &candidate.imdb).is_none()
                && same(&wanted.tmdb, &candidate.tmdb).is_none()
                && !wanted.name.is_empty()
                && plain(&wanted.name) == plain(&candidate.name)
                && match (year_of(&wanted.year), year_of(&candidate.year)) {
                    (Some(a), Some(b)) => a == b,
                    _ => true,
                }
        })
    })
}

/// The id Haven uses for an item on a server.
pub fn title_id(server: i64, item: &str) -> String {
    format!("srv:{server}:{item}")
}

/// Splits a `srv:<server>:<item>` id; `None` for any other id.
pub fn split_id(id: &str) -> Option<(i64, &str)> {
    let (server, item) = id.strip_prefix("srv:")?.split_once(':')?;
    Some((server.parse().ok()?, item)).filter(|(_, item)| !item.is_empty())
}

/// Tidies an address the user typed: adds `http://` when no scheme is
/// given and drops trailing slashes. Only http and https are accepted.
pub fn server_address(typed: &str) -> Result<String> {
    let typed = typed.trim();
    let with_scheme =
        if typed.contains("://") { typed.to_owned() } else { format!("http://{typed}") };
    let url = haven_core::http::web_url(&with_scheme)
        .map_err(|_| Error::InvalidAddress(typed.to_owned()))?;
    if url.host_str().is_none_or(str::is_empty) {
        return Err(Error::InvalidAddress(typed.to_owned()));
    }
    Ok(url.as_str().trim_end_matches('/').to_owned())
}

const ANSWER_BYTES: usize = 16 * 1024 * 1024;
/// Titles fetched at once for a shelf of a whole library.
const LIST_LENGTH: u32 = 300;

/// A picture's height in words: "4K", "1080p", "720p"…
fn quality(width: Option<f64>, height: Option<f64>) -> Option<String> {
    let (width, height) = (width.unwrap_or(0.0), height.unwrap_or(0.0));
    if width <= 0.0 && height <= 0.0 {
        return None;
    }
    Some(if width >= 3800.0 || height >= 2000.0 {
        "4K".to_owned()
    } else if width >= 1900.0 || height >= 1000.0 {
        "1080p".to_owned()
    } else if width >= 1260.0 || height >= 700.0 {
        "720p".to_owned()
    } else if height > 0.0 {
        format!("{height:.0}p")
    } else {
        "SD".to_owned()
    })
}

/// A file size in words: "4.2 GB", "700 MB".
fn size(bytes: f64) -> Option<String> {
    const MB: f64 = 1024.0 * 1024.0;
    if bytes < MB {
        None
    } else if bytes >= 1024.0 * MB {
        Some(format!("{:.1} GB", bytes / (1024.0 * MB)))
    } else {
        Some(format!("{:.0} MB", bytes / MB))
    }
}

/// Talks to media servers.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    /// Identifies this copy of Haven to servers; made once and kept.
    device: String,
}

impl Client {
    pub fn new(http: reqwest::Client, device: String) -> Self {
        Self { http, device }
    }

    /// Sends a request and reads a JSON answer of limited size. `body` makes
    /// it a POST. An empty answer reads as `null`.
    async fn call(
        &self,
        url: &str,
        headers: &[(&str, String)],
        body: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let url = haven_core::http::web_url(url).map_err(|_| Error::InvalidAddress(url.to_owned()))?;
        let mut request = match &body {
            Some(body) => self
                .http
                .post(url)
                .header("Content-Type", "application/json")
                .body(body.to_string()),
            None => self.http.get(url),
        };
        request = request.header("Accept", "application/json");
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let mut response = request.send().await.map_err(reqwest::Error::without_url)?;
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err(Error::SignInRefused),
            other => return Err(Error::Status(other)),
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(reqwest::Error::without_url)? {
            if bytes.len() + chunk.len() > ANSWER_BYTES {
                return Err(Error::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        if bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(serde_json::Value::Null);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::BadAnswer)
    }

    /// Signs in to a Jellyfin or Emby server with a user name and password.
    /// The password is used once and not kept.
    pub async fn sign_in(
        &self,
        kind: Kind,
        address: &str,
        user: &str,
        password: &str,
    ) -> Result<Server> {
        emby::sign_in(self, kind, &server_address(address)?, user, password).await
    }

    /// The server's libraries of films and series.
    pub async fn libraries(&self, server: &Server) -> Result<Vec<Library>> {
        match server.kind {
            Kind::Plex => plex::libraries(self, server).await,
            _ => emby::libraries(self, server).await,
        }
    }

    /// What was added to a library most recently.
    pub async fn latest(&self, server: &Server, library: &str) -> Result<Vec<MetaPreview>> {
        match server.kind {
            Kind::Plex => plex::titles(self, server, library, true).await,
            _ => emby::latest(self, server, library).await,
        }
    }

    /// A library's titles by name (the first few hundred).
    pub async fn all(&self, server: &Server, library: &str) -> Result<Vec<MetaPreview>> {
        self.page(server, library, None, 0, LIST_LENGTH).await
    }

    /// Part of a library's titles by name: `length` of them from `start`,
    /// narrowed to a genre if one is given (Jellyfin and Emby only).
    pub async fn page(
        &self,
        server: &Server,
        library: &str,
        genre: Option<&str>,
        start: u32,
        length: u32,
    ) -> Result<Vec<MetaPreview>> {
        match server.kind {
            Kind::Plex => plex::page(self, server, library, start, length).await,
            _ => emby::page(self, server, library, genre, start, length).await,
        }
    }

    /// The genres a library or collection can be narrowed to; empty when
    /// the server does not say.
    pub async fn genres(&self, server: &Server, library: &str) -> Vec<String> {
        match server.kind {
            Kind::Plex => Vec::new(),
            _ => emby::genres(self, server, library).await,
        }
    }

    /// The add-ons the server itself uses, when it is the kind of server
    /// that gathers add-ons (Trellis) and the signed-in user owns it.
    /// Empty for every other server.
    pub async fn shared_addons(&self, server: &Server) -> Result<Vec<SharedAddon>> {
        match server.kind {
            Kind::Jellyfin => emby::shared_addons(self, server).await,
            _ => Ok(Vec::new()),
        }
    }

    /// Whether the server answers right now. Needs no sign-in.
    pub async fn online(&self, server: &Server) -> bool {
        match server.kind {
            Kind::Plex => plex::online(self, server).await,
            _ => emby::online(self, server).await,
        }
    }

    /// Looks for a film or episode on the server. Gives the item's id
    /// there (the part after the server's number), or `None` if the
    /// server does not have it.
    pub async fn find(&self, server: &Server, wanted: &Wanted) -> Result<Option<String>> {
        match server.kind {
            Kind::Plex => plex::find(self, server, wanted).await,
            _ => emby::find(self, server, wanted).await,
        }
    }

    /// Titles on the server whose name matches `words`.
    pub async fn search(&self, server: &Server, words: &str) -> Result<Vec<MetaPreview>> {
        match server.kind {
            Kind::Plex => plex::search(self, server, words).await,
            _ => emby::search(self, server, words).await,
        }
    }

    /// Everything the server knows about one film or series (`item` is
    /// the part of the id after the server's number).
    pub async fn meta(&self, server: &Server, item: &str) -> Result<MetaDetail> {
        match server.kind {
            Kind::Plex => plex::meta(self, server, item).await,
            _ => emby::meta(self, server, item).await,
        }
    }

    /// The ways to play one film or episode from the server.
    pub async fn streams(&self, server: &Server, item: &str) -> Result<Vec<Stream>> {
        match server.kind {
            Kind::Plex => plex::streams(self, server, item).await,
            _ => emby::streams(self, server, item).await,
        }
    }

    /// As [`Client::streams`], handing the ways to `found` a few at a
    /// time while a long list is still arriving. `found` answers
    /// whether more are wanted; on no, the rest is not read.
    pub async fn streams_live(
        &self,
        server: &Server,
        item: &str,
        mut found: impl FnMut(Vec<Stream>) -> bool + Send,
    ) -> Result<()> {
        match server.kind {
            Kind::Plex => {
                let _ = found(plex::streams(self, server, item).await?);
                Ok(())
            }
            _ => emby::streams_live(self, server, item, found).await,
        }
    }

    /// Tells the server how far a film or episode was watched, in seconds.
    pub async fn report(
        &self,
        server: &Server,
        item: &str,
        position: f64,
        duration: f64,
        finished: bool,
    ) -> Result<()> {
        match server.kind {
            Kind::Plex => plex::report(self, server, item, position, duration, finished).await,
            _ => emby::report(self, server, item, position, finished).await,
        }
    }

    /// Starts signing in to Plex: gives a code and the page to enter it on.
    pub async fn plex_pin(&self) -> Result<PlexPin> {
        plex::pin(self).await
    }

    /// Whether the user has approved the code yet: the account's token
    /// once they have.
    pub async fn plex_token(&self, pin: &PlexPin) -> Result<Option<String>> {
        plex::token(self, pin).await
    }

    /// The Plex servers the account can use, each with an address that
    /// answered from here.
    pub async fn plex_servers(&self, account_token: &str) -> Result<Vec<Server>> {
        plex::servers(self, account_token).await
    }
}

/// Reads text from a JSON field, trimmed; `None` if missing or blank.
fn text(value: &serde_json::Value, key: &str) -> Option<String> {
    match value.get(key)? {
        serde_json::Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        serde_json::Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn number(value: &serde_json::Value, key: &str) -> Option<f64> {
    match value.get(key)? {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

fn list<'a>(value: &'a serde_json::Value, key: &str) -> &'a [serde_json::Value] {
    value.get(key).and_then(|list| list.as_array()).map_or(&[], Vec::as_slice)
}

/// Percent-encodes text for use inside a query string.
fn query(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip_and_other_ids_are_left_alone() {
        assert_eq!(title_id(3, "abc"), "srv:3:abc");
        assert_eq!(split_id("srv:3:abc"), Some((3, "abc")));
        assert_eq!(split_id("srv:12:a:b"), Some((12, "a:b")));
        assert_eq!(split_id("tt0903747"), None);
        assert_eq!(split_id("srv:x:abc"), None);
        assert_eq!(split_id("srv:3:"), None);
    }

    #[test]
    fn addresses_are_tidied() {
        assert_eq!(server_address(" 192.168.1.5:8096/ ").unwrap(), "http://192.168.1.5:8096");
        assert_eq!(server_address("https://media.example.org/jf/").unwrap(), "https://media.example.org/jf");
        assert!(matches!(server_address("ftp://x"), Err(Error::InvalidAddress(_))));
        assert!(matches!(server_address(""), Err(Error::InvalidAddress(_))));
    }

    #[test]
    fn the_wanted_title_is_matched_by_id_then_by_name_and_year() {
        let candidate = |id: &str, name: &str, year: &str, imdb: Option<&str>| Candidate {
            id: id.into(),
            name: name.into(),
            year: Some(year.into()),
            imdb: imdb.map(Into::into),
            tmdb: None,
        };
        let on_server = [
            candidate("a", "The Film", "1990", Some("tt1")),
            candidate("b", "The Film", "2021", Some("tt2")),
            candidate("c", "Another: Film!", "2005", None),
        ];
        let wanted = |name: &str, year: &str, imdb: Option<&str>| Wanted {
            name: name.into(),
            year: Some(year.into()),
            imdb: imdb.map(Into::into),
            ..Wanted::default()
        };
        // The id decides, whatever the name says.
        assert_eq!(best_match(&wanted("Renamed", "2021", Some("TT2")), &on_server).unwrap().id, "b");
        // A different id of the same kind is another title, even by the same name.
        assert!(best_match(&wanted("The Film", "1990", Some("tt9")), &on_server).is_none());
        // No id to compare: name and year.
        assert_eq!(best_match(&wanted("another film", "2005-2009", None), &on_server).unwrap().id, "c");
        assert!(best_match(&wanted("another film", "2006", None), &on_server).is_none());
        assert!(best_match(&wanted("", "2005", None), &on_server).is_none());
    }

    #[test]
    fn sizes_and_qualities_read_well() {
        assert_eq!(quality(Some(3840.0), Some(1600.0)).as_deref(), Some("4K"));
        assert_eq!(quality(Some(1920.0), Some(800.0)).as_deref(), Some("1080p"));
        assert_eq!(quality(None, Some(720.0)).as_deref(), Some("720p"));
        assert_eq!(quality(Some(720.0), Some(576.0)).as_deref(), Some("576p"));
        assert_eq!(quality(None, None), None);
        assert_eq!(size(4.5 * 1024.0 * 1024.0 * 1024.0).as_deref(), Some("4.5 GB"));
        assert_eq!(size(700.0 * 1024.0 * 1024.0).as_deref(), Some("700 MB"));
        assert_eq!(size(10.0), None);
    }

    #[test]
    fn query_text_is_escaped() {
        assert_eq!(query("the matrix & more"), "the%20matrix%20%26%20more");
    }

    #[test]
    fn a_server_never_prints_its_token() {
        let server = Server {
            id: 1,
            kind: Kind::Plex,
            name: "Home".into(),
            address: "http://x".into(),
            user_id: String::new(),
            token: "topsecret".into(),
        };
        assert!(!format!("{server:?}").contains("topsecret"));
    }
}
