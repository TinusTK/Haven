import Foundation
import Observation

/// The Live TV page: television and radio, each with its categories down
/// the side (Favourites groups first) and the chosen category's channels,
/// and the user's Favourites groups. Sources are fetched again quietly at
/// start; Xtream Codes accounts are asked every minute whether they answer.
/// The Rust build's `livetv.rs` (the Settings part comes in S17).
@MainActor @Observable
final class LiveTVModel {
    /// Television or radio: its channels and the filters on them.
    struct Board: Equatable {
        var channels: [Channel] = []
        /// The chosen category's key; empty for every channel.
        var category = LiveTV.all
        /// Words the channels' names must hold.
        var words = ""
        /// Words the categories' names must hold.
        var categoryWords = ""
    }

    @ObservationIgnored private let store: Store
    @ObservationIgnored private let http: HTTP

    private(set) var tv = Board()
    private(set) var radio = Board()
    private(set) var favourites: [LiveTV.FavouriteGroup]
    /// 0 television, 1 radio.
    var band = 0
    /// A phone shows the categories instead of the channels when asked.
    var picking = false
    /// The channel whose Favourites groups are being chosen.
    private(set) var choosing: Channel?
    /// Whether each Xtream Codes account answers, by its source address.
    private(set) var online: [String: Bool] = [:]
    /// Plays a channel.
    @ObservationIgnored var play: (PlayRequest) -> Void = { _ in }
    @ObservationIgnored private var checker: Task<Void, Never>?

    init(store: Store, http: HTTP = .shared) {
        self.store = store
        self.http = http
        favourites = LiveTV.favourites(store)
    }

    /// Shows the stored sources' channels, then fetches each source again.
    /// Reads the Favourites again (brought in from another device).
    func reloadFavourites() {
        favourites = LiveTV.favourites(store)
    }

    func start() {
        rebuild()
        let off = (try? SwitchedOff.load(store).tvSources) ?? []
        for source in (try? store.tvSources()) ?? [] where !off.contains(source.url) {
            refresh(source.url)
        }
        checker?.cancel()
        checker = Task { [weak self] in
            while !Task.isCancelled {
                try? await Task.sleep(for: .seconds(LiveTV.checkEvery))
                await self?.checkAccounts()
            }
        }
    }

    /// Fetches one source again, quietly: an answer that cannot be read
    /// keeps what was there.
    func refresh(_ url: String) {
        let http = http
        Task {
            let result = await LiveTV.fetch(url, http: http)
            let fetched = LiveTV.fetched(url, account: XtreamAccount(sourceURL: url), adding: false, result, store: store)
            if let state = fetched.online { online[url] = state }
            if fetched.saved { rebuild() }
        }
    }

    /// Asks every switched-on Xtream Codes account whether it answers.
    func checkAccounts() async {
        for account in LiveTV.accounts(store) {
            online[account.sourceURL] = await LiveTV.check(account, http: http)
        }
    }

    /// Reads every stored playlist again and refills both boards.
    func rebuild() {
        let boards = LiveTV.boards(store)
        tv.channels = boards.tv
        radio.channels = boards.radio
        keepCategory()
    }

    /// A chosen category that is gone falls back to every channel.
    private func keepCategory() {
        if !LiveTV.categoryExists(tv.category, channels: tv.channels, favourites: favourites) { tv.category = LiveTV.all }
        if !LiveTV.categoryExists(radio.category, channels: radio.channels, favourites: favourites) { radio.category = LiveTV.all }
    }

    // MARK: What the page shows

    var isRadio: Bool { band == 1 }
    var board: Board { isRadio ? radio : tv }

    var categories: [LiveTV.CategoryRow] {
        LiveTV.categories(board.channels, radio: isRadio, favourites: favourites, words: board.categoryWords)
    }

    var channels: [LiveTV.ChannelRow] {
        LiveTV.channelRows(board.channels, category: board.category, words: board.words, favourites: favourites)
    }

    /// "120 in all", or "12 of 120".
    var countText: String {
        let shown = channels.count
        let total = board.channels.count
        return shown == total ? "\(total) in all" : "\(shown) of \(total)"
    }

    /// The chosen category's name.
    var categoryName: String {
        categories.first { $0.key == board.category }?.label ?? (isRadio ? "All stations" : "All channels")
    }

    func setWords(_ words: String) {
        if isRadio { radio.words = words } else { tv.words = words }
    }

    func setCategoryWords(_ words: String) {
        if isRadio { radio.categoryWords = words } else { tv.categoryWords = words }
    }

    func choose(_ key: String) {
        if isRadio { radio.category = key } else { tv.category = key }
        picking = false
    }

    // MARK: Playing

    /// Plays a channel by its place among all of the board's channels.
    func play(_ index: Int) {
        guard board.channels.indices.contains(index) else { return }
        let channel = board.channels[index]
        // A channel has no place to come back to.
        var request = PlayRequest(address: channel.address, title: channel.name, detail: channel.group ?? "",
                                  kind: isRadio ? "radio" : "tv", titleID: "", videoID: "", poster: channel.logo, source: "")
        request.remember = false
        play(request)
    }

    // MARK: Favourites

    /// Opens the chooser of Favourites groups for a channel.
    func chooseFavourites(_ index: Int) {
        guard board.channels.indices.contains(index) else { return }
        choosing = board.channels[index]
    }

    func closeChooser() {
        choosing = nil
    }

    /// Whether the channel being chosen for is in a group.
    func has(_ group: LiveTV.FavouriteGroup) -> Bool {
        choosing.map { group.channels.contains($0.address) } ?? false
    }

    func toggle(_ name: String) {
        guard let address = choosing?.address else { return }
        LiveTV.toggle(address, group: name, in: &favourites)
        saveFavourites()
    }

    /// Makes a new group; the channel being chosen for, if any, goes in.
    func newGroup(_ name: String) {
        LiveTV.newGroup(name, adding: choosing?.address, to: &favourites)
        saveFavourites()
    }

    func deleteGroup(_ name: String) {
        LiveTV.deleteGroup(name, from: &favourites)
        saveFavourites()
    }

    private func saveFavourites() {
        LiveTV.save(favourites, store: store)
        keepCategory()
    }
}
