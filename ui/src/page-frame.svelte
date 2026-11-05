<!-- EDITOR-14: a full page in a body, drawn as itself — interactive, in a
     sandboxed frame at its own height. The owner, 2026-11-05: "we currently
     don't support the same level of tickets rendering full iframes. full
     pages inside. we need to support that", and "also you know that v1 could
     just host raw html inside a frame right?". v1 drew these (tag v0.0.1,
     frontend/src/md.rs `is_framed`, mockup.rs, frame.rs); v2 had cut them.
     ticket-view draws a piece through this component when it is a page:

     - `<m-mockup label|title|description|width|height|widths>…a page…</m-mockup>`
       (v1's form, as stored), and the fences ```mockup / ```page, or ```html
       with a page's attributes (`label`, `title`, `width`, `height`,
       `widths="390,1440"`, or the bare word `page`) — the page in a frame;
     - a body that is a whole page (`<!doctype html>` …) — one frame, its
       widths from `<meta name="menger-widths">`, its label its `<title>`;
     - a body holding raw HTML the page cannot draw (`holdsRaw` below: an HTML
       element outside the sanitizer's allow-list, or an allowed one with an
       attribute it drops) — the whole body as one frame (`doc`, built by
       ticket-view: the markdown around the raw HTML, a page block inside it a
       nested frame), the app's tokens in its head and sent again when the
       theme flips, `<base target="_blank">` and popups that escape the
       sandbox, so a link opens a tab;
     - `<m-live route widths>` and ```live (APP-110) — the test app at that
       route, from the origin the shell stamps (`--live-origin`,
       `<meta name="menger-live-origin">`); none configured, or `env=` naming
       another, says so in words;
     - `<m-mockup ref="X">` — a link to X, whose body is the page.

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
     handed over (`prepare`, ticket-view), so a diagram draws inside the
     frame as outside. Here: which pages are frames, a width the page names
     drawn at that width and scaled down to the column (EDITOR-12: nothing
     wider than the column on a phone), with a switch when it names
     several; the label and the description above the frame, never inside
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

  /** The live origin this server's bodies may frame (APP-110), stamped by the shell; "" when none. */
  const liveOrigin = () => (typeof document === "object" && document.querySelector('meta[name="menger-live-origin"]')?.content?.trim()) || "";
</script>

<script>
  import { untrack } from "svelte";
  import { bus as member } from "plexus-ui/host";

  const bus = member();

  /** `source`: the piece's markdown. `doc`: a body framed whole (ticket-view built it), its kind `body`;
   *  `prepare(html, within)` → a promise of the html to hand over (ticket-view draws its diagrams, in `within`). */
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

  const W = $derived(size ? size.w : Math.max(1, col));
  const scale = $derived(size && col > 0 ? Math.min(1, col / size.w) : 1);
  const framed = $derived(spec?.kind === "page" || spec?.kind === "body");
  // A height the page names (`WxH`, or `height=`) is kept; else the content's.
  const fixed = $derived(size?.h ?? (Number(spec?.attrs?.height) > 0 ? Math.round(Number(spec.attrs.height)) : null));
  const H = $derived(Math.max(40, Math.min(MAX, fixed ?? said ?? startHeight(W, !!size))));

  // COMPS-2: the frame, the engine's — made once its stage is drawn, handed every new page after.
  let stage = $state(null);
  let f = null, asked = 0;
  $effect(() => {
    if (!framed || !stage) return;
    const html = spec.html, body = spec.kind === "body", width = size?.w, height = fixed ?? undefined, start = startHeight(W, !!size), title = label || (body ? "the body" : "a page");
    untrack(() => {
      const n = ++asked;
      const hand = (page) => {
        if (n !== asked) return;
        if (!f) {
          f = bus.frame(stage, { html: page, width, height, max: MAX, start, title, popups: body, theme: body, uses: READS,
            onsize: (h) => (said = h), onstate: (s) => { if (s !== "loading") phase = s; } });
        } else f.update({ html: page, width, height });
      };
      if (body && prepare) Promise.resolve(prepare(html, stage)).then(hand, () => hand(html));
      else hand(html);
    });
  });
  $effect(() => () => { f?.destroy(); f = null; });

  // The live view: test's app at the route, or why not.
  const live = $derived.by(() => {
    if (spec?.kind !== "live") return null;
    const env = (spec.attrs.env || "test").toLowerCase();
    if (env !== "test") return { refused: `A live view draws the test app only; env=${env} is refused.` };
    const origin = liveOrigin();
    if (!origin) return { refused: "No live origin is configured on this server (--live-origin), so the test app is not drawn here." };
    if (typeof location === "object" && origin === location.origin) return { refused: "The live origin is this app's own, which is never framed with its scripts." };
    const route = spec.attrs.route || "#/";
    return { src: `${origin}/${route.replace(/^\//, "")}` };
  });
