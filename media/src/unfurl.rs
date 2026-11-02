//! EDITOR-10: **a link becomes a card.** `media.unfurl { url }` reads a
//! public page through [`Remote`] (every SSRF rule there) and answers what
//! it is: its title, description, site and preview image — the image
//! fetched and kept as media, so a card never hot-links. A page's
//! OpenGraph and Twitter tags first (`og:title`, `twitter:image`, …, then
//! `<title>` and `description`); for the five providers ([`Provider`]) their
//! oEmbed answer, and for four of them a player: the iframe address is
//! built here from the link's own id, on the provider's own player host —
//! never an address or markup a page or an oEmbed answer gave. GitHub has
//! no player; its card is a repository's.
//!
//! What a URL unfurled to is kept a day ([`CACHE_SECONDS`]) in the
//! service's file, by the URL as given.

use crate::kinds;
use crate::remote::{Limits, Refusal, Remote};
use crate::store::{MediaStore, Owner};
use crate::MediaEvent;
use std::sync::Arc;

/// How long an unfurl is kept: a day.
pub const CACHE_SECONDS: i64 = 24 * 3600;

/// The providers drawn as more than a link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    YouTube { id: String },
    Vimeo { id: String },
    X { id: String },
    GitHub,
    Figma,
}

impl Provider {
    /// The provider a link is on, by its host and path — `None` for any other.
    pub fn of(u: &url::Url) -> Option<Provider> {
        let host = u.host_str()?.trim_start_matches("www.").trim_start_matches("m.").to_ascii_lowercase();
        let segs: Vec<&str> = u.path_segments().map(|s| s.filter(|p| !p.is_empty()).collect()).unwrap_or_default();
        let youtube_id = |s: &str| (s.len() == 11 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')).then(|| s.to_string());
        match host.as_str() {
            "youtube.com" | "music.youtube.com" | "youtube-nocookie.com" => {
                let id = match segs.as_slice() {
                    ["watch"] => u.query_pairs().find(|(k, _)| k == "v").and_then(|(_, v)| youtube_id(&v)),
                    ["shorts" | "embed" | "live" | "v", id, ..] => youtube_id(id),
                    _ => None,
                };
                id.map(|id| Provider::YouTube { id })
            }
            "youtu.be" => segs.first().and_then(|s| youtube_id(s)).map(|id| Provider::YouTube { id }),
            "vimeo.com" | "player.vimeo.com" => segs.iter().rev().find(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).map(|id| Provider::Vimeo { id: id.to_string() }),
            "twitter.com" | "x.com" | "mobile.twitter.com" | "mobile.x.com" => match segs.as_slice() {
                [_, "status" | "statuses", id, ..] if id.bytes().all(|b| b.is_ascii_digit()) && !id.is_empty() => Some(Provider::X { id: id.to_string() }),
                _ => None,
            },
            "github.com" => (segs.len() >= 2).then_some(Provider::GitHub),
            "figma.com" => matches!(segs.first(), Some(&("file" | "design" | "proto" | "board" | "slides"))).then_some(Provider::Figma),
            _ => None,
        }
    }

    /// Its name on the wire.
    pub fn name(&self) -> &'static str {
        match self {
            Provider::YouTube { .. } => "youtube",
            Provider::Vimeo { .. } => "vimeo",
            Provider::X { .. } => "x",
            Provider::GitHub => "github",
            Provider::Figma => "figma",
        }
    }

