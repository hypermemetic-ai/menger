<!-- FRAME-14: ticket-page in real Svelte (svelte/1), ticket-page.rhx's twin
     with ticket-chip.rhx inside it (a svelte/1 module imports no other
     component) — `get` (live on its own status and amend), its title,
     status, kind, parent, blockers and unlocks as chips, the body as markdown
     drawn through an allow-list, its children by one `list { parent }`,
     `set_status` from a menu. The engine's `markdown()` and sanitizer are not
     on the bus, so the body takes the same road here: markdown to HTML, the
     HTML parsed inert (DOMParser), and only the engine's allow-list kept —
     its tags, its attributes, its URL schemes — drawn as Svelte elements; no
     HTML string reaches the page. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.list", "tickets.set_status"];

  // The engine's allow-list (plexus-ui engine/src/html/mod.rs), as it is.
  const TAGS = new Set(["a", "abbr", "b", "blockquote", "br", "code", "dd", "del", "div", "dl", "dt", "em", "h1", "h2", "h3", "h4", "h5", "h6", "hr", "i", "img", "kbd", "li",
    "mark", "ol", "p", "pre", "q", "s", "small", "span", "strong", "sub", "sup", "table", "tbody", "td", "tfoot", "th", "thead", "tr", "u", "ul"]);
  const WHOLE = new Set(["script", "style", "iframe", "object", "embed", "noscript", "template", "svg", "math", "textarea", "title", "head", "frameset", "frame", "noembed", "xmp", "plaintext"]);
  const ATTRS = [["*", "title"], ["a", "href"], ["code", "class"], ["img", "alt"], ["img", "height"], ["img", "src"], ["img", "width"], ["ol", "start"], ["pre", "class"],
    ["td", "colspan"], ["td", "rowspan"], ["th", "colspan"], ["th", "rowspan"]];
  const URLS = new Set(["href", "src"]);
  const VOID = new Set(["br", "hr", "img"]);
  const attrOk = (tag, a) => TAGS.has(tag) && ATTRS.some(([t, n]) => (t === "*" || t === tag) && n === a);
  const urlOk = (v) => { const s = v.trim().toLowerCase(); return s.startsWith("https://") || s.startsWith("http://") || s.startsWith("mailto:") || s.startsWith("#"); };

  /** An inert DOM's nodes as the allow-list keeps them: { tag, attrs, kids } or a string. */
  function clean(node) {
    const out = [];
    for (const n of node.childNodes) {
      if (n.nodeType === 3) { out.push(n.data); continue; }
      if (n.nodeType !== 1) continue;
      const tag = n.localName;
      if (WHOLE.has(tag)) continue;
      const kids = clean(tag === "template" ? n.content : n);
      if (!TAGS.has(tag)) { out.push(...kids); continue; }
      const attrs = {};
      for (const a of n.attributes) {
        if (!attrOk(tag, a.name)) continue;
        if (URLS.has(a.name) && !urlOk(a.value)) continue;
        attrs[a.name] = a.value;
      }
      out.push({ tag, attrs, kids });
    }
    return out;
  }

  const esc = (s) => s.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;").replaceAll('"', "&quot;");
  const RAW = /<\/?[A-Za-z][A-Za-z0-9-]*(?:\s[^<>]*)?\/?>/g;

  /** Inline markdown: code spans, raw tags as written (the allow-list judges them), links, images, emphasis. */
  function inline(text) {
    const parts = text.split(/(`[^`]*`)/);
    return parts.map((p, i) => {
      if (i % 2 === 1) return "<code>" + esc(p.slice(1, -1)) + "</code>";
      let out = "", at = 0;
      for (const m of p.matchAll(RAW)) {
        out += prose(p.slice(at, m.index)) + m[0];
        at = m.index + m[0].length;
      }
      return out + prose(p.slice(at));
    }).join("");
  }
  function prose(t) {
    return esc(t)
      .replace(/!\[([^\]]*)\]\(([^)\s]*)\)/g, (_, alt, src) => `<img alt="${alt}" src="${src}">`)
      .replace(/\[([^\]]+)\]\(([^)\s]*)\)/g, (_, label, href) => `<a href="${href}">${label}</a>`)
      .replace(/&lt;(https?:\/\/[^\s&]+)&gt;/g, (_, u) => `<a href="${u}">${u}</a>`)
      .replace(/\*\*([^*]+)\*\*|__([^_]+)__/g, (_, a, z) => `<strong>${a ?? z}</strong>`)
      .replace(/\*([^*]+)\*|\b_([^_]+)_\b/g, (_, a, z) => `<em>${a ?? z}</em>`)
      .replace(/~~([^~]+)~~/g, "<del>$1</del>");
  }

  /** Block markdown (CommonMark's common blocks and GFM's tables) to HTML; an HTML block passes as written. */
  function markdown(src) {
    const lines = String(src ?? "").replace(/\r\n?/g, "\n").split("\n");
    const out = [];
    let i = 0;
    const blank = (l) => l.trim() === "";
    while (i < lines.length) {
      const l = lines[i];
      if (blank(l)) { i++; continue; }
      let m;
      if ((m = l.match(/^\s*(```|~~~)\s*([\w-]*)/))) {
        const fence = m[1], lang = m[2];
        const body = [];
        for (i++; i < lines.length && !lines[i].trim().startsWith(fence); i++) body.push(lines[i]);
        i++;
        out.push(`<pre><code${lang ? ` class="language-${esc(lang)}"` : ""}>${esc(body.join("\n"))}</code></pre>`);
        continue;
      }
      if ((m = l.match(/^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/))) { out.push(`<h${m[1].length}>${inline(m[2])}</h${m[1].length}>`); i++; continue; }
      if (/^\s{0,3}([-*_])(\s*\1){2,}\s*$/.test(l)) { out.push("<hr>"); i++; continue; }
      if (/^\s{0,3}</.test(l)) {
        const body = [];
        for (; i < lines.length && !blank(lines[i]); i++) body.push(lines[i]);
        out.push(body.join("\n"));
        continue;
      }
      if (/^\s{0,3}>/.test(l)) {
        const body = [];
        for (; i < lines.length && /^\s{0,3}>/.test(lines[i]); i++) body.push(lines[i].replace(/^\s{0,3}>\s?/, ""));
        out.push(`<blockquote>${markdown(body.join("\n"))}</blockquote>`);
        continue;
      }
      if ((m = l.match(/^\s{0,3}([-*+]|\d+[.)])\s+/))) {
        const ordered = /\d/.test(m[1]);
        const items = [];
        for (; i < lines.length; i++) {
          const x = lines[i].match(/^\s{0,3}([-*+]|\d+[.)])\s+(.*)$/);
          if (x && /\d/.test(x[1]) === ordered) items.push(x[2]);
          else if (!blank(lines[i]) && items.length && /^\s+\S/.test(lines[i])) items[items.length - 1] += " " + lines[i].trim();
          else break;
        }
        const start = ordered && parseInt(m[1], 10) !== 1 ? ` start="${parseInt(m[1], 10)}"` : "";
        out.push(`<${ordered ? "ol" + start : "ul"}>${items.map((t) => `<li>${inline(t)}</li>`).join("")}</${ordered ? "ol" : "ul"}>`);
        continue;
      }
      if (l.includes("|") && i + 1 < lines.length && /^\s*\|?\s*:?-+:?\s*(\|\s*:?-+:?\s*)*\|?\s*$/.test(lines[i + 1])) {
        const cells = (r) => r.trim().replace(/^\|/, "").replace(/\|$/, "").split("|").map((c) => inline(c.trim()));
        const head = cells(l);
        const rows = [];
        for (i += 2; i < lines.length && !blank(lines[i]) && lines[i].includes("|"); i++) rows.push(cells(lines[i]));
        out.push(`<table><thead><tr>${head.map((c) => `<th>${c}</th>`).join("")}</tr></thead><tbody>${rows.map((r) => `<tr>${r.map((c) => `<td>${c}</td>`).join("")}</tr>`).join("")}</tbody></table>`);
        continue;
      }
      const para = [];
      for (; i < lines.length && !blank(lines[i]) && !/^\s{0,3}(#{1,6}\s|```|~~~|>|[-*+]\s|\d+[.)]\s|<)/.test(lines[i]); i++) para.push(lines[i].trim());
      if (!para.length) { para.push(l.trim()); i++; }
      out.push(`<p>${inline(para.join("\n"))}</p>`);
    }
    return out.join("\n");
  }

  /** A body as the page draws it: markdown, parsed inert, kept by the allow-list. */
  export function bodyNodes(text) {
    const doc = new DOMParser().parseFromString(`<!doctype html><body>${markdown(text)}`, "text/html");
    return clean(doc.body);
  }
