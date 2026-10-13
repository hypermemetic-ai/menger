//! MENGER-22: code is always highlighted — one vendored highlighter,
//! coloured by the theme.
//!
//! The highlighter is highlight.js, vendored at `frontend/vendor/highlight/`
//! and served by menger at [`ASSET_URL`] — never a CDN: the app talks to
//! nothing but menger (src/mermaid.rs's rule, and its pattern). Nothing
//! references it at page load. The first time a page holds a code block —
//! a `<pre><code>` that is not a mermaid placeholder's — [`mount`] injects a
//! `<script>` for it; concurrent callers share the one load, and a page
//! without code never fetches it. A frame does the same with the URL its
//! init block carries ([`init_json`]; frontend/sdk/menger-sdk.js).
//!
//! **What a block asks for** ([`ask`]) is read from its `<code>`'s class:
//! `language-x` (or `lang-x`) is `x` — highlight.js's names and aliases, so
//! `toml`, `html`, `ts`, `sh` all resolve — `text`, `plain` or `plaintext`
//! is plain, and a fence with no language is [`UNLABELED`]'s to decide
//! (MENGER-22's open decision `unlabeled`: guess it from [`LANGUAGES`], or
//! leave it plain — one constant). A component's definition block
//! (` ```component `) reads as HTML.
//!
//! **The colours are classes.** highlight.js wraps tokens in `hljs-*`
//! spans; index.html (and the SDK's base, in a frame) colour those from
//! the `--code-*` tokens — THEME-8 theme tokens like any other — with no
//! vendor stylesheet. So a theme, a region's scope or the light/dark
//! toggle recolours code with no parse again.
//!
//! **Failure stays readable.** A language highlight.js does not know, a
//! bundle that fails to load, a highlighter that throws: the block keeps
//! its text, plain, and nothing is thrown. Each block is claimed
//! (`data-hl`) before its async pass, so a second pass over the same
//! subtree never highlights it twice; a body that changes is rendered
//! anew (a new subtree), and highlighted when it lands — a half-written
//! body still streaming is highlighted as it settles.

use js_sys::{Array, Function, Object, Promise, Reflect};
use std::cell::RefCell;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Element, HtmlElement, Node};

/// Where the shell serves the vendored bundle: `frontend/index.html`
/// copy-files `vendor/highlight/<this file>` into `dist/vendor/`.
pub const ASSET_URL: &str = "/app/vendor/highlight-11.11.1.min.js";

/// The house's languages — every one is in the bundle (pinned by a native
/// test against its registered names), and an unlabeled fence is guessed
/// from these alone.
pub const LANGUAGES: [&str; 14] =
    ["rust", "typescript", "javascript", "json", "toml", "yaml", "bash", "sql", "html", "css", "markdown", "haskell", "python", "diff"];

/// The words that opt a fence out: it stays plain.
pub const PLAIN: [&str; 3] = ["text", "plain", "plaintext"];

/// Names a block may carry that highlight.js knows by another: a
/// component's definition is HTML.
pub const ALIASES: [(&str, &str); 1] = [("component", "html")];

/// MENGER-22's open decision `unlabeled` — a fence with no language: guess
/// it from [`LANGUAGES`] ([`Unlabeled::Detect`], the recommendation and the
/// owner's "always"), or leave it plain ([`Unlabeled::Plain`]). The app and
/// every frame read this one constant: the answer is this line.
pub const UNLABELED: Unlabeled = Unlabeled::Detect;

/// A guess below this relevance (highlight.js's score) stays plain: prose
/// in a fence is not a language. Measured on 11.11.1 against
/// [`LANGUAGES`]: a sentence scores 1 (stays plain); `pub enum Status {…}`
/// and a two-line YAML map score 2, a Rust signature 5 (coloured).
pub const DETECT_MIN_RELEVANCE: u32 = 2;

/// What an unlabeled fence does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unlabeled {
    Detect,
    /// The decision's other answer: [`UNLABELED`] names one or the other.
    #[allow(dead_code)]
    Plain,
}

impl Unlabeled {
    pub const fn as_str(self) -> &'static str {
        match self {
            Unlabeled::Detect => "detect",
            Unlabeled::Plain => "plain",
        }
    }
}

/// What one block asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ask {
    /// This language (highlight.js's name or alias); plain if it knows none.
    Language(String),
    /// Guess from [`LANGUAGES`].
    Detect,
    /// Stay as written.
    Plain,
}

