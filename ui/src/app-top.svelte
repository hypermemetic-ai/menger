<!-- SURFACE-2: the app's top — board-frame's `top` section. V2-25: the epic
     switcher went with the `epics` activation (the board is the organization's
     whole set), and the `me` link with its page. The health
     (`tickets.health`'s count, live by `tickets.watch {
     membership }`: one collection keeps what entered, one what left, so a
     ticket made and deleted counts once each way), a search box (enter →
     `#/search/:q`, or `#/search/:q/f/:filter` under the board's filter —
     SURFACE-8's routes), the links, and the palette's button (`palette.open` on
     the bus; the board's `ctrl+k` says the same). STUDIO-11: for an editor —
     `ui.author`'s roles (`auth.requires`) held on `*` (`auth.whoami`'s
     memberships) — the pencil and `ctrl+e`: this page in the studio,
     `/studio/#/page/<the hash, encodeURIComponent'd>`; a viewer has neither.
     STUDIO-30: the fixed links went — one dropdown ("pages", showing the page
     you are on) of every route that needs no argument: the live version's
     manifest routes (its hash the newest move of `ui.history`, kept by
     `ui.watch`'s `live`) with `ui.routes.list`'s rows over them (kept by
     `route_changed`, so a route the studio sets appears without a push),
     as the engine's router merges them. Aliases are left out (`#/rhx/…`,
     `#/sv/…`, and a pattern whose component a listed one already shows);
     the board's pages first, then the rest by name. Click, Enter or Space
     opens it; the arrows move, typing filters, Enter goes, Escape closes.
     The note window (note-quick, by its bound mark) rides here, so every page
     with the top bar has its keys — cmd+option+n / ctrl+alt+n — and the
     bar's `note` button opens it (`note.open` on the bus). -->
