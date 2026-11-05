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

     The frame: `<iframe sandbox="allow-scripts allow-forms">`, never
     `allow-same-origin` — an opaque origin, so the page's scripts run but read
     no cookie or storage of the app, reach no socket and cannot touch
     `parent` or `top`. `allow-forms` because Chrome fires no `submit` in a
     sandbox without it, so a form a script handles would be dead; a real
     submission is still refused, by the frame's `form-action 'none'`.
     Not a `srcdoc`, which would inherit the app's policy and refuse the
     page's inline scripts: the frame loads the shell's `/frame` (served
     under its own policy — no connection, no form, `sandbox allow-scripts`
     even opened alone; src/shell.rs FRAME_CSP), which says `menger:boot` on
     its channel and is answered `menger:doc { html }` — the page, with a
     few lines after it that post its height (`menger:size`) and take the
     theme (`menger:theme`). A message is taken only from that frame's own
     `contentWindow` carrying its channel. The height follows the content's,
     at most MAX px (then the frame scrolls inside); a page that grows with
     its frame (`100vh` and a margin) is held after a few steps. A width the
     page names is drawn at that width and scaled down to the column
     (EDITOR-12: nothing wider than the column on a phone), with a switch
     when it names several. A changed page (an amend, an edit, a width
     switched) loads in a second, hidden frame that replaces the first once
     it says its height, so nothing flashes. The label and the description
     sit above the frame, never inside it. A frame that never says it is up
     (an older server) draws a note and the source. -->
<script module>
  export const uses = [];

  /** The frame's height at most, in the page's own pixels. */
  export const MAX = 2000;
  /** How many times in a row a frame may grow before it is held where it is. */
  const GROWS = 8;

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

  /** The lines after the page that post its height on its channel — the content's height (the document's
   *  scroll height when it overflows, else its box's) whenever it changes — and take the theme's tokens. */
  const reporter = (channel) => "<scr" + `ipt>(function(){var c=${JSON.stringify(channel)},last=-1;function s(){var de=document.documentElement,b=document.body,`
    + `h=de.scrollHeight>de.clientHeight+1?de.scrollHeight:Math.ceil(Math.max(de.getBoundingClientRect().height,b?b.getBoundingClientRect().height:0));`
    + `if(h!==last){last=h;parent.postMessage({type:"menger:size",channel:c,height:h},"*");}}`
    + `addEventListener("message",function(e){var d=e.data;if(e.source!==parent||!d||d.type!=="menger:theme"||d.channel!==c)return;var t=document.getElementById("menger-theme");if(t)t.textContent=d.css;});`
    + `if(window.ResizeObserver){var r=new ResizeObserver(s);r.observe(document.documentElement);if(document.body)r.observe(document.body);}`
    + `addEventListener("load",s);setTimeout(s,0);setInterval(s,1000);})();</scr` + "ipt>";

  /** The document a frame is handed: the page, then its reporter. */
  export const docOf = (html, channel) => `${html}\n${reporter(channel)}`;

  /** The app's theme as the frame's `:root`: every token the page's root holds, and its colour scheme. */
  export function themeCss() {
    if (typeof document !== "object") return "";
    const cs = getComputedStyle(document.documentElement);
    const out = [];
    for (let i = 0; i < cs.length; i++) if (cs[i].startsWith("--")) out.push(`${cs[i]}:${cs.getPropertyValue(cs[i]).trim()}`);
    return `:root{color-scheme:${cs.colorScheme || "normal"};${out.join(";")}}`;
  }

  /** The live origin this server's bodies may frame (APP-110), stamped by the shell; "" when none. */
  const liveOrigin = () => (typeof document === "object" && document.querySelector('meta[name="menger-live-origin"]')?.content?.trim()) || "";
</script>

