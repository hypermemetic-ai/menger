//! AGS-14: ```mermaid fences become SVG — lazily, in strict mode, with
//! every failure caught and painted in place.
//!
//! The renderer is the mermaid JS library, vendored at
//! `frontend/vendor/mermaid/` and served by menger at [`ASSET_URL`]
//! (never a CDN: the app talks to nothing but menger). Nothing references
//! it at page load. The first time a rendered body actually holds a
//! placeholder (md.rs emits one per fence), [`mount`] injects a `<script>`
//! for it; concurrent callers share the one in-flight load, so a page with
//! several diagrams fetches the bundle once, and a page with none never
//! fetches it.
//!
//! Every render goes through `mermaid.initialize` with the config
//! [`config_json`] builds: `securityLevel: "strict"` (no click handlers,
//! scripts stripped from labels), `htmlLabels: false` everywhere (strict
//! mode alone keeps sanitized label HTML *live* — verified against the
//! 11.17.2 source and in the browser — so labels are SVG text and markup
//! in them is literal), a DOMPurify config forbidding anything that could
//! fetch or navigate, and those keys added to mermaid's `secure` list,
//! which its directive sanitizer applies at every nesting level — so no
//! `%%{init: …}%%` inside diagram source can loosen any of it. The theme
//! is chosen from the page's actual background color (dark palette →
//! mermaid's `dark`), so nothing here names a color — AGS-10's tokens
//! will drive it without edits.
//!
//! Failure is visible, never thrown: a parse error (or a bundle that
//! fails to load) becomes an inline message above the still-visible
//! source, and only that placeholder changes. A body streamed in
//! half-written (AGS-7) rides the same path.
//!
//! AGS-17: a drawn diagram is the one thing the theme tokens cannot
//! re-color, so the source stays on the placeholder (`data-src`) and
//! [`rerender_all`] draws every finished diagram again when the painted
//! mode changes — each lands in the mermaid theme matching the new
//! background. A drawing generation per placeholder (`data-gen`) makes
//! two quick toggles safe: only the newest draw may land.

use crate::md::MERMAID_CLASS;
use js_sys::{Function, Object, Promise, Reflect};
use std::cell::{Cell, RefCell};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::{spawn_local, JsFuture};
use web_sys::{Element, HtmlElement, Node};

/// Where the shell serves the vendored bundle: `frontend/index.html`
/// copy-files `vendor/mermaid/<this file>` into `dist/vendor/`. Absolute
/// like every trunk-emitted asset URL (public_url is `/app/`).
pub const ASSET_URL: &str = "/app/vendor/mermaid-11.17.2.min.js";

thread_local! {
    /// The one in-flight script load, shared by every caller until it
    /// settles; cleared on failure so a later diagram can retry.
    static LOADING: RefCell<Option<Promise>> = RefCell::new(None);
    /// `mermaid.render` wants a unique id per call (it is the temp
    /// element's id while drawing, and the SVG's id after).
    static NEXT_ID: Cell<u32> = Cell::new(0);
    /// AGS-17: the drawing generation — bumped per draw and stamped on
    /// the placeholder, so a draw that finishes after a newer one began
    /// on the same node leaves it alone.
    static NEXT_GEN: Cell<u32> = Cell::new(0);
}

/// Render every pending placeholder under `root`. Idempotent: a node is
/// claimed (`data-state="rendering"`) before its async render starts, so
/// a second pass over the same subtree never renders a diagram twice, and
/// a placeholder whose page was navigated away mid-render simply finishes
/// into a detached node. Nothing here can throw into the caller.
pub fn mount(root: &Element) {
    let selector = format!(".{MERMAID_CLASS}[data-state=\"pending\"]");
    let Ok(nodes) = root.query_selector_all(&selector) else {
        return;
    };
    for i in 0..nodes.length() {
        let Some(el) = nodes.get(i).and_then(|n| n.dyn_into::<Element>().ok()) else {
            continue;
        };
        // The placeholder's text IS the source: entities decoded by the
        // browser, exactly what md.rs escaped. It stays on the node for
        // a later redraw (AGS-17) once the SVG has replaced it.
        let src = el.text_content().unwrap_or_default();
        let _ = el.set_attribute("data-src", &src);
        let _ = el.set_attribute("data-state", "rendering");
        draw(el, src);
    }
}