    /// The card's kind.
    pub fn kind(&self) -> &'static str {
        match self {
            Provider::YouTube { .. } | Provider::Vimeo { .. } => "video",
            Provider::X { .. } => "tweet",
            Provider::GitHub => "repo",
            Provider::Figma => "design",
        }
    }

    /// The site's name, as the card says it.
    pub fn site(&self) -> &'static str {
        match self {
            Provider::YouTube { .. } => "YouTube",
            Provider::Vimeo { .. } => "Vimeo",
            Provider::X { .. } => "X",
            Provider::GitHub => "GitHub",
            Provider::Figma => "Figma",
        }
    }

    /// The player's address, built from the link's id on the provider's
    /// own player host; GitHub has none.
    pub fn embed(&self, u: &url::Url) -> Option<String> {
        match self {
            Provider::YouTube { id } => Some(format!("https://www.youtube-nocookie.com/embed/{id}")),
            Provider::Vimeo { id } => Some(format!("https://player.vimeo.com/video/{id}")),
            Provider::X { id } => Some(format!("https://platform.twitter.com/embed/Tweet.html?id={id}&dnt=true")),
            Provider::GitHub => None,
            Provider::Figma => Some(format!("https://www.figma.com/embed?embed_host=menger&url={}", encode(u.as_str()))),
        }
    }

    /// Its oEmbed endpoint for `u`; GitHub has none.
    fn oembed(&self, u: &url::Url) -> Option<String> {
        let at = match self {
            Provider::YouTube { .. } => "https://www.youtube.com/oembed?format=json&url=",
            Provider::Vimeo { .. } => "https://vimeo.com/api/oembed.json?url=",
            Provider::X { .. } => "https://publish.twitter.com/oembed?omit_script=1&dnt=true&url=",
            Provider::GitHub => return None,
            Provider::Figma => "https://www.figma.com/api/oembed?url=",
        };
        Some(format!("{at}{}", encode(u.as_str())))
    }
}

/// The frame origins a page may frame for these providers — what the
/// shell's `frame-src` names, and nothing broader.
pub const EMBED_ORIGINS: [&str; 4] = ["https://www.youtube-nocookie.com", "https://player.vimeo.com", "https://platform.twitter.com", "https://www.figma.com"];

fn encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// What a page says about itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    pub title: Option<String>,
    pub description: Option<String>,
    pub site: Option<String>,
    pub image: Option<String>,
}

/// `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&#39;`, `&#NN;`, `&#xNN;` read; any
/// other entity left as written.
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            e if e.starts_with("#x") || e.starts_with("#X") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32),
            e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Text as a card shows it: entities read, whitespace folded, at most
/// `max` characters.
fn tidy(s: &str, max: usize) -> Option<String> {
    let t = unescape(s).split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = t.chars().filter(|c| !c.is_control()).collect();
    let t = if t.chars().count() > max { format!("{}…", t.chars().take(max).collect::<String>()) } else { t };
    (!t.is_empty()).then_some(t)
}

/// A tag's attributes, lower-cased names: `<meta property="og:title" content='x'>`.
fn attrs(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let b = tag.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let start = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'>' && b[i] != b'/' {
            i += 1;
        }
        let name = tag[start..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let mut value = String::new();
        if i < b.len() && b[i] == b'=' {
            i += 1;
            while i < b.len() && b[i].is_ascii_whitespace() {
                i += 1;
            }
            if i < b.len() && (b[i] == b'"' || b[i] == b'\'') {
                let q = b[i];
                i += 1;
                let s = i;
                while i < b.len() && b[i] != q {
                    i += 1;
                }
                value = tag[s..i].to_string();
                i += 1;
            } else {
                let s = i;
                while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'>' {
                    i += 1;
                }
                value = tag[s..i].to_string();
            }
        }
        if name.is_empty() {
            i += 1;
        } else {
            out.push((name, value));
        }
    }
    out
}

