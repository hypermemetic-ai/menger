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

/// ACCESS-30: what `--public-url` must be, for a server with login on.
///
/// A sign-in sends a credential and sets a cookie; over plain http both
/// travel in clear. So once anyone beyond this machine can reach the server
/// — `--bind` is not loopback, or `--public-url` names a host that is not —
/// the public URL is required and is `https://` (a proxy terminates TLS in
/// front: README "Behind https"). Then the cookies are `Secure` and
/// `__Host-`, the provider's redirect URI is `<public-url>/login/callback`,
/// and the app opens its socket at `wss://<host>/ws`.
///
/// One exception, said at startup: an `http://` URL whose host is on the
/// LAN — a private, link-local or unique-local address, or a `.local`,
/// `.lan`, `.home.arpa`, `.internal` or one-label name — is served with a
/// warning (`Ok(Some(warning))`), so a household LAN that has no TLS yet
/// (prod since MENGER-46) keeps working until it has. Loopback alone, and
/// login off, are not checked (`Ok(None)`).
pub fn check_public_url(bind: IpAddr, public_url: Option<&str>, login: bool) -> Result<Option<String>, String> {
    if !login {
        return Ok(None);
    }
    let parsed = match public_url {
        None => None,
        Some(u) => Some(url::Url::parse(u).map_err(|e| format!("--public-url `{u}` is not a URL: {e}"))?),
    };
    let host = parsed.as_ref().and_then(|u| u.host());
    let beyond = !bind.is_loopback() || host.as_ref().is_some_and(|h| !is_loopback_host(h));
    if !beyond {
        return Ok(None);
    }
    let Some(u) = parsed.as_ref() else {
        return Err(format!(
            "--bind {bind} with login on needs --public-url https://<host> (MENGER_PUBLIC_URL): the URL people reach, \
             behind a proxy that terminates TLS — sign-ins and cookies never travel in clear (ACCESS-30)"
        ));
    };
    match u.scheme() {
        "https" => Ok(None),
        "http" if host.as_ref().is_some_and(is_lan_host) => Ok(Some(format!(
            "--public-url {u} is plain http on the LAN: sign-ins and the session cookie travel in clear there, \
             and cookies are not Secure. Put TLS in front and make it https://… (ACCESS-30)"
        ))),
        s => Err(format!(
            "--public-url {u} is {s}:// for a server reached beyond this machine: it must be https://, \
             behind a proxy that terminates TLS (README \"Behind https\"; ACCESS-30)"
        )),
    }
}

fn is_loopback_host(h: &url::Host<&str>) -> bool {
    match h {
        url::Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    }
}

/// ACCESS-30: a host only a LAN reaches — see [`check_public_url`].
fn is_lan_host(h: &url::Host<&str>) -> bool {
    match h {
        url::Host::Domain(d) => {
            let d = d.trim_end_matches('.').to_ascii_lowercase();
            !d.contains('.') || [".local", ".lan", ".home.arpa", ".internal"].iter().any(|s| d.ends_with(s))
        }
        url::Host::Ipv4(ip) => ip.is_private() || ip.is_link_local(),
        // fc00::/7 unique-local, fe80::/10 link-local.
        url::Host::Ipv6(ip) => (ip.segments()[0] & 0xfe00) == 0xfc00 || (ip.segments()[0] & 0xffc0) == 0xfe80,
    }
}

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

    /// ACCESS-30: beyond loopback with login on, the public URL is https —
    /// an http one on the LAN is served with a warning, anything else is
    /// refused; loopback alone and login off are not checked.
    #[test]
    fn beyond_loopback_a_sign_in_needs_an_https_public_url() {
        let ip = |s| parse_bind(s).unwrap();
        let lo = ip("127.0.0.1");
        // Loopback alone, or login off: nothing to say.
        for u in [None, Some("http://127.0.0.1:4456"), Some("http://localhost:4451"), Some("https://127.0.0.1:8443")] {
            assert_eq!(check_public_url(lo, u, true), Ok(None), "{u:?}");
        }
        assert_eq!(check_public_url(ip("0.0.0.0"), None, false), Ok(None));
        assert_eq!(check_public_url(lo, Some("http://app.menger.sh"), false), Ok(None));
        // https: served.
        for (b, u) in [(lo, "https://app.menger.sh"), (ip("0.0.0.0"), "https://menger.lan.test"), (ip("192.168.8.117"), "https://192.168.8.117")] {
            assert_eq!(check_public_url(b, Some(u), true), Ok(None), "{u}");
        }
        // Beyond loopback with no public URL: refused.
        let e = check_public_url(ip("0.0.0.0"), None, true).unwrap_err();
        assert!(e.contains("needs --public-url https://"), "{e}");
        // Plain http on the LAN: served, said.
        for u in ["http://192.168.8.117:4456", "http://10.0.0.5", "http://dream.local:4456", "http://dream:4456", "http://[fd00::1]:4456", "http://box.home.arpa"] {
            let w = check_public_url(lo, Some(u), true).unwrap().unwrap_or_else(|| panic!("{u}: a warning"));
            assert!(w.contains("plain http on the LAN"), "{w}");
        }
        assert!(check_public_url(ip("192.168.8.117"), Some("http://192.168.8.117:4456"), true).unwrap().is_some());
        // Plain http anywhere else: refused.
        for u in ["http://app.menger.sh", "http://45.56.124.48", "ftp://app.menger.sh"] {
            let e = check_public_url(lo, Some(u), true).unwrap_err();
            assert!(e.contains("must be https://"), "{u}: {e}");
        }
        let e = check_public_url(ip("0.0.0.0"), Some("http://127.0.0.1:4456"), true).unwrap_err();
        assert!(e.contains("must be https://"), "a loopback URL does not excuse a public bind: {e}");
        assert!(check_public_url(lo, Some("not a url"), true).unwrap_err().contains("is not a URL"));
    }

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
