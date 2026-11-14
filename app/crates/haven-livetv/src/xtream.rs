//! Xtream Codes: a provider's account (server address, user name and
//! password) instead of a playlist address. The account's live channels
//! are asked for through its `player_api.php` and turned into the same
//! playlist text an M3U source gives, so the rest of live TV does not
//! care which kind a source is.

use serde_json::Value;

use crate::m3u::Channel;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum XtreamError {
    #[error("the server's answer was not an Xtream Codes answer")]
    BadAnswer,
    #[error("the user name or password was not accepted")]
    Refused,
    #[error("the account is {0}")]
    NotActive(String),
}

/// An Xtream Codes account.
#[derive(Clone, PartialEq, Eq)]
pub struct XtreamAccount {
    /// Scheme, host and port, without a trailing slash.
    pub server: String,
    pub username: String,
    pub password: String,
}

/// Never print the password.
impl std::fmt::Debug for XtreamAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("XtreamAccount")
            .field("server", &self.server)
            .field("username", &self.username)
            .finish_non_exhaustive()
    }
}

/// What the server says about the account when signing in.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct XtreamInfo {
    /// When the account runs out, in seconds since 1970; `None` if never.
    pub expires: Option<i64>,
    /// The stream formats the account may use ("ts", "m3u8"…).
    pub formats: Vec<String>,
}

