<!-- SURFACE-7: what can start now, at #/ready and #/ready/:filter —
     `work.ready { filter, fields }` (Ready, every blocker done; the route's
     filter narrows it) as the board's rows, worst first, each with its
     parent as a ticket-chip and its severity bars. Live by `watch { filter:
     "ready and (…)", membership: true }`: the set's own word for each ticket
     that enters it (a blocker completing, a ticket made Ready) and each that
     leaves (taken, blocked again). Every row is work-ready-row (one sheet);
     an arrival names its id alone, so that row reads the ticket itself, live
     for the first LIVE arrivals (each holds a get and a watch) and once after.
     The service cuts the set at 200 in (epic, seq) order before the rows are
     sorted worst first, so past 200 the head says it is the first 200 by id
     of how many (`data-approx`). A write that moves more than 256 tickets is
     a `recount`, not their entered/left: the list is then marked stale
     (`data-state="stale"`) until it is read again.
     STUDIO-12: the page's title is page-ready's. -->
<script module>
  const FIELDS = ["id", "uid", "title", "kind", "status", "severity", "parent"];
  const RANK = { Critical: 0, High: 1, Medium: 2, Low: 3 };
  const LIMIT = 200, LIVE = 24;
  export const uses = ["tickets.work.ready", "tickets.watch"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import Row from "./work-ready-row.svelte";

  let { filter = null, sheet = null } = $props();
  const b = bus();
  const ready = live({ snapshot: "tickets.work.ready",
    params: () => ({ filter: filter ?? null, summary: true, limit: LIMIT, fields: FIELDS }),
    changes: "tickets.watch", changes_params: () => ({ filter: watched(), membership: true }),
    key: "id", on: { entered: "upsert:.", left: "remove:id", page: "upsert:." } });
  // `recount` carries no id, so the rows' fold passes it by: one small read of
  // the same membership stream keeps it (a row keyed by its shortcode).
  const recounts = live({ snapshot: "tickets.work.ready", params: () => ({ filter: filter ?? null, limit: 1, fields: ["id"] }),
    changes: "tickets.watch", changes_params: () => ({ filter: watched(), membership: true }),
    key: "shortcode", on: { recount: "upsert:." } });
  function watched() { return filter ? `ready and (${filter})` : "ready"; }
  const page = $derived(ready.rows.find((r) => r.id == null && "remaining" in r));
  const rows = $derived(ready.rows.filter((r) => r.id != null).sort((x, y) => (RANK[x.severity] ?? 4) - (RANK[y.severity] ?? 4)));
  const arrived = $derived(rows.filter((r) => r.title == null).map((r) => r.id));
  const total = $derived(page && page.remaining ? rows.length + page.remaining : null);
  const stale = $derived(recounts.rows.some((r) => r.held != null));

  function apply(ev) {
    const t = ev.currentTarget.value.trim();
    b.navigate(t === "" ? "#/ready" : "#/ready/" + encodeURIComponent(t));
  }
</script>

<section class="work-ready" data-surface="work-ready" data-state={stale ? "stale" : ready.status}>
  <header class="wr-head">
    <span class="wr-count" data-field="count">{rows.length}</span>
    <span class="wr-sub">can start now</span>
    {#if total != null}<span class="wr-approx" data-approx={total}>the first {LIMIT} by id of {total}, worst first among them</span>{/if}
    <label class="wr-filter">filter
      <input class="wr-field" data-form="tickets.work.ready" type="search" spellcheck="false" autocomplete="off"
             placeholder="e.g. kind build and in SCALE" aria-label="filter the ready set"
             value={filter ?? ""} onchange={apply}/>
    </label>
    {#if stale}<span class="wr-live" data-state="stale" title="a write moved more than 256 tickets; reload to read the set again">stale</span>
    {:else if ready.status === "live"}<span class="wr-live" data-live>live</span>{/if}
    {#if ready.status === "resyncing"}<span class="wr-live" data-state="resyncing">resyncing</span>{/if}
  </header>
  {#if ready.status === "error" || ready.status === "refused"}
    <p class="wr-refused" data-refused role="alert">{ready.message ?? "this filter is not taken"}</p>
  {/if}
  <div class="wr-scroll">
    {#if sheet === "loading" || ready.status === "pending"}
      <div class="wr-skel" data-loading aria-busy="true"><i></i><i></i><i></i><i></i><i></i></div>
    {/if}
    {#each rows as r (r.id)}
      <Row id={r.id} row={r.title == null ? null : r} live={arrived.indexOf(r.id) < LIVE}/>
    {/each}
    {#if ready.status === "live" && rows.length === 0}
      <p class="wr-empty" data-empty>Nothing can start now{filter ? " under this filter" : ""}: every Ready ticket waits on another.</p>
    {/if}
  </div>
</section>

<style>
  .work-ready { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; height: 100%; background: var(--bg); color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .wr-head { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; padding: 8px 14px; background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .wr-count { color: var(--ink); font-weight: var(--weight-bold); font-variant-numeric: tabular-nums; }
  .wr-sub { color: var(--muted); font-size: 12px; }
  .wr-approx { color: var(--muted); font-size: 11px; }
  .wr-filter { display: flex; gap: 6px; align-items: center; margin-left: 18px; font-size: 11px; color: var(--muted); }
  .wr-field { width: 340px; max-width: 60vw; padding: 3px 9px; font-size: 12px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .wr-field:hover, .wr-field:global([data-sheet~=hover]) { border-color: var(--muted); }
  .wr-field:focus-visible, .wr-field:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .wr-live { margin-left: auto; display: inline-flex; gap: 6px; align-items: center; font-size: 11px; color: var(--muted); }
  .wr-live::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--live); }
  .wr-live[data-state=resyncing]::before, .wr-live[data-state=stale]::before { background: var(--warn); }
  .wr-refused { margin: 0; padding: 6px 14px; font-size: 11px; color: var(--warn); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .wr-scroll { flex: 1; min-height: 0; overflow: auto; }
  .wr-skel { display: flex; flex-direction: column; }
  .wr-skel i { display: block; height: 12px; margin: 8px 14px; background: var(--skeleton); border-radius: var(--radius-sm); }
  .wr-empty { margin: 0; padding: 40px 14px; color: var(--muted); }
  @media (max-width: 720px) {
    .wr-filter { margin-left: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .wr-field { transition: none; }
  }
</style>
