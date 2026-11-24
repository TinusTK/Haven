import Foundation
import Testing
@testable import Haven

/// Test answers copied from the Rust build (haven-addons/tests/fixtures):
/// the shape real services send, extras in both styles, a year sent as a
/// number, entries with missing fields.
enum Fixture {
    private final class Token {}

    static func text(_ name: String) -> String {
        let url = Bundle(for: Token.self).url(forResource: name, withExtension: "json")!
        return try! String(contentsOf: url, encoding: .utf8)
    }

    static var manifest: String { text("manifest") }
    static var catalog: String { text("catalog") }
    static var meta: String { text("meta") }
    static var streams: String { text("streams") }
}

/// The Rust build's tests of titles (haven-titles, haven-addons samples),
/// ported. Those about add-on manifests are in AddonTests.
struct TitlesTests {
    @Test func readsACatalogueAndSkipsBrokenEntries() throws {
        let metas = try Titles.parseCatalog(Fixture.catalog)
        #expect(metas.count == 4, "entries with no id, or that are not objects, are left out")
        #expect(metas[0].id == "tt0000001")
        #expect(metas[0].poster == "https://images.example/poster/tt0000001.jpg")
        #expect(metas[0].releaseInfo == "2026")
        #expect(metas[0].rating == "7.2")
        #expect(metas[1].releaseInfo == "2024", "a year sent as a number")
        #expect(metas[2].poster == nil)
        #expect(metas[3].kind == "series")
    }

    @Test func anEmptyAnswerIsAnEmptyCatalogue() throws {
        #expect(try Titles.parseCatalog("{}").isEmpty)
        #expect(try Titles.parseCatalog(#"{"metas":[]}"#).isEmpty)
        #expect(throws: TitlesError.self) { try Titles.parseCatalog("nope") }
    }

    @Test func readsDetailsWithOldAndNewSpellings() throws {
        let meta = try Titles.parseMeta(Fixture.meta)
        #expect(meta.name == "Long Runner")
        #expect(meta.rating == "8.4", "a rating sent as a number")
        #expect(meta.genres == ["Drama", "Crime"], "the current spelling wins")
        #expect(meta.cast == ["Ada Example", "Ben Sample"])
        #expect(meta.director.isEmpty, "null is an empty list")
        #expect(meta.runtime == "49 min")
        #expect(meta.videos.count == 4, "entries with no id, or not objects, are left out")
        #expect(meta.videos[0].released == "2020-03-01T05:00:00.000Z")
    }

    @Test func ordersSeasonsAndEpisodes() throws {
        let meta = try Titles.parseMeta(Fixture.meta)
        #expect(meta.seasons == [1, 2, 0], "specials go last")
        let first = meta.episodes(season: 1).map { ($0.episode, $0.title) }
        #expect(first.count == 2)
        #expect(first[0] == (1, "Pilot") && first[1] == (2, "Second"))
        #expect(meta.episodes(season: 0)[0].id == "tt0000004:0:1", "season sent as text")
    }

    @Test func aMissingTitleIsNotAParseError() {
        #expect(throws: TitlesError.unknownTitle) { try Titles.parseMeta("{}") }
        #expect(throws: TitlesError.unknownTitle) { try Titles.parseMeta(#"{"meta":{}}"#) }
        #expect(throws: TitlesError.unknownTitle) { try Titles.parseMeta(#"{"meta":null}"#) }
        #expect(throws: TitlesError.badResponse("not JSON")) { try Titles.parseMeta("nope") }
    }

