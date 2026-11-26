//! Tracking what was watched with Trakt and Simkl: signing in with a
//! short code, marking films and episodes watched, and keeping the
//! watchlist in step. Both services know titles by their IMDb id, so only
//! titles that carry one can be tracked.
//!
//! Each service needs the app's own keys (a client id; Trakt also a
//! client secret), which come from Settings.

pub mod sync;

use haven_core::{http, secrets::Secrets};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const TRAKT_API: &str = "https://api.trakt.tv";
/// Where Trakt signs people in and asks them to allow an app.
const TRAKT_SITE: &str = "https://auth.trakt.tv";
/// Where trakt.tv sends the browser back to after the user allows Haven:
/// a small listener Haven runs on this computer while linking. Register
/// it (and `urn:ietf:wg:oauth:2.0:oob`) as Redirect URIs of Haven's
/// Trakt app.
pub const TRAKT_REDIRECT: &str = "http://127.0.0.1:47219/trakt";
/// The port in [`TRAKT_REDIRECT`].
pub const TRAKT_REDIRECT_PORT: u16 = 47219;

/// Trakt's API, or a stand-in named by `HAVEN_TRAKT_API` in a debug build.
fn trakt_api() -> String {
    if cfg!(debug_assertions) {
        if let Ok(at) = std::env::var("HAVEN_TRAKT_API") {
            return at;
        }
    }
    TRAKT_API.to_owned()
}

/// Trakt's website, or a stand-in named by `HAVEN_TRAKT_SITE` in a debug build.
fn trakt_site() -> String {
    if cfg!(debug_assertions) {
        if let Ok(at) = std::env::var("HAVEN_TRAKT_SITE") {
            return at;
        }
    }
    TRAKT_SITE.to_owned()
}
const SIMKL: &str = "https://api.simkl.com";
const ANSWER_BYTES: usize = 16 * 1024 * 1024;
/// A Trakt token is renewed when it has less than this long to live.
const RENEW_BEFORE: i64 = 60 * 60;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Http(#[from] http::HttpError),
    #[error("no {0} key is set in Settings")]
    NoKey(&'static str),
    #[error("{0} did not accept the key or the sign-in")]
    Refused(&'static str),
    #[error("the service answered with error {0}")]
    Status(u16),
    #[error("the service's answer could not be read")]
    BadAnswer,
    #[error("the code ran out before it was approved")]
    Expired,
    #[error("could not reach the service: {0}")]
    Network(String),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Service {
    Trakt,
    Simkl,
}

impl Service {
    pub fn label(self) -> &'static str {
        match self {
            Service::Trakt => "Trakt",
            Service::Simkl => "Simkl",
        }
    }
}

/// A signed-in account, as kept between runs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub service: Service,
    pub token: String,
    /// Trakt only: exchanges for a new token when this one runs out.
    #[serde(default)]
    pub refresh: Option<String>,
    /// When the token runs out, in seconds since 1970, if it does.
    #[serde(default)]
    pub expires_at: Option<i64>,
}

/// Never print the tokens.
impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("service", &self.service).finish_non_exhaustive()
    }
}

/// A sign-in in progress: the user types `user_code` at `link`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Code {
    pub service: Service,
    pub user_code: String,
    pub link: String,
    /// What Haven asks with while waiting (Trakt's device code; for Simkl
    /// the user code again).
    pub device_code: String,
    /// Seconds between asks.
    pub interval: u64,
    /// Seconds until the code runs out.
    pub expires_in: u64,
}

/// A film, a series, or one episode, by IMDb id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub imdb: String,
    pub series: bool,
    /// Season and episode numbers, for one episode.
    pub episode: Option<(u32, u32)>,
}

/// A title on a service's watchlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub imdb: String,
    pub series: bool,
    pub title: String,
    pub year: String,
}

