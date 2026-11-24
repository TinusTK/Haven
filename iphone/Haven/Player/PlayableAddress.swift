import Foundation

/// Which addresses the player may open.
///
/// Stream addresses come from other people's services. mpv opens far more
/// than web addresses (local files, devices, its own `edl://` and
/// `lavfi://` sources), so anything that is not plain http or https is
/// refused before it reaches mpv; television playlists may also use the
/// streaming kinds (RTSP, RTMP, RTP, UDP, MMS).
enum PlayableAddress {
    /// The address, trimmed, if it is an http or https address.
    static func web(_ address: String) -> String? {
        network(address, schemes: ["http", "https"])
    }

    /// Like `web`, and also the streaming kinds television playlists use.
    /// Still never a local file or one of mpv's own sources.
    static func stream(_ address: String) -> String? {
        network(address, schemes: ["http", "https", "rtsp", "rtsps", "rtmp", "rtmps", "rtp", "udp", "mms", "mmsh"])
    }

    private static func network(_ address: String, schemes: [String]) -> String? {
        let address = address.trimmingCharacters(in: .whitespacesAndNewlines)
        guard let mark = address.range(of: "://") else { return nil }
        let scheme = address[..<mark.lowerBound].lowercased()
        guard schemes.contains(scheme) else { return nil }
        var rest = address[mark.upperBound...]
        // "@" starts a multicast address, as in udp://@239.0.0.1:1234.
        if scheme == "udp" || scheme == "rtp", rest.hasPrefix("@") { rest = rest.dropFirst() }
        guard let first = rest.first, (first.isASCII && (first.isLetter || first.isNumber)) || first == "[" else { return nil }
        let clean = !address.unicodeScalars.contains {
            $0.properties.generalCategory == .control || $0.properties.isWhitespace
        }
        return clean ? address : nil
    }
}