    @Test func readsStreamsAndTellsHowEachIsDelivered() throws {
        let streams = try MediaStream.parseList(Fixture.streams)
        #expect(streams.map(\.kind) == [.direct, .direct, .external, .youTube, .peerToPeer],
                "entries that name no source are left out")
        #expect(streams[0].details == "Direct file, 1.2 GB")
        #expect(streams[1].details == "Older add-ons put the text here")
        #expect(try MediaStream.parseList("{}").isEmpty)
    }

    @Test func aStreamSizeHintIsReadAsNumberOrText() throws {
        let json = #"{"streams":[{"url":"https://a/1","behaviorHints":{"videoSize":123}},"#
            + #"{"url":"https://a/2","behaviorHints":{"videoSize":"456"}},{"url":"https://a/3","behaviorHints":{"videoSize":0}}]}"#
        #expect(try MediaStream.parseList(json).map(\.videoSize) == [123, 456, nil])
    }

    @Test func repeatsOfATitleAreDroppedAndTheCleanNameWins() {
        #expect(Titles.titleKey(kind: "movie", name: "Dune Part Two (2024) [1080p] [WEBRip] 88", year: nil)
            == Titles.titleKey(kind: "movie", name: "Dune: Part Two", year: "2024"))
        #expect(Titles.titleKey(kind: "movie", name: "Dune", year: "2021")
            != Titles.titleKey(kind: "movie", name: "Dune", year: "1984"))
        #expect(Titles.titleKey(kind: "movie", name: "Dune", year: "2021")
            != Titles.titleKey(kind: "series", name: "Dune", year: "2021"))
        #expect(Titles.titleKey(kind: "movie", name: "Dune: Part Two", year: "2024") == "movie|duneparttwo|2024")

        func title(_ id: String, _ kind: String, _ name: String, _ year: String?) -> MetaPreview {
            MetaPreview(id: id, kind: kind, name: name, releaseInfo: year)
        }
        let kept = Titles.withoutRepeats([
            title("a", "movie", "Dune Part Two (2024) [1080p] [WEBRip] 88", nil),
            title("b", "movie", "The Odyssey", "2026"),
            title("c", "movie", "Dune: Part Two", "2024"),
            title("d", "movie", "The Odyssey", "2026"),
            title("e", "movie", "Dune", "2021"),
            title("f", "series", "The Odyssey", "2026"),
        ])
        #expect(kept.map(\.id) == ["c", "b", "e", "f"])
    }

    // MARK: Catalogues (haven-titles)

    private func catalog(_ kind: String, _ id: String) -> Catalog { Catalog(kind: kind, id: id) }

    @Test func cataloguesSitNextToTheManifest() throws {
        #expect(try ResourceURL.catalog("https://service.example/manifest.json", catalog("movie", "top"))
            == "https://service.example/catalog/movie/top.json")
        #expect(try ResourceURL.catalog("https://service.example/cfg=a%7Cb/manifest.json", catalog("series", "new shows/1"))
            == "https://service.example/cfg=a%7Cb/catalog/series/new%20shows%2F1.json")
    }

    @Test func searchTermsAreFullyEncoded() throws {
        #expect(try ResourceURL.catalog("https://service.example/manifest.json", catalog("movie", "top"),
                                        search: "tom & jerry: 100% café?")
            == "https://service.example/catalog/movie/top/search=tom%20%26%20jerry%3A%20100%25%20caf%C3%A9%3F.json")
    }

    @Test func detailsAndStreamsUseTheTitleID() throws {
        let manifest = "https://service.example/manifest.json"
        #expect(try ResourceURL.make(manifest, resource: "meta", kind: "series", id: "tt0000004")
            == "https://service.example/meta/series/tt0000004.json")
        #expect(try ResourceURL.make(manifest, resource: "stream", kind: "series", id: "tt0000004:1:2")
            == "https://service.example/stream/series/tt0000004%3A1%3A2.json")
        #expect(try ResourceURL.make(manifest, resource: "catalog", kind: "movie", id: "top",
                                     extras: [("genre", "Sci-Fi"), ("skip", "100")])
            == "https://service.example/catalog/movie/top/genre=Sci-Fi&skip=100.json")
        #expect(throws: TitlesError.self) {
            try ResourceURL.make("https://service.example/other.json", resource: "meta", kind: "movie", id: "x")
        }
    }

    @Test func cinemetaGivesDetailsForIMDbIDsOnly() {
        let source = Cinemeta.source()
        #expect(source.provides("meta", kind: "movie", id: "tt0111161"))
        #expect(source.provides("meta", kind: "series", id: "tt0903747"))
        #expect(!source.provides("meta", kind: "movie", id: "tmdb:603"))
        #expect(!source.provides("stream", kind: "movie", id: "tt0111161"))
        #expect(source.catalogs.count == 4)
        #expect(source.catalogs[0].searchable)
        #expect(source.catalogs.map(\.title) == ["Popular films", "Popular series", "Featured films", "Featured series"])
        #expect(source.catalogs.allSatisfy { !$0.needsInput && $0.pages })
    }

    @Test func cinemetaKeepsItsLastDescription() async throws {
        let store = try Store.inMemory()
        #expect(Cinemeta.kept(store) == Cinemeta.source())
        let described = #"{"id":"com.linvo.cinemeta","name":"Cinemeta","catalogs":[{"type":"movie","id":"top","name":"Popular","extra":[{"name":"search"}]}]}"#
        FixtureServer.answer(Cinemeta.url, described)
        let catalogues = Catalogues(http: HTTP(protocolClasses: [FixtureServer.self]))
        let fresh = try #require(await Cinemeta.refresh(store, current: Cinemeta.source(), catalogues: catalogues))
        #expect(fresh.catalogs.map(\.title) == ["Popular films"])
        #expect(Cinemeta.kept(store) == fresh)
        // Asked again with nothing new: nothing changes.
        #expect(await Cinemeta.refresh(store, current: fresh, catalogues: catalogues) == nil)
    }
}

/// Answers requests from a table of address → (status, body).
/// Tests run side by side: each adds its own addresses, under a lock.
final class FixtureServer: URLProtocol {
    nonisolated(unsafe) private static var table: [String: (Int, Data)] = [:]
    private static let lock = NSLock()

    static func answer(_ url: String, status: Int = 200, _ body: String) {
        answer(url, status: status, data: Data(body.utf8))
    }

    static func answer(_ url: String, status: Int = 200, data: Data) {
        lock.lock()
        table[url] = (status, data)
        lock.unlock()
    }

    /// How many times `url` was asked for.
    nonisolated(unsafe) private static var counts: [String: Int] = [:]

    static func count(_ url: String) -> Int {
        lock.lock()
        defer { lock.unlock() }
        return counts[url] ?? 0
    }

    override class func canInit(with request: URLRequest) -> Bool { true }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    nonisolated(unsafe) private static var asked: [String: [String: String]] = [:]

    /// The headers of the last request to `url`; nil if it was not asked.
    static func headers(_ url: String) -> [String: String]? {
        lock.lock()
        defer { lock.unlock() }
        return asked[url]
    }

    override func startLoading() {
        let url = request.url!
        Self.lock.lock()
        let (status, body) = Self.table[url.absoluteString] ?? (404, Data())
        Self.counts[url.absoluteString, default: 0] += 1
        Self.asked[url.absoluteString] = request.allHTTPHeaderFields ?? [:]
        Self.lock.unlock()
        let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
