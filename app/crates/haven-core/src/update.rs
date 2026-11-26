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
    pub download: Option<Download>,
    /// What is new in it: the release's notes as plain text.
    pub notes: String,
}

/// A file of a release that installs Haven on this kind of computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Download {
    /// Its file name, such as "Haven-0.1.9-windows-x64-setup.exe".
    pub name: String,
    pub address: String,
    /// Its SHA-256 in hex as GitHub states it, to check the copy by.
    pub sha256: Option<String>,
    pub size: u64,
}

/// Only files of Haven's own releases are ever fetched and run.
const DOWNLOADS: &str = "https://github.com/TinusTK/Haven/releases/download/";

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

/// The build of a version: "0.2.6-build3" is build 3. A release with no
/// build named ("v0.2.6") is the version's first build.
fn build_of(version: &str) -> u64 {
    let lower = version.trim().to_ascii_lowercase();
    lower
        .rsplit_once("-build")
        .and_then(|(_, number)| number.trim().parse().ok())
        .unwrap_or(1)
}

/// True if `other` is a later version than `current`. Releases of one
/// version go by build ("v0.2.6-build4" is newer than "0.2.6-build3"),
/// since the version is held while builds come out.
pub fn newer(current: &str, other: &str) -> bool {
    let (mut a, mut b) = (numbers(current), numbers(other));
    let length = a.len().max(b.len());
    a.resize(length, 0);
    b.resize(length, 0);
    if b != a {
        return b > a;
    }
    build_of(other) > build_of(current)
}

/// A version as Haven shows it: "0.2.6-build3" reads "0.2.6 build 3".
pub fn shown(version: &str) -> String {
    let lower = version.to_ascii_lowercase();
    match lower.rfind("-build") {
        Some(at) => format!("{} build {}", &version[..at], &version[at + 6..]),
        None => version.to_owned(),
    }
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
            let name = asset.get("name")?.as_str()?;
            // A plain file name: it becomes a file on this computer.
            let plain = !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
                && !name.starts_with('.');
            if !plain || !name.to_lowercase().ends_with(ending) {
                return None;
            }
            let address = web(asset.get("browser_download_url")?)
                .filter(|address| address.starts_with(DOWNLOADS))?;
            let sha256 = asset
                .get("digest")
                .and_then(Value::as_str)
                .and_then(|digest| digest.strip_prefix("sha256:"))
                .filter(|hex| hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
                .map(str::to_lowercase);
            Some(Download {
                name: name.to_owned(),
                address,
                sha256,
                size: asset.get("size").and_then(Value::as_u64).unwrap_or(0),
            })
        })
    });
    let notes = answer.get("body").and_then(Value::as_str).map(plain_notes).unwrap_or_default();
    Some(Latest { version, page: web(answer.get("html_url")?)?, download, notes })
}

/// The longest release notes shown; the rest is on the release's page.
const MOST_NOTES: usize = 4000;

