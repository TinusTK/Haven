import Foundation

/// One audio or subtitle track of what is playing.
struct Track: Equatable, Sendable, Identifiable {
    enum Kind: Equatable, Sendable { case audio, subtitle }

    /// mpv's number for the track within its kind; what `aid`/`sid` take.
    var id: Int
    var kind: Kind
    /// "English · Commentary · AC3 5.1"
    var label: String
    var selected: Bool
}

/// The player's rules, ported from the Rust build (`haven-player/src/
/// tracks.rs`, `haven-app/src/player.rs`, `external.rs`).
enum PlayerRules {
    /// How often the place is saved while playing.
    static let saveEvery: TimeInterval = 5
    /// After a seek the bar is left alone this long, so it does not jump
    /// back while mpv catches up.
    static let seekHold: TimeInterval = 0.7
    /// The bar of controls at the bottom, plus a little air: subtitles are
    /// lifted clear of it.
    static let controlsHeight: Double = 112
    /// mpv's own subtitle margin, used while the controls are hidden.
    static let subtitleMargin = 22
    /// How long the next episode waits before it starts by itself.
    static let nextAfter: TimeInterval = 10
    /// The controls hide after this long untouched.
    static let hideAfter: TimeInterval = 3
    static let speeds: [Double] = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2]
    static let fits = ["Whole picture", "Fill the screen (edges cut off)", "Stretch to the screen"]

    // MARK: Tracks

    /// Reads mpv's `track-list`. Video tracks and entries that cannot be
    /// read are left out; text that is not a track list gives no tracks.
    static func tracks(_ json: String) -> [Track] {
        guard let data = json.data(using: .utf8),
              let entries = try? JSONSerialization.jsonObject(with: data) as? [Any] else { return [] }
        return entries.compactMap { entry -> Track? in
            guard let raw = entry as? [String: Any], let id = (raw["id"] as? NSNumber)?.intValue,
                  let type = raw["type"] as? String else { return nil }
            let kind: Track.Kind
            switch type {
            case "audio": kind = .audio
            case "sub": kind = .subtitle
            default: return nil
            }
            return Track(id: id, kind: kind, label: label(raw, id: id, kind: kind), selected: raw["selected"] as? Bool ?? false)
        }
    }

    private static func label(_ raw: [String: Any], id: Int, kind: Track.Kind) -> String {
        func text(_ key: String) -> String? {
            (raw[key] as? String).map { $0.trimmingCharacters(in: .whitespaces) }.flatMap { $0.isEmpty ? nil : $0 }
        }
        let language = text("lang").flatMap { $0 == "und" ? nil : languageName($0) }
        var parts: [String] = language.map { [$0] } ?? []
        // Many files repeat the language as the title.
        if let title = text("title"), language.map({ $0.caseInsensitiveCompare(title) != .orderedSame }) ?? true {
            parts.append(title)
        }
        if parts.isEmpty { parts.append("Track \(id)") }
        if kind == .audio {
            let codec = text("codec")?.uppercased()
            let layout = (raw["demux-channel-count"] as? NSNumber).flatMap { channelLayout($0.intValue) }
            let sound = [codec, layout].compactMap { $0 }
            if !sound.isEmpty { parts.append(sound.joined(separator: " ")) }
        }
        if raw["forced"] as? Bool == true { parts.append("Forced") }
        return parts.joined(separator: " · ")
    }

    private static func channelLayout(_ channels: Int) -> String? {
        switch channels {
        case 1: "Mono"
        case 2: "Stereo"
        case 6: "5.1"
        case 8: "7.1"
        case let n where n > 0: "\(n) ch"
        default: nil
        }
    }

    private static let languages: [String: String] = {
        let names: [(codes: [String], name: String)] = [
            (["en", "eng"], "English"), (["af", "afr"], "Afrikaans"), (["ar", "ara"], "Arabic"),
            (["bg", "bul"], "Bulgarian"), (["ca", "cat"], "Catalan"), (["cs", "ces", "cze"], "Czech"),
            (["da", "dan"], "Danish"), (["de", "deu", "ger"], "German"), (["el", "ell", "gre"], "Greek"),
            (["es", "spa"], "Spanish"), (["et", "est"], "Estonian"), (["fa", "fas", "per"], "Persian"),
            (["fi", "fin"], "Finnish"), (["fr", "fra", "fre"], "French"), (["he", "heb"], "Hebrew"),
            (["hi", "hin"], "Hindi"), (["hr", "hrv"], "Croatian"), (["hu", "hun"], "Hungarian"),
            (["id", "ind"], "Indonesian"), (["is", "isl", "ice"], "Icelandic"), (["it", "ita"], "Italian"),
            (["ja", "jpn"], "Japanese"), (["ko", "kor"], "Korean"), (["lt", "lit"], "Lithuanian"),
            (["lv", "lav"], "Latvian"), (["ms", "msa", "may"], "Malay"), (["nb", "nob", "no", "nor"], "Norwegian"),
            (["nl", "nld", "dut"], "Dutch"), (["pl", "pol"], "Polish"), (["pt", "por"], "Portuguese"),
            (["ro", "ron", "rum"], "Romanian"), (["ru", "rus"], "Russian"), (["sk", "slk", "slo"], "Slovak"),
            (["sl", "slv"], "Slovenian"), (["sr", "srp"], "Serbian"), (["sv", "swe"], "Swedish"),
            (["ta", "tam"], "Tamil"), (["te", "tel"], "Telugu"), (["th", "tha"], "Thai"),
            (["tr", "tur"], "Turkish"), (["uk", "ukr"], "Ukrainian"), (["vi", "vie"], "Vietnamese"),
            (["zh", "zho", "chi"], "Chinese"),
        ]
        var all: [String: String] = [:]
        for entry in names { for code in entry.codes { all[code] = entry.name } }
        return all
    }()

    /// The English name of a language code as files carry them ("en",
    /// "eng", "pt-BR"); codes not known are shown as they are, in capitals.
    static func languageName(_ code: String) -> String {
        let base = code.split(whereSeparator: { $0 == "-" || $0 == "_" }).first.map(String.init)?.lowercased() ?? code.lowercased()
        return languages[base] ?? code.uppercased()
    }

    // MARK: What the controls say

    /// "4.2 MB/s  ·  42% loaded": how fast a stream arrives and how much of
    /// the whole has arrived. Empty when mpv does not say.
    static func arrival(_ bytesASecond: Double?, loaded: Double?) -> String {
        var parts: [String] = []
        // Nothing arriving (all of it is here already) is not worth saying.
        if let speed = bytesASecond, speed >= 1_000 {
            parts.append(speed >= 1_000_000 ? String(format: "%.1f MB/s", speed / 1_000_000) : String(format: "%.0f KB/s", speed / 1_000))
        }
        if let loaded {
            parts.append(loaded >= 99.5 ? "100% loaded" : "\(Int(loaded.rounded(.down)))% loaded")
        }
        return parts.joined(separator: "  ·  ")
    }

    /// How much of a film has arrived, 0 to 100; nil for a live stream.
    static func shareLoaded(_ status: MPV.Status) -> Double? {
        guard status.duration > 0 else { return nil }
        return min(max((status.position + (status.ahead ?? 0)) / status.duration * 100, 0), 100)
    }

    /// "Ends at 21:40", by the clock, at the speed playing.
    static func endsAt(left: Double, speed: Double, now: Date = Date(), zone: TimeZone = .current) -> String {
        let ends = now.addingTimeInterval(left / max(speed, 0.25))
        let format = DateFormatter()
        format.dateFormat = "HH:mm"
        format.timeZone = zone
        format.locale = Locale(identifier: "en_GB")
        return "Ends at \(format.string(from: ends))"
    }

    /// What is known of the picture and sound, a fact a line.
    static func info(_ details: MPV.Details, source: String) -> String {
        var lines: [String] = []
        if let width = details.width, let height = details.height {
            let rate = details.framesPerSecond.map { ", \(formatted(($0 * 1000).rounded() / 1000)) frames a second" } ?? ""
            lines.append("Picture: \(width) × \(height)\(rate)")
        }
        if let video = details.video {
            lines.append("Video: \(video)\(details.decoder.map { " (decoded by \($0))" } ?? "")")
        }
        if let audio = details.audio {
            lines.append("Sound: \(audio)\(details.channels.map { ", \($0)" } ?? "")")
        }
        if let container = details.container { lines.append("File: \(container)") }
        if !source.isEmpty { lines.append("From: \(source)") }
        return lines.joined(separator: "\n")
    }

    /// A number without a needless ".0".
    static func formatted(_ number: Double) -> String {
        number == number.rounded() ? String(Int(number)) : String(number)
    }

    /// The speed as its menu says it: "Normal", "1.5×".
    static func speedLabel(_ speed: Double) -> String {
        speed == 1 ? "Normal" : "\(formatted(speed))×"
    }

    // MARK: Settings kept between runs

    /// What the player keeps between runs (the Rust build's key and names).
    struct Preferences: Codable, Equatable, Sendable {
        static let key = "player"

        /// 0 silent to 1 as recorded; up to 2 (200%) when `loud` is on.
        var volume = 1.0
        var muted = false
        var subtitleScale = 1.0
        /// Dolby and DTS sound sent on as it is, to a receiver.
        var passthrough = false
        /// Settings › General: the volume may go up to 200%.
        var loud = false

        /// The highest volume allowed: 1 (100%), or 2 (200%) when `loud`
        /// is on. Apple TV has no volume slider, only the Sound boost in
        /// the player's menu, so there it is always 2.
        var top: Double {
            #if os(tvOS)
            2
            #else
            loud ? 2 : 1
            #endif
        }

        /// Apple TV's Sound boost choices.
        static let boosts = [1.0, 1.25, 1.5, 2.0]

        enum CodingKeys: String, CodingKey {
            case volume, muted, passthrough, loud
            case subtitleScale = "subtitle_scale"
        }

        init() {}

        init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            volume = try values.decodeIfPresent(Double.self, forKey: .volume) ?? 1
            muted = try values.decodeIfPresent(Bool.self, forKey: .muted) ?? false
            subtitleScale = try values.decodeIfPresent(Double.self, forKey: .subtitleScale) ?? 1
            passthrough = try values.decodeIfPresent(Bool.self, forKey: .passthrough) ?? false
            loud = try values.decodeIfPresent(Bool.self, forKey: .loud) ?? false
        }
    }

    // MARK: Other player apps

    /// What plays videos (the Rust build's `external_player`): 0 Haven's own
    /// player; on an iPhone 1 VLC, 2 Infuse, 3 Outplayer.
    struct OtherPlayer: Codable, Equatable, Sendable {
        static let key = "external_player"
        static let names = ["Haven's own player", "VLC", "Infuse", "Outplayer"]

        var kind = 0
        /// A program to run (computers only; kept as it is).
        var program = ""

        init(kind: Int = 0, program: String = "") {
            self.kind = kind
            self.program = program
        }

        init(from decoder: Decoder) throws {
            let values = try decoder.container(keyedBy: CodingKeys.self)
            kind = try values.decodeIfPresent(Int.self, forKey: .kind) ?? 0
            program = try values.decodeIfPresent(String.self, forKey: .program) ?? ""
        }

        static func load(_ store: Store) -> OtherPlayer {
            ((try? store.get(key, as: OtherPlayer.self)) ?? nil) ?? OtherPlayer()
        }
    }

    /// The address that hands a video to a player app. The video's own
    /// address travels inside it, spelt out.
    static func otherPlayerLink(kind: Int, address: String) -> String {
        let unreserved = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~")
        let inside = address.addingPercentEncoding(withAllowedCharacters: unreserved) ?? address
        switch kind {
        case 2: return "infuse://x-callback-url/play?url=\(inside)"
        case 3: return "outplayer://\(address)"
        default: return "vlc-x-callback://x-callback-url/stream?url=\(inside)"
        }
    }

    /// Whether a video goes to another app: one is chosen, and the video is
    /// not a file inside Haven (other apps cannot open those).
    static func goesElsewhere(_ choice: OtherPlayer, address: String) -> Bool {
        choice.kind != 0 && address.contains("://") && !address.hasPrefix("file://")
    }
}
