import Foundation
import Testing
@testable import Haven

private func json(_ text: String) -> Any {
    try! JSONSerialization.jsonObject(with: Data(text.utf8), options: [.fragmentsAllowed])
}

/// JSON values compared as JSON.
private func same(_ a: Any, _ b: String) -> Bool {
    NSDictionary(dictionary: a as! [String: Any]).isEqual(to: json(b) as! [String: Any])
}

/// The Rust build's tracking tests (haven-tracking lib and sync), ported.
/// The PKCE test is left out: PKCE is the desktop's browser sign-in; the
/// iPhone links with a code.
struct TrackingRulesTests {
    @Test func simklAddressesCarryTheAppsNameAndVersion() {
        let url = TrackingClient.simklURL(TrackingClient.simklAPI, "/sync/history", clientID: "abc")
        #expect(url.hasPrefix("https://api.simkl.com/sync/history?client_id=abc&app-name=haven&app-version="))
        #expect(TrackingClient.simklURL(TrackingClient.simklAPI, "/x?y=1", clientID: "abc").contains("/x?y=1&client_id=abc"))
        #expect(TrackingClient.formEscape("media:read media:write") == "media%3Aread%20media%3Awrite")
    }

    @Test func aSimklTokenKeepsItsRefreshToken() throws {
        let account = try #require(Tracking.readSimklToken(json(#"{"access_token": "a", "refresh_token": "r", "expires_in": 604800}"#), now: 100, refresh: nil))
        #expect(account.refresh == "r" && account.expiresAt == 604_900)
        let renewed = try #require(Tracking.readSimklToken(json(#"{"access_token": "b", "expires_in": 10}"#), now: 0, refresh: "r"))
        #expect(renewed.refresh == "r")
    }

    @Test func aSecretGoesAlongOnlyWhenThereIsOne() {
        #expect(same(TrackingClient.withSecret(["a": 1], nil), #"{"a": 1}"#))
        #expect(same(TrackingClient.withSecret(["a": 1], "s"), #"{"a": 1, "client_secret": "s"}"#))
    }

    @Test func traktsWatchedListIsRead() {
        let shows = json("""
            [{"last_watched_at": "2026-01-02T03:04:05.000Z", "show": {"title": "A Show", "ids": {"imdb": "tt1"}},
              "seasons": [{"number": 2, "episodes": [{"number": 5, "last_watched_at": "2026-01-02T03:04:05.000Z"}]}]},
             {"show": {"title": "No id", "ids": {}}, "seasons": []}]
            """)
        let found = Tracking.readWatched(shows, series: true)
        #expect(found.count == 1)
        #expect(found[0].episode?.season == 2 && found[0].episode?.number == 5)
        #expect(found[0].at == 1_767_323_045)
        let films = json(#"[{"movie": {"title": "F", "ids": {"imdb": "tt2"}}, "last_watched_at": "2026-01-02T03:04:05Z"}]"#)
        #expect(Tracking.readWatched(films, series: false)[0].episode == nil)
        #expect(Tracking.readWatched(films, series: false)[0].at == 1_767_323_045)
    }

    @Test func subjectsNeedAnIMDbID() {
        #expect(Tracking.subject(kind: "movie", titleID: "tt0111161", videoID: "tt0111161") == Subject(imdb: "tt0111161", series: false))
        #expect(Tracking.subject(kind: "series", titleID: "tt0903747", videoID: "tt0903747:2:5")
            == Subject(imdb: "tt0903747", series: true, episode: (2, 5)))
        // The series itself, or an episode id in another shape.
        #expect(Tracking.subject(kind: "series", titleID: "tt0903747", videoID: nil)?.episode == nil)
        #expect(Tracking.subject(kind: "series", titleID: "tt0903747", videoID: "tt1:2:5")?.episode == nil)
        #expect(Tracking.subject(kind: "series", titleID: "tt0903747", videoID: "tt0903747:2:5:9")?.episode == nil)
        #expect(Tracking.subject(kind: "movie", titleID: "srv:1:abc", videoID: nil) == nil)
        #expect(Tracking.subject(kind: "movie", titleID: "hv1", videoID: nil) == nil)
    }

    @Test func bodiesNameFilmsSeriesAndEpisodes() {
        let film = Subject(imdb: "tt1", series: false)
        #expect(same(Tracking.body(film), #"{"movies": [{"ids": {"imdb": "tt1"}}]}"#))
        let episode = Subject(imdb: "tt2", series: true, episode: (1, 3))
        #expect(same(Tracking.body(episode), #"{"shows": [{"ids": {"imdb": "tt2"}, "seasons": [{"number": 1, "episodes": [{"number": 3}]}]}]}"#))
        #expect(same(Tracking.body(film, simklList: "plantowatch"), #"{"movies": [{"ids": {"imdb": "tt1"}, "to": "plantowatch"}]}"#))
    }

