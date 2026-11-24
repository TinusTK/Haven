import Foundation
import Network

/// App Store builds reach media servers only on the user's own network:
/// the home network or Tailscale. A server at a public address is refused.
///
/// The check runs before every request to a server (not only when it is
/// added), so a name that later points somewhere public is refused too.
/// plex.tv itself (signing in, the list of the account's servers) is not a
/// media server and is not checked.
enum OwnNetwork {
    /// Whether this build keeps to the user's own network.
    static let only = Build.ownNetworkOnly

    /// An address on a home network, this device, or Tailscale.
    static func isOwn(_ text: String) -> Bool {
        if let v4 = IPv4Address(text) { return isOwn(v4: [UInt8](v4.rawValue)) }
        if let v6 = IPv6Address(text) { return isOwn(v6: [UInt8](v6.rawValue)) }
        return false
    }

    static func isOwn(v4 b: [UInt8]) -> Bool {
        guard b.count == 4 else { return false }
        return b[0] == 10                                   // 10.0.0.0/8
            || (b[0] == 172 && (16 ... 31).contains(b[1]))  // 172.16.0.0/12
            || (b[0] == 192 && b[1] == 168)                 // 192.168.0.0/16
            || b[0] == 127                                  // loopback
            || (b[0] == 169 && b[1] == 254)                 // link-local
            || (b[0] == 100 && (64 ..< 128).contains(b[1])) // Tailscale, 100.64.0.0/10
    }

    static func isOwn(v6 b: [UInt8]) -> Bool {
        guard b.count == 16 else { return false }
        // An IPv4 address written as IPv6 (::ffff:a.b.c.d).
        if b[0 ..< 10].allSatisfy({ $0 == 0 }) && b[10] == 0xff && b[11] == 0xff {
            return isOwn(v4: Array(b[12 ..< 16]))
        }
        let loopback = b[0 ..< 15].allSatisfy { $0 == 0 } && b[15] == 1
        return loopback
            || b[0] & 0xfe == 0xfc                     // unique local, fc00::/7 (Tailscale too)
            || (b[0] == 0xfe && b[1] & 0xc0 == 0x80)   // link-local, fe80::/10
    }

    /// Names that can only mean the local network, whatever DNS says.
    static func isLocalName(_ host: String) -> Bool {
        let host = trimmed(host)
        return host == "localhost" || host.hasSuffix(".localhost") || host.hasSuffix(".local")
    }

    /// plex.tv's own addresses: sign-in and the account's server list.
    static func isPlexTV(_ host: String) -> Bool {
        let host = trimmed(host)
        return host == "plex.tv" || host.hasSuffix(".plex.tv")
    }

    private static func trimmed(_ host: String) -> String {
        var host = host.lowercased()
        while host.hasSuffix(".") { host.removeLast() }
        return host
    }

    /// Whether a request to `url` may go ahead in this build. Names are
    /// looked up and every address they give must be on the own network.
    static func allowed(_ url: URL, enforce: Bool = only) async -> Bool {
        guard enforce else { return true }
        guard var host = url.host(percentEncoded: false), !host.isEmpty else { return false }
        if host.hasPrefix("["), host.hasSuffix("]") { host = String(host.dropFirst().dropLast()) }
        if IPv4Address(host) != nil || IPv6Address(host) != nil { return isOwn(host) }
        if isPlexTV(host) || isLocalName(host) { return true }
        guard let found = await lookUp(host) else {
            // Not found: let the request fail in the usual way.
            return true
        }
        return !found.isEmpty && found.allSatisfy(isOwn)
    }

    /// The addresses a name stands for; nil if the lookup failed.
    static func lookUp(_ host: String) async -> [String]? {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                var hints = addrinfo()
                hints.ai_socktype = SOCK_STREAM
                var list: UnsafeMutablePointer<addrinfo>?
                guard getaddrinfo(host, nil, &hints, &list) == 0, let first = list else {
                    continuation.resume(returning: nil)
                    return
                }
                defer { freeaddrinfo(list) }
                var found: [String] = []
                var node: UnsafeMutablePointer<addrinfo>? = first
                while let current = node {
                    var text = [CChar](repeating: 0, count: Int(NI_MAXHOST))
                    if getnameinfo(current.pointee.ai_addr, current.pointee.ai_addrlen, &text, socklen_t(text.count),
                                   nil, 0, NI_NUMERICHOST) == 0 {
                        // Drop a scope ("fe80::1%en0").
                        let bytes = text.prefix { $0 != 0 }.map { UInt8(bitPattern: $0) }
                        let address = String(decoding: bytes, as: UTF8.self).split(separator: "%").first.map(String.init) ?? ""
                        if !address.isEmpty, !found.contains(address) { found.append(address) }
                    }
                    node = current.pointee.ai_next
                }
                continuation.resume(returning: found)
            }
        }
    }
}