/// AGS-17: draw every finished diagram in the document again, against
/// the background painted *now* — called when the effective theme
/// changes. The old SVG stays in place until the new one lands (the state
/// stays `done`: no blank, no layout jump); a diagram still pending or
/// mid-render is left to its own pass. Nothing here can throw.
pub fn rerender_all() {
    let Some(doc) = crate::dom::window().document() else {
        return;
    };
    let selector = format!(".{MERMAID_CLASS}[data-state=\"done\"][data-src]");
    let Ok(nodes) = doc.query_selector_all(&selector) else {
        return;
    };
    for i in 0..nodes.length() {
        let Some(el) = nodes.get(i).and_then(|n| n.dyn_into::<Element>().ok()) else {
            continue;
        };
        let Some(src) = el.get_attribute("data-src") else {
            continue;
        };
        draw(el, src);
    }
}

/// One asynchronous draw of `src` into `el`: claim a generation, render,
/// and land the result only if no newer draw has claimed the node since.
fn draw(el: Element, src: String) {
    let gen = NEXT_GEN.with(|g| {
        let v = g.get().wrapping_add(1);
        g.set(v);
        v
    });
    let _ = el.set_attribute("data-gen", &gen.to_string());
    spawn_local(async move {
        let result = render_svg(src, &el).await;
        if el.get_attribute("data-gen").as_deref() != Some(gen.to_string().as_str()) {
            return; // superseded by a newer draw of this node
        }
        match result {
            Ok(svg) => {
                el.set_inner_html(&svg);
                let _ = el.set_attribute("data-state", "done");
            }
            Err(msg) => paint_error(&el, &msg),
        }
    });
}

/// The inline failure state: the message above the source, which stays.
fn paint_error(el: &Element, msg: &str) {
    let _ = el.set_attribute("data-state", "error");
    let Some(doc) = crate::dom::window().document() else {
        return;
    };
    if let Ok(p) = doc.create_element("p") {
        let _ = p.set_attribute("class", "mermaid-error");
        p.set_text_content(Some(&format!("mermaid: {msg}")));
        let first = el.first_child();
        let _ = el.insert_before(&p, first.as_ref());
    }
}

async fn render_svg(src: String, at: &Element) -> Result<String, String> {
    let m = ensure_loaded().await?;
    initialize(&m, at)?;
    let id = NEXT_ID.with(|n| {
        let v = n.get();
        n.set(v.wrapping_add(1));
        v
    });
    let render = method(&m, "render")?;
    let promise: Promise = render
        .call2(&m, &format!("menger-mermaid-{id}").into(), &src.into())
        .map_err(|e| js_message(&e))?
        .dyn_into()
        .map_err(|_| "mermaid.render did not return a promise".to_string())?;
    let out = JsFuture::from(promise).await.map_err(|e| js_message(&e))?;
    Reflect::get(&out, &"svg".into())
        .ok()
        .and_then(|v| v.as_string())
        .ok_or_else(|| "mermaid.render returned no svg".to_string())
}

/// The `mermaid` global once the bundle has run; `None` until then.
fn mermaid_global() -> Option<Object> {
    let v = Reflect::get(crate::dom::window().as_ref(), &"mermaid".into()).ok()?;
    v.is_object().then(|| v.unchecked_into())
}

/// Resolve the renderer, fetching the bundle on first need. Callers
/// arriving while a load is in flight await the same promise.
async fn ensure_loaded() -> Result<Object, String> {
    if let Some(m) = mermaid_global() {
        return Ok(m);
    }
    let promise = LOADING.with(|l| l.borrow_mut().get_or_insert_with(inject_script).clone());
    match JsFuture::from(promise).await {
        Ok(_) => mermaid_global()
            .ok_or_else(|| format!("{ASSET_URL} loaded but defined no `mermaid` global")),
        Err(e) => {
            LOADING.with(|l| *l.borrow_mut() = None);
            Err(format!(
                "could not load the diagram renderer from {ASSET_URL}: {}",
                js_message(&e)
            ))
        }
    }
}

