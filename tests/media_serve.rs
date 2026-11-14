//! V2-24: `GET /media/<hash>` over HTTP — a spawned `menger` with its shell,
//! under the fixture operator, its own media file (`--media-db-path`). The
//! route is behind a session (as the ui routes are: no session, the sign-in
//! page) and asks the media service `media.get` as that session: the bytes
//! with their type, kept forever (content-addressed); 404 for a hash that
//! names nothing or an image the session may not see, one answer. The media
//! file is the service's own: the tickets file holds no media table.
//!
//! Each server on ports the OS hands out.

mod support;

use serde_json::json;
use support::{call, connect, free_port, spawn_menger, ui_args};

/// A key that holds viewer on a shortcode of its own, and nothing on KILN.
const ELSEWHERE: [u8; 32] = [0xe1; 32];

/// A real 2×1 PNG (signature, IHDR, IDAT, IEND).
const PNG_2X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII=";

struct Served {
    _server: support::Spawned,
    dir: tempfile::TempDir,
    ws: u16,
    shell: u16,
    http: reqwest::Client,
}

impl Served {
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.shell)
    }

    /// The shell's session cookie carrying `token`.
    fn jar(&self, token: &str) -> String {
        format!("{}={token}", menger::login_names().session_cookie_for(&format!("http://127.0.0.1:{}", self.shell)))
    }

    async fn get(&self, path: &str, token: Option<&str>) -> reqwest::Response {
        let mut r = self.http.get(self.url(path));
        if let Some(t) = token {
            r = r.header("cookie", self.jar(t));
        }
        r.send().await.unwrap()
    }
}

