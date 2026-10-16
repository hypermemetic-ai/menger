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
 *     target (the frame document's base URL is `/app/frame`, so the
 *     browser would navigate the frame away); an app route (`#/…`) navigates the app,
 *     and any other link is handed to the host as `menger:link { href }` —
 *     MENGER-14's path: inside menger it navigates, leaving it asks first.
 *   - **Says how tall its page is** (APP-42): `menger:extent { height }`,
 *     the document's scroll height in px, whenever it or the viewport
 *     changes — the host
 *     keeps it for a mockup's device (a frame held to its screen) and notes
 *     in the bar when the page scrolls several screens; elsewhere it is
 *     ignored. A number, nothing of the page's content.
 *   - **Annotation mode** (APP-45): in the mode (`menger:annotate { on }`,
 *     the SDK's message), a scroll of the page is said
 *     (`menger:scrolled`), so the host moves what it draws over a device's
 *     page. In a page (not under the SDK, which does all of it in a body)
 *     the shim takes the whole mode as the SDK does: every element outlined
 *     (in `outline`, the app's `--annot`, which the host sends), the element
 *     under the pointer posted (`menger:hover`), every click the mode's
 *     (`menger:pick`), `menger:probe` / `menger:locate` (`reveal`: the
 *     first scrolled into view first) answered — and the host's keyboard
 *     path, `menger:walk { step }`, walked; the walk's keys pressed inside
 *     the page (the arrows, Enter, `[` `]`) are handed to the host
 *     (`menger:key`), whose they are.
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
    } else if (m.type === "menger:annotate") {
      annot.on = m.on === true;
      if (annot.set) annot.set(m);
    } else if (annot.handle) {
      annot.handle(m);
    }
  });

  // -- APP-45: annotation mode — a scroll is said ---------------------------
  // `set` and `handle` are the page's own (below); under the SDK they stay
  // unset, and the SDK answers the mode.
  var annot = { on: false, set: null, handle: null, scrolled: null };
  var scrollQueued = false;
  window.addEventListener(
    "scroll",
    function () {
      if (!annot.on || scrollQueued) return;
      scrollQueued = true;
      requestAnimationFrame(function () {
        scrollQueued = false;
        post("menger:scrolled", {});
        if (annot.scrolled) annot.scrolled();
      });
    },
    true
  );

  // -- APP-42: how tall the page is -----------------------------------------
  var extentSent = "";
  var extentQueued = false;
  function extent() {
    extentQueued = false;
    var de = doc.documentElement;
    var b = doc.body;
    var h = Math.max(de ? de.scrollHeight : 0, b ? b.scrollHeight : 0);
    // Said again when the viewport moves too (APP-33's resizes): the host
    // measures the screen it is held to as it hears this.
    var key = h + ":" + window.innerWidth + "x" + window.innerHeight;
    if (!isFinite(h) || key === extentSent) return;
    extentSent = key;
    post("menger:extent", { height: h });
  }
  function queueExtent() {
    if (extentQueued) return;
    extentQueued = true;
    setTimeout(extent, 60);
  }
  if (typeof ResizeObserver === "function") {
    var extentRO = new ResizeObserver(queueExtent);
    if (doc.documentElement) extentRO.observe(doc.documentElement);
    var watchBody = function () {
      if (doc.body) extentRO.observe(doc.body);
    };
    if (doc.body) watchBody();
    else doc.addEventListener("DOMContentLoaded", watchBody);
  }
  window.addEventListener("resize", queueExtent);
  window.addEventListener("load", queueExtent);
  doc.addEventListener("DOMContentLoaded", queueExtent);

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

  // -- APP-45: annotation mode, in a page --------------------------------------
  // What the SDK does in a body (COMMENT-3): the host decides what an element
  // is from the chain posted here (src/anchor.rs); this side reads the DOM,
  // outlines, keeps the clicks, and walks by the host's keys.
  var annotStyle = null;
  var hovered = null;
  var cursor = null;
  var picked = Object.create(null);
  var nextPick = 0;
  var WALK_KEYS = /^(ArrowUp|ArrowDown|ArrowLeft|ArrowRight|Enter|\[|\])$/;
  var SKIP = /^(script|style|template|noscript|link|meta|base|title|head)$/;

  function words(t, max) {
    return String(t || "").replace(/\s+/g, " ").trim().slice(0, max);
  }
  function textOf(el) {
    var t = typeof el.innerText === "string" ? el.innerText : el.textContent;
    return String(t || "").slice(0, 600);
  }
  function rectOf(el) {
    var r = el.getBoundingClientRect();
    return { x: r.left, y: r.top, w: r.width, h: r.height };
  }
  function captionOf(n) {
    var tag = n.localName;
    if (tag === "dd") {
      var dt = n.previousElementSibling;
      return dt && dt.localName === "dt" ? words(dt.textContent, 60) || null : null;
    }
    if (tag === "td" || tag === "th") {
      var row = n.parentElement;
      if (!row || (row.parentElement && row.parentElement.localName === "thead")) return null;
      var at = Array.prototype.indexOf.call(row.children, n);
      var table = n.closest("table");
      var head = table && table.querySelector("thead tr");
      var h = head && head.children[at];
      return h ? words(h.textContent, 60) || null : null;
    }
    if (tag === "section" || tag === "article" || tag === "aside" || tag === "details") {
      var hd = n.querySelector(":scope > h1, :scope > h2, :scope > h3, :scope > h4, :scope > summary");
      return hd ? words(hd.textContent, 60) || null : null;
    }
    return null;
  }
  function nodeOf(n) {
    var attrs = [];
    for (var i = 0; i < n.attributes.length && attrs.length < 24; i++) {
      var a = n.attributes[i];
      if (a.name !== "class" && a.name !== "style") attrs.push([a.name, a.value.slice(0, 300)]);
    }
    var nth = 1;
    var of = 0;
    var parent = n.parentElement;
    if (parent) {
      for (var s = parent.firstElementChild; s; s = s.nextElementSibling) {
        if (s.localName === n.localName) {
          of++;
          if (s === n) nth = of;
        }
      }
    }
    return { tag: n.localName, attrs: attrs, classes: Array.prototype.slice.call(n.classList, 0, 8), nth: nth, of: Math.max(of, 1), caption: captionOf(n) };
  }
  // The element, then its ancestors below the body element.
  function chainOf(el) {
    var out = [];
    for (var n = el; n && n.nodeType === 1 && n !== doc.body && n !== doc.documentElement && out.length < 32; n = n.parentElement) out.push(nodeOf(n));
    return out;
  }
  function isPage(el) {
    return !el || el.nodeType !== 1 || el === doc.body || el === doc.documentElement;
  }
  function hover(el) {
    hovered = el;
    post("menger:hover", el ? { chain: chainOf(el), rect: rectOf(el), text: textOf(el) } : { chain: null });
  }
  function pickEl(el) {
    var id = ++nextPick;
    picked[id] = el;
    post("menger:pick", { id: id, chain: chainOf(el), rect: rectOf(el), text: textOf(el) });
  }
  // The outline's colour, as the host sent it — a colour, nothing else.
  function outlineOf(v) {
    v = String(v || "");
    return /^[#(),.%\w\s-]{1,64}$/.test(v) ? v : "currentColor";
  }
  annot.set = function (m) {
    if (annot.on && !annotStyle) {
      annotStyle = doc.createElement("style");
      annotStyle.id = "menger-annotate";
      annotStyle.textContent = "*{outline:1px dashed " + outlineOf(m.outline) + "!important;outline-offset:-1px!important;cursor:crosshair!important}";
      (doc.head || doc.documentElement).appendChild(annotStyle);
    } else if (!annot.on) {
      if (annotStyle && annotStyle.parentNode) annotStyle.parentNode.removeChild(annotStyle);
      annotStyle = null;
      hovered = null;
      cursor = null;
      picked = Object.create(null);
    }
  };
  annot.scrolled = function () {
    if (hovered && hovered.isConnected) hover(hovered);
  };

  function walkable(n) {
    return !!n && n.nodeType === 1 && !SKIP.test(n.localName) && n.getClientRects().length > 0;
  }
  function firstIn(n) {
    for (var c = n.firstElementChild; c; c = c.nextElementSibling) if (walkable(c)) return c;
    return null;
  }
  function sibling(n, d) {
    for (var c = d > 0 ? n.nextElementSibling : n.previousElementSibling; c; c = d > 0 ? c.nextElementSibling : c.previousElementSibling) {
      if (walkable(c)) return c;
    }
    return null;
  }
  function walk(step) {
    var body = doc.body;
    if (!body) return;
    var at = cursor && cursor !== body && body.contains(cursor) ? cursor : null;
    if (step === "pick") {
      if (at) pickEl(at);
      return;
    }
    var next = null;
    if (!at || step === "start") next = firstIn(body);
    else if (step === "next") next = sibling(at, 1);
    else if (step === "prev") next = sibling(at, -1);
    else if (step === "in") next = firstIn(at);
    else if (step === "out") next = at.parentElement !== body ? at.parentElement : null;
    cursor = next || at;
    if (!cursor) return;
    try {
      cursor.scrollIntoView({ block: "nearest", inline: "nearest" });
    } catch (e) {
      /* stays */
    }
    hover(cursor);
  }

  // Where elements are is asked outside the mode too (a link back, as the
  // SDK answers it); probes and walks only in it.
  annot.handle = function (m) {
    if (!annot.on && m.type !== "menger:locate") return;
    if (m.type === "menger:probe") {
      var el = picked[m.id];
      delete picked[m.id];
      var sels = Array.isArray(m.selectors) ? m.selectors : [];
      post("menger:probed", {
        id: m.id,
        ok: sels.map(function (sel) {
          try {
            var all = doc.querySelectorAll(String(sel));
            return !!el && all.length === 1 && all[0] === el;
          } catch (e) {
            return false;
          }
        }),
      });
    } else if (m.type === "menger:locate") {
      var paths = Array.isArray(m.paths) ? m.paths : [];
      if (m.reveal === true && paths.length) {
        try {
          var first = doc.querySelector(String(paths[0]));
          if (first) first.scrollIntoView({ block: "center", inline: "nearest" });
        } catch (e) {
          /* not a selector */
        }
      }
      post("menger:located", {
        id: m.id,
        found: paths.map(function (p) {
          try {
            var el = doc.querySelector(String(p));
            return el ? { rect: rectOf(el), text: textOf(el) } : null;
          } catch (e) {
            return null;
          }
        }),
      });
    } else if (m.type === "menger:walk") {
      walk(m.step);
    }
  };

  doc.addEventListener(
    "mouseover",
    function (ev) {
      if (!annot.on) return;
      var el = ev.target;
      if (isPage(el)) {
        if (hovered) hover(null);
        return;
      }
      if (el !== hovered) hover(el);
    },
    true
  );
  doc.addEventListener(
    "mouseout",
    function (ev) {
      if (annot.on && !ev.relatedTarget) hover(null);
    },
    true
  );
  // Every pointer and mouse event is the mode's, as in a body: stopped
  // before the page hears it, its default prevented; a click is a pick.
  ["pointerdown", "pointerup", "mousedown", "mouseup", "click", "dblclick", "auxclick", "contextmenu"].forEach(function (type) {
    window.addEventListener(
      type,
      function (ev) {
        if (!annot.on) return;
        ev.stopImmediatePropagation();
        if (type.indexOf("pointer") === 0) return;
        ev.preventDefault();
        if (type === "click" && !isPage(ev.target)) pickEl(ev.target);
      },
      true
    );
  });
  // The walk's keys, pressed inside the page: the host's.
  window.addEventListener(
    "keydown",
    function (ev) {
      if (!annot.on || ev.isComposing || ev.ctrlKey || ev.metaKey || ev.altKey || !WALK_KEYS.test(ev.key)) return;
      var t = ev.composedPath ? ev.composedPath()[0] : ev.target;
      if (t && t.nodeType === 1 && (/^(input|textarea|select)$/i.test(t.tagName) || t.isContentEditable)) return;
      ev.preventDefault();
      ev.stopImmediatePropagation();
      post("menger:key", { key: ev.key });
    },
    true
  );
})();
