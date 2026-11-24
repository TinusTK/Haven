import Foundation

/// Picks the entries of `MediaSources` out of an answer while it is still
/// arriving, each as soon as its last byte is in.
struct SourceScanner {
    /// The most one entry may take (one with more is cut short and fails to read).
    static let entryBytes = 4 * 1024 * 1024

    private var depth = 0
    private var inString = false
    private var escape = false
    /// The last string seen at the top level: the key before a value.
    private var key: [UInt8] = []
    private var inList = false
    /// The entry being gathered, if inside one.
    private var entry: [UInt8]?
    /// The list has ended.
    private(set) var done = false

    private static let mediaSources = Array("MediaSources".utf8)

    /// Takes one byte; gives an entry once one is complete.
    mutating func push(_ byte: UInt8) -> Any? {
        if done { return nil }
        if entry != nil, entry!.count < Self.entryBytes { entry!.append(byte) }
        let top = depth == 1 && !inList
        if inString {
            if escape {
                escape = false
            } else if byte == UInt8(ascii: "\\") {
                escape = true
            } else if byte == UInt8(ascii: "\"") {
                inString = false
            } else if top {
                key.append(byte)
            }
            return nil
        }
        switch byte {
        case UInt8(ascii: "\""):
            inString = true
            if top { key.removeAll() }
        case UInt8(ascii: "{"), UInt8(ascii: "["):
            if byte == UInt8(ascii: "["), top, key == Self.mediaSources {
                inList = true
            } else if byte == UInt8(ascii: "{"), inList, depth == 2 {
                entry = [byte]
            }
            depth += 1
        case UInt8(ascii: "}"), UInt8(ascii: "]"):
            depth = max(depth - 1, 0)
            if inList, depth == 2 {
                if let finished = entry {
                    entry = nil
                    return try? JSONSerialization.jsonObject(with: Data(finished))
                }
            } else if inList, depth == 1 {
                inList = false
                done = true
            }
        default:
            break
        }
        return nil
    }

    /// Takes a piece of the answer; gives the entries it completed.
    mutating func feed<S: Sequence>(_ part: S) -> [Any] where S.Element == UInt8 {
        var entries: [Any] = []
        for byte in part {
            if done { break }
            if let finished = push(byte) { entries.append(finished) }
        }
        return entries
    }
}

extension Emby {
    /// Sources handed on at a time while a long list is still arriving.
    static let batch = 10
    /// The most sources read from one answer.
    static let mostSources = 3000
    /// How long a list of sources may take in all, and between two parts.
    static let listWait: TimeInterval = 900
    static let partWait: TimeInterval = 45
    /// How often an empty list of sources is asked for again, and how long
    /// is waited before each.
    static let emptyRetries = 2
    static let emptyWait: Double = 5

    /// The playback information request: shaped like the server's own
    /// apps', with a profile that says this player takes any file as it is.
    static func playbackBody(_ server: Server) -> [String: Any] {
        [
            "UserId": server.userID,
            "StartTimeTicks": 0,
            "IsPlayback": false,
            "AutoOpenLiveStream": false,
            "MaxStreamingBitrate": 1_000_000_000,
            "EnableDirectPlay": true,
            "EnableDirectStream": true,
            "EnableTranscoding": false,
            "DeviceProfile": [
                "Name": "Haven",
                "MaxStreamingBitrate": 1_000_000_000,
                "MaxStaticBitrate": 1_000_000_000,
                "DirectPlayProfiles": [["Type": "Video"], ["Type": "Audio"]],
                "TranscodingProfiles": [],
                "ContainerProfiles": [],
                "CodecProfiles": [],
                "SubtitleProfiles": [],
            ] as [String: Any],
        ]
    }

