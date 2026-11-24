import Foundation

/// Plex. Signing in goes through plex.tv with a short code the user
/// approves in a web browser; the servers are then asked directly.
enum Plex {
    static let plexTV = "https://plex.tv/api/v2"

    private typealias J = ServerJSON

    /// A sign-in in progress.
    struct Pin: Equatable, Sendable {
        var id: String
        /// The code plex.tv shows the user to confirm.
        var code: String
        /// The page to open in a web browser.
        var link: String
    }

    /// A server as plex.tv lists it, before an address has been tried.
    struct Listed: Equatable, Sendable {
        var name: String
        var token: String
        /// Addresses to try, best first: local, then remote, then relayed.
        var addresses: [String]
    }

    static func headers(device: String, token: String?) -> [(String, String)] {
        var headers = [("X-Plex-Product", "Haven"), ("X-Plex-Version", Build.version), ("X-Plex-Client-Identifier", device)]
        if let token { headers.append(("X-Plex-Token", token)) }
        return headers
    }

    private static func q(_ text: String) -> String { Servers.query(text) }

    /// Starts signing in: gives a code and the page to enter it on.
    static func pin(_ client: ServerClient) async throws -> Pin {
        let answer = try await client.call("\(plexTV)/pins?strong=true", headers: headers(device: client.device, token: nil), body: [:])
        guard let id = J.text(answer, "id"), let code = J.text(answer, "code") else { throw ServerError.badAnswer }
        let link = "https://app.plex.tv/auth#?clientID=\(q(client.device))&code=\(q(code))&context%5Bdevice%5D%5Bproduct%5D=Haven"
        return Pin(id: id, code: code, link: link)
    }

    /// Whether the user has approved the code yet: the account's token once
    /// they have.
    static func token(_ client: ServerClient, _ pin: Pin) async throws -> String? {
        let answer = try await client.call("\(plexTV)/pins/\(q(pin.id))?code=\(q(pin.code))",
                                           headers: headers(device: client.device, token: nil))
        return J.text(answer, "authToken")
    }

    static func readResources(_ resources: Any) -> [Listed] {
        var listed: [Listed] = []
        for resource in resources as? [Any] ?? [] {
            let provides = (J.text(resource, "provides") ?? "").split(separator: ",")
            guard provides.contains(where: { $0.trimmingCharacters(in: .whitespaces) == "server" }),
                  let name = J.text(resource, "name"), let token = J.text(resource, "accessToken") else { continue }
            // Local first, then remote, then relayed; the same rank keeps its order.
            var ranked: [(rank: Int, place: Int, uri: String)] = []
            for connection in J.list(resource, "connections") {
                guard var uri = J.text(connection, "uri") else { continue }
                while uri.hasSuffix("/") { uri.removeLast() }
                let flags = connection as? [String: Any] ?? [:]
                let rank = flags["relay"] as? Bool == true ? 2 : flags["local"] as? Bool == true ? 0 : 1
                ranked.append((rank, ranked.count, uri))
            }
            ranked.sort { $0.rank != $1.rank ? $0.rank < $1.rank : $0.place < $1.place }
            listed.append(Listed(name: name, token: token, addresses: ranked.map(\.uri)))
        }
        return listed
    }

    /// The Plex servers the account can use, each with an address that
    /// answered from here.
    static func servers(_ client: ServerClient, accountToken: String) async throws -> [Server] {
        // App Store builds use only the server's own-network addresses: no relay.
        let relay = client.ownNetworkOnly ? 0 : 1
        let resources = try await client.call("\(plexTV)/resources?includeHttps=1&includeRelay=\(relay)",
                                              headers: headers(device: client.device, token: accountToken))
        let listed = readResources(resources)
        if listed.isEmpty { return [] }
        var found: [Server] = []
        var refused = false
        for server in listed {
            for address in server.addresses {
                do {
                    _ = try await client.call("\(address)/identity", headers: headers(device: client.device, token: server.token))
                    found.append(Server(kind: .plex, name: server.name, address: address, token: server.token))
                    break
                } catch ServerError.notOwnNetwork {
                    refused = true
                } catch {}
            }
        }
        if !found.isEmpty { return found }
        throw refused ? ServerError.notOwnNetwork : ServerError.unreachable
    }

    private static func get(_ client: ServerClient, _ server: Server, _ path: String) async throws -> Any {
        let answer = try await client.call(server.address + path, headers: headers(device: client.device, token: server.token))
        return J.object(answer, "MediaContainer") ?? NSNull()
    }

    static func libraries(_ client: ServerClient, _ server: Server) async throws -> [MediaLibrary] {
        readLibraries(try await get(client, server, "/library/sections"))
    }

