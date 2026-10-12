//! The app's build as a table (menger's ACCESS-33), from disk or embedded.

use crate::EmbeddedTable;
use axum::body::Bytes;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

/// Where the app's build is served from.
#[derive(Clone, PartialEq, Eq)]
pub enum AppSource {
    /// A build output directory on disk, read into the table and read again
    /// when its entrypoint (`index.html`) changes.
    Dir(PathBuf),
    /// A build compiled into the binary ([`crate::embedded_app!`]): read
    /// once, since a binary's build never changes. With no entrypoint in it
    /// it is "not built", as an empty directory is.
    Embedded(EmbeddedTable),
}

impl AppSource {
    /// `table` when it holds a build ([`has_entrypoint`]), else the
    /// directory — what a binary that may or may not carry its build serves
    /// when none is named.
    pub fn embedded_or(table: EmbeddedTable, dir: impl Into<PathBuf>) -> Self {
        if has_entrypoint(table) {
            Self::Embedded(table)
        } else {
            Self::Dir(dir.into())
        }
    }
}

impl std::fmt::Debug for AppSource {
    /// An embedded table by its size, never its bytes.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dir(dir) => f.debug_tuple("Dir").field(dir).finish(),
            Self::Embedded(table) => write!(f, "Embedded({} files)", table.len()),
        }
    }
}

/// Whether an embedded table holds a build: its entrypoint, `index.html`.
pub fn has_entrypoint(table: &[(&str, &[u8])]) -> bool {
    table.iter().any(|(key, _)| *key == "index.html")
}

/// Every `(placeholder, value)` replaced in `text`, in order.
pub fn stamp(text: &str, stamps: &[(String, String)]) -> String {
    stamps.iter().fold(text.to_string(), |text, (placeholder, value)| text.replace(placeholder.as_str(), value))
}

/// A served file's MIME type, by its key's extension.
pub fn mime_of(key: &str) -> &'static str {
    match key.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        Some("json") | Some("map") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("png") => "image/png",
        Some("woff2") => "font/woff2",
        // TypeScript declarations (menger's COMP-10 SDK) shown as text.
        Some("txt") | Some("ts") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// One file of the app's build, as served.
pub(crate) struct Asset {
    pub(crate) bytes: Bytes,
    pub(crate) mime: &'static str,
}

impl Asset {
    /// A build's file as served: the entrypoint stamped, everything else
    /// byte-exact; its MIME type from its key.
    fn of(key: &str, data: Vec<u8>, stamps: &[(String, String)]) -> Self {
        let data = if key == "index.html" {
            match String::from_utf8(data) {
                Ok(text) => stamp(&text, stamps).into_bytes(),
                Err(e) => e.into_bytes(),
            }
        } else {
            data
        };
        Asset { bytes: Bytes::from(data), mime: mime_of(key) }
    }
}

/// The app's build as a table — the only files the app's routes can
/// answer. A request path is a key into it, never a path on disk: what is
/// not in the table is 404 whatever the filesystem holds, so there is no
/// path to sanitise. Read whole (regular files only — a symlink is not
/// followed), with the entrypoint stamped.
#[derive(Default)]
pub(crate) struct AppFiles {
    /// The entrypoint's modification time when this table was read —
    /// `None` when there was no build.
    pub(crate) built: Option<SystemTime>,
    pub(crate) files: HashMap<String, Asset>,
}

/// A table bigger than this is not an app build (a mistaken directory):
/// refused whole rather than read into memory.
const MAX_FILES: usize = 4096;
const MAX_BYTES: u64 = 256 << 20;

