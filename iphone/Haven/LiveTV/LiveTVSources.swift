import Foundation

/// Live TV and Radio: the user's sources (M3U playlists and Xtream Codes
/// accounts), the channels in them, the categories down the side of the
/// page, and the user's own Favourites groups. What each screen shows is
/// worked out here; the screens (S15, S17) only display it.
enum LiveTV {
    /// Playlists can be long, but not endless.
    static let playlistLimit = 32 * 1024 * 1024
    /// How often Xtream Codes accounts are asked whether they still answer,
    /// and how long one may take.
    static let checkEvery: Double = 60
    static let checkWait: Double = 8

    // MARK: Favourites (kv `tv_favourites`)

    static let favouritesKey = "tv_favourites"

    /// One of the user's own groups of channels. A channel is known by the
    /// address it plays from.
    struct FavouriteGroup: Codable, Equatable, Sendable {
        var name: String
        var channels: [String]

        init(name: String, channels: [String] = []) {
            self.name = name
            self.channels = channels
        }

        init(from decoder: Decoder) throws {
            let c = try decoder.container(keyedBy: CodingKeys.self)
            name = try c.decodeIfPresent(String.self, forKey: .name) ?? ""
            channels = try c.decodeIfPresent([String].self, forKey: .channels) ?? []
        }
    }

    static func favourites(_ store: Store) -> [FavouriteGroup] {
        (try? store.get(favouritesKey, as: [FavouriteGroup].self)) ?? []
    }

    static func save(_ favourites: [FavouriteGroup], store: Store) {
        do {
            try store.put(favouritesKey, favourites)
        } catch {
            Log.note("could not save the favourites: \(error.localizedDescription)")
        }
    }

    /// Puts a channel into a group, or takes it out.
    static func toggle(_ address: String, group name: String, in favourites: inout [FavouriteGroup]) {
        guard let at = favourites.firstIndex(where: { $0.name == name }) else { return }
        if let place = favourites[at].channels.firstIndex(of: address) {
            favourites[at].channels.remove(at: place)
        } else {
            favourites[at].channels.append(address)
        }
    }

    /// Makes a new group (if there is none by that name); the channel being
    /// chosen for, if any, goes in.
    static func newGroup(_ name: String, adding address: String?, to favourites: inout [FavouriteGroup]) {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty else { return }
        if !favourites.contains(where: { $0.name == name }) { favourites.append(FavouriteGroup(name: name)) }
        if let address, let at = favourites.firstIndex(where: { $0.name == name }),
           !favourites[at].channels.contains(address) {
            favourites[at].channels.append(address)
        }
    }

    /// Deletes a group; its channels stay in the playlist.
    static func deleteGroup(_ name: String, from favourites: inout [FavouriteGroup]) {
        favourites.removeAll { $0.name == name }
    }

    // MARK: Channels

    /// Keeps the channels whose address the player may open.
    static func playable(_ channels: [Channel]) -> [Channel] {
        channels.filter { PlayableAddress.stream($0.address) != nil }
    }

    /// Every channel of the switched-on sources, television and radio apart.
    static func boards(_ store: Store) -> (tv: [Channel], radio: [Channel]) {
        let off = (try? SwitchedOff.load(store).tvSources) ?? []
        var tv: [Channel] = []
        var radio: [Channel] = []
        for source in (try? store.tvSources()) ?? [] where !off.contains(source.url) {
            for channel in playable(M3U.parse(source.playlist)) {
                if channel.radio { radio.append(channel) } else { tv.append(channel) }
            }
        }
        return (tv, radio)
    }

    /// How a category is named: every channel (""), one of the playlist's
    /// groups ("g:<name>") or a Favourites group ("f:<name>").
    static let all = ""
    static let groupMark = "g:"
    static let favouriteMark = "f:"

    struct CategoryRow: Equatable, Sendable {
        var key: String
        var label: String
        var count: Int
        var favourite: Bool
    }

    /// The list down the side: the Favourites groups first (radio lists only
    /// those holding stations), then every channel, then the playlists' own
    /// groups that match the category filter.
    static func categories(_ channels: [Channel], radio: Bool, favourites: [FavouriteGroup], words: String) -> [CategoryRow] {
        let words = words.lowercased().split(whereSeparator: \.isWhitespace)
        var rows: [CategoryRow] = []
        // Favourites always come first, whatever the filter says.
        for group in favourites {
            let count = channels.filter { group.channels.contains($0.address) }.count
            if !radio || count > 0 {
                rows.append(CategoryRow(key: favouriteMark + group.name, label: group.name, count: count, favourite: true))
            }
        }
        rows.append(CategoryRow(key: all, label: radio ? "All stations" : "All channels", count: channels.count, favourite: false))
        var counts: [String: Int] = [:]
        for group in channels.compactMap(\.group) { counts[group, default: 0] += 1 }
        for name in M3U.groups(channels) where words.allSatisfy({ name.lowercased().contains($0) }) {
            rows.append(CategoryRow(key: groupMark + name, label: name, count: counts[name] ?? 0, favourite: false))
        }
        return rows
    }

