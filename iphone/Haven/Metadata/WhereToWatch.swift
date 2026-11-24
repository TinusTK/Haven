import Foundation

/// The services that carry a title (subscription, rental, purchase), as
/// TMDB passes them on from JustWatch, and the country they are shown for.
enum WhereToWatch {
    /// Where the chosen country is kept (kv), as a two-letter code; missing
    /// means Automatic (the device's own country).
    static let countryKey = "country"
    /// Looked in, in this order, when the chosen country has nothing.
    static let fallbacks = ["US", "GB", "CA", "AU"]

    /// The countries offered in Settings: two-letter code and name.
    static let countries: [(code: String, name: String)] = [
        ("AR", "Argentina"), ("AU", "Australia"), ("AT", "Austria"), ("BE", "Belgium"),
        ("BR", "Brazil"), ("CA", "Canada"), ("CL", "Chile"), ("CO", "Colombia"),
        ("CZ", "Czechia"), ("DK", "Denmark"), ("FI", "Finland"), ("FR", "France"),
        ("DE", "Germany"), ("GR", "Greece"), ("HK", "Hong Kong"), ("HU", "Hungary"),
        ("IN", "India"), ("ID", "Indonesia"), ("IE", "Ireland"), ("IL", "Israel"),
        ("IT", "Italy"), ("JP", "Japan"), ("MY", "Malaysia"), ("MX", "Mexico"),
        ("NL", "Netherlands"), ("NZ", "New Zealand"), ("NO", "Norway"), ("PH", "Philippines"),
        ("PL", "Poland"), ("PT", "Portugal"), ("SG", "Singapore"), ("ZA", "South Africa"),
        ("KR", "South Korea"), ("ES", "Spain"), ("SE", "Sweden"), ("CH", "Switzerland"),
        ("TH", "Thailand"), ("TR", "Türkiye"), ("GB", "United Kingdom"), ("US", "United States"),
    ]

    static let noKeyNote =
        "Add a TMDB key in Settings to see which streaming services, rental and download shops have this."
    static let lookingNote = "Looking for services that have this…"
    static let creditNote = "Tap a service to see it on JustWatch. Availability by JustWatch."

    /// Haven's own tile for a service: the letters on it, its background
    /// and its ink. Our own designs (no company's logo), matched by name.
    static let tiles: [(key: String, letters: String, background: UInt32, ink: UInt32)] = [
        ("netflix", "N", 0x1a0709, 0xe5484d),
        ("amazon prime", "pv", 0x0b2236, 0x5ab0f0),
        ("prime video", "pv", 0x0b2236, 0x5ab0f0),
        ("amazon video", "a", 0x0b2236, 0x5ab0f0),
        ("amazon", "a", 0x0b2236, 0x5ab0f0),
        ("disney", "D+", 0x101a40, 0x8fb0ff),
        ("hbo max", "max", 0x1c1030, 0xb993ff),
        ("max", "max", 0x1c1030, 0xb993ff),
        ("apple tv", "tv", 0x1d1f24, 0xe9ebef),
        ("apple itunes", "tv", 0x1d1f24, 0xe9ebef),
        ("google play", "G", 0x0d2418, 0x5fd18f),
        ("youtube", "YT", 0x240808, 0xff5a5a),
        ("stan", "S", 0x0a1a33, 0x4a9cff),
        ("binge", "B", 0x21082a, 0xe86bf5),
        ("foxtel", "F", 0x2a1206, 0xff8a3d),
        ("kayo", "K", 0x062a20, 0x45e0b0),
        ("paramount", "P+", 0x081a3a, 0x5c8dff),
        ("hulu", "h", 0x062612, 0x3ee07a),
        ("peacock", "pk", 0x141414, 0xf3c94b),
        ("tubi", "tb", 0x2a1a05, 0xffb347),
        ("pluto", "pl", 0x1f1a05, 0xf5e04a),
        ("sbs", "SBS", 0x1e1a05, 0xf7d24a),
        ("abc iview", "iv", 0x062224, 0x46d6dc),
        ("7plus", "7+", 0x260808, 0xff6b6b),
        ("9now", "9N", 0x07173a, 0x6aa2ff),
        ("10 play", "10", 0x1a1a1a, 0xf0f0f0),
        ("bbc iplayer", "iP", 0x1f0a26, 0xe07bff),
        ("itvx", "X", 0x08232a, 0x4fd2e8),
        ("channel 4", "4", 0x14141f, 0xb8b8ff),
        ("now tv", "NOW", 0x0b2a1f, 0x3fe0a0),
        ("sky", "sky", 0x0a1630, 0x6a9cff),
        ("crunchyroll", "CR", 0x2a1405, 0xff9a3d),
        ("mubi", "M", 0x161616, 0xeeeeee),
        ("microsoft", "MS", 0x0f1a24, 0x7cc4ff),
    ]

    /// One tile on a title's page.
    struct Tile: Equatable, Sendable {
        var name: String
        /// "Subscription, Rent"…
        var ways: String
        /// The group's name on the first tile of each group; empty after.
        var group: String
        var letters: String
        /// Background and ink as 0xRRGGBB.
        var tint: UInt32
        var ink: UInt32
    }

    /// The tile for a service name: letters, background, ink.
    static func tile(_ name: String) -> (letters: String, tint: UInt32, ink: UInt32) {
        let lower = name.lowercased()
        if let found = tiles.first(where: { lower.contains($0.key) }) {
            return (found.letters, found.background, found.ink)
        }
        // Any other service: its first letter in Haven's gold.
        let first = name.first { $0.isLetter || $0.isNumber }.map { String($0).uppercased() } ?? ""
        return (first, 0x1b1d24, 0xf0c36a)
    }

