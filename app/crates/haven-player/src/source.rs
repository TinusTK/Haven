//! Which addresses the player may open.
//!
//! Stream addresses come from add-ons, which are other people's web
//! services. mpv itself opens far more than web addresses (local files,
//! devices, its own `edl://` and `lavfi://` sources), so anything that is
//! not plain http or https is refused before it reaches mpv.

/// The address, trimmed, if it is an http or https address the player may
/// open; `None` otherwise.
pub fn web_address(address: &str) -> Option<&str> {
    network_address(address, &["http", "https"])
}

/// Like [`web_address`], and also the streaming kinds television
/// playlists use (RTSP, RTMP, RTP, UDP, MMS). Still never a local file
/// or one of mpv's own sources.
pub fn stream_address(address: &str) -> Option<&str> {
    network_address(
        address,
        &["http", "https", "rtsp", "rtsps", "rtmp", "rtmps", "rtp", "udp", "mms", "mmsh"],
    )
}

fn network_address<'a>(address: &'a str, schemes: &[&str]) -> Option<&'a str> {
    let address = address.trim();
    let (scheme, rest) = address.split_once("://")?;
    let known = schemes.iter().any(|known| scheme.eq_ignore_ascii_case(known));
    // "@" starts a multicast address, as in udp://@239.0.0.1:1234.
    let multicast = scheme.eq_ignore_ascii_case("udp") || scheme.eq_ignore_ascii_case("rtp");
    let rest = if multicast { rest.strip_prefix('@').unwrap_or(rest) } else { rest };
    let host_starts = rest.chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '[');
    let clean = !address.chars().any(|c| c.is_control() || c.is_whitespace());
    (known && host_starts && clean).then_some(address)
}

#[cfg(test)]
mod tests {
    use super::{stream_address, web_address};

    #[test]
    fn web_addresses_pass() {
        assert_eq!(web_address("https://example.org/a.mp4"), Some("https://example.org/a.mp4"));
        assert_eq!(web_address("  HTTP://10.0.0.2:8080/live.m3u8\n"), Some("HTTP://10.0.0.2:8080/live.m3u8"));
        assert_eq!(web_address("http://[::1]/v.mkv"), Some("http://[::1]/v.mkv"));
    }

    #[test]
    fn television_addresses_pass_as_streams_only() {
        for address in ["rtsp://10.0.0.5/live", "udp://@239.0.0.1:1234", "RTMP://example.org/app/key"] {
            assert_eq!(stream_address(address), Some(address));
            assert_eq!(web_address(address), None);
        }
        assert_eq!(stream_address("https://example.org/a.m3u8"), Some("https://example.org/a.m3u8"));
        for address in ["file:///etc/passwd", "/dev/video0", "edl://x", "<html>", "https://@"] {
            assert_eq!(stream_address(address), None, "{address:?} should be refused");
        }
    }

    #[test]
    fn everything_else_is_refused() {
        for address in [
            "",
            "/etc/passwd",
            "file:///etc/passwd",
            "edl://a.mkv",
            "lavfi://testsrc",
            "av://v4l2:/dev/video0",
            "smb://server/share/a.mkv",
            "magnet:?xt=urn:btih:abc",
            "https://",
            "https:///no-host",
            "https://example.org/a b.mp4",
            "https://example.org/a\nb.mp4",
            "https://@example.org/a.mp4",
            "javascript://https://example.org",
        ] {
            assert_eq!(web_address(address), None, "{address:?} should be refused");
        }
    }
}
