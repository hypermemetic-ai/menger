<!-- SURFACE-3: the ticket page, a composition (FRAME-29) in real Svelte
     (svelte/1) — each part of a ticket is a component the host fills into
     its bound mark (`plexus-ui:<name>`): the head, the body, the relations
     (children, blockers, subtasks), the commits, the history and the
     revisions; SURFACE-4's `ticket-edit` and `ticket-actions` placed the same
     way as marks (`[data-slot][data-component]`), holes until their
     components are in the bundle. The page reads `get` on `watch` (the spec
     every part reads: one snapshot, one stream) for its skeleton and its
     "no such ticket", and `tomb` for what is not a ticket row — `deleted`,
     `undeleted`, `archived`, `restored` — so it says "deleted" and offers the
     actions mark (undelete). Keys (FRAME-36): `e` says `ticket.edit` on the
     bus (SURFACE-4's edit hears it), `escape` and `g b` go to the board.
     ticket-page.rhx is the rhx board's page, kept as it was. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import Head from "plexus-ui:ticket-head";
  import Body from "plexus-ui:ticket-body";
  import Relations from "plexus-ui:ticket-relations";
  import Commits from "plexus-ui:ticket-commits";
  import History from "plexus-ui:ticket-history";
  import Revisions from "plexus-ui:ticket-revisions";

  let { id } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket" } });
  const tomb = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "ids",
                      on: { deleted: "upsert:.", undeleted: "upsert:.", archived: "upsert:.", restored: "upsert:." } });
  const t = $derived(ticket.rows.find((r) => r.title != null));
  // The last word on this ticket's life: a deleted row with its tombstones, or undeleted.
  const life = $derived(tomb.rows.filter((r) => Array.isArray(r.ids) && r.ids.includes(id) && (r.tombstones || r.tickets)).at(-1));
  const gone = $derived(life?.tombstones ? life.tombstones.find((x) => x.id === id) ?? life.tombstones[0] ?? {} : null);
  const loading = $derived(ticket.status === "pending" || (ticket.status === "resyncing" && ticket.rows.length === 0));

  b.keys({
    e: { to: () => b.emit("ticket.edit", { id }), label: "edit the ticket" },
    escape: { to: () => b.navigate(base), label: "back to the board" },
    "g b": { to: () => b.navigate(base), label: "go to the board" },
  });
</script>

