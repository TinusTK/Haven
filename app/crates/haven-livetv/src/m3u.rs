/// One channel or station from a playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Channel {
    pub name: String,
    /// Where it plays from.
    pub address: String,
    /// `group-title`, or the older `#EXTGRP` line.
    pub group: Option<String>,
    /// Address of its logo (`tvg-logo`).
    pub logo: Option<String>,
    /// Its id in a programme guide (`tvg-id`).
    pub guide_id: Option<String>,
    /// Marked `radio="true"`.
    pub radio: bool,
}

/// Reads an M3U playlist. Lenient: a missing `#EXTM3U` header, blank
/// lines, Windows line ends and unknown `#` lines are all fine; an entry
/// without an address is left out, and an address without an `#EXTINF`
/// line is named after its last path segment.
pub fn parse_m3u(text: &str) -> Vec<Channel> {
    let mut channels = Vec::new();
    let mut pending: Option<Channel> = None;
    // Some files start with a byte-order mark.
    for line in text.trim_start_matches('\u{feff}').lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(info) = strip_prefix_ignore_case(line, "#EXTINF:") {
            pending = Some(read_info(info));
        } else if let Some(group) = strip_prefix_ignore_case(line, "#EXTGRP:") {
            if let Some(channel) = pending.as_mut() {
                if channel.group.is_none() {
                    channel.group = text_of(group);
                }
            }
        } else if line.starts_with('#') {
            // #EXTM3U, #EXTVLCOPT and anything else: not needed.
        } else {
            let mut channel = pending.take().unwrap_or_else(|| Channel {
                name: String::new(),
                address: String::new(),
                group: None,
                logo: None,
                guide_id: None,
                radio: false,
            });
            channel.address = line.to_owned();
            if channel.name.is_empty() {
                channel.name = name_from_address(line);
            }
            channels.push(channel);
        }
    }
    channels
}

fn strip_prefix_ignore_case<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let head = line.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &line[prefix.len()..])
}

fn text_of(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn name_from_address(address: &str) -> String {
    let path = address.split(['?', '#']).next().unwrap_or(address);
    let last = path.trim_end_matches('/').rsplit('/').next().unwrap_or(path);
    if last.is_empty() { address.to_owned() } else { last.to_owned() }
}

/// Reads what follows `#EXTINF:`: a duration, `key="value"` pairs, then
/// a comma and the channel's name.
fn read_info(info: &str) -> Channel {
    // The name starts after the first comma that is not inside quotes.
    let mut in_quotes = false;
    let mut split = info.len();
    for (at, c) in info.char_indices() {
        match c {
            '"' => in_quotes = !in_quotes,
            ',' if !in_quotes => {
                split = at;
                break;
            }
            _ => {}
        }
    }
    let (details, name) = info.split_at(split);
    let name = name.strip_prefix(',').unwrap_or(name);

    let attribute = |key: &str| -> Option<String> {
        let mut rest = details;
        // Find `key="` at a word start, in any case.
        while let Some(at) = find_ignore_case(rest, key) {
            let before_ok = rest[..at].chars().next_back().is_none_or(|c| c.is_whitespace());
            let after = &rest[at + key.len()..];
            if before_ok {
                if let Some(value) = after.strip_prefix("=\"") {
                    return value.split('"').next().and_then(text_of);
                }
            }
            rest = after;
        }
        None
    };

    Channel {
        name: text_of(name).or_else(|| attribute("tvg-name")).unwrap_or_default(),
        address: String::new(),
        // Several groups may be listed with ';': the first is used.
        group: attribute("group-title")
            .and_then(|groups| groups.split(';').next().and_then(text_of)),
        logo: attribute("tvg-logo"),
        guide_id: attribute("tvg-id"),
        radio: attribute("radio").is_some_and(|value| value.eq_ignore_ascii_case("true")),
    }
}

fn find_ignore_case(text: &str, needle: &str) -> Option<usize> {
    let (text_lower, needle_lower) = (text.to_ascii_lowercase(), needle.to_ascii_lowercase());
    text_lower.find(&needle_lower)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Made up for these tests.
    const SAMPLE: &str = "\u{feff}#EXTM3U url-tvg=\"http://example.org/guide.xml\"\r\n\
#EXTINF:-1 tvg-id=\"one.example\" tvg-logo=\"http://example.org/one.png\" group-title=\"News\",Channel One\r\n\
#EXTVLCOPT:http-user-agent=Example\r\n\
http://example.org/one.m3u8\r\n\
\r\n\
#EXTINF:-1 group-title=\"Sport;Local\" tvg-name=\"Unused\",Two, the \"Second\"\n\
http://example.org/two.ts\n\
#extinf:0 radio=\"true\" Group-Title=\"Music\",Radio Three\n\
#EXTGRP:Ignored because a group is set\n\
http://example.org/three.mp3\n\
#EXTINF:-1,Four\n\
#EXTGRP:Old Style\n\
rtsp://example.org/four\n\
#EXTINF:-1 tvg-name=\"Named By Attribute\",\n\
http://example.org/five\n\
#EXTINF:-1,No address follows\n\
#EXTINF:-1 tvg-logo=\"\",Six\n\
http://example.org/six\n\
http://example.org/path/seven.m3u8?token=1\n";

    #[test]
    fn reads_channels_with_their_details() {
        let channels = parse_m3u(SAMPLE);
        let names: Vec<&str> = channels.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["Channel One", "Two, the \"Second\"", "Radio Three", "Four", "Named By Attribute", "Six", "seven.m3u8"]
        );

        let one = &channels[0];
        assert_eq!(one.address, "http://example.org/one.m3u8");
        assert_eq!(one.group.as_deref(), Some("News"));
        assert_eq!(one.logo.as_deref(), Some("http://example.org/one.png"));
        assert_eq!(one.guide_id.as_deref(), Some("one.example"));
        assert!(!one.radio);

        assert_eq!(channels[1].group.as_deref(), Some("Sport"));
        assert!(channels[2].radio);
        assert_eq!(channels[2].group.as_deref(), Some("Music"));
        assert_eq!(channels[3].group.as_deref(), Some("Old Style"));
        assert_eq!(channels[3].address, "rtsp://example.org/four");
        assert_eq!(channels[5].logo, None);
        assert_eq!(channels[6].group, None);
    }

    #[test]
    fn text_that_is_not_a_playlist_gives_what_it_can() {
        assert!(parse_m3u("").is_empty());
        assert!(parse_m3u("#EXTM3U\n#EXTINF:-1,Alone\n").is_empty());
        // A web page instead of a playlist: lines are taken as addresses,
        // which the app then refuses as unplayable.
        assert_eq!(parse_m3u("<html>").len(), 1);
    }
}
