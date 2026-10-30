<!-- SURFACE-7: one ticket beside another in the order of work — a chip live
     on its own id (`get`, `watch`): its status moves it, and done (Complete,
     Superseded, Canceled) it draws nothing, so it drops out of what holds or
     what is held. An id that names no ticket is a dangling chip.
     work-blockers imports it; it is a component in its own right too. -->
<script module>
  const DONE = ["Complete", "Superseded", "Canceled"];
  export const uses = ["tickets.get", "tickets.watch"];
</script>

<script>
  import { live } from "plexus-ui/host";

  let { id, side = "blocker" } = $props();
  const t = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                   on: { status_changed: "upsert:ticket", amended: "upsert:ticket", updated: "upsert:ticket", kind_changed: "upsert:ticket", deleted: "remove:ids" } });
  // A `not_found` answer is a row of its id alone: no title.
  const real = $derived(t.rows.filter((r) => r.title != null));
  const open = $derived(real.filter((r) => !DONE.includes(r.status)));
</script>

<span class="wb-one" data-surface="work-blocker">
  {#each open as r (r.id)}
    <a class="chip" data-id={r.id} data-side={side} data-status={r.status} href={"#/ticket/" + r.id} title={`${r.id} ${r.title} · ${r.status}`}>
      <i class="kmark" data-kind={r.kind} aria-hidden="true"></i>
      <span class="chip-code">{r.id}</span>
      <span class="chip-title" data-field="title">{r.title}</span>
      <i class="st" data-field="status" aria-label={r.status}></i>
    </a>
  {/each}
  {#if t.status === "live" && real.length === 0}
    <span class="chip" data-id={id} data-side={side} data-dangling title={`${id} names no ticket`}><span class="chip-code">{id}</span><span class="chip-title">no such ticket</span></span>
  {/if}
  {#if t.status === "pending"}<span class="chip" data-loading aria-busy="true"><span class="chip-code">{id}</span></span>{/if}
</span>

<style>
  .wb-one { display: inline-flex; max-width: 100%; }
  .chip { display: inline-flex; gap: 6px; align-items: center; max-width: 100%; padding: 2px 10px 2px 8px; font-size: 12px; white-space: nowrap; color: var(--chip-fg); text-decoration: none; background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); transition: background-color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  a.chip:hover { background: var(--hover); border-color: var(--line); }
  .chip:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .chip-code { color: var(--accent); }
  .chip-title { max-width: 28ch; overflow: hidden; text-overflow: ellipsis; color: var(--ink); }
  .chip[data-dangling] { border-style: dashed; border-color: var(--warn); }
  .chip[data-dangling] .chip-title { color: var(--warn); }
  .chip[data-loading] { background: var(--skeleton); }
  .chip[data-loading] .chip-code { color: transparent; }
  .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .st { flex: none; width: 6px; height: 6px; border-radius: 50%; background: var(--st-draft-fg); }
  .chip[data-status=Pending] .st { background: var(--st-pending-fg); }
  .chip[data-status=Ready] .st { background: var(--st-ready-fg); }
  .chip[data-status=Coding] .st { background: var(--st-coding-fg); }
  .chip[data-status="In Review"] .st { background: var(--st-in-review-fg); }
  .chip[data-status=Complete] .st { background: var(--st-complete-fg); }
  @media (prefers-reduced-motion: reduce) {
    .chip { transition: none; }
  }
</style>