    static func readLibraries(_ container: Any) -> [MediaLibrary] {
        J.list(container, "Directory").filter { ["movie", "show"].contains(J.text($0, "type") ?? "") }.compactMap { section in
            guard let id = J.text(section, "key"), let name = J.text(section, "title") else { return nil }
            return MediaLibrary(id: id, name: name, collection: false)
        }
    }

    static func listPage(_ client: ServerClient, _ server: Server, path: String, start: Int, length: Int) async throws -> [MetaPreview] {
        let join = path.contains("?") ? "&" : "?"
        return readPreviews(server, try await get(client, server, "\(path)\(join)X-Plex-Container-Start=\(start)&X-Plex-Container-Size=\(length)"))
    }

    static func latest(_ client: ServerClient, _ server: Server, library: String) async throws -> [MetaPreview] {
        try await listPage(client, server, path: "/library/sections/\(q(library))/recentlyAdded", start: 0, length: 30)
    }

    static func page(_ client: ServerClient, _ server: Server, library: String, start: Int, length: Int) async throws -> [MetaPreview] {
        try await listPage(client, server, path: "/library/sections/\(q(library))/all?sort=titleSort", start: start, length: length)
    }

    static func search(_ client: ServerClient, _ server: Server, words: String) async throws -> [MetaPreview] {
        try await listPage(client, server, path: "/search?query=\(q(words))", start: 0, length: 30)
    }

    static func online(_ client: ServerClient, _ server: Server) async -> Bool {
        (try? await client.call("\(server.address)/identity", headers: headers(device: client.device, token: nil))) != nil
    }

    /// A title's ids at IMDb and TMDB: from its `Guid` list, or from the
    /// single `guid` older libraries carry.
    static func providerIDs(_ item: Any) -> (imdb: String?, tmdb: String?) {
        let guids = J.list(item, "Guid").compactMap { J.text($0, "id") } + [J.text(item, "guid")].compactMap { $0 }
        func id(_ marks: [String]) -> String? {
            for guid in guids {
                guard let mark = marks.lazy.compactMap({ guid.range(of: $0) }).first else { continue }
                let rest = guid[mark.upperBound...]
                let id = (rest.split(omittingEmptySubsequences: false) { $0 == "?" || $0 == "/" }.first ?? rest)
                    .trimmingCharacters(in: .whitespaces)
                if !id.isEmpty { return id }
            }
            return nil
        }
        return (id(["imdb://"]), id(["tmdb://", "themoviedb://"]))
    }

    static func readCandidates(_ container: Any) -> [Candidate] {
        J.list(container, "Metadata").compactMap { item in
            guard let id = J.text(item, "ratingKey"), let name = J.text(item, "title") else { return nil }
            let ids = providerIDs(item)
            return Candidate(id: id, name: name, year: J.text(item, "year"), imdb: ids.imdb, tmdb: ids.tmdb)
        }
    }

    static func readEpisode(_ leaves: Any, season: Int, number: Int) -> String? {
        J.list(leaves, "Metadata").first {
            J.number($0, "parentIndex") == Double(season) && J.number($0, "index") == Double(number)
        }.flatMap { J.text($0, "ratingKey") }
    }