async fn start() -> Served {
    let (ws, shell) = (free_port().await, free_port().await);
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let empty = dir.path().join("no-dist");
    std::fs::create_dir_all(&empty).unwrap();
    let mut extra = vec!["--shell-port".to_string(), shell.to_string(), "--app-dist".into(), empty.display().to_string()];
    extra.extend(ui_args(&dir.path().join("ui.db")));
    extra.extend(["--media-db-path".to_string(), dir.path().join("images/media.db").display().to_string()]);
    extra.extend(["--media-blob-dir".to_string(), dir.path().join("blobs").display().to_string()]);
    let server = spawn_menger(ws, &dir.path().join("t.db"), &extra).await;
    for _ in 0..100 {
        if tokio::net::TcpStream::connect(("127.0.0.1", shell)).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Served { _server: server, dir, ws, shell, http }
}

fn header(r: &reqwest::Response, name: &str) -> Option<String> {
    r.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}

#[tokio::test]
async fn an_image_is_served_to_a_session_that_sees_its_ticket_and_kept_forever() {
    let m = start().await;
    let admin = connect(m.ws).await;
    assert_eq!(call(&admin, "tickets.create", json!({"epic": "KILN", "title": "kiln", "kind": "scope"})).await[0]["type"], "created");
    let stored = call(&admin, "media.put", json!({"ticket": "KILN-1", "mime": "image/png", "data": PNG_2X1})).await;
    assert_eq!(stored[0]["type"], "stored", "{stored:?}");
    let hash = stored[0]["hash"].as_str().unwrap().to_string();
    let token = support::mint_session(m.ws).await;

    let r = m.get(&format!("/media/{hash}"), Some(&token)).await;
    assert_eq!(r.status(), 200);
    assert_eq!(header(&r, "content-type").as_deref(), Some("image/png"));
    assert_eq!(header(&r, "cache-control").as_deref(), Some("public, max-age=31536000, immutable"), "content-addressed: kept forever");
    assert_eq!(header(&r, "x-content-type-options").as_deref(), Some("nosniff"));
    use base64::Engine;
    assert_eq!(r.bytes().await.unwrap().to_vec(), base64::engine::general_purpose::STANDARD.decode(PNG_2X1).unwrap(), "the bytes as put");
    // Any case, as `media.get` reads a hash.
    assert_eq!(m.get(&format!("/media/{}", hash.to_uppercase()), Some(&token)).await.status(), 200);

    // No session: the sign-in page, as the ui routes answer.
    let anonymous = m.get(&format!("/media/{hash}"), None).await;
    assert_eq!(anonymous.status(), 303, "no session: the login page");
    assert!(header(&anonymous, "location").unwrap_or_default().starts_with("/login?next="));

    // None, and not a hash: 404, never kept.
    let none = m.get(&format!("/media/{}", "0".repeat(64)), Some(&token)).await;
    assert_eq!((none.status().as_u16(), header(&none, "cache-control").as_deref()), (404, Some("no-store")));
    assert_eq!(m.get("/media/abc", Some(&token)).await.status(), 404);

    // A session that does not see KILN-1: not found, as for none.
    assert_eq!(call(&admin, "tickets.create", json!({"epic": "ELSE", "title": "else", "kind": "scope"})).await[0]["type"], "created");
    let g = call(&admin, "auth.grant", json!({ "principal": support::principal_of(&ELSEWHERE), "role": "viewer", "scope": "ELSE" })).await;
    assert!(g.first().is_some_and(|g| g.get("error_kind").is_none()), "{g:?}");
    let theirs = support::mint_session_for(&ELSEWHERE, m.ws).await;
    assert_eq!(m.get(&format!("/media/{hash}"), Some(&theirs)).await.status(), 404, "KILN-1's image, to ELSE's viewer: not found");

    // The file is the service's own, where the flag said; the tickets file holds no media.
    assert!(m.dir.path().join("images/media.db").is_file(), "--media-db-path, its directory made");
    let tickets = rusqlite::Connection::open(m.dir.path().join("t.db")).unwrap();
    let n: i64 = tickets.query_row("SELECT count(*) FROM sqlite_master WHERE name IN ('media', 'media_owners')", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 0, "the tickets file holds no media table");
    let media = rusqlite::Connection::open(m.dir.path().join("images/media.db")).unwrap();
    let n: i64 = media.query_row("SELECT count(*) FROM media", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1, "the one image, in the media file");
}

/// EDITOR-10: each kind served as it is drawn — a video by range (so it
/// seeks), a PDF inline and framable by this origin, an SVG or a page as an
/// attachment of no active type, never sniffed; the bytes of a chunked
/// upload on the disk under `--media-blob-dir`, not in the media file.
#[tokio::test]
async fn each_kind_is_served_as_it_is_drawn_and_a_video_seeks() {
    use base64::Engine;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let m = start().await;
    let admin = connect(m.ws).await;
    let token = support::mint_session(m.ws).await;
    let mut video = [&[0, 0, 0, 0x20][..], b"ftypisom"].concat();
    video.extend((0..300_000).map(|i| (i % 251) as u8));
    let begun = call(&admin, "media.begin", json!({"name": "clip.mp4", "size": video.len()})).await;
    let upload = begun[0]["upload"].as_str().unwrap().to_string();
    let c = call(&admin, "media.chunk", json!({"upload": upload, "offset": 0, "data": b64(&video)})).await;
    assert_eq!(c[0]["type"], "chunked", "{c:?}");
    let stored = call(&admin, "media.finish", json!({"upload": upload})).await;
    let hash = stored[0]["hash"].as_str().unwrap().to_string();
    assert!(m.dir.path().join("blobs").join(&hash).is_file(), "the bytes under --media-blob-dir");

    let r = m.http.get(m.url(&format!("/media/{hash}"))).header("cookie", m.jar(&token)).header("range", "bytes=100-199").send().await.unwrap();
    assert_eq!(r.status(), 206);
    assert_eq!(header(&r, "content-range").as_deref(), Some(format!("bytes 100-199/{}", video.len()).as_str()));
    assert_eq!(header(&r, "accept-ranges").as_deref(), Some("bytes"));
    assert_eq!(header(&r, "content-type").as_deref(), Some("video/mp4"));
    assert_eq!(header(&r, "content-disposition").as_deref(), Some("inline; filename=\"clip.mp4\"; filename*=UTF-8''clip.mp4"));
    assert_eq!(r.bytes().await.unwrap().to_vec(), video[100..200].to_vec());
    let past = m.http.get(m.url(&format!("/media/{hash}"))).header("cookie", m.jar(&token)).header("range", format!("bytes={}-", video.len())).send().await.unwrap();
    assert_eq!(past.status(), 416);
    let whole = m.get(&format!("/media/{hash}"), Some(&token)).await;
    assert_eq!((whole.status().as_u16(), header(&whole, "content-length")), (200, Some(video.len().to_string())));
    assert_eq!(whole.bytes().await.unwrap().len(), video.len());

    let pdf = call(&admin, "media.put", json!({"data": b64(b"%PDF-1.7 x"), "name": "deck.pdf"})).await;
    let r = m.get(&format!("/media/{}", pdf[0]["hash"].as_str().unwrap()), Some(&token)).await;
    assert_eq!(header(&r, "content-type").as_deref(), Some("application/pdf"));
    assert_eq!(header(&r, "x-frame-options").as_deref(), Some("SAMEORIGIN"), "the editor's viewer frames it");
    assert_eq!(header(&r, "content-security-policy").as_deref(), Some("frame-ancestors 'self'"));

    for (name, bytes) in [("x.svg", &br#"<svg xmlns="http://www.w3.org/2000/svg" onload="alert(1)"/>"#[..]), ("page.html", &b"<!doctype html><script>alert(1)</script>"[..])] {
        let put = call(&admin, "media.put", json!({"data": b64(bytes), "name": name})).await;
        let r = m.get(&format!("/media/{}", put[0]["hash"].as_str().unwrap()), Some(&token)).await;
        assert_eq!(header(&r, "content-type").as_deref(), Some("application/octet-stream"), "{name}: never as itself");
        assert!(header(&r, "content-disposition").unwrap_or_default().starts_with("attachment;"), "{name}");
        assert_eq!(header(&r, "x-content-type-options").as_deref(), Some("nosniff"));
        assert!(header(&r, "content-security-policy").unwrap_or_default().ends_with("sandbox"), "{name}");
    }
}

/// SCRATCH-NAMES: a server on `menger-x.db` names every file of its own
/// after it — `menger-x.media.db`, `.docs.db`, `.quota.db`, its blobs in
/// `menger-x.media.blobs/` — so two such servers in one directory share
/// nothing and `scripts/cleanup-gate.sh x` takes them all; a flag that names
/// a file still wins (`--ui-db-path` here, `--media-db-path` above).
#[tokio::test]
async fn a_scratch_server_names_its_files_after_its_database() {
    let dir = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).unwrap();
    let ui = dir.path().join("named-ui.db");
    let _server = spawn_menger(free_port().await, &dir.path().join("menger-x.db"), &ui_args(&ui)).await;
    for name in ["menger-x.media.db", "menger-x.docs.db", "menger-x.quota.db"] {
        assert!(dir.path().join(name).is_file(), "{name} beside menger-x.db");
    }
    assert!(dir.path().join("menger-x.media.blobs").is_dir(), "the blobs named after it too");
    assert!(ui.is_file(), "--ui-db-path wins");
    for plain in ["media.db", "docs.db", "quota.db", "ui.db", "menger-x.ui.db", "media"] {
        assert!(!dir.path().join(plain).exists(), "{plain}: no file shared with another server, none the flag replaced");
    }
}
