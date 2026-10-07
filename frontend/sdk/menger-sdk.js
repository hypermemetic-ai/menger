/* menger SDK shell (COMP-4) — the script every framed body runs first.
 *
 * A framed body (frontend/src/frame.rs) is one sandboxed iframe —
 * `sandbox="allow-scripts allow-forms allow-popups
 * allow-popups-to-escape-sandbox"`, no `allow-same-origin`, so its origin is
 * opaque — whose `srcdoc` is the body's HTML with this script inlined at the
 * top of <head>. It runs before anything the body holds, and it is the
 * frame's whole contract with the app (lavish-axi's injected SDK, in
 * menger's idiom):
 *
 *   - `window.menger` — `{ version }` today; COMP-5 adds `call` and `on`.
 *   - base typography matching the app's ticket text (the `.md` rules), at
 *     zero specificity (`:where`), so any rule the body writes wins;
 *   - the app's theme tokens (`--bg`, `--ink`, …, `--sans`, `--mono`) on the
 *     frame's `:root`, and `color-scheme` + `<html data-theme>`, following
 *     the app's light/dark flips;
 *   - mermaid placeholders (md.rs) drawn here, with the vendored bundle,
 *     in the theme's mermaid palette, redrawn on a flip;
 *   - the document's height, reported to the host (ResizeObserver), which
 *     sizes the frame to it: a frame never scrolls inside.
 *
 * Handshake (postMessage; targetOrigin "*" both ways — the frame's origin is
 * opaque, and the messages carry nothing secret). Every message is
 * `{ type: "menger:<kind>", channel, ... }`; `channel` is the id the host
 * stamped into this frame's srcdoc (#menger-init) and on its <iframe
 * data-channel>. The host accepts a message only from the frame's own
 * contentWindow carrying the frame's channel; this script accepts one only
 * from `window.parent` carrying its channel.
 *
 *   frame → host  menger:ready  { version }               once, at start
 *   host → frame  menger:theme  { scheme, tokens }        on ready, and on
 *                                                         every theme flip
 *   frame → host  menger:size   { height, settled }       whenever the
 *                 height changes; `settled` turns true once the first
 *                 paint is complete (diagrams drawn) and stays true — the
 *                 host swaps a re-rendered frame in only then, so an
 *                 amended body never flashes.
 *
 * The init block (#menger-init, JSON) carries
 * `{ channel, version, scheme, mermaid: { url, config } }`. The first
 * theme is already in the srcdoc (<style id="menger-theme">, <html
 * data-theme>), so the first paint is right before any message arrives.
 */