<script module>
  export const uses = ["tickets.health", "tickets.watch", "auth.requires", "auth.whoami", "ui.routes.list", "ui.watch", "ui.history"];

  // STUDIO-30: the pages a person can open with no argument, from the merged
  // routes `{ pattern: component }` — the aliases out, a readable name each,
  // the board's pages first, then the rest by name.
  const BOARD_PAGES = ["board", "queue", "ready", "waves", "filters", "search", "new", "bulk"];
  export const pageName = (pattern) => pattern.replace(/^#\//, "").replace(/\/$/, "") || "home";
  export function pagesOf(routes) {
    const rank = (n) => (BOARD_PAGES.includes(n) ? BOARD_PAGES.indexOf(n) : BOARD_PAGES.length);
    const all = Object.entries(routes ?? {})
      .filter(([p, c]) => typeof c === "string" && p.startsWith("#/") && !p.split("/").some((s) => s.startsWith(":")) && !/^#\/(rhx|sv)(\/|$)/.test(p))
      .map(([href, component]) => ({ name: pageName(href), href, component }))
      .sort((a, b) => rank(a.name) - rank(b.name) || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    const shown = new Set();
    return all.filter((e) => (shown.has(e.component) ? false : (shown.add(e.component), true)));
  }
</script>

<script>
  import { tick } from "svelte";
  import { bus, live } from "plexus-ui/host";
  import NoteQuick from "plexus-ui:note-quick";

  let { here = "board", filter = null } = $props();
  const b = bus();
  const born = live({ snapshot: "tickets.health", params: {}, changes: "tickets.watch", changes_params: { membership: true },
                      key: "id", on: { health: "upsert:.", entered: "upsert:." } });
  const gone = live({ snapshot: "tickets.health", params: {}, changes: "tickets.watch", changes_params: { membership: true },
                      key: "id", on: { health: "upsert:.", left: "upsert:." } });
  const loading = $derived(born.status === "pending" || gone.status === "pending");
  const base = $derived(born.rows.find((r) => r.ticket_count != null)?.ticket_count ?? 0);
  const count = $derived(base + born.rows.filter((r) => r.ticket_count == null).length - gone.rows.filter((r) => r.ticket_count == null).length);

  // STUDIO-30: the live version (the newest move; this page's own before
  // any), its manifest's routes and components, and the rows over them.
  const steps = live({ snapshot: "ui.history", params: { limit: 1 }, changes: "ui.watch", key: "seq", on: { live: "upsert:." } });
  const rows = live({ snapshot: "ui.routes.list", params: {}, changes: "ui.watch", key: "pattern", on: { route_changed: "upsert:." } });
  const served = document.querySelector('meta[name="menger-ui-version"]')?.content ?? null;
  const version = $derived.by(() => {
    let at = null;
    for (const r of steps.rows) if (r.seq != null && r.version && (at == null || r.seq > at.seq)) at = r;
    return at?.version ?? served;
  });
  let manifest = $state(null);
  $effect(() => {
    const v = version;
    if (!v || !/^[0-9a-f]{64}$/.test(v)) return;
    let gone = false;
    fetch(`/app/${v}/manifest.json`).then((r) => (r.ok ? r.json() : null), () => null).then((m) => { if (!gone && m) manifest = m; }, () => {});
    return () => (gone = true);
  });
  const pages = $derived.by(() => {
    const routes = { ...(manifest?.routes ?? {}) };
    const has = (c) => !manifest?.components || c in manifest.components;
    for (const r of rows.rows) {
      if (typeof r.pattern !== "string") continue;
      // A row whose component the live version lacks is passed over, as the router passes it;
      // a row gone (`component: null`) leaves the manifest's.
      if (typeof r.component === "string" && has(r.component)) routes[r.pattern] = r.component;
    }
    return pagesOf(routes);
  });

  // STUDIO-11: who may edit a page — ui.author's roles, one held on * (the
  // UI has no scope).
  const needs = live({ snapshot: "auth.requires", params: { method: "ui.author" }, key: "method", on: {} });
  const door = live({ snapshot: "auth.whoami", params: {}, key: "principal", on: {} });
  const editor = $derived.by(() => {
    const roles = needs.rows.find((r) => r.method === "ui.author")?.roles ?? [];
    return (door.rows[0]?.memberships ?? []).some((m) => m.scope === "*" && roles.includes(m.role));
  });
  // The page's hash, as it moves: the pencil names where it goes.
  let hash = $state(location.hash || "#/");
  const hashMoved = () => (hash = location.hash || "#/");
  $effect(() => {
    addEventListener("hashchange", hashMoved);
    return () => removeEventListener("hashchange", hashMoved);
  });
  const studio = $derived("/studio/#/page/" + encodeURIComponent(hash));
  // The page you are on: the listed one the hash names, else the page's own word (`here`).
  const current = $derived(pages.find((p) => p.href === hash.split("?")[0])?.name ?? (pages.some((p) => p.name === here) ? here : null) ?? here);

  // The dropdown: open, what is typed, the option the arrows are on.
  let open = $state(false);
  let typed = $state("");
  let at = $state(0);
  let toggle = $state(null);
  let field = $state(null);
  let menu = $state(null);
  const shown = $derived.by(() => {
    const q = typed.trim().toLowerCase();
    return q === "" ? pages : pages.filter((p) => p.name.toLowerCase().includes(q) || p.href.toLowerCase().includes(q));
  });
  function show() {
    typed = "";
    at = Math.max(0, pages.findIndex((p) => p.name === current));
    open = true;
    tick().then(() => field?.focus());
  }
  function hide(refocus) {
    open = false;
    if (refocus) toggle?.focus();
  }
  function go(p) {
    if (!p) return;
    hide(false);
    b.navigate(p.href);
  }
  function toggled(ev) {
    if (ev.key === "Enter" || ev.key === " " || ev.key === "ArrowDown") {
      ev.preventDefault();
      show();
    }
  }
  const step = (d) => { const n = shown.length; if (n) at = (at + d + n) % n; };
  // The open dropdown's keys, the host's while it is open (as the palette's): they run alone, in its filter too.
  $effect(() => {
    if (!open) return;
    return b.keys({
      "arrowdown": { to: () => step(1), in_inputs: true, only: true, label: "the next page" },
      "arrowup": { to: () => step(-1), in_inputs: true, only: true, label: "the page before" },
      "enter": { to: () => go(shown[at]), in_inputs: true, only: true, label: "go to the page" },
      "escape": { to: () => hide(true), in_inputs: true, only: true, label: "close the pages" },
    });
  });
  $effect(() => {
    if (shown.length && at >= shown.length) at = shown.length - 1;
  });
  $effect(() => {
    if (!open) return;
    menu?.querySelector("[data-active]")?.scrollIntoView?.({ block: "nearest" });
  });
  // A press outside closes it.
  $effect(() => {
    if (!open) return;
    const away = (e) => { if (!e.composedPath().some((n) => n === menu || n === toggle)) hide(false); };
    addEventListener("pointerdown", away, true);
    return () => removeEventListener("pointerdown", away, true);
  });
  // On the app's own page only: drawn in a studio frame, it binds nothing.
  const onApp = location.pathname.startsWith("/app");
  $effect(() => {
    if (!editor || !onApp) return;
    return b.keys({ "ctrl+e": { to: () => location.assign("/studio/#/page/" + encodeURIComponent(location.hash || "#/")), label: "edit this page" } });
  });
  // EDITOR-13: the app's page-wide chords are the engine's on every route, so the browser never acts
  // on them: ctrl/⌘+s is `page.save` (the editor's own binding saves on its page and runs alone), and
  // for a viewer ctrl+e is held too — it does nothing, where the browser would take it to its search.
  if (onApp) {
    b.keys({
      "ctrl+s": { to: () => b.emit("page.save", {}), label: "save" },
      "meta+s": { to: () => b.emit("page.save", {}), label: "save" },
    });
  }
  $effect(() => {
    if (editor || !onApp) return;
    return b.keys({ "ctrl+e": { to: () => {}, label: "edit this page (an editor's)" } });
  });

  const enc = (t) => t.trim().replaceAll("%", "%25").replaceAll("/", "%2F").replaceAll("#", "%23");
  function searched(ev) {
    if (ev.key !== "Enter") return;
    const t = enc(ev.currentTarget.value);
    if (t !== "") b.navigate("#/search/" + t + (filter ? "/f/" + enc(filter) : ""));
  }
</script>

<header class="app-top" data-surface="app-top">
  <a class="brand" href="#/">menger</a>
  <span class="top-health" data-field="health" data-loading={loading ? "" : undefined} data-state={born.status === "live" ? "live" : "offline"} title={born.status}><b data-health-count>{loading ? 0 : count}</b>tickets</span>
  <nav class="top-pages" aria-label="pages" data-pages>
    <button class="top-pages-toggle" type="button" bind:this={toggle} data-action="pages.open" aria-haspopup="listbox" aria-expanded={open ? "true" : "false"}
            title="every page" onclick={() => (open ? hide(true) : show())} onkeydown={toggled}>
      <span class="top-pages-lbl">pages</span><b data-pages-here>{current}</b><span class="top-caret" aria-hidden="true">▾</span>
    </button>
    {#if open}
      <div class="top-pages-menu" bind:this={menu} data-pages-menu>
        <input type="text" bind:this={field} bind:value={typed} oninput={() => (at = 0)} onkeydown={(e) => e.key === "Tab" && hide(false)} placeholder="filter pages" aria-label="filter pages"
               spellcheck="false" autocomplete="off" role="combobox" aria-expanded="true" aria-controls="top-pages-list" aria-activedescendant={shown[at] ? "top-page-" + shown[at].name : undefined} data-pages-filter/>
        <div class="top-pages-list" id="top-pages-list" role="listbox" aria-label="pages">
          {#each shown as p, i (p.href)}
            <a id={"top-page-" + p.name} href={p.href} role="option" aria-selected={i === at ? "true" : "false"} data-page={p.name} data-active={i === at ? "" : undefined}
               aria-current={p.name === current ? "page" : undefined} tabindex="-1" onmousemove={() => (at = i)} onclick={(e) => { e.preventDefault(); go(p); }}>
              <span class="top-page-name">{p.name}</span><code class="top-page-href">{p.href}</code>
            </a>
          {:else}
            <p class="top-pages-none" data-pages-none>{pages.length ? "no page matches" : "no pages yet"}</p>
          {/each}
        </div>
      </div>
    {/if}
  </nav>
  <div class="top-search" data-form="search" role="search">
    <input type="search" name="q" placeholder="search titles and bodies" aria-label="search" spellcheck="false" autocomplete="off" onkeydown={searched}/>
  </div>
  <button class="top-jump" type="button" data-action="note.open" title="write a note (cmd+option+n, or ctrl+alt+n)" onclick={() => b.emit("note.open", {})}>note <kbd class="top-kbd">ctrl</kbd><kbd class="top-kbd">alt</kbd><kbd class="top-kbd">n</kbd></button>
  <button class="top-jump" type="button" data-action="palette.open" onclick={() => b.emit("palette.open", {})}>jump <kbd class="top-kbd">ctrl</kbd><kbd class="top-kbd">k</kbd></button>
  {#if editor && onApp}
    <a class="top-edit" href={studio} data-action="page.edit" aria-keyshortcuts="Control+E" aria-label="edit this page" title={"edit this page in the studio: " + hash}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M11.2 1.8l3 3L5.6 13.4 2 14l.6-3.6z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M9.6 3.4l3 3" stroke="currentColor" stroke-width="1.5"/></svg>edit <kbd class="top-kbd">ctrl</kbd><kbd class="top-kbd">e</kbd>
      <span class="top-tip" role="tooltip">edit this page in the studio<br><code>{studio}</code></span>
    </a>
  {/if}
  <NoteQuick/>
</header>

<style>
  .app-top { display: flex; align-items: center; gap: 12px; height: 44px; padding: 0 max(14px, env(safe-area-inset-left)); background: var(--panel); border-bottom: var(--border) solid var(--line); white-space: nowrap; }
  .brand { font-family: var(--font-display); font-weight: var(--weight-bold); color: var(--ink); text-decoration: none; padding: 2px 4px; border-radius: var(--radius); transition: color var(--motion-fast) var(--ease); }
  .brand:hover, .brand:global([data-sheet~=hover]) { color: var(--accent); }
  .top-health { display: inline-flex; gap: 5px; align-items: baseline; font-size: 12px; color: var(--muted); }
  .top-health b { color: var(--ink); font-weight: var(--weight-bold); font-variant-numeric: tabular-nums; }
  .top-health::before { content: ""; display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--live); align-self: center; }
  .top-health[data-state=offline]::before { background: var(--warn); }
  .top-health[data-loading] { color: transparent; background: var(--skeleton); border-color: transparent; border-radius: var(--radius-sm); }
  .top-health[data-loading] b { color: transparent; }
  .top-health[data-loading]::before { background: var(--skeleton); }
  .top-pages { position: relative; display: flex; margin-left: 6px; min-width: 0; }
  .top-pages-toggle { display: inline-flex; gap: 6px; align-items: baseline; padding: 4px 10px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .top-pages-toggle b { color: var(--ink); font-weight: var(--weight-bold); }
  .top-pages-lbl { font-size: 12px; }
  .top-caret { font-size: 10px; }
  .top-pages-toggle:hover, .top-pages-toggle:global([data-sheet~=hover]) { color: var(--ink); border-color: var(--muted); }
  .top-pages-toggle[aria-expanded=true] { border-color: var(--focus); }
  .top-pages-menu { position: absolute; left: 0; top: calc(100% + 6px); z-index: 40; display: flex; flex-direction: column; gap: 4px; width: 300px; max-width: calc(100vw - 24px); padding: 6px; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); box-shadow: 0 6px 18px var(--shadow); }
  .top-pages-menu input { padding: 4px 8px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .top-pages-menu input:focus-visible { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .top-pages-list { display: flex; flex-direction: column; max-height: min(60vh, 420px); overflow-y: auto; }
  .top-pages-list a { display: flex; gap: 10px; justify-content: space-between; align-items: baseline; padding: 4px 8px; color: var(--ink); text-decoration: none; border-radius: var(--radius-sm); }
  .top-pages-list a[data-active] { background: var(--sel); }
  .top-pages-list a[aria-current=page] .top-page-name { font-weight: var(--weight-bold); }
  .top-page-href { font-size: 11px; color: var(--muted); }
  .top-pages-none { margin: 4px 8px; font-size: 12px; color: var(--muted); }
  .top-pages-toggle:focus-visible, .brand:focus-visible, .top-jump:focus-visible, .top-edit:focus-visible, .top-pages-toggle:global([data-sheet~=focus]), .brand:global([data-sheet~=focus]), .top-jump:global([data-sheet~=focus]), .top-edit:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .top-search { margin-left: auto; display: flex; }
  .top-search input { width: 260px; padding: 4px 10px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .top-search input:hover, .top-search input:global([data-sheet~=hover]) { border-color: var(--muted); }
  .top-search input:focus-visible, .top-search input:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .top-jump { display: inline-flex; gap: 6px; align-items: center; padding: 3px 8px; font-size: 12px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .app-top > :global([data-slot=note-quick]) { display: contents; }
  .top-jump:hover, .top-jump:global([data-sheet~=hover]) { color: var(--ink); border-color: var(--muted); }
  .top-edit { position: relative; display: inline-flex; gap: 6px; align-items: center; padding: 3px 8px; font-size: 12px; color: var(--accent); text-decoration: none; background: var(--sel); border: var(--border) solid var(--accent); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .top-edit:hover, .top-edit:global([data-sheet~=hover]) { background: var(--card); }
  .top-edit svg { width: 13px; height: 13px; flex: none; }
  .top-edit .top-kbd { color: var(--accent); border-color: var(--accent); }
  .top-tip { position: absolute; right: 0; top: calc(100% + 6px); z-index: 30; display: none; padding: 5px 9px; font-size: 11px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); box-shadow: 0 6px 18px var(--shadow); white-space: nowrap; text-align: left; pointer-events: none; }
  .top-tip code { color: var(--accent); }
  .top-edit:hover .top-tip, .top-edit:focus-visible .top-tip { display: block; }
  .top-kbd { display: inline-block; min-width: 18px; padding: 0 5px; font: 11px/17px var(--font-text); text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  @media (max-width: 720px) {
    .top-jump, .top-edit, .top-pages-lbl { display: none; }
    .top-search input { width: 140px; }
  }
  /* EDITOR-12: on a phone the bar fits the width: the search gives way first. */
  @media (max-width: 640px) {
    .app-top { gap: 8px; min-width: 0; }
    .top-search { flex: 0 1 140px; min-width: 0; }
    .top-search input { width: 100%; min-width: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .brand, .top-pages-toggle, .top-search input, .top-jump, .top-edit { transition: none; }
  }
</style>
