import Foundation
import Testing
@testable import Haven

/// The Rust build's settings, secrets, paths, http, diag and own-network
/// tests, ported, plus the size limit on answers.
struct SettingsTests {
    @Test func defaultsWhenNothingSaved() throws {
        let store = try Store.inMemory()
        #expect(try Settings.load(store) == Settings())
        #expect(Settings().theme == .dark)
    }

    @Test func savesAndLoads() throws {
        let store = try Store.inMemory()
        let settings = Settings(theme: .light, lastPage: 3)
        try settings.save(store)
        #expect(try Settings.load(store) == settings)
    }

    @Test func typedKeysWinOverTheSecretsFile() throws {
        let store = try Store.inMemory()
        #expect(try Keys.load(store) == Keys())
        let keys = Keys(tmdb: " typed ")
        try keys.save(store)
        let file = Secrets.parse("TMDB_API_KEY=filed\nSIMKL_CLIENT_ID=simkl")
        let resolved = try Keys.load(store).resolved(file: file, builtIn: Secrets(simklClientID: "built"))
        #expect(resolved.tmdbAPIKey == "typed")
        #expect(resolved.simklClientID == "simkl")
        #expect(!String(describing: keys).contains("typed"))
        #expect(!String(reflecting: keys).contains("typed"))
    }

    @Test func builtInKeysComeLastAndEmptyOnesCountAsMissing() {
        let resolved = Keys().resolved(file: Secrets(), builtIn: Secrets(traktClientID: "built", omdbAPIKey: ""))
        #expect(resolved.traktClientID == "built")
        #expect(resolved.omdbAPIKey == nil)
        #expect(Secrets.builtIn.tmdbAPIKey == nil) // never a built-in TMDB key
    }

    @Test func switchingOffIsRememberedAndUndone() throws {
        let store = try Store.inMemory()
        var off = try SwitchedOff.load(store)
        #expect(off == SwitchedOff())
        SwitchedOff.switch(&off.servers, 3, on: false)
        SwitchedOff.switch(&off.servers, 3, on: false)
        SwitchedOff.switch(&off.addons, "https://a.example/manifest.json", on: false)
        try off.save(store)
        off = try SwitchedOff.load(store)
        #expect(off.servers == [3])
        SwitchedOff.switch(&off.servers, 3, on: true)
        #expect(off.servers.isEmpty)
        #expect(off.addons.count == 1)
    }

    @Test func olderSavedSettingsStillLoad() throws {
        let store = try Store.inMemory()
        try store.put(Settings.key, ["theme": "light"])
        let loaded = try Settings.load(store)
        #expect(loaded.theme == .light)
        #expect(loaded.lastPage == 0)
    }

    @Test func savedJSONMatchesTheRustBuild() throws {
        let store = try Store.inMemory()
        try SwitchedOff(addons: ["https://a/manifest.json"], servers: [2], tvSources: []).save(store)
        // Rust's serde names, and slashes left alone.
        let json = try #require(try store.get(SwitchedOff.key, as: [String: AnyCodableArray].self))
        #expect(json.keys.sorted() == ["addons", "servers", "tv_sources"])
        let data = try Store.encoder.encode(Keys(traktID: "x"))
        let text = String(decoding: data, as: UTF8.self)
        #expect(text.contains("\"trakt_id\":\"x\""))
        #expect(!String(decoding: try Store.encoder.encode(["u": "a/b"]), as: UTF8.self).contains("\\/"))
    }
}

/// Reads any JSON array, for checking key names only.
struct AnyCodableArray: Decodable {
    init(from decoder: Decoder) throws {
        _ = try decoder.unkeyedContainer()
    }
}

struct SecretsTests {
    @Test func parsesCommentsQuotesAndExport() {
        let secrets = Secrets.parse(
            "# keys\nTMDB_API_KEY = abc123\nexport TRAKT_CLIENT_ID=\"id\"\n"
                + "TRAKT_CLIENT_SECRET='s=1'\nSIMKL_CLIENT_ID=\nUNKNOWN=x\nnot a line\r\nOMDB_API_KEY=o\r\n")
        #expect(secrets.tmdbAPIKey == "abc123")
        #expect(secrets.traktClientID == "id")
        #expect(secrets.traktClientSecret == "s=1")
        #expect(secrets.simklClientID == nil)
        #expect(secrets.omdbAPIKey == "o")
    }