    /// Whether a chosen category still exists (else the page goes back to all).
    static func categoryExists(_ key: String, channels: [Channel], favourites: [FavouriteGroup]) -> Bool {
        if key == all { return true }
        if key.hasPrefix(favouriteMark) {
            let name = key.dropFirst(favouriteMark.count)
            return favourites.contains { $0.name == name }
        }
        guard key.hasPrefix(groupMark) else { return false }
        let name = key.dropFirst(groupMark.count)
        return channels.contains { $0.group.map { $0 == name } ?? false }
    }

    struct ChannelRow: Equatable, Sendable {
        /// Its place among all of the board's channels (not the shown ones).
        var index: Int
        var name: String
        var group: String
        var starred: Bool
    }

    /// The channels in the chosen category with every word in their name.
    static func channelRows(_ channels: [Channel], category: String, words: String, favourites: [FavouriteGroup]) -> [ChannelRow] {
        let words = words.lowercased().split(whereSeparator: \.isWhitespace)
        func inCategory(_ channel: Channel) -> Bool {
            if category == all { return true }
            if category.hasPrefix(favouriteMark) {
                let name = category.dropFirst(favouriteMark.count)
                return favourites.contains { $0.name == name && $0.channels.contains(channel.address) }
            }
            guard category.hasPrefix(groupMark) else { return false }
            return channel.group.map { $0 == category.dropFirst(groupMark.count) } ?? false
        }
        return channels.enumerated().compactMap { index, channel in
            let name = channel.name.lowercased()
            guard inCategory(channel), words.allSatisfy({ name.contains($0) }) else { return nil }
            return ChannelRow(index: index, name: channel.name, group: channel.group ?? "",
                              starred: favourites.contains { $0.channels.contains(channel.address) })
        }
    }

    // MARK: Sources (Settings › Live TV)

    /// One source as Settings lists it.
    struct SourceRow: Equatable, Sendable {
        var url: String
        var name: String
        /// Playable channels and stations in it.
        var channels: Int
        /// The playlist's address, or "Xtream Codes account of <user>" (an
        /// account's address holds its password and is never shown).
        var detail: String
        var enabled: Bool
        var isAccount: Bool
    }

    static func sourceRows(_ store: Store) -> [SourceRow] {
        let off = (try? SwitchedOff.load(store).tvSources) ?? []
        return ((try? store.tvSources()) ?? []).map { source in
            let account = XtreamAccount(sourceURL: source.url)
            return SourceRow(url: source.url, name: source.name,
                             channels: playable(M3U.parse(source.playlist)).count,
                             detail: account.map { "Xtream Codes account of \($0.username)" } ?? source.url,
                             enabled: !off.contains(source.url), isAccount: account != nil)
        }
    }

    /// The Xtream Codes accounts that are switched on.
    static func accounts(_ store: Store) -> [XtreamAccount] {
        let off = (try? SwitchedOff.load(store).tvSources) ?? []
        return ((try? store.tvSources()) ?? []).filter { !off.contains($0.url) }.compactMap { XtreamAccount(sourceURL: $0.url) }
    }

    /// A message for Settings › Live TV.
    struct Status: Error, Equatable, Sendable {
        var text: String
        var isError: Bool
        /// It belongs under the Xtream Codes fields (not the playlist's).
        var account: Bool
    }

    static let fetchingPlaylist = "Fetching the channel list…"
    static let signingIn = "Signing in and fetching the channel list…"
    static let accountNeeds = "Could not add the account: give the server's web address, your user name and your password."

    /// The playlist address to add, or why it cannot be.
    static func playlistAddress(_ typed: String) -> Result<String, Status> {
        do {
            return .success(try HTTP.webURL(typed).absoluteString)
        } catch {
            return .failure(Status(text: "Could not add the source: \(error.localizedDescription).", isError: true, account: false))
        }
    }

    /// Fetches a source's channels as playlist text: a playlist's own text,
    /// or an account's live channels turned into one. Errors never hold the
    /// address (an account's holds its password).
    static func fetch(_ url: String, http: HTTP = .shared) async -> Result<String, Problem> {
        if let account = XtreamAccount(sourceURL: url) { return await fetch(account, http: http) }
        do {
            return .success(String(decoding: try await http.getBytes(url, limit: playlistLimit), as: UTF8.self))
        } catch {
            return .failure(Problem(error.localizedDescription))
        }
    }

