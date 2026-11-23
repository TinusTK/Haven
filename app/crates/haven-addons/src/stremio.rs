//! A person's Stremio account: signing in, the list of services the
//! account uses (each with its address and manifest), and signing out.
//! Haven then asks those services itself, exactly as Stremio does; the
//! account only holds the list.
//!
//! Stremio's API answers every call with `{"result": …}` or
//! `{"error": {"message": …}}`.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const API: &str = "https://api.strem.io/api";
/// The most an answer from Stremio may hold.
const MOST_BYTES: usize = 8 * 1024 * 1024;

/// A signed-in Stremio account, as Haven keeps it. Never print `auth_key`.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub email: String,
    pub auth_key: String,
}

impl std::fmt::Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account").field("email", &self.email).finish_non_exhaustive()
    }
}

/// One service on the account: its address and its manifest (raw JSON).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    pub url: String,
    pub manifest: String,
}

#[derive(Debug, thiserror::Error)]
pub enum StremioError {
    /// What Stremio said, e.g. a wrong password.
    #[error("{0}")]
    Refused(String),
    #[error("Stremio could not be reached")]
    Unreachable,
    #[error("Stremio's answer could not be read")]
    BadAnswer,
}

/// Signs in with an email address and password.
pub async fn login(http: &reqwest::Client, email: &str, password: &str) -> Result<Account, StremioError> {
    let body = json!({"type": "Login", "email": email.trim(), "password": password, "facebook": false});
    parse_login(&call(http, "login", body).await?)
}

/// The services on the account, in the account's order.
pub async fn listed(http: &reqwest::Client, account: &Account) -> Result<Vec<Listed>, StremioError> {
    let body = json!({"type": "AddonCollectionGet", "authKey": account.auth_key, "update": true});
    parse_listed(&call(http, "addonCollectionGet", body).await?)
}

/// Ends the sign-in on Stremio's side (best effort).
pub async fn logout(http: &reqwest::Client, account: &Account) {
    let _ = call(http, "logout", json!({"type": "Logout", "authKey": account.auth_key})).await;
}

async fn call(http: &reqwest::Client, method: &str, body: Value) -> Result<Value, StremioError> {
    let (_, bytes) = haven_core::http::send(http, &format!("{API}/{method}"), &[], Some(body.to_string()), MOST_BYTES)
        .await
        .map_err(|_| StremioError::Unreachable)?;
    let answer: Value = serde_json::from_slice(&bytes).map_err(|_| StremioError::BadAnswer)?;
    if let Some(error) = answer.get("error").filter(|e| !e.is_null()) {
        let message = error.get("message").and_then(Value::as_str).unwrap_or("Stremio said no");
        return Err(StremioError::Refused(message.to_owned()));
    }
    answer.get("result").cloned().ok_or(StremioError::BadAnswer)
}

fn parse_login(result: &Value) -> Result<Account, StremioError> {
    let auth_key = result.get("authKey").and_then(Value::as_str).filter(|k| !k.is_empty()).ok_or(StremioError::BadAnswer)?;
    let email = result.pointer("/user/email").and_then(Value::as_str).unwrap_or_default();
    Ok(Account { email: email.to_owned(), auth_key: auth_key.to_owned() })
}

fn parse_listed(result: &Value) -> Result<Vec<Listed>, StremioError> {
    let list = result.get("addons").and_then(Value::as_array).ok_or(StremioError::BadAnswer)?;
    Ok(list
        .iter()
        .filter_map(|entry| {
            let url = entry.get("transportUrl")?.as_str()?.trim().to_owned();
            let manifest = entry.get("manifest").filter(|m| m.is_object())?;
            wanted(&url).then(|| Listed { url, manifest: manifest.to_string() })
        })
        .collect())
}

/// Leaves out Cinemeta (built into Haven) and services that only run on
/// the computer Stremio itself runs on.
fn wanted(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else { return false };
    let local = matches!(parsed.host_str(), Some("127.0.0.1" | "localhost" | "::1" | "[::1]"));
    !local && matches!(parsed.scheme(), "http" | "https") && url != haven_titles::CINEMETA_URL
}

/// What to add and remove so the installed list (`have`, addresses)
/// matches the account's (`wanted`).
pub fn changes<'a>(have: &[String], wanted: &'a [Listed]) -> (Vec<&'a Listed>, Vec<String>) {
    let add = wanted.iter().filter(|w| !have.contains(&w.url)).collect();
    let remove = have.iter().filter(|h| !wanted.iter().any(|w| &w.url == *h)).cloned().collect();
    (add, remove)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sign_in_gives_the_key_and_email() {
        let result = json!({"authKey": "k1", "user": {"_id": "u", "email": "a@b.c"}});
        let account = parse_login(&result).unwrap();
        assert_eq!((account.email.as_str(), account.auth_key.as_str()), ("a@b.c", "k1"));
        assert!(!format!("{account:?}").contains("k1"));
        assert!(parse_login(&json!({"user": {}})).is_err());
    }

    #[test]
    fn the_list_leaves_out_what_haven_has_or_cannot_reach() {
        let result = json!({"addons": [
            {"transportUrl": haven_titles::CINEMETA_URL, "manifest": {"id": "com.linvo.cinemeta"}},
            {"transportUrl": "http://127.0.0.1:11470/local-addon/manifest.json", "manifest": {"id": "local"}},
            {"transportUrl": "https://aiostreams.example/u/manifest.json", "manifest": {"id": "aio", "name": "AIO"}},
            {"transportUrl": "https://broken.example/manifest.json"}
        ]});
        let listed = parse_listed(&result).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].url, "https://aiostreams.example/u/manifest.json");
        assert!(listed[0].manifest.contains("\"AIO\""));
    }

    #[test]
    fn changes_add_new_and_remove_gone() {
        let wanted = vec![
            Listed { url: "https://a/manifest.json".into(), manifest: "{}".into() },
            Listed { url: "https://b/manifest.json".into(), manifest: "{}".into() },
        ];
        let have = vec!["https://b/manifest.json".to_owned(), "https://old/manifest.json".to_owned()];
        let (add, remove) = changes(&have, &wanted);
        assert_eq!(add.iter().map(|l| l.url.as_str()).collect::<Vec<_>>(), ["https://a/manifest.json"]);
        assert_eq!(remove, ["https://old/manifest.json"]);
    }
}
