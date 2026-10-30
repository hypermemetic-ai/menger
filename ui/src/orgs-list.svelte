<!-- SURFACE-9: the organizations the reader holds a role in (#/orgs),
     svelte/1 — `orgs.list`, one `org { name, aliases, roots, shortcodes,
     tickets }` each, in the order they were made. A one-shot read: the
     organizations have no watch. -->
<script module>
  export const uses = ["orgs.list"];
</script>

<script>
  import { live } from "plexus-ui/host";

  const orgs = live({ snapshot: "orgs.list", params: {}, key: "name", on: {} });
  const root = (r) => r.split(":").pop();
</script>

<section class="orgs-list" data-surface="orgs-list">
  <div class="page-head">
    <h1>organizations</h1>
    <span class="sub">the ones you hold a role in</span>
    {#if $orgs.status !== "live"}<span class="state" data-loading={$orgs.status}>{$orgs.status}</span>{/if}
  </div>
  <div class="panel">
    <div class="th"><span>organization</span><span>tickets</span><span>shortcodes</span><span>was</span><span>roots</span></div>
    {#each orgs.rows as o (o.name)}
      <div class="tr" data-id={o.name}>
        <span class="name" data-field="name">{o.name}</span>
        <span class="meta" data-field="tickets">{o.tickets}</span>
        <span class="meta" data-field="shortcodes">{o.shortcodes}</span>
        <span class="meta" data-field="aliases">{(o.aliases ?? []).join(", ")}</span>
        <span class="roots" data-field="roots">{#each o.roots ?? [] as r (r)}<span class="chip">{root(r)}</span>{/each}</span>
      </div>
    {/each}
    {#if $orgs.status === "pending"}<div class="tr"><span data-loading="">hypermemetic</span></div>{/if}
    {#if $orgs.status === "live" && orgs.rows.length === 0}<p class="empty" data-empty="">You hold a role in no organization.</p>{/if}
    {#if $orgs.status === "error" || $orgs.status === "refused"}<p class="empty" data-refused="">refused: {$orgs.message}</p>{/if}
  </div>
</section>

<style>
  .orgs-list { max-width: 1180px; margin: 0 auto; padding: 18px 24px 40px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .page-head { display: flex; align-items: baseline; gap: 10px; margin: 0 0 14px; }
  .page-head h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); }
  .sub { color: var(--muted); font-size: 12px; }
  .state { margin-left: auto; font-size: 11px; color: var(--muted); }
  .panel { overflow: hidden; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .th, .tr { display: grid; grid-template-columns: minmax(140px, 1fr) 90px 90px minmax(0, 1fr) minmax(0, 2fr); gap: 0 12px; align-items: center; padding: 0 10px; }
  .th { padding-top: 6px; padding-bottom: 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); border-bottom: var(--border) solid var(--line); }
  .tr { min-height: 34px; padding-top: 4px; padding-bottom: 4px; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .tr:last-child { border-bottom: 0; }
  .tr:hover { background: var(--hover); }
  .name { color: var(--accent); font-weight: var(--weight-medium); }
  .meta { color: var(--muted); font-size: 12px; font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .roots { display: flex; flex-wrap: wrap; gap: 4px; }
  .chip { padding: 0 7px; font-size: 11px; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  [data-loading] { color: transparent; background: var(--skeleton); border-radius: var(--radius-sm); }
  .empty { margin: 8px 10px; color: var(--muted); }
  [data-refused] { color: var(--warn); }
  @media (prefers-reduced-motion: reduce) { .tr { transition: none; } }
  @media (max-width: 720px) { .th { display: none; } .tr { grid-template-columns: 1fr auto; } .tr > span:nth-child(n+3) { display: none; } }
</style>
