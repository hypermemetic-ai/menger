/* menger page shim (MENGER-15) — the script a frame runs first when its body
 * is a whole HTML document (frontend/src/frame.rs, `page_srcdoc`), and in a
 * mockup's frames (any body). It is inlined right after IDENTITY-28's CSP,
 * the first element of the page's head, and before anything the page holds;
 * the frame keeps its sandbox (`allow-scripts allow-popups
 * allow-popups-to-escape-sandbox`, no same origin, no forms) and its CSP
 * (`connect-src 'none'; form-action 'none'`). A page gets no menger SDK, no
 * base typography and no theme tokens: it renders as itself.
 *
 * What it does:
 *
 *   - **Emulates the viewer's preferences** the host sets — `scheme`
 *     (`light` / `dark`: `prefers-color-scheme`) and `reduce` (`true` /
 *     `false`: `prefers-reduced-motion`), each `null` for the browser's own.
 *     An iframe's `prefers-color-scheme` is the embedder's in Chromium
 *     whatever the iframe element's `color-scheme` says (Firefox follows the
 *     element's — the host sets that too), and nothing sets
 *     `prefers-reduced-motion` per frame, so the shim makes the page's own
 *     queries answer as emulated: every `@media` rule (nested ones, and
 *     `@import`'s) whose query names either feature has the feature replaced
 *     by an always-true or always-false condition — kept reversible, the
 *     original text remembered per rule; the `media` attribute of every
 *     `<source>`, `<link>` and `<style>` likewise (a `<picture>`'s dark
 *     variant included); `window.matchMedia` of such a query answers a live
 *     list whose `matches` follows the emulation and which fires `change`
 *     when a toggle moves it; and the root carries `data-menger-scheme` /
 *     `data-menger-motion` for a page that would rather read a class, and
 *     `color-scheme` when the page declared both (`light dark`), so its form
 *     controls and canvas follow. Style sheets added later are taken as they
 *     arrive. A cross-origin style sheet cannot be read, and is left alone.
 *   - **Listens** for `menger:emulate { channel, scheme, reduce }` from the
 *     host (its own parent, on its own channel) — a mockup's toggles, or the
 *     app's theme flipped — and applies it live, no reload.
 *   - In a page (not under the SDK): **says it is ready** (`menger:ready`,
 *     which shows the frame), **hands the host its keys** — Esc
 *     (`menger:key { key: "Escape" }`: leave full screen), `z` (full screen
 *     on and off) and `a`/`A` (annotation mode), never from a field — and
 *     **keeps in-page links in the page**: a `#fragment` link scrolls to its
 *     target (a srcdoc document's base URL is the app's, so the browser
 *     would navigate the frame away); an app route (`#/…`) navigates the app,
 *     and any other link is handed to the host as `menger:link { href }` —
 *     MENGER-14's path: inside menger it navigates, leaving it asks first.
 */
