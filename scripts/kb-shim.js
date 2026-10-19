// CHAT-42: an iPhone's on-screen keyboard, for a headless WebKit that has
// none. Loaded before the app (Playwright's addInitScript, with
// window.__kbConf = { k, ms } set first); scripts/kb-check.mjs drives it.
//
// The model, iOS Safari as the owner's phone shows it:
//
// - The page is laid out in the layout viewport (the browser's real one,
//   390 × 844 here). The keyboard does not shrink it: it shrinks the
//   *visual* viewport by its height (k), and window.innerHeight follows the
//   visual one (APP-62).
// - Where the reader sees is `pageTop`: the visible area's top in the
//   document. The layout viewport sits inside the document (scrollY =
//   clamp(pageTop, 0, docH − layoutH)); the visual one is panned within it
//   (offsetTop = clamp(pageTop − scrollY, 0, layoutH − vvH)).
// - A field that takes typing, focused by a tap (iOS opens no keyboard
//   for a script's focus — the chat's composer focuses itself as it
//   mounts), or tapped while it has focus, raises the keyboard over `ms` and,
//   in the same animation, scrolls pageTop to bring the field above it — a
//   target computed once, at focus, from the document as it was then. iOS
//   runs that scroll natively and does not clamp it again when the document
//   shrinks under it.
// - A script's scroll moves the layout viewport (clamped to the document)
//   and carries the visual viewport with it, its pan kept — so it cannot
//   bring back a visible area left past a document that shrank. A
//   visible area past the document's end shows the page's background —
//   the owner's screenshot (CHAT-42).
// - The keyboard going: the visual viewport grows back over `ms`, and
//   pageTop is clamped to the document.
//
// What it reads back: window.__kb.state() — the true visible area in the
// layout viewport's coordinates (`top`, `bottom`), what the page was told
// (offsetTop, scrollY, innerHeight) and the keyboard's height now.
(() => {
  const conf = window.__kbConf || {};
  const K = conf.k ?? 336;
  const MS = conf.ms ?? 300;
  const se = () => document.scrollingElement || document.documentElement;
  const layoutH = () => document.documentElement.clientHeight;
  const docH = () => se().scrollHeight;
  const realY = () => se().scrollTop;
  let kb = 0; // the keyboard's height now
  let anim = null; // { t0, from, to, k0, k1 }
  let pageTop = 0;
  let up = false; // a field that takes typing has focus
  let told = { h: 0, top: 0, y: 0 };
  const clamp = (v, a, b) => Math.max(a, Math.min(b, v));
  const vvH = () => layoutH() - kb;
  const offsetTop = () => clamp(pageTop - realY(), 0, Math.max(0, layoutH() - vvH()));
  const reportedY = () => (kb > 0 || anim ? pageTop - offsetTop() : realY());

  const vv = new EventTarget();
  for (const [k, get] of Object.entries({
    width: () => document.documentElement.clientWidth,
    height: vvH,
    offsetTop: () => (kb > 0 || anim ? offsetTop() : 0),
    offsetLeft: () => 0,
    pageTop: () => (kb > 0 || anim ? pageTop : realY()),
    pageLeft: () => 0,
    scale: () => 1,
  })) Object.defineProperty(vv, k, { get, enumerable: true });
  vv.onresize = null;
  vv.onscroll = null;
  Object.defineProperty(window, "visualViewport", { get: () => vv, configurable: true });
  Object.defineProperty(window, "innerHeight", { get: () => vvH(), configurable: true });
  Object.defineProperty(window, "scrollY", { get: reportedY, configurable: true });
  Object.defineProperty(window, "pageYOffset", { get: reportedY, configurable: true });

  // A script's scroll: the layout viewport moves, the pan with it.
  const realTo = window.scrollTo.bind(window);
  const jsScroll = (y) => {
    if (kb > 0 || anim) {
      const docMax = Math.max(0, docH() - layoutH());
      pageTop = clamp(y, 0, docMax) + (pageTop - clamp(pageTop, 0, docMax));
      place();
    } else {
      realTo(0, y);
      pageTop = realY();
    }
  };
  const target = (a, b) => (typeof a === "object" && a ? a.top ?? reportedY() : b ?? reportedY());
  window.scrollTo = window.scroll = (a, b) => jsScroll(target(a, b));
  window.scrollBy = (a, b) => jsScroll(reportedY() + (typeof a === "object" && a ? a.top ?? 0 : b ?? 0));

  // The layout viewport follows pageTop inside the document.
  let placing = false;
  function place() {
    const L = clamp(pageTop, 0, Math.max(0, docH() - layoutH()));
    if (Math.abs(realY() - L) > 0.5) {
      placing = true;
      realTo(0, L);
      placing = false;
    }
    lastL = realY();
  }
  // The document's scroll set directly (scrollingElement.scrollTop): with
  // the keyboard up, as a script's scroll — the pan kept.
  let lastL = 0;
  window.addEventListener("scroll", () => {
    if (placing) return;
    if (!(kb > 0 || anim)) pageTop = realY();
    else if (Math.abs(realY() - lastL) > 0.5) {
      const docMax = Math.max(0, docH() - layoutH());
      pageTop = realY() + (pageTop - clamp(pageTop, 0, docMax));
    }
    lastL = realY();
  }, true);

  // A sandboxed frame without allow-same-origin is an opaque origin: never
  // touched (WebKit reports each try as a page error, caught or not).
  const opaque = (f) => f.hasAttribute("sandbox") && !/\ballow-same-origin\b/.test(f.getAttribute("sandbox"));
  function deepActive() {
    let a = document.activeElement;
    while (a && a.tagName === "IFRAME" && !opaque(a)) {
      try { a = a.contentDocument.activeElement; } catch { break; }
    }
    return a;
  }
  // APP-83: a body's frame is sandboxed (an opaque origin): its focused
  // field is out of this script's reach, as it is out of the app's. iOS
  // knows; here the check says so — __kb.frameField(rect) after tapping a
  // field in the frame (its rect in the top layout viewport), null after.
  let ext = null;
  const opaqueFocused = () => {
    const a = document.activeElement;
    return !!a && a.tagName === "IFRAME" && opaque(a);
  };
  function typing(el) {
    if (!el) return false;
    const t = el.tagName;
    if (t === "TEXTAREA") return !el.readOnly;
    if (t === "INPUT") return /^(|text|search|email|url|tel|password|number)$/.test((el.getAttribute("type") || "").toLowerCase()) && !el.readOnly;
    return !!el.isContentEditable;
  }
  // The field's rect in the top layout viewport (through its frames).
  function rectOf(el) {
    const r = el.getBoundingClientRect();
    let top = r.top, bottom = r.bottom;
    let w = el.ownerDocument.defaultView;
    while (w && w !== window && w.frameElement) {
      const f = w.frameElement.getBoundingClientRect();
      top += f.top; bottom += f.top;
      w = w.parent;
    }
    return { top, bottom };
  }

  function start(show) {
    const el = deepActive();
    const from = pageTop;
    let to = from;
    if (show && el) {
      const r = ext && el.tagName === "IFRAME" ? ext : rectOf(el);
      const L = realY();
      const vis = layoutH() - K;
      const top = L + r.top, bottom = L + r.bottom;
      if (bottom > from + vis - 8) to = bottom - vis + 8;
      else if (top < from) to = top - 8;
      to = clamp(to, 0, Math.max(0, docH() - vis));
    }
    anim = { t0: performance.now(), from, to, k0: kb, k1: show ? K : 0 };
  }

  function fire() {
    const now = { h: vvH(), top: vv.offsetTop, y: reportedY() };
    if (now.h !== told.h) vv.dispatchEvent(new Event("resize"));
    if (now.top !== told.top) vv.dispatchEvent(new Event("scroll"));
    if (now.y !== told.y) window.dispatchEvent(new Event("scroll"));
    if (now.h !== told.h) window.dispatchEvent(new Event("resize"));
    told = now;
  }

  // When a finger last lifted, and on what: a focus within TAP_MS of it is
  // the reader's.
  const TAP_MS = 800;
  let tapAt = -1e9;
  const tapped = () => { tapAt = performance.now(); };
  for (const t of ["touchend", "mouseup", "click"]) window.addEventListener(t, tapped, true);
  const frames = new WeakSet();
  function watchFrames() {
    for (const f of document.querySelectorAll("iframe")) {
      if (frames.has(f)) continue;
      if (opaque(f)) { frames.add(f); continue; }
      try {
        const w = f.contentWindow;
        if (!w || !w.document || w.document.readyState === "loading") continue;
        for (const t of ["touchend", "mouseup", "click"]) w.addEventListener(t, tapped, true);
        frames.add(f);
      } catch { frames.add(f); }
    }
  }

  function frame() {
    watchFrames();
    const focused = typing(deepActive()) || (!!ext && opaqueFocused());
    const want = focused && (up || performance.now() - tapAt < TAP_MS);
    if (want !== up) {
      up = want;
      start(want);
    }
    if (anim) {
      const p = Math.min(1, (performance.now() - anim.t0) / MS);
      const e = 1 - (1 - p) ** 3;
      kb = anim.k0 + (anim.k1 - anim.k0) * e;
      pageTop = anim.from + (anim.to - anim.from) * e;
      // Going down, the visual viewport grows inside the layout one: its
      // pan shrinks with it.
      if (anim.k1 === 0) {
        const L = clamp(pageTop, 0, Math.max(0, docH() - layoutH()));
        pageTop = L + clamp(pageTop - L, 0, layoutH() - vvH());
        anim.from = pageTop; anim.to = pageTop;
      }
      if (p >= 1) {
        kb = anim.k1;
        if (kb === 0) pageTop = clamp(pageTop, 0, Math.max(0, docH() - layoutH()));
        anim = null;
      }
    }
    if (kb > 0 || anim) place();
    fire();
    requestAnimationFrame(frame);
  }
  requestAnimationFrame(frame);

  window.__kb = {
    state: () => {
      const L = realY();
      return { top: pageTop - L, bottom: pageTop - L + vvH(), kb, up, pageTop, scrollY: reportedY(), realY: L, offsetTop: vv.offsetTop, innerHeight: vvH(), docH: docH(), layoutH: layoutH() };
    },
    // A finger's drag on the page with the keyboard down (the reader
    // scrolling the thread): the real document moves.
    drag: (dy) => { realTo(0, realY() + dy); pageTop = realY(); },
    // APP-83: a field inside an opaque frame took focus (its rect), or left.
    frameField: (r) => { ext = r ? { top: r.y, bottom: r.y + r.height } : null; if (r) tapAt = performance.now(); },
  };
})();
