//! EDITOR-10: **the server fetches a public URL, and nothing else.** What
//! `media.unfurl` and `media.fetch` read from outside goes through
//! [`Remote::get`], whose rules are the whole of the server-side request
//! forgery defence:
//!
//! - `http` and `https` only, no credentials in the URL;
//! - the host resolved here, and **every** address it resolves to must be
//!   public ([`public`]): never loopback, private (RFC 1918, IPv6 ULA),
//!   link-local (with the cloud metadata address `169.254.169.254` and
//!   `fd00:ec2::254`), CGNAT (`100.64.0.0/10`), unspecified, multicast,
//!   broadcast, documentation or reserved space — an IPv4 address inside an
//!   IPv6 one (mapped, compatible, NAT64, 6to4) judged as the IPv4 it holds;
//! - the connection goes to the addresses checked (the client is pinned to
//!   them), so a name that resolves twice cannot answer a private address
//!   the second time; no proxy from the environment;
//! - redirects are followed by hand, at most [`MAX_REDIRECTS`], each new URL
//!   held to every rule above before it is asked;
//! - a time limit on the whole fetch and a cap on the bytes read (a
//!   `Content-Length` past it refused before the body, a body past it cut
//!   off and refused); the content type checked against what the caller
//!   takes.
//!
//! A test (and only a test, through a hidden server flag) may name exact
//! `ip:port` pairs that are let through although private ([`Remote::allowing`]):
//! a local server standing in for the internet.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

/// The most redirects one fetch follows.
pub const MAX_REDIRECTS: usize = 3;

/// The time and bytes a page may take: 5 s, 5 MiB.
pub const PAGE_TIMEOUT: Duration = Duration::from_secs(5);
pub const PAGE_MAX: usize = 5 * 1024 * 1024;

/// Why a fetch did not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Not an http(s) URL, or one with credentials, or no host.
    BadUrl(String),
    /// The host is, or resolves to, an address that is not public.
    Blocked(String),
    /// More than [`MAX_REDIRECTS`] redirects.
    TooManyRedirects,
    /// Past the time limit.
    Timeout,
    /// Past the byte cap.
    TooLarge { max: usize },
    /// A content type the caller does not take.
    BadType(String),
    /// The server answered an error status.
    Status(u16),
    /// No answer: the name does not resolve, the connection failed.
    Unreachable(String),
}

impl Refusal {
    /// The wire's word — `refused { reason }`.
    pub fn reason(&self) -> &'static str {
        match self {
            Refusal::BadUrl(_) => "bad_url",
            Refusal::Blocked(_) => "blocked_address",
            Refusal::TooManyRedirects => "too_many_redirects",
            Refusal::Timeout => "timeout",
            Refusal::TooLarge { .. } => "too_large",
            Refusal::BadType(_) => "bad_type",
            Refusal::Status(_) => "http_status",
            Refusal::Unreachable(_) => "unreachable",
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::BadUrl(why) => write!(f, "not a public web address: {why}"),
            Refusal::Blocked(what) => write!(f, "{what} is not a public address; menger fetches only from the public internet"),
            Refusal::TooManyRedirects => write!(f, "more than {MAX_REDIRECTS} redirects"),
            Refusal::Timeout => f.write_str("no answer in time"),
            Refusal::TooLarge { max } => write!(f, "larger than {}", crate::image::size(*max as u64)),
            Refusal::BadType(t) => write!(f, "the answer is {t}, which this does not take"),
            Refusal::Status(s) => write!(f, "the server answered {s}"),
            Refusal::Unreachable(why) => write!(f, "unreachable: {why}"),
        }
    }
}

impl From<Refusal> for crate::MediaEvent {
    fn from(r: Refusal) -> Self {
        let max = match r {
            Refusal::TooLarge { max } => Some(max as u64),
            _ => None,
        };
        crate::MediaEvent::Refused { reason: r.reason().into(), message: format!("{r} (nothing was kept)"), bytes: None, max }
    }
}

