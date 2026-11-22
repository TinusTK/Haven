//! Store builds reach media servers only on the user's own network: the
//! home network or Tailscale. A server at a public address is refused.
//!
//! The check runs before every request to a server (not only when it is
//! added), so a name that later points somewhere public is refused too.
//! plex.tv itself (signing in, the list of the account's servers) is not
//! a media server and is not checked.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Whether this build keeps to the user's own network.
pub const ONLY_OWN_NETWORK: bool = cfg!(feature = "own-network");

/// An address on a home network, this device, or Tailscale.
pub fn own_network_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => own_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return own_v4(v4);
            }
            let first = v6.segments()[0];
            v6 == Ipv6Addr::LOCALHOST
                // Unique local addresses, fc00::/7 (Tailscale's fd7a:115c:a1e0::/48 too).
                || first & 0xfe00 == 0xfc00
                // Link-local, fe80::/10.
                || first & 0xffc0 == 0xfe80
        }
    }
}

fn own_v4(v4: Ipv4Addr) -> bool {
    let [a, b, ..] = v4.octets();
    v4.is_private()
        || v4.is_loopback()
        || v4.is_link_local()
        // Tailscale hands out addresses from 100.64.0.0/10.
        || (a == 100 && (64..128).contains(&b))
}

/// Names that can only mean the local network, whatever DNS says.
fn local_name(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local")
}

/// plex.tv's own addresses: sign-in and the account's server list.
pub(crate) fn plex_tv(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "plex.tv" || host.ends_with(".plex.tv")
}

/// Whether a request to `url` may go ahead in this build. Names are looked
/// up and every address they give must be on the user's own network.
pub(crate) async fn allowed(url: &reqwest::Url) -> bool {
    if !ONLY_OWN_NETWORK {
        return true;
    }
    match url.host() {
        Some(url::Host::Ipv4(ip)) => own_network_ip(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => own_network_ip(IpAddr::V6(ip)),
        Some(url::Host::Domain(name)) => {
            if plex_tv(name) || local_name(name) {
                return true;
            }
            let port = url.port_or_known_default().unwrap_or(80);
            match tokio::net::lookup_host((name, port)).await {
                Ok(found) => {
                    let found: Vec<_> = found.map(|address| address.ip()).collect();
                    !found.is_empty() && found.into_iter().all(own_network_ip)
                }
                // Not found: let the request fail in the usual way.
                Err(_) => true,
            }
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn home_network_and_tailscale_are_own() {
        for own in [
            "192.168.1.9", "10.0.0.4", "172.16.0.1", "172.31.255.254", "127.0.0.1",
            "169.254.3.3", "100.64.0.1", "100.101.102.103", "100.127.255.255",
            "::1", "fd7a:115c:a1e0::1", "fc00::5", "fe80::1", "::ffff:192.168.0.2",
        ] {
            assert!(own_network_ip(ip(own)), "{own}");
        }
    }

    #[test]
    fn public_addresses_are_not() {
        for public in [
            "8.8.8.8", "1.1.1.1", "172.32.0.1", "100.63.255.255", "100.128.0.1",
            "203.0.113.5", "2001:4860:4860::8888", "::ffff:8.8.8.8",
        ] {
            assert!(!own_network_ip(ip(public)), "{public}");
        }
    }

    #[test]
    fn names() {
        assert!(local_name("nas.local"));
        assert!(local_name("localhost"));
        assert!(!local_name("demo.jellyfin.org"));
        assert!(plex_tv("plex.tv"));
        assert!(plex_tv("clients.plex.tv"));
        assert!(!plex_tv("notplex.tv"));
        assert!(!plex_tv("x.plex.direct"));
    }
}
