//! Live TV and radio: reading channel lists.
//!
//! A source is the address of an M3U playlist (the usual "M3U Plus" kind:
//! one `#EXTINF` line of details, then the channel's address). Entries
//! marked `radio="true"` are radio stations; everything else is television.

mod m3u;

pub use m3u::{Channel, parse_m3u};

/// The channels' groups, in the order they first appear, without repeats.
pub fn groups(channels: &[Channel]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for group in channels.iter().filter_map(|channel| channel.group.as_deref()) {
        if !seen.iter().any(|known| known == group) {
            seen.push(group.to_owned());
        }
    }
    seen
}

/// Whether a channel passes the filter: in `group` (when one is chosen)
/// and with every word of `words` somewhere in its name, in any case.
pub fn matches(channel: &Channel, group: Option<&str>, words: &str) -> bool {
    if group.is_some_and(|group| channel.group.as_deref() != Some(group)) {
        return false;
    }
    let name = channel.name.to_lowercase();
    words.split_whitespace().all(|word| name.contains(&word.to_lowercase()))
}

/// A short name for a source, from its address: the host.
pub fn source_name(address: &str) -> String {
    let rest = address.split_once("://").map_or(address, |(_, rest)| rest);
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    // Drop "user:password@" and the port.
    let host = host.rsplit('@').next().unwrap_or(host);
    let host = if host.starts_with('[') {
        host.split_inclusive(']').next().unwrap_or(host)
    } else {
        host.split(':').next().unwrap_or(host)
    };
    if host.is_empty() { address.to_owned() } else { host.to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(name: &str, group: Option<&str>) -> Channel {
        Channel {
            name: name.into(),
            address: "http://example.org/a".into(),
            group: group.map(Into::into),
            logo: None,
            guide_id: None,
            radio: false,
        }
    }

    #[test]
    fn groups_keep_their_first_order() {
        let channels = [
            channel("A", Some("News")),
            channel("B", None),
            channel("C", Some("Sport")),
            channel("D", Some("News")),
        ];
        assert_eq!(groups(&channels), ["News", "Sport"]);
    }

    #[test]
    fn filter_takes_a_group_and_words_in_any_case() {
        let one = channel("BBC World News HD", Some("News"));
        assert!(matches(&one, None, ""));
        assert!(matches(&one, Some("News"), "world bbc"));
        assert!(!matches(&one, Some("Sport"), ""));
        assert!(!matches(&one, None, "cnn"));
        assert!(!matches(&channel("X", None), Some("News"), ""));
    }

    #[test]
    fn a_source_is_named_after_its_host() {
        assert_eq!(source_name("https://tv.example.org/lists/all.m3u"), "tv.example.org");
        assert_eq!(source_name("http://user:secret@10.0.0.2:8080/get.php?x=1"), "10.0.0.2");
        assert_eq!(source_name("http://[::1]:80/a"), "[::1]");
        assert_eq!(source_name("nonsense"), "nonsense");
    }
}