impl AppFiles {
    /// Reads `dist` (blocking). A dist without an entrypoint is no build,
    /// and is not walked; an oversized one is refused whole, logged. Both
    /// are an empty table — "not built".
    pub(crate) fn load(dist: &Path, stamps: &[(String, String)]) -> Self {
        let built = std::fs::metadata(dist.join("index.html")).and_then(|m| m.modified()).ok();
        if built.is_none() {
            return Self::default();
        }
        let mut files = HashMap::new();
        let mut bytes = 0u64;
        let mut dirs = vec![(dist.to_path_buf(), String::new())];
        while let Some((dir, prefix)) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let (Ok(kind), Some(name)) = (entry.file_type(), entry.file_name().to_str().map(str::to_string)) else {
                    continue;
                };
                let key = format!("{prefix}{name}");
                if kind.is_dir() {
                    dirs.push((entry.path(), format!("{key}/")));
                } else if kind.is_file() {
                    bytes += entry.metadata().map(|m| m.len()).unwrap_or(0);
                    if files.len() >= MAX_FILES || bytes > MAX_BYTES {
                        tracing::error!("{dist:?} is not an app build (over {MAX_FILES} files or {MAX_BYTES} bytes): serving none of it");
                        return Self::default();
                    }
                    let Ok(data) = std::fs::read(entry.path()) else { continue };
                    files.insert(key.clone(), Asset::of(&key, data, stamps));
                }
            }
        }
        Self { built, files }
    }

    /// The table from a build compiled into the binary — the same keys,
    /// stamping and MIME types as [`Self::load`]. Without an entrypoint it
    /// is no build, as on disk. Its `built` is a fixed time: an embedded
    /// build never changes.
    pub(crate) fn embedded(table: &[(&str, &[u8])], stamps: &[(String, String)]) -> Self {
        if !has_entrypoint(table) {
            return Self::default();
        }
        let files = table.iter().map(|(key, data)| (key.to_string(), Asset::of(key, data.to_vec(), stamps))).collect();
        Self { built: Some(SystemTime::UNIX_EPOCH), files }
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Asset> {
        self.files.get(key)
    }
}

/// The app's table, and when to read it again. A release's build never
/// changes, so the table is read once; in a checkout a watching frontend
/// build rewrites the directory, and a new entrypoint (its modification
/// time — one fixed file, never a request path) has the table read again,
/// so a reload picks up a rebuild without a restart. An embedded build is
/// read once and never again.
pub(crate) struct App {
    pub(crate) source: AppSource,
    stamps: Arc<Vec<(String, String)>>,
    files: RwLock<Arc<AppFiles>>,
}

impl App {
    pub(crate) async fn new(source: AppSource, stamps: Vec<(String, String)>) -> Self {
        let app = Self { source, stamps: Arc::new(stamps), files: RwLock::default() };
        app.reload().await;
        app
    }

    pub(crate) fn table(&self) -> Arc<AppFiles> {
        self.files.read().expect("app table lock poisoned").clone()
    }

    async fn reload(&self) -> Arc<AppFiles> {
        let stamps = self.stamps.clone();
        let fresh = Arc::new(match &self.source {
            AppSource::Dir(dist) => {
                let dist = dist.clone();
                tokio::task::spawn_blocking(move || AppFiles::load(&dist, &stamps)).await.unwrap_or_default()
            }
            AppSource::Embedded(table) => AppFiles::embedded(table, &stamps),
        });
        tracing::debug!("app build read from {:?}: {} files", self.source, fresh.files.len());
        *self.files.write().expect("app table lock poisoned") = fresh.clone();
        fresh
    }

