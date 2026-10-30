<!-- SURFACE-7: what can start now, at #/ready and #/ready/:filter —
     `work.ready { filter, fields }` (Ready, every blocker done; the route's
     filter narrows it) as the board's rows, worst first, each with its
     parent as a ticket-chip and its severity bars. Live by `watch { filter:
     "ready and (…)", membership: true }`: the set's own word for each ticket
     that enters it (a blocker completing, a ticket made Ready) and each that
     leaves (taken, blocked again). An arrival names its id alone, so its row
     is work-ready-row, a sibling that reads the ticket itself. -->
<script module>
  const FIELDS = ["id", "title", "kind", "status", "severity", "parent"];
  const RANK = { Critical: 0, High: 1, Medium: 2, Low: 3 };
  export const uses = ["tickets.work.ready", "tickets.watch"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import TicketChip from "plexus-ui:ticket-chip";
  import Arrived from "./work-ready-row.svelte";

  let { filter = null } = $props();
  const b = bus();
  const ready = live({ snapshot: "tickets.work.ready",
    params: () => ({ filter: filter ?? null, summary: true, limit: 200, fields: FIELDS }),
    changes: "tickets.watch", changes_params: () => ({ filter: filter ? `ready and (${filter})` : "ready", membership: true }),
    key: "id", on: { entered: "upsert:.", left: "remove:id" } });
  const rows = $derived([...ready.rows].sort((x, y) => (RANK[x.severity] ?? 4) - (RANK[y.severity] ?? 4)));

  function apply(ev) {
    const t = ev.currentTarget.value.trim();
    b.navigate(t === "" ? "#/ready" : "#/ready/" + encodeURIComponent(t));
  }
</script>

<section class="work-ready" data-surface="work-ready">
  <header class="wr-head">
    <h1>ready</h1>
    <span class="wr-count" data-field="count">{rows.length}</span>
    <span class="wr-sub">can start now{rows.length >= 200 ? " · the first 200" : ""}</span>
    <label class="wr-filter">filter
      <input class="wr-field" data-form="tickets.work.ready" type="search" spellcheck="false" autocomplete="off"
             placeholder="e.g. kind build and in SCALE" aria-label="filter the ready set"
             value={filter ?? ""} onchange={apply}/>
    </label>
    {#if ready.status === "live"}<span class="wr-live" data-live>live</span>{/if}
    {#if ready.status === "resyncing"}<span class="wr-live" data-state="resyncing">resyncing</span>{/if}
  </header>
  {#if ready.status === "error" || ready.status === "refused"}
    <p class="wr-refused" data-refused role="alert">{ready.message ?? "this filter is not taken"}</p>
  {/if}
  <div class="wr-scroll">
    {#if ready.status === "pending"}
      <div class="wr-skel" data-loading aria-busy="true"><i></i><i></i><i></i><i></i><i></i></div>
    {/if}
    {#each rows as r (r.id)}
      {#if r.title == null}
        <Arrived id={r.id}/>
      {:else}
        <a class="board-row" data-id={r.id} data-status={r.status} href={"#/ticket/" + r.id}>
          <i class="kmark" data-kind={r.kind} aria-hidden="true"></i>
          <span class="row-id" data-field="id">{r.id}</span>
          <span class="row-title" data-field="title">{r.title}</span>
          <span class="row-parent" data-field="parent">{#if r.parent}<TicketChip id={r.parent} kind="parent"/>{/if}</span>
          <span class="sev" data-field="severity" data-sev={r.severity ?? ""} title={r.severity ?? "no severity"}><i></i><i></i><i></i></span>
          <span class="row-status" data-field="status">{r.status}</span>
        </a>
      {/if}
    {/each}
    {#if ready.status === "live" && rows.length === 0}
      <p class="wr-empty" data-empty>Nothing can start now{filter ? " under this filter" : ""}: every Ready ticket waits on another.</p>
    {/if}
  </div>
</section>

<style>
  .work-ready { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; height: 100%; background: var(--bg); color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .wr-head { display: flex; flex-wrap: wrap; align-items: center; gap: 8px 12px; padding: 8px 14px; background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .wr-head h1 { margin: 0; font-family: var(--font-display); font-size: 16px; font-weight: var(--weight-strong); }
  .wr-count { color: var(--ink); font-weight: var(--weight-bold); font-variant-numeric: tabular-nums; }
  .wr-sub { color: var(--muted); font-size: 12px; }
  .wr-filter { display: flex; gap: 6px; align-items: center; margin-left: 18px; font-size: 11px; color: var(--muted); }
  .wr-field { width: 340px; max-width: 60vw; padding: 3px 9px; font-size: 12px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .wr-field:hover { border-color: var(--muted); }
  .wr-field:focus-visible { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .wr-live { margin-left: auto; display: inline-flex; gap: 6px; align-items: center; font-size: 11px; color: var(--muted); }
  .wr-live::before { content: ""; width: 6px; height: 6px; border-radius: 50%; background: var(--live); }
  .wr-live[data-state=resyncing]::before { background: var(--warn); }
  .wr-refused { margin: 0; padding: 6px 14px; font-size: 11px; color: var(--warn); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .wr-scroll { flex: 1; min-height: 0; overflow: auto; }
  .wr-skel { display: flex; flex-direction: column; }
  .wr-skel i { display: block; height: 12px; margin: 8px 14px; background: var(--skeleton); border-radius: var(--radius-sm); }
  .wr-empty { margin: 0; padding: 40px 14px; color: var(--muted); }
  .board-row { display: grid; grid-template-columns: 10px 104px minmax(0, 1fr) 260px 24px auto; gap: 10px; align-items: center; height: var(--pitch); padding: 0 14px; color: var(--ink); text-decoration: none; white-space: nowrap; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .board-row:hover { background: var(--hover); }
  .board-row:focus-visible { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .row-id { color: var(--accent); font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; }
  .row-title { overflow: hidden; text-overflow: ellipsis; }
  .row-parent { min-width: 0; overflow: hidden; }
  .row-status { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .board-row[data-status=Pending] .row-status { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .board-row[data-status=Ready] .row-status { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .board-row[data-status=Coding] .row-status { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .board-row[data-status="In Review"] .row-status { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .board-row[data-status=Complete] .row-status { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .sev { display: inline-flex; gap: 2px; align-items: flex-end; height: 12px; }
  .sev i { width: 3px; background: var(--line); border-radius: 1px; }
  .sev i:nth-child(1) { height: 5px; }
  .sev i:nth-child(2) { height: 8px; }
  .sev i:nth-child(3) { height: 11px; }
  .sev[data-sev=Critical] i { background: var(--sev-critical); }
  .sev[data-sev=High] i { background: var(--sev-high); }
  .sev[data-sev=Medium] i:nth-child(-n+2) { background: var(--sev-medium); }
  .sev[data-sev=Low] i:nth-child(1) { background: var(--sev-low); }
  .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  @media (max-width: 720px) {
    .board-row { grid-template-columns: 10px 88px minmax(0, 1fr) 24px auto; }
    .row-parent { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .board-row { transition: none; animation: none; }
  }
  @media (max-width: 720px) {
    .wr-filter { margin-left: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .wr-field { transition: none; }
  }
</style>