/// A release's notes (Markdown) as plain text: list marks become
/// bullets, heading marks, bold and code marks go, and a link keeps
/// only its words.
pub fn plain_notes(markdown: &str) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in markdown.replace("\r\n", "\n").lines() {
        let trimmed = line.trim_end();
        let indent = trimmed.len() - trimmed.trim_start().len();
        let mut text = trimmed.trim_start();
        let mut bullet = false;
        if let Some(rest) = text.strip_prefix("- ").or_else(|| text.strip_prefix("* ")).or_else(|| text.strip_prefix("+ ")) {
            text = rest;
            bullet = true;
        }
        let text = text.trim_start_matches('#').trim_start();
        let mut out = String::new();
        let mut rest = text;
        // [words](address) keeps the words.
        while let Some(open) = rest.find('[') {
            let after = &rest[open + 1..];
            match after.find("](").and_then(|close| after[close + 2..].find(')').map(|end| (close, close + 2 + end))) {
                Some((close, end)) => {
                    out.push_str(&rest[..open]);
                    out.push_str(&after[..close]);
                    rest = &after[end + 1..];
                }
                None => break,
            }
        }
        out.push_str(rest);
        let out = out.replace("**", "").replace("__", "").replace('`', "");
        if out.is_empty() {
            if lines.last().is_some_and(|last| !last.is_empty()) {
                lines.push(String::new());
            }
            continue;
        }
        let pad = if indent >= 2 { "    " } else { "" };
        lines.push(if bullet { format!("{pad}•  {out}") } else { format!("{pad}{out}") });
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    let mut notes = lines.join("\n");
    if notes.chars().count() > MOST_NOTES {
        notes = notes.chars().take(MOST_NOTES).collect::<String>().trim_end().to_owned() + "…";
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn release_notes_read_as_plain_text() {
        let notes = plain_notes(
            "## What's new\r\n\r\n- **Calendar**: dates in `local` time.\n  * See [the guide](https://example.com/x).\n\n\n\nThanks!\n",
        );
        assert_eq!(notes, "What's new\n\n•  Calendar: dates in local time.\n    •  See the guide.\n\nThanks!");
        assert_eq!(plain_notes("[broken link"), "[broken link");
        assert_eq!(plain_notes(""), "");
    }

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
        // Builds of a held version (tags v0.2.6, v0.2.6-build2, …).
        assert!(newer("0.2.6-build3", "v0.2.6-build4"));
        assert!(newer("0.2.6", "v0.2.6-build2"));
        assert!(!newer("0.2.6-build3", "v0.2.6-build3"));
        assert!(!newer("0.2.6-build3", "v0.2.6-build2"));
        assert!(!newer("0.2.6-build3", "v0.2.6"));
        assert!(newer("0.2.6-build9", "v0.2.7"));
        assert!(!newer("0.2.7", "v0.2.6-build9"));
        assert_eq!(shown("0.2.6-build4"), "0.2.6 build 4");
        assert_eq!(shown("0.2.6"), "0.2.6");
    }

    #[test]
    fn the_latest_release_is_read_with_the_download_for_this_computer() {
        let base = "https://github.com/TinusTK/Haven/releases/download/v0.1.9/";
        let sum = "9a6b8b49a8a3374019d73eab255bdb7cd489fbd1536a7f43d63cb81e675688d0";
        let answer = json!({
            "tag_name": "v0.1.9",
            "html_url": "https://github.com/TinusTK/Haven/releases/tag/v0.1.9",
            "assets": [
                {"name": "Haven-0.1.9-windows-x64-setup.exe", "size": 44652553,
                 "digest": format!("sha256:{sum}"),
                 "browser_download_url": format!("{base}Haven-0.1.9-windows-x64-setup.exe")},
                {"name": "haven_0.1.9-1_amd64.deb", "browser_download_url": format!("{base}haven_0.1.9-1_amd64.deb")},
                {"name": "Haven-0.1.9.dmg", "browser_download_url": "https://elsewhere.example/Haven-0.1.9.dmg"}
            ]
        });
        let windows = parse_latest(&answer, "windows").unwrap();
        assert_eq!(windows.version, "0.1.9");
        let download = windows.download.unwrap();
        assert_eq!(download.name, "Haven-0.1.9-windows-x64-setup.exe");
        assert_eq!(download.sha256.as_deref(), Some(sum));
        assert_eq!(download.size, 44652553);
        let linux = parse_latest(&answer, "linux").unwrap().download.unwrap();
        assert_eq!(linux.address, format!("{base}haven_0.1.9-1_amd64.deb"));
        assert_eq!(linux.sha256, None);
        // A file kept anywhere but Haven's own releases is not taken.
        let mac = parse_latest(&answer, "macos").unwrap();
        assert_eq!(mac.download, None);
        assert_eq!(mac.page, "https://github.com/TinusTK/Haven/releases/tag/v0.1.9");
        // Nor one whose name is not a plain file name.
        let odd = json!({"tag_name": "v0.1.9", "html_url": "https://github.com/x", "assets": [
            {"name": "../evil-setup.exe", "browser_download_url": format!("{base}evil-setup.exe")}]});
        assert_eq!(parse_latest(&odd, "windows").unwrap().download, None);
        assert_eq!(parse_latest(&json!({"message": "Not Found"}), "windows"), None);
        assert_eq!(parse_latest(&json!({"tag_name": "libmpv-win", "html_url": "https://x"}), "linux"), None);
    }
}