/// What a fetch read.
#[derive(Debug, Clone)]
pub struct Got {
    /// The URL that answered, after redirects.
    pub url: url::Url,
    /// Its `Content-Type`'s essence, lower-cased (`text/html`).
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// What one fetch takes.
#[derive(Debug, Clone)]
pub struct Limits {
    pub timeout: Duration,
    pub max: usize,
    /// Content types taken, by essence or by `type/` prefix (`image/`);
    /// empty: any.
    pub accept: Vec<&'static str>,
    /// Content types refused, the same way.
    pub refuse: Vec<&'static str>,
}

impl Limits {
    /// A page: 5 s, 5 MiB, HTML (or an image, which is a card of its own).
    pub fn page() -> Self {
        Limits { timeout: PAGE_TIMEOUT, max: PAGE_MAX, accept: vec!["text/html", "application/xhtml+xml", "image/"], refuse: vec![] }
    }

    fn takes(&self, mime: &str) -> bool {
        let hit = |t: &&str| if t.ends_with('/') { mime.starts_with(*t) } else { mime == *t };
        (self.accept.is_empty() || self.accept.iter().any(hit)) && !self.refuse.iter().any(hit)
    }
}

/// Whether `ip` is an address on the public internet.
pub fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => public_v6(v6),
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(a == 0                                   // "this" network, 0.0.0.0/8
        || a == 10                             // private
        || a == 127                            // loopback
        || (a == 100 && (64..128).contains(&b)) // CGNAT, 100.64.0.0/10
        || (a == 169 && b == 254)              // link-local, the metadata address
        || (a == 172 && (16..32).contains(&b)) // private
        || (a == 192 && b == 0 && c == 0)      // IETF protocol assignments
        || (a == 192 && b == 0 && c == 2)      // TEST-NET-1
        || (a == 192 && b == 88 && c == 99)    // 6to4 relay
        || (a == 192 && b == 168)              // private
        || (a == 198 && (b == 18 || b == 19))  // benchmarking
        || (a == 198 && b == 51 && c == 100)   // TEST-NET-2
        || (a == 203 && b == 0 && c == 113)    // TEST-NET-3
        || a >= 224)                           // multicast, reserved, broadcast
}

fn public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    // An IPv4 address carried inside: mapped (::ffff:a.b.c.d), compatible
    // (::a.b.c.d), NAT64 (64:ff9b::/96), 6to4 (2002:AABB:CCDD::/48).
    let v4 = |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    if s[..5] == [0; 5] && s[5] == 0xffff {
        return public_v4(v4(s[6], s[7]));
    }
    if s[..6] == [0; 6] {
        // ::, ::1, and the deprecated compatible form.
        return !(ip.is_unspecified() || ip.is_loopback()) && public_v4(v4(s[6], s[7]));
    }
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0; 4] {
        return public_v4(v4(s[6], s[7]));
    }
    if s[0] == 0x2002 {
        return public_v4(v4(s[1], s[2]));
    }
    !((s[0] & 0xfe00) == 0xfc00            // unique local, fc00::/7 (fd00:ec2::254 among them)
        || (s[0] & 0xffc0) == 0xfe80       // link-local
        || (s[0] & 0xffc0) == 0xfec0       // site-local, deprecated
        || (s[0] & 0xff00) == 0xff00       // multicast
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || (s[0] == 0x0100 && s[1..4] == [0; 3]) // discard-only
        || (s[0] == 0x2001 && s[1] < 0x0200)) // IETF protocol assignments (Teredo, ORCHID, …)
}

/// The server's fetcher: its rules, and the `ip:port` pairs a test lets
/// through.
#[derive(Debug, Clone, Default)]
pub struct Remote {
    allow: Vec<SocketAddr>,
}

impl Remote {
    /// The rules, nothing let through.
    pub fn new() -> Self {
        Self::default()
    }