<article class="ticket-page" data-surface="ticket-page" data-ticket={id}>
  <nav class="crumbs">
    <a href={base} data-back>← board</a>
    <span class="keys" aria-hidden="true"><kbd>e</kbd> edit<i class="sep"></i><kbd>esc</kbd> board<i class="sep"></i><kbd>[</kbd><kbd>]</kbd> siblings<i class="sep"></i><kbd>?</kbd> keys</span>
  </nav>
  {#if gone}
    <section class="tp">
      <div class="gone" data-deleted>
        <p class="gone-line"><span class="badge">deleted</span> <strong>{id}</strong> was deleted{gone.title ? ` — ${gone.title}` : ""}. Its body is kept 30 days; undelete brings it back.</p>
        <div class="hole" data-mark="ticket-actions" data-slot="ticket-actions" data-component="ticket-actions" data-id={id}></div>
      </div>
    </section>
  {:else if loading}
    <div class="tp skeleton" data-loading aria-busy="true">
      <div class="wide"><i class="bone short"></i><i class="bone title"></i><i class="bone third"></i></div>
      <div class="main"><i class="bone"></i><i class="bone"></i><i class="bone half"></i><i class="bone"></i></div>
      <div class="side"><i class="bone"></i><i class="bone half"></i><i class="bone"></i></div>
    </div>
  {:else if !t}
    <p class="tp empty" data-empty data-not-found>No ticket {id} here{ticket.message ? ` — ${ticket.message}` : ""}.</p>
  {:else}
    <div class="tp">
      <div class="cell head"><Head id={id}/></div>
      <div class="cell main">
        <Body id={id}/>
        <div class="hole" data-mark="ticket-edit" data-slot="ticket-edit" data-component="ticket-edit" data-id={id}></div>
        <Relations id={id}/>
      </div>
      <aside class="cell side">
        <div class="hole" data-mark="ticket-actions" data-slot="ticket-actions" data-component="ticket-actions" data-id={id}></div>
        <Commits id={id}/>
        <History id={id}/>
      </aside>
      <div class="cell revs"><Revisions id={id}/></div>
    </div>
  {/if}
</article>

<style>
  .ticket-page { min-height: 100%; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .ticket-page .crumbs { position: sticky; top: 0; z-index: 4; display: flex; gap: 14px; align-items: center; padding: 10px max(28px, calc((100% - 1284px) / 2)); font-size: 12px; background: var(--bg); border-bottom: var(--border) solid var(--line); }
  .ticket-page .crumbs a { color: var(--muted); text-decoration: none; border-radius: var(--radius-sm); transition: color var(--motion-fast) var(--ease); }
  .ticket-page .crumbs a:hover { color: var(--accent); }
  .ticket-page .crumbs a:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .ticket-page .keys { display: flex; gap: 5px; align-items: center; margin-left: auto; font-size: 11px; color: var(--muted); }
  .ticket-page .sep { display: inline-block; width: 8px; }
  .ticket-page kbd { display: inline-block; min-width: 16px; padding: 0 4px; font-family: var(--font-text); font-size: 10px; line-height: 15px; text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  .ticket-page .tp { display: grid; grid-template-columns: minmax(0, 1fr) 400px; gap: 0 36px; align-items: start; max-width: 1340px; margin: 0 auto; padding: 16px 28px 48px; }
  .ticket-page .head { grid-column: 1 / -1; grid-row: 1; }
  .ticket-page .main { grid-column: 1; grid-row: 3 / span 2; min-width: 0; }
  .ticket-page .side { grid-column: 2; grid-row: 3; min-width: 0; }
  .ticket-page .revs { grid-column: 2; grid-row: 4; min-width: 0; }
  .ticket-page .revs:has(:global([data-open="true"])) { grid-column: 1 / -1; grid-row: 2; }
  .ticket-page .hole { margin-top: 14px; }
  .ticket-page .side > .hole { margin-top: 0; }
  .ticket-page .hole:empty { display: flex; align-items: center; min-height: 34px; padding: 6px 10px; border: var(--border) dashed var(--tree-line); border-radius: var(--radius); }
  .ticket-page .hole:empty::before { content: "<" attr(data-component) " id/>"; font-size: 11px; color: var(--muted); }
  .ticket-page .skeleton .wide { grid-column: 1 / -1; padding-bottom: 14px; }
  .ticket-page .skeleton .main, .ticket-page .skeleton .side { grid-row: 2; }
  .ticket-page .bone { display: block; width: 100%; height: 10px; margin: 6px 0; background: var(--skeleton); border-radius: var(--radius-sm); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .ticket-page .bone.short { width: 90px; }
  .ticket-page .bone.title { width: 55%; height: 24px; margin: 10px 0; }
  .ticket-page .bone.third { width: 33%; }
  .ticket-page .bone.half { width: 50%; }
  @keyframes breathe { from { opacity: 1; } to { opacity: .45; } }
  .ticket-page .empty { display: block; color: var(--muted); }
  .ticket-page .gone { grid-column: 1 / -1; padding: 14px 16px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .ticket-page .gone-line { margin: 0 0 10px; }
  .ticket-page .badge { padding: 0 6px; font-size: 10px; line-height: 16px; text-transform: uppercase; letter-spacing: .06em; color: var(--warn); border: var(--border) dashed var(--warn); border-radius: var(--radius-sm); }
  @media (max-width: 960px) {
    .ticket-page .tp { grid-template-columns: minmax(0, 1fr); padding: 12px 16px 40px; }
    .ticket-page .head, .ticket-page .main, .ticket-page .side, .ticket-page .revs, .ticket-page .revs:has(:global([data-open="true"])) { grid-column: 1; grid-row: auto; }
    .ticket-page .keys { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .ticket-page .bone { animation: none; }
    .ticket-page .crumbs a { transition: none; }
  }
</style>