</script>

<script>
  import { bus } from "plexus-ui/host";
  import { onDestroy } from "svelte";

  let { id } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const STATUSES = ["Draft", "Pending", "Ready", "Coding", "In Review", "Complete", "Blocked", "Superseded", "Canceled"];

  const ticket = $derived(b.live({ snapshot: "tickets.get", params: { id }, changes: "tickets.watch", key: "id",
                                   on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket" } }));
  const children = $derived(b.live({ snapshot: "tickets.list", params: { parent: id, summary: true, limit: 200 }, changes: "tickets.watch", key: "id",
                                     on: { created: "upsert:ticket", amended: "upsert:ticket", status_changed: "upsert:ticket", deleted: "remove:ids" } }));

  // The chips: each named ticket's title when `get` answers (ticket-chip.rhx's live), one per id.
  let titles = $state({});
  const held = {};
  $effect(() => {
    const ids = new Set($ticket.rows.flatMap((t) => [t.parent, ...(t.blocked_by ?? []), ...(t.unlocks ?? [])]).filter((x) => x != null));
    for (const c of ids) {
      if (held[c]) continue;
      const s = b.live({ snapshot: "tickets.get", params: { id: c }, key: "id" });
      held[c] = s.subscribe((v) => (titles[c] = v.rows[0]?.title ?? ""));
    }
    for (const c of Object.keys(held)) {
      if (ids.has(c)) continue;
      held[c]();
      delete held[c];
      delete titles[c];
    }
  });
  onDestroy(() => Object.values(held).forEach((off) => off()));

  function setStatus(ev) {
    const status = ev.currentTarget.value;
    if (status !== "") b.call("tickets.set_status", { id, status }).catch((e) => console.error(`set_status: ${e?.message ?? e}`));
  }
</script>

{#snippet chip(cid, kind)}
  <a class="chip" data-chip={kind} data-chip-id={cid} href={base + "ticket/" + cid}>
    <span class="chip-kind">{kind}</span>
    <span class="chip-code">{cid}</span>
    <span class="chip-title" data-chip-title>{titles[cid] ?? ""}</span>
  </a>
{/snippet}

{#snippet nodes(list)}
  {#each list as n}
    {#if typeof n === "string"}{n}{:else if VOID.has(n.tag)}<svelte:element this={n.tag} {...n.attrs}/>{:else}<svelte:element this={n.tag} {...n.attrs}>{@render nodes(n.kids)}</svelte:element>{/if}
  {/each}
{/snippet}

<article class="ticket-page" data-ticket={id}>
  <nav class="crumbs"><a href={base} data-back>← board</a></nav>
  {#each $ticket.rows as t (t.id)}
    <header class="ticket-head">
      <p class="ticket-code"><span data-ticket-id>{t.id}</span></p>
      <h1 class="ticket-title" data-ticket-title>{t.title}</h1>
      <div class="ticket-meta">
        <span class="tag status" data-ticket-status>{t.status}</span>
        <span class="tag kind" data-ticket-kind>{t.kind}</span>
        <label class="status-menu">set status
          <select data-status-menu onchange={setStatus}>
            <option value="">…</option>
            {#each STATUSES as s}<option value={s}>{s}</option>{/each}
          </select>
        </label>
      </div>
      <div class="chips">
        {#if t.parent != null}{@render chip(t.parent, "parent")}{/if}
        {#each t.blocked_by ?? [] as bid (bid)}{@render chip(bid, "blocker")}{/each}
        {#each t.unlocks ?? [] as u (u)}{@render chip(u, "unlocks")}{/each}
      </div>
    </header>
    <div class="ticket-body" data-ticket-body>{@render nodes(bodyNodes(t.body))}</div>
  {/each}
  {#if $ticket.status === "live" && $ticket.rows.length === 0}<p class="empty" data-not-found>No ticket {id} here.</p>{/if}
  <section class="ticket-children">
    <h2>Children</h2>
    <ul>
      {#each $children.rows as c (c.id)}
        <li><a data-ticket-child={c.id} data-status={c.status} href={base + "ticket/" + c.id}><span class="row-id">{c.id}</span> <span class="row-title">{c.title}</span> <span class="tag">{c.status}</span></a></li>
      {/each}
    </ul>
    {#if $children.status === "live" && $children.rows.length === 0}<p class="empty">none</p>{/if}
  </section>
</article>
