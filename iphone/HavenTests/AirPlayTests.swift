import Foundation
import Testing
@testable import Haven

/// The Rust build's resume and clock tests (haven-player), ported.
struct ResumeTests {
    @Test func judgesTheStartMiddleAndEnd() {
        #expect(Resume.judge(position: 5, duration: 3600) == .started)
        #expect(Resume.judge(position: 30, duration: 3600) == .partway)
        #expect(Resume.judge(position: 1800, duration: 3600) == .partway)
        #expect(Resume.judge(position: 3420, duration: 3600) == .finished)
        #expect(Resume.judge(position: 3600, duration: 3600) == .finished)
    }

    @Test func aLiveStreamHasNoPlaceToRemember() {
        #expect(Resume.judge(position: 500, duration: 0) == nil)
        #expect(Resume.judge(position: 500, duration: .nan) == nil)
        #expect(Resume.judge(position: .infinity, duration: 100) == nil)
        #expect(Resume.from(position: 500, duration: 0, finished: false) == nil)
    }

    @Test func resumesALittleBeforeWhereItStopped() {
        #expect(Resume.from(position: 600, duration: 3600, finished: false) == 597)
        #expect(Resume.from(position: 10, duration: 3600, finished: false) == nil)
        #expect(Resume.from(position: 3500, duration: 3600, finished: false) == nil)
        #expect(Resume.from(position: 600, duration: 3600, finished: true) == nil)
    }

    @Test func shareStaysBetweenNothingAndAll() {
        #expect(Resume.share(position: 900, duration: 3600) == 0.25)
        #expect(Resume.share(position: 4000, duration: 3600) == 1)
        #expect(Resume.share(position: -1, duration: 3600) == 0)
        #expect(Resume.share(position: 10, duration: 0) == 0)
    }

    @Test func clockShowsHoursOnlyWhenNeeded() {
        #expect(clockText(0) == "0:00")
        #expect(clockText(245.9) == "4:05")
        #expect(clockText(3723) == "1:02:03")
        #expect(clockText(-5) == "0:00")
        #expect(clockText(.nan) == "0:00")
    }
}

struct AirPlayTests {
    @Test func whatAppleSPlayerTakesIsToldByTheFileEnding() {
        #expect(AirPlay.guess("https://media.example/film.mp4") == .yes)
        #expect(AirPlay.guess("https://media.example/live/index.m3u8?token=1") == .yes)
        #expect(AirPlay.guess("http://plex.example:32400/library/parts/7/file.MKV?X-Plex-Token=t") == .no)
        #expect(AirPlay.guess("http://tv.example/live/u/p/101.ts") == .no)
        #expect(AirPlay.guess("rtsp://10.0.0.5/live") == .no)
        #expect(AirPlay.guess("not an address") == .no)
        // A server's stream address names no file: Apple's player is asked.
        #expect(AirPlay.guess("http://jf.example/Videos/m1/stream?static=true&api_key=t") == .ask)
    }

    @Test func sendingIsDecidedWithoutAskingWhenTheEndingSays() async {
        #expect(await AirPlay.canSend("https://media.example/film.m4v"))
        #expect(await !AirPlay.canSend("https://media.example/film.webm"))
    }

    @Test func theBarSaysWhatPlaysWhereAndHowFar() {
        let film = AirPlaySession.Request(address: "https://a/f.mp4", title: "A Film", detail: "", start: nil)
        #expect(AirPlaySession.barTitle(film) == "A Film")
        var episode = film
        episode.detail = "S1 E2 · Pilot"
        #expect(AirPlaySession.barTitle(episode) == "A Film · S1 E2 · Pilot")
        #expect(AirPlaySession.barStatus(position: 0, duration: 0, device: "Living Room", failure: nil, started: false)
            == "Connecting to Living Room…")
        #expect(AirPlaySession.barStatus(position: 0, duration: 0, device: nil, failure: nil, started: false) == "Starting…")
        #expect(AirPlaySession.barStatus(position: 65, duration: 5400, device: "Living Room", failure: nil, started: true)
            == "On Living Room · 1:05 / 1:30:00")
        #expect(AirPlaySession.barStatus(position: 65, duration: 0, device: nil, failure: nil, started: true) == "1:05")
        #expect(AirPlaySession.barStatus(position: 65, duration: 5400, device: "Den", failure: "the stream stopped", started: true)
            == "AirPlay stopped: the stream stopped.")
    }

    @MainActor
    @Test func stoppingSavesThePlaceAndSaysItEnded() {
        let session = AirPlaySession(.init(address: "https://airplay.test/f.mp4", title: "F", detail: "", start: 600))
        var saved: [(Double, Double)] = []
        var ended = 0
        session.progress = { saved.append(($0, $1)) }
        session.finished = { ended += 1 }
        session.stop()
        session.stop()
        #expect(saved.count == 1, "the place is kept once")
        #expect(ended == 1, "stopping twice ends once")
        #expect(session.ended)
    }
}
