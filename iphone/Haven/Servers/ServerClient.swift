import Foundation

/// Talks to media servers. Every request first passes the Local Area
/// Network rule in the App Store build.
struct ServerClient: Sendable {
    var http: HTTP = .shared
    /// Identifies this copy of Haven to servers (`DeviceID`).
    var device: String
    /// Whether requests must stay on the user's own network.
    var ownNetworkOnly = OwnNetwork.only
    /// For reading a long answer as it arrives (stream lists).
    var streaming: URLSession = ServerClient.streamingSession

    static let streamingSession: URLSession = {
        let config = URLSessionConfiguration.default
        config.httpAdditionalHeaders = ["User-Agent": HTTP.userAgent]
        config.urlCache = nil
        return URLSession(configuration: config)
    }()

    init(device: String, http: HTTP = .shared) {
        self.device = device
        self.http = http
    }

    /// Sends a request and reads a JSON answer of limited size. A `body`
    /// makes it a POST. An empty answer reads as NSNull.
    func call(_ address: String, headers: [(String, String)], body: [String: Any]? = nil) async throws -> Any {
        guard let url = try? HTTP.webURL(address) else { throw ServerError.invalidAddress(address) }
        guard await OwnNetwork.allowed(url, enforce: ownNetworkOnly) else { throw ServerError.notOwnNetwork }
        let data = try body.map { try JSONSerialization.data(withJSONObject: $0) }
        let answer: HTTPAnswer
        do {
            answer = try await http.send(address, method: body == nil ? "GET" : "POST",
                                         headers: [("Accept", "application/json")] + headers,
                                         body: data, limit: Servers.answerLimit)
        } catch let error as HTTPError {
            switch error {
            case .tooLarge: throw ServerError.tooLarge
            case let .request(why): throw ServerError.request(why)
            case let .status(code): throw ServerError.status(code)
            case .invalidURL, .unsupportedScheme: throw ServerError.invalidAddress(address)
            }
        }
        switch answer.status {
        case 200 ..< 300: break
        case 401, 403: throw ServerError.signInRefused
        default: throw ServerError.status(answer.status)
        }
        if answer.body.allSatisfy({ $0 == 0x20 || $0 == 0x09 || $0 == 0x0a || $0 == 0x0d || $0 == 0x0c }) {
            return NSNull()
        }
        guard let json = try? JSONSerialization.jsonObject(with: answer.body, options: [.fragmentsAllowed]) else {
            throw ServerError.badAnswer
        }
        return json
    }

    // MARK: Every kind of server

    /// Signs in to a Jellyfin or Emby server with a user name and password.
    /// The password is used once and not kept.
    func signIn(_ kind: ServerKind, address: String, user: String, password: String) async throws -> Server {
        try await Emby.signIn(self, kind: kind, address: try Servers.address(address), user: user, password: password)
    }

    /// The server's libraries of films and series.
    func libraries(_ server: Server) async throws -> [MediaLibrary] {
        server.kind == .plex ? try await Plex.libraries(self, server) : try await Emby.libraries(self, server)
    }

    /// What was added to a library most recently.
    func latest(_ server: Server, library: String) async throws -> [MetaPreview] {
        server.kind == .plex
            ? try await Plex.latest(self, server, library: library)
            : try await Emby.latest(self, server, library: library)
    }

    /// A library's titles by name (the first few hundred).
    func all(_ server: Server, library: String) async throws -> [MetaPreview] {
        try await page(server, library: library, genre: nil, start: 0, length: Servers.listLength)
    }

    /// Part of a library's titles by name: `length` of them from `start`,
    /// narrowed to a genre if one is given (Jellyfin and Emby only).
    func page(_ server: Server, library: String, genre: String?, start: Int, length: Int) async throws -> [MetaPreview] {
        server.kind == .plex
            ? try await Plex.page(self, server, library: library, start: start, length: length)
            : try await Emby.page(self, server, library: library, genre: genre, start: start, length: length)
    }

    /// The genres a library or collection can be narrowed to; empty when
    /// the server does not say.
    func genres(_ server: Server, library: String) async -> [String] {
        server.kind == .plex ? [] : await Emby.genres(self, server, library: library)
    }

    /// Whether the server answers right now. Needs no sign-in.
    func online(_ server: Server) async -> Bool {
        server.kind == .plex ? await Plex.online(self, server) : await Emby.online(self, server)
    }

    /// Looks for a film or episode on the server: the item's id there, or
    /// nil if the server does not have it.
    func find(_ server: Server, _ wanted: Wanted) async throws -> String? {
        server.kind == .plex ? try await Plex.find(self, server, wanted) : try await Emby.find(self, server, wanted)
    }

    /// Titles on the server whose name matches `words`.
    func search(_ server: Server, words: String) async throws -> [MetaPreview] {
        server.kind == .plex
            ? try await Plex.search(self, server, words: words)
            : try await Emby.search(self, server, words: words)
    }

    /// Everything the server knows about one film or series.
    func meta(_ server: Server, item: String) async throws -> MetaDetail {
        server.kind == .plex ? try await Plex.meta(self, server, item: item) : try await Emby.meta(self, server, item: item)
    }

    /// The ways to play one film or episode from the server.
    func streams(_ server: Server, item: String) async throws -> [MediaStream] {
        var all: [MediaStream] = []
        try await streamsLive(server, item: item) { some in
            all += some
            return true
        }
        return all
    }

    /// As `streams`, handing the ways to `found` a few at a time while a
    /// long list is still arriving. `found` answers whether more are
    /// wanted; on no, the rest is not read.
    func streamsLive(_ server: Server, item: String, found: ([MediaStream]) -> Bool) async throws {
        if server.kind == .plex {
            _ = found(try await Plex.streams(self, server, item: item))
        } else {
            try await Emby.streamsLive(self, server, item: item, found: found)
        }
    }

    /// Tells the server how far a film or episode was watched, in seconds.
    func report(_ server: Server, item: String, position: Double, duration: Double, finished: Bool) async throws {
        if server.kind == .plex {
            try await Plex.report(self, server, item: item, position: position, duration: duration, finished: finished)
        } else {
            try await Emby.report(self, server, item: item, position: position, finished: finished)
        }
    }

    #if ADDONS
    /// The add-ons the server itself uses, when it gathers them (Trellis)
    /// and the signed-in user owns it. Empty for every other server.
    func sharedAddons(_ server: Server) async throws -> [SharedAddon] {
        server.kind == .jellyfin ? try await Emby.sharedAddons(self, server) : []
    }
    #endif
}
