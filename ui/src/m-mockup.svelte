<!-- COMPS-3: `<m-mockup>` — a full page in a body, drawn as itself,
     interactive, in a sandboxed frame at its own height: an ordinary
     svelte/1 component on plexus-ui's frame API (`bus.frame`), mounted where
     the body has the tag. The owner, 2026-11-05: "adding m-mockups actual
     implementation should be a plexus-ui svelte component". ticket-view keeps
     no frame code: it draws a page piece by its tag's component (its `TAGS`
     table) — this one, `m-live` (the test app) or `m-page` (a body of raw
     HTML, framed whole through this one). EDITOR-14 (the owner: "we currently
     don't support the same level of tickets rendering full iframes. full
     pages inside. we need to support that", and "also you know that v1 could
     just host raw html inside a frame right?"); v1 drew these (tag v0.0.1,
     frontend/src/md.rs `is_framed`, mockup.rs, frame.rs).
     What it draws (`source`, the piece's markdown):
     - `<m-mockup label|title|description|width|height|widths>…a page…</m-mockup>`
       (v1's form, as stored), and the fences ```mockup / ```page, or ```html
       with a page's attributes (`label`, `title`, `width`, `height`,
       `widths="390,1440"`, or the bare word `page`) — the page in a frame;
     - a body that is a whole page (`<!doctype html>` …) — one frame, its
       widths from `<meta name="menger-widths">`, its label its `<title>`;
     - `<m-mockup ref="X">` — a link to X, whose body is the page;
     - `doc` (m-page's): a body holding raw HTML the page cannot draw
       (`holdsRaw` below), its frame document built by m-page, drawn whole
       (`data-kind=body`): the app's tokens in its head and sent again when
       the theme flips, popups that escape the sandbox, so a link opens a tab.
     The parsing below (`pageSpec`, `holdsRaw`, …) is ticket-view's and
     m-live's too. Every framed page carries `data-surface=page-frame`.

     COMPS-2: the frame is plexus-ui's — the engine's `bus.frame` (its
     CONTRACT.md "Frames"), on the service's `/frame`. `<iframe
     sandbox="allow-scripts allow-forms">`, never `allow-same-origin` — an
     opaque origin, so the page's scripts run but read no cookie or storage
     of the app, reach no socket and cannot touch `parent` or `top`.
     `allow-forms` because Chrome fires no `submit` in a sandbox without it,
     so a form a script handles would be dead; a real submission is still
     refused, by the frame's `form-action 'none'`. The engine does the
     handshake (a message taken only from that frame's own `contentWindow`
     carrying its channel), the height (the content's, at most MAX px, then
     the frame scrolls inside; a page that grows with its frame is held
     after a few steps), the theme (a body's: the app's tokens, again on a
     flip) and the double buffer (a changed page — an amend, an edit, a
     width switched — loads in a second, hidden frame that replaces the
     first once it says its height, so nothing flashes). And the bridge: a
     framed page's `plexus.call(method, params)` reads the tracker as the
     reader — `READS` below, kept only where the component drawing the
     ticket holds it too (EDITOR-15, v1's `menger.call`); anything else is
     refused in words. A body's ```mermaid fences are drawn before it is
     handed over (`prepare`, m-page's), so a diagram draws inside the
     frame as outside. Here: which pages are frames, a width the page names
     drawn at that width and scaled down to the column (EDITOR-12: nothing
     wider than the column on a phone), with a switch when it names
     several; COMPS-17: a named width is a device — its screen's height by
     default (844, 1024, 900), its body drawn around the frame
     (`[data-device=phone|tablet|screen]`), a longer page scrolling inside;
     COMPS-19: a window — `[data-field=window]`'s w and h and the corner
     `[data-action=resize]` set the viewport by hand (to 4,000 high), drawn
     only under the pointer or the focus; the label and the description above the frame, never inside
     it; a frame that never says it is up (an older server) a note and the
     source. -->
<script module>
  export const uses = [];

  /** The frame's height at most, in the page's own pixels. */
  export const MAX = 2000;
  /** COMPS-2: what a framed page may read through the bridge (`plexus.call`) — held, by the engine, to what the
   *  component drawing the ticket holds too, and to the reader's grants by the service. */
  export const READS = ["tickets.get", "tickets.list", "tickets.search"];

  const ATTR = /([a-zA-Z][\w:-]*)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'>]+)))?/g;
  /** `key=value`, `key="a value"`, `key='…'` and bare words, as an object (keys lower-cased). */
  export function attrsOf(s) {
    const out = {};
    for (const m of String(s ?? "").matchAll(ATTR)) out[m[1].toLowerCase()] = m[2] ?? m[3] ?? m[4] ?? "";
    return out;
  }

  const PAGE_ATTRS = ["label", "title", "description", "width", "height", "widths", "page"];
  /** A fence's info string as a page — ```mockup, ```page, ```live, or ```html with a page's attributes — else null. */
  export function fencePage(info) {
    const m = /^\s*([\w-]+)(.*)$/.exec(String(info ?? ""));
    if (!m) return null;
    const lang = m[1].toLowerCase(), attrs = attrsOf(m[2]);
    if (lang === "mockup" || lang === "page") return { kind: "page", attrs };
    if (lang === "live") return { kind: "live", attrs };
    if (lang === "html" && PAGE_ATTRS.some((k) => k in attrs)) return { kind: "page", attrs };
    return null;
  }

  /** A line that opens a page element: `<m-mockup …>` or `<m-live …>`. */
  export const PAGE_TAG = /^ {0,3}<(m-mockup|m-live)(\s[^>]*)?>/i;
  /** A body that is a whole page: `<!doctype html>` or `<html>` first (after comments and blank lines). */
  export const WHOLE = /^\s*(?:<!--[\s\S]*?-->\s*)*<(?:!doctype\s+html|html[\s>])/i;

  /** The line after the page element opened on line `i` closes (`</m-mockup>`, on that line or a later one), or the end. */
  export function pageEnd(lines, i) {
    const tag = PAGE_TAG.exec(lines[i])[1].toLowerCase();
    const close = `</${tag}>`;
    if (lines[i].toLowerCase().includes(close)) return i + 1;
    for (let k = i + 1; k < lines.length; k++) if (lines[k].toLowerCase().includes(close)) return k + 1;
    return lines.length;
  }

  // The engine's sanitizer (../plexus-ui/engine/src/html/mod.rs): the tags it keeps and the attributes it keeps on them.
  const KEPT = new Set(["a", "abbr", "b", "blockquote", "br", "code", "dd", "del", "div", "dl", "dt", "em", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "kbd", "li",
    "mark", "ol", "p", "pre", "q", "s", "small", "span", "strong", "sub", "sup", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "u", "ul"]);
  const KEPT_ATTRS = new Set(["a href", "code class", "img alt", "img height", "img src", "img width", "ol start", "pre class", "td colspan", "td rowspan", "th colspan", "th rowspan"]);
  // HTML's own elements beyond those (a placeholder like `<org>` is none, so it never frames a body).
  const ELEMENTS = new Set(("address area article aside audio base bdi bdo body button canvas caption cite col colgroup data datalist details dfn dialog embed fieldset "
    + "figcaption figure footer form head header hgroup html iframe input ins label legend link main map menu meta meter nav noscript object optgroup option "
    + "output picture progress rp rt ruby samp script search section select slot source style summary svg math template textarea time title track var video wbr "
    + "center font marquee").split(" "));

  /** EDITOR-14: whether markdown `text` holds raw HTML the page cannot draw: outside code spans (a fence is the
   *  caller's to skip), a tag naming an HTML element the sanitizer drops, or one it keeps with an attribute it
   *  drops (`<div class>`, `<span style>`). The house's marks (`m-*`), custom tags and comments never do. */
  export function holdsRaw(text) {
    const src = String(text ?? "").replace(/(`+)[\s\S]*?\1/g, "");
    for (const m of src.matchAll(/<(\/?)([a-zA-Z][a-zA-Z0-9-]*)(\s[^<>]*?)?\/?>/g)) {
      const name = m[2].toLowerCase();
      if (name.includes("-")) continue;
      if (!KEPT.has(name)) { if (ELEMENTS.has(name)) return true; continue; }
      if (m[1]) continue;
      for (const a of Object.keys(attrsOf(m[3] ?? ""))) if (!KEPT_ATTRS.has(`${name} ${a}`)) return true;
    }
    return false;
  }

  /** What `text` (one piece of a body) draws as a page: `{ kind: "page" | "live" | "ref", attrs, html }`, else null. */
  export function pageSpec(text) {
    const src = String(text ?? "");
    const fm = /^ {0,3}(`{3,}|~{3,})([^\n]*)(?:\n|$)/.exec(src);
    if (fm) {
      const f = fencePage(fm[2]);
      if (!f) return null;
      const fence = fm[1][0] === "`" ? "`" : "~";
      const html = src.slice(fm[0].length).replace(new RegExp(`\\n? {0,3}\\${fence}{${fm[1].length},}\\s*$`), "");
      return { ...f, html };
    }
    const tm = PAGE_TAG.exec(src);
    if (tm) {
      const kind = tm[1].toLowerCase() === "m-live" ? "live" : "page";
      const attrs = attrsOf(tm[2]);
      const rest = src.slice(tm[0].length);
      const end = rest.toLowerCase().indexOf(`</${tm[1].toLowerCase()}>`);
      const html = (end >= 0 ? rest.slice(0, end) : rest).replace(/^[ \t]*\n/, "");
      if (kind === "page" && attrs.ref && !html.trim()) return { kind: "ref", attrs, html: "" };
      return { kind, attrs, html };
    }
    if (WHOLE.test(src)) {
      const attrs = {};
      const meta = (name) => new RegExp(`<meta\\s+name=["']?${name}["']?\\s+content=["']([^"']*)["']`, "i").exec(src)?.[1];
      const widths = meta("menger-widths");
      if (widths) attrs.widths = widths;
      const title = /<title[^>]*>([^<]*)<\/title>/i.exec(src)?.[1]?.trim();
      if (title) attrs.title = title;
      return { kind: "page", attrs, html: src, whole: true };
    }
    return null;
  }

  /** The widths a page names — `widths="390,1440"` (each `W` or `WxH`, at most 6), else `width` (and `height`) —
   *  as `[{ w, h }]`, `h` null when the content decides; none: the column's width. */
  export function widthsOf(attrs) {
    const n = (v) => { const x = Math.round(Number(v)); return Number.isFinite(x) && x > 0 ? x : null; };
    const fit = (w) => Math.min(2560, Math.max(200, w));
    const out = [];
    if (attrs?.widths) {
      for (const part of String(attrs.widths).split(/[,\s]+/)) {
        const m = /^(\d+)(?:x(\d+))?$/i.exec(part.trim());
        if (m && n(m[1])) out.push({ w: fit(n(m[1])), h: m[2] ? Math.min(MAX, n(m[2]) ?? 0) || null : null });
        if (out.length === 6) break;
      }
    }
    if (!out.length && n(attrs?.width)) out.push({ w: fit(n(attrs.width)), h: null });
    return out;
  }

  /** The height a frame starts at, before its page says its own: a device's screen at a named width. */
  export const startHeight = (w, named) => (!named ? 320 : w <= 480 ? 844 : w <= 1024 ? 1024 : 900);
  /** COMPS-17: the device a named width is drawn in — its body around the frame. */
  export const deviceOf = (w) => (w <= 480 ? "phone" : w <= 1024 ? "tablet" : "screen");
  /** The device's edge, in the page's own pixels (it scales with the frame). */
  const EDGE = { phone: 12, tablet: 14, screen: 10 };
</script>

<script>
  import { untrack } from "svelte";
  import { bus as member } from "plexus-ui/host";
  import { ticketHref } from "./ticket-links.svelte";

  const bus = member();

  /** `source`: the piece's markdown. `doc`: a body framed whole (m-page built it), its kind `body`;
   *  `prepare(html, within)` → a promise of the html to hand over (m-page draws its diagrams, in `within`). */
  let { source = "", doc = null, prepare = null } = $props();
  const spec = $derived(doc != null ? { kind: "body", attrs: {}, html: doc } : pageSpec(source));
  const sizes = $derived(widthsOf(spec?.attrs ?? {}));
  let pick = $state(0);
  const size = $derived(sizes[Math.min(pick, Math.max(0, sizes.length - 1))] ?? null);
  const label = $derived(spec?.attrs?.label || spec?.attrs?.title || "");
  const about = $derived(spec?.attrs?.description || "");
  let col = $state(0);
  let phase = $state("loading"); // loading · drawn · failed
  let said = $state(null); // the content's height, as the frame said it

  // COMPS-19 (the owner: "I want the mockup viewport to have a defined height, settable … implicit windows
  // (don't render it unless we mouse over) where we can adjust the size"): a size set by hand — typed in the
  // window's bar or dragged by its corner — over the one the body names. `drag` is the size while the corner
  // is held: only the box follows it, and the page is handed its new viewport when the drag ends.
  const LIMIT = 4000;
  let own = $state(null);
  let drag = $state(null);
  const sized = $derived(!!(size || own));
  const W = $derived(own?.w ?? (size ? size.w : Math.max(1, col)));
  // COMPS-17 (the owner: "default to the height of the viewport set inside the mockup. it should show it as if
  // it's inside a phone"): a named width is a device — its screen's height unless the page names one, its body
  // drawn around the frame, and a longer page scrolls inside it.
  const device = $derived(sized ? deviceOf(W) : null);
  const edge = $derived(device ? EDGE[device] : 0);
  const scale = $derived(sized && col > 0 ? Math.min(1, col / ((drag?.w ?? W) + 2 * edge)) : 1);
  const framed = $derived(spec?.kind === "page" || spec?.kind === "body");
  // A height the page names (`WxH`, or `height=`) is kept; else the device's screen at a named width; else the content's.
  const fixed = $derived(own?.h ?? size?.h ?? (Number(spec?.attrs?.height) > 0 ? Math.round(Number(spec.attrs.height)) : size ? startHeight(size.w, true) : null));
  const H = $derived(Math.max(40, Math.min(own ? LIMIT : MAX, fixed ?? said ?? startHeight(W, !!size))));
  const Wv = $derived(drag?.w ?? W), Hv = $derived(drag?.h ?? H);
  const fitW = (v) => Math.min(2560, Math.max(200, Math.round(Number(v) || 0)));
  const fitH = (v) => Math.min(LIMIT, Math.max(120, Math.round(Number(v) || 0)));
  const setSize = (w, h) => { own = { w: fitW(w), h: fitH(h) }; };
  function grab(ev) {
    const x0 = ev.clientX, y0 = ev.clientY, w0 = W, h0 = H, k = scale || 1, el = ev.currentTarget;
    el.setPointerCapture(ev.pointerId);
    const move = (e) => { drag = { w: fitW(w0 + (e.clientX - x0) / k), h: fitH(h0 + (e.clientY - y0) / k) }; };
    const up = () => { el.removeEventListener("pointermove", move); el.removeEventListener("pointerup", up); el.removeEventListener("pointercancel", up); if (drag) setSize(drag.w, drag.h); drag = null; };
    el.addEventListener("pointermove", move); el.addEventListener("pointerup", up); el.addEventListener("pointercancel", up);
    ev.preventDefault();
  }
  function nudge(ev) {
    const d = { ArrowRight: [20, 0], ArrowLeft: [-20, 0], ArrowDown: [0, 20], ArrowUp: [0, -20] }[ev.key];
    if (d) { ev.preventDefault(); setSize(W + d[0], H + d[1]); }
  }

  // COMPS-2: the frame, the engine's — made once its stage is drawn, handed every new page after.
  let stage = $state(null);
  let f = null, asked = 0;
  $effect(() => {
    if (!framed || !stage) return;
    const html = spec.html, body = spec.kind === "body", width = sized ? W : undefined, height = fixed ?? undefined, start = startHeight(W, !!size), title = label || (body ? "the body" : "a page");
    untrack(() => {
      const n = ++asked;
      const hand = (page) => {
        if (n !== asked) return;
        if (!f) {
          f = bus.frame(stage, { html: page, width, height, max: LIMIT, start, title, popups: body, theme: body, uses: READS,
            onsize: (h) => (said = h), onstate: (s) => { if (s !== "loading") phase = s; } });
        } else f.update({ html: page, width, height });
      };
      if (body && prepare) Promise.resolve(prepare(html, stage)).then(hand, () => hand(html));
      else hand(html);
    });
  });
  $effect(() => () => { f?.destroy(); f = null; });
</script>

{#if spec && spec.kind !== "live"}
<figure class="pf" data-surface="page-frame" data-kind={spec.kind} data-state={framed ? phase : "note"} data-width={size?.w ?? undefined} data-device={device ?? undefined}>
  {#if label || about || sizes.length > 1}
    <figcaption class="pf-head">
      {#if label}<span class="pf-label" data-field="label">{label}</span>{/if}
      {#if sizes.length > 1}
        <span class="pf-widths" role="group" aria-label="width">
          {#each sizes as s, i (i)}<button type="button" data-action="width" data-width={s.w} aria-pressed={i === pick && !own} onclick={() => { pick = i; own = null; }}>{s.w}{s.h ? `×${s.h}` : ""}</button>{/each}
        </span>
      {/if}
      {#if about}<span class="pf-about" data-field="description">{about}</span>{/if}
    </figcaption>
  {/if}
  {#if spec.kind === "ref"}
    <p class="pf-note" data-field="note">The page is <a href={ticketHref({ id: encodeURIComponent(spec.attrs.ref) })}>{spec.attrs.ref}</a>'s body; open it there.</p>
  {:else}
    <div class="pf-stage" bind:clientWidth={col}>
      <div class="pf-box" class:bare={spec.kind === "body" || !!device} class:held={!!drag}
           style:width={sized ? `${Math.round((Wv + 2 * edge) * scale)}px` : "100%"} style:height={`${Math.round((Hv + 2 * edge) * scale)}px`}>
        <div class="pf-dev" data-device={device ?? undefined} style:width={sized ? `${Wv + 2 * edge}px` : "100%"} style:padding={edge ? `${edge}px` : null} style:transform={scale < 1 ? `scale(${scale})` : null}>
          <div class="pf-frames" bind:this={stage} style:width={sized ? `${Wv}px` : "100%"} style:height={`${Hv}px`}></div>
        </div>
        {#if framed}<button type="button" class="pf-grip" data-action="resize" aria-label="Resize the viewport (arrow keys move it by 20)" onpointerdown={grab} onkeydown={nudge}></button>{/if}
      </div>
      {#if framed}
        <div class="pf-win" data-field="window" class:held={!!drag}>
          <label>w <input type="number" data-field="w" min="200" max="2560" step="10" value={Wv} onchange={(e) => setSize(e.currentTarget.value, H)}/></label>
          <label>h <input type="number" data-field="h" min="120" max={LIMIT} step="10" value={Hv} onchange={(e) => setSize(W, e.currentTarget.value)}/></label>
          {#if scale < 1}<span data-field="scale">{Math.round(scale * 100)}%</span>{/if}
          {#if own}<button type="button" data-action="reset-size" onclick={() => (own = null)}>reset</button>{/if}
        </div>
      {/if}
    </div>
    {#if phase === "failed"}
      <p class="pf-note" data-field="note">This page could not be drawn here; its source is below.</p>
      <details class="pf-src"><summary>the page's source</summary><pre>{spec.kind === "body" ? source : spec.html}</pre></details>
    {/if}
  {/if}
</figure>
{/if}

<style>
  .pf { display: block; margin: 8px 0; min-width: 0; max-width: 100%; }
  .pf-head { display: flex; flex-wrap: wrap; align-items: baseline; gap: 4px 10px; margin: 0 0 6px; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .pf-label { color: var(--ink); font-weight: var(--weight-strong); overflow-wrap: anywhere; }
  .pf-about { flex-basis: 100%; overflow-wrap: anywhere; }
  .pf-widths { display: inline-flex; gap: 4px; }
  .pf-widths button { padding: 0 7px; font: inherit; font-size: 11px; line-height: 18px; color: var(--muted); background: none; border: var(--border) solid var(--line); border-radius: var(--radius-md); cursor: pointer; }
  .pf-widths button[aria-pressed=true], .pf-widths button:global([data-sheet~=checked]) { color: var(--ink); border-color: var(--muted); background: var(--panel); }
  .pf-stage { width: 100%; min-width: 0; }
  .pf-box { position: relative; max-width: 100%; overflow: hidden; border: var(--border) solid var(--line); border-radius: var(--radius); background: var(--panel); }
  .pf-box.bare { border: 0; border-radius: 0; background: none; }
  .pf-dev { box-sizing: border-box; transform-origin: 0 0; }
  /* COMPS-19: the window — its bar and its corner are drawn only while the pointer is over the mockup, a
     control of it has the focus, or its corner is held; always where there is no pointer to hover with. */
  .pf-win { display: flex; gap: 10px; align-items: center; height: 26px; font-family: var(--font-text); font-size: 11px; color: var(--muted); opacity: 0; }
  .pf-win label { display: inline-flex; gap: 4px; align-items: center; }
  .pf-win input { width: 7ch; padding: 1px 4px; font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .pf-win button { padding: 0 7px; font: inherit; line-height: 18px; color: var(--muted); background: none; border: var(--border) solid var(--line); border-radius: var(--radius-md); cursor: pointer; }
  .pf-win button:hover { color: var(--ink); }
  .pf-grip { position: absolute; right: 0; bottom: 0; width: 18px; height: 18px; padding: 0; border: 0; border-radius: var(--radius-sm) 0 0 0; background: var(--accent); cursor: nwse-resize; opacity: 0; touch-action: none; }
  .pf:hover .pf-win, .pf:focus-within .pf-win, .pf-win.held, .pf:hover .pf-grip, .pf-grip:focus-visible, .pf-box.held .pf-grip { opacity: 1; }
  .pf-grip:focus-visible { outline: none; box-shadow: 0 0 0 2px var(--focus); }
  .pf-box.held :global(iframe) { pointer-events: none; }
  @media (hover: none) { .pf-win, .pf-grip { opacity: 1; } }
  .pf-frames { position: relative; }
  /* COMPS-17: the device's body — the frame is its screen; it scales with the frame. */
  .pf-dev[data-device] { background: var(--ink); }
  .pf-dev[data-device] .pf-frames { overflow: hidden; background: var(--panel); }
  .pf-dev[data-device=phone] { border-radius: 40px; }
  .pf-dev[data-device=phone] .pf-frames { border-radius: 28px; }
  .pf-dev[data-device=tablet] { border-radius: 26px; }
  .pf-dev[data-device=tablet] .pf-frames { border-radius: 12px; }
  .pf-dev[data-device=screen] { border-radius: 12px; }
  .pf-dev[data-device=screen] .pf-frames { border-radius: 3px; }
  .pf-note { margin: 0; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .pf-src summary { cursor: pointer; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .pf-src pre { max-height: 320px; overflow: auto; margin: 4px 0 0; padding: 8px 10px; font-family: var(--font-text); font-size: 12px; white-space: pre-wrap; overflow-wrap: anywhere; background: var(--panel); border-radius: var(--radius); }
</style>
