<!-- FRAME-14: ticket-page in real Svelte (svelte/1), ticket-page.rhx's twin —
     `get` (live on its own status and amend), its title, status, kind,
     parent, blockers and unlocks as chips, the body as markdown, its
     children by one `list { parent }`, `set_status` from a menu. FRAME-25:
     on FRAME-19's bus — the body is `markdown` from plexus-ui/host, the
     engine's own render and sanitizer (what rhx's `{@html markdown(…)}`
     draws; no allow-list here), and each chip is ticket-chip-sv, a sibling
     imported from ui/src and bundled into this module. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  const FIELDS = ["id", "title", "kind", "status"];
  export const uses = ["tickets.get", "tickets.watch", "tickets.list", "tickets.set_status"];
</script>

<script>
  import { bus, live, markdown } from "plexus-ui/host";
  import Chip from "./ticket-chip-sv.svelte";

  let { id } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const STATUSES = ["Draft", "Pending", "Ready", "Coding", "In Review", "Complete", "Blocked", "Superseded", "Canceled"];

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket" } });
  const children = live({ snapshot: "tickets.list", params: () => ({ parent: id, summary: true, limit: 200, fields: FIELDS }), changes: "tickets.watch", key: "id",
                          on: { created: "upsert:ticket", amended: "upsert:ticket", status_changed: "upsert:ticket", deleted: "remove:ids" } });
  const shown = $derived($ticket.rows);
  const kids = $derived($children.rows);

  function setStatus(ev) {
    const status = ev.currentTarget.value;
    if (status !== "") b.call("tickets.set_status", { id, status }).catch((e) => console.error(`set_status: ${e?.message ?? e}`));
  }
</script>

<article class="ticket-page" data-ticket={id}>
  <nav class="crumbs"><a href={base} data-back>← board</a></nav>
  {#each shown as t (t.id)}
    <header class="ticket-head">
      <p class="ticket-code"><span data-ticket-id>{t.id}</span></p>
      <h1 class="ticket-title" data-ticket-title>{t.title}</h1>
      <div class="ticket-meta">
        <span class="tag status" data-status={t.status} data-ticket-status>{t.status}</span>
        <span class="tag kind" data-ticket-kind>{t.kind}</span>
        <label class="status-menu">set status
          <select data-status-menu onchange={setStatus}>
            <option value="">…</option>
            {#each STATUSES as s}<option value={s}>{s}</option>{/each}
          </select>
        </label>
      </div>
      <div class="chips">
        {#if t.parent != null}<Chip id={t.parent} kind="parent"/>{/if}
        {#each t.blocked_by ?? [] as bid (bid)}<Chip id={bid} kind="blocker"/>{/each}
        {#each t.unlocks ?? [] as u (u)}<Chip id={u} kind="unlocks"/>{/each}
      </div>
    </header>
    <div class="ticket-body" data-ticket-body>{@html markdown(t.body)}</div>
  {/each}
  {#if $ticket.status === "live" && shown.length === 0}<p class="empty" data-not-found>No ticket {id} here.</p>{/if}
  <section class="ticket-children">
    <h2>Children</h2>
    <ul>
      {#each kids as c (c.id)}
        <li><a data-ticket-child={c.id} data-status={c.status} href={base + "ticket/" + c.id}><span class="row-id">{c.id}</span> <span class="row-title">{c.title}</span> <span class="tag">{c.status}</span></a></li>
      {/each}
    </ul>
    {#if $children.status === "live" && kids.length === 0}<p class="empty">none</p>{/if}
  </section>
</article>

<style>
  .ticket-page { max-width: 860px; margin: 0 auto; padding: 16px 20px 40px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .crumbs { margin-bottom: 12px; font-size: 12px; }
  .crumbs a { color: var(--muted); text-decoration: none; border-radius: var(--radius-sm); }
  .crumbs a:hover { color: var(--accent); }
  .crumbs a:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .ticket-code { margin: 0; font-size: 12px; color: var(--accent); }
  .ticket-title { margin: 4px 0 10px; font-family: var(--font-display); font-size: 22px; line-height: 1.25; font-weight: var(--weight-strong); }
  .ticket-meta { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; font-size: 12px; color: var(--muted); }
  .tag { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; color: var(--muted); border: var(--border) solid var(--line); border-radius: var(--radius-md); }
  .tag.status { border-color: transparent; background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .tag.status[data-status=Draft] { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .tag.status[data-status=Pending] { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .tag.status[data-status=Ready] { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .tag.status[data-status=Coding] { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .tag.status[data-status="In Review"] { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .tag.status[data-status=Complete] { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .tag.status[data-status=Blocked] { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .tag.status[data-status=Superseded] { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .tag.status[data-status=Canceled] { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .status-menu { display: flex; gap: 6px; align-items: center; }
  .status-menu select { padding: 1px 4px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .status-menu select:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; margin: 12px 0; }
  .ticket-body { padding-top: 10px; font-family: var(--sans); line-height: 1.55; overflow-wrap: anywhere; border-top: var(--border) solid var(--line); }
  .ticket-body :global(a) { color: var(--accent); text-decoration: underline; }
  .ticket-body :global(pre) { overflow: auto; padding: 8px 10px; font-family: var(--font-text); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .ticket-body :global(code) { font-family: var(--font-text); }
  .ticket-children h2 { margin: 22px 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .ticket-children ul { list-style: none; margin: 0; padding: 0; }
  .ticket-children a { display: flex; gap: 10px; align-items: baseline; padding: 4px 6px; color: var(--ink); text-decoration: none; border-radius: var(--radius); transition: background-color var(--motion-fast) var(--ease); }
  .ticket-children a:hover { background: var(--hover); }
  .ticket-children a:focus-visible { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .row-id { flex: none; color: var(--accent); }
  .row-title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .empty { color: var(--muted); }
</style>