    /// The table, read again first if the build's entrypoint changed.
    pub(crate) async fn current(&self) -> Arc<AppFiles> {
        let table = self.table();
        let AppSource::Dir(dist) = &self.source else {
            return table;
        };
        let built = tokio::fs::metadata(dist.join("index.html")).await.and_then(|m| m.modified()).ok();
        if built == table.built {
            table
        } else {
            self.reload().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamps(ws: u16, title: &str) -> Vec<(String, String)> {
        vec![("{{WS_PORT}}".into(), ws.to_string()), ("{{TITLE}}".into(), title.into())]
    }

    /// The table holds the build's regular files by their '/'-joined
    /// paths, the entrypoint stamped — and nothing a request path could
    /// reach beyond them.
    #[test]
    fn the_app_table_holds_the_build_and_nothing_else() {
        let root = tempfile::tempdir().unwrap();
        let dist = root.path().join("dist");
        std::fs::create_dir_all(dist.join("vendor")).unwrap();
        std::fs::write(dist.join("index.html"), "<title>{{TITLE}}</title>{{WS_PORT}}").unwrap();
        std::fs::write(dist.join("app.js"), "js").unwrap();
        std::fs::write(dist.join("vendor/mermaid.min.js"), "m").unwrap();
        std::fs::write(root.path().join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(root.path().join("secret.txt"), dist.join("link.txt")).unwrap();

        let t = AppFiles::load(&dist, &stamps(4450, "app.test"));
        let mut keys: Vec<_> = t.files.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["app.js", "index.html", "vendor/mermaid.min.js"], "a symlink is not followed");
        assert_eq!(&t.get("index.html").unwrap().bytes[..], b"<title>app.test</title>4450");
        assert_eq!(t.get("vendor/mermaid.min.js").unwrap().mime, "text/javascript");
        assert!(t.built.is_some());
        for miss in ["/etc/hostname", "../secret.txt", "secret.txt", "link.txt", "vendor", "./app.js", ""] {
            assert!(t.get(miss).is_none(), "{miss:?}");
        }
        let none = AppFiles::load(&root.path().join("missing"), &[]);
        assert!(none.files.is_empty() && none.built.is_none());
        std::fs::remove_file(dist.join("index.html")).unwrap();
        let headless = AppFiles::load(&dist, &[]);
        assert!(headless.files.is_empty(), "no entrypoint, no build: nothing under it is served");
    }

    /// A rebuilt entrypoint has the table read again; an asset dropped into
    /// dist without a rebuild is not.
    #[tokio::test]
    async fn the_table_follows_a_rebuild_and_nothing_less() {
        let dist = tempfile::tempdir().unwrap();
        std::fs::write(dist.path().join("index.html"), "v1").unwrap();
        let app = App::new(AppSource::Dir(dist.path().to_path_buf()), Vec::new()).await;
        std::fs::write(dist.path().join("late.js"), "x").unwrap();
        assert!(app.current().await.get("late.js").is_none(), "no rebuild, no new file");
        let index = std::fs::File::options().write(true).open(dist.path().join("index.html")).unwrap();
        index.set_modified(SystemTime::now() + std::time::Duration::from_secs(60)).unwrap();
        let t = app.current().await;
        assert!(t.get("late.js").is_some(), "a rebuild is read whole");
    }

    /// An embedded build is the same table a directory gives — the
    /// entrypoint stamped, MIME types by key — and one with no entrypoint
    /// is no build.
    #[tokio::test]
    async fn an_embedded_build_is_the_same_table() {
        static TABLE: EmbeddedTable =
            &[("index.html", b"<title>{{TITLE}}</title>{{WS_PORT}}"), ("app.wasm", b"\0asm"), ("vendor/m.js", b"m")];
        let t = AppFiles::embedded(TABLE, &stamps(4471, "app"));
        let mut keys: Vec<_> = t.files.keys().cloned().collect();
        keys.sort();
        assert_eq!(keys, ["app.wasm", "index.html", "vendor/m.js"]);
        assert_eq!(&t.get("index.html").unwrap().bytes[..], b"<title>app</title>4471");
        assert_eq!(t.get("app.wasm").unwrap().mime, "application/wasm");
        assert!(t.get("../index.html").is_none());
        static HEADLESS: EmbeddedTable = &[("app.js", b"x")];
        let headless = AppFiles::embedded(HEADLESS, &[]);
        assert!(headless.files.is_empty() && headless.built.is_none());
        assert_eq!(AppSource::embedded_or(TABLE, "/nowhere"), AppSource::Embedded(TABLE));
        assert_eq!(AppSource::embedded_or(HEADLESS, "/nowhere"), AppSource::Dir("/nowhere".into()));
        assert_eq!(format!("{:?}", AppSource::Embedded(TABLE)), "Embedded(3 files)");
        let app = App::new(AppSource::Embedded(&[]), Vec::new()).await;
        assert!(app.current().await.get("index.html").is_none(), "nothing embedded is not built");
    }

    #[test]
    fn a_page_is_stamped_with_every_placeholder() {
        let page = "<title>{{TITLE}}</title><meta content=\"{{WS_PORT}}\">{{X}}";
        let mut s = stamps(4450, "menger.test");
        s.push(("{{X}}".into(), "x".into()));
        assert_eq!(stamp(page, &s), "<title>menger.test</title><meta content=\"4450\">x");
    }
}