    static func find(_ client: ServerClient, _ server: Server, _ wanted: Wanted) async throws -> String? {
        let name = wanted.name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return nil }
        // Type 1 is films, 2 is shows.
        let path = "/library/all?type=\(wanted.series ? 2 : 1)&title=\(q(name))&includeGuids=1"
            + "&X-Plex-Container-Start=0&X-Plex-Container-Size=25"
        guard let found = Servers.bestMatch(wanted, readCandidates(try await get(client, server, path))) else { return nil }
        guard let episode = wanted.episode else { return wanted.series ? nil : found.id }
        let leaves = try await get(client, server, "/library/metadata/\(q(found.id))/allLeaves")
        return readEpisode(leaves, season: episode.season, number: episode.number)
    }

    /// A picture (or file) on the server; the token goes in the address.
    static func image(_ server: Server, path: String) -> String {
        "\(server.address)\(path)?X-Plex-Token=\(q(server.token))"
    }

    /// Films and shows as posters. A season or episode stands for its show;
    /// a show appears once.
    static func readPreviews(_ server: Server, _ container: Any) -> [MetaPreview] {
        var previews: [MetaPreview] = []
        for item in J.list(container, "Metadata") {
            let kind: String, id: String?, name: String?, thumb: String?
            switch J.text(item, "type") {
            case "movie": (kind, id, name, thumb) = ("movie", J.text(item, "ratingKey"), J.text(item, "title"), J.text(item, "thumb"))
            case "show": (kind, id, name, thumb) = ("series", J.text(item, "ratingKey"), J.text(item, "title"), J.text(item, "thumb"))
            case "season":
                (kind, id, name, thumb) = ("series", J.text(item, "parentRatingKey"), J.text(item, "parentTitle"), J.text(item, "parentThumb"))
            case "episode":
                (kind, id, name, thumb) = ("series", J.text(item, "grandparentRatingKey"), J.text(item, "grandparentTitle"),
                                           J.text(item, "grandparentThumb"))
            default: continue
            }
            guard let id, let name else { continue }
            let full = Servers.titleID(server: server.id, item: id)
            if previews.contains(where: { $0.id == full }) { continue }
            previews.append(MetaPreview(
                id: full, kind: kind, name: name, poster: thumb.map { image(server, path: $0) },
                releaseInfo: J.text(item, "year"),
                rating: (J.number(item, "audienceRating") ?? J.number(item, "rating")).flatMap { $0 > 0 ? J.rating($0) : nil }))
        }
        return previews
    }

    static func meta(_ client: ServerClient, _ server: Server, item: String) async throws -> MetaDetail {
        let item = q(item)
        let detail = try await get(client, server, "/library/metadata/\(item)")
        let show = J.text(J.list(detail, "Metadata").first, "type") == "show"
        let episodes: Any = show ? try await get(client, server, "/library/metadata/\(item)/allLeaves") : NSNull()
        guard let meta = readDetail(server, detail, episodes: episodes) else { throw ServerError.badAnswer }
        return meta
    }

    static func readDetail(_ server: Server, _ detail: Any, episodes: Any) -> MetaDetail? {
        guard let item = J.list(detail, "Metadata").first, let id = J.text(item, "ratingKey"),
              let name = J.text(item, "title") else { return nil }
        let ids = providerIDs(item)
        func tags(_ key: String) -> [String] { J.list(item, key).compactMap { J.text($0, "tag") } }
        var meta = MetaDetail(id: Servers.titleID(server: server.id, item: id),
                              kind: J.text(item, "type") == "show" ? "series" : "movie", name: name)
        meta.videos = J.list(episodes, "Metadata").compactMap { episode in
            guard let episodeID = J.text(episode, "ratingKey") else { return nil }
            return Video(id: Servers.titleID(server: server.id, item: episodeID), title: J.text(episode, "title"),
                         season: J.number(episode, "parentIndex").map(J.whole), episode: J.number(episode, "index").map(J.whole),
                         released: J.text(episode, "originallyAvailableAt"))
        }
        meta.poster = J.text(item, "thumb").map { image(server, path: $0) }
        meta.background = J.text(item, "art").map { image(server, path: $0) }
        meta.description = J.text(item, "summary")
        meta.releaseInfo = J.text(item, "year")
        meta.released = J.text(item, "originallyAvailableAt")
        // Plex counts in milliseconds.
        meta.runtime = J.number(item, "duration").map { ($0 / 60_000).rounded() }
            .flatMap { $0 > 0 && $0.isFinite ? "\(Int64($0)) min" : nil }
        meta.rating = (J.number(item, "rating") ?? J.number(item, "audienceRating")).map(J.rating)
        meta.genres = tags("Genre")
        meta.cast = tags("Role")
        meta.director = tags("Director")
        meta.imdbID = ids.imdb
        meta.tmdbID = ids.tmdb
        return meta
    }

    static func streams(_ client: ServerClient, _ server: Server, item: String) async throws -> [MediaStream] {
        readStreams(server, try await get(client, server, "/library/metadata/\(q(item))"))
    }

    /// One stream per version of the file the server holds.
    static func readStreams(_ server: Server, _ detail: Any) -> [MediaStream] {
        guard let item = J.list(detail, "Metadata").first else { return [] }
        return J.list(item, "Media").compactMap { media in
            guard let part = J.list(media, "Part").first, let key = J.text(part, "key") else { return nil }
            let quality = J.text(media, "videoResolution").map { resolution in
                switch resolution {
                case "4k": "4K"
                case "sd": "SD"
                default: "\(resolution)p"
                }
            } ?? "Direct"
            var facts = [J.text(media, "container"), J.text(media, "videoCodec"), J.text(media, "audioCodec")]
                .compactMap { $0?.uppercased() }
            if let size = J.number(part, "size").flatMap(Servers.size) { facts.append(size) }
            return MediaStream(name: quality,
                               description: facts.isEmpty ? "The file as stored on \(server.name)" : facts.joined(separator: " · "),
                               url: image(server, path: key))
        }
    }

    static func report(_ client: ServerClient, _ server: Server, item: String, position: Double, duration: Double, finished: Bool) async throws {
        let item = q(item)
        let path = finished
            ? "/:/scrobble?identifier=com.plexapp.plugins.library&key=\(item)"
            : "/:/timeline?ratingKey=\(item)&key=%2Flibrary%2Fmetadata%2F\(item)&state=stopped"
                + "&time=\(Int64(max(position, 0) * 1000))&duration=\(Int64(max(duration, 0) * 1000))"
                + "&identifier=com.plexapp.plugins.library"
        _ = try await get(client, server, path)
    }
}
