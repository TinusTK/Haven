import Foundation
import Testing
@testable import Haven

/// Media servers, Live TV and accounts shared between devices via iCloud.
struct SharedSetupTests {
    @Test func additionsChangesAndRemovalsCarryOverWithoutComingBack() {
        let base = ["server|a": "1", "tv|x": "1", "tv|gone": "1", "account|Trakt": #"{"expires_at":5}"#]
        // Here: a server added, a source removed. There: the token renewed,
        // one source removed, another added.
        let mine = ["server|a": "1", "server|new": "1", "account|Trakt": #"{"expires_at":5}"#, "tv|gone": "1"]
        let theirs = ["server|a": "2", "tv|x": "1", "tv|more": "1", "account|Trakt": #"{"expires_at":9}"#]
        let result = SharedSetup.merge(mine: mine, theirs: theirs, base: base)
        #expect(result == ["server|a": "2", "server|new": "1", "tv|more": "1", "account|Trakt": #"{"expires_at":9}"#])
    }

    @Test func aFreshDeviceTakesEverythingAndNothingIsLostFirstTime() {
        let theirs = ["server|a": "1", "tv|x": "1"]
        #expect(SharedSetup.merge(mine: [:], theirs: theirs, base: [:]) == theirs)
        let mine = ["server|b": "1"]
        #expect(SharedSetup.merge(mine: mine, theirs: theirs, base: [:]) == ["server|a": "1", "tv|x": "1", "server|b": "1"])
    }

    @Test func aSignInChangedOnBothSidesKeepsTheOneThatLastsLonger() {
        let base = ["account|Trakt": #"{"expires_at":1}"#]
        let mine = ["account|Trakt": #"{"expires_at":7}"#]
        let theirs = ["account|Trakt": #"{"expires_at":3}"#]
        #expect(SharedSetup.merge(mine: mine, theirs: theirs, base: base) == mine)
        #expect(SharedSetup.merge(mine: theirs, theirs: mine, base: base) == mine)
    }

    @Test func applyingPutsServersSourcesFavouritesAndAccountsInPlace() throws {
        let from = try Store.inMemory()
        let id = try from.saveServer(StoredServer(kind: "jellyfin", name: "Home", address: "http://h", userID: "u", token: "t"))
        try from.saveTvSource(TvSource(url: "http://tv/list.m3u", name: "TV", playlist: "#EXTM3U"))
        var off = try SwitchedOff.load(from)
        off.servers = [id]
        try off.save(from)
        try from.putRaw(LiveTV.favouritesKey, #"[{"name":"News","channels":[]}]"#)
        try from.putRaw(TrackedAccount.key, #"[{"service":"Trakt","expires_at":9}]"#)
        let shared = try SharedSetup.snapshot(from)

        let to = try Store.inMemory()
        let mine = try SharedSetup.snapshot(to)
        let applied = try SharedSetup.apply(shared, over: mine, into: to)
        #expect(applied == .init(servers: true, tv: true, favourites: true, accounts: true))
        #expect(try SharedSetup.snapshot(to) == shared)
        let server = try #require(try to.servers().first)
        let offNow = try SwitchedOff.load(to).servers
        #expect(server.token == "t" && offNow == [server.id])

        // Removed on the other device: removed here too.
        let removed = shared.filter { !$0.key.hasPrefix("server|") && !$0.key.hasPrefix("tv|") }
        try SharedSetup.apply(removed, over: shared, into: to)
        let (servers, sources) = (try to.servers(), try to.tvSources())
        #expect(servers.isEmpty && sources.isEmpty)
        #expect(try SharedSetup.snapshot(to) == removed)
    }

    @Test func theSharedTextReadsBack() {
        let shot = ["server|jellyfin|http://h|u": #"{"name":"Home","off":false,"token":"t"}"#]
        #expect(SharedSetup.decode(SharedSetup.encode(shot)) == shot)
        #expect(SharedSetup.decode(nil) == nil)
        #expect(SharedSetup.decode("not json") == nil)
    }
}