fn is_imdb_id(id: &str) -> bool {
    id.strip_prefix("tt").is_some_and(|digits| {
        !digits.is_empty() && digits.len() <= 12 && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

/// What to track for a title of Haven's: `None` if it has no IMDb id.
/// `video_id` is the film's or episode's own id; episodes of add-on
/// series are named `<imdb id>:<season>:<episode>`.
pub fn subject(kind: &str, title_id: &str, video_id: Option<&str>) -> Option<Subject> {
    if !is_imdb_id(title_id) {
        return None;
    }
    let series = kind == "series";
    let episode = video_id.filter(|_| series).and_then(|video| {
        let mut parts = video.split(':');
        (parts.next()? == title_id).then_some(())?;
        let numbers = (parts.next()?.parse().ok()?, parts.next()?.parse().ok()?);
        parts.next().is_none().then_some(numbers)
    });
    Some(Subject { imdb: title_id.to_owned(), series, episode })
}

/// The JSON both services take to name a subject.
fn body(subject: &Subject, simkl_list: Option<&str>) -> Value {
    let mut entry = json!({ "ids": { "imdb": subject.imdb } });
    if let Some(list) = simkl_list {
        entry["to"] = json!(list);
    }
    if let Some((season, episode)) = subject.episode {
        entry["seasons"] = json!([{ "number": season, "episodes": [{ "number": episode }] }]);
    }
    if subject.series { json!({ "shows": [entry] }) } else { json!({ "movies": [entry] }) }
}

/// A film or episode the service says was watched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watched {
    pub imdb: String,
    pub title: String,
    /// Season and episode for an episode; `None` for a film.
    pub episode: Option<(u32, u32)>,
    /// When, in seconds since 1970 (0 if not given).
    pub at: i64,
}

fn moment(text: Option<&Value>) -> i64 {
    text.and_then(Value::as_str)
        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
        .map_or(0, |moment| moment.timestamp())
}

/// Trakt's `/sync/watched/movies` or `/sync/watched/shows` answer, or
/// Simkl's `/sync/all-items/…` one (wrapped in {"movies": […]} or
/// {"shows": […]}, an episode's time in `watched_at`).
pub fn read_watched(answer: &Value, series: bool) -> Vec<Watched> {
    let key = if series { "show" } else { "movie" };
    let mut found = Vec::new();
    let entries = answer.as_array().or_else(|| answer.get(if series { "shows" } else { "movies" })?.as_array());
    for entry in entries.map(Vec::as_slice).unwrap_or_default() {
        let Some(title) = entry.get(key) else { continue };
        let Some(imdb) = title.get("ids").and_then(|ids| ids.get("imdb")).and_then(Value::as_str).filter(|id| is_imdb_id(id)) else {
            continue;
        };
        let name = title.get("title").and_then(Value::as_str).unwrap_or_default().to_owned();
        if !series {
            found.push(Watched { imdb: imdb.to_owned(), title: name, episode: None, at: moment(entry.get("last_watched_at")) });
            continue;
        }
        for season in entry.get("seasons").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
            let Some(number) = season.get("number").and_then(Value::as_u64) else { continue };
            for episode in season.get("episodes").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default() {
                let Some(episode_number) = episode.get("number").and_then(Value::as_u64) else { continue };
                found.push(Watched {
                    imdb: imdb.to_owned(),
                    title: name.clone(),
                    episode: Some((number as u32, episode_number as u32)),
                    at: moment(episode.get("last_watched_at").or_else(|| episode.get("watched_at"))),
                });
            }
        }
    }
    found
}

fn read_listed(answer: &Value, series: bool) -> Vec<Listed> {
    let key = if series { "show" } else { "movie" };
    // Trakt answers with a list; Simkl wraps it in {"movies": […]} or {"shows": […]}.
    let entries = answer
        .as_array()
        .or_else(|| answer.get(if series { "shows" } else { "movies" })?.as_array());
    entries
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| {
            let title = entry.get(key)?;
            let imdb = title.get("ids")?.get("imdb")?.as_str().filter(|id| is_imdb_id(id))?;
            Some(Listed {
                imdb: imdb.to_owned(),
                series,
                title: title.get("title")?.as_str()?.to_owned(),
                year: match title.get("year") {
                    Some(Value::Number(year)) => year.to_string(),
                    Some(Value::String(year)) => year.clone(),
                    _ => String::new(),
                },
            })
        })
        .collect()
}