/// Append `<script src=ASSET_URL>` and settle when it has run (or failed).
/// The promise's own resolve/reject functions are the load handlers — no
/// closures to keep alive.
fn inject_script() -> Promise {
    Promise::new(&mut |resolve, reject| {
        let Some(doc) = crate::dom::window().document() else {
            let _ = reject.call1(&JsValue::NULL, &"no document".into());
            return;
        };
        let script = doc
            .create_element("script")
            .ok()
            .and_then(|e| e.dyn_into::<HtmlElement>().ok());
        let Some(script) = script else {
            let _ = reject.call1(&JsValue::NULL, &"cannot create a script element".into());
            return;
        };
        let _ = script.set_attribute("src", ASSET_URL);
        script.set_onload(Some(&resolve));
        script.set_onerror(Some(&reject));
        let parent: Option<Node> = doc
            .head()
            .map(Node::from)
            .or_else(|| doc.body().map(Node::from));
        match parent {
            Some(p) if p.append_child(&script).is_ok() => {}
            _ => {
                let _ = reject.call1(&JsValue::NULL, &"cannot attach the script element".into());
            }
        }
    })
}

/// Configure for this render. Cheap, and re-reading the palette each time
/// means a diagram rendered after a theme change picks the right side.
/// THEME-7: the diagram colours are read where the diagram sits — its
/// region's theme (a ticket view or a frame box wearing another theme than
/// the site's), as a frame reads its tokens.
fn initialize(m: &Object, at: &Element) -> Result<(), String> {
    let mut cfg = config_json(theme_for_background(&body_background()));
    let style = crate::dom::window().get_computed_style(at).ok().flatten().or_else(body_style);
    with_diagram_tokens(&mut cfg, |token| style.as_ref().and_then(|s| s.get_property_value(token).ok()));
    let cfg = js_sys::JSON::parse(&cfg.to_string()).map_err(|e| js_message(&e))?;
    method(m, "initialize")?
        .call1(m, &cfg)
        .map(|_| ())
        .map_err(|e| js_message(&e))
}

/// mermaid's own default `secure` list (11.17.2). Setting `secure` in the
/// site config *replaces* it, so the defaults are restated here.
const MERMAID_DEFAULT_SECURE: [&str; 6] = [
    "secure",
    "securityLevel",
    "startOnLoad",
    "maxTextSize",
    "suppressErrorRendering",
    "maxEdges",
];

/// The site config every render runs under. Verified against the 11.17.2
/// source: in `strict` mode mermaid strips scripts and handlers from
/// label HTML but keeps the rest *live* (an `<img>` in a label would be a
/// real element), so labels are switched to SVG text — markup in a label
/// is then literal text. `dompurifyConfig` is defense in depth for any
/// renderer that still sanitizes HTML text. Both keys join `secure`,
/// which mermaid's directive sanitizer applies at every nesting level, so
/// no `%%{init: …}%%` in diagram source can re-enable HTML labels (even
/// as `flowchart.htmlLabels`) or loosen the purifier.
pub fn config_json(theme: &str) -> serde_json::Value {
    let mut secure: Vec<&str> = MERMAID_DEFAULT_SECURE.to_vec();
    secure.extend(["htmlLabels", "dompurifyConfig"]);
    serde_json::json!({
        "startOnLoad": false,
        "securityLevel": "strict",
        "suppressErrorRendering": true,
        "htmlLabels": false,
        "flowchart": { "htmlLabels": false },
        "class": { "htmlLabels": false },
        "state": { "htmlLabels": false },
        "dompurifyConfig": {
            // mermaid's own default forbids `style`; the rest closes every
            // tag that could fetch, navigate, or embed.
            "FORBID_TAGS": ["style", "img", "a", "svg", "math", "iframe", "object", "embed",
                            "video", "audio", "source", "picture", "link", "meta", "base",
                            "form", "input", "button", "textarea", "select", "foreignobject",
                            "use", "image"],
            "FORBID_ATTR": ["style", "src", "href", "xlink:href", "srcset", "ping",
                            "background", "poster", "action", "formaction"],
        },
        "secure": secure,
        "theme": theme,
        // The diagram reads as part of the page: its text takes the page's
        // font rather than mermaid's default.
        "themeVariables": { "fontFamily": "inherit" },
    })
}

