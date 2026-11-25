import Foundation
import Testing
@testable import Haven

/// The Rust build's backup (1) and setup (3) tests (`haven-core`), ported,
/// a backup the Rust build wrote, and Settings' own model.
struct BackupTests {
    private func progress(_ video: String, _ position: Double, _ at: Int64) -> WatchProgress {
        WatchProgress(kind: "series", titleID: "tt1", videoID: video, title: "A Show", detail: "", poster: nil,
                      position: position, duration: 100, finished: false, updatedAt: at)
    }

    private func server(_ name: String = "Den") -> StoredServer {
        StoredServer(kind: "jellyfin", name: name, address: "http://192.168.1.9:8096", userID: "u", token: "t")
    }

    @Test func aBackupRoundTripsThroughAFile() throws {
        let from = try Store.inMemory()
        _ = try from.saveServer(server())
        try from.putRaw("settings", #"{"theme":"light","last_page":2}"#)
        try from.put("device_id", "abc")
        let backup = try Backup.make(from, sources: true, now: 100)
        #expect((backup["settings"] as? [String: Any])?["device_id"] == nil, "a device's own settings stay with it")

        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("haven-backup-\(UUID().uuidString)")
        let file = folder.appendingPathComponent("sub").appendingPathComponent(Backup.fileName)
        try Backup.write(backup, to: file)
        try Backup.write(backup, to: file) // replacing one works too
        let read = try Backup.read(try String(contentsOf: file, encoding: .utf8))
        #expect(Backup.contents(read) == Backup.contents(backup) && Backup.madeAt(read) == 100)
        #expect((try? Backup.read(#"{"something": "else"}"#)) == nil)

        let to = try Store.inMemory()
        let merged = try Backup.restore(read, into: to, sources: true)
        #expect(merged.added == 1)
        #expect(try to.servers().first?.name == "Den")
        #expect(try to.get(Settings.key, as: Settings.self)?.theme == .light)
        try? FileManager.default.removeItem(at: folder)
    }

    @Test func aSetupGoesFromOneStoreToAnotherAndNewerProgressWins() throws {
        let from = try Store.inMemory()
        _ = try from.saveServer(StoredServer(kind: "jellyfin", name: "Den", address: "http://192.168.1.9:8096", userID: "u", token: "secret"))
        #if ADDONS
        try from.saveAddon(url: "https://one.example/manifest.json", manifest: #"{"id":"x","name":"One"}"#)
        #endif
        try from.saveProgress(progress("tt1:1:1", 50, 20))
        try from.saveProgress(progress("tt1:1:2", 10, 5))
        let to = try Store.inMemory()
        try to.saveProgress(progress("tt1:1:2", 70, 30))

        // Through JSON text and back, as a file carries it.
        let text = try Backup.text(try Backup.setup(from, sources: true))
        let setup = try #require(try JSONSerialization.jsonObject(with: Data(text.utf8)) as? [String: Any])
        // Without add-ons: they are neither sent nor taken.
        #expect((try Backup.setup(from, sources: false)["sources"] as? [Any])?.isEmpty == true)
        _ = try Backup.merge(setup, into: to, sources: false)
        #if ADDONS
        #expect(try to.addons().isEmpty)
        #endif
        #expect(try to.servers().count == 1)
        #expect(try to.progress(kind: "series", titleID: "tt1", videoID: "tt1:1:1")?.position == 50)
        // Its own newer position stays.
        #expect(try to.progress(kind: "series", titleID: "tt1", videoID: "tt1:1:2")?.position == 70)
        // A second time adds nothing.
        #expect(try Backup.merge(setup, into: to, sources: false).added == 0)
        #if ADDONS
        _ = try Backup.merge(setup, into: to, sources: true)
        #expect(try to.addons().count == 1)
        #endif
    }

    @Test func theNewerTrackingSignInWins() throws {
        let store = try Store.inMemory()
        try store.putRaw(TrackedAccount.key, #"[{"service":"Trakt","token":"old","expires_at":10}]"#)
        let setup: [String: Any] = [
            "tracking": [["service": "Trakt", "token": "new", "expires_at": 20], ["service": "Simkl", "token": "s"]],
            "progress": [["kind": "movie", "title_id": "tt9", "video_id": "tt9", "title": "A Film", "detail": "", "poster": NSNull(),
                          "position": 0.0, "duration": 0.0, "finished": true, "updated_at": 1]],
        ]
        let merged = try Backup.merge(setup, into: store, sources: false)
        #expect(merged.tracking)
        #expect(try store.progress(kind: "movie", titleID: "tt9", videoID: "tt9")?.finished == true)
        let raw = try #require(try store.rawValue(TrackedAccount.key))
        let kept = try #require(try JSONSerialization.jsonObject(with: Data(raw.utf8)) as? [[String: Any]])
        #expect(kept.count == 2 && kept.contains { $0["token"] as? String == "new" })
        // An older one does not replace it.
        let older: [String: Any] = ["tracking": [["service": "Trakt", "token": "old", "expires_at": 10]]]
        #expect(!(try Backup.merge(older, into: store, sources: false).tracking))
    }

    @Test func whatArrivesSwitchedOffStaysOff() throws {
        let setup: [String: Any] = [
            "servers": [["kind": "plex", "name": "Den", "address": "http://den", "user_id": "u", "token": "t"]],
            "tv_sources": [["url": "http://tv", "name": "TV"]],
            "off": ["servers": [["kind": "plex", "address": "http://den"]], "addons": [], "tv_sources": ["http://tv"]],
        ]
        let store = try Store.inMemory()
        _ = try Backup.merge(setup, into: store, sources: false)
        let off = try SwitchedOff.load(store)
        #expect(off.servers == [try store.servers()[0].id])
        #expect(off.tvSources == ["http://tv"])
    }

    /// A backup as the Rust build writes it (serde's pretty JSON).
    private static let rustBackup = """
        {
          "kind": "Haven backup",
          "version": 1,
          "made_at": 1764000000,
          "setup": {
            "servers": [
              {
                "kind": "emby",
                "name": "Living room",
                "address": "http://192.168.1.20:8096",
                "user_id": "abc",
                "token": "tok"
              }
            ],
            "sources": [],
            "tv_sources": [
              {
                "url": "https://tv.example/list.m3u",
                "name": "tv.example"
              }
            ],
            "keys": {
              "tmdb": "k1",
              "trakt_id": "",
              "trakt_secret": "",
              "simkl_id": "",
              "omdb": ""
            },
            "watchlist": [
              {
                "kind": "series",
                "title_id": "tt0944947",
                "title": "Game of Thrones",
                "year": "2011–2019",
                "poster": null,
                "added_at": 1700000000000
              }
            ],
            "progress": [
              {
                "kind": "movie",
                "title_id": "tt0133093",
                "video_id": "tt0133093",
                "title": "The Matrix",
                "detail": "",
                "poster": "https://images.metahub.space/poster/medium/tt0133093/img",
                "position": 1234.5,
                "duration": 8160.0,
                "finished": false,
                "updated_at": 1700000000001
              }
            ],
            "tracking": [],
            "off": {
              "servers": [],
              "addons": [],
              "tv_sources": []
            }
          },
          "settings": {
            "country": "AU",
            "player": {
              "volume": 0.8,
              "muted": false,
              "subtitle_scale": 1.2,
              "passthrough": false
            }
          }
        }
        """

    @Test func aBackupTheRustBuildWroteIsRestored() throws {
        let backup = try Backup.read(Self.rustBackup)
        #expect(Backup.madeAt(backup) == 1_764_000_000)
        let store = try Store.inMemory()
        let merged = try Backup.restore(backup, into: store)
        #expect(merged.added == 4)
        #expect(try store.servers().first?.name == "Living room")
        #expect(try store.watchlist().first?.title == "Game of Thrones" && (try store.watchlist().first?.poster) == nil)
        #expect(try store.progress(kind: "movie", titleID: "tt0133093", videoID: "tt0133093")?.position == 1234.5)
        #expect(try Keys.load(store).tmdb == "k1")
        #expect(WhereToWatch.chosenCountry(store) == "AU")
        #expect(try store.get(PlayerRules.Preferences.key, as: PlayerRules.Preferences.self)?.subtitleScale == 1.2)
        // And what Haven writes back has the same names.
        let again = try Backup.text(try Backup.make(store, sources: false, now: 1))
        for name in ["\"made_at\"", "\"user_id\"", "\"title_id\"", "\"added_at\"", "\"tv_sources\"", "\"subtitle_scale\"", "\"Haven backup\""] {
            #expect(again.contains(name), "\(name) is written")
        }
        #expect(!again.contains("\\/"), "slashes are written plainly")
    }

    @Test func whenIsSaidPlainly() {
        let utc = TimeZone(identifier: "UTC")!
        let now = Date(timeIntervalSince1970: 1_764_072_000) // 25 Nov 2025 12:00 UTC
        #expect(Backup.when(1_764_066_600, now: now, zone: utc) == "today at 10:30")
        #expect(Backup.when(1_762_165_800, now: now, zone: utc) == "3 Nov at 10:30")
    }
}

@MainActor
struct SettingsModelTests {
    private func model() throws -> (SettingsModel, AppModel) {
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("haven-settings-\(UUID().uuidString)")
        let paths = try AppPaths.at(folder)
        let core = Core(paths: paths, store: try Store.inMemory(), secrets: Secrets(), existing: false)
        let app = AppModel(core: core)
        return (app.settingsPage, app)
    }

    @Test func keysAreSavedAndUsed() throws {
        let (settings, app) = try model()
        #expect(app.library.tmdb == nil)
        settings.keys.tmdb = "  mykey "
        settings.saveKeys()
        #expect(settings.keysStatus == "Keys saved.")
        #expect(try Keys.load(settings.store).tmdb == "  mykey ")
        #expect(app.library.keys.tmdbAPIKey == "mykey")
    }

    @Test func generalChoicesAreKept() throws {
        let (settings, _) = try model()
        settings.setCountry(1)
        #expect(settings.country == 1 && WhereToWatch.chosenCountry(settings.store) == WhereToWatch.countries[0].code)
        settings.setCountry(0)
        #expect(settings.country == 0 && WhereToWatch.chosenCountry(settings.store) == nil)
        settings.setOtherPlayer(2)
        #expect(settings.otherPlayer == 2 && PlayerRules.OtherPlayer.load(settings.store).kind == 2)
        settings.setPassthrough(true)
        #expect(settings.passthrough)
    }

    @Test func theListFollowsTheBuild() {
        let sections = SettingsModel.Section.groups.flatMap(\.sections)
        #expect(sections.contains(.addons) == Build.hasAddons)
        #expect(sections.first == .general && sections.last == .about)
    }

    @Test func liveTVSourcesAreAddedAndRemoved() async throws {
        let (settings, app) = try model()
        let address = "https://tv.settings.example/list.m3u"
        FixtureServer.answer(address, """
            #EXTM3U
            #EXTINF:-1 group-title="News",One
            https://tv.settings.example/one.m3u8
            #EXTINF:-1 radio="true",Jazz
            https://tv.settings.example/jazz.mp3
            """)
        _ = app
        settings.playlistAddress = "not an address"
        settings.addPlaylist()
        #expect(settings.tvStatus?.isError == true)
        // The real fetch goes through HTTP.shared; the rules are proved in
        // LiveTVTests. Here: a source saved shows, and goes.
        try settings.store.saveTvSource(TvSource(url: address, name: "tv.settings.example", playlist: """
            #EXTM3U
            #EXTINF:-1,One
            https://tv.settings.example/one.m3u8
            """))
        #expect(settings.tvSources.map(\.name) == ["tv.settings.example"])
        settings.setTVSource(address, on: false)
        #expect(settings.tvSources.first?.enabled == false)
        settings.removeTVSource(address)
        #expect(settings.tvSources.isEmpty && settings.tvStatus?.text == "Source removed.")
    }
}