(function () {
  "use strict";
  var doc = document;
  var init = {};
  try {
    init = JSON.parse((doc.getElementById("menger-init") || {}).textContent || "{}") || {};
  } catch (e) {
    init = {};
  }
  var channel = init.channel || "";
  var page = !!init.page;
  var host = window.parent;
  var em = { scheme: null, reduce: null };
  function take(e) {
    if (!e || typeof e !== "object") return;
    em.scheme = e.scheme === "light" || e.scheme === "dark" ? e.scheme : null;
    em.reduce = typeof e.reduce === "boolean" ? e.reduce : null;
  }
  take(init.emulate);

  function post(type, payload) {
    var msg = payload || {};
    msg.type = type;
    msg.channel = channel;
    try {
      host.postMessage(msg, "*");
    } catch (e) {
      /* detached */
    }
  }

  // -- the rewrite --------------------------------------------------------
  var YES = "(min-width: 0px)";
  var NO = "(not (min-width: 0px))";
  var SCHEME = /\(\s*prefers-color-scheme\s*(?::\s*([a-z-]+)\s*)?\)/gi;
  var MOTION = /\(\s*prefers-reduced-motion\s*(?::\s*([a-z-]+)\s*)?\)/gi;
  function names(q) {
    return /prefers-(color-scheme|reduced-motion)/i.test(q || "");
  }
  // A query with the emulated features answered: each becomes YES or NO.
  function rewrite(q) {
    if (em.scheme) {
      q = q.replace(SCHEME, function (_, v) {
        return !v || v.toLowerCase() === em.scheme ? YES : NO;
      });
    }
    if (em.reduce !== null) {
      q = q.replace(MOTION, function (_, v) {
        if (!v) return em.reduce ? YES : NO; // the boolean form: "reduce"
        v = v.toLowerCase();
        return (v === "reduce") === em.reduce ? YES : NO;
      });
    }
    return q;
  }

  var original = typeof WeakMap === "function" ? new WeakMap() : null;
  function rewriteList(owner, list) {
    if (!list || !original) return;
    var was = original.get(owner);
    if (was === undefined) {
      if (!names(list.mediaText)) return;
      was = list.mediaText;
      original.set(owner, was);
    }
    var now = rewrite(was);
    if (list.mediaText !== now) {
      try {
        list.mediaText = now;
      } catch (e) {
        /* a list that will not take it keeps its own */
      }
    }
  }
  function walkRules(rules) {
    if (!rules) return;
    for (var i = 0; i < rules.length; i++) {
      var r = rules[i];
      if (r.media && (r.type === 4 || r.type === 3)) rewriteList(r, r.media); // @media, @import
      if (r.type === 3 && r.styleSheet) walkSheet(r.styleSheet);
      if (r.cssRules) walkRules(r.cssRules);
    }
  }
  function walkSheet(sheet) {
    var rules;
    try {
      rules = sheet.cssRules;
    } catch (e) {
      return; // cross-origin: unreadable, left alone
    }
    walkRules(rules);
  }
  function attrs() {
    var els = doc.querySelectorAll("source[media], link[media], style[media], [data-menger-media]");
    for (var i = 0; i < els.length; i++) {
      var el = els[i];
      var was = el.getAttribute("data-menger-media");
      if (was === null) {
        was = el.getAttribute("media") || "";
        if (!names(was)) continue;
        el.setAttribute("data-menger-media", was);
      }
      var now = rewrite(was);
      if (el.getAttribute("media") !== now) el.setAttribute("media", now);
    }
  }
  function root() {
    var r = doc.documentElement;
    if (!r) return;
    if (em.scheme) r.setAttribute("data-menger-scheme", em.scheme);
    else r.removeAttribute("data-menger-scheme");
    if (em.reduce !== null) r.setAttribute("data-menger-motion", em.reduce ? "reduce" : "no-preference");
    else r.removeAttribute("data-menger-motion");
    // The page's own `color-scheme`, read with ours taken off.
    r.style.removeProperty("color-scheme");
    if (em.scheme) {
      var cs = "";
      try {
        cs = getComputedStyle(r).colorScheme || "";
      } catch (e) {
        cs = "";
      }
      var meta = doc.querySelector('meta[name="color-scheme"]');
      var declared = cs + " " + (meta ? meta.getAttribute("content") || "" : "");
      if (/light/.test(declared) && /dark/.test(declared)) r.style.setProperty("color-scheme", em.scheme);
    }
  }
  function apply() {
    var sheets = doc.styleSheets;
    // A sheet's own `media` is its owner's attribute (attrs()); its rules here.
    for (var i = 0; i < sheets.length; i++) walkSheet(sheets[i]);
    attrs();
    root();
    lists.forEach(function (l) {
      l._sync();
    });
  }

  // -- matchMedia ----------------------------------------------------------
  var native = window.matchMedia ? window.matchMedia.bind(window) : null;
  var lists = [];
  function List(query) {
    var t = typeof EventTarget === "function" ? new EventTarget() : doc.createElement("i");
    var last = null;
    function now() {
      return native ? native(rewrite(query)).matches : false;
    }
    Object.defineProperty(t, "media", { value: query, enumerable: true });
    Object.defineProperty(t, "matches", { get: now, enumerable: true });
    t.onchange = null;
    t.addListener = function (cb) {
      t.addEventListener("change", cb);
    };
    t.removeListener = function (cb) {
      t.removeEventListener("change", cb);
    };
    t._sync = function () {
      var m = now();
      if (last !== null && m !== last) {
        var ev = new Event("change");
        ev.matches = m;
        ev.media = query;
        if (typeof t.onchange === "function") {
          try {
            t.onchange(ev);
          } catch (e) {
            setTimeout(function () {
              throw e;
            });
          }
        }
        t.dispatchEvent(ev);
      }
      last = m;
    };
    t._sync();
    return t;
  }
  if (native) {
    window.matchMedia = function (query) {
      query = String(query);
      if (!names(query)) return native(query);
      var l = List(query);
      lists.push(l);
      return l;
    };
  }

  // -- when ------------------------------------------------------------------
  apply();
  if (typeof MutationObserver === "function") {
    new MutationObserver(function (records) {
      for (var i = 0; i < records.length; i++) {
        var added = records[i].addedNodes;
        for (var j = 0; j < added.length; j++) {
          var n = added[j];
          if (n.nodeType === 1 && /^(style|link|source)$/i.test(n.tagName)) {
            apply();
            if (/^link$/i.test(n.tagName)) n.addEventListener("load", apply);
            return;
          }
        }
      }
    }).observe(doc, { childList: true, subtree: true });
  }
  doc.addEventListener("DOMContentLoaded", apply);
  window.addEventListener("load", apply);

  window.addEventListener("message", function (ev) {
    if (ev.source !== host) return;
    var m = ev.data;
    if (!m || typeof m !== "object" || m.channel !== channel) return;
    if (m.type === "menger:emulate") {
      take(m);
      apply();
    }
  });

  if (!page) return;

  // -- a page's own: ready, keys, links ---------------------------------------
  post("menger:ready", {});
  doc.addEventListener("keydown", function (ev) {
    if (ev.defaultPrevented || ev.isComposing || ev.ctrlKey || ev.metaKey || ev.altKey) return;
    var t = ev.composedPath ? ev.composedPath()[0] : ev.target;
    if (t && t.nodeType === 1 && (/^(input|textarea|select)$/i.test(t.tagName) || t.isContentEditable)) return;
    if (ev.key === "Escape" || ev.key === "z" || ev.key === "a" || ev.key === "A") {
      if (ev.key !== "Escape") ev.preventDefault();
      post("menger:key", { key: ev.key });
    }
  });
  // Links, as the SDK takes them (MENGER-14): an app route (`#/…`) is
  // `menger:navigate`; any other link but an anchor in this page is handed
  // to the host as `menger:link { href }`, as written — it navigates inside
  // menger, asks before one that leaves it, refuses the rest. A link the
  // page's own handler prevented is the page's.
  function takeLink(ev) {
    if (ev.defaultPrevented) return;
    if (ev.type === "click" ? ev.button !== 0 : ev.button !== 1) return;
    var path = ev.composedPath ? ev.composedPath() : [ev.target];
    var a = null;
    for (var i = 0; i < path.length && !a; i++) {
      var n = path[i];
      if (n && n.nodeType === 1 && n.localName === "a" && n.hasAttribute("href")) a = n;
    }
    if (!a) return;
    var href = a.getAttribute("href") || "";
    ev.preventDefault();
    if (href.indexOf("#/") === 0) {
      post("menger:navigate", { route: href });
      return;
    }
    if (href.charAt(0) === "#") {
      var id = href.slice(1);
      try {
        id = decodeURIComponent(id);
      } catch (e) {
        /* as written */
      }
      var to = id ? doc.getElementById(id) || doc.getElementsByName(id)[0] : null;
      if (to) to.scrollIntoView({ behavior: em.reduce ? "auto" : "smooth" });
      else if (!id || id === "top") window.scrollTo(0, 0);
      return;
    }
    post("menger:link", { href: href });
  }
  doc.addEventListener("click", takeLink);
  doc.addEventListener("auxclick", takeLink);
})();
