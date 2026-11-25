import Foundation

/// Jellyfin and Emby. Jellyfin grew out of Emby and the two still share the
/// parts of the API used here; they differ in the sign-in header.
enum Emby {
    /// Collections listed per server (each becomes a shelf).
    static let collections = 60
    /// One tick is 100 nanoseconds.
    static let ticksPerSecond = 10_000_000.0

    private typealias J = ServerJSON

    static func headers(device: String, kind: ServerKind, token: String?) -> [(String, String)] {
        var value = "MediaBrowser Client=\"Haven\", Device=\"Haven\", DeviceId=\"\(device)\", Version=\"\(Build.version)\""
        if let token { value += ", Token=\"\(token)\"" }
        switch (kind, token) {
        case let (.emby, .some(token)): return [("X-Emby-Authorization", value), ("X-Emby-Token", token)]
        case (.emby, nil): return [("X-Emby-Authorization", value)]
        default: return [("Authorization", value)]
        }
    }

    static func get(_ client: ServerClient, _ server: Server, _ path: String) async throws -> Any {
        try await client.call(server.address + path, headers: headers(device: client.device, kind: server.kind, token: server.token))
    }

    static func q(_ text: String) -> String { Servers.query(text) }

    static func signIn(_ client: ServerClient, kind: ServerKind, address: String, user: String, password: String) async throws -> Server {
        let answer = try await client.call(
            "\(address)/Users/AuthenticateByName", headers: headers(device: client.device, kind: kind, token: nil),
            body: ["Username": user, "Pw": password])
        guard let token = J.text(answer, "AccessToken"), let userID = J.text(J.object(answer, "User"), "Id") else {
            throw ServerError.badAnswer
        }
        // The server's own name, if it will say; otherwise its address.
        let info = try? await client.call("\(address)/System/Info/Public", headers: headers(device: client.device, kind: kind, token: nil))
        let name = J.text(info, "ServerName") ?? address
        return Server(kind: kind, name: name, address: address, userID: userID, token: token)
    }

    static func libraries(_ client: ServerClient, _ server: Server) async throws -> [MediaLibrary] {
        let views = try await get(client, server, "/Users/\(server.userID)/Views")
        var libraries = readLibraries(views)
        // The collections view holds lists, not titles: each list is offered
        // as a library of its own (on a server that gathers catalogues, the
        // catalogues).
        for view in collectionViews(views) {
            let path = "/Users/\(server.userID)/Items?ParentId=\(q(view))&StartIndex=0&Limit=\(collections)"
            if let found = try? await get(client, server, path) {
                libraries += readCollections(found)
            }
        }
        return libraries
    }

    static func collectionViews(_ views: Any) -> [String] {
        J.list(views, "Items").filter { J.text($0, "CollectionType") == "boxsets" }.compactMap { J.text($0, "Id") }
    }

    static func readCollections(_ found: Any) -> [MediaLibrary] {
        J.list(found, "Items").filter { J.text($0, "Type") == "BoxSet" }.compactMap { item in
            guard let id = J.text(item, "Id"), let name = J.text(item, "Name") else { return nil }
            return MediaLibrary(id: id, name: name, collection: true)
        }
    }

    static func readLibraries(_ views: Any) -> [MediaLibrary] {
        J.list(views, "Items").filter { view in
            // No type means a mixed library; other types (music, books) are skipped.
            let type = J.text(view, "CollectionType")
            return type == nil || type == "movies" || type == "tvshows"
        }.compactMap { view in
            guard let id = J.text(view, "Id"), let name = J.text(view, "Name") else { return nil }
            return MediaLibrary(id: id, name: name, collection: false)
        }
    }

    static func genres(_ client: ServerClient, _ server: Server, library: String) async -> [String] {
        let path = "/Items/Filters?UserId=\(server.userID)&ParentId=\(q(library))"
        guard let filters = try? await get(client, server, path) else { return [] }
        return J.list(filters, "Genres").compactMap { genre in
            guard let text = (genre as? String)?.trimmingCharacters(in: .whitespacesAndNewlines), !text.isEmpty else { return nil }
            return text
        }
    }