/// What a `<code>` whose class list is `class` asks for.
pub fn ask(class: Option<&str>) -> Ask {
    let lang = class.into_iter().flat_map(str::split_whitespace).find_map(|c| c.strip_prefix("language-").or_else(|| c.strip_prefix("lang-")));
    match lang.map(str::to_ascii_lowercase) {
        Some(l) if PLAIN.contains(&l.as_str()) => Ask::Plain,
        Some(l) if l.is_empty() => Ask::Plain,
        Some(l) => Ask::Language(ALIASES.iter().find(|(a, _)| *a == l).map_or(l, |(_, to)| to.to_string())),
        None => match UNLABELED {
            Unlabeled::Detect => Ask::Detect,
            Unlabeled::Plain => Ask::Plain,
        },
    }
}

/// The frame's init block half (`highlight` beside `mermaid`): the bundle's
/// absolute URL, what an unlabeled fence does, the languages it is guessed
/// from, the opt-outs and the aliases — the rules [`ask`] keeps, said once
/// for the SDK.
pub fn init_json(url: &str) -> serde_json::Value {
    serde_json::json!({
        "url": url,
        "unlabeled": UNLABELED.as_str(),
        "detect": LANGUAGES,
        "plain": PLAIN,
        "aliases": ALIASES.iter().map(|(a, b)| (a.to_string(), serde_json::Value::from(*b))).collect::<serde_json::Map<_, _>>(),
        "min_relevance": DETECT_MIN_RELEVANCE,
    })
}

/// The selector for the blocks a pass takes: code in a `<pre>`, not a
/// mermaid placeholder's source, not claimed yet.
const BLOCKS: &str = "pre > code:not([data-hl])";

thread_local! {
    /// The one in-flight script load, shared until it settles; cleared on
    /// failure so a later block can retry.
    static LOADING: RefCell<Option<Promise>> = const { RefCell::new(None) };
}

/// Highlight every unclaimed block under `root`. Idempotent, and a subtree
/// with no code fetches nothing. Nothing here can throw into the caller.
pub fn mount(root: &Element) {
    let Ok(nodes) = root.query_selector_all(BLOCKS) else { return };
    let mut blocks = Vec::new();
    for i in 0..nodes.length() {
        let Some(el) = nodes.get(i).and_then(|n| n.dyn_into::<Element>().ok()) else { continue };
        if el.closest(".mermaid").ok().flatten().is_some() {
            continue;
        }
        let asked = ask(el.get_attribute("class").as_deref());
        if asked == Ask::Plain {
            let _ = el.set_attribute("data-hl", "plain");
            continue;
        }
        let _ = el.set_attribute("data-hl", "pending");
        blocks.push((el, asked));
    }
    if blocks.is_empty() {
        return;
    }
    spawn_local(async move {
        let hljs = ensure_loaded().await;
        for (el, asked) in blocks {
            let done = match &hljs {
                Ok(h) => paint(h, &el, &asked).unwrap_or_else(|_| "plain".to_string()),
                Err(_) => "plain".to_string(),
            };
            let _ = el.set_attribute("data-hl", &done);
        }
    });
}

/// One block: its text through highlight.js, the spans in place of the
/// text, `hljs` on its class. Returns what it was painted as.
fn paint(hljs: &Object, el: &Element, asked: &Ask) -> Result<String, String> {
    let text = el.text_content().unwrap_or_default();
    let opts = Object::new();
    let out = match asked {
        Ask::Plain => return Ok("plain".into()),
        Ask::Language(lang) => {
            let known = method(hljs, "getLanguage")?.call1(hljs, &lang.into()).map_err(|_| "getLanguage threw".to_string())?;
            if known.is_undefined() || known.is_null() {
                return Ok("plain".into());
            }
            let _ = Reflect::set(&opts, &"language".into(), &lang.into());
            let _ = Reflect::set(&opts, &"ignoreIllegals".into(), &JsValue::TRUE);
            method(hljs, "highlight")?.call2(hljs, &text.into(), &opts).map_err(|_| "highlight threw".to_string())?
        }
        Ask::Detect => {
            let subset: Array = LANGUAGES.iter().map(|l| JsValue::from_str(l)).collect();
            let out = method(hljs, "highlightAuto")?.call2(hljs, &text.into(), &subset).map_err(|_| "highlightAuto threw".to_string())?;
            let relevance = Reflect::get(&out, &"relevance".into()).ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
            if relevance < f64::from(DETECT_MIN_RELEVANCE) {
                return Ok("plain".into());
            }
            out
        }
    };
    let html = Reflect::get(&out, &"value".into()).ok().and_then(|v| v.as_string()).ok_or("no value")?;
    let lang = Reflect::get(&out, &"language".into()).ok().and_then(|v| v.as_string()).unwrap_or_default();
    el.set_inner_html(&html);
    let _ = el.class_list().add_1("hljs");
    Ok(if lang.is_empty() { "on".into() } else { lang })
}

/// The `hljs` global once the bundle has run; `None` until then.
fn hljs_global() -> Option<Object> {
    let v = Reflect::get(crate::dom::window().as_ref(), &"hljs".into()).ok()?;
    v.is_object().then(|| v.unchecked_into())
}

