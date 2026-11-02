//! EDITOR-10: what the server reads from outside, and what it refuses —
//! [`Remote`]'s rules proved one by one, against a local server standing in
//! for the internet. The local server is on loopback, which the rules
//! refuse: the allowed cases let exactly its `ip:port` through
//! ([`Remote::allowing`], what the hidden `--media-fetch-allow` flag sets
//! for the browser tests), and every refusal is proved with the default
//! rules or with an address that pair does not name.

use menger_media::remote::{Limits, Refusal, Remote};
use menger_media::unfurl;
use menger_media::{MediaEvent, MediaStore};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// One answer: status, content type, body; `delay` before it.
#[derive(Clone)]
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
}

fn ok(mime: &str, body: impl Into<Vec<u8>>) -> Answer {
    Answer { status: 200, headers: vec![("content-type".into(), mime.into())], body: body.into(), delay: Duration::ZERO }
}

fn redirect(to: &str) -> Answer {
    Answer { status: 302, headers: vec![("location".into(), to.into())], body: Vec::new(), delay: Duration::ZERO }
}

/// A local server answering each path by `route`.
async fn serve(route: impl Fn(&str) -> Answer + Send + Sync + 'static) -> SocketAddr {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    let route = Arc::new(route);
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else { return };
            let route = route.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let mut got = Vec::new();
                while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                    match sock.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => got.extend_from_slice(&buf[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&got).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
                let a = route(&path);
                tokio::time::sleep(a.delay).await;
                let mut out = format!("HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\n", a.status, a.body.len());
                for (k, v) in &a.headers {
                    out.push_str(&format!("{k}: {v}\r\n"));
                }
                out.push_str("\r\n");
                let _ = sock.write_all(out.as_bytes()).await;
                let _ = sock.write_all(&a.body).await;
            });
        }
    });
    at
}

/// A real 2×1 PNG.
fn png() -> Vec<u8> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII=").unwrap()
}

fn page(image: &str) -> String {
    format!(
        r#"<!doctype html><html><head><title>fallback</title>
        <meta property="og:title" content="A local page">
        <meta property="og:description" content="Standing in for the internet.">
        <meta property="og:site_name" content="Local">
        <meta property="og:image" content="{image}">
        </head><body>hi</body></html>"#
    )
}

fn store() -> (tempfile::TempDir, Arc<MediaStore>) {
    let dir = tempfile::tempdir().unwrap();
    let s = Arc::new(MediaStore::open(&dir.path().join("media.db")).unwrap());
    (dir, s)
}

fn short() -> Limits {
    Limits { timeout: Duration::from_millis(800), ..Limits::page() }
}

#[tokio::test]
async fn an_allowed_page_unfurls_to_a_card_its_image_kept_and_cached_a_day() {
    let at = serve(|p| match p {
        "/page" => ok("text/html; charset=utf-8", page("/card.png")),
        "/card.png" => ok("image/png", png()),
        "/raw.png" => ok("image/png", png()),
        _ => Answer { status: 404, ..ok("text/plain", "no") },
    })
    .await;
    let remote = Remote::allowing(vec![at]);
    let (_dir, s) = store();
    let url = format!("http://{at}/page");
    let ev = unfurl::unfurl(&remote, &s, &url).await;
    let MediaEvent::Unfurled { kind, title, description, site, image, provider, embed, .. } = ev.clone() else { panic!("{ev:?}") };
    assert_eq!((kind.as_str(), title.as_deref(), description.as_deref(), site.as_deref()), ("link", Some("A local page"), Some("Standing in for the internet."), Some("Local")));
    assert_eq!((provider, embed), (None, None), "no provider: no player");
    let hash = image.expect("the preview image, kept");
    let (info, bytes) = s.bytes(&hash).await.unwrap();
    assert_eq!((info.mime.as_str(), bytes), ("image/png", png()), "kept as media, never hot-linked");
    assert_eq!(s.kept(&hash).await.unwrap().owners, vec![menger_media::Owner::Anyone], "a public page's preview: anyone signed in sees it");
    // Cached: the same answer with the server gone quiet.
    let again = unfurl::unfurl(&Remote::new(), &s, &url).await;
    assert_eq!(again, ev, "from the day's cache, not fetched again (the default rules would refuse it)");
    // A link that is an image: its card is the image.
    let img = unfurl::unfurl(&remote, &s, &format!("http://{at}/raw.png")).await;
    assert!(matches!(&img, MediaEvent::Unfurled { kind, image: Some(_), .. } if kind == "image"), "{img:?}");
    // And `fetch` keeps a remote file as the caller's.
    let owner = menger_media::Owner::App(Some("key:aa".into()));
    let got = unfurl::fetch(&remote, &s, &format!("http://{at}/card.png"), owner, None, "key:aa".into()).await;
    assert!(matches!(&got, MediaEvent::Stored { name: Some(n), mime, .. } if n == "card.png" && mime == "image/png"), "{got:?}");
    // A page is no file to keep.
    let page = unfurl::fetch(&remote, &s, &url, menger_media::Owner::App(None), None, "key:aa".into()).await;
    assert!(matches!(&page, MediaEvent::Refused { reason, .. } if reason == "bad_type"), "{page:?}");
}

