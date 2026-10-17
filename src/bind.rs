//! RUN-8: the address `--bind` names (`MENGER_BIND`, default `127.0.0.1`;
//! `0.0.0.0`, `::` and any address of this machine, IPv6 too) and what a
//! listener there answers to.
//!
//! Every listener binds it: the shell itself ([`crate::shell::serve_at`]),
//! the WebSocket and the REST bridge through plexus-transport's builder
//! (`TransportServerBuilder::bind`, TRANSPORT-7), which also tells the
//! registry a specific address. What stays here is menger's half: the flag's
//! spelling, and the names and origin a specific address adds (ACCESS-25,
//! IDENTITY-29).

use std::net::{IpAddr, SocketAddr};

/// `--bind`'s value: an IP address, IPv6 with or without its brackets.
pub fn parse_bind(s: &str) -> Result<IpAddr, String> {
    let t = s.trim();
    let t = t.strip_prefix('[').and_then(|r| r.strip_suffix(']')).unwrap_or(t);
    t.parse().map_err(|_| format!("`{s}` is not an IP address (e.g. 127.0.0.1, 0.0.0.0, ::, 192.168.1.10)"))
}

/// The `Host` a client names reaching `bind` on `port` by its address —
/// `192.168.1.10:4455`, `[fd00::1]:4455` — answered beside
/// [`crate::local::listener_hosts`]' (ACCESS-25). Loopback is already
/// answered, and an unspecified address (`0.0.0.0`, `::`) is every address
/// this machine has: none is assumed, each is an `--allowed-host`.
pub fn bind_hosts(bind: IpAddr, port: u16) -> Vec<String> {
    if bind.is_loopback() || bind.is_unspecified() {
        return Vec::new();
    }
    vec![SocketAddr::new(bind, port).to_string()]
}

/// The shell's origin at a specific bound address (`http://192.168.1.10:4456`),
/// whose pages may open the socket (IDENTITY-29); none for loopback (already
/// admitted) or an unspecified address (`--allowed-origin` names them).
pub fn bind_origin(bind: IpAddr, shell_port: u16) -> Option<String> {
    (!bind.is_loopback() && !bind.is_unspecified()).then(|| format!("http://{}", SocketAddr::new(bind, shell_port)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_takes_v4_v6_bracketed_and_unspecified_and_refuses_names() {
        assert_eq!(parse_bind("127.0.0.1").unwrap(), IpAddr::from([127, 0, 0, 1]));
        assert!(parse_bind("0.0.0.0").unwrap().is_unspecified());
        assert!(parse_bind("::").unwrap().is_unspecified());
        assert_eq!(parse_bind("[::1]").unwrap(), parse_bind("::1").unwrap());
        assert_eq!(parse_bind(" 192.168.8.117 ").unwrap().to_string(), "192.168.8.117");
        for bad in ["localhost", "", "dream.local", "1.2.3", "127.0.0.1:4455"] {
            assert!(parse_bind(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_specific_address_is_answered_by_its_literal_loopback_and_unspecified_add_nothing() {
        let ip = |s| parse_bind(s).unwrap();
        assert_eq!(bind_hosts(ip("192.168.8.117"), 4455), ["192.168.8.117:4455"]);
        assert_eq!(bind_hosts(ip("fd00::1"), 4455), ["[fd00::1]:4455"]);
        for none in ["127.0.0.1", "::1", "0.0.0.0", "::"] {
            assert!(bind_hosts(ip(none), 4455).is_empty(), "{none}");
            assert_eq!(bind_origin(ip(none), 4456), None, "{none}");
        }
        assert_eq!(bind_origin(ip("192.168.8.117"), 4456).as_deref(), Some("http://192.168.8.117:4456"));
        assert_eq!(bind_origin(ip("fd00::1"), 4456).as_deref(), Some("http://[fd00::1]:4456"));
    }
}
