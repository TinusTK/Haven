#if ADDONS
import Foundation
import Testing
@testable import Haven

/// The Rust build's add-on tests (haven-addons samples and url), ported.
/// GitHub build only.
struct AddonTests {
    @Test func readsAManifest() throws {
        let manifest = try AddonManifest.parse(Fixture.manifest)
        #expect(manifest.id == "org.example.sample")
        #expect(manifest.name == "Sample Catalogue")
        #expect(manifest.version == "1.2.0")
        #expect(manifest.types == ["movie", "series"])
        #expect(manifest.catalogs.count == 5)
        let top = manifest.catalogs[0]
        #expect(top.kind == "movie" && top.id == "top")
        #expect(top.title == "Popular films")
        #expect(!top.needsInput, "optional extras do not block a shelf")
        #expect(top.genres == ["Action", "Drama"])
        #expect(manifest.catalogs[4].title == "Channels")
    }

    @Test func cataloguesThatNeedAGenreOrIDsAreNotShelves() throws {
        let manifest = try AddonManifest.parse(Fixture.manifest)
        #expect(manifest.catalogs.filter { !$0.needsInput }.map(\.id) == ["top", "top", "picks"])
        // "New" needs a genre but has genres: it can be browsed; "Last videos" cannot.
        #expect(manifest.catalogs.filter(\.browsable).map(\.id) == ["top", "top", "year", "picks"])
    }

    @Test func badManifestsAreRefused() throws {
        #expect(throws: AddonError.self) { try AddonManifest.parse("<html>") }
        #expect(throws: AddonError.self) { try AddonManifest.parse(#"{"name":"x"}"#) }
        #expect(throws: AddonError.incompleteManifest("id")) { try AddonManifest.parse(#"{"id":" ","name":"x"}"#) }
        let minimal = try AddonManifest.parse(#"{"id":"a.b","name":"Tiny"}"#)
        #expect(minimal.catalogs.isEmpty)
    }

    @Test func knowsWhichCataloguesCanBeSearched() throws {
        let manifest = try AddonManifest.parse(Fixture.manifest)
        let searchable = manifest.catalogs.filter(\.searchable).map { "\($0.kind)/\($0.id)" }
        #expect(searchable == ["movie/top"])
        #expect(manifest.catalogs[0].resultsTitle == "Films")
    }

    @Test func knowsWhatAnAddonCanBeAskedFor() throws {
        let manifest = try AddonManifest.parse(Fixture.manifest)
        // "meta" is in the short form: the manifest's types and id prefixes apply.
        #expect(manifest.provides("meta", kind: "movie", id: "tt0000001"))
        #expect(manifest.provides("meta", kind: "series", id: "tt0000004"))
        #expect(!manifest.provides("meta", kind: "channel", id: "tt0000001"))
        #expect(!manifest.provides("meta", kind: "movie", id: "kitsu:1"))
        // "stream" is in the full form and only covers films.
        #expect(manifest.provides("stream", kind: "movie", id: "tt0000001"))
        #expect(!manifest.provides("stream", kind: "series", id: "tt0000004:1:1"))
        #expect(!manifest.provides("subtitles", kind: "movie", id: "tt0000001"))
        // The same answers through the add-on as a source.
        let source = manifest.source(url: "https://addon.example/manifest.json")
        #expect(source.provides("stream", kind: "movie", id: "tt0000001"))
        #expect(!source.provides("stream", kind: "series", id: "tt0000004:1:1"))
    }

    @Test func acceptsTheWaysPeoplePasteAnAddress() throws {
        let want = "https://addon.example/manifest.json"
        for input in ["https://addon.example/manifest.json", "  https://addon.example  ", "https://addon.example/",
                      "stremio://addon.example/manifest.json", "https://addon.example/manifest.json#top"] {
            #expect(try AddonManifest.manifestURL(input) == want, "\(input)")
        }
        #expect(try AddonManifest.manifestURL("http://127.0.0.1:7000/cfg=a%7Cb")
            == "http://127.0.0.1:7000/cfg=a%7Cb/manifest.json")
    }

    @Test func refusesThingsThatAreNotWebAddresses() {
        for input in ["", "   ", "addon", "ftp://addon.example", "file:///tmp/manifest.json"] {
            #expect(throws: AddonError.self, "\(input)") { try AddonManifest.manifestURL(input) }
        }
    }

    @Test func installingKeepsTheManifestAndCinemetaIsBuiltIn() async throws {
        let store = try Store.inMemory()
        let url = "https://addon.example/manifest.json"
        FixtureServer.answer(url, Fixture.manifest)
        let client = AddonClient(catalogues: Catalogues(http: HTTP(protocolClasses: [FixtureServer.self])))
        #expect(await InstalledAddons.install("https://addon.example", store: store, client: client)
            == .installed(name: "Sample Catalogue"))
        #expect(await InstalledAddons.install("https://v3-cinemeta.strem.io", store: store, client: client) == .builtIn)
        let broken = await InstalledAddons.install("https://missing.example", store: store, client: client)
        #expect(broken.isError)
        #expect(broken.message.hasPrefix("Could not install the add-on: the server answered with error 404"))

        var installed = InstalledAddons.load(store)
        #expect(installed.rows.map(\.name) == ["Sample Catalogue"])
        #expect(installed.rows[0].catalogs == 3)
        #expect(installed.sources.count == 1)

        // Switched off: listed, never asked.
        try InstalledAddons.setEnabled(url, on: false, store: store)
        installed = InstalledAddons.load(store)
        #expect(installed.rows[0].enabled == false && installed.sources.isEmpty)
        try InstalledAddons.remove(url, store: store)
        #expect(InstalledAddons.load(store).rows.isEmpty)
        #expect(try SwitchedOff.load(store).addons.isEmpty)
    }

    @Test func anOldCinemetaAddonIsRemoved() throws {
        let store = try Store.inMemory()
        try store.saveAddon(url: Cinemeta.url, manifest: #"{"id":"com.linvo.cinemeta","name":"Cinemeta"}"#)
        try store.saveAddon(url: "https://bad.example/manifest.json", manifest: "not json")
        #expect(InstalledAddons.load(store).rows.isEmpty)
        #expect(try store.addons().map(\.url) == ["https://bad.example/manifest.json"], "a broken one is kept, skipped")
    }

    @Test func streamsComeFromNextToTheManifest() async throws {
        let manifest = "https://addon.example/manifest.json"
        FixtureServer.answer("https://addon.example/stream/movie/tt0000001.json", Fixture.streams)
        let client = AddonClient(catalogues: Catalogues(http: HTTP(protocolClasses: [FixtureServer.self])))
        let streams = try await client.streams(manifest, kind: "movie", id: "tt0000001")
        #expect(streams.count == 5)
    }
}
#endif
