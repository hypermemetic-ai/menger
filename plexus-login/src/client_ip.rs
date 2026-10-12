//! AUTH-22: who a request comes from — the client's IP address, believing
//! `X-Forwarded-For` only from the proxies the operator named.
//!
//! SCAFFOLD-20 found that nothing upstream does this safely: a forwarded
//! header trusted from any peer lets a caller pick its own address, which
//! defeats every per-IP throttle and feeds WorkOS Radar a lie. The rule
//! here is the standard one:
//!
//! - the TCP peer is not a trusted proxy → the client is the peer, and any
//!   `X-Forwarded-For` it sent is ignored;
//! - the peer is a trusted proxy → walk `X-Forwarded-For` from the right
//!   (the hop nearest us) leftwards, skipping addresses that are themselves
//!   trusted proxies; the first that is not is the client. A hop that does
//!   not parse ends the walk at the last trusted hop (whatever is left of
//!   it was written by the client, and proves nothing). If every hop is
//!   trusted, the leftmost is the client.
//!
//! With no trusted proxy configured (the default) the peer is always the
//! client. AUTH-24 #2: behind an edge, that makes every visitor one client,
//! so a page served off loopback must say which it is — `--trusted-proxy
//! <the edge>` or `--trusted-proxy none` (people connect directly) — or it
//! does not start ([`crate::wiring::Wiring::new`]). And an
//! `X-Forwarded-For` arriving from a peer that is not trusted is logged
//! once, loudly: a proxy is in front and is not named. The peer comes from axum's `ConnectInfo<SocketAddr>`: serve the
//! router with `into_make_service_with_connect_info::<SocketAddr>()`
//! (plexus-shell's `serve_on` does). Without it there is no peer, and the
//! throttles share one bucket for every unknown client.

use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;
use axum::http::HeaderMap;
use axum::Extension;
pub use ipnet::IpNet;

/// The proxies whose `X-Forwarded-For` is believed (`--trusted-proxy CIDR`),
/// or `direct`: no proxy, said so (`--trusted-proxy none`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrustedProxies {
    pub nets: Vec<IpNet>,
    /// People connect directly: no proxy is in front.
    pub direct: bool,
}

impl TrustedProxies {
    /// Parses CIDRs (`10.0.0.0/8`, `::1/128`), bare addresses (a /32 or
    /// /128), or `none` alone (direct).
    pub fn parse<I, S>(specs: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let specs: Vec<String> = specs.into_iter().map(|s| s.as_ref().trim().to_string()).filter(|s| !s.is_empty()).collect();
        if specs.iter().any(|s| s == "none") {
            if specs.len() > 1 {
                return Err("--trusted-proxy none stands alone: no proxy, or name them".into());
            }
            return Ok(Self { nets: Vec::new(), direct: true });
        }
        Ok(Self { nets: specs.iter().map(|s| parse_net(s)).collect::<Result<_, _>>()?, direct: false })
    }

    /// Whether anything was said: a proxy named, or `none`.
    pub fn configured(&self) -> bool {
        self.direct || !self.nets.is_empty()
    }

    /// Whether `ip` is one of the trusted proxies.
    pub fn contains(&self, ip: &IpAddr) -> bool {
        let ip = canonical(*ip);
        self.nets.iter().any(|n| n.contains(&ip))
    }

    /// The client's address for a request from `peer` carrying `headers`.
    /// See the module docs.
    pub fn client_ip(&self, peer: Option<IpAddr>, headers: &HeaderMap) -> Option<IpAddr> {
        let peer = canonical(peer?);
        if !self.contains(&peer) {
            if headers.contains_key("x-forwarded-for") && !self.direct {
                warn_once(&UNTRUSTED_XFF, "X-Forwarded-For arrives from a peer that is not a trusted proxy: if a proxy is in front, name it with --trusted-proxy, or every visitor counts as that proxy");
            }
            return Some(peer);
        }
        // Every X-Forwarded-For header, in order, as one list.
        let hops: Vec<&str> = headers
            .get_all("x-forwarded-for")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .map(str::trim)
            .collect();
        let mut nearest_trusted = peer;
        for hop in hops.iter().rev() {
            match parse_hop(hop) {
                Some(ip) if self.contains(&ip) => nearest_trusted = ip,
                Some(ip) => return Some(ip),
                None => return Some(nearest_trusted),
            }
        }
        Some(nearest_trusted)
    }
}

static UNTRUSTED_XFF: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static NO_PEER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn warn_once(flag: &std::sync::atomic::AtomicBool, words: &str) {
    if !flag.swap(true, std::sync::atomic::Ordering::Relaxed) {
        tracing::error!("{words}");
    }
}