    @Test func missingFileGivesEmptySecrets() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("none-\(UUID().uuidString).env")
        #expect(try Secrets.fromFile(url) == Secrets())
    }

    @Test func printingHidesValues() {
        let secrets = Secrets.parse("TMDB_API_KEY=topsecret")
        let shown = String(describing: secrets) + String(reflecting: secrets)
        #expect(!shown.contains("topsecret"))
        #expect(shown.contains("set"))
    }
}

struct PathsTests {
    @Test func createsTheFolderAndNamesTheFiles() throws {
        let base = FileManager.default.temporaryDirectory.appendingPathComponent("haven-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: base) }
        let paths = try AppPaths.at(base.appendingPathComponent("nested/haven"))
        var isDir: ObjCBool = false
        #expect(FileManager.default.fileExists(atPath: paths.dataDir.path, isDirectory: &isDir) && isDir.boolValue)
        #expect(paths.database.lastPathComponent == "haven.sqlite3")
        #expect(paths.logFile.lastPathComponent == "haven.log")
        #expect(paths.secretsFile.lastPathComponent == "secrets.env")
    }

    @Test func theUsualPlaceIsWhereTheRustBuildKeptIt() throws {
        let paths = try AppPaths.discover()
        #expect(paths.dataDir.path.hasSuffix("/Library/Application Support/Haven"))
    }
}

struct LogTests {
    @Test func loggedAddressesCarryNoSecrets() {
        #expect(Log.plainAddress("https://me:secret@tv.example.org:8096/Items/1/Images/Primary?api_key=abc&tag=1")
            == "https://tv.example.org:8096/Items/1/Images/Primary")
        #expect(Log.plainAddress("http://tv.example.org?x=1") == "http://tv.example.org")
        #expect(Log.plainAddress("nonsense") == "nonsense")
    }

    @Test func theLogStartsAfreshWhenTooBig() throws {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("haven-\(UUID().uuidString).log")
        defer { try? FileManager.default.removeItem(at: url) }
        try Data(repeating: 65, count: Log.maxBytes + 10).write(to: url)
        Log.start(url)
        let size = try FileManager.default.attributesOfItem(atPath: url.path)[.size] as? Int ?? 0
        #expect(size < 200)
    }
}

struct OwnNetworkTests {
    @Test func homeNetworkAndTailscaleAreOwn() {
        for own in ["192.168.1.9", "10.0.0.4", "172.16.0.1", "172.31.255.254", "127.0.0.1",
                    "169.254.3.3", "100.64.0.1", "100.101.102.103", "100.127.255.255",
                    "::1", "fd7a:115c:a1e0::1", "fc00::5", "fe80::1", "::ffff:192.168.0.2"] {
            #expect(OwnNetwork.isOwn(own), "\(own)")
        }
    }

    @Test func publicAddressesAreNot() {
        for outside in ["8.8.8.8", "1.1.1.1", "172.32.0.1", "100.63.255.255", "100.128.0.1",
                        "203.0.113.5", "2001:4860:4860::8888", "::ffff:8.8.8.8"] {
            #expect(!OwnNetwork.isOwn(outside), "\(outside)")
        }
    }

    @Test func names() {
        #expect(OwnNetwork.isLocalName("nas.local"))
        #expect(OwnNetwork.isLocalName("localhost"))
        #expect(!OwnNetwork.isLocalName("demo.jellyfin.org"))
        #expect(OwnNetwork.isPlexTV("plex.tv"))
        #expect(OwnNetwork.isPlexTV("clients.plex.tv"))
        #expect(!OwnNetwork.isPlexTV("notplex.tv"))
        #expect(!OwnNetwork.isPlexTV("x.plex.direct"))
    }