    @Test func watchlistsOfBothServicesAreRead() {
        let trakt = json(#"[{"movie": {"title": "A Film", "year": 2020, "ids": {"imdb": "tt10", "trakt": 1}}}, {"movie": {"title": "No IMDb id", "year": 2021, "ids": {"trakt": 2}}}]"#)
        #expect(Tracking.readListed(trakt, series: false) == [Listed(imdb: "tt10", series: false, title: "A Film", year: "2020")])
        let simkl = json(#"{"shows": [{"show": {"title": "A Show", "ids": {"imdb": "tt20"}}}]}"#)
        let shows = Tracking.readListed(simkl, series: true)
        #expect(shows[0].title == "A Show" && shows[0].year == "" && shows[0].series)
        #expect(Tracking.readListed(NSNull(), series: true).isEmpty)
    }

    @Test func aTraktTokenKnowsWhenItRunsOut() throws {
        let account = try #require(Tracking.readTraktToken(json(#"{"access_token": "a", "refresh_token": "r", "expires_in": 86400, "created_at": 1000}"#), now: 5))
        #expect(account.expiresAt == 87_400)
        #expect(account.refresh == "r")
        #expect(!String(describing: account).contains("\"a\"") && !String(reflecting: account).contains("token: a"))
        #expect(Tracking.readTraktToken(json("{}"), now: 5) == nil)
    }

    @Test func withoutKeysNothingIsAsked() async {
        let client = TrackingClient(keys: Secrets())
        #expect(!client.hasKeys(.trakt) && !client.hasKeys(.simkl))
        await #expect(throws: TrackingError.noKey("Trakt")) { _ = try await client.begin(.trakt) }
    }

    @Test func whatAccountsHoldIsAddedOnce() throws {
        let store = try Store.inMemory()
        let listed = [Listed(imdb: "tt1", series: true, title: "A Show", year: "2020")]
        #expect(Sync.addListed(store, listed, now: 100) == 1)
        #expect(Sync.addListed(store, listed, now: 100) == 0)
        let watched = [Watched(imdb: "tt1", title: "A Show", episode: (1, 2), at: 5)]
        #expect(Sync.addWatched(store, watched) == 1)
        #expect(Sync.addWatched(store, watched) == 0)
        let saved = try #require(try store.progress(kind: "series", titleID: "tt1", videoID: "tt1:1:2"))
        #expect(saved.finished && saved.detail == "S1 E2" && saved.updatedAt == 5000)
    }

    @Test func accountsAndOptionsAreKeptInTheRustBuildsShape() throws {
        let store = try Store.inMemory()
        // As the Rust build saved them.
        let rust = try JSONDecoder().decode([TrackedAccount].self, from: Data(#"[{"service": "trakt", "token": "t", "refresh": "r", "expires_at": 99}, {"service": "simkl", "token": "s", "refresh": null, "expires_at": null}]"#.utf8))
        #expect(rust == [TrackedAccount(service: .trakt, token: "t", refresh: "r", expiresAt: 99), TrackedAccount(service: .simkl, token: "s")])
        TrackedAccount.save(rust, store: store)
        #expect(TrackedAccount.load(store) == rust)
        let saved = String(decoding: try Store.encoder.encode(TrackedAccount(service: .simkl, token: "x", expiresAt: 5)), as: UTF8.self)
        #expect(saved.contains("\"service\":\"simkl\"") && saved.contains("\"expires_at\":5"))
        let options = try JSONDecoder().decode(Sync.Options.self, from: Data(#"{"watched": false, "last_sync": 7, "off": ["Simkl"]}"#.utf8))
        #expect(options.watched && options.lastSync == 7 && options.off == ["Simkl"], "the old switches are gone: always on")
        let text = String(decoding: try Store.encoder.encode(options), as: UTF8.self)
        #expect(text.contains("\"last_sync\":7") && text.contains("\"last_pull\":0"))
    }
}

/// Talking to the services, a whole sync round and the Tracker, against a
/// stand-in server.
@MainActor
struct TrackerTests {
    private let keys = Secrets(traktClientID: "tid", simklClientID: "sid")

    private func client(_ name: String) -> TrackingClient {
        var client = TrackingClient(keys: keys, http: HTTP(protocolClasses: [FixtureServer.self]))
        client.trakt = "https://\(name).trakt.test"
        client.simkl = "https://\(name).simkl.test"
        return client
    }

    @Test func aTraktCodeIsGivenAndApproved() async throws {
        let client = client("t1")
        FixtureServer.answer("https://t1.trakt.test/oauth/device/code",
                             #"{"user_code": "ABCD1234", "verification_url": "https://trakt.tv/activate", "device_code": "dev", "interval": 0, "expires_in": 600}"#)
        let code = try await client.begin(.trakt)
        #expect(code.userCode == "ABCD1234" && code.interval == 1, "never asks more than once a second")
        FixtureServer.answer("https://t1.trakt.test/oauth/device/token", #"{"access_token": "tok", "refresh_token": "ref", "expires_in": 100, "created_at": 1}"#)
        #expect(try await client.poll(code, now: 0)?.token == "tok")
        FixtureServer.answer("https://t1.trakt.test/oauth/device/token", status: 400, "{}")
        #expect(try await client.poll(code, now: 0) == nil, "not approved yet")
        FixtureServer.answer("https://t1.trakt.test/oauth/device/token", status: 410, "{}")
        await #expect(throws: TrackingError.expired) { _ = try await client.poll(code, now: 0) }
    }

    @Test func aSimklCodeComesWithItsLinkFilledIn() async throws {
        let client = client("s1")
        let device = TrackingClient.simklURL("https://s1.simkl.test", "/oauth2/device", clientID: "sid")
        FixtureServer.answer(device, #"{"user_code": "XY12", "verification_uri": "https://simkl.com/pin", "verification_uri_complete": "https://simkl.com/pin/XY12", "device_code": "d"}"#)
        let code = try await client.begin(.simkl)
        #expect(code.link == "https://simkl.com/pin/XY12" && code.expiresIn == 900)
        let token = TrackingClient.simklURL("https://s1.simkl.test", "/oauth2/token", clientID: "sid")
        FixtureServer.answer(token, status: 400, #"{"error": "authorization_pending"}"#)
        #expect(try await client.poll(code, now: 0) == nil)
        FixtureServer.answer(token, status: 400, #"{"error": "expired_token"}"#)
        await #expect(throws: TrackingError.expired) { _ = try await client.poll(code, now: 0) }
    }

    @Test func aRoundRenewsSendsAndBringsIn() async throws {
        let client = client("r1")
        let near = TrackedAccount(service: .trakt, token: "old", refresh: "ref", expiresAt: 1000)
        FixtureServer.answer("https://r1.trakt.test/oauth/token", #"{"access_token": "new", "refresh_token": "ref2", "expires_in": 86400, "created_at": 900}"#)
        FixtureServer.answer("https://r1.trakt.test/sync/history", "{}")
        FixtureServer.answer("https://r1.trakt.test/sync/watchlist/movies", #"[{"movie": {"title": "Listed", "year": 2021, "ids": {"imdb": "tt5"}}}]"#)
        FixtureServer.answer("https://r1.trakt.test/sync/watchlist/shows", "[]")
        FixtureServer.answer("https://r1.trakt.test/sync/watched/movies", #"[{"movie": {"title": "Seen", "ids": {"imdb": "tt6"}}}]"#)
        FixtureServer.answer("https://r1.trakt.test/sync/watched/shows", "[]")
        let queued = [
            Change(id: 1, what: "watched", kind: "movie", titleID: "tt7", videoID: "tt7", createdAt: 0),
            Change(id: 2, what: "watched", kind: "series", titleID: "tt8", videoID: "", createdAt: 0),
            Change(id: 3, what: "watched", kind: "movie", titleID: "srv:1:x", videoID: "", createdAt: 0),
            Change(id: 4, what: "watchlist_add", kind: "movie", titleID: "tt9", videoID: "", createdAt: 0),
        ]
        let outcome = await Sync.exchange(client, accounts: [near], queued: queued, pull: true, now: 950)
        #expect(outcome.renewed.map(\.token) == ["new"])
        // 1 sent, 2 nothing to send, 3 not trackable; 4 failed (no answer set up: 404).
        #expect(outcome.sent == [1, 2, 3])
        #expect(outcome.problems == ["Trakt: the service answered with error 404"])
        #expect(outcome.watchlists.map(\.imdb) == ["tt5"] && outcome.watched.map(\.imdb) == ["tt6"])
        var accounts = [near]
        Sync.keepRenewed(&accounts, outcome.renewed)
        #expect(accounts.map(\.token) == ["new"])
    }

    @Test func theTrackerQueuesSendsAndSaysHowItStands() async throws {
        let store = try Store.inMemory()
        try store.put(TrackedAccount.key, [TrackedAccount(service: .trakt, token: "t")])
        let tracker = Tracker(store: store, keys: keys, http: HTTP(protocolClasses: [FixtureServer.self]))
        tracker.traktAPI = "https://q1.trakt.test"
        tracker.afterAChange = 0.05
        tracker.now = { 1_767_323_045 }
        #expect(tracker.status.text == "Not synced yet. Everything is in step with Trakt.")
        // Not trackable: not queued.
        tracker.trackWatched(kind: "movie", titleID: "srv:1:x", videoID: "srv:1:x")
        #expect(try store.queuedChanges().isEmpty)
        // Trackable, but the service fails: it stays queued and the problem shows.
        tracker.trackWatched(kind: "movie", titleID: "tt1", videoID: "tt1")
        #expect(tracker.status.text == "Not synced yet. 1 change still to send to Trakt.")
        try await Task.sleep(for: .seconds(0.5))
        #expect(try store.queuedChanges().count == 1)
        #expect(tracker.status.problem)
        #expect(tracker.status.text.hasSuffix("Last try: Trakt: the service answered with error 404. Haven tries again later."))
        // The service answers: sent, and in step.
        FixtureServer.answer("https://q1.trakt.test/sync/history", "{}")
        await tracker.run(pull: false)
        #expect(try store.queuedChanges().isEmpty)
        #expect(tracker.status.text.hasSuffix(". Everything is in step with Trakt."))
        #expect(tracker.status.text.hasPrefix("Last synced "))
        // Switched off: nothing syncs, and nothing is queued.
        tracker.setSync(.trakt, on: false)
        #expect(tracker.status.rows[0].detail == "Linked, but not syncing.")
        #expect(tracker.status.text.hasPrefix("Link Trakt or Simkl"))
        tracker.trackWatchlist(kind: "movie", titleID: "tt2", on: true)
        #expect(try store.queuedChanges().isEmpty)
    }

    @Test func linkingShowsTheCodeAndKeepsTheAccount() async throws {
        let store = try Store.inMemory()
        let tracker = Tracker(store: store, keys: keys, http: HTTP(protocolClasses: [FixtureServer.self]))
        tracker.traktAPI = "https://l1.trakt.test"
        #expect(tracker.rowText(.trakt) == "Not linked. Link opens trakt.tv and shows a short code to approve there.")
        FixtureServer.answer("https://l1.trakt.test/oauth/device/code", #"{"user_code": "CODE", "verification_url": "https://trakt.tv/activate", "device_code": "dev", "interval": 1}"#)
        FixtureServer.answer("https://l1.trakt.test/oauth/device/token", #"{"access_token": "tok"}"#)
        var opened: [String] = []
        tracker.openLink = { opened.append($0) }
        tracker.connect(.trakt)
        for _ in 0 ..< 40 where tracker.account(.trakt) == nil { try await Task.sleep(for: .seconds(0.1)) }
        #expect(opened == ["https://trakt.tv/activate"])
        #expect(tracker.linkStatus == "Linked to Trakt.")
        #expect(TrackedAccount.load(store).map(\.token) == ["tok"])
        #expect(tracker.rowText(.trakt).hasPrefix("Linked:"))
        tracker.disconnect(.trakt)
        #expect(tracker.linkStatus == "Unlinked from Trakt. Haven has forgotten the sign-in.")
        #expect(TrackedAccount.load(store).isEmpty)
        let bare = Tracker(store: store, keys: Secrets())
        #expect(bare.rowText(.simkl) == "Not available in this copy of Haven yet (no Simkl app key).")
    }
}

/// Media-server titles linked to their IMDb id for Trakt and Simkl.
struct IMDbLinkTests {
    /// `srv:1:<hex of s|tt7631058|series>`, as a gathering server names it.
    let gathered = "srv:1:" + "s|tt7631058|series".utf8.map { String(format: "%02x", $0) }.joined()

    @Test func aGatheredIdSpellsItsIMDbId() {
        #expect(IMDbLink.of(titleID: gathered, detail: nil)?.imdb == "tt7631058")
        #expect(IMDbLink.of(titleID: "tt7631058", detail: nil) == nil)
        #expect(IMDbLink.of(titleID: "srv:1:abcdef12", detail: nil) == nil)
    }

    @Test func episodesAreSentAndShownUnderTheirNumbers() {
        let link = IMDbLink(imdb: "tt1", episodes: ["e1": (1, 1), "e2": (1, 2)])
        #expect(link.serviceIDs(kind: "series", videoID: "e2")! == ("tt1", "tt1:1:2"))
        #expect(link.serviceIDs(kind: "series", videoID: "unknown") == nil)
        #expect(link.serviceIDs(kind: "series", videoID: "")! == ("tt1", ""))
        #expect(link.serviceIDs(kind: "movie", videoID: "srv:1:x")! == ("tt1", "tt1"))
        let done = link.merged(kind: "series", titleID: "s", own: ["e1"], elsewhere: ["tt1:1:2", "tt1:5:5"])
        #expect(done == ["e1", "e2"])
        #expect(link.merged(kind: "movie", titleID: "m", own: [], elsewhere: ["tt1"]) == ["m"])
    }
}