    /// Every way the server offers to play the film or episode, handed to
    /// `found` a few at a time as the answer arrives: each version of the
    /// file it holds, and, on servers that gather streams from elsewhere,
    /// each of those (hundreds, in an answer of many megabytes). `found`
    /// answers whether more are wanted; on no, the rest is not read. The
    /// token goes in the stream addresses because the player opens them.
    static func streamsLive(_ client: ServerClient, _ server: Server, item: String, found: ([MediaStream]) -> Bool) async throws {
        let address = "\(server.address)/Items/\(q(item))/PlaybackInfo?UserId=\(q(server.userID))"
            + "&StartTimeTicks=0&IsPlayback=false&AutoOpenLiveStream=false&MaxStreamingBitrate=1000000000"
        var listed = 0
        var seen = Set<String>()
        var asked: Error?
        // A server that gathers streams answers with what it has so far,
        // which can be nothing; asked again a little later it has more.
        for attempt in 0 ... emptyRetries {
            if attempt > 0 { try await Task.sleep(for: .seconds(emptyWait)) }
            do {
                asked = nil
                try await readPlaybackInfo(client, server, address: address, item: item, listed: &listed, seen: &seen, found: found)
            } catch is CancellationError {
                throw CancellationError()
            } catch {
                asked = error
            }
            Log.note("\(server.name) sources: playback information "
                + (asked.map { "lists \(listed), then failed (\($0.localizedDescription))" } ?? "lists \(listed)"))
            if listed > 0 || asked != nil { break }
        }
        if listed > 0 { return }
        // Older servers, or one that listed nothing: the item itself may list its files.
        let detail: Any
        do {
            detail = try await get(client, server, "/Users/\(server.userID)/Items/\(q(item))?Fields=MediaSources,MediaStreams")
        } catch {
            // Both failed: say why the item could not be read.
            if asked != nil { throw error }
            _ = found(readStreams(server, item: item, [String: Any]()))
            return
        }
        Log.note("\(server.name) sources: the item \(describeSources(detail))")
        // An item that says outright it has no sources has none (a gathering
        // server found nothing): only a server that does not speak of
        // sources at all gets its file offered.
        if (detail as? [String: Any])?["MediaSources"] != nil, ServerJSON.list(detail, "MediaSources").isEmpty { return }
        _ = found(readStreams(server, item: item, detail))
    }

    private static func readPlaybackInfo(_ client: ServerClient, _ server: Server, address: String, item: String,
                                         listed: inout Int, seen: inout Set<String>,
                                         found: ([MediaStream]) -> Bool) async throws {
        guard let url = try? HTTP.webURL(address) else { throw ServerError.invalidAddress(address) }
        guard await OwnNetwork.allowed(url, enforce: client.ownNetworkOnly) else { throw ServerError.notOwnNetwork }
        var request = URLRequest(url: url, timeoutInterval: partWait)
        request.httpMethod = "POST"
        request.httpBody = try JSONSerialization.data(withJSONObject: playbackBody(server))
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        for (name, value) in headers(device: client.device, kind: server.kind, token: server.token) {
            request.setValue(value, forHTTPHeaderField: name)
        }
        let bytes: URLSession.AsyncBytes
        let response: URLResponse
        do {
            (bytes, response) = try await client.streaming.bytes(for: request)
        } catch {
            throw ServerError.request(HTTP.words(error))
        }
        defer { bytes.task.cancel() }
        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        switch status {
        case 200 ..< 300: break
        case 401, 403: throw ServerError.signInRefused
        default: throw ServerError.status(status)
        }
        let deadline = Date().addingTimeInterval(listWait)
        var scanner = SourceScanner()
        var waiting: [Any] = []
        var count = 0
        do {
            for try await byte in bytes {
                count += 1
                // The whole list's time limit, looked at now and then.
                if count % 65536 == 0, Date() > deadline {
                    throw ServerError.request("the server took too long to answer")
                }
                guard let source = scanner.push(byte) else {
                    if scanner.done { break }
                    continue
                }
                if let id = ServerJSON.text(source, "Id"), !seen.insert(id).inserted { continue }
                listed += 1
                waiting.append(source)
                if waiting.count >= batch {
                    let wanted = found(readSources(server, item: item, waiting, several: listed > 1))
                    waiting.removeAll()
                    if !wanted { break }
                }
                if scanner.done || listed >= mostSources { break }
            }
        } catch let error as URLError where error.code == .timedOut {
            // Nothing more for a while: what came so far is the list.
        } catch let error as URLError where error.code != .cancelled {
            if !waiting.isEmpty { _ = found(readSources(server, item: item, waiting, several: listed > 1)) }
            throw ServerError.request(HTTP.words(error))
        }
        if !waiting.isEmpty {
            _ = found(readSources(server, item: item, waiting, several: listed > 1))
        }
    }

