import Foundation

// Live TV and radio: reading channel lists. A source is the address of an
// M3U playlist (the usual "M3U Plus" kind: one `#EXTINF` line of details,
// then the channel's address). Entries marked `radio="true"` are radio
// stations; everything else is television. An Xtream Codes account is the
// other kind of source (`XtreamAccount`), turned into the same text.

/// One channel or station from a playlist.
struct Channel: Equatable, Sendable {
    var name: String
    /// Where it plays from.
    var address: String
    /// `group-title`, or the older `#EXTGRP` line.
    var group: String?
    /// Address of its logo (`tvg-logo`).
    var logo: String?
    /// Its id in a programme guide (`tvg-id`).
    var guideID: String?
    /// Marked `radio="true"`.
    var radio = false
}

enum M3U {
    /// Reads an M3U playlist. Lenient: a missing `#EXTM3U` header, blank
    /// lines, Windows line ends and unknown `#` lines are all fine; an entry
    /// without an address is left out, and an address without an `#EXTINF`
    /// line is named after its last path segment.
    static func parse(_ text: String) -> [Channel] {
        var channels: [Channel] = []
        var pending: Channel?
        // Some files start with a byte-order mark.
        var body = Substring(text)
        while body.hasPrefix("\u{feff}") { body = body.dropFirst() }
        for raw in body.split(omittingEmptySubsequences: true, whereSeparator: { $0 == "\n" || $0 == "\r\n" || $0 == "\r" }) {
            let line = raw.trimmingCharacters(in: .whitespacesAndNewlines)
            if line.isEmpty { continue }
            if let info = stripPrefix(line, "#EXTINF:") {
                pending = readInfo(info)
            } else if let group = stripPrefix(line, "#EXTGRP:") {
                if pending != nil, pending?.group == nil { pending?.group = textOf(group) }
            } else if line.hasPrefix("#") {
                // #EXTM3U, #EXTVLCOPT and anything else: not needed.
            } else {
                var channel = pending ?? Channel(name: "", address: "")
                pending = nil
                channel.address = line
                if channel.name.isEmpty { channel.name = nameFromAddress(line) }
                channels.append(channel)
            }
        }
        return channels
    }

    /// The rest of `line` after `prefix`, in any case.
    private static func stripPrefix(_ line: String, _ prefix: String) -> String? {
        guard line.utf8.count >= prefix.utf8.count else { return nil }
        let head = line.prefix(prefix.count)
        return head.caseInsensitiveCompare(prefix) == .orderedSame ? String(line.dropFirst(prefix.count)) : nil
    }

    private static func textOf(_ text: some StringProtocol) -> String? {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private static func nameFromAddress(_ address: String) -> String {
        let path = address.split(omittingEmptySubsequences: false) { $0 == "?" || $0 == "#" }.first.map(String.init) ?? address
        var trimmed = Substring(path)
        while trimmed.hasSuffix("/") { trimmed = trimmed.dropLast() }
        let last = trimmed.split(separator: "/", omittingEmptySubsequences: false).last.map(String.init) ?? path
        return last.isEmpty ? address : last
    }

    /// Reads what follows `#EXTINF:`: a duration, `key="value"` pairs, then a
    /// comma and the channel's name.
    private static func readInfo(_ info: String) -> Channel {
        // The name starts after the first comma that is not inside quotes.
        var inQuotes = false
        var split = info.endIndex
        for index in info.indices {
            let c = info[index]
            if c == "\"" { inQuotes.toggle() } else if c == ",", !inQuotes {
                split = index
                break
            }
        }
        let details = String(info[..<split])
        var name = info[split...]
        if name.hasPrefix(",") { name = name.dropFirst() }

        func attribute(_ key: String) -> String? {
            var rest = Substring(details)
            // Find `key="` at a word start, in any case.
            while let found = rest.range(of: key, options: .caseInsensitive) {
                let beforeOK = found.lowerBound == rest.startIndex || rest[rest.index(before: found.lowerBound)].isWhitespace
                let after = rest[found.upperBound...]
                if beforeOK, after.hasPrefix("=\"") {
                    let value = after.dropFirst(2).split(separator: "\"", omittingEmptySubsequences: false).first ?? ""
                    return textOf(value)
                }
                rest = after
            }
            return nil
        }

        return Channel(
            name: textOf(name) ?? attribute("tvg-name") ?? "",
            address: "",
            // Several groups may be listed with ';': the first is used.
            group: attribute("group-title").flatMap { groups in
                textOf(groups.split(separator: ";", omittingEmptySubsequences: false).first ?? "")
            },
            logo: attribute("tvg-logo"),
            guideID: attribute("tvg-id"),
            radio: attribute("radio")?.caseInsensitiveCompare("true") == .orderedSame)
    }

    /// Writes channels as playlist text that `parse` reads back the same.
    static func write(_ channels: [Channel]) -> String {
        // Quotes and line ends would break a line apart; a `;` splits groups.
        func value(_ text: String) -> String {
            String(text.map { $0 == "\"" || $0.unicodeScalars.contains(where: isControl) ? " " : $0 })
                .trimmingCharacters(in: .whitespaces)
        }
        var out = "#EXTM3U\n"
        for channel in channels {
            out += "#EXTINF:-1"
            if let id = channel.guideID { out += " tvg-id=\"\(value(id))\"" }
            if let logo = channel.logo { out += " tvg-logo=\"\(value(logo))\"" }
            if let group = channel.group { out += " group-title=\"\(value(group).replacingOccurrences(of: ";", with: ","))\"" }
            if channel.radio { out += " radio=\"true\"" }
            let name = String(channel.name.map { $0.unicodeScalars.contains(where: isControl) ? " " : $0 })
                .trimmingCharacters(in: .whitespaces)
            out += ",\(name)\n\(channel.address.trimmingCharacters(in: .whitespacesAndNewlines))\n"
        }
        return out
    }

    private static func isControl(_ scalar: Unicode.Scalar) -> Bool {
        scalar.properties.generalCategory == .control
    }

    /// The channels' groups, in the order they first appear, without repeats.
    static func groups(_ channels: [Channel]) -> [String] {
        var seen: [String] = []
        for group in channels.compactMap(\.group) where !seen.contains(group) {
            seen.append(group)
        }
        return seen
    }

    /// Whether a channel passes the filter: in `group` (when one is chosen)
    /// and with every word of `words` somewhere in its name, in any case.
    static func matches(_ channel: Channel, group: String?, words: String) -> Bool {
        if let group, channel.group != group { return false }
        let name = channel.name.lowercased()
        return words.split(whereSeparator: \.isWhitespace).allSatisfy { name.contains($0.lowercased()) }
    }

    /// A short name for a source, from its address: the host.
    static func sourceName(_ address: String) -> String {
        let rest = address.range(of: "://").map { String(address[$0.upperBound...]) } ?? address
        let host = rest.split(omittingEmptySubsequences: false) { $0 == "/" || $0 == "?" || $0 == "#" }.first.map(String.init) ?? rest
        // Drop "user:password@" and the port.
        let bare = host.split(separator: "@", omittingEmptySubsequences: false).last.map(String.init) ?? host
        let name: String
        if bare.hasPrefix("[") {
            name = bare.firstIndex(of: "]").map { String(bare[...$0]) } ?? bare
        } else {
            name = bare.split(separator: ":", omittingEmptySubsequences: false).first.map(String.init) ?? bare
        }
        return name.isEmpty ? address : name
    }
}