    /// The four groups shown, and the ways of watching each one holds.
    static let groups: [(name: String, ways: [String])] = [
        ("Stream", ["Subscription"]),
        ("Free", ["Free", "Free with ads"]),
        ("Rent", ["Rent"]),
        ("Buy", ["Buy"]),
    ]

    /// JustWatch's search for a title in a country (JustWatch writes the
    /// United Kingdom as "uk").
    static func justWatchLink(country: String, title: String) -> String {
        let country = country.uppercased() == "GB" ? "uk" : country.lowercased()
        return "https://www.justwatch.com/\(country)/search?q=\(ResourceURL.encoded(title))"
    }

    /// A service's name as it fits under its tile: without the company in
    /// front ("Amazon Prime Video" is "Prime Video") or " with Ads".
    static func shortName(_ name: String) -> String {
        var name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        for suffix in [" with Ads", " with ads"] where name.hasSuffix(suffix) {
            name.removeLast(suffix.count)
            break
        }
        if name.hasPrefix("Amazon "), name.dropFirst("Amazon ".count).hasPrefix("Prime") {
            name.removeFirst("Amazon ".count)
        }
        return name
    }

    /// One tile per service per group, groups in order; the first tile of
    /// each group carries the group's name. Two listings of one service
    /// ("Amazon Prime Video" and "… with Ads") show once.
    static func tiles(for services: [Service]) -> [Tile] {
        var rows: [Tile] = []
        for group in groups {
            var first = true
            var shown: [(String, UInt32)] = []
            for service in services where service.ways.contains(where: group.ways.contains) {
                let (letters, tint, ink) = tile(service.name)
                if shown.contains(where: { $0 == (letters, tint) }) { continue }
                shown.append((letters, tint))
                rows.append(Tile(name: shortName(service.name), ways: service.ways.joined(separator: ", "),
                                 group: first ? group.name : "", letters: letters, tint: tint, ink: ink))
                first = false
            }
        }
        return rows
    }

    static func countryName(_ code: String) -> String {
        countries.first { $0.code == code }?.name ?? code
    }

    /// The region part of a locale such as "en-AU", "en_AU.UTF-8" or
    /// "zh-Hant-HK", if it is one of the countries offered.
    static func region(of locale: String) -> String? {
        guard let head = locale.split(whereSeparator: { $0 == "." || $0 == "@" }).first else { return nil }
        return head.split(whereSeparator: { $0 == "_" || $0 == "-" }).dropFirst()
            .map { $0.uppercased() }
            .first { part in part.count == 2 && countries.contains { $0.code == part } }
    }

    /// The country this device is set to: its region setting, then its
    /// languages; the United States if none says.
    static func deviceCountry() -> String {
        if let region = Locale.current.region?.identifier.uppercased(), countries.contains(where: { $0.code == region }) {
            return region
        }
        return Locale.preferredLanguages.lazy.compactMap(region(of:)).first ?? "US"
    }

    /// The country the user picked, if they did.
    static func chosenCountry(_ store: Store) -> String? {
        guard let code = try? store.get(countryKey, as: String.self),
              countries.contains(where: { $0.code == code }) else { return nil }
        return code
    }

    /// The country in use: the user's pick, else the device's own (looked
    /// at each time, so it follows the device until one is picked).
    static func country(_ store: Store) -> String {
        chosenCountry(store) ?? deviceCountry()
    }

    /// The list in Settings: "Automatic (the device's country)" first,
    /// then every country.
    static func settingsList() -> [String] {
        ["Automatic (\(countryName(deviceCountry())))"] + countries.map(\.name)
    }

    /// Which line of `settingsList` is chosen.
    static func settingsIndex(_ store: Store) -> Int {
        guard let code = chosenCountry(store), let at = countries.firstIndex(where: { $0.code == code }) else { return 0 }
        return at + 1
    }

    /// Line 0 is Automatic: forget the pick and follow the device.
    static func choose(_ index: Int, store: Store) throws {
        if index <= 0 {
            try store.remove(countryKey)
        } else if index - 1 < countries.count {
            try store.put(countryKey, countries[index - 1].code)
        }
    }

    /// What the title page shows under "Where to watch".
    struct Found: Equatable, Sendable {
        var tiles: [Tile]
        /// JustWatch's search for the title; empty when there is nothing.
        var link: String
        /// The country shown ("United Kingdom (nothing in Australia)"), or
        /// why there is nothing.
        var note: String
    }

    /// Looks up which services carry a title, in `chosen` first and then
    /// the fallback countries.
    static func lookUp(_ meta: MetaDetail, tmdb: TMDB, chosen: String) async -> Found {
        let series = meta.kind == "series"
        let known = meta.tmdbID.flatMap { Int64($0) } ?? TMDB.splitID(meta.id)?.number
        let imdb = [meta.imdbID, meta.id].compactMap { $0 }.first.flatMap { TMDB.isIMDbID($0) ? $0 : nil }
        let wanted = [chosen] + fallbacks.filter { $0 != chosen }
        do {
            var number = known
            if number == nil, let imdb {
                number = try await tmdb.numberOf(series: series, imdbID: imdb)
            }
            if let number, let offers = try await tmdb.offers(series: series, number: number, countries: wanted) {
                let note = offers.country == chosen
                    ? countryName(offers.country)
                    : "\(countryName(offers.country)) (nothing in \(countryName(chosen)))"
                return Found(tiles: tiles(for: offers.services),
                             link: justWatchLink(country: offers.country, title: meta.name), note: note)
            }
            return Found(tiles: [], link: "", note:
                "No service is known to have this in \(countryName(chosen)) or the other countries Haven looks in.")
        } catch {
            return Found(tiles: [], link: "", note: "Could not look this up: \(error.localizedDescription).")
        }
    }
}