#[tokio::test]
async fn loopback_private_link_local_cgnat_and_metadata_addresses_are_refused() {
    let at = serve(|_| ok("text/html", page("/x.png"))).await;
    let remote = Remote::new();
    let refuse = |url: String| {
        let remote = remote.clone();
        async move { remote.get(&url, &short()).await.map(|_| ()).map_err(|e| e.reason()) }
    };
    // The very server the allowed test reads: refused without the allowance.
    assert_eq!(refuse(format!("http://{at}/page")).await, Err("blocked_address"));
    // A name that resolves to loopback is judged by what it resolves to.
    assert_eq!(refuse(format!("http://localhost:{}/page", at.port())).await, Err("blocked_address"));
    for host in [
        "127.0.0.1", "10.0.0.1", "172.16.0.1", "192.168.1.1", "169.254.169.254", "100.64.0.1", "0.0.0.0", "[::1]", "[fe80::1]", "[fd00:ec2::254]",
        "[::ffff:127.0.0.1]", "[::ffff:169.254.169.254]", "2130706433", "0x7f.1",
    ] {
        assert_eq!(refuse(format!("http://{host}/latest/meta-data/")).await, Err("blocked_address"), "{host}");
    }
    // The allowance is the exact pair: another port on the same address is still refused.
    let narrow = Remote::allowing(vec![SocketAddr::new(at.ip(), at.port() + 1)]);
    assert_eq!(narrow.get(&format!("http://{at}/page"), &short()).await.map(|_| ()).map_err(|e| e.reason()), Err("blocked_address"));
    // Schemes and credentials.
    for bad in ["file:///etc/passwd", "ftp://example.com/", "gopher://example.com/", "http://user:pw@example.com/"] {
        assert_eq!(refuse(bad.to_string()).await, Err("bad_url"), "{bad}");
    }
}

#[tokio::test]
async fn a_redirect_is_checked_again_and_at_most_three_are_followed() {
    let at = serve(move |p| match p {
        "/to-metadata" => redirect("http://169.254.169.254/latest/meta-data/"),
        "/to-loopback" => redirect("http://127.0.0.2/admin"),
        "/to-file" => redirect("file:///etc/passwd"),
        "/r1" => redirect("/r2"),
        "/r2" => redirect("/r3"),
        "/r3" => redirect("/done"),
        "/r0" => redirect("/r1"),
        "/done" => ok("text/html", page("/x.png")),
        _ => Answer { status: 404, ..ok("text/plain", "no") },
    })
    .await;
    let remote = Remote::allowing(vec![at]);
    let get = |p: &str| {
        let (remote, url) = (remote.clone(), format!("http://{at}{p}"));
        async move { remote.get(&url, &short()).await.map(|g| g.url.path().to_string()).map_err(|e| e.reason()) }
    };
    assert_eq!(get("/to-metadata").await, Err("blocked_address"), "a redirect to the metadata address");
    assert_eq!(get("/to-loopback").await, Err("blocked_address"), "a redirect to another loopback address");
    assert_eq!(get("/to-file").await, Err("bad_url"));
    assert_eq!(get("/r1").await, Ok("/done".to_string()), "three redirects are followed");
    assert_eq!(get("/r0").await, Err("too_many_redirects"), "a fourth is not");
}

#[tokio::test]
async fn too_slow_too_large_and_the_wrong_type_are_refused() {
    let big = vec![b'a'; 6 * 1024 * 1024];
    let at = serve(move |p| match p {
        "/slow" => Answer { delay: Duration::from_secs(3), ..ok("text/html", "late") },
        "/big" => ok("text/html", big.clone()),
        "/bin" => ok("application/octet-stream", vec![0u8; 10]),
        "/500" => Answer { status: 500, ..ok("text/plain", "boom") },
        _ => ok("text/html", "x"),
    })
    .await;
    let remote = Remote::allowing(vec![at]);
    let get = |p: &str, limits: Limits| {
        let (remote, url) = (remote.clone(), format!("http://{at}{p}"));
        async move { remote.get(&url, &limits).await.map(|_| ()) }
    };
    assert_eq!(get("/slow", short()).await, Err(Refusal::Timeout));
    assert_eq!(get("/big", Limits { timeout: Duration::from_secs(5), ..Limits::page() }).await, Err(Refusal::TooLarge { max: 5 * 1024 * 1024 }), "5 MiB at most");
    assert_eq!(get("/bin", short()).await, Err(Refusal::BadType("application/octet-stream".into())), "a page is what an unfurl reads");
    assert_eq!(get("/500", short()).await, Err(Refusal::Status(500)));
    // In band, on the wire's words.
    let (_dir, s) = store();
    let ev = unfurl::unfurl(&remote, &s, &format!("http://{at}/bin")).await;
    assert!(matches!(&ev, MediaEvent::Refused { reason, .. } if reason == "bad_type"), "{ev:?}");
    let ev = unfurl::unfurl(&Remote::new(), &s, "http://169.254.169.254/").await;
    assert!(matches!(&ev, MediaEvent::Refused { reason, message, .. } if reason == "blocked_address" && message.contains("169.254.169.254")), "{ev:?}");
}
