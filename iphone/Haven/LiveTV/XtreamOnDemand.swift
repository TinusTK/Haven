import Foundation

/// Films and series from the user's Xtream Codes accounts, offered as
/// places to watch a title beside the media servers. An account's lists are
/// fetched the first time they are needed in a run and kept in memory: they
/// can be tens of thousands of titles long.
actor XtreamOnDemand {
    /// An account's whole list of films can be large.
    static let listLimit = 128 * 1024 * 1024
    static let listWait: TimeInterval = 120
    static let answerLimit = 8 * 1024 * 1024
    /// A provider may list a series several times; this many are looked into.
    static let seriesTried = 3

    private let http: HTTP
    /// Lists fetched (or being fetched) this run: by account and films/series.
    private var lists: [String: Task<[OnDemand], Error>] = [:]

    init(http: HTTP = .shared) {
        self.http = http
    }

    /// An error in words that never holds the address (it holds the
    /// account's password).
    private static func words(_ error: Error) -> String {
        switch error {
        case HTTPError.status(let code): "the server answered with error \(code)"
        case HTTPError.tooLarge: "the list was too long"
        case HTTPError.request(let why) where why.contains("too long"): "the server took too long"
        case HTTPError.invalidURL, HTTPError.unsupportedScheme: "the account's address is not usable"
        case let error as XtreamError: error.localizedDescription
        default: "the server could not be reached"
        }
    }

    /// The account's films or series, from memory or the network. Two titles
    /// opened at once fetch a list once.
    private func listed(_ account: XtreamAccount, series: Bool) async throws -> [OnDemand] {
        let key = "\(series ? "s" : "f")|\(account.sourceURL)"
        if let known = lists[key] { return try await known.value }
        let http = http
        let task = Task.detached { () throws -> [OnDemand] in
            let data = try await http.getBytes(account.api(series ? "get_series" : "get_vod_streams"),
                                               limit: Self.listLimit, timeout: Self.listWait)
            let text = String(decoding: data, as: UTF8.self)
            return series ? try Xtream.series(text) : try Xtream.films(text)
        }
        lists[key] = task
        do {
            return try await task.value
        } catch {
            // A failed list is asked for again next time.
            lists[key] = nil
            throw error
        }
    }

    /// The ways to play the wanted film or episode from one account.
    func streams(_ account: XtreamAccount, wanted: Wanted) async -> Result<[MediaStream], Problem> {
        let found: [OnDemand]
        do {
            found = Xtream.matches(try await listed(account, series: wanted.series),
                                   name: wanted.name, year: wanted.year, tmdb: wanted.tmdb)
        } catch {
            return .failure(Problem(Self.words(error)))
        }
        func file(_ ext: String) -> String { ext.isEmpty ? "File" : ext.uppercased() }
        if !wanted.series {
            return .success(found.map {
                MediaStream(name: $0.name, description: file($0.ext), url: account.film(id: $0.id, extension: $0.ext))
            })
        }
        guard let (season, episode) = wanted.episode else { return .success([]) }
        var streams: [MediaStream] = []
        for show in found.prefix(Self.seriesTried) {
            let url = "\(account.api("get_series_info"))&series_id=\(show.id)"
            guard let info = try? await http.getBytes(url, limit: Self.answerLimit, timeout: 30),
                  let (id, ext) = Xtream.episode(String(decoding: info, as: UTF8.self), season: season, episode: episode)
            else { continue }
            streams.append(MediaStream(name: "\(show.name)  S\(season) E\(episode)", description: file(ext),
                                       url: account.episode(id: id, extension: ext)))
        }
        return .success(streams)
    }

    /// Whether the accounts are worth asking about a title at all.
    static func worthAsking(_ wanted: Wanted) -> Bool {
        !wanted.name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty && !(wanted.series && wanted.episode == nil)
    }

    /// The name a stream list shows for an account.
    static func sourceLabel(_ account: XtreamAccount) -> String { "Xtream · \(account.label)" }
}