/// The highlighter, fetching the bundle on first need. Callers arriving
/// while a load is in flight await the same promise.
async fn ensure_loaded() -> Result<Object, String> {
    if let Some(h) = hljs_global() {
        return Ok(h);
    }
    let promise = LOADING.with(|l| l.borrow_mut().get_or_insert_with(inject_script).clone());
    match JsFuture::from(promise).await {
        Ok(_) => hljs_global().ok_or_else(|| format!("{ASSET_URL} loaded but defined no `hljs` global")),
        Err(_) => {
            LOADING.with(|l| *l.borrow_mut() = None);
            Err(format!("could not load the highlighter from {ASSET_URL}"))
        }
    }
}

/// Append `<script src=ASSET_URL>` and settle when it has run (or failed)
/// — src/mermaid.rs's way.
fn inject_script() -> Promise {
    Promise::new(&mut |resolve, reject| {
        let Some(doc) = crate::dom::window().document() else {
            let _ = reject.call1(&JsValue::NULL, &"no document".into());
            return;
        };
        let Some(script) = doc.create_element("script").ok().and_then(|e| e.dyn_into::<HtmlElement>().ok()) else {
            let _ = reject.call1(&JsValue::NULL, &"cannot create a script element".into());
            return;
        };
        let _ = script.set_attribute("src", ASSET_URL);
        script.set_onload(Some(&resolve));
        script.set_onerror(Some(&reject));
        let parent: Option<Node> = doc.head().map(Node::from).or_else(|| doc.body().map(Node::from));
        match parent {
            Some(p) if p.append_child(&script).is_ok() => {}
            _ => {
                let _ = reject.call1(&JsValue::NULL, &"cannot attach the script element".into());
            }
        }
    })
}

fn method(obj: &Object, name: &str) -> Result<Function, String> {
    Reflect::get(obj, &name.into())
        .ok()
        .and_then(|f| f.dyn_into::<Function>().ok())
        .ok_or_else(|| format!("hljs.{name} is not a function"))
}

/// A comment's text in pieces: prose, and the fenced blocks in it (a line
/// that opens with three backticks or tildes, its info string's first word
/// the language, to the line that closes it — or to the end, still being
/// written).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Piece {
    Text(String),
    Code { lang: Option<String>, code: String },
}

pub fn pieces(text: &str) -> Vec<Piece> {
    let mut out = Vec::new();
    let mut prose = String::new();
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        let t = line.trim_start();
        let fence = ["```", "~~~"].into_iter().find(|f| t.starts_with(f));
        let Some(fence) = fence else {
            prose.push_str(line);
            continue;
        };
        let info = t[fence.len()..].trim();
        let lang = info.split_whitespace().next().map(str::to_string);
        let mut code = String::new();
        for l in lines.by_ref() {
            if l.trim() == fence || (l.trim().starts_with(fence) && l.trim().trim_start_matches(fence.chars().next().unwrap()).is_empty()) {
                break;
            }
            code.push_str(l);
        }
        if !prose.is_empty() {
            out.push(Piece::Text(std::mem::take(&mut prose)));
        }
        out.push(Piece::Code { lang, code: code.trim_end_matches('\n').to_string() });
    }
    if !prose.is_empty() {
        out.push(Piece::Text(prose));
    }
    out
}