/// The body's computed style.
fn body_style() -> Option<web_sys::CssStyleDeclaration> {
    let window = crate::dom::window();
    window.document().and_then(|d| d.body()).and_then(|b| window.get_computed_style(&b).ok().flatten())
}

/// The page's computed body background, e.g. `rgb(16, 16, 20)`.
fn body_background() -> String {
    body_style().and_then(|s| s.get_property_value("background-color").ok()).unwrap_or_default()
}

/// THEME-2: a diagram drawn in the theme's colours — each `colour.diagram`
/// token that names a colour (not its default `auto`) set into mermaid's
/// `themeVariables` ([`menger_tickets_api::tokens::DIAGRAM`]); with none,
/// the config is as it was, mermaid's own palette for the mode.
pub fn with_diagram_tokens(cfg: &mut serde_json::Value, read: impl Fn(&str) -> Option<String>) {
    for (token, keys) in menger_tickets_api::tokens::DIAGRAM {
        let Some(v) = read(token).map(|v| v.trim().to_string()) else { continue };
        if v.is_empty() || v == "auto" || !menger_tickets_api::themes::value_ok(&v) {
            continue;
        }
        for k in *keys {
            cfg["themeVariables"][*k] = serde_json::Value::String(v.clone());
        }
    }
}

/// Pick mermaid's built-in theme from the palette actually painted behind
/// the page: dark background → `dark`, otherwise mermaid's `default`. A
/// color that cannot be read (or is transparent) falls back to `default`.
pub fn theme_for_background(css_color: &str) -> &'static str {
    match luminance(css_color) {
        Some(l) if l < 0.5 => "dark",
        _ => "default",
    }
}