    static func latest(_ client: ServerClient, _ server: Server, library: String) async throws -> [MetaPreview] {
        let path = "/Users/\(server.userID)/Items/Latest?ParentId=\(q(library))&Limit=30&IncludeItemTypes=Movie,Series,Episode"
        return readPreviews(server, try await get(client, server, path) as? [Any] ?? [])
    }

    static func page(_ client: ServerClient, _ server: Server, library: String, genre: String?, start: Int, length: Int) async throws -> [MetaPreview] {
        var path = "/Users/\(server.userID)/Items?ParentId=\(q(library))&Recursive=true&IncludeItemTypes=Movie,Series"
            + "&SortBy=SortName&SortOrder=Ascending&StartIndex=\(start)&Limit=\(length)"
        if let genre { path += "&Genres=\(q(genre))" }
        return readPreviews(server, J.list(try await get(client, server, path), "Items"))
    }

    static func search(_ client: ServerClient, _ server: Server, words: String) async throws -> [MetaPreview] {
        let path = "/Users/\(server.userID)/Items?SearchTerm=\(q(words))&Recursive=true&IncludeItemTypes=Movie,Series&Limit=30"
        return readPreviews(server, J.list(try await get(client, server, path), "Items"))
    }

    static func online(_ client: ServerClient, _ server: Server) async -> Bool {
        (try? await client.call("\(server.address)/System/Info/Public",
                                headers: headers(device: client.device, kind: server.kind, token: nil))) != nil
    }

    /// A title's ids at IMDb and TMDB, as the server recorded them. The
    /// keys' case differs between servers and versions.
    static func providerIDs(_ item: Any) -> (imdb: String?, tmdb: String?) {
        let ids = J.object(item, "ProviderIds") ?? [:]
        func id(_ name: String) -> String? {
            guard let value = ids.first(where: { $0.key.caseInsensitiveCompare(name) == .orderedSame })?.value as? String
            else { return nil }
            let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
            return trimmed.isEmpty ? nil : trimmed
        }
        return (id("Imdb"), id("Tmdb"))
    }

    static func readCandidates(_ items: Any) -> [Candidate] {
        J.list(items, "Items").compactMap { item in
            guard let id = J.text(item, "Id"), let name = J.text(item, "Name") else { return nil }
            let ids = providerIDs(item)
            return Candidate(id: id, name: name, year: J.text(item, "ProductionYear"), imdb: ids.imdb, tmdb: ids.tmdb)
        }
    }

    /// The id of the episode with that season and number, from a series'
    /// episode list.
    static func readEpisode(_ episodes: Any, season: Int, number: Int) -> String? {
        J.list(episodes, "Items").first {
            J.number($0, "ParentIndexNumber") == Double(season) && J.number($0, "IndexNumber") == Double(number)
        }.flatMap { J.text($0, "Id") }
    }