<script>
  import { onMount, untrack } from "svelte";
  import { ticketHref } from "./ticket-links.svelte";

  /** `source`: the piece's markdown. `doc`: a body framed whole (ticket-view built it), its kind `body`. */
  let { source = "", doc = null } = $props();
  const spec = $derived(doc != null ? { kind: "body", attrs: {}, html: doc } : pageSpec(source));
  const sizes = $derived(widthsOf(spec?.attrs ?? {}));
  let pick = $state(0);
  const size = $derived(sizes[Math.min(pick, Math.max(0, sizes.length - 1))] ?? null);
  const label = $derived(spec?.attrs?.label || spec?.attrs?.title || "");
  const about = $derived(spec?.attrs?.description || "");
  let col = $state(0);
  let phase = $state("loading"); // loading · drawn · failed

  const W = $derived(size ? size.w : Math.max(1, col));
  const scale = $derived(size && col > 0 ? Math.min(1, col / size.w) : 1);
  const framed = $derived(spec?.kind === "page" || spec?.kind === "body");
  // A body's links open a tab (its `<base target="_blank">`); a mockup's stay in it.
  const SANDBOX = $derived(spec?.kind === "body" ? "allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox" : "allow-scripts allow-forms");

  // The frames: the one on screen, and a newer one loading behind it until it says its height.
  let bufs = $state([]);
  let shown = $state(0);
  let seq = 0;
  $effect(() => {
    if (!framed) return;
    const html = spec.html, w = size?.w ?? 0;
    untrack(() => {
      const id = ++seq;
      bufs = [...bufs.filter((b) => b.id === shown), { id, html, w, h: null, grows: 0 }];
      if (!bufs.some((b) => b.id === shown)) shown = id;
    });
  });
  const cur = $derived(bufs.find((b) => b.id === shown) ?? bufs[0] ?? null);
  // A height the page names (`WxH`, or `height=`) is kept; else the content's.
  const fixed = $derived(size?.h ?? (Number(spec?.attrs?.height) > 0 ? Math.round(Number(spec.attrs.height)) : null));
  const H = $derived(Math.max(40, Math.min(MAX, fixed ?? cur?.h ?? startHeight(W, !!size))));
  const hOf = (b) => Math.max(40, Math.min(MAX, fixed ?? b.h ?? startHeight(W, !!size)));

  /** A buffer's frame: `/frame#<channel>` says it is up, is handed the page once, then says its height. */
  function frame(node, buf) {
    const channel = "f" + Array.from(crypto.getRandomValues(new Uint32Array(3)), (x) => x.toString(36)).join("");
    let sent = false, wait = null;
    node.dataset.channel = channel;
    const on = (e) => {
      if (e.source !== node.contentWindow) return;
      const d = e.data;
      if (!d || typeof d !== "object" || d.channel !== channel) return;
      if (d.type === "menger:boot" && !sent) {
        sent = true;
        // Handed over once the frame has a width (a page reads its viewport as it runs; a section still hidden has none).
        const give = () => {
          const html = spec.kind === "body" ? buf.html.replace("/*menger-theme*/", themeCss()) : buf.html;
          node.contentWindow?.postMessage({ type: "menger:doc", channel, html: docOf(html, channel) }, "*");
        };
        if (node.offsetWidth > 0) give();
        else { wait = new ResizeObserver(() => { if (node.offsetWidth > 0) { wait.disconnect(); wait = null; give(); } }); wait.observe(node); }
      } else if (d.type === "menger:size" && sent && Number.isFinite(d.height)) {
        const h = Math.round(d.height);
        const b = bufs.find((x) => x.id === buf.id);
        if (!b) return;
        if (b.h != null && h > b.h) { if (++b.grows > GROWS) return; } else b.grows = 0;
        b.h = h;
        if (shown !== buf.id && buf.id === bufs[bufs.length - 1]?.id) { shown = buf.id; bufs = bufs.filter((x) => x.id >= buf.id); }
        if (shown === buf.id) phase = "drawn";
      }
    };
    addEventListener("message", on);
    const late = setTimeout(() => { if (!sent && shown === buf.id) phase = "failed"; }, 8000);
    node.src = `/frame#${channel}`;
    return { destroy() { removeEventListener("message", on); clearTimeout(late); wait?.disconnect(); } };
  }

  // A body takes the app's theme again when it flips (an attribute on the root, or the system's scheme).
  onMount(() => {
    if (spec?.kind !== "body") return;
    const send = () => {
      const css = themeCss();
      for (const f of document.querySelectorAll("[data-surface=page-frame][data-kind=body] iframe[data-channel]")) f.contentWindow?.postMessage({ type: "menger:theme", channel: f.dataset.channel, css }, "*");
    };
    const mo = new MutationObserver(send);
    mo.observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "class", "style"] });
    const mq = matchMedia("(prefers-color-scheme: dark)");
    mq.addEventListener("change", send);
    return () => { mo.disconnect(); mq.removeEventListener("change", send); };
  });

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
    <p class="pf-note" data-field="note">The page is <a href={ticketHref({ id: encodeURIComponent(spec.attrs.ref) })}>{spec.attrs.ref}</a>'s body; open it there.</p>
  {:else if spec.kind === "live" && live?.refused}
    <p class="pf-note" data-field="note">{live.refused} <code>{spec.attrs.route || "#/"}</code></p>
  {:else}
    <div class="pf-stage" bind:clientWidth={col}>
      <div class="pf-box" class:bare={spec.kind === "body"} style:width={size ? `${Math.round(W * scale)}px` : "100%"} style:height={`${Math.round(H * scale)}px`}>
        {#if spec.kind === "live"}
          <iframe src={live.src} title={label || "the test app"} loading="lazy" sandbox="allow-scripts allow-same-origin allow-forms allow-popups" referrerpolicy="no-referrer"
                  style:width={size ? `${W}px` : "100%"} style:height={`${H}px`} style:transform={scale < 1 ? `scale(${scale})` : null}></iframe>
        {:else}
          {#each bufs as b (b.id)}
            <iframe use:frame={b} title={label || (spec.kind === "body" ? "the body" : "a page")} sandbox={SANDBOX} referrerpolicy="no-referrer" class:behind={b.id !== shown}
                    data-buffer={b.id === shown ? "shown" : "loading"}
                    style:width={size ? `${W}px` : "100%"} style:height={`${hOf(b)}px`} style:transform={scale < 1 ? `scale(${scale})` : null}></iframe>
          {/each}
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
  .pf-box iframe.behind { position: absolute; top: 0; left: 0; visibility: hidden; pointer-events: none; }
  .pf-note { margin: 0; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .pf-src summary { cursor: pointer; font-family: var(--font-text); font-size: 12px; color: var(--muted); }
  .pf-src pre { max-height: 320px; overflow: auto; margin: 4px 0 0; padding: 8px 10px; font-family: var(--font-text); font-size: 12px; white-space: pre-wrap; overflow-wrap: anywhere; background: var(--panel); border-radius: var(--radius); }
</style>