    /// Signs in to an Xtream Codes account and fetches its live channels.
    static func fetch(_ account: XtreamAccount, http: HTTP = .shared) async -> Result<String, Problem> {
        func text(_ data: Data) -> String { String(decoding: data, as: UTF8.self) }
        do {
            let info = try Xtream.signIn(text(try await http.getBytes(account.api(), limit: playlistLimit)))
            // The groups' names are worth having but not worth failing for.
            let categories = (try? await http.getBytes(account.api("get_live_categories"), limit: playlistLimit)).map(text) ?? ""
            let streams = text(try await http.getBytes(account.api("get_live_streams"), limit: playlistLimit))
            return .success(M3U.write(try Xtream.channels(account, info: info, categories: categories, streams: streams)))
        } catch {
            return .failure(Problem(error.localizedDescription))
        }
    }

    /// What a fetch came to.
    struct Fetched: Equatable, Sendable {
        /// What to say (only when the user just added the source).
        var status: Status?
        /// An account's state now: true online, false offline, nil unchanged.
        var online: Bool?
        /// The source was saved: the pages are to be rebuilt.
        var saved: Bool
    }

    /// Keeps a source's channels once they arrive. `adding` is true when the
    /// user just asked for it (failures are then said); false for the quiet
    /// refresh at start. An empty or unreadable list keeps what was there;
    /// a source removed while its refresh was on the way stays removed.
    static func fetched(_ url: String, account: XtreamAccount?, adding: Bool, _ result: Result<String, Problem>,
                        store: Store) -> Fetched {
        let isAccount = account != nil
        func say(_ text: String, error: Bool) -> Status { Status(text: text, isError: error, account: isAccount) }
        let known = ((try? store.tvSources()) ?? []).contains { $0.url == url }
        var outcome = Fetched(status: nil, online: nil, saved: false)
        // An account that answered is online; one that did not is offline.
        if isAccount, known || (try? result.get()) != nil {
            outcome.online = (try? result.get()) != nil
        }
        let playlist: String
        switch result {
        case let .success(text):
            playlist = text
        case let .failure(problem):
            if adding {
                outcome.status = say("Could not add the \(isAccount ? "account" : "source"): \(problem.words).", error: true)
            } else {
                Log.note("could not refresh \(M3U.sourceName(Log.plainAddress(url))): \(problem.words)")
            }
            return outcome
        }
        let channels = playable(M3U.parse(playlist))
        if channels.isEmpty {
            if adding {
                outcome.status = say(isAccount ? "The account has no live channels." : "No channels were found at that address.", error: true)
            }
            return outcome
        }
        if !adding && !known { return outcome }
        do {
            try store.saveTvSource(TvSource(url: url, name: account?.label ?? M3U.sourceName(url), playlist: playlist))
        } catch {
            outcome.status = say("Could not save the source: \(error.localizedDescription)", error: true)
            return outcome
        }
        outcome.saved = true
        if adding {
            let stations = channels.filter(\.radio).count
            func count(_ n: Int, _ one: String, _ many: String) -> String { n == 1 ? "1 \(one)" : "\(n) \(many)" }
            var found = [count(channels.count - stations, "channel", "channels")]
            if stations > 0 { found.append(count(stations, "radio station", "radio stations")) }
            outcome.status = say("Added \(found.joined(separator: " and ")). They are on the Live TV tab.", error: false)
        }
        return outcome
    }

    /// Switches a source on or off. Off, it stays in the list but its
    /// channels are not shown and it is not fetched.
    static func setEnabled(_ url: String, on: Bool, store: Store) -> Status? {
        do {
            var off = try SwitchedOff.load(store)
            SwitchedOff.switch(&off.tvSources, url, on: on)
            try off.save(store)
            return nil
        } catch {
            return Status(text: "Could not save that: \(error.localizedDescription)", isError: true, account: true)
        }
    }

    static func remove(_ url: String, store: Store) -> Status {
        do {
            try store.removeTvSource(url: url)
        } catch {
            return Status(text: "Could not remove the source: \(error.localizedDescription)", isError: true, account: true)
        }
        _ = setEnabled(url, on: true, store: store)
        return Status(text: "Source removed.", isError: false, account: true)
    }

    /// Whether an account's server answers and still accepts the sign-in.
    static func check(_ account: XtreamAccount, http: HTTP = .shared) async -> Bool {
        let answer = await withTimeout(checkWait) { try? await http.getBytes(account.api(), limit: playlistLimit) }
        guard let data = answer ?? nil else { return false }
        return (try? Xtream.signIn(String(decoding: data, as: UTF8.self))) != nil
    }
}