    /// What an answer's `MediaSources` look like, for the log: how many, and
    /// for each which fields it has. No ids, names or addresses.
    static func describeSources(_ answer: Any) -> String {
        let sources = ServerJSON.list(answer, "MediaSources")
        let each = sources.prefix(5).map { source -> String in
            func has(_ key: String) -> String { ServerJSON.text(source, key) == nil ? "no" : "yes" }
            let path = ServerJSON.text(source, "Path")
            let webPath = path.map { $0.hasPrefix("http://") || $0.hasPrefix("https://") } ?? false
            return "[protocol \(ServerJSON.text(source, "Protocol") ?? "none") id \(has("Id")) name \(has("Name")) "
                + "path \(webPath ? "web" : has("Path")) url \(has("DirectStreamUrl")) "
                + "streams \(ServerJSON.list(source, "MediaStreams").count)]"
        }
        let keys = ((answer as? [String: Any])?.keys.filter { $0.contains("Source") || $0.contains("Error") } ?? []).sorted()
        return "lists \(sources.count) \(each.joined(separator: " ")) (fields: \(keys.joined(separator: ", ")))"
    }

    static func fileAddress(_ server: Server, item: String, version: String?) -> String {
        var address = "\(server.address)/Videos/\(q(item))/stream?static=true"
        if let version { address += "&MediaSourceId=\(q(version))" }
        return address + "&api_key=\(q(server.token))"
    }

    /// One stream per entry of `MediaSources` in a playback-information or
    /// item answer; the file itself when the server lists none.
    static func readStreams(_ server: Server, item: String, _ answer: Any) -> [MediaStream] {
        let versions = ServerJSON.list(answer, "MediaSources")
        let streams = readSources(server, item: item, versions, several: versions.count > 1)
        if !streams.isEmpty { return streams }
        return [MediaStream(name: "Direct", description: "The file as stored on \(server.name)",
                            url: fileAddress(server, item: item, version: nil))]
    }

    /// One stream per source; `several` says the film has more than one.
    static func readSources(_ server: Server, item: String, _ versions: [Any], several: Bool) -> [MediaStream] {
        typealias J = ServerJSON
        return versions.compactMap { version in
            let id = J.text(version, "Id")
            func track(_ kind: String) -> Any? { J.list(version, "MediaStreams").first { J.text($0, "Type") == kind } }
            let video = track("Video")
            func codec(_ stream: Any?) -> String? { J.text(stream, "Codec")?.uppercased() }
            let quality = Servers.quality(width: J.number(video, "Width"), height: J.number(video, "Height"))
            // A source that is itself a web address (a stream gathered from
            // elsewhere) plays from there; a file plays from the server.
            let remote = J.text(version, "Path").flatMap { path in
                let http = J.text(version, "Protocol")?.caseInsensitiveCompare("http") == .orderedSame
                return http && (path.hasPrefix("http://") || path.hasPrefix("https://")) ? path : nil
            }
            // Or the server names the address to play it from itself.
            let given = J.text(version, "DirectStreamUrl").map { url in
                if url.hasPrefix("http://") || url.hasPrefix("https://") { return url }
                let join = url.contains("?") ? "&" : "?"
                let slash = url.hasPrefix("/") ? "" : "/"
                return "\(server.address)\(slash)\(url)\(join)api_key=\(q(server.token))"
            }
            guard let url = remote ?? given ?? id.map({ fileAddress(server, item: item, version: $0) }) else { return nil }
            var facts: [String] = []
            // The server's name for the source tells several apart. When the
            // server says nothing of the picture (a gathered stream), the
            // name's first line heads the row and the rest describes it.
            var heading = quality
            if let name = J.text(version, "Name") {
                if quality != nil {
                    if several { facts.append(name) }
                } else {
                    let lines = name.split(whereSeparator: \.isNewline)
                        .map { $0.trimmingCharacters(in: .whitespaces) }.filter { !$0.isEmpty }
                    heading = lines.first
                    facts += lines.dropFirst()
                }
            }
            // Containers can come as a list ("mov,mp4,m4a"): the first will do.
            if let container = J.text(version, "Container")?.split(separator: ",", omittingEmptySubsequences: false).first {
                facts.append(container.uppercased())
            }
            if let c = codec(video) { facts.append(c) }
            if let c = codec(track("Audio")) { facts.append(c) }
            if let size = J.number(version, "Size").flatMap(Servers.size) { facts.append(size) }
            return MediaStream(
                name: heading ?? "Direct",
                description: facts.isEmpty ? "The file as stored on \(server.name)" : facts.joined(separator: " · "),
                url: url,
                // A server that gathers streams names where each came from.
                provider: J.text(J.object(J.object(version, "Remux"), "ProviderInfo"), "source"))
        }
    }
}
