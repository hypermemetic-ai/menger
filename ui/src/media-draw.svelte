<!-- EDITOR-10: how a doc draws what it absorbed — the widgets for the new
     media kinds, one drawing for the read-only view (ticket-view) and the
     editor's drawn blocks (editor-body). The markdown stays plain; this
     reads what the host's render made of it and draws over it:

     - `[name](media:<hash>)` alone on its line (rewritten to this origin's
       `/media/<hash>` before the render) — `media.info` says its kind: a
       video or audio as the browser's own player, a PDF in the browser's
       viewer (an iframe of `/media/<hash>`, which only this origin may
       frame), any other file as a card: its name, its size, a download.
       An image is the render's own `<img>`, as before.
     - a bare `https://…` alone on its line — `media.unfurl` says what it is:
       a card with its title, description, site and kept image; a YouTube,
       Vimeo, X or Figma link as its player (an iframe whose address the
       server built and the page policy names), GitHub's as a repository's
       card. The URL stays in the markdown, so it round-trips; a link that
       cannot be read stays a link.

     `drawMedia(node, { bus, text })` is a Svelte action: it draws `node` now
     and again when `text` changes (ticket-body's); `drawAll(node, bus)` draws
     once (the editor's drawn block, after its mount). The component itself
     draws `body` as a doc does — the sheet's tile.
     Answers are kept per hash and per URL for the page's life. -->
<script module>
  export const uses = ["media.info", "media.unfurl"];

  const HASH = /\/media\/([0-9a-f]{64})(?:[?#].*)?$/;
  const PLAYERS = ["https://www.youtube-nocookie.com/", "https://player.vimeo.com/", "https://platform.twitter.com/", "https://www.figma.com/"];
  const infos = new Map();
  const cards = new Map();

  /** `bytes` in words. */
  export function size(bytes) {
    const n = Number(bytes) || 0;
    if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
    if (n >= 1024 ** 2) return `${(n / 1024 ** 2).toFixed(1)} MB`;
    if (n >= 1024) return `${Math.round(n / 1024)} KB`;
    return `${n} bytes`;
  }

  function el(tag, attrs = {}, ...kids) {
    const e = document.createElement(tag);
    for (const [k, v] of Object.entries(attrs)) if (v != null) e.setAttribute(k, v);
    for (const k of kids) if (k != null) e.append(k);
    return e;
  }

  /** The link alone in its paragraph, if `a` is one. */
  function alone(a) {
    const p = a.parentElement;
    if (!p || p.tagName !== "P") return null;
    for (const n of p.childNodes) if (n !== a && !(n.nodeType === 3 && !n.textContent.trim())) return null;
    return p;
  }

  async function info(bus, hash) {
    if (!infos.has(hash)) infos.set(hash, bus.call("media.info", { hash }).then((r) => r?.events?.find((e) => e.type === "info") ?? null).catch(() => null));
    return infos.get(hash);
  }

  async function unfurl(bus, url) {
    if (!cards.has(url)) cards.set(url, bus.call("media.unfurl", { url }).then((r) => r?.events?.find((e) => e.type === "unfurled") ?? null).catch(() => null));
    return cards.get(url);
  }

  /** A kept file as its kind draws it. */
  export function fileWidget(i, href, label) {
    const name = label || i.name || "file";
    const box = el("div", { class: "mx", "data-media": i.kind, "data-media-live": "" });
    if (i.kind === "video") box.append(el("video", { src: href, controls: "", preload: "metadata", playsinline: "", "aria-label": name }));
    else if (i.kind === "audio") box.append(el("audio", { src: href, controls: "", preload: "metadata", "aria-label": name }));
    else if (i.kind === "pdf") box.append(el("iframe", { src: href, title: name, loading: "lazy" }));
    if (i.kind !== "file") {
      box.append(el("div", { class: "mx-cap" }, el("span", { class: "mx-name" }, name), el("span", { class: "mx-size" }, size(i.bytes)), el("a", { href, download: i.name || name, class: "mx-dl" }, "download")));
      return box;
    }
    const ext = (/\.([a-z0-9]{1,6})$/i.exec(i.name || name)?.[1] ?? "file").toUpperCase();
    box.append(el("span", { class: "mx-ext", "aria-hidden": "true" }, ext), el("span", { class: "mx-meta" }, el("span", { class: "mx-name" }, name), el("span", { class: "mx-size" }, size(i.bytes))), el("a", { href, download: i.name || name, class: "mx-dl" }, "download"));
    return box;
  }

  /** A link's card, or its player. */
  export function linkWidget(c, href) {
    const embed = c.embed && PLAYERS.some((p) => c.embed.startsWith(p)) ? c.embed : null;
    if (embed) {
      const box = el("div", { class: "mx mx-embed", "data-media": "embed", "data-provider": c.provider ?? "", "data-media-live": "" });
      box.append(el("iframe", { src: embed, title: c.title ?? c.site ?? "embed", loading: "lazy", allow: "encrypted-media; picture-in-picture; fullscreen", allowfullscreen: "", referrerpolicy: "strict-origin-when-cross-origin", sandbox: "allow-scripts allow-same-origin allow-popups allow-presentation" }));
      box.append(el("a", { class: "mx-src", href, target: "_blank", rel: "noopener noreferrer" }, c.title ?? href));
      return box;
    }
    const a = el("a", { class: "mx mx-card", href, target: "_blank", rel: "noopener noreferrer", "data-media": "card", "data-kind": c.kind, "data-provider": c.provider ?? undefined, "data-media-live": "" });
    if (c.image) a.append(el("img", { src: `${location.origin}/media/${c.image}`, alt: "", loading: "lazy" }));
    a.append(el("span", { class: "mx-text" }, el("span", { class: "mx-site" }, c.site ?? new URL(href).host), el("span", { class: "mx-title" }, c.title ?? href), c.description ? el("span", { class: "mx-desc" }, c.description) : null));
    return a;
  }

  /** Every lone media link and bare URL under `node`, drawn. */
  export function drawAll(node, bus) {
    // A bare URL alone in its paragraph, which the render leaves as text: a link first.
    for (const p of node.querySelectorAll("p")) {
      if (p.closest("[data-media]") || p.children.length || !/^https?:\/\/\S+$/i.test(p.textContent.trim())) continue;
      const url = p.textContent.trim();
      p.replaceChildren(el("a", { href: url }, url));
    }
    for (const a of node.querySelectorAll("a[href]")) {
      if (a.closest("[data-media]")) continue;
      const p = alone(a);
      if (!p) continue;
      const href = a.getAttribute("href") ?? "";
      const m = HASH.exec(href);
      if (m && href.startsWith(location.origin)) {
        const label = a.textContent.trim();
        p.setAttribute("data-media-pending", "");
        info(bus, m[1]).then((i) => {
          p.removeAttribute("data-media-pending");
          if (i && p.isConnected && a.isConnected) p.replaceChildren(fileWidget(i, href, label));
        });
        continue;
      }
      // A bare URL: the link's text is its address.
      if (/^https?:\/\//i.test(href) && a.textContent.trim() === href.trim() && !href.startsWith(location.origin)) {
        p.setAttribute("data-media-pending", "");
        unfurl(bus, href).then((c) => {
          p.removeAttribute("data-media-pending");
          if (c && p.isConnected && a.isConnected) p.replaceChildren(linkWidget(c, href));
        });
      }
    }
  }

  /** The action: `node` drawn now, and again whenever `text` changes — `{ bus, text }`, the
   *  component's bus (media.info and media.unfurl are asked on it). */
  export function drawMedia(node, { bus, text } = {}) {
    let b = bus;
    const go = () => setTimeout(() => node.isConnected && b && drawAll(node, b), 0);
    go();
    return { update(p) { b = p?.bus ?? b; go(); }, destroy() {} };
  }
</script>

<script>
  import { bus as hostBus } from "plexus-ui/host";
  import TicketView from "./ticket-view.svelte";
  let { body = "" } = $props();
  const b = hostBus();
</script>

<div data-surface="media-draw" use:drawMedia={{ bus: b, text: body }}><TicketView {body}/></div>
