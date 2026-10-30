<!-- SURFACE-3: a ticket's relations — its children (`list { parent }`, live
     by `watch`: a child made, moved in or out, its status, a delete) with
     their roll-up; what blocks it and what it blocks (SURFACE-7's
     `work-blockers` by its mark, the ticket's own `blocked_by` / `unlocks`
     as chips beside it); its subtasks (`ticket-subtasks`, by its mark). The
     parent is the head's chip. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.list"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import Chip from "./ticket-chip-sv.svelte";
  import Subtasks from "plexus-ui:ticket-subtasks";

  let { id } = $props();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const FIELDS = ["id", "title", "kind", "status"];
  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket" } });
  const children = live({ snapshot: "tickets.list", params: () => ({ parent: id, summary: true, limit: 200, fields: FIELDS }), changes: "tickets.watch", key: "id",
                          on: { created: "upsert:ticket", amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket",
                                parent_changed: "upsert:ticket", deleted: "remove:ids" } });
  const t = $derived(ticket.rows.find((r) => r.title != null));
  const kids = $derived(children.rows.filter((c) => !c.id.startsWith(id + ".")));
  const done = $derived(kids.filter((c) => c.status === "Complete").length);
</script>

<section class="ticket-relations" data-surface="ticket-relations">
  <h2 class="sec">Children <span class="n">{kids.length}</span><span class="aside"><b>{done}</b> of {kids.length} done</span></h2>
  <div class="rows" data-field="children">
    {#if children.status === "pending"}<div data-loading aria-busy="true"><i class="bone"></i><i class="bone"></i></div>{/if}
    {#each kids as c (c.id)}
      <a class="row" data-id={c.id} data-ticket-child={c.id} data-status={c.status} href={base + "ticket/" + c.id}>
        <i class="kmark" data-kind={c.kind} aria-hidden="true"></i>
        <span class="row-id" data-field="id">{c.id}</span>
        <span class="row-title" data-field="title">{c.title}</span>
        <span class="row-kind" data-field="kind">{c.kind}</span>
        <span class="row-status" data-field="status">{c.status}</span>
      </a>
    {/each}
    {#if children.status === "live" && kids.length === 0}<p class="empty" data-empty>no children</p>{/if}
  </div>
  <h2 class="sec">Blocked by · blocks</h2>
  <div class="hole" data-mark="work-blockers" data-slot="work-blockers" data-component="work-blockers" data-id={id}></div>
  {#if t}
    <div class="chips">
      <span class="set" data-field="blocked_by">{#each t.blocked_by ?? [] as bid (bid)}<Chip id={bid} kind="blocker"/>{/each}</span>
      <span class="set" data-field="unlocks">{#each t.unlocks ?? [] as u (u)}<Chip id={u} kind="unlocks"/>{/each}</span>
      {#if !(t.blocked_by ?? []).length && !(t.unlocks ?? []).length}<span class="empty" data-empty>nothing blocks it, and it blocks nothing</span>{/if}
    </div>
  {/if}
  <Subtasks id={id}/>
</section>

<style>
  .ticket-relations { color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .ticket-relations .sec { display: flex; gap: 7px; align-items: baseline; margin: 22px 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .ticket-relations .n { color: var(--ink); font-variant-numeric: tabular-nums; text-transform: none; letter-spacing: 0; }
  .ticket-relations .aside { margin-left: auto; text-transform: none; letter-spacing: 0; }
  .ticket-relations .aside b { color: var(--live); font-weight: var(--weight-medium); }
  .ticket-relations .rows { border-top: var(--border) solid var(--line); }
  .ticket-relations .row { display: grid; grid-template-columns: 10px 84px minmax(0, 1fr) auto auto; gap: 10px; align-items: center; height: var(--pitch); padding: 0 8px; color: var(--ink); text-decoration: none; white-space: nowrap; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .ticket-relations .row:hover { background: var(--hover); }
  .ticket-relations .row:focus-visible { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .ticket-relations .row-id { color: var(--accent); font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; }
  .ticket-relations .row-title { overflow: hidden; text-overflow: ellipsis; }
  .ticket-relations .row-kind { color: var(--muted); font-size: 11px; }
  .ticket-relations .row-status { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .ticket-relations .row[data-status=Pending] .row-status { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .ticket-relations .row[data-status=Ready] .row-status { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .ticket-relations .row[data-status=Coding] .row-status { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .ticket-relations .row[data-status="In Review"] .row-status { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .ticket-relations .row[data-status=Complete] .row-status { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .ticket-relations .row[data-status=Blocked] .row-status { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .ticket-relations .row[data-status=Superseded] .row-status { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .ticket-relations .row[data-status=Canceled] .row-status { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .ticket-relations .kmark { display: inline-block; width: 8px; height: 8px; background: var(--kind-build); border-radius: 50%; }
  .ticket-relations .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .ticket-relations .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .ticket-relations .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .ticket-relations .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .ticket-relations .kmark[data-kind=doc] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--muted); border-radius: var(--radius-xs); }
  .ticket-relations .empty { display: block; padding: 8px 6px; font-size: 12px; color: var(--muted); }
  .ticket-relations .hole:empty { display: flex; align-items: center; min-height: 34px; padding: 6px 10px; border: var(--border) dashed var(--tree-line); border-radius: var(--radius); }
  .ticket-relations .hole:empty::before { content: "<work-blockers id/>"; font-size: 11px; color: var(--muted); }
  .ticket-relations .chips { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin-top: 8px; }
  .ticket-relations .set { display: contents; }
  .ticket-relations .bone { display: block; height: 10px; margin: 6px 0; background: var(--skeleton); border-radius: var(--radius-sm); }
  @media (max-width: 720px) {
    .ticket-relations .row { grid-template-columns: 10px 84px minmax(0, 1fr) auto; }
    .ticket-relations .row-kind { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .ticket-relations .row { transition: none; }
  }
</style>