/// Talks to Trakt and Simkl.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    keys: Secrets,
}

/// Adds the Client Secret to a Trakt token request, when there is one.
fn with_secret(mut body: Value, secret: Option<&str>) -> Value {
    if let (Some(secret), Some(fields)) = (secret, body.as_object_mut()) {
        fields.insert("client_secret".to_owned(), Value::String(secret.to_owned()));
    }
    body
}

/// One sign-in's PKCE pair: the verifier Haven keeps, and the challenge
/// (its SHA-256, base64url without padding) sent to Trakt.
#[derive(Debug, Clone)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Self {
        use ring::rand::SecureRandom;
        const LETTERS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
        let mut bytes = [0u8; 64];
        let _ = ring::rand::SystemRandom::new().fill(&mut bytes);
        let verifier: String = bytes.iter().map(|b| LETTERS[*b as usize % LETTERS.len()] as char).collect();
        Self::from_verifier(verifier)
    }

    pub fn from_verifier(verifier: String) -> Self {
        let digest = ring::digest::digest(&ring::digest::SHA256, verifier.as_bytes());
        Self { challenge: base64url(digest.as_ref()), verifier }
    }
}

impl Default for Pkce {
    fn default() -> Self {
        Self::new()
    }
}

/// Base64url without padding.
fn base64url(bytes: &[u8]) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..chunk.len() + 1 {
            out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

impl Client {
    pub fn new(http: reqwest::Client, keys: Secrets) -> Self {
        Self { http, keys }
    }

    /// Whether the keys a service needs are set.
    pub fn has_keys(&self, service: Service) -> bool {
        match service {
            // Trakt's apps sign people in with PKCE: the Client ID is all
            // they need (new apps get no Client Secret, 2026).
            Service::Trakt => self.keys.trakt_client_id.is_some(),
            Service::Simkl => self.keys.simkl_client_id.is_some(),
        }
    }

    /// The Client ID, and the Client Secret if this copy has one (older
    /// Trakt apps); a secret, if any, goes along with token requests.
    fn trakt_keys(&self) -> Result<(&str, Option<&str>)> {
        let id = self.keys.trakt_client_id.as_deref().ok_or(Error::NoKey("Trakt"))?;
        Ok((id, self.keys.trakt_client_secret.as_deref()))
    }

    fn simkl_key(&self) -> Result<&str> {
        self.keys.simkl_client_id.as_deref().ok_or(Error::NoKey("Simkl"))
    }

    fn headers(&self, account: &Account) -> Result<Vec<(&'static str, String)>> {
        let bearer = ("Authorization", format!("Bearer {}", account.token));
        Ok(match account.service {
            Service::Trakt => vec![
                bearer,
                ("trakt-api-version", "2".to_owned()),
                ("trakt-api-key", self.trakt_keys()?.0.to_owned()),
            ],
            Service::Simkl => vec![bearer, ("simkl-api-key", self.simkl_key()?.to_owned())],
        })
    }

    async fn call(
        &self,
        service: Service,
        url: &str,
        headers: &[(&'static str, String)],
        body: Option<Value>,
    ) -> Result<(u16, Value)> {
        let (status, bytes) =
            http::send(&self.http, url, headers, body.map(|b| b.to_string()), ANSWER_BYTES).await?;
        if matches!(status, 401 | 403) {
            return Err(Error::Refused(service.label()));
        }
        let answer = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        Ok((status, answer))
    }

    /// Starts a sign-in and gives the code to show the user.
    pub async fn begin(&self, service: Service) -> Result<Code> {
        let field = |answer: &Value, key: &str| -> Result<String> {
            answer.get(key).and_then(Value::as_str).map(str::to_owned).ok_or(Error::BadAnswer)
        };
        let seconds = |answer: &Value, key: &str, default: u64| {
            answer.get(key).and_then(Value::as_u64).unwrap_or(default)
        };
        match service {
            Service::Trakt => {
                let (id, _) = self.trakt_keys()?;
                let (status, answer) = self
                    .call(
                        service,
                        &format!("{}/oauth/device/code", trakt_api()),
                        &[],
                        Some(json!({ "client_id": id })),
                    )
                    .await?;
                if status != 200 {
                    return Err(Error::Status(status));
                }
                Ok(Code {
                    service,
                    user_code: field(&answer, "user_code")?,
                    link: field(&answer, "verification_url")?,
                    device_code: field(&answer, "device_code")?,
                    interval: seconds(&answer, "interval", 5).max(1),
                    expires_in: seconds(&answer, "expires_in", 600),
                })
            }
            // Simkl's AUTH V2 device sign-in: the person approves the code
            // at simkl.com/pin (the link has it filled in).
            Service::Simkl => {
                let id = self.simkl_key()?;
                let (status, answer) = self
                    .simkl_form(
                        "/oauth2/device",
                        &[("client_id", id), ("scope", "media:read media:write")],
                    )
                    .await?;
                if status != 200 {
                    return Err(Error::Status(status));
                }
                let user_code = field(&answer, "user_code")?;
                let link = field(&answer, "verification_uri_complete")
                    .or_else(|_| field(&answer, "verification_uri"))?;
                Ok(Code {
                    service,
                    link,
                    device_code: field(&answer, "device_code")?,
                    user_code,
                    interval: seconds(&answer, "interval", 5).max(1),
                    expires_in: seconds(&answer, "expires_in", 900),
                })
            }
        }
    }

    /// The trakt.tv page where the user signs in and allows Haven; it
    /// sends the browser back to [`TRAKT_REDIRECT`] with a code and `state`.
    /// `challenge` is the PKCE challenge of this sign-in ([`Pkce`]).
    pub fn trakt_authorize_url(&self, state: &str, challenge: &str) -> Result<String> {
        let (id, _) = self.trakt_keys()?;
        let mut url = reqwest::Url::parse(&format!("{}/oauth/authorize", trakt_site())).map_err(|_| Error::BadAnswer)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", id)
            .append_pair("redirect_uri", TRAKT_REDIRECT)
            .append_pair("state", state)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(url.to_string())
    }

    /// Trades the code trakt.tv sent back for a sign-in.
    /// `verifier` is the PKCE verifier the sign-in started with.
    pub async fn trakt_exchange(&self, code: &str, verifier: &str, now: i64) -> Result<Account> {
        let (id, secret) = self.trakt_keys()?;
        let (status, answer) = self
            .call(
                Service::Trakt,
                &format!("{}/oauth/token", trakt_api()),
                &[],
                Some(with_secret(
                    json!({
                        "code": code,
                        "client_id": id,
                        "code_verifier": verifier,
                        "redirect_uri": TRAKT_REDIRECT,
                        "grant_type": "authorization_code",
                    }),
                    secret,
                )),
            )
            .await?;
        if status != 200 {
            return Err(Error::Status(status));
        }
        read_trakt_token(&answer, now).ok_or(Error::BadAnswer)
    }

    /// Tells Trakt to forget the sign-in (best effort).
    pub async fn trakt_revoke(&self, account: &Account) -> Result<()> {
        let (id, secret) = self.trakt_keys()?;
        let _ = self
            .call(
                Service::Trakt,
                &format!("{}/oauth/revoke", trakt_api()),
                &[],
                Some(with_secret(json!({ "token": account.token, "client_id": id }), secret)),
            )
            .await?;
        Ok(())
    }

    /// Asks whether the user has approved the code yet. `now` is the time
    /// in seconds since 1970.
    pub async fn poll(&self, code: &Code, now: i64) -> Result<Option<Account>> {
        match code.service {
            Service::Trakt => {
                let (id, secret) = self.trakt_keys()?;
                let (status, answer) = self
                    .call(
                        code.service,
                        &format!("{}/oauth/device/token", trakt_api()),
                        &[],
                        Some(with_secret(json!({ "code": code.device_code, "client_id": id }), secret)),
                    )
                    .await?;
                match status {
                    200 => read_trakt_token(&answer, now).map(Some).ok_or(Error::BadAnswer),
                    // Not approved yet, or asked too soon.
                    400 | 429 => Ok(None),
                    404 | 409 | 410 | 418 => Err(Error::Expired),
                    other => Err(Error::Status(other)),
                }
            }
            Service::Simkl => {
                let id = self.simkl_key()?;
                let (status, answer) = self
                    .simkl_form(
                        "/oauth2/token",
                        &[
                            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                            ("client_id", id),
                            ("device_code", &code.device_code),
                        ],
                    )
                    .await?;
                if status == 200 {
                    return read_simkl_token(&answer, now, None).map(Some).ok_or(Error::BadAnswer);
                }
                match answer.get("error").and_then(Value::as_str) {
                    // Not approved yet (a "no" looks the same), or asked too soon.
                    Some("authorization_pending" | "slow_down") => Ok(None),
                    Some("expired_token" | "invalid_grant") => Err(Error::Expired),
                    _ => Err(Error::Status(status)),
                }
            }
        }
    }

    /// A renewed account if this one's token is about to run out;
    /// `None` if it is still good (or never runs out).
    pub async fn renewed(&self, account: &Account, now: i64) -> Result<Option<Account>> {
        let (Some(expires_at), Some(refresh)) = (account.expires_at, account.refresh.as_deref())
        else {
            return Ok(None);
        };
        if expires_at - now > RENEW_BEFORE {
            return Ok(None);
        }
        if account.service == Service::Simkl {
            let id = self.simkl_key()?;
            let (status, answer) = self
                .simkl_form("/oauth2/token", &[("grant_type", "refresh_token"), ("client_id", id), ("refresh_token", refresh)])
                .await?;
            if status != 200 {
                return Err(Error::Refused("Simkl"));
            }
            // Simkl keeps the same refresh token (it may not send it again).
            return read_simkl_token(&answer, now, Some(refresh)).map(Some).ok_or(Error::BadAnswer);
        }
        let (id, secret) = self.trakt_keys()?;
        let (status, answer) = self
            .call(
                account.service,
                &format!("{}/oauth/token", trakt_api()),
                &[],
                Some(with_secret(
                    json!({
                        "refresh_token": refresh,
                        "client_id": id,
                        "redirect_uri": TRAKT_REDIRECT,
                        "grant_type": "refresh_token",
                    }),
                    secret,
                )),
            )
            .await?;
        if status != 200 {
            return Err(Error::Refused("Trakt"));
        }
        read_trakt_token(&answer, now).map(Some).ok_or(Error::BadAnswer)
    }

    /// A form-encoded POST to Simkl's sign-in endpoints (AUTH V2).
    async fn simkl_form(&self, path: &str, fields: &[(&str, &str)]) -> Result<(u16, Value)> {
        let body: String = fields
            .iter()
            .map(|(key, value)| format!("{}={}", form_escape(key), form_escape(value)))
            .collect::<Vec<_>>()
            .join("&");
        let url = simkl_url(path, self.simkl_key()?);
        let response = self
            .http
            .post(&url)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .await
            .map_err(|err| Error::Network(err.without_url().to_string()))?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await.map_err(|err| Error::Network(err.without_url().to_string()))?;
        if bytes.len() > ANSWER_BYTES {
            return Err(Error::BadAnswer);
        }
        Ok((status, serde_json::from_slice(&bytes).unwrap_or(Value::Null)))
    }

    /// Tells Simkl to forget the sign-in (best effort).
    pub async fn simkl_revoke(&self, account: &Account) -> Result<()> {
        let id = self.simkl_key()?;
        let _ = self.simkl_form("/oauth2/revoke", &[("client_id", id), ("token", &account.token)]).await?;
        Ok(())
    }

    async fn post(&self, account: &Account, path: &str, body: Value) -> Result<()> {
        let url = match account.service {
            Service::Trakt => format!("{}{path}", trakt_api()),
            Service::Simkl => simkl_url(path, self.simkl_key()?),
        };
        let (status, _) = self.call(account.service, &url, &self.headers(account)?, Some(body)).await?;
        if (200..300).contains(&status) { Ok(()) } else { Err(Error::Status(status)) }
    }

    /// Marks a film or an episode as watched, now.
    pub async fn mark_watched(&self, account: &Account, subject: &Subject) -> Result<()> {
        self.post(account, "/sync/history", body(subject, None)).await
    }

    /// Takes a film or episode off the account's watched history.
    pub async fn mark_unwatched(&self, account: &Account, subject: &Subject) -> Result<()> {
        self.post(account, "/sync/history/remove", body(subject, None)).await
    }

    /// Puts a film or series on the service's watchlist, or takes it off.
    /// Simkl has no way to take a title off its list without also
    /// erasing its watched history, so there a removal is not passed on.
    pub async fn set_watchlist(&self, account: &Account, subject: &Subject, on: bool) -> Result<()> {
        let whole = Subject { episode: None, ..subject.clone() };
        match (account.service, on) {
            (Service::Trakt, true) => self.post(account, "/sync/watchlist", body(&whole, None)).await,
            (Service::Trakt, false) => {
                self.post(account, "/sync/watchlist/remove", body(&whole, None)).await
            }
            (Service::Simkl, true) => {
                self.post(account, "/sync/add-to-list", body(&whole, Some("plantowatch"))).await
            }
            (Service::Simkl, false) => Ok(()),
        }
    }

    /// What the account has watched: films, and each episode of a series.
    /// Simkl: completed films, and the watched episodes of every series it
    /// has (whatever its list), with when each was watched.
    pub async fn watched(&self, account: &Account) -> Result<Vec<Watched>> {
        let headers = self.headers(account)?;
        let mut found = Vec::new();
        for series in [false, true] {
            let what = if series { "shows" } else { "movies" };
            let url = match account.service {
                Service::Trakt => format!("{}/sync/watched/{what}", trakt_api()),
                Service::Simkl if series => {
                    simkl_url("/sync/all-items/shows?extended=full&episode_watched_at=yes", self.simkl_key()?)
                }
                Service::Simkl => simkl_url("/sync/all-items/movies/completed", self.simkl_key()?),
            };
            let (status, answer) = self.call(account.service, &url, &headers, None).await?;
            if !(200..300).contains(&status) {
                return Err(Error::Status(status));
            }
            found.extend(read_watched(&answer, series));
        }
        Ok(found)
    }

    /// The films and series on the service's watchlist.
    pub async fn watchlist(&self, account: &Account) -> Result<Vec<Listed>> {
        let headers = self.headers(account)?;
        let mut listed = Vec::new();
        for series in [false, true] {
            let what = if series { "shows" } else { "movies" };
            let url = match account.service {
                Service::Trakt => format!("{}/sync/watchlist/{what}", trakt_api()),
                Service::Simkl => simkl_url(&format!("/sync/all-items/{what}/plantowatch"), self.simkl_key()?),
            };
            let (status, answer) = self.call(account.service, &url, &headers, None).await?;
            if !(200..300).contains(&status) {
                return Err(Error::Status(status));
            }
            listed.extend(read_listed(&answer, series));
        }
        Ok(listed)
    }
}

/// A Simkl address with what Simkl asks every request to carry: the
/// app's client id, name and version.
fn simkl_url(path: &str, client_id: &str) -> String {
    let joiner = if path.contains('?') { '&' } else { '?' };
    format!(
        "{SIMKL}{path}{joiner}client_id={}&app-name=haven&app-version={}",
        form_escape(client_id),
        env!("CARGO_PKG_VERSION")
    )
}

/// Percent-encodes a form or query value.
fn form_escape(text: &str) -> String {
    text.bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// A Simkl sign-in from a token answer; `refresh` is kept when the answer
/// leaves the refresh token out.
fn read_simkl_token(answer: &Value, now: i64, refresh: Option<&str>) -> Option<Account> {
    Some(Account {
        service: Service::Simkl,
        token: answer.get("access_token")?.as_str()?.to_owned(),
        refresh: answer
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| refresh.map(str::to_owned)),
        expires_at: answer.get("expires_in").and_then(Value::as_i64).map(|lasts| now + lasts),
    })
}

fn read_trakt_token(answer: &Value, now: i64) -> Option<Account> {
    Some(Account {
        service: Service::Trakt,
        token: answer.get("access_token")?.as_str()?.to_owned(),
        refresh: answer.get("refresh_token").and_then(Value::as_str).map(str::to_owned),
        expires_at: answer.get("expires_in").and_then(Value::as_i64).map(|lasts| {
            answer.get("created_at").and_then(Value::as_i64).unwrap_or(now) + lasts
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simkls_watched_answer_is_read_like_trakts() {
        let shows = json!({"shows": [{
            "status": "watching",
            "show": {"title": "A Show", "ids": {"simkl": 1, "imdb": "tt9"}},
            "seasons": [{"number": 1, "episodes": [{"number": 2, "watched_at": "2026-11-20T10:00:00Z"}]}]
        }]});
        let found = read_watched(&shows, true);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].imdb.as_str(), found[0].episode), ("tt9", Some((1, 2))));
        assert!(found[0].at > 0);
        let films = json!({"movies": [{"last_watched_at": "2026-11-20T10:00:00Z", "movie": {"title": "A Film", "ids": {"imdb": "tt5"}}}]});
        assert_eq!(read_watched(&films, false)[0].imdb, "tt5");
        assert!(read_watched(&Value::Null, true).is_empty());
    }

    #[test]
    fn simkl_addresses_carry_the_apps_name_and_version() {
        let url = simkl_url("/sync/history", "abc");
        assert!(url.starts_with("https://api.simkl.com/sync/history?client_id=abc&app-name=haven&app-version="));
        assert!(simkl_url("/x?y=1", "abc").contains("/x?y=1&client_id=abc"));
        assert_eq!(form_escape("media:read media:write"), "media%3Aread%20media%3Awrite");
    }

    #[test]
    fn a_simkl_token_keeps_its_refresh_token() {
        let fresh = json!({"access_token": "a", "refresh_token": "r", "expires_in": 604800});
        let account = read_simkl_token(&fresh, 100, None).unwrap();
        assert_eq!((account.refresh.as_deref(), account.expires_at), (Some("r"), Some(604900)));
        let renewed = read_simkl_token(&json!({"access_token": "b", "expires_in": 10}), 0, Some("r")).unwrap();
        assert_eq!(renewed.refresh.as_deref(), Some("r"));
    }

    #[test]
    fn the_pkce_challenge_matches_the_standards_example() {
        // SHA-256 of "abc" (FIPS 180-2's example), in base64url without padding.
        assert_eq!(Pkce::from_verifier("abc".to_owned()).challenge, "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0");
        // Checked against Python's hashlib and base64.
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mJ92K9XmLkKJCOnYO9bE-2xLRT2Y7Y".to_owned());
        assert_eq!(pkce.challenge, "CtX1tp2zMMpagvBDrLIq_VHHJLSnmoS3HIIISV6heU0");
        let fresh = Pkce::new();
        assert_eq!(fresh.verifier.len(), 64);
        assert_ne!(fresh.verifier, Pkce::new().verifier);
    }

    #[test]
    fn a_secret_goes_along_only_when_there_is_one() {
        assert_eq!(with_secret(json!({"a": 1}), None), json!({"a": 1}));
        assert_eq!(with_secret(json!({"a": 1}), Some("s")), json!({"a": 1, "client_secret": "s"}));
    }

    #[test]
    fn trakts_watched_list_is_read() {
        let shows = json!([{ "last_watched_at": "2026-01-02T03:04:05.000Z",
            "show": { "title": "A Show", "ids": { "imdb": "tt1" } },
            "seasons": [{ "number": 2, "episodes": [{ "number": 5, "last_watched_at": "2026-01-02T03:04:05.000Z" }] }] },
            { "show": { "title": "No id", "ids": {} }, "seasons": [] }]);
        let found = read_watched(&shows, true);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].episode, Some((2, 5)));
        assert!(found[0].at > 0);
        let films = json!([{ "movie": { "title": "F", "ids": { "imdb": "tt2" } } }]);
        assert_eq!(read_watched(&films, false)[0].episode, None);
    }

    #[test]
    fn subjects_need_an_imdb_id() {
        assert_eq!(
            subject("movie", "tt0111161", Some("tt0111161")),
            Some(Subject { imdb: "tt0111161".into(), series: false, episode: None })
        );
        assert_eq!(
            subject("series", "tt0903747", Some("tt0903747:2:5")),
            Some(Subject { imdb: "tt0903747".into(), series: true, episode: Some((2, 5)) })
        );
        // The series itself, or an episode id in another shape.
        assert_eq!(subject("series", "tt0903747", None).unwrap().episode, None);
        assert_eq!(subject("series", "tt0903747", Some("tt1:2:5")).unwrap().episode, None);
        assert_eq!(subject("series", "tt0903747", Some("tt0903747:2:5:9")).unwrap().episode, None);
        assert_eq!(subject("movie", "srv:1:abc", None), None);
        assert_eq!(subject("movie", "hv1", None), None);
    }

    #[test]
    fn bodies_name_films_series_and_episodes() {
        let film = Subject { imdb: "tt1".into(), series: false, episode: None };
        assert_eq!(body(&film, None), json!({"movies": [{"ids": {"imdb": "tt1"}}]}));
        let episode = Subject { imdb: "tt2".into(), series: true, episode: Some((1, 3)) };
        assert_eq!(
            body(&episode, None),
            json!({"shows": [{"ids": {"imdb": "tt2"},
                   "seasons": [{"number": 1, "episodes": [{"number": 3}]}]}]})
        );
        assert_eq!(
            body(&film, Some("plantowatch")),
            json!({"movies": [{"ids": {"imdb": "tt1"}, "to": "plantowatch"}]})
        );
    }

    // Made up in the shapes the two services answer with.
    #[test]
    fn watchlists_of_both_services_are_read() {
        let trakt = json!([
            {"movie": {"title": "A Film", "year": 2020, "ids": {"imdb": "tt10", "trakt": 1}}},
            {"movie": {"title": "No IMDb id", "year": 2021, "ids": {"trakt": 2}}}
        ]);
        assert_eq!(
            read_listed(&trakt, false),
            [Listed { imdb: "tt10".into(), series: false, title: "A Film".into(), year: "2020".into() }]
        );
        let simkl = json!({"shows": [{"show": {"title": "A Show", "ids": {"imdb": "tt20"}}}]});
        let shows = read_listed(&simkl, true);
        assert_eq!((shows[0].title.as_str(), shows[0].year.as_str(), shows[0].series), ("A Show", "", true));
        assert!(read_listed(&json!(null), true).is_empty());
    }

    #[test]
    fn a_trakt_token_knows_when_it_runs_out() {
        let answer = json!({"access_token": "a", "refresh_token": "r", "expires_in": 86400, "created_at": 1000});
        let account = read_trakt_token(&answer, 5).unwrap();
        assert_eq!(account.expires_at, Some(87400));
        assert_eq!(account.refresh.as_deref(), Some("r"));
        assert!(!format!("{account:?}").contains("\"a\""));
        assert_eq!(read_trakt_token(&json!({}), 5), None);
    }

    #[test]
    fn without_keys_nothing_is_asked() {
        let client = Client::new(http::client().unwrap(), Secrets::default());
        assert!(!client.has_keys(Service::Trakt));
        assert!(!client.has_keys(Service::Simkl));
        assert!(matches!(client.trakt_keys(), Err(Error::NoKey("Trakt"))));
    }
}