fn encoded(part: &str) -> String {
    let mut out = String::with_capacity(part.len());
    for byte in part.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn decoded(part: &str) -> String {
    let bytes = part.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        let hex = (bytes[at] == b'%')
            .then(|| part.get(at + 1..at + 3))
            .flatten()
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                at += 3;
            }
            None => {
                out.push(if bytes[at] == b'+' { b' ' } else { bytes[at] });
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl XtreamAccount {
    /// Tidies what the user typed. The server may be given with or
    /// without `http://`, and with a path (a pasted playlist or API
    /// address): only the scheme, host and port are kept. `None` if any
    /// of the three parts is missing or the server is not a web address.
    pub fn new(server: &str, username: &str, password: &str) -> Option<Self> {
        let (server, username, password) = (server.trim(), username.trim(), password.trim());
        let (scheme, rest) = match server.split_once("://") {
            Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
            None => ("http".to_owned(), server),
        };
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        // "user:password@" in front of the host is not part of it.
        let host = host.rsplit('@').next().unwrap_or(host);
        let fine = !host.is_empty()
            && !host.chars().any(char::is_whitespace)
            && matches!(scheme.as_str(), "http" | "https")
            && !username.is_empty()
            && !password.is_empty();
        fine.then(|| Self {
            server: format!("{scheme}://{host}"),
            username: username.to_owned(),
            password: password.to_owned(),
        })
    }

    /// The address to ask: with no action, about the account itself.
    pub fn api(&self, action: Option<&str>) -> String {
        let mut url = format!(
            "{}/player_api.php?username={}&password={}",
            self.server,
            encoded(&self.username),
            encoded(&self.password)
        );
        if let Some(action) = action {
            url.push_str(&format!("&action={}", encoded(action)));
        }
        url
    }

    /// The address Haven stores the account under, as a source.
    pub fn source_url(&self) -> String {
        self.api(None)
    }

    /// Reads an account back from the address it was stored under;
    /// `None` for any other address (an M3U playlist's).
    pub fn from_source_url(url: &str) -> Option<Self> {
        let (server, query) = url.split_once("/player_api.php?")?;
        let value = |name: &str| {
            query
                .split('&')
                .filter_map(|pair| pair.split_once('='))
                .find(|(key, _)| *key == name)
                .map(|(_, value)| decoded(value))
        };
        if query.split('&').any(|pair| pair.starts_with("action=")) {
            return None;
        }
        Self::new(server, &value("username")?, &value("password")?)
    }

    /// How the account is shown in lists: the user name and the host,
    /// never the password.
    pub fn label(&self) -> String {
        format!("{} at {}", self.username, crate::source_name(&self.server))
    }

    /// Where one live channel plays from.
    fn stream(&self, id: &str, format: &str) -> String {
        format!(
            "{}/live/{}/{}/{}.{format}",
            self.server,
            encoded(&self.username),
            encoded(&self.password),
            encoded(id)
        )
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    match value.get(key)? {
        Value::String(text) if !text.trim().is_empty() => Some(text.trim().to_owned()),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// Reads the answer to signing in (the API asked with no action).
pub fn xtream_sign_in(json: &str) -> Result<XtreamInfo, XtreamError> {
    let answer: Value = serde_json::from_str(json).map_err(|_| XtreamError::BadAnswer)?;
    let user = answer.get("user_info").filter(|user| user.is_object()).ok_or(XtreamError::BadAnswer)?;
    if text(user, "auth").as_deref() != Some("1") {
        return Err(XtreamError::Refused);
    }
    if let Some(status) = text(user, "status").filter(|status| !status.eq_ignore_ascii_case("active")) {
        return Err(XtreamError::NotActive(status.to_lowercase()));
    }
    Ok(XtreamInfo {
        expires: text(user, "exp_date").and_then(|date| date.parse().ok()),
        formats: user
            .get("allowed_output_formats")
            .and_then(Value::as_array)
            .map(|formats| formats.iter().filter_map(|f| f.as_str().map(str::to_lowercase)).collect())
            .unwrap_or_default(),
    })
}

/// The account's live channels, from its answers to `get_live_categories`
/// and `get_live_streams`. Channels without an id or a name are left out;
/// a missing or unreadable category list only costs the group names.
pub fn xtream_channels(
    account: &XtreamAccount,
    info: &XtreamInfo,
    categories_json: &str,
    streams_json: &str,
) -> Result<Vec<Channel>, XtreamError> {
    let streams: Value = serde_json::from_str(streams_json).map_err(|_| XtreamError::BadAnswer)?;
    let streams = streams.as_array().ok_or(XtreamError::BadAnswer)?;
    let categories: Vec<(String, String)> = serde_json::from_str::<Value>(categories_json)
        .ok()
        .and_then(|categories| categories.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|category| Some((text(category, "category_id")?, text(category, "category_name")?)))
        .collect();
    // The plain transport stream unless the account is only let have HLS.
    let only_hls = !info.formats.is_empty()
        && !info.formats.iter().any(|format| format == "ts")
        && info.formats.iter().any(|format| format == "m3u8");
    let format = if only_hls { "m3u8" } else { "ts" };
    Ok(streams
        .iter()
        .filter_map(|stream| {
            let id = text(stream, "stream_id")?;
            let category = text(stream, "category_id");
            Some(Channel {
                name: text(stream, "name")?,
                address: account.stream(&id, format),
                group: category.and_then(|id| {
                    categories.iter().find(|(known, _)| *known == id).map(|(_, name)| name.clone())
                }),
                logo: text(stream, "stream_icon"),
                guide_id: text(stream, "epg_channel_id"),
                radio: text(stream, "stream_type").is_some_and(|kind| kind.contains("radio")),
            })
        })
        .collect())
}

/// Writes channels as playlist text that [`crate::parse_m3u`] reads back
/// the same.
pub fn to_m3u(channels: &[Channel]) -> String {
    // Quotes and line ends would break a line apart; a `;` splits groups.
    let value = |text: &str| -> String {
        text.chars().map(|c| if c == '"' || c.is_control() { ' ' } else { c }).collect::<String>().trim().to_owned()
    };
    let mut out = String::from("#EXTM3U\n");
    for channel in channels {
        out.push_str("#EXTINF:-1");
        if let Some(id) = &channel.guide_id {
            out.push_str(&format!(" tvg-id=\"{}\"", value(id)));
        }
        if let Some(logo) = &channel.logo {
            out.push_str(&format!(" tvg-logo=\"{}\"", value(logo)));
        }
        if let Some(group) = &channel.group {
            out.push_str(&format!(" group-title=\"{}\"", value(group).replace(';', ",")));
        }
        if channel.radio {
            out.push_str(" radio=\"true\"");
        }
        let name: String = channel.name.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
        out.push_str(&format!(",{}\n{}\n", name.trim(), channel.address.trim()));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_m3u;

    fn account() -> XtreamAccount {
        XtreamAccount::new("tv.example.org:8080", "me", "p&ss word").unwrap()
    }

    #[test]
    fn the_typed_server_is_tidied() {
        assert_eq!(account().server, "http://tv.example.org:8080");
        let pasted = XtreamAccount::new(
            " HTTPS://tv.example.org/get.php?username=x&password=y&type=m3u_plus ",
            " me ",
            " secret ",
        )
        .unwrap();
        assert_eq!((pasted.server.as_str(), pasted.username.as_str(), pasted.password.as_str()),
            ("https://tv.example.org", "me", "secret"));
        for (server, user, password) in
            [("", "me", "x"), ("tv.example.org", "", "x"), ("tv.example.org", "me", ""), ("ftp://tv.example.org", "me", "x")]
        {
            assert!(XtreamAccount::new(server, user, password).is_none(), "{server}");
        }
    }

    #[test]
    fn an_account_survives_being_stored_as_an_address() {
        let account = account();
        let url = account.source_url();
        assert_eq!(url, "http://tv.example.org:8080/player_api.php?username=me&password=p%26ss%20word");
        assert_eq!(XtreamAccount::from_source_url(&url), Some(account.clone()));
        assert_eq!(account.api(Some("get_live_streams")), format!("{url}&action=get_live_streams"));
        // A playlist address, or an API address with an action, is not an account.
        assert_eq!(XtreamAccount::from_source_url("http://tv.example.org/get.php?username=a&password=b"), None);
        assert_eq!(XtreamAccount::from_source_url(&account.api(Some("x"))), None);
        assert_eq!(account.label(), "me at tv.example.org");
        assert!(!format!("{account:?}").contains("ss word"));
    }

    // Made up in the shape Xtream Codes servers answer with.
    #[test]
    fn signing_in_reads_the_account_state() {
        let fine = r#"{"user_info": {"auth": 1, "status": "Active", "exp_date": "1900000000",
            "allowed_output_formats": ["m3u8", "ts"]}, "server_info": {}}"#;
        assert_eq!(
            xtream_sign_in(fine),
            Ok(XtreamInfo { expires: Some(1_900_000_000), formats: vec!["m3u8".into(), "ts".into()] })
        );
        assert_eq!(xtream_sign_in(r#"{"user_info": {"auth": 0}}"#), Err(XtreamError::Refused));
        assert_eq!(
            xtream_sign_in(r#"{"user_info": {"auth": "1", "status": "Expired"}}"#),
            Err(XtreamError::NotActive("expired".into()))
        );
        assert_eq!(xtream_sign_in("<html>"), Err(XtreamError::BadAnswer));
        assert_eq!(xtream_sign_in("[]"), Err(XtreamError::BadAnswer));
        assert_eq!(xtream_sign_in(r#"{"user_info": {"auth": 1, "exp_date": null}}"#), Ok(XtreamInfo::default()));
    }

    #[test]
    fn channels_come_with_groups_and_survive_the_playlist_form() {
        let categories = r#"[{"category_id": "1", "category_name": "News; World"}, {"category_id": "2", "category_name": "Music"}]"#;
        let streams = r#"[
            {"num": 1, "name": "Channel \"One\"", "stream_type": "live", "stream_id": 101,
             "stream_icon": "http://tv.example.org/one.png", "epg_channel_id": "one.example", "category_id": "1"},
            {"name": "Radio Two", "stream_type": "radio_streams", "stream_id": "202", "category_id": 2},
            {"name": "No category", "stream_id": 303, "category_id": "9", "stream_icon": ""},
            {"name": "No id"},
            {"stream_id": 404}
        ]"#;
        let info = XtreamInfo::default();
        let channels = xtream_channels(&account(), &info, categories, streams).unwrap();
        assert_eq!(channels.len(), 3);
        assert_eq!(channels[0].address, "http://tv.example.org:8080/live/me/p%26ss%20word/101.ts");
        assert_eq!(channels[0].group.as_deref(), Some("News; World"));
        assert_eq!(channels[0].guide_id.as_deref(), Some("one.example"));
        assert!(channels[1].radio && !channels[0].radio);
        assert_eq!(channels[1].group.as_deref(), Some("Music"));
        assert_eq!((channels[2].group.as_deref(), channels[2].logo.as_deref()), (None, None));

        let read_back = parse_m3u(&to_m3u(&channels));
        assert_eq!(read_back.len(), 3);
        assert_eq!(read_back[0].name, "Channel \"One\"");
        assert_eq!(read_back[0].address, channels[0].address);
        assert_eq!(read_back[0].group.as_deref(), Some("News, World"));
        assert_eq!(read_back[0].logo, channels[0].logo);
        assert!(read_back[1].radio);
        assert_eq!(read_back[2], channels[2]);

        // No category list still gives the channels.
        assert_eq!(xtream_channels(&account(), &info, "oops", streams).unwrap().len(), 3);
        assert_eq!(xtream_channels(&account(), &info, categories, "{}"), Err(XtreamError::BadAnswer));

        let hls = XtreamInfo { expires: None, formats: vec!["m3u8".into()] };
        assert!(xtream_channels(&account(), &hls, categories, streams).unwrap()[0].address.ends_with("/101.m3u8"));
    }
}