    /// For a test alone: `allow`'s exact `ip:port` pairs are fetched from
    /// although they are not public (a local server standing in for the
    /// internet). Every other rule still holds.
    pub fn allowing(allow: Vec<SocketAddr>) -> Self {
        Self { allow }
    }

    fn may(&self, at: SocketAddr) -> bool {
        public(at.ip()) || self.allow.contains(&at)
    }

    /// `url` checked: http(s), a host, no credentials.
    pub fn parse(text: &str) -> Result<url::Url, Refusal> {
        let u = url::Url::parse(text.trim()).map_err(|e| Refusal::BadUrl(e.to_string()))?;
        if !matches!(u.scheme(), "http" | "https") {
            return Err(Refusal::BadUrl(format!("{}: is not http or https", u.scheme())));
        }
        if !u.username().is_empty() || u.password().is_some() {
            return Err(Refusal::BadUrl("credentials in the address".into()));
        }
        if u.host().is_none() {
            return Err(Refusal::BadUrl("no host".into()));
        }
        Ok(u)
    }

    /// Where `u` connects: its host resolved here, each address checked.
    async fn resolve(&self, u: &url::Url) -> Result<(String, Vec<SocketAddr>), Refusal> {
        let port = u.port_or_known_default().ok_or_else(|| Refusal::BadUrl("no port".into()))?;
        let host = match u.host() {
            Some(url::Host::Ipv4(ip)) => return self.checked(String::new(), vec![SocketAddr::new(ip.into(), port)]),
            Some(url::Host::Ipv6(ip)) => return self.checked(String::new(), vec![SocketAddr::new(ip.into(), port)]),
            Some(url::Host::Domain(d)) => d.to_string(),
            None => return Err(Refusal::BadUrl("no host".into())),
        };
        let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), port)).await.map_err(|e| Refusal::Unreachable(format!("{host}: {e}")))?.collect();
        if addrs.is_empty() {
            return Err(Refusal::Unreachable(format!("{host} resolves to nothing")));
        }
        self.checked(host, addrs)
    }

    fn checked(&self, host: String, addrs: Vec<SocketAddr>) -> Result<(String, Vec<SocketAddr>), Refusal> {
        match addrs.iter().find(|a| !self.may(**a)) {
            Some(bad) if host.is_empty() => Err(Refusal::Blocked(bad.ip().to_string())),
            Some(bad) => Err(Refusal::Blocked(format!("{host} ({})", bad.ip()))),
            None => Ok((host, addrs)),
        }
    }

    /// Fetches `url` under `limits` and every rule above.
    pub async fn get(&self, url: &str, limits: &Limits) -> Result<Got, Refusal> {
        let url = Self::parse(url)?;
        match tokio::time::timeout(limits.timeout, self.follow(url, limits)).await {
            Ok(r) => r,
            Err(_) => Err(Refusal::Timeout),
        }
    }

    async fn follow(&self, mut url: url::Url, limits: &Limits) -> Result<Got, Refusal> {
        for hop in 0..=MAX_REDIRECTS {
            let (host, addrs) = self.resolve(&url).await?;
            let mut client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .timeout(limits.timeout)
                .user_agent("menger-unfurl/1 (+https://menger.sh)");
            if !host.is_empty() {
                // Pinned: the connection goes to the addresses just checked.
                client = client.resolve_to_addrs(&host, &addrs);
            }
            let client = client.build().map_err(|e| Refusal::Unreachable(e.to_string()))?;
            let mut r = client
                .get(url.clone())
                .header(reqwest::header::ACCEPT, "text/html,application/xhtml+xml,application/json;q=0.9,image/*;q=0.8,*/*;q=0.5")
                .send()
                .await
                .map_err(|e| if e.is_timeout() { Refusal::Timeout } else { Refusal::Unreachable(e.to_string()) })?;
            let status = r.status();
            if status.is_redirection() {
                if hop == MAX_REDIRECTS {
                    return Err(Refusal::TooManyRedirects);
                }
                let to = r.headers().get(reqwest::header::LOCATION).and_then(|v| v.to_str().ok()).ok_or(Refusal::Status(status.as_u16()))?;
                let next = url.join(to).map_err(|e| Refusal::BadUrl(e.to_string()))?;
                url = Self::parse(next.as_str())?;
                continue;
            }
            if !status.is_success() {
                return Err(Refusal::Status(status.as_u16()));
            }
            let mime = r
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|t| t.split(';').next().unwrap_or("").trim().to_ascii_lowercase())
                .unwrap_or_else(|| crate::kinds::OCTET_STREAM.into());
            if !limits.takes(&mime) {
                return Err(Refusal::BadType(mime));
            }
            if r.content_length().is_some_and(|n| n as usize > limits.max) {
                return Err(Refusal::TooLarge { max: limits.max });
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = r.chunk().await.map_err(|e| if e.is_timeout() { Refusal::Timeout } else { Refusal::Unreachable(e.to_string()) })? {
                if bytes.len() + chunk.len() > limits.max {
                    return Err(Refusal::TooLarge { max: limits.max });
                }
                bytes.extend_from_slice(&chunk);
            }
            return Ok(Got { url, mime, bytes });
        }
        Err(Refusal::TooManyRedirects)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_public_addresses_are_public() {
        let no = [
            "127.0.0.1", "127.255.0.9", "10.0.0.1", "172.16.0.1", "172.31.255.255", "192.168.1.1", "169.254.169.254", "169.254.0.1",
            "100.64.0.1", "100.127.255.254", "0.0.0.0", "0.1.2.3", "224.0.0.1", "239.255.255.250", "255.255.255.255", "240.0.0.1",
            "192.0.2.1", "198.51.100.7", "203.0.113.9", "198.18.0.1", "192.0.0.8",
            "::", "::1", "fe80::1", "fc00::1", "fd00:ec2::254", "ff02::1", "2001:db8::1", "::ffff:127.0.0.1", "::ffff:10.1.2.3",
            "::ffff:169.254.169.254", "64:ff9b::a9fe:a9fe", "2002:7f00:1::", "2002:c0a8:101::1", "::127.0.0.1", "fec0::1", "100::1",
        ];
        for a in no {
            assert!(!public(a.parse().unwrap()), "{a} is not public");
        }
        let yes = ["1.1.1.1", "8.8.8.8", "93.184.216.34", "100.63.255.255", "100.128.0.1", "172.15.0.1", "172.32.0.1", "2606:4700:4700::1111", "2a00:1450:4001:80b::200e", "::ffff:8.8.8.8", "64:ff9b::808:808", "2002:808:808::1"];
        for a in yes {
            assert!(public(a.parse().unwrap()), "{a} is public");
        }
    }

    #[test]
    fn only_http_and_https_without_credentials_are_fetched() {
        for bad in ["ftp://example.com/x", "file:///etc/passwd", "javascript:alert(1)", "gopher://example.com/", "data:text/html,hi", "http://user:pw@example.com/", "http://user@example.com/", "not a url", "http://"] {
            assert_eq!(Remote::parse(bad).map_err(|e| e.reason()), Err("bad_url"), "{bad}");
        }
        assert!(Remote::parse("https://example.com/a?b#c").is_ok());
    }

    #[test]
    fn a_limit_takes_types_by_essence_or_prefix() {
        let page = Limits::page();
        assert!(page.takes("text/html") && page.takes("image/png") && !page.takes("application/octet-stream") && !page.takes("text/plain"));
        let any = Limits { timeout: PAGE_TIMEOUT, max: 1, accept: vec![], refuse: vec!["text/html"] };
        assert!(any.takes("application/pdf") && !any.takes("text/html"));
    }
}
