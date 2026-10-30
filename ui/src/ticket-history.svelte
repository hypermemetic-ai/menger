<!-- SURFACE-3: a ticket's history (MENGER-13) as a timeline, newest first —
     each status, kind and archive transition: when (UTC), who, from where,
     why; a status move as two chips. `history { id }`, then live: `watch`
     says each new `transition`, folded whole (`upsert:.`, keyed by the
     transition itself — its `id` is a number the snapshot's `id` param would
     refuse); rows of other tickets are passed over here, since neither the
     fold nor the stream can drop them: `admits` reads only a row's top-level
     fields, and `watch --filter 'ticket X'` passes every ticket's
     `transition` (a service gap), so they are kept for the page's life. -->
<script module>
  export const uses = ["tickets.history", "tickets.watch"];
</script>

<script>
  import { live } from "plexus-ui/host";

  let { id, sheet = null } = $props();
  const history = live({ snapshot: "tickets.history", params: () => ({ id }), changes: "tickets.watch", key: "transition", on: { transition: "upsert:." } });
  const steps = $derived(history.rows.map((r) => r.transition).filter((x) => x && x.ticket === id).sort((a, z) => z.id - a.id));
  const two = (n) => String(n).padStart(2, "0");
  const when = (s) => { const d = new Date(s * 1000); return `${two(d.getUTCMonth() + 1)}-${two(d.getUTCDate())} ${two(d.getUTCHours())}:${two(d.getUTCMinutes())}`; };
  const full = (s) => new Date(s * 1000).toISOString().replace("T", " ").slice(0, 16) + " UTC";
</script>

<section class="ticket-history" data-surface="ticket-history">
  <h2 class="sec">History <span class="n">{steps.length}</span><span class="aside">UTC</span></h2>
  {#if sheet === "loading" || history.status === "pending"}<div data-loading aria-busy="true"><i class="bone"></i><i class="bone"></i></div>{/if}
  {#if history.status === "error" || history.status === "refused"}<p class="refused" data-refused>{history.message}</p>{/if}
  <ol class="hist">
    {#each steps as h (h.id)}
      <li class="item" data-id={h.id} data-transition={h.field}>
        <time class="at" data-field="at" data-at={h.at_unix} title={full(h.at_unix)}>{when(h.at_unix)}</time>
        <span class="by" data-field="actor" title={h.actor}>{h.name ?? h.actor.slice(0, 10) + "…"}</span>
        <span class="what">
          {#if h.field === "status"}
            {#if h.from == null}<span class="word">created ·</span>{:else}<span class="st" data-status={h.from} data-field="from">{h.from}</span><span class="ar" aria-hidden="true">→</span>{/if}
            <span class="st" data-status={h.to} data-field="to">{h.to}</span>
          {:else}
            <span class="word">{h.field}</span>
            <code data-field="from">{h.from ?? "—"}</code><span class="ar" aria-hidden="true">→</span><code data-field="to">{h.to}</code>
          {/if}
          <span class="src">{h.source}</span>
          {#if h.message}<span class="why">{h.message}</span>{/if}
        </span>
      </li>
    {/each}
  </ol>
  {#if history.status === "live" && steps.length === 0}<p class="empty" data-empty>no transitions yet</p>{/if}
</section>

<style>
  .ticket-history { color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .ticket-history .sec { display: flex; gap: 7px; align-items: baseline; margin: 22px 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .ticket-history .n { color: var(--ink); font-variant-numeric: tabular-nums; text-transform: none; letter-spacing: 0; }
  .ticket-history .aside { margin-left: auto; text-transform: none; letter-spacing: 0; }
  .ticket-history .hist { list-style: none; margin: 0; padding: 0 0 0 14px; border-left: var(--border) solid var(--tree-line); }
  .ticket-history .item { position: relative; display: grid; grid-template-columns: 76px 58px minmax(0, 1fr); gap: 6px; align-items: baseline; padding: 4px; font-size: 12px; border-radius: var(--radius); transition: background-color var(--motion-fast) var(--ease); }
  .ticket-history .item:hover, .ticket-history .item:global([data-sheet~=hover]) { background: var(--hover); }
  .ticket-history .item::before { content: ""; position: absolute; left: -18px; top: 10px; width: 7px; height: 7px; border-radius: 50%; background: var(--bg); border: var(--border) solid var(--tree-line); }
  .ticket-history .item[data-transition=status]::before { background: var(--accent); border-color: var(--accent); }
  .ticket-history .at { font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .ticket-history .by { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: var(--weight-medium); }
  .ticket-history .what { display: flex; flex-wrap: wrap; gap: 4px; align-items: center; color: var(--muted); }
  .ticket-history code { font-family: var(--font-text); color: var(--accent); }
  .ticket-history .src { padding: 0 5px; font-size: 10px; line-height: 15px; color: var(--chip-fg); background: var(--chip-bg); border-radius: var(--radius-sm); }
  .ticket-history .why { flex-basis: 100%; color: var(--ink); font-family: var(--sans); }
  .ticket-history .st { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; white-space: nowrap; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .ticket-history .st[data-status=Pending] { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .ticket-history .st[data-status=Ready] { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .ticket-history .st[data-status=Coding] { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .ticket-history .st[data-status="In Review"] { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .ticket-history .st[data-status=Complete] { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .ticket-history .st[data-status=Blocked] { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .ticket-history .st[data-status=Superseded] { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .ticket-history .st[data-status=Canceled] { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .ticket-history .st[data-status="Draft"] { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .ticket-history .st[data-status="Epic"] { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .ticket-history .st[data-status="Doc"] { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  .ticket-history .empty { margin: 0; padding: 8px 6px; font-size: 12px; color: var(--muted); }
  .ticket-history .refused { margin: 4px 0; font-size: 11px; color: var(--warn); }
  .ticket-history .bone { display: block; height: 10px; margin: 6px 0; background: var(--skeleton); border-radius: var(--radius-sm); }
  @media (prefers-reduced-motion: reduce) {
    .ticket-history .item { transition: none; }
  }
</style>
