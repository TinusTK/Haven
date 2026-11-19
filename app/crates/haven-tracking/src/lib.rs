//! Tracking what was watched with Trakt and Simkl: signing in with a
//! short code, marking films and episodes watched, and keeping the
//! watchlist in step. Both services know titles by their IMDb id, so only
//! titles that carry one can be tracked.
//!
//! Each service needs the app's own keys (a client id; Trakt also a
//! client secret), which come from Settings.

use haven_core::{http, secrets::Secrets};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const TRAKT_API: &str = "https://api.trakt.tv";
const TRAKT_SITE: &str = "https://trakt.tv";
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

/// Trakt's `/sync/watched/movies` or `/sync/watched/shows` answer.
pub fn read_watched(answer: &Value, series: bool) -> Vec<Watched> {
    let key = if series { "show" } else { "movie" };
    let mut found = Vec::new();
    for entry in answer.as_array().map(Vec::as_slice).unwrap_or_default() {
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
                    at: moment(episode.get("last_watched_at")),
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

impl Client {
    pub fn new(http: reqwest::Client, keys: Secrets) -> Self {
        Self { http, keys }
    }

    /// Whether the keys a service needs are set.
    pub fn has_keys(&self, service: Service) -> bool {
        match service {
            Service::Trakt => {
                self.keys.trakt_client_id.is_some() && self.keys.trakt_client_secret.is_some()
            }
            Service::Simkl => self.keys.simkl_client_id.is_some(),
        }
    }

    fn trakt_keys(&self) -> Result<(&str, &str)> {
        self.keys
            .trakt_client_id
            .as_deref()
            .zip(self.keys.trakt_client_secret.as_deref())
            .ok_or(Error::NoKey("Trakt"))
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
            Service::Simkl => {
                let id = self.simkl_key()?;
                let (status, answer) =
                    self.call(service, &format!("{SIMKL}/oauth/pin?client_id={id}"), &[], None).await?;
                if status != 200 {
                    return Err(Error::Status(status));
                }
                let user_code = field(&answer, "user_code")?;
                Ok(Code {
                    service,
                    link: field(&answer, "verification_url")?,
                    device_code: user_code.clone(),
                    user_code,
                    interval: seconds(&answer, "interval", 5).max(1),
                    expires_in: seconds(&answer, "expires_in", 900),
                })
            }
        }
    }

    /// The trakt.tv page where the user signs in and allows Haven; it
    /// sends the browser back to [`TRAKT_REDIRECT`] with a code and `state`.
    pub fn trakt_authorize_url(&self, state: &str) -> Result<String> {
        let (id, _) = self.trakt_keys()?;
        let mut url = reqwest::Url::parse(&format!("{}/oauth/authorize", trakt_site())).map_err(|_| Error::BadAnswer)?;
        url.query_pairs_mut()
            .append_pair("response_type", "code")
            .append_pair("client_id", id)
            .append_pair("redirect_uri", TRAKT_REDIRECT)
            .append_pair("state", state);
        Ok(url.to_string())
    }

    /// Trades the code trakt.tv sent back for a sign-in.
    pub async fn trakt_exchange(&self, code: &str, now: i64) -> Result<Account> {
        let (id, secret) = self.trakt_keys()?;
        let (status, answer) = self
            .call(
                Service::Trakt,
                &format!("{}/oauth/token", trakt_api()),
                &[],
                Some(json!({
                    "code": code,
                    "client_id": id,
                    "client_secret": secret,
                    "redirect_uri": TRAKT_REDIRECT,
                    "grant_type": "authorization_code",
                })),
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
                Some(json!({ "token": account.token, "client_id": id, "client_secret": secret })),
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
                        Some(json!({
                            "code": code.device_code, "client_id": id, "client_secret": secret
                        })),
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
                let url = format!("{SIMKL}/oauth/pin/{}?client_id={id}", code.device_code);
                let (status, answer) = self.call(code.service, &url, &[], None).await?;
                if status != 200 {
                    return Err(Error::Status(status));
                }
                Ok(answer.get("access_token").and_then(Value::as_str).map(|token| Account {
                    service: Service::Simkl,
                    token: token.to_owned(),
                    refresh: None,
                    expires_at: None,
                }))
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
        let (id, secret) = self.trakt_keys()?;
        let (status, answer) = self
            .call(
                account.service,
                &format!("{}/oauth/token", trakt_api()),
                &[],
                Some(json!({
                    "refresh_token": refresh,
                    "client_id": id,
                    "client_secret": secret,
                    "redirect_uri": TRAKT_REDIRECT,
                    "grant_type": "refresh_token",
                })),
            )
            .await?;
        if status != 200 {
            return Err(Error::Refused("Trakt"));
        }
        read_trakt_token(&answer, now).map(Some).ok_or(Error::BadAnswer)
    }

    async fn post(&self, account: &Account, path: &str, body: Value) -> Result<()> {
        let base = if account.service == Service::Trakt { trakt_api() } else { SIMKL.to_owned() };
        let (status, _) = self
            .call(account.service, &format!("{base}{path}"), &self.headers(account)?, Some(body))
            .await?;
        if (200..300).contains(&status) { Ok(()) } else { Err(Error::Status(status)) }
    }

    /// Marks a film or an episode as watched, now.
    pub async fn mark_watched(&self, account: &Account, subject: &Subject) -> Result<()> {
        self.post(account, "/sync/history", body(subject, None)).await
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

    /// What the account has watched (Trakt only; Simkl answers empty).
    pub async fn watched(&self, account: &Account) -> Result<Vec<Watched>> {
        if account.service != Service::Trakt {
            return Ok(Vec::new());
        }
        let headers = self.headers(account)?;
        let mut found = Vec::new();
        for series in [false, true] {
            let what = if series { "shows" } else { "movies" };
            let url = format!("{}/sync/watched/{what}", trakt_api());
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
                Service::Simkl => format!("{SIMKL}/sync/all-items/{what}/plantowatch"),
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