    static func find(_ client: ServerClient, _ server: Server, _ wanted: Wanted) async throws -> String? {
        let name = wanted.name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return nil }
        let path = "/Users/\(server.userID)/Items?SearchTerm=\(q(name))&Recursive=true"
            + "&IncludeItemTypes=\(wanted.series ? "Series" : "Movie")&Fields=ProviderIds&Limit=25"
        guard let match = Servers.bestMatch(wanted, readCandidates(try await get(client, server, path))) else { return nil }
        // A gathered title listed under another number (TMDB, say) is asked
        // for under its IMDb id, which is what finds its streams.
        let found = wanted.imdb.flatMap { Servers.byIMDb(match.id, imdb: $0) } ?? match.id
        guard let episode = wanted.episode else { return wanted.series ? nil : found }
        let episodes = try await get(client, server, "/Shows/\(q(found))/Episodes?UserId=\(server.userID)")
        return readEpisode(episodes, season: episode.season, number: episode.number)
    }

    /// The address of one of an item's pictures. `tag` is the picture's
    /// version as the server names it. The token is included because some
    /// servers only hand pictures to a signed-in app.
    static func image(_ server: Server, item: String, which: String, height: Int, tag: String?) -> String {
        var address = "\(server.address)/Items/\(q(item))/Images/\(which)?maxHeight=\(height)&quality=90"
        if let tag { address += "&tag=\(q(tag))" }
        return address + "&api_key=\(q(server.token))"
    }

    static func posterTag(_ item: Any) -> String? { J.text(J.object(item, "ImageTags"), "Primary") }

    /// A picture the item names by its full web address, as servers that
    /// gather titles from elsewhere sometimes do.
    static func remoteImage(_ item: Any, kind: String) -> String? {
        J.list(item, "RemoteImages").first { J.text($0, "Type") == kind }
            .flatMap { J.text($0, "Url") }
            .flatMap { $0.hasPrefix("http://") || $0.hasPrefix("https://") ? $0 : nil }
    }

    /// Films and series as posters. An episode stands for its series; a
    /// series shows once however many of its episodes are listed.
    static func readPreviews(_ server: Server, _ items: [Any]) -> [MetaPreview] {
        var previews: [MetaPreview] = []
        for item in items {
            let kind: String, id: String?, name: String?, tag: String?
            switch J.text(item, "Type") {
            case "Movie": (kind, id, name, tag) = ("movie", J.text(item, "Id"), J.text(item, "Name"), posterTag(item))
            case "Series": (kind, id, name, tag) = ("series", J.text(item, "Id"), J.text(item, "Name"), posterTag(item))
            case "Episode":
                (kind, id, name, tag) = ("series", J.text(item, "SeriesId"), J.text(item, "SeriesName"),
                                         J.text(item, "SeriesPrimaryImageTag"))
            default: continue
            }
            guard let id, let name, !isErrorEntry(id) else { continue }
            let full = Servers.titleID(server: server.id, item: id)
            if previews.contains(where: { $0.id == full }) { continue }
            previews.append(MetaPreview(
                id: full, kind: kind, name: name,
                poster: image(server, item: id, which: "Primary", height: 450, tag: tag),
                releaseInfo: J.text(item, "ProductionYear"),
                rating: J.number(item, "CommunityRating").flatMap { $0 > 0 ? J.rating($0) : nil }))
        }
        return previews
    }

    /// A server that gathers catalogues passes on a source's error (such as
    /// "Rate Limit Exceeded") as if it were a film. Its id spells
    /// `<kind>|<source id>|<type>` in hex, and the source id says it is an error.
    static func isErrorEntry(_ id: String) -> Bool {
        guard let spelt = Servers.unhex(id) else { return false }
        let parts = spelt.split(separator: "|", omittingEmptySubsequences: false)
        return parts.count > 1 && parts[1].contains("streamserror.")
    }

    static func meta(_ client: ServerClient, _ server: Server, item: String) async throws -> MetaDetail {
        let detail = try await get(client, server, "/Users/\(server.userID)/Items/\(q(item))")
        let episodes: Any = J.text(detail, "Type") == "Series"
            ? try await get(client, server, "/Shows/\(q(item))/Episodes?UserId=\(server.userID)")
            : NSNull()
        guard let meta = readDetail(server, detail, episodes: episodes) else { throw ServerError.badAnswer }
        return meta
    }

    static func readDetail(_ server: Server, _ detail: Any, episodes: Any) -> MetaDetail? {
        guard let id = J.text(detail, "Id"), let name = J.text(detail, "Name") else { return nil }
        let series = J.text(detail, "Type") == "Series"
        let ids = providerIDs(detail)
        func people(_ role: String) -> [String] {
            J.list(detail, "People").filter { J.text($0, "Type") == role }.compactMap { J.text($0, "Name") }
        }
        var meta = MetaDetail(id: Servers.titleID(server: server.id, item: id), kind: series ? "series" : "movie", name: name)
        meta.videos = J.list(episodes, "Items").compactMap { episode in
            guard let episodeID = J.text(episode, "Id") else { return nil }
            return Video(id: Servers.titleID(server: server.id, item: episodeID), title: J.text(episode, "Name"),
                         season: J.number(episode, "ParentIndexNumber").map(J.whole),
                         episode: J.number(episode, "IndexNumber").map(J.whole),
                         released: J.text(episode, "PremiereDate"),
                         // The episode's own still, if the server has one.
                         thumbnail: remoteImage(episode, kind: "Primary")
                             ?? posterTag(episode).map { image(server, item: episodeID, which: "Primary", height: 360, tag: $0) })
        }
        meta.poster = remoteImage(detail, kind: "Primary")
            ?? image(server, item: id, which: "Primary", height: 450, tag: posterTag(detail))
        // Asked for even when the item lists no backdrop: a server that
        // gathers its titles from elsewhere may still have one to give.
        meta.background = remoteImage(detail, kind: "Backdrop")
            ?? image(server, item: id, which: "Backdrop", height: 900, tag: J.list(detail, "BackdropImageTags").first as? String)
        meta.description = J.text(detail, "Overview")
        meta.releaseInfo = J.text(detail, "ProductionYear")
        meta.released = J.text(detail, "PremiereDate")
        meta.runtime = J.number(detail, "RunTimeTicks")
            .map { ($0 / ticksPerSecond / 60).rounded() }
            .flatMap { $0 > 0 && $0.isFinite ? "\(Int64($0)) min" : nil }
        meta.rating = J.number(detail, "CommunityRating").map(J.rating)
        meta.genres = J.list(detail, "Genres").compactMap { $0 as? String }
        meta.cast = people("Actor")
        meta.director = people("Director")
        meta.imdbID = ids.imdb
        meta.tmdbID = ids.tmdb
        return meta
    }

    static func report(_ client: ServerClient, _ server: Server, item: String, position: Double, finished: Bool) async throws {
        let headers = headers(device: client.device, kind: server.kind, token: server.token)
        _ = try await client.call("\(server.address)/Sessions/Playing/Stopped", headers: headers,
                                  body: ["ItemId": item, "PositionTicks": Int64(max(position, 0) * ticksPerSecond)])
        if finished {
            _ = try await client.call("\(server.address)/Users/\(server.userID)/PlayedItems/\(q(item))",
                                      headers: headers, body: [:])
        }
    }

    #if ADDONS
    /// Trellis tells its owner which add-ons it uses, at `/api/config`. Any
    /// other server has no such page, which reads as no add-ons.
    static func sharedAddons(_ client: ServerClient, _ server: Server) async throws -> [SharedAddon] {
        do {
            return readSharedAddons(try await get(client, server, "/api/config"))
        } catch ServerError.status, ServerError.badAnswer {
            return []
        }
    }

    static func readSharedAddons(_ config: Any) -> [SharedAddon] {
        J.list(config, "addons").compactMap { addon in
            if (addon as? [String: Any])?["disabled"] as? Bool == true { return nil }
            guard let url = J.text(addon, "manifestUrl"), url.hasPrefix("http://") || url.hasPrefix("https://"),
                  let name = J.text(addon, "nameOverride") ?? J.text(addon, "name") else { return nil }
            return SharedAddon(name: name, manifestURL: url)
        }
    }
    #endif
}

#if ADDONS
/// An add-on a server uses and is willing to tell its owner about. Its
/// manifest address can hold the user's keys for other services, so it is
/// never printed.
struct SharedAddon: Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    var name: String
    var manifestURL: String
    var description: String { "SharedAddon(name: \(name), ..)" }
    var debugDescription: String { description }
}
#endif
