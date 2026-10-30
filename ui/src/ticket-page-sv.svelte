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
        <span class="tag status" data-ticket-status>{t.status}</span>
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
