import Foundation
import Testing
import UIKit
@testable import Haven

/// A PNG of the given size, drawn for the tests.
private func png(_ width: Int, _ height: Int) -> Data {
    let format = UIGraphicsImageRendererFormat()
    format.scale = 1
    return UIGraphicsImageRenderer(size: CGSize(width: width, height: height), format: format).pngData { context in
        UIColor.orange.setFill()
        context.fill(CGRect(x: 0, y: 0, width: width, height: height))
    }
}

private func tempFolder() -> URL {
    FileManager.default.temporaryDirectory.appendingPathComponent("haven-\(UUID().uuidString)", isDirectory: true)
}

/// Kept answers (the Rust build's kept.rs tests, ported) and pictures.
struct KeptTests {
    @Test func keptAnswersRoundTrip() {
        let kept = Kept(dir: tempFolder())
        defer { kept.clear() }
        kept.write("shelf|Home|Popular", ["a", "b"])
        #expect(kept.read("shelf|Home|Popular", as: [String].self) == ["a", "b"])
        #expect(kept.read("shelf|Home|Other", as: [String].self) == nil)
    }

    @Test func detailsSurviveBeingKept() throws {
        let meta = try Titles.parseMeta(#"{"meta": {"id":"tt1","type":"series","name":"Show","releaseInfo":2020, "videos":[{"id":"tt1:1:1","name":"Pilot","season":"1","number":1,"released":"2020-01-01"}]}}"#)
        let kept = Kept(dir: tempFolder())
        defer { kept.clear() }
        kept.write(Kept.details(kind: "series", id: "tt1"), meta)
        #expect(kept.read(Kept.details(kind: "series", id: "tt1"), as: MetaDetail.self) == meta)
    }

    @Test func aFreshAnswerReplacesOnlyWhenItDiffers() {
        let kept = Kept(dir: tempFolder())
        defer { kept.clear() }
        let shelf = [MetaPreview(id: "tt1", kind: "movie", name: "One")]
        #expect(kept.freshen("shelf|Home|A|Cinemeta", shelf))
        #expect(!kept.freshen("shelf|Home|A|Cinemeta", shelf))
        #expect(kept.freshen("shelf|Home|A|Cinemeta", shelf + [MetaPreview(id: "tt2", kind: "movie", name: "Two")]))
        // A damaged file reads as nothing kept.
        try? Data("{oops".utf8).write(to: kept.file("broken"))
        #expect(kept.read("broken", as: [MetaPreview].self) == nil)
    }

    @Test func omdbScoresAreKeptForAWeek() async {
        let kept = Kept(dir: tempFolder())
        defer { kept.clear() }
        let http = HTTP(protocolClasses: [FixtureServer.self])
        let url = "https://www.omdbapi.com/?i=tt0000777&apikey=k7"
        FixtureServer.answer(url, #"{"Metascore": "70"}"#)
        #expect(await kept.scores(imdb: "tt0000777", key: "k7", now: 1000, http: http)?.metacritic == "70")
        FixtureServer.answer(url, #"{"Metascore": "80"}"#)
        // Within the week: the kept ones, nothing asked.
        #expect(await kept.scores(imdb: "tt0000777", key: "k7", now: 1000 + 6 * 86_400, http: http)?.metacritic == "70")
        #expect(FixtureServer.count(url) == 1)
        // After it: fresh ones.
        #expect(await kept.scores(imdb: "tt0000777", key: "k7", now: 1000 + 8 * 86_400, http: http)?.metacritic == "80")
        // No key, or OMDb failing: the kept ones.
        #expect(await kept.scores(imdb: "tt0000777", key: nil, now: 1000 + 30 * 86_400, http: http)?.metacritic == "80")
    }

    @Test func theRustBuildsCacheIsRemovedOnce() throws {
        let paths = try AppPaths.at(tempFolder())
        defer { try? FileManager.default.removeItem(at: paths.dataDir) }
        let old = paths.dataDir.appendingPathComponent("cache/pictures", isDirectory: true)
        try FileManager.default.createDirectory(at: old, withIntermediateDirectories: true)
        try Data("x".utf8).write(to: old.appendingPathComponent("0123456789abcdef"))
        Cache.removeRustCache(paths)
        #expect(!FileManager.default.fileExists(atPath: paths.dataDir.appendingPathComponent("cache").path))
        #expect(FileManager.default.fileExists(atPath: paths.dataDir.path))
    }
}

struct PicturesTests {
    @Test func picturesAreFittedInsideTheirBoxNeverEnlarged() throws {
        let wide = try #require(Pictures.shrink(png(1000, 500), to: .poster))
        #expect(wide.size == CGSize(width: 300, height: 150))
        let tall = try #require(Pictures.shrink(png(600, 1800), to: .poster))
        #expect(tall.size == CGSize(width: 150, height: 450))
        let small = try #require(Pictures.shrink(png(100, 60), to: .background))
        #expect(small.size == CGSize(width: 100, height: 60))
        #expect(Pictures.shrink(Data("<html>".utf8), to: .poster) == nil)
        #expect(Pictures.shrink(png(8001, 2), to: .wide) == nil, "too large to read")
    }

    @Test func aPictureIsFetchedOnceThenComesFromDisk() async throws {
        let folder = tempFolder()
        defer { try? FileManager.default.removeItem(at: folder) }
        let http = HTTP(protocolClasses: [FixtureServer.self])
        let url = "https://pictures.test/p1.png?api_key=secret"
        FixtureServer.answer(url, data: png(900, 1350))
        let first = Pictures(folder: folder, http: http)
        let image = try #require(await first.load(url, .poster))
        #expect(image.size == CGSize(width: 300, height: 450))
        #expect(first.file(url, .poster) == first.file("https://pictures.test/p1.png?api_key=renewed", .poster),
                "a new token is the same picture")
        #expect(first.file(url, .poster) != first.file(url, .wide))
        // A new run: from disk, not the network.
        let second = Pictures(folder: folder, http: http)
        #expect(await second.load(url, .poster) != nil)
        #expect(FixtureServer.count(url) == 1)
        #expect(second.usage().count == 1)
        #expect(second.usageText().hasPrefix("1 picture kept on this iPhone, "))
        // A damaged file is fetched again.
        try Data("broken".utf8).write(to: second.file(url, .poster))
        let third = Pictures(folder: folder, http: http)
        #expect(await third.load(url, .poster) != nil)
        #expect(FixtureServer.count(url) == 2)
        // Not a picture, or not there: nothing, and nothing kept.
        FixtureServer.answer("https://pictures.test/page", "<html>")
        #expect(await third.load("https://pictures.test/page", .poster) == nil)
        #expect(await third.load("https://pictures.test/missing", .poster) == nil)
        #expect(third.usage().count == 1)
        third.clear(kept: Kept(dir: tempFolder()))
        #expect(third.usage().count == 0)
        #expect(third.usageText() == "No pictures are kept on this iPhone yet.")
    }

    @Test func aGateLetsSixAtATime() async {
        let gate = Gate(6)
        final class Counter: @unchecked Sendable {
            let lock = NSLock()
            var now = 0
            var most = 0
            func change(_ by: Int) {
                lock.lock()
                now += by
                most = max(most, now)
                lock.unlock()
            }
        }
        let counter = Counter()
        await withTaskGroup(of: Void.self) { group in
            for _ in 0 ..< 20 {
                group.addTask {
                    await gate.run {
                        counter.change(1)
                        try? await Task.sleep(for: .milliseconds(20))
                        counter.change(-1)
                    }
                }
            }
        }
        #expect(counter.most == 6)
        #expect(counter.now == 0)
    }

    @Test func aPauseHoldsWorkUntilOpened() async {
        let pause = Pause()
        pause.set(shut: true)
        let started = Date()
        async let passed: Void = pause.pass()
        try? await Task.sleep(for: .milliseconds(100))
        pause.set(shut: false)
        await passed
        #expect(Date().timeIntervalSince(started) >= 0.1)
        // Open: straight through.
        let quick = Date()
        await pause.pass()
        #expect(Date().timeIntervalSince(quick) < 0.1)
    }

    @MainActor
    @Test func lateAnswersAreDropped() {
        let generation = Generation()
        let first = generation.next()
        #expect(generation.isCurrent(first))
        let second = generation.next()
        #expect(!generation.isCurrent(first))
        #expect(generation.isCurrent(second))
    }
}
