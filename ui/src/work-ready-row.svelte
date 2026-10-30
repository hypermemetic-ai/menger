<!-- SURFACE-7: one row of the ready set, the board's row — kind mark, id,
     title, the parent as a ticket-chip mark, severity bars, status chip.
     work-ready hands it `row` when the answer carried the ticket; a row that
     arrived live (`entered` names the id alone) is read here: `get`, live
     on its own changes while `live` (work-ready keeps at most a few live:
     each holds a get and a watch), else read once. It flashes in. work-ready
     imports it; it is a component in its own right too. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import TicketChip from "plexus-ui:ticket-chip";

  let { id, row = null, live: watching = true } = $props();
  const held = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id", when: () => row == null && watching,
                      on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket", severity_changed: "upsert:ticket" } });
  const once = live({ snapshot: "tickets.get", params: () => ({ id }), key: "id", when: () => row == null && !watching });
  const t = $derived(watching ? held : once);
  // A `not_found` answer is a row of its id alone: no title.
  const shown = $derived(row != null ? [row] : t.rows.filter((r) => r.title != null));
</script>

{#each shown as r (r.id)}
  <a class="board-row" class:arrived={row == null} data-id={r.id} data-status={r.status} data-arrived={row == null ? (watching ? "live" : "once") : undefined} href={"#/ticket/" + r.id}>
    <i class="kmark" data-kind={r.kind} aria-hidden="true"></i>
    <span class="row-id" data-field="id">{r.id}</span>
    <span class="row-title" data-field="title">{r.title}</span>
    <span class="row-parent" data-field="parent">{#if r.parent}<TicketChip id={r.parent} kind="parent"/>{/if}</span>
    <span class="sev" data-field="severity" data-sev={r.severity ?? ""} title={r.severity ?? "no severity"}><i></i><i></i><i></i></span>
    <span class="row-status" data-field="status">{r.status}</span>
  </a>
{/each}
{#if row == null && t.status === "pending"}
  <div class="board-row" data-id={id} data-loading aria-busy="true"><i></i><span class="row-id">{id}</span><span class="row-skel"></span></div>
{/if}

<style>
  @keyframes arrive { from { background: var(--sel); } to { background: transparent; } }
  .arrived { animation: arrive var(--motion-flash) var(--ease); }
  .row-skel { height: 12px; background: var(--skeleton); border-radius: var(--radius-sm); }
  .board-row { display: grid; grid-template-columns: 10px 104px minmax(0, 1fr) 260px 24px auto; gap: 10px; align-items: center; height: var(--pitch); padding: 0 14px; color: var(--ink); text-decoration: none; white-space: nowrap; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .board-row:hover, .board-row:global([data-sheet~=hover]) { background: var(--hover); }
  .board-row:focus-visible, .board-row:global([data-sheet~=focus]) { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .row-id { color: var(--accent); font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; }
  .row-title { overflow: hidden; text-overflow: ellipsis; }
  .row-parent { min-width: 0; overflow: hidden; }
  .row-status { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .board-row[data-status=Pending] .row-status { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .board-row[data-status=Ready] .row-status { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .board-row[data-status=Coding] .row-status { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .board-row[data-status="In Review"] .row-status { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .board-row[data-status=Complete] .row-status { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .board-row[data-status="Draft"] .row-status { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .board-row[data-status="Blocked"] .row-status { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .board-row[data-status="Superseded"] .row-status { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .board-row[data-status="Canceled"] .row-status { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .board-row[data-status="Epic"] .row-status { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .board-row[data-status="Doc"] .row-status { background: var(--st-doc-bg); color: var(--st-doc-fg); }
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
  .kmark[data-kind=doc] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  @media (max-width: 720px) {
    .board-row { grid-template-columns: 10px 88px minmax(0, 1fr) 24px auto; }
    .row-parent { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .board-row { transition: none; animation: none; }
  }
</style>