/// AUTH-24 #12: a router served without `ConnectInfo` knows no peer, and
/// every client shares one bucket: said once.
pub fn warn_no_peer() {
    warn_once(&NO_PEER, "the sign-in page is served without the peer's address (serve with into_make_service_with_connect_info::<SocketAddr>()): every client shares one throttle bucket");
}

/// A CIDR or a bare address.
pub fn parse_net(s: &str) -> Result<IpNet, String> {
    let s = s.trim();
    s.parse::<IpNet>()
        .or_else(|_| s.parse::<IpAddr>().map(IpNet::from))
        .map_err(|_| format!("`{s}` is not a CIDR (10.0.0.0/8, fd00::/8) or an address"))
}

/// One `X-Forwarded-For` entry: an address, maybe with a port
/// (`1.2.3.4:5678`, `[::1]:5678`).
fn parse_hop(hop: &str) -> Option<IpAddr> {
    if let Ok(ip) = hop.parse::<IpAddr>() {
        return Some(canonical(ip));
    }
    hop.parse::<SocketAddr>().ok().map(|s| canonical(s.ip()))
}

/// An IPv4-mapped IPv6 address as the IPv4 address it is.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        v4 => v4,
    }
}

/// The TCP peer, when the server was made with connect info.
pub type Peer = Option<Extension<ConnectInfo<SocketAddr>>>;

/// The peer's address out of [`Peer`].
pub fn peer_ip(peer: &Peer) -> Option<IpAddr> {
    peer.as_ref().map(|Extension(ConnectInfo(addr))| addr.ip())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn xff(v: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.append("x-forwarded-for", HeaderValue::from_str(v).unwrap());
        h
    }

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn an_untrusted_peer_is_the_client_whatever_it_forwards() {
        let none = TrustedProxies::default();
        assert_eq!(none.client_ip(Some(ip("203.0.113.9")), &xff("1.1.1.1")), Some(ip("203.0.113.9")));
        let edge = TrustedProxies::parse(["10.0.0.0/8"]).unwrap();
        assert_eq!(edge.client_ip(Some(ip("203.0.113.9")), &xff("1.1.1.1")), Some(ip("203.0.113.9")), "not the edge: ignored");
    }

    #[test]
    fn behind_a_trusted_proxy_the_rightmost_untrusted_hop_is_the_client() {
        let edge = TrustedProxies::parse(["10.0.0.0/8", "192.0.2.7"]).unwrap();
        // The client claimed 6.6.6.6; the edge appended the real address.
        assert_eq!(edge.client_ip(Some(ip("10.1.2.3")), &xff("6.6.6.6, 198.51.100.4")), Some(ip("198.51.100.4")));
        // Two trusted hops before the real one.
        assert_eq!(edge.client_ip(Some(ip("10.1.2.3")), &xff("6.6.6.6, 198.51.100.4, 192.0.2.7, 10.9.9.9")), Some(ip("198.51.100.4")));
        // Several headers are one list.
        let mut h = xff("6.6.6.6");
        h.append("x-forwarded-for", HeaderValue::from_static("198.51.100.4:4431"));
        assert_eq!(edge.client_ip(Some(ip("10.1.2.3")), &h), Some(ip("198.51.100.4")));
        // No header: the proxy itself.
        assert_eq!(edge.client_ip(Some(ip("10.1.2.3")), &HeaderMap::new()), Some(ip("10.1.2.3")));
        // Garbage nearest us ends the walk at the last trusted hop.
        assert_eq!(edge.client_ip(Some(ip("10.1.2.3")), &xff("6.6.6.6, not-an-ip")), Some(ip("10.1.2.3")));
        // All trusted: the leftmost.
        assert_eq!(edge.client_ip(Some(ip("10.1.2.3")), &xff("10.0.0.5, 10.0.0.6")), Some(ip("10.0.0.5")));
    }

    #[test]
    fn mapped_addresses_and_bare_addresses_count() {
        let edge = TrustedProxies::parse(["127.0.0.1"]).unwrap();
        assert!(edge.contains(&ip("::ffff:127.0.0.1")));
        assert_eq!(edge.client_ip(Some(ip("::ffff:127.0.0.1")), &xff("[2001:db8::1]:443")), Some(ip("2001:db8::1")));
        assert!(TrustedProxies::parse(["10.0.0.0/33"]).is_err());
        assert!(TrustedProxies::parse(["edge"]).is_err());
        let none = TrustedProxies::parse(["none"]).unwrap();
        assert!(none.direct && none.configured() && none.nets.is_empty());
        assert!(TrustedProxies::parse(["none", "10.0.0.0/8"]).is_err());
        assert!(!TrustedProxies::default().configured());
    }

    #[test]
    fn no_peer_is_no_client() {
        assert_eq!(TrustedProxies::default().client_ip(None, &xff("1.1.1.1")), None);
    }
}
