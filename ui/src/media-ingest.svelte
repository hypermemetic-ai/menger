<!-- EDITOR-10: a doc absorbs anything from outside — the editor's paste and
     drop, one module (EDITOR-5's images grown to every kind). What comes in,
     and what it lands as:

     - a file pasted or dropped, several at once, any type: kept by the media
       service — in one message up to 7 MiB (`media.put`), else in chunks
       (`media.begin`, `media.chunk` at each offset, `media.finish`), resuming
       from what the upload has after a dropped call — with its progress in a
       widget where it was put; then `![name](media:<hash>)` for an image,
       `[name](media:<hash>)` for anything else (media-draw draws a video,
       audio, a PDF or a file card from it), on a line of its own;
     - a URL pasted alone, the selection empty: the URL on a line of its own
       (media-draw draws its card; the markdown stays the URL); over a
       selection, a link (EDITOR-5);
     - HTML pasted (a web page, a Google Doc, Notion, an email): turned into
       markdown by turndown with its GFM plugin — headings, lists, tables,
       links, code, emphasis — its images kept as media (`media.fetch` for an
       address, `media.put` for a `data:` image), never hot-linked; an image
       that cannot be kept is its alt text. HTML that carries no structure is
       pasted as its plain text, and so is anything inside a fence;
     - an image dragged from another tab (its address, not a file): fetched
       and kept by the server (`media.fetch`).

     An upload is a widget beside the text, never text in it (EDITOR-5): the
     source holds no placeholder, so no amend can carry one. -->
<script module>
  export const uses = ["media.put", "media.begin", "media.chunk", "media.finish", "media.fetch"];

  import { StateEffect, StateField } from "@codemirror/state";
  import { Decoration, EditorView, WidgetType } from "@codemirror/view";
  import TurndownService from "turndown";
  import { gfm } from "turndown-plugin-gfm";

  /** The most `media.put` takes in one message; past it, chunks. */
  export const PUT_MAX = 7 * 1024 * 1024;
  const IMAGE = /^image\/(png|jpeg|gif|webp)$/;
  const URL_ALONE = /^https?:\/\/\S+$/i;
  /** HTML worth converting: what plain text would lose. */
  const STRUCTURE = /<(h[1-6]|ul|ol|li|table|tr|td|th|a\s[^>]*href|img|pre|blockquote|strong|b|em|i|code|hr)\b/i;

  const base64 = (blob) => new Promise((done, fail) => {
    const r = new FileReader();
    r.onload = () => done(String(r.result).replace(/^data:[^,]*,/, ""));
    r.onerror = () => fail(r.error);
    r.readAsDataURL(blob);
  });
  const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
  const said = (r) => String(r?.message ?? r?.events?.find((e) => e.message)?.message ?? r?.events?.[0]?.type ?? "refused").replace(/^Invalid params: /, "").replace(/ \(nothing was kept\)$/, "");
  const stored = (r) => r?.events?.find((e) => e.type === "stored") ?? null;

  /** pasted HTML → `{ md, images }`: the markdown, each image as `\u0000IMG<n>\u0000` with its src in `images`. */
  export function htmlToMarkdown(html) {
    const td = new TurndownService({ headingStyle: "atx", codeBlockStyle: "fenced", bulletListMarker: "-", emDelimiter: "_", hr: "---" });
    td.use(gfm);
    // What a page carries that is no writing: dropped with what it holds (node names, upper case).
    const DROPPED = new Set(["SCRIPT", "STYLE", "META", "LINK", "TITLE", "HEAD", "NOSCRIPT", "IFRAME", "OBJECT", "EMBED", "FORM", "INPUT", "BUTTON", "TEMPLATE"]);
    td.remove((n) => DROPPED.has(n.nodeName));
    // Google Docs wraps the whole paste in a <b> that bolds nothing.
    td.addRule("docs-wrapper", { filter: (n) => n.nodeName === "B" && /^docs-internal-guid/.test(n.getAttribute("id") ?? ""), replacement: (c) => c });
    // Bold and italic said by style (Google Docs, Notion, mail).
    td.addRule("styled", {
      filter: (n) => n.nodeName === "SPAN" && /font-weight:\s*(700|bold)|font-style:\s*italic/i.test(n.getAttribute("style") ?? ""),
      replacement: (c, n) => {
        if (!c.trim()) return c;
        const st = n.getAttribute("style") ?? "";
        let out = c.trim();
        if (/font-style:\s*italic/i.test(st)) out = `_${out}_`;
        if (/font-weight:\s*(700|bold)/i.test(st)) out = `**${out}**`;
        return (/^\s/.test(c) ? " " : "") + out + (/\s$/.test(c) ? " " : "");
      },
    });
    const images = [];
    td.addRule("kept-images", {
      filter: "img",
      replacement: (_c, n) => {
        const src = n.getAttribute("src") ?? "";
        if (!/^(https?:|data:image\/)/i.test(src)) return n.getAttribute("alt") ?? "";
        images.push({ src, alt: (n.getAttribute("alt") ?? "").replace(/[[\]\n]/g, " ").trim() });
        return `\u0000IMG${images.length - 1}\u0000`;
      },
    });
    // A list item as people write it: `- one`, `1. one` (turndown pads the marker to four columns).
    const md = td.turndown(html).replace(/^(\s*)([-*+]|\d+\.) {2,}/gm, "$1$2 ").replace(/\n{3,}/g, "\n\n").trim();
    return { md, images };
  }

  /** Whether the caret at `pos` is inside a fence. */
  function inFence(state, pos) {
    let open = false;
    const upto = state.doc.lineAt(pos).number;
    for (let n = 1; n < upto; n++) if (/^ {0,3}(`{3,}|~{3,})/.test(state.doc.line(n).text)) open = !open;
    return open;
  }

  /** The editor's ingest over its `bus`, the files kept for `owner` (`{ doc }` or `{ ticket }`, V2-29):
   *  its extensions, and the effect a key dispatches to take a refusal away. */
  export function ingest({ bus, owner = {} }) {
    let next = 0;
    const add = StateEffect.define({ map: (v, m) => ({ ...v, pos: m.mapPos(v.pos) }) });
    const drop = StateEffect.define();
    const set = StateEffect.define();
    const fade = StateEffect.define();

    class Upload extends WidgetType {
      constructor(key, name, said, pct) { super(); this.key = key; this.name = name; this.said = said; this.pct = pct; }
      eq(o) { return o.key === this.key && o.said === this.said && o.pct === this.pct; }
      toDOM() {
        const d = document.createElement("span");
        d.className = "eb-upload";
        d.setAttribute("data-upload", this.name);
        d.setAttribute("data-state", this.said == null ? "uploading" : "refused");
        if (this.said != null) d.setAttribute("data-refused", "");
        if (this.pct != null) d.setAttribute("data-progress", String(this.pct));
        if (this.said != null) d.textContent = `${this.name}: ${this.said}`;
        else {
          d.textContent = this.pct == null ? `uploading ${this.name}…` : `uploading ${this.name} · ${this.pct}%`;
          if (this.pct != null) {
            const bar = document.createElement("span");
            bar.className = "eb-upload-bar";
            bar.style.cssText = `position:absolute;left:0;bottom:0;height:2px;width:${this.pct}%;background:var(--accent)`;
            d.style.position = "relative";
            d.style.overflow = "hidden";
            d.append(bar);
          }
        }
        return d;
      }
    }
    const widget = (key, name, s, pct, pos) => Decoration.widget({ widget: new Upload(key, name, s, pct), side: 1 + key / 1e6 }).range(pos);
    const field = StateField.define({
      create: () => Decoration.none,
      update(deco, tr) {
        deco = deco.map(tr.changes);
        for (const e of tr.effects) {
          if (e.is(add)) deco = deco.update({ add: [widget(e.value.key, e.value.name, null, null, e.value.pos)], sort: true });
          else if (e.is(drop)) deco = deco.update({ filter: (_f, _t, d) => d.spec.widget.key !== e.value });
          else if (e.is(fade)) deco = deco.update({ filter: (_f, _t, d) => d.spec.widget.said == null });
          else if (e.is(set)) {
            let at = null;
            deco.between(0, tr.state.doc.length, (f, _t, d) => { if (d.spec.widget.key === e.value.key) at = f; });
            if (at != null) deco = deco.update({ filter: (_f, _t, d) => d.spec.widget.key !== e.value.key, add: [widget(e.value.key, e.value.name, e.value.said ?? null, e.value.pct ?? null, at)] });
          }
        }
        return deco;
      },
      provide: (f) => EditorView.decorations.from(f),
    });

    /** Where the widget `key` stands now, or null. */
    const where = (view, key) => {
      let at = null;
      view.state.field(field).between(0, view.state.doc.length, (f, _t, d) => { if (d.spec.widget.key === key) at = f; });
      return at;
    };
    /** `text` as a paragraph of its own at the widget `key`, which goes. */
    const land = (view, key, text) => {
      const at = where(view, key);
      if (at == null) return;
      const line = view.state.doc.lineAt(at);
      const above = at > line.from ? "\n\n" : line.number > 1 && view.state.doc.line(line.number - 1).text.trim() !== "" ? "\n" : "";
      const below = at < line.to ? "\n\n" : line.number < view.state.doc.lines && view.state.doc.line(line.number + 1).text.trim() !== "" ? "\n" : "";
      view.dispatch({ changes: { from: at, insert: `${above}${text}${below}` }, effects: drop.of(key) });
    };
    const start = (view, name, pos) => {
      const key = ++next;
      view.dispatch({ effects: add.of({ key, name, pos }) });
      return key;
    };
    const refuse = (view, key, name, why) => view.dispatch({ effects: set.of({ key, name, said: why }) });

    /** One file kept: in one message, or in chunks with progress; its `stored`, or throws its refusal. */
    async function keep(file, progress) {
      if (file.size <= PUT_MAX) {
        const data = await base64(file);
        const r = await bus.call("media.put", { ...owner, name: file.name || undefined, data });
        return stored(r) ?? Promise.reject(said(r));
      }
      const begun = await bus.call("media.begin", { ...owner, name: file.name || undefined, size: file.size });
      let up = begun?.events?.find((e) => e.type === "upload");
      if (!up) throw said(begun);
      let at = up.received, tries = 0;
      while (at < file.size) {
        const end = Math.min(file.size, at + up.chunk);
        const c = await bus.call("media.chunk", { upload: up.upload, offset: at, data: await base64(file.slice(at, end)) }).catch((e) => ({ message: String(e?.message ?? e) }));
        const ok = c?.events?.find((e) => e.type === "chunked");
        if (ok) {
          at = ok.received;
          tries = 0;
          progress(Math.floor((at / file.size) * 100));
          continue;
        }
        if (c?.events?.some((e) => e.type === "refused" && e.reason !== "out_of_order") || ++tries > 6) throw said(c);
        // A dropped call: ask the upload what it has, and go on from there.
        await sleep(500 * tries);
        const again = await bus.call("media.begin", { upload: up.upload }).catch(() => null);
        const u = again?.events?.find((e) => e.type === "upload");
        if (u) at = u.received;
      }
      const f = await bus.call("media.finish", { upload: up.upload });
      return stored(f) ?? Promise.reject(said(f));
    }

    /** What a kept file's line says. */
    const mdOf = (s, name) => s.kind === "image"
      ? `![${(name || s.name || "image").replace(/\.[a-z0-9]+$/i, "").replace(/[[\]\n]/g, " ") || "image"}](media:${s.hash})`
      : `[${(name || s.name || "file").replace(/[[\]\n]/g, " ")}](media:${s.hash})`;

    async function upload(view, file, pos) {
      const name = file.name || (file.type.startsWith("image/") ? "image" : "file");
      const key = start(view, name.replace(/\.[a-z0-9]+$/i, "") || name, pos);
      try {
        const s = await keep(file, (pct) => view.dispatch({ effects: set.of({ key, name: name.replace(/\.[a-z0-9]+$/i, "") || name, pct }) }));
        land(view, key, mdOf(s, file.name));
      } catch (why) {
        refuse(view, key, name.replace(/\.[a-z0-9]+$/i, "") || name, typeof why === "string" ? why : String(why?.message ?? why ?? "refused"));
      }
    }

    /** A remote image or file, kept by the server; its markdown, or null. */
    async function fetched(url, alt, name) {
      const r = await bus.call("media.fetch", { url, ...owner, name: name || undefined }).catch(() => null);
      const s = stored(r);
      return s ? mdOf(s, alt || null) : null;
    }

    /** Pasted HTML, its images kept, at `pos`. */
    async function pasteHtml(view, html, pos) {
      const { md, images } = htmlToMarkdown(html);
      if (!images.length) {
        view.dispatch({ changes: { from: pos, to: view.state.selection.main.to > pos ? view.state.selection.main.to : pos, insert: md }, selection: { anchor: pos + md.length } });
        return;
      }
      const key = start(view, `${images.length} image${images.length > 1 ? "s" : ""}`, pos);
      const kept = await Promise.all(images.map(async ({ src, alt }) => {
        if (src.startsWith("data:")) {
          // The data itself goes: media.put reads past a `data:…;base64,` (the page policy lets no fetch read it).
          if (!/^data:image\/[a-z+.-]+;base64,/i.test(src) || (src.length * 3) / 4 > PUT_MAX) return alt;
          const r = await bus.call("media.put", { ...owner, data: src }).catch(() => null);
          const s = stored(r);
          return s ? mdOf(s, alt || "image") : alt;
        }
        return (await fetched(src, alt)) ?? alt;
      }));
      const text = md.replace(/\u0000IMG(\d+)\u0000/g, (_, n) => kept[Number(n)] ?? "");
      const at = where(view, key);
      if (at == null) return;
      view.dispatch({ changes: { from: at, insert: text }, effects: drop.of(key) });
    }

    const filesOf = (list) => [...(list ?? [])].filter((f) => f && f.kind !== "string");
    function pasted(ev, view) {
      const cd = ev.clipboardData;
      const files = filesOf(cd?.files);
      const sel = view.state.selection.main;
      if (files.length) {
        ev.preventDefault();
        for (const f of files) upload(view, f, sel.head);
        return true;
      }
      const text = (cd?.getData("text/plain") ?? "").trim();
      if (URL_ALONE.test(text)) {
        ev.preventDefault();
        if (!sel.empty) {
          const label = view.state.sliceDoc(sel.from, sel.to);
          view.dispatch({ changes: { from: sel.from, to: sel.to, insert: `[${label}](${text})` } });
          return true;
        }
        // Alone on its line, so it draws as its card; the markdown is the URL.
        const line = view.state.doc.lineAt(sel.head);
        if (line.text.trim() !== "") {
          view.dispatch({ changes: { from: sel.head, insert: text }, selection: { anchor: sel.head + text.length } });
          return true;
        }
        const above = line.number > 1 && view.state.doc.line(line.number - 1).text.trim() !== "" ? "\n" : "";
        const below = line.number < view.state.doc.lines && view.state.doc.line(line.number + 1).text.trim() !== "" ? "\n" : "";
        const insert = `${above}${text}${below}`;
        view.dispatch({ changes: { from: line.from, to: line.to, insert }, selection: { anchor: line.from + above.length + text.length } });
        return true;
      }
      const html = cd?.getData("text/html") ?? "";
      if (!html || !STRUCTURE.test(html) || inFence(view.state, sel.head)) return false;
      ev.preventDefault();
      if (!sel.empty) view.dispatch({ changes: { from: sel.from, to: sel.to, insert: "" } });
      pasteHtml(view, html, view.state.selection.main.head);
      return true;
    }
    function dropped(ev, view) {
      const dt = ev.dataTransfer;
      const pos = view.posAtCoords({ x: ev.clientX, y: ev.clientY }) ?? view.state.selection.main.head;
      const files = filesOf(dt?.files);
      if (files.length) {
        ev.preventDefault();
        for (const f of files) upload(view, f, pos);
        return true;
      }
      // An image dragged from another tab: its address, fetched and kept by the server.
      const html = dt?.getData("text/html") ?? "";
      const src = /<img[^>]+src="(https?:[^"]+)"/i.exec(html)?.[1]?.replace(/&amp;/g, "&");
      const uri = (dt?.getData("text/uri-list") ?? "").split("\n").map((l) => l.trim()).find((l) => l && !l.startsWith("#"));
      const url = src ?? (uri && /\.(png|jpe?g|gif|webp|svg|mp4|webm|mov|mp3|ogg|wav|m4a|flac|pdf)(\?|#|$)/i.test(uri) ? uri : null);
      if (!url) return false;
      ev.preventDefault();
      const name = decodeURIComponent(new URL(url).pathname.split("/").pop() || "image");
      const key = start(view, name.replace(/\.[a-z0-9]+$/i, "") || name, pos);
      fetched(url, name.replace(/\.[a-z0-9]+$/i, ""), name).then((md) => (md ? land(view, key, md) : refuse(view, key, name, "could not be fetched")));
      return true;
    }

    return { extension: [field, EditorView.domEventHandlers({ paste: pasted, drop: dropped })], fade: () => fade.of(null) };
  }
</script>