/// Relative luminance in 0..=1 of an `rgb(r, g, b)` / `rgba(r, g, b, a)`
/// string; `None` if it is not one, or is fully transparent.
fn luminance(css: &str) -> Option<f64> {
    let css = css.trim();
    let inner = css
        .strip_prefix("rgba(") // tokens-lint: parses a computed colour
        .or_else(|| css.strip_prefix("rgb("))? // tokens-lint: parses a computed colour
        .strip_suffix(')')?;
    let parts: Vec<f64> = inner
        .split([',', '/', ' '])
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .ok()?;
    let (r, g, b) = match parts.as_slice() {
        [r, g, b] => (*r, *g, *b),
        [_, _, _, a] if *a == 0.0 => return None,
        [r, g, b, _] => (*r, *g, *b),
        _ => return None,
    };
    Some((0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0)
}

fn method(obj: &Object, name: &str) -> Result<Function, String> {
    Reflect::get(obj, &name.into())
        .ok()
        .and_then(|f| f.dyn_into::<Function>().ok())
        .ok_or_else(|| format!("mermaid.{name} is not a function"))
}

/// A JS rejection as text: an Error's message (mermaid's parse errors
/// are Errors — "Parse error on line 2: …"), a string as itself, and a
/// load-failure `Event` as a plain sentence.
fn js_message(v: &JsValue) -> String {
    if let Some(e) = v.dyn_ref::<js_sys::Error>() {
        return String::from(e.message());
    }
    if let Some(s) = v.as_string() {
        return s;
    }
    if Reflect::has(v, &"type".into()).unwrap_or(false) {
        return "the script failed to load (network error or 404)".into();
    }
    format!("{v:?}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// The three places that name the bundle agree: the URL the app will
    /// fetch, the file vendored in the repo, and index.html's copy-file
    /// link that puts it into dist. Upgrading mermaid means moving all
    /// three — this fails until they line up. Also pins laziness: the
    /// bundle is never a page-load `<script>` in index.html.
    #[test]
    fn vendored_asset_named_by_asset_url_exists() {
        let name = ASSET_URL.rsplit('/').next().unwrap();
        assert!(ASSET_URL.starts_with("/app/vendor/"), "{ASSET_URL}");
        let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor/mermaid");
        let bundle = vendor.join(name);
        assert!(
            bundle.is_file(),
            "vendored bundle missing: {}",
            bundle.display()
        );
        assert!(
            std::fs::metadata(&bundle).unwrap().len() > 1_000_000,
            "the full IIFE bundle, not a stub"
        );
        assert!(
            vendor.join("LICENSE").is_file(),
            "license recorded beside it"
        );
        let readme = std::fs::read_to_string(vendor.join("README.md")).unwrap();
        let version = name
            .strip_prefix("mermaid-")
            .and_then(|s| s.strip_suffix(".min.js"))
            .unwrap();
        assert!(readme.contains(version), "README records version {version}");

        let index =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("index.html"))
                .unwrap();
        assert!(
            index.contains(&format!("href=\"vendor/mermaid/{name}\"")),
            "index.html copy-files the bundle into dist"
        );
        for line in index.lines() {
            assert!(
                !(line.contains("<script") && line.contains("src=") && line.contains("mermaid")),
                "the bundle must not load with the page: {line}"
            );
        }
    }

    /// The security posture, pinned: strict, no HTML labels anywhere, no
    /// startOnLoad, errors suppressed (ours paint), and the keys a
    /// directive must not touch listed in `secure` — mermaid's own
    /// defaults included, since setting `secure` replaces them.
    /// THEME-2: `auto` (the default) leaves mermaid's palette; a colour
    /// sets every variable its token names.
    #[test]
    fn diagram_tokens_set_theme_variables_only_when_a_theme_names_a_colour() {
        let mut c = config_json("default");
        with_diagram_tokens(&mut c, |_| Some("auto".into()));
        assert_eq!(c, config_json("default"));
        with_diagram_tokens(&mut c, |t| (t == "--diagram-border").then(|| " #d4572a ".into()));
        assert_eq!(c["themeVariables"]["primaryBorderColor"], "#d4572a");
        assert_eq!(c["themeVariables"]["nodeBorder"], "#d4572a");
        assert_eq!(c["themeVariables"]["fontFamily"], "inherit");
        assert!(c["themeVariables"].get("primaryColor").is_none());
    }

    #[test]
    fn config_is_strict_and_directive_proof() {
        let c = config_json("dark");
        assert_eq!(c["securityLevel"], "strict");
        assert_eq!(c["startOnLoad"], false);
        assert_eq!(c["suppressErrorRendering"], true);
        assert_eq!(c["htmlLabels"], false);
        for section in ["flowchart", "class", "state"] {
            assert_eq!(c[section]["htmlLabels"], false, "{section}");
        }
        assert_eq!(c["theme"], "dark");
        let secure: Vec<&str> = c["secure"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for key in MERMAID_DEFAULT_SECURE {
            assert!(secure.contains(&key), "default secure key {key} kept");
        }
        for key in ["htmlLabels", "dompurifyConfig"] {
            assert!(secure.contains(&key), "{key} locked against directives");
        }
        let forbid: Vec<&str> = c["dompurifyConfig"]["FORBID_TAGS"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for tag in ["style", "img", "a", "iframe", "foreignobject"] {
            assert!(forbid.contains(&tag), "{tag} forbidden in label HTML");
        }
        // Valid JSON for JSON.parse on the JS side.
        assert!(serde_json::to_string(&c).is_ok());
    }

    #[test]
    fn theme_follows_the_painted_background() {
        assert_eq!(theme_for_background("rgb(251, 251, 249)"), "default"); // light --bg (AGS-15, #fbfbf9)
        assert_eq!(theme_for_background("rgb(16, 16, 20)"), "dark"); // dark --bg (AGS-17, #101014; the vanilla page's too)
        assert_eq!(theme_for_background("rgb(23, 23, 29)"), "dark"); // dark --panel (#17171d)
        assert_eq!(theme_for_background("rgb(255, 255, 255)"), "default");
        assert_eq!(theme_for_background("rgba(250, 250, 250, 1)"), "default");
        assert_eq!(theme_for_background("rgb(0 0 0)"), "dark");
        // Unknown or transparent: mermaid's own default, never a panic.
        assert_eq!(theme_for_background("rgba(0, 0, 0, 0)"), "default");
        assert_eq!(theme_for_background(""), "default");
        assert_eq!(theme_for_background("color(srgb 0 0 0)"), "default");
    }
}