(function () {
  "use strict";

  var VERSION = "0.1.0";
  // The height a frame may claim. A body sized from the viewport (`110vh`)
  // would otherwise grow its frame forever.
  var MAX_HEIGHT = 50000;
  // Diagrams that never finish (a bundle that will not load) do not hold
  // the frame back past this.
  var SETTLE_MS = 4000;

  // Taken before the body's own scripts run: they may shadow these.
  var host = window.parent;
  var doc = document;
  var RO = window.ResizeObserver;

  var init = {};
  try {
    init = JSON.parse(doc.getElementById("menger-init").textContent) || {};
  } catch (e) {
    init = {};
  }
  var channel = String(init.channel || "");
  var scheme = init.scheme === "dark" ? "dark" : "light";
  var mermaidInit = init.mermaid && init.mermaid.url ? init.mermaid : null;

  function post(type, payload) {
    var msg = payload || {};
    msg.type = type;
    msg.channel = channel;
    try {
      host.postMessage(msg, "*");
    } catch (e) {
      /* a detached frame has no one to tell */
    }
  }

  // -- window.menger ------------------------------------------------------

  var api = Object.freeze({ version: VERSION });
  try {
    Object.defineProperty(window, "menger", { value: api, enumerable: true, writable: false, configurable: false });
  } catch (e) {
    window.menger = api;
  }

  // -- base typography: the app's `.md` rules, overridable ----------------

  var BASE_CSS = [
    "*,*::before,*::after{box-sizing:border-box}",
    ":where(html){overflow:hidden;height:auto;background:transparent}",
    ":where(body){margin:0;padding:0;display:flow-root;overflow-x:auto;background:transparent;" +
      "color:var(--ink);font:14px/1.6 var(--sans)}",
    ":where(h1){font-size:1.1rem;margin:1.1rem 0 .4rem}",
    ":where(h2,h3,h4){font-size:1rem;margin:1.1rem 0 .4rem}",
    ":where(code){font-family:var(--mono);font-size:.92em;background:var(--panel);" +
      "border:1px solid var(--line);border-radius:3px;padding:0 .25rem}",
    ":where(pre){font-family:var(--mono);background:var(--panel);border:1px solid var(--line);" +
      "border-radius:4px;padding:.6rem .8rem;overflow-x:auto}",
    ":where(pre code){border:0;padding:0;background:none;font-size:inherit}",
    ":where(a){color:var(--accent)}",
    ":where(blockquote){border-left:3px solid var(--line);margin:.5rem 0;padding:0 .8rem;color:var(--muted)}",
    ":where(table){border-collapse:collapse}",
    ":where(th,td){border:1px solid var(--line);padding:.25rem .5rem;text-align:left}",
    ":where(img,video){max-width:100%}",
    ":where(details>summary){cursor:pointer}",
    ":where(.mermaid){background:var(--panel);border:1px solid var(--line);border-radius:4px;" +
      "padding:.6rem .8rem;margin:.5rem 0;overflow-x:auto}",
    ":where(.mermaid pre){margin:0;padding:0;background:none;border:0}",
    ":where(.mermaid[data-state=rendering] pre){color:var(--muted)}",
    ":where(.mermaid[data-state=done] svg){display:block;max-width:100%;height:auto;margin:0 auto}",
    ":where(.mermaid[data-state=error]){border-color:var(--err-fg)}",
    ":where(.mermaid .mermaid-error){color:var(--err-fg);margin:0 0 .6rem;white-space:pre-wrap;font-size:.85rem}",
    ":where(.wave-ref){font-family:var(--mono);font-size:.85em;white-space:nowrap;text-decoration:none;" +
      "border:1px solid var(--chip-line);background:var(--chip-bg);color:var(--chip-fg);border-radius:6px;padding:0 5px}",
    ":where(a.wave-ref){color:var(--accent)}",
    ":where(.wave-ref.missing){color:var(--err-fg)}",
  ].join("\n");

  var base = doc.createElement("style");
  base.id = "menger-base";
  base.textContent = BASE_CSS;
  var head = doc.head || doc.documentElement;
  head.insertBefore(base, head.firstChild);

  // -- theme tokens ---------------------------------------------------------

  var TOKEN_NAME = /^--[a-z0-9-]+$/;

  function themeRule() {
    var el = doc.getElementById("menger-theme");
    if (!el) {
      el = doc.createElement("style");
      el.id = "menger-theme";
      el.textContent = ":root{}";
      head.insertBefore(el, base.nextSibling);
    }
    var sheet = el.sheet;
    if (!sheet || !sheet.cssRules.length) {
      el.textContent = ":root{}";
      sheet = el.sheet;
    }
    return sheet && sheet.cssRules[0];
  }

  function applyTheme(next, tokens) {
    var rule = themeRule();
    if (rule && rule.style) {
      rule.style.setProperty("color-scheme", next);
      for (var name in tokens) {
        if (Object.prototype.hasOwnProperty.call(tokens, name) && TOKEN_NAME.test(name)) {
          rule.style.setProperty(name, String(tokens[name]));
        }
      }
    }
    doc.documentElement.setAttribute("data-theme", next);
    if (next !== scheme) {
      scheme = next;
      redrawDiagrams();
    }
  }

  window.addEventListener("message", function (ev) {
    if (ev.source !== host) return;
    var m = ev.data;
    if (!m || typeof m !== "object" || m.channel !== channel) return;
    if (m.type === "menger:theme") {
      applyTheme(m.scheme === "dark" ? "dark" : "light", m.tokens && typeof m.tokens === "object" ? m.tokens : {});
    }
  });

  // -- links ------------------------------------------------------------------
  // Every link opens a new top-level tab (<base target="_blank"> in the
  // srcdoc; the sandbox's popups escape it). A link to an anchor in this
  // body (a footnote) scrolls to it instead.

  doc.addEventListener("click", function (ev) {
    var a = ev.target && ev.target.closest ? ev.target.closest("a[href^='#']") : null;
    if (!a || ev.defaultPrevented) return;
    var id = decodeURIComponent(a.getAttribute("href").slice(1));
    var target = id && doc.getElementById(id);
    if (target) {
      ev.preventDefault();
      target.scrollIntoView({ block: "start" });
    }
  });

  // -- mermaid ----------------------------------------------------------------
  // The placeholders md.rs writes (`.mermaid[data-state=pending]`, the
  // source as text), drawn with the app's vendored bundle under the app's
  // strict config (src/mermaid.rs), one at a time; the source stays on the
  // node (`data-src`) so a theme flip can draw it again, and a generation
  // (`data-gen`) lets only the newest draw of a node land.

  var loading = null;
  var queue = Promise.resolve();
  var nextId = 0;
  var nextGen = 0;

  function loadMermaid() {
    if (loading) return loading;
    loading = new Promise(function (resolve, reject) {
      var s = doc.createElement("script");
      s.src = mermaidInit.url;
      s.onload = function () {
        if (window.mermaid && typeof window.mermaid.render === "function") resolve(window.mermaid);
        else reject(new Error(mermaidInit.url + " loaded but defined no `mermaid` global"));
      };
      s.onerror = function () {
        loading = null;
        reject(new Error("could not load the diagram renderer from " + mermaidInit.url));
      };
      head.appendChild(s);
    });
    return loading;
  }

  function paintError(el, err) {
    el.setAttribute("data-state", "error");
    var p = doc.createElement("p");
    p.className = "mermaid-error";
    p.textContent = "mermaid: " + (err && err.message ? err.message : String(err));
    el.insertBefore(p, el.firstChild);
  }

  function draw(el, src) {
    var gen = String(++nextGen);
    el.setAttribute("data-gen", gen);
    var job = queue.then(function () {
      return loadMermaid()
        .then(function (m) {
          var config = JSON.parse(JSON.stringify(mermaidInit.config || {}));
          config.theme = scheme === "dark" ? "dark" : "default";
          m.initialize(config);
          return m.render("menger-mermaid-" + nextId++, src);
        })
        .then(
          function (out) {
            if (el.getAttribute("data-gen") !== gen) return;
            el.innerHTML = out && out.svg ? out.svg : "";
            el.setAttribute("data-state", "done");
          },
          function (err) {
            if (el.getAttribute("data-gen") !== gen) return;
            paintError(el, err);
          }
        );
    });
    queue = job.then(null, function () {});
    return job;
  }

  function mountDiagrams() {
    if (!mermaidInit) return Promise.resolve();
    var nodes = doc.querySelectorAll('.mermaid[data-state="pending"]');
    var jobs = [];
    for (var i = 0; i < nodes.length; i++) {
      var el = nodes[i];
      var src = el.textContent || "";
      el.setAttribute("data-src", src);
      el.setAttribute("data-state", "rendering");
      jobs.push(draw(el, src));
    }
    return Promise.all(jobs);
  }

  function redrawDiagrams() {
    if (!mermaidInit) return;
    var nodes = doc.querySelectorAll('.mermaid[data-state="done"][data-src]');
    for (var i = 0; i < nodes.length; i++) draw(nodes[i], nodes[i].getAttribute("data-src"));
  }

  // -- height -----------------------------------------------------------------

  var lastHeight = -1;
  var settled = false;

  function measure() {
    var h = doc.documentElement.getBoundingClientRect().height;
    if (doc.body) h = Math.max(h, doc.body.scrollHeight);
    return Math.min(MAX_HEIGHT, Math.max(0, Math.ceil(h)));
  }

  function report(force) {
    var h = measure();
    if (!force && h === lastHeight) return;
    lastHeight = h;
    post("menger:size", { height: h, settled: settled });
  }

  function settle() {
    if (settled) return;
    settled = true;
    report(true);
  }

  function start() {
    if (RO) {
      var ro = new RO(function () {
        report(false);
      });
      ro.observe(doc.documentElement);
      if (doc.body) ro.observe(doc.body);
    }
    window.addEventListener("load", function () {
      report(false);
    });
    if (doc.fonts && doc.fonts.ready) {
      doc.fonts.ready.then(function () {
        report(false);
      });
    }
    report(true);
    mountDiagrams().then(settle, settle);
    setTimeout(settle, SETTLE_MS);
  }

  post("menger:ready", { version: VERSION });
  if (doc.readyState === "loading") doc.addEventListener("DOMContentLoaded", start);
  else start();
})();
