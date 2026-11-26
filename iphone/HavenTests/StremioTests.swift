#if ADDONS
import Foundation
import Testing
@testable import Haven

/// The Rust build's Stremio tests (haven-addons/src/stremio.rs), ported.
/// GitHub build only.
struct StremioTests {
    @Test func onlyTheAccountsOwnAddOnsFollowIt() {
        let listed = { (url: String) in Stremio.Listed(url: url, manifest: "{}") }
        let (add, remove, kept) = Stremio.followedChanges(have: ["mine", "both", "old"], followed: ["old"],
                                                          wanted: [listed("both"), listed("new")])
        #expect(add.map(\.url) == ["new"])
        // "old" came from the account and left it; "mine" was added by hand.
        #expect(remove == ["old"])
        // "both" was installed by hand first: signing out leaves it.
        #expect(kept == ["new"])
        // Signed out: only what the account brought in goes.
        let out = Stremio.followedChanges(have: ["mine", "new"], followed: kept, wanted: [])
        #expect(out.add.isEmpty && out.kept.isEmpty)
        #expect(out.remove == ["new"])
    }

    @Test func aSignInGivesTheKeyAndEmail() throws {
        let account = try Stremio.parseLogin(["authKey": "k1", "user": ["_id": "u", "email": "a@b.c"]])
        #expect(account.email == "a@b.c" && account.authKey == "k1")
        #expect(!String(describing: account).contains("k1"))
        #expect(throws: Stremio.Failure.badAnswer) { try Stremio.parseLogin(["user": [:]]) }
    }

    @Test func theAccountIsKeptAsTheRustBuildKeepsIt() throws {
        let json = try JSONEncoder().encode(Stremio.Account(email: "a@b.c", authKey: "k1"))
        let back = try JSONSerialization.jsonObject(with: json) as? [String: String]
        #expect(back == ["email": "a@b.c", "auth_key": "k1"])
    }

    @Test func theListLeavesOutWhatHavenHasOrCannotReach() throws {
        let result: [String: Any] = ["addons": [
            ["transportUrl": Cinemeta.url, "manifest": ["id": "com.linvo.cinemeta"]],
            ["transportUrl": "http://127.0.0.1:11470/local-addon/manifest.json", "manifest": ["id": "local"]],
            ["transportUrl": "https://aiostreams.example/u/manifest.json", "manifest": ["id": "aio", "name": "AIO"]],
            ["transportUrl": "https://broken.example/manifest.json"],
        ]]
        let listed = try Stremio.parseListed(result)
        #expect(listed.count == 1)
        #expect(listed.first?.url == "https://aiostreams.example/u/manifest.json")
        #expect(listed.first?.manifest.contains("\"AIO\"") == true)
    }

    @Test func anErrorAnswerSaysWhatStremioSaid() {
        let body = Data(#"{"error":{"message":"Wrong password"}}"#.utf8)
        #expect(throws: Stremio.Failure.refused("Wrong password")) { try Stremio.result(body) }
        #expect(throws: Stremio.Failure.badAnswer) { try Stremio.result(Data("nope".utf8)) }
    }
}
#endif
