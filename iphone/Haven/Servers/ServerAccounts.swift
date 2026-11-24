import Foundation

/// The media servers the user signed in to: reading them, signing in
/// (Settings › Media servers), switching on and off, removing, whether each
/// answers, their shelves on Home, and telling a server what was watched.
enum ServerAccounts {
    /// How long a Plex code stays worth waiting for, and how often to ask.
    static let plexWait: Double = 5 * 60
    static let plexAskEvery: Double = 2
    /// How often the servers are asked whether they still answer.
    static let checkEvery: Double = 60
    /// A server that takes longer than this to answer counts as offline.
    static let checkWait: Double = 8

    /// Whether a server answers, as the list in Settings shows it.
    enum State: Int, Sendable {
        case unknown = 0
        case online = 1
        case offline = 2
    }

    /// The servers on record, and those switched on (a switched-off server
    /// is listed but never asked).
    struct Loaded: Equatable, Sendable {
        var all: [Server] = []
        var enabled: [Server] = []
    }

    static func load(_ store: Store) -> Loaded {
        let all: [Server]
        do {
            all = try store.servers().compactMap(Server.init)
        } catch {
            Log.note("could not read the media servers: \(error.localizedDescription)")
            all = []
        }
        let off = (try? SwitchedOff.load(store).servers) ?? []
        return Loaded(all: all, enabled: all.filter { !off.contains($0.id) })
    }

    /// The server a `srv:…` id belongs to and the item's id on it.
    static func serverItem(_ id: String, in servers: [Server]) -> (server: Server, item: String)? {
        guard let (number, item) = Servers.splitID(id), let server = servers.first(where: { $0.id == number }) else { return nil }
        return (server, item)
    }

    /// Whether a server answers within `checkWait`.
    static func check(_ server: Server, client: ServerClient) async -> State {
        let online = await withTimeout(checkWait) { await client.online(server) } ?? false
        return online ? .online : .offline
    }

    // MARK: Signing in

    /// A message for Settings › Media servers.
    struct Status: Equatable, Sendable {
        var text: String
        var isError: Bool
    }

    /// Signs in to a Jellyfin or Emby server. The password is passed on
    /// once and never stored.
    static func signIn(_ kind: ServerKind, address: String, user: String, password: String,
                       client: ServerClient) async -> Result<[Server], ServerError> {
        do {
            return .success([try await client.signIn(kind, address: address, user: user, password: password)])
        } catch let error as ServerError {
            return .failure(error)
        } catch {
            return .failure(.request(error.localizedDescription))
        }
    }

    /// What signing in to Plex has come to.
    enum PlexOutcome: Equatable, Sendable {
        case servers([Server])
        case failed(String)
    }

    /// Signs in to Plex: asks for a code, hands it to `show` (which shows it
    /// and opens the Plex page in the browser), then waits for the user to
    /// approve it there. nil when cancelled (the task was cancelled).
    static func plexSignIn(client: ServerClient, show: @Sendable (Plex.Pin) async -> Void) async -> PlexOutcome? {
        let pin: Plex.Pin
        do {
            pin = try await Plex.pin(client)
        } catch {
            return .failed(error.localizedDescription)
        }
        await show(pin)
        let started = Date()
        while true {
            do {
                try await Task.sleep(for: .seconds(plexAskEvery))
            } catch {
                return nil // cancelled
            }
            if Date().timeIntervalSince(started) > plexWait {
                return .failed("The Plex code was not approved in time.")
            }
            do {
                guard let token = try await Plex.token(client, pin) else { continue }
                if Task.isCancelled { return nil }
                let servers = try await Plex.servers(client, accountToken: token)
                return servers.isEmpty ? .failed("This Plex account has no servers.") : .servers(servers)
            } catch {
                if Task.isCancelled { return nil }
                return .failed(error.localizedDescription)
            }
        }
    }

    static let askingPlex = "Asking Plex for a code…"
    static let approvePlex = "Approve Haven on the Plex page that just opened in your web browser."
    static let signingIn = "Signing in…"

    /// Keeps the servers a sign-in gave. Signing in again to one already
    /// there (same kind, address and user) keeps its number.
    static func added(_ servers: [Server], store: Store) -> Status {
        var names: [String] = []
        for server in servers {
            do {
                try store.saveServer(server.stored)
                names.append(server.name)
            } catch {
                return Status(text: "Could not save the server: \(error.localizedDescription)", isError: true)
            }
        }
        return Status(text: "Signed in to \(names.joined(separator: ", ")).", isError: false)
    }

    static func failed(_ why: String) -> Status {
        Status(text: "Could not sign in: \(why).", isError: true)
    }

    /// Switches a server on or off. Off, Haven stays signed in to it but
    /// does not ask it for anything.
    static func setEnabled(_ id: Int64, on: Bool, store: Store) -> Status {
        do {
            var off = try SwitchedOff.load(store)
            SwitchedOff.switch(&off.servers, id, on: on)
            try off.save(store)
        } catch {
            return Status(text: "Could not save that: \(error.localizedDescription)", isError: true)
        }
        return Status(text: on ? "Server switched on." : "Server switched off.", isError: false)
    }

    static func remove(_ id: Int64, store: Store) -> Status {
        do {
            try store.removeServer(id: id)
        } catch {
            return Status(text: "Could not remove the server: \(error.localizedDescription)", isError: true)
        }
        return Status(text: "Server removed. Haven has forgotten its sign-in.", isError: false)
    }

    // MARK: Home

    /// One shelf on Home from a server's library.
    struct ShelfPlan: Equatable, Sendable {
        var title: String
        /// The server's name, shown with the shelf.
        var source: String
        var server: Server
        var library: String
        /// What is new, or the titles by name.
        var latest: Bool
    }

    /// The shelves for servers' libraries: a collection is one list, one
    /// shelf; a library also gets a shelf of what is new.
    static func libraryPlans(_ listed: [(server: Server, libraries: [MediaLibrary])]) -> [ShelfPlan] {
        var plans: [ShelfPlan] = []
        for (server, libraries) in listed {
            for library in libraries {
                for latest in library.collection ? [false] : [true, false] {
                    plans.append(ShelfPlan(title: latest ? "New in \(library.name)" : library.name, source: server.name,
                                           server: server, library: library.id, latest: latest))
                }
            }
        }
        return plans
    }

    /// The name a server's list of libraries is kept under.
    static func librariesName(_ server: Server) -> String { "libraries|\(server.id)|\(server.address)" }

    // MARK: Watching

    /// Tells the server a film or episode came from how far it was watched
    /// (nothing for live television, which has no length).
    static func report(_ server: Server, item: String, position: Double, duration: Double, finished: Bool,
                       client: ServerClient) async {
        guard duration > 0 else { return }
        do {
            try await client.report(server, item: item, position: position, duration: duration, finished: finished)
        } catch {
            Log.note("could not tell \(server.name) what was watched: \(error.localizedDescription)")
        }
    }
}