</script>

{#if spec}
<figure class="pf" data-surface="page-frame" data-kind={spec.kind} data-state={framed ? phase : live?.src ? "drawn" : "note"} data-width={size?.w ?? undefined}>
  {#if label || about || sizes.length > 1}
    <figcaption class="pf-head">
      {#if label}<span class="pf-label" data-field="label">{label}</span>{/if}
      {#if sizes.length > 1}
        <span class="pf-widths" role="group" aria-label="width">
          {#each sizes as s, i (i)}<button type="button" data-action="width" data-width={s.w} aria-pressed={i === pick} onclick={() => (pick = i)}>{s.w}{s.h ? `×${s.h}` : ""}</button>{/each}
        </span>
      {/if}
      {#if about}<span class="pf-about" data-field="description">{about}</span>{/if}
    </figcaption>
  {/if}
  {#if spec.kind === "ref"}
    <p class="pf-note" data-field="note">The page is <a href={"#/ticket/" + encodeURIComponent(spec.attrs.ref)}>{spec.attrs.ref}</a>'s body; open it there.</p>
  {:else if spec.kind === "live" && live?.refused}
    <p class="pf-note" data-field="note">{live.refused} <code>{spec.attrs.route || "#/"}</code></p>
  {:else}
    <div class="pf-stage" bind:clientWidth={col}>
      <div class="pf-box" class:bare={spec.kind === "body"} style:width={size ? `${Math.round(W * scale)}px` : "100%"} style:height={`${Math.round(H * scale)}px`}>
        {#if spec.kind === "live"}
          <iframe src={live.src} title={label || "the test app"} loading="lazy" sandbox="allow-scripts allow-same-origin allow-forms allow-popups" referrerpolicy="no-referrer"
                  style:width={size ? `${W}px` : "100%"} style:height={`${H}px`} style:transform={scale < 1 ? `scale(${scale})` : null}></iframe>
        {:else}
          <div class="pf-frames" bind:this={stage} style:width={size ? `${W}px` : "100%"} style:height={`${H}px`} style:transform={scale < 1 ? `scale(${scale})` : null}></div>
        {/if}
      </div>
    </div>
    {#if framed && phase === "failed"}
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
  .pf-widths button[aria-pressed=true] { color: var(--ink); border-color: var(--muted); background: var(--panel); }
  .pf-stage { width: 100%; min-width: 0; }
  .pf-box { position: relative; max-width: 100%; overflow: hidden; border: var(--border) solid var(--line); border-radius: var(--radius); background: var(--panel); }
  .pf-box.bare { border: 0; border-radius: 0; background: none; }
  .pf-box iframe { display: block; max-width: none !important; border: 0; transform-origin: 0 0; }
  .pf-frames { position: relative; transform-origin: 0 0; }
  .pf-note { margin: 0; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .pf-src summary { cursor: pointer; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .pf-src pre { max-height: 320px; overflow: auto; margin: 4px 0 0; padding: 8px 10px; font-family: var(--font-text); font-size: 12px; white-space: pre-wrap; overflow-wrap: anywhere; background: var(--panel); border-radius: var(--radius); }
</style>
