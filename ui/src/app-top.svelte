<!-- SURFACE-2: the app's top — board-frame's `top` section. The epic
     (`epics.current`, one read; `epic.switched { epic }` on the bus, from the
     epics page, moves it at once — a switch says nothing on `epics.watch`),
     the health (`tickets.health`'s count, live by `tickets.watch {
     membership }`: one collection keeps what entered, one what left, so a
     ticket made and deleted counts once each way), a search box (enter →
     `#/search/:q`, or `#/search/:q/f/:filter` under the board's filter —
     SURFACE-8's routes), the links, and the palette's button (`palette.open` on
     the bus; the board's `ctrl+k` says the same). -->
<script module>
  export const uses = ["epics.current", "tickets.health", "tickets.watch"];
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
  .top-links a:focus-visible, .top-epic:focus-visible, .brand:focus-visible, .top-jump:focus-visible, .top-links a:global([data-sheet~=focus]), .top-epic:global([data-sheet~=focus]), .brand:global([data-sheet~=focus]), .top-jump:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .top-search { margin-left: auto; display: flex; }
  .top-search input { width: 260px; padding: 4px 10px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .top-search input:hover, .top-search input:global([data-sheet~=hover]) { border-color: var(--muted); }
  .top-search input:focus-visible, .top-search input:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .top-jump { display: inline-flex; gap: 6px; align-items: center; padding: 3px 8px; font-size: 12px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .top-jump:hover, .top-jump:global([data-sheet~=hover]) { color: var(--ink); border-color: var(--muted); }
  .top-kbd { display: inline-block; min-width: 18px; padding: 0 5px; font: 11px/17px var(--font-text); text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  @media (max-width: 720px) {
    .top-links, .top-jump { display: none; }
    .top-search input { width: 140px; }
  }
  @media (prefers-reduced-motion: reduce) {
    .brand, .top-epic, .top-links a, .top-search input, .top-jump { transition: none; }
  }
</style>