    @Test func requestsAreCheckedByAddress() async {
        func url(_ s: String) -> URL { URL(string: s)! }
        #expect(await OwnNetwork.allowed(url("http://192.168.1.10:8096/x"), enforce: true))
        #expect(await OwnNetwork.allowed(url("http://[fd7a:115c:a1e0::1]:8096/"), enforce: true))
        #expect(await !OwnNetwork.allowed(url("http://8.8.8.8/"), enforce: true))
        #expect(await OwnNetwork.allowed(url("http://nas.local:8096/"), enforce: true))
        #expect(await OwnNetwork.allowed(url("https://plex.tv/api/v2/pins"), enforce: true))
        #expect(await OwnNetwork.allowed(url("http://localhost:8096/"), enforce: true))
        // Not enforced in the GitHub build.
        #expect(await OwnNetwork.allowed(url("http://8.8.8.8/"), enforce: false))
    }
}

struct HTTPTests {
    @Test func acceptsOnlyWebAddresses() throws {
        #expect(try HTTP.webURL(" https://example.com/a ").absoluteString == "https://example.com/a")
        #expect(throws: Never.self) { try HTTP.webURL("http://127.0.0.1:7000/manifest.json") }
        #expect(throws: HTTPError.unsupportedScheme) { try HTTP.webURL("file:///etc/passwd") }
        #expect(throws: HTTPError.invalidURL("not an address")) { try HTTP.webURL("not an address") }
    }

    @Test func answersAreReadUpToTheLimit() async throws {
        let http = HTTP(protocolClasses: [StubServer.self])
        let body = try await http.getBytes("https://stub.test/ok", limit: 100)
        #expect(body == Data("hello".utf8))
        await #expect(throws: HTTPError.tooLarge(limit: 4)) {
            _ = try await http.getBytes("https://stub.test/ok", limit: 4)
        }
        await #expect(throws: HTTPError.status(404)) {
            _ = try await http.getBytes("https://stub.test/missing", limit: 100)
        }
        // send() hands back an error's body with its status.
        let answer = try await http.send("https://stub.test/missing", limit: 100)
        #expect(answer.status == 404 && answer.body == Data("no".utf8))
    }

    @Test func sendsHavensNameAndTheBody() async throws {
        let http = HTTP(protocolClasses: [StubServer.self])
        let answer = try await http.send("https://stub.test/echo", headers: [("X-Test", "1")],
                                         body: Data("{\"a\":1}".utf8), limit: 1000)
        let echoed = String(decoding: answer.body, as: UTF8.self)
        #expect(echoed.contains("POST"))
        #expect(echoed.contains("Haven/"))
        #expect(echoed.contains("application/json"))
        #expect(echoed.contains("X-Test=1"))
    }
}

/// Answers requests to stub.test without a network.
final class StubServer: URLProtocol {
    override class func canInit(with request: URLRequest) -> Bool { request.url?.host() == "stub.test" }
    override class func canonicalRequest(for request: URLRequest) -> URLRequest { request }

    override func startLoading() {
        let url = request.url!
        var status = 200
        var body = Data()
        switch url.path() {
        case "/ok":
            body = Data("hello".utf8)
        case "/echo":
            let agent = request.value(forHTTPHeaderField: "User-Agent") ?? ""
            let type = request.value(forHTTPHeaderField: "Content-Type") ?? ""
            let test = request.value(forHTTPHeaderField: "X-Test") ?? ""
            body = Data("\(request.httpMethod ?? "") \(agent) \(type) X-Test=\(test)".utf8)
        default:
            status = 404
            body = Data("no".utf8)
        }
        let response = HTTPURLResponse(url: url, statusCode: status, httpVersion: "HTTP/1.1", headerFields: nil)!
        client?.urlProtocol(self, didReceive: response, cacheStoragePolicy: .notAllowed)
        client?.urlProtocol(self, didLoad: body)
        client?.urlProtocolDidFinishLoading(self)
    }

    override func stopLoading() {}
}
