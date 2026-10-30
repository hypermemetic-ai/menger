<!-- SURFACE-6: a count for a filter, svelte/1, placed anywhere —
     `<nav-counts filter="ready and kind build"/>` (a `label` beside it,
     optional; on its own at `#/counts/:filter`) — and live by `watch
     { filter, membership }`: `nav counts` gives the total (a `recount` —
     a write moving more than 256 — folded into it), and the filter's first
     page of ids (`list { filter, fields: [id], limit: 1000 }`) is folded by
     the membership stream, `entered` in and `left` out, so the count is the
     total less that page plus what the page holds now. Exact up to 1,000
     matches; past them a ticket leaving from past the page is seen at the
     next read, and the count says so (`[data-approx]`). FRAME-40's refetch
     is the exact path.

     With `ids` (an array, from a mark: `<nav-counts filter="status Ready"
     ids={selected}/>`) it is how many of those tickets the filter admits:
     `nav member { ids, filter }`, folded by the same stream. -->
<script module>
  export const uses = ["tickets.nav.counts", "tickets.list", "tickets.watch", "tickets.nav.member"];
  const PAGE = 1000;
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { filter = null, label = null, ids = null } = $props();
  const b = bus();
  const many = () => Array.isArray(ids) && ids.length > 0;
  const membership = () => ({ filter: filter ?? null, membership: true });
  const total = live({ snapshot: "tickets.nav.counts", params: () => ({ filter: filter ?? null }), changes: "tickets.watch", changes_params: membership,
                       key: "shortcode", on: { recount: "upsert:." }, when: () => !many() });
  const held = live({ snapshot: "tickets.list", params: () => ({ filter: filter ?? null, summary: true, fields: ["id"], limit: PAGE }),
                      changes: "tickets.watch", changes_params: membership, key: "id", on: { entered: "upsert:.", left: "remove:id" }, when: () => !many() });
  const member = live({ snapshot: "tickets.nav.member", params: () => ({ ids: many() ? ids : [], filter: filter ?? null }),
                        changes: "tickets.watch", changes_params: membership, key: "id", on: { entered: "upsert:.", left: "remove:id" }, when: many });

  const sum = $derived(total.rows.reduce((a, c) => a + c.held, 0));
  const count = $derived(sum - Math.min(sum, PAGE) + held.rows.length);
  const matched = $derived(many() ? member.rows.filter((r) => r.matched !== false && ids.includes(r.id)).length : 0);
  const refusal = (...ls) => ls.find((l) => l.status === "error" || l.status === "refused");

  function open() {
    const t = (filter ?? "").replaceAll("%", "%25").replaceAll("/", "%2F").replaceAll("#", "%23");
    b.navigate(t === "" ? "#/" : "#/f/" + t);
  }
</script>

<span class="nav-counts" data-surface="nav-counts" data-filter={filter}>
  {#if many()}
    {#if refusal(member)}
      <span class="nc-refused" data-refused title={filter}>{member.message ?? "this filter is not taken"}</span>
    {:else}
      <button class="nc" type="button" data-action="open" title={filter} onclick={open}>
        {#if member.status === "pending"}<span class="nc-n" data-loading aria-busy="true"></span>{:else}<span class="nc-n" data-field="count" data-empty={matched === 0 ? "" : undefined}>{matched}</span>{/if}<span class="nc-of">of {ids.length}</span>{#if label != null}<span class="nc-label">{label}</span>{/if}
      </button>
    {/if}
  {:else if refusal(total, held)}
    <span class="nc-refused" data-refused title={filter}>{refusal(total, held).message ?? "this filter is not taken"}</span>
  {:else}
    <button class="nc" type="button" data-action="open" title={filter} onclick={open}>
      {#if total.status === "pending" || held.status === "pending"}
        <span class="nc-n" data-loading aria-busy="true"></span>
      {:else}
        <span class="nc-n" data-field="count" data-empty={count === 0 ? "" : undefined} data-approx={sum > PAGE ? "" : undefined}
              title={sum > PAGE ? "an estimate past 1,000 matches: a ticket leaving from past the first page is seen at the next read" : undefined}>{count}</span>
      {/if}{#if label != null}<span class="nc-label">{label}</span>{/if}
    </button>
  {/if}
</span>

<style>
  .nav-counts { display: inline-flex; align-items: center; font-family: var(--font-text); font-size: 11px; }
  .nc { display: inline-flex; gap: 5px; align-items: center; padding: 0 7px; font: inherit; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); cursor: pointer; transition: background-color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .nc:hover { background: var(--hover); border-color: var(--muted); }
  .nc:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .nc-n { min-width: 1ch; color: var(--ink); font-weight: var(--weight-bold); font-variant-numeric: tabular-nums; }
  .nc-n[data-empty] { color: var(--muted); font-weight: var(--weight-medium); }
  .nc-n[data-approx]::after { content: "≈"; margin-left: 2px; color: var(--muted); font-weight: var(--weight-medium); }
  .nc-n[data-loading] { width: 3ch; height: 10px; border-radius: var(--radius-sm); background: var(--skeleton); animation: nc-pulse var(--motion-pulse) var(--ease) infinite alternate; }
  .nc-of, .nc-label { color: var(--muted); }
  .nc-refused { padding: 0 7px; line-height: 18px; color: var(--warn); border: var(--border) dashed var(--warn); border-radius: var(--radius-pill); }
  @keyframes nc-pulse { from { opacity: .55; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) {
    .nc { transition: none; }
    .nc-n[data-loading] { animation: none; }
  }
</style>
