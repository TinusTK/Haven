//! Whether a newer Haven has been published: reads the answer of
//! GitHub's "latest release" address and compares version numbers.

use serde_json::Value;

/// Where the latest release is asked for.
pub const LATEST: &str = "https://api.github.com/repos/TinusTK/Haven/releases/latest";

/// The newest published release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    /// Its version without the leading "v", such as "0.1.9".
    pub version: String,
    /// The release's page.
    pub page: String,
    /// The download for this kind of computer, if the release has one.
    pub download: Option<String>,
}

/// The numbers of a version such as "v0.1.10" or "0.1.10-beta".
fn numbers(version: &str) -> Vec<u64> {
    version
        .trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// True if `other` is a later version than `current`.
pub fn newer(current: &str, other: &str) -> bool {
    let (mut a, mut b) = (numbers(current), numbers(other));
    let length = a.len().max(b.len());
    a.resize(length, 0);
    b.resize(length, 0);
    b > a
}

/// What a download for `os` ("windows", "linux", "macos") ends in.
fn ending(os: &str) -> Option<&'static str> {
    match os {
        "windows" => Some("-setup.exe"),
        "linux" => Some(".deb"),
        "macos" => Some(".dmg"),
        _ => None,
    }
}

/// Reads GitHub's answer. `None` if it does not name a release.
pub fn parse_latest(answer: &Value, os: &str) -> Option<Latest> {
    let web = |value: &Value| {
        value.as_str().filter(|link| link.starts_with("https://")).map(str::to_owned)
    };
    let version = answer.get("tag_name")?.as_str()?.trim().trim_start_matches(['v', 'V']).to_owned();
    if numbers(&version).iter().all(|part| *part == 0) {
        return None;
    }
    let download = ending(os).and_then(|ending| {
        answer.get("assets")?.as_array()?.iter().find_map(|asset| {
            asset
                .get("name")?
                .as_str()
                .filter(|name| name.to_lowercase().ends_with(ending))
                .and_then(|_| web(asset.get("browser_download_url")?))
        })
    });
    Some(Latest { version, page: web(answer.get("html_url")?)?, download })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn versions_compare_by_their_numbers() {
        assert!(newer("0.1.8", "0.1.9"));
        assert!(newer("0.1.9", "v0.1.10"));
        assert!(newer("0.1.8", "0.2"));
        assert!(newer("0.9.9", "1.0.0"));
        assert!(!newer("0.1.8", "0.1.8"));
        assert!(!newer("0.1.8", "v0.1.7"));
        assert!(!newer("0.1.8", "libmpv-win-20261113"));
        assert!(!newer("0.1.8", ""));
    }

    #[test]
    fn the_latest_release_is_read_with_the_download_for_this_computer() {
        let answer = json!({
            "tag_name": "v0.1.9",
            "html_url": "https://github.com/TinusTK/Haven/releases/tag/v0.1.9",
            "assets": [
                {"name": "Haven-0.1.9-windows-x64-setup.exe", "browser_download_url": "https://github.com/x/setup.exe"},
                {"name": "haven_0.1.9-1_amd64.deb", "browser_download_url": "https://github.com/x/haven.deb"},
                {"name": "odd", "browser_download_url": "file:///x"}
            ]
        });
        let windows = parse_latest(&answer, "windows").unwrap();
        assert_eq!(windows.version, "0.1.9");
        assert_eq!(windows.download.as_deref(), Some("https://github.com/x/setup.exe"));
        assert_eq!(parse_latest(&answer, "linux").unwrap().download.as_deref(), Some("https://github.com/x/haven.deb"));
        // No Mac download in this release: the page is still known.
        let mac = parse_latest(&answer, "macos").unwrap();
        assert_eq!(mac.download, None);
        assert_eq!(mac.page, "https://github.com/TinusTK/Haven/releases/tag/v0.1.9");
        assert_eq!(parse_latest(&json!({"message": "Not Found"}), "windows"), None);
        assert_eq!(parse_latest(&json!({"tag_name": "libmpv-win", "html_url": "https://x"}), "linux"), None);
    }
}
