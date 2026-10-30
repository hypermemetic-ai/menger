<!-- SURFACE-2: the app's top — board-frame's `top` section. The epic
     (`epics.current`, one read; `epic.switched { epic }` on the bus, from the
     epics page, moves it at once — a switch says nothing on `epics.watch`),
     the health (`tickets.health`'s count, live by `tickets.watch {
     membership }`: one collection keeps what entered, one what left, so a
     ticket made and deleted counts once each way), a search box (enter →
     `#/search/:q`, or `#/search/:q/f/:filter` under the board's filter —
     SURFACE-8's routes), the links, and the palette's button (`palette.open` on
     the bus; the board's `ctrl+k` says the same). STUDIO-11: for an editor —
     `ui.author`'s roles (`auth.requires`) held on `*` (`auth.whoami`'s
     memberships) — the pencil and `ctrl+e`: this page in the studio,
     `/studio/#/page/<the hash, encodeURIComponent'd>`; a viewer has neither. -->
<script module>
  export const uses = ["epics.current", "tickets.health", "tickets.watch", "auth.requires", "auth.whoami"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { here = "board", filter = null } = $props();
  const b = bus();
  const epic = live({ snapshot: "epics.current", params: {}, key: "name", on: { current: "upsert:epic" } });
  const born = live({ snapshot: "tickets.health", params: {}, changes: "tickets.watch", changes_params: { membership: true },
                      key: "id", on: { health: "upsert:.", entered: "upsert:." } });
  const gone = live({ snapshot: "tickets.health", params: {}, changes: "tickets.watch", changes_params: { membership: true },
                      key: "id", on: { health: "upsert:.", left: "upsert:." } });
  let switched = $state(null);
  b.on("epic.switched", (d) => { switched = typeof d?.epic === "string" ? d.epic : d?.epic?.name ?? null; });

  const name = $derived(switched ?? epic.rows[0]?.name ?? null);
  const loading = $derived(born.status === "pending" || gone.status === "pending");
  const base = $derived(born.rows.find((r) => r.ticket_count != null)?.ticket_count ?? 0);
  const count = $derived(base + born.rows.filter((r) => r.ticket_count == null).length - gone.rows.filter((r) => r.ticket_count == null).length);
  const LINKS = [["board", "#/"], ["ready", "#/ready"], ["waves", "#/waves"], ["filters", "#/filters"], ["new", "#/new"], ["me", "#/me"]];

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
  const moved = () => (hash = location.hash || "#/");
  $effect(() => {
    addEventListener("hashchange", moved);
    return () => removeEventListener("hashchange", moved);
  });
  const studio = $derived("/studio/#/page/" + encodeURIComponent(hash));
  // On the app's own page only: drawn in a studio frame, it binds nothing.
  const onApp = location.pathname.startsWith("/app");
  $effect(() => {
    if (!editor || !onApp) return;
    return b.keys({ "ctrl+e": { to: () => location.assign("/studio/#/page/" + encodeURIComponent(location.hash || "#/")), label: "edit this page" } });
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
  <a class="top-epic" data-field="epic" href="#/epics" data-loading={name == null ? "" : undefined}><span class="lbl">epic</span><span data-epic-name>{name ?? "epic"}</span></a>
  <span class="top-health" data-field="health" data-loading={loading ? "" : undefined} data-state={born.status === "live" ? "live" : "offline"} title={born.status}><b data-health-count>{loading ? 0 : count}</b>tickets</span>
  <nav class="top-links" aria-label="pages">
    {#each LINKS as [link, href] (link)}
      <a {href} data-link={link} aria-current={here === link ? "page" : undefined}>{link}</a>
    {/each}
  </nav>
  <div class="top-search" data-form="search" role="search">
    <input type="search" name="q" placeholder="search titles and bodies" aria-label="search" spellcheck="false" autocomplete="off" onkeydown={searched}/>
  </div>
  <button class="top-jump" type="button" data-action="palette.open" onclick={() => b.emit("palette.open", {})}>jump <kbd class="top-kbd">ctrl</kbd><kbd class="top-kbd">k</kbd></button>
  {#if editor && onApp}
    <a class="top-edit" href={studio} data-action="page.edit" aria-keyshortcuts="Control+E" aria-label="edit this page" title={"edit this page in the studio: " + hash}>
      <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M11.2 1.8l3 3L5.6 13.4 2 14l.6-3.6z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M9.6 3.4l3 3" stroke="currentColor" stroke-width="1.5"/></svg>edit <kbd class="top-kbd">ctrl</kbd><kbd class="top-kbd">e</kbd>
      <span class="top-tip" role="tooltip">edit this page in the studio<br><code>{studio}</code></span>
    </a>
  {/if}
</header>

<style>
  .app-top { display: flex; align-items: center; gap: 12px; height: 44px; padding: 0 max(14px, env(safe-area-inset-left)); background: var(--panel); border-bottom: var(--border) solid var(--line); white-space: nowrap; }
  .brand { font-family: var(--font-display); font-weight: var(--weight-bold); color: var(--ink); text-decoration: none; padding: 2px 4px; border-radius: var(--radius); transition: color var(--motion-fast) var(--ease); }
  .brand:hover, .brand:global([data-sheet~=hover]) { color: var(--accent); }
  .top-epic { display: inline-flex; gap: 6px; align-items: center; padding: 2px 9px; font-size: 12px; color: var(--ink); text-decoration: none; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .top-epic:hover, .top-epic:global([data-sheet~=hover]) { border-color: var(--muted); }
  .top-epic .lbl { font-size: 10px; color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .top-health { display: inline-flex; gap: 5px; align-items: baseline; font-size: 12px; color: var(--muted); }
  .top-health b { color: var(--ink); font-weight: var(--weight-bold); font-variant-numeric: tabular-nums; }
  .top-health::before { content: ""; display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--live); align-self: center; }
  .top-health[data-state=offline]::before { background: var(--warn); }
  .top-epic[data-loading], .top-health[data-loading] { color: transparent; background: var(--skeleton); border-color: transparent; border-radius: var(--radius-sm); }
  .top-epic[data-loading] span, .top-health[data-loading] b { color: transparent; }
  .top-health[data-loading]::before { background: var(--skeleton); }
  .top-links { display: flex; gap: 2px; margin-left: 6px; }
  .top-links a { padding: 4px 10px; color: var(--muted); text-decoration: none; border: var(--border) solid transparent; border-radius: var(--radius); transition: color var(--motion-fast) var(--ease), background-color var(--motion-fast) var(--ease); }
  .top-links a:hover, .top-links a:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .top-links a[aria-current=page] { color: var(--ink); background: var(--card); border-color: var(--line); }
  .top-links a:focus-visible, .top-epic:focus-visible, .brand:focus-visible, .top-jump:focus-visible, .top-edit:focus-visible, .top-links a:global([data-sheet~=focus]), .top-epic:global([data-sheet~=focus]), .brand:global([data-sheet~=focus]), .top-jump:global([data-sheet~=focus]), .top-edit:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .top-search { margin-left: auto; display: flex; }
  .top-search input { width: 260px; padding: 4px 10px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .top-search input:hover, .top-search input:global([data-sheet~=hover]) { border-color: var(--muted); }
  .top-search input:focus-visible, .top-search input:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .top-jump { display: inline-flex; gap: 6px; align-items: center; padding: 3px 8px; font-size: 12px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
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
    .top-links, .top-jump, .top-edit { display: none; }
    .top-search input { width: 140px; }
  }
  @media (prefers-reduced-motion: reduce) {
    .brand, .top-epic, .top-links a, .top-search input, .top-jump, .top-edit { transition: none; }
  }
</style>
