<!-- SURFACE-3: a ticket's subtasks (TICKETS-28) — `subtasks list { parent }`
     (it lists every child: the `<id>.N` are drawn), live by `watch` (a
     subtask made here or anywhere appears, its status moves, a delete goes),
     and `subtasks create` with a title: its answer said under the form, the
     new id or the service's refusal in its words (FRAME-37's `{ ok, events,
     message }`). -->
<script module>
  export const uses = ["tickets.subtasks.list", "tickets.watch", "tickets.subtasks.create"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { id, sheet = null } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const subtasks = live({ snapshot: "tickets.subtasks.list", params: () => ({ parent: id, limit: 200 }), changes: "tickets.watch", key: "id",
                          on: { created: "upsert:ticket", amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", deleted: "remove:ids" } });
  const subs = $derived(subtasks.rows.filter((s) => s.id.startsWith(id + ".")));
  let title = $state("");
  let refused = $state("");
  let landed = $state("");
  let busy = $state(false);

  // FRAME-37: the answer always resolves — `ok`, `events`, `message` (the
  // service's words; an invalid parameter's without the JSON-RPC prefix).
  const words = (m) => String(m ?? "refused").replace(/^Invalid params: /, "");

  async function create(ev) {
    ev?.preventDefault?.();
    if (busy) return;
    busy = true;
    const a = await b.call("tickets.subtasks.create", { parent: id, title });
    busy = false;
    if (a.ok) {
      const made = a.events.find((e) => e.type === "created")?.ticket?.id;
      landed = made ? `created ${made}` : "created";
      refused = "";
      title = "";
    } else {
      refused = words(a.message);
      landed = "";
    }
  }
  function typed() {
    refused = "";
    landed = "";
  }
</script>

<section class="ticket-subtasks" data-surface="ticket-subtasks">
  <h2 class="sec">Subtasks <span class="n">{subs.length}</span></h2>
  <div class="rows" data-field="subtasks">
    {#if sheet === "loading" || subtasks.status === "pending"}<div data-loading aria-busy="true"><i class="bone"></i></div>{/if}
    {#each subs as s (s.id)}
      <a class="row" data-id={s.id} data-status={s.status} href={base + "ticket/" + s.id}>
        <i class="kmark" data-kind={s.kind} aria-hidden="true"></i>
        <span class="row-id">{s.id}</span>
        <span class="row-title" data-field="title">{s.title}</span>
        <span class="row-kind">{s.kind}</span>
        <span class="row-status">{s.status}</span>
      </a>
    {/each}
    {#if subtasks.status === "live" && subs.length === 0}<p class="empty" data-empty>no subtasks</p>{/if}
  </div>
  <form class="add" data-form="tickets.subtasks.create" onsubmit={create}>
    <input class="in" name="title" placeholder="a subtask’s title" autocomplete="off" spellcheck="false" aria-invalid={refused ? "true" : "false"} bind:value={title} oninput={typed}/>
    <button class="btn" data-action="tickets.subtasks.create" data-idle={title === "" || busy ? "true" : "false"}>add</button>
  </form>
  {#if refused}<p class="refused" data-refused role="alert">{refused}</p>{/if}
  {#if landed}<p class="landed">{landed}</p>{/if}
</section>

<style>
  .ticket-subtasks { color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .ticket-subtasks .sec { display: flex; gap: 7px; align-items: baseline; margin: 22px 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .ticket-subtasks .n { color: var(--ink); font-variant-numeric: tabular-nums; text-transform: none; letter-spacing: 0; }
  .ticket-subtasks .rows { border-top: var(--border) solid var(--line); }
  .ticket-subtasks .row { display: grid; grid-template-columns: 10px 84px minmax(0, 1fr) auto auto; gap: 10px; align-items: center; height: var(--pitch); padding: 0 8px; color: var(--ink); text-decoration: none; white-space: nowrap; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .ticket-subtasks .row:hover, .ticket-subtasks .row:global([data-sheet~=hover]) { background: var(--hover); }
  .ticket-subtasks .row:focus-visible, .ticket-subtasks .row:global([data-sheet~=focus]) { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .ticket-subtasks .row-id { color: var(--accent); font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; }
  .ticket-subtasks .row-title { overflow: hidden; text-overflow: ellipsis; }
  .ticket-subtasks .row-kind { color: var(--muted); font-size: 11px; }
  .ticket-subtasks .row-status { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .ticket-subtasks .row[data-status=Pending] .row-status { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .ticket-subtasks .row[data-status=Ready] .row-status { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .ticket-subtasks .row[data-status=Coding] .row-status { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .ticket-subtasks .row[data-status="In Review"] .row-status { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .ticket-subtasks .row[data-status=Complete] .row-status { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .ticket-subtasks .row[data-status=Superseded] .row-status { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .ticket-subtasks .row[data-status=Canceled] .row-status { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .ticket-subtasks .row[data-status="Draft"] .row-status { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .ticket-subtasks .row[data-status="Blocked"] .row-status { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .ticket-subtasks .row[data-status="Epic"] .row-status { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .ticket-subtasks .row[data-status="Doc"] .row-status { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  .ticket-subtasks .kmark { display: inline-block; width: 8px; height: 8px; background: var(--kind-build); border-radius: 50%; }
  .ticket-subtasks .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .ticket-subtasks .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .ticket-subtasks .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .ticket-subtasks .kmark[data-kind=doc] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  .ticket-subtasks .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .ticket-subtasks .empty { margin: 0; padding: 8px 6px; font-size: 12px; color: var(--muted); }
  .ticket-subtasks .bone { display: block; height: 10px; margin: 6px 0; background: var(--skeleton); border-radius: var(--radius-sm); }
  .ticket-subtasks .add { display: flex; gap: 6px; margin: 8px 0 0; }
  .ticket-subtasks .in { flex: 1; min-width: 0; padding: 3px 8px; font: inherit; font-size: 12px; line-height: 18px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .ticket-subtasks .in::placeholder { color: var(--muted); }
  .ticket-subtasks .in:hover, .ticket-subtasks .in:global([data-sheet~=hover]) { border-color: var(--muted); }
  .ticket-subtasks .in:focus, .ticket-subtasks .in:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .ticket-subtasks .in[aria-invalid=true] { border-color: var(--warn); }
  .ticket-subtasks .btn { padding: 1px 10px; font: inherit; font-size: 12px; line-height: 20px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .ticket-subtasks .btn:hover, .ticket-subtasks .btn:global([data-sheet~=hover]) { background: var(--hover); border-color: var(--muted); }
  .ticket-subtasks .btn:focus-visible, .ticket-subtasks .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .ticket-subtasks .btn[data-idle=true] { color: var(--disabled); }
  .ticket-subtasks .refused { margin: 5px 0 0; font-size: 11px; line-height: 1.4; color: var(--warn); overflow-wrap: anywhere; }
  .ticket-subtasks .refused::before { content: ""; display: inline-block; width: 6px; height: 6px; margin-right: 6px; vertical-align: 1px; border-radius: 50%; background: var(--warn); }
  .ticket-subtasks .landed { margin: 5px 0 0; font-size: 11px; color: var(--live); }
  @media (prefers-reduced-motion: reduce) {
    .ticket-subtasks .row, .ticket-subtasks .in, .ticket-subtasks .btn { transition: none; }
  }
</style>