/// MENGER-22: a comment's text — its prose with its bare URLs as links
/// (src/links.rs), its fenced blocks as code, highlighted.
#[leptos::component]
pub(crate) fn CommentText(#[prop(into)] text: String) -> impl leptos::IntoView {
    use leptos::prelude::*;
    let node = NodeRef::<leptos::html::Div>::new();
    node.on_load(move |el: web_sys::HtmlDivElement| mount(&el));
    let parts = pieces(&text)
        .into_iter()
        .map(|p| match p {
            Piece::Text(t) => view! { <crate::links::Linkified text=t /> }.into_any(),
            Piece::Code { lang, code } => {
                let class = lang.map(|l| format!("language-{l}"));
                view! { <pre class="ccode"><code class=class>{code}</code></pre> }.into_any()
            }
        })
        .collect_view();
    view! { <div class="ctext" node_ref=node>{parts}</div> }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The three places that name the bundle agree — the URL the app
    /// fetches, the file vendored with its license and README, index.html's
    /// copy-file link — and it never loads with the page.
    #[test]
    fn vendored_bundle_named_by_asset_url_exists() {
        let name = ASSET_URL.rsplit('/').next().unwrap();
        assert!(ASSET_URL.starts_with("/app/vendor/"), "{ASSET_URL}");
        let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/highlight");
        let bundle = std::fs::read_to_string(vendor.join(name)).expect("the vendored bundle");
        assert!(bundle.len() > 100_000, "the whole IIFE bundle, not a stub");
        assert!(bundle.starts_with("/*!\n  Highlight.js v11.11.1"), "the release named");
        assert!(vendor.join("LICENSE").is_file() && std::fs::read_to_string(vendor.join("LICENSE")).unwrap().contains("BSD 3-Clause"));
        let version = name.strip_prefix("highlight-").and_then(|s| s.strip_suffix(".min.js")).unwrap();
        assert!(std::fs::read_to_string(vendor.join("README.md")).unwrap().contains(version));
        let index = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("index.html")).unwrap();
        assert!(index.contains(&format!("href=\"vendor/highlight/{name}\"")), "index.html copy-files the bundle into dist");
        for line in index.lines() {
            assert!(!(line.contains("<script") && line.contains("src=") && line.contains("highlight")), "the bundle must not load with the page: {line}");
        }
        // Every house language is in it: registered by name, or an alias of
        // one that is (toml is `ini`'s; html, `xml`'s).
        for lang in LANGUAGES {
            let registered = match lang {
                "toml" | "html" => bundle.contains(&format!("\"{lang}\"")),
                l => bundle.contains(&format!("grmr_{l}")) || bundle.contains(&format!("registerLanguage(\"{l}\"")),
            };
            assert!(registered, "{lang} is not in the bundle");
        }
    }

    /// MENGER-22 acceptance 3: no shipped asset, and nothing that renders a
    /// body, names a CDN host for the highlighter — nor holds one at all,
    /// for the bundle itself.
    #[test]
    fn no_cdn_host_serves_the_highlighter() {
        const HOSTS: [&str; 7] = ["cdnjs.cloudflare.com", "cdn.jsdelivr.net", "unpkg.com", "esm.sh", "cdn.skypack.dev", "fastly.jsdelivr.net", "highlightjs.org/static"];
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let bundle = std::fs::read_to_string(root.join("vendor/highlight").join(ASSET_URL.rsplit('/').next().unwrap())).unwrap();
        for h in HOSTS {
            assert!(!bundle.contains(h), "the bundle names {h}");
        }
        let mut files = vec![root.join("index.html"), root.join("sdk/menger-sdk.js"), root.join("sdk/menger-page.js")];
        files.extend(crate::tokens_lint::rust_files(&root.join("src")));
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            for (n, line) in text.lines().enumerate() {
                let lower = line.to_ascii_lowercase();
                if lower.contains("highlight") || lower.contains("hljs") {
                    for h in HOSTS {
                        // This test's own list is the one place that may.
                        if f.ends_with("highlight.rs") && line.contains("const HOSTS") {
                            continue;
                        }
                        assert!(!lower.contains(h), "{}:{}: names {h} for the highlighter: {line}", f.display(), n + 1);
                    }
                }
            }
        }
        assert_eq!(init_json("http://h:1/app/vendor/x.js")["url"], "http://h:1/app/vendor/x.js");
    }

    /// A block's class decides: its language kept, `text`/`plain` plain,
    /// none the open decision's answer.
    #[test]
    fn a_block_asks_by_its_class() {
        assert_eq!(ask(Some("language-rust")), Ask::Language("rust".into()));
        assert_eq!(ask(Some("hljs language-TS")), Ask::Language("ts".into()));
        assert_eq!(ask(Some("lang-toml")), Ask::Language("toml".into()));
        assert_eq!(ask(Some("language-component")), Ask::Language("html".into()));
        for plain in ["language-text", "language-plain", "language-plaintext"] {
            assert_eq!(ask(Some(plain)), Ask::Plain, "{plain}");
        }
        let unlabeled = match UNLABELED {
            Unlabeled::Detect => Ask::Detect,
            Unlabeled::Plain => Ask::Plain,
        };
        assert_eq!(ask(None), unlabeled);
        assert_eq!(ask(Some("x y")), unlabeled);
        assert_eq!(init_json("u")["unlabeled"], UNLABELED.as_str());
        assert_eq!(init_json("u")["detect"].as_array().unwrap().len(), LANGUAGES.len());
    }

    /// A comment's fences become code; its prose stays prose; a fence still
    /// being written runs to the end.
    #[test]
    fn a_comments_fences_are_code() {
        let p = pieces("look:\n```rust\nfn main() {}\n```\nand `x` then\n~~~\nplain\n~~~\n");
        assert_eq!(
            p,
            [
                Piece::Text("look:\n".into()),
                Piece::Code { lang: Some("rust".into()), code: "fn main() {}".into() },
                Piece::Text("and `x` then\n".into()),
                Piece::Code { lang: None, code: "plain".into() },
            ]
        );
        assert_eq!(pieces("no code here"), [Piece::Text("no code here".into())]);
        assert_eq!(pieces("```ts\nlet a = 1"), [Piece::Code { lang: Some("ts".into()), code: "let a = 1".into() }]);
    }
}