/// What a page's head says: OpenGraph, then Twitter's tags, then
/// `<title>` and `description`; a relative image read against `base`.
pub fn meta(html: &str, base: &url::Url) -> Meta {
    let lower = html.to_ascii_lowercase();
    let mut tags: Vec<(String, String)> = Vec::new();
    let mut from = 0;
    while let Some(at) = lower[from..].find("<meta") {
        let s = from + at + 5;
        let Some(end) = lower[s..].find('>') else { break };
        let a = attrs(&html[s..s + end]);
        let key = a.iter().find(|(n, _)| n == "property" || n == "name").map(|(_, v)| v.to_ascii_lowercase());
        let content = a.iter().find(|(n, _)| n == "content").map(|(_, v)| v.clone());
        if let (Some(k), Some(c)) = (key, content) {
            tags.push((k, c));
        }
        from = s + end;
    }
    let first = |keys: &[&str]| keys.iter().find_map(|k| tags.iter().find(|(n, c)| n == k && !c.trim().is_empty()).map(|(_, c)| c.clone()));
    let title_tag = lower.find("<title").and_then(|s| {
        let open = s + lower[s..].find('>')? + 1;
        let close = open + lower[open..].find("</title")?;
        Some(html[open..close].to_string())
    });
    let image = first(&["og:image:secure_url", "og:image", "og:image:url", "twitter:image", "twitter:image:src"])
        .and_then(|i| base.join(unescape(i.trim()).as_str()).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(|u| u.to_string());
    Meta {
        title: first(&["og:title", "twitter:title"]).or(title_tag).and_then(|t| tidy(&t, 300)),
        description: first(&["og:description", "twitter:description", "description"]).and_then(|t| tidy(&t, 500)),
        site: first(&["og:site_name", "application-name"]).and_then(|t| tidy(&t, 100)),
        image,
    }
}

/// The text of an oEmbed `html` (a tweet's blockquote): its tags dropped.
fn text_of(html: &str) -> Option<String> {
    let p = html.find("<p").and_then(|s| Some((s, s + html[s..].find("</p>")?))).map(|(s, e)| &html[s..e]).unwrap_or(html);
    let mut out = String::new();
    let mut inside = false;
    for c in p.chars() {
        match c {
            '<' => inside = true,
            '>' => inside = false,
            c if !inside => out.push(c),
            _ => {}
        }
    }
    tidy(&out, 500)
}

/// The image a card shows, fetched and kept as anyone's (a public page's
/// preview) — `None` when it cannot be fetched or is not an image.
async fn keep_image(remote: &Remote, store: &Arc<MediaStore>, src: &str) -> Option<String> {
    let limits = Limits { accept: vec!["image/"], ..Limits::page() };
    let got = remote.get(src, &limits).await.ok()?;
    let name = got.url.path_segments().and_then(|mut s| s.next_back().map(str::to_string));
    if kinds::sniff(&got.bytes).ok()?.kind != kinds::Kind::Image {
        return None;
    }
    store.put_bytes(Owner::Anyone, got.bytes, name, None, kinds::PUT_MAX).await.ok().map(|p| p.info.hash)
}

/// `media.unfurl`: the card `url` draws as — from the day's cache, or read now.
pub async fn unfurl(remote: &Remote, store: &Arc<MediaStore>, url: &str) -> MediaEvent {
    let u = match Remote::parse(url) {
        Ok(u) => u,
        Err(r) => return r.into(),
    };
    if let Ok(Some(answer)) = store.unfurl_cached(u.as_str(), CACHE_SECONDS).await {
        if let Ok(ev) = serde_json::from_str::<MediaEvent>(&answer) {
            return ev;
        }
    }
    let ev = read(remote, store, &u).await;
    if matches!(ev, MediaEvent::Unfurled { .. }) {
        if let Ok(answer) = serde_json::to_string(&ev) {
            let _ = store.unfurl_keep(u.as_str(), answer).await;
        }
    }
    ev
}

async fn read(remote: &Remote, store: &Arc<MediaStore>, u: &url::Url) -> MediaEvent {
    let provider = Provider::of(u);
    let mut m = Meta::default();
    // A provider's oEmbed first: what its pages hide behind script.
    if let Some(endpoint) = provider.as_ref().and_then(|p| p.oembed(u)) {
        let limits = Limits { accept: vec!["application/json", "text/json", "text/javascript"], ..Limits::page() };
        if let Ok(got) = remote.get(&endpoint, &limits).await {
            if let Ok(o) = serde_json::from_slice::<serde_json::Value>(&got.bytes) {
                let s = |k: &str| o[k].as_str().map(str::to_string);
                let tweet = matches!(provider, Some(Provider::X { .. }));
                m.title = (if tweet { s("author_name") } else { s("title").or(s("author_name")) }).and_then(|t| tidy(&t, 300));
                m.description = if tweet { s("html").and_then(|h| text_of(&h)) } else { s("description").and_then(|t| tidy(&t, 500)) };
                m.site = s("provider_name").and_then(|t| tidy(&t, 100));
                m.image = s("thumbnail_url").filter(|i| i.starts_with("https://") || i.starts_with("http://"));
            }
        }
    }
    let mut kind = provider.as_ref().map_or("link", Provider::kind);
    if m.title.is_none() {
        match remote.get(u.as_str(), &Limits::page()).await {
            Ok(got) if got.mime.starts_with("image/") => {
                // The link is an image itself: its card is the image, kept.
                let name = got.url.path_segments().and_then(|mut s| s.next_back().map(str::to_string)).filter(|n| !n.is_empty());
                let image = match kinds::sniff(&got.bytes) {
                    Ok(s) if s.kind == kinds::Kind::Image => store.put_bytes(Owner::Anyone, got.bytes, name.clone(), None, kinds::PUT_MAX).await.ok().map(|p| p.info.hash),
                    _ => None,
                };
                return MediaEvent::Unfurled { url: u.to_string(), kind: "image".into(), title: name, description: None, site: u.host_str().map(str::to_string), image, provider: None, embed: None };
            }
            Ok(got) => {
                let page = meta(&String::from_utf8_lossy(&got.bytes), &got.url);
                m = Meta { title: m.title.or(page.title), description: m.description.or(page.description), site: m.site.or(page.site), image: m.image.or(page.image) };
            }
            Err(r) if provider.is_none() => return r.into(),
            Err(_) => {}
        }
    }
    if provider.is_none() && m.title.is_none() && m.description.is_none() && m.image.is_none() {
        kind = "link";
    }
    let image = match &m.image {
        Some(src) => keep_image(remote, store, src).await,
        None => None,
    };
    MediaEvent::Unfurled {
        url: u.to_string(),
        kind: kind.into(),
        title: m.title,
        description: m.description,
        site: m.site.or_else(|| provider.as_ref().map(|p| p.site().to_string())).or_else(|| u.host_str().map(|h| h.trim_start_matches("www.").to_string())),
        image,
        provider: provider.as_ref().map(|p| p.name().to_string()),
        embed: provider.as_ref().and_then(|p| p.embed(u)),
    }
}

/// `media.fetch`: a remote image or file kept as media — `owner`'s, by
/// `by` — under every rule of [`Remote`]; a page is refused (it is a link:
/// unfurl it).
pub async fn fetch(remote: &Remote, store: &Arc<MediaStore>, url: &str, owner: Owner, name: Option<String>, by: String) -> MediaEvent {
    let limits = Limits { timeout: FETCH_TIMEOUT, max: kinds::PUT_MAX, accept: vec![], refuse: vec!["text/html", "application/xhtml+xml"] };
    let got = match remote.get(url, &limits).await {
        Ok(g) => g,
        Err(r) => return r.into(),
    };
    let name = name.or_else(|| got.url.path_segments().and_then(|mut s| s.next_back().map(str::to_string)).filter(|n| !n.is_empty()));
    match store.put_bytes(owner, got.bytes, name, Some(by), kinds::PUT_MAX).await {
        Ok(p) => MediaEvent::stored(p.info, p.duplicate),
        Err(crate::store::StoreError::Refused(p)) => p.into(),
        Err(e) => MediaEvent::Error { message: e.to_string() },
    }
}

/// How long `media.fetch` waits for a file: 10 s (a file is larger than a page).
pub const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

impl From<&Refusal> for MediaEvent {
    fn from(r: &Refusal) -> Self {
        r.clone().into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> url::Url {
        url::Url::parse(s).unwrap()
    }

    #[test]
    fn a_provider_is_known_by_its_link_and_its_player_is_built_here() {
        let yt = Provider::of(&u("https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1")).unwrap();
        assert_eq!(yt, Provider::YouTube { id: "dQw4w9WgXcQ".into() });
        assert_eq!(yt.embed(&u("https://x")).as_deref(), Some("https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ"));
        assert_eq!(Provider::of(&u("https://youtu.be/dQw4w9WgXcQ")), Some(yt.clone()));
        assert_eq!(Provider::of(&u("https://m.youtube.com/shorts/dQw4w9WgXcQ")), Some(yt));
        assert_eq!(Provider::of(&u("https://www.youtube.com/watch?v=<script>")), None, "an id that is not one is no player");
        assert_eq!(Provider::of(&u("https://vimeo.com/76979871")), Some(Provider::Vimeo { id: "76979871".into() }));
        let x = Provider::of(&u("https://x.com/jack/status/20")).unwrap();
        assert_eq!((x.kind(), x.embed(&u("https://x")).as_deref()), ("tweet", Some("https://platform.twitter.com/embed/Tweet.html?id=20&dnt=true")));
        assert_eq!(Provider::of(&u("https://twitter.com/jack")), None, "a profile is a link");
        let gh = Provider::of(&u("https://github.com/rust-lang/rust")).unwrap();
        assert_eq!((gh.kind(), gh.embed(&u("https://github.com/rust-lang/rust"))), ("repo", None));
        assert_eq!(Provider::of(&u("https://github.com/rust-lang")), None);
        let f = u("https://www.figma.com/design/abc123/Name?node-id=1-2");
        let fig = Provider::of(&f).unwrap();
        assert_eq!(fig.embed(&f).as_deref(), Some("https://www.figma.com/embed?embed_host=menger&url=https%3A%2F%2Fwww.figma.com%2Fdesign%2Fabc123%2FName%3Fnode-id%3D1-2"));
        assert_eq!(Provider::of(&u("https://evil.example/youtube.com/watch?v=dQw4w9WgXcQ")), None);
        assert_eq!(Provider::of(&u("https://youtube.com.evil.example/watch?v=dQw4w9WgXcQ")), None);
        for p in [Provider::YouTube { id: "a".into() }, Provider::Vimeo { id: "1".into() }, Provider::X { id: "1".into() }, Provider::Figma] {
            let e = p.embed(&f).unwrap();
            assert!(EMBED_ORIGINS.iter().any(|o| e.starts_with(&format!("{o}/"))), "{e} is on an allowed origin");
        }
    }

    #[test]
    fn a_page_says_its_title_description_site_and_image() {
        let html = r#"<!doctype html><html><head><title>Plain &amp; title</title>
            <meta property="og:title" content="The &quot;real&quot; title">
            <meta name=description content='A page,
               about   things'>
            <META PROPERTY="og:site_name" CONTENT="Example">
            <meta property="og:image" content="/img/card.png" />
            </head><body>…</body></html>"#;
        let m = meta(html, &u("https://example.com/a/b"));
        assert_eq!(m.title.as_deref(), Some("The \"real\" title"));
        assert_eq!(m.description.as_deref(), Some("A page, about things"));
        assert_eq!(m.site.as_deref(), Some("Example"));
        assert_eq!(m.image.as_deref(), Some("https://example.com/img/card.png"));
        let bare = meta("<title>Only &#x41;</title><meta name='twitter:image' content='javascript:alert(1)'>", &u("https://e.com/"));
        assert_eq!((bare.title.as_deref(), bare.image), (Some("Only A"), None), "an image that is not http(s) is none");
    }

    #[test]
    fn a_tweets_text_is_its_paragraph_without_markup() {
        let html = r#"<blockquote class="twitter-tweet"><p lang="en" dir="ltr">just setting up my <a href="x">twttr</a></p>&mdash; jack</blockquote>"#;
        assert_eq!(text_of(html).as_deref(), Some("just setting up my twttr"));
    }
}
