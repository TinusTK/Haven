import Foundation

/// API keys for the services Haven talks to. Read from `secrets.env` in
/// Haven's data folder (`KEY=value` lines, `#` comments), or built in.
/// Never printed: only which ones are set.
struct Secrets: Equatable, Sendable, CustomStringConvertible, CustomDebugStringConvertible {
    var tmdbAPIKey: String?
    var traktClientID: String?
    var traktClientSecret: String?
    var simklClientID: String?
    var omdbAPIKey: String?

    /// The keys built into this copy (scripts/build-keys.sh). Never a TMDB
    /// key: people add their own.
    static let builtIn = Secrets(
        tmdbAPIKey: nil,
        traktClientID: BuildKeys.traktClientID,
        traktClientSecret: BuildKeys.traktClientSecret,
        simklClientID: BuildKeys.simklClientID,
        omdbAPIKey: BuildKeys.omdbAPIKey)

    /// A missing file is not an error: it gives empty secrets.
    static func fromFile(_ url: URL) throws -> Secrets {
        do {
            return parse(try String(contentsOf: url, encoding: .utf8))
        } catch let error as CocoaError where error.code == .fileReadNoSuchFile {
            return Secrets()
        } catch {
            throw HavenError.io(path: url.path, message: error.localizedDescription)
        }
    }

    static func parse(_ text: String) -> Secrets {
        var map: [String: String] = [:]
        for raw in text.split(omittingEmptySubsequences: false, whereSeparator: \.isNewline) {
            var line = raw.trimmingCharacters(in: .whitespaces)
            if line.isEmpty || line.hasPrefix("#") { continue }
            if line.hasPrefix("export ") { line.removeFirst("export ".count) }
            guard let eq = line.firstIndex(of: "=") else { continue }
            let key = line[..<eq].trimmingCharacters(in: .whitespaces)
            let value = line[line.index(after: eq)...].trimmingCharacters(in: .whitespaces)
            map[key] = unquote(value)
        }
        func take(_ key: String) -> String? {
            guard let value = map[key], !value.isEmpty else { return nil }
            return value
        }
        return Secrets(
            tmdbAPIKey: take("TMDB_API_KEY"),
            traktClientID: take("TRAKT_CLIENT_ID"),
            traktClientSecret: take("TRAKT_CLIENT_SECRET"),
            simklClientID: take("SIMKL_CLIENT_ID"),
            omdbAPIKey: take("OMDB_API_KEY"))
    }

    private static func unquote(_ value: String) -> String {
        for quote in ["\"", "'"] where value.count >= 2 && value.hasPrefix(quote) && value.hasSuffix(quote) {
            return String(value.dropFirst().dropLast())
        }
        return value
    }

    var description: String {
        func set(_ v: String?) -> String { v == nil ? "missing" : "set" }
        return "Secrets(tmdb: \(set(tmdbAPIKey)), traktClientID: \(set(traktClientID)), "
            + "traktClientSecret: \(set(traktClientSecret)), simklClientID: \(set(simklClientID)), "
            + "omdb: \(set(omdbAPIKey)))"
    }
    var debugDescription: String { description }
}
