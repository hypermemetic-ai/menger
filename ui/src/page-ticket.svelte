<!-- STUDIO-12: the ticket page, placing the ticket's parts, each a svelte/1
     component the host fills into its bound mark (`plexus-ui:ticket-head`):
     the head, the body, its questions, the edit, the relations (which place
     the subtasks and the blockers), the actions, the commits, the history
     and the revisions; the app's top in its `top` section (app-top by
     ui/slots.json). What the page itself decides stays here: the skeleton
     while `get` is out, "no such ticket", and a ticket deleted — `get`
     answering `deleted` (folded whole), or the ticket's newest revision
     saying `deleted` with no `undeleted` after it (ticket-revisions' spec,
     one collection) — which draws the actions alone (undelete). The ticket
     is read on ticket-head's spec word for word, so the page holds one `get`
     and one stream of it. Keys: `e` says `ticket.edit` on the bus
     (ticket-edit hears it; the page counts it in `data-edit-asked` and the
     edit's mark flashes), `escape` and `g b` go to the board — `#/board`
     (EDITOR-4: `#/` is the editor; `#/sv/` is the same board's alias); `[`
     and `]` are ticket-head's, which knows the siblings (and keeps the
     prefix). The key strip says them; the palette is the page's `palette`
     section (jump-palette-sv by ui/slots.json: app-top's jump button and
     ctrl+k). V2-50 (main's TICKETS-36): a name that fits several tickets —
     `get` answering `ambiguous { id, candidates, choices }` — draws the
     collision page in the ticket's place (ticket-collision: every choice
     with its parent, each a link by uid); one ticket is its page, its
     address put in the uid form (ticket-head); none, "No ticket"; a deleted
     one, as above. SURFACE-10: `sheet`, the sheet's held state, "loading"
     its skeleton. SV-4: real Svelte (svelte/1), page-ticket.rhx's port
     (ui/retired-rhx/). -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.revisions.list"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import TicketHead from "plexus-ui:ticket-head";
  import TicketBody from "plexus-ui:ticket-body";
  import TicketQuestions from "plexus-ui:ticket-questions";
  import TicketEdit from "plexus-ui:ticket-edit";
  import TicketRelations from "plexus-ui:ticket-relations";
  import TicketActions from "plexus-ui:ticket-actions";
  import TicketCommits from "plexus-ui:ticket-commits";
  import TicketHistory from "plexus-ui:ticket-history";
  import TicketRevisions from "plexus-ui:ticket-revisions";
  import TicketCollision from "plexus-ui:ticket-collision";

  let { id, sheet = null } = $props();
  const b = bus();
  let knocked = $state(0);

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  const latest = live({ snapshot: "tickets.revisions.list", params: () => ({ id, limit: 1 }), changes: "tickets.watch", changes_params: { revisions: true },
                        key: "ticket", on: { revised: "upsert:." } });

  b.keys({
    "e": { to: () => { b.emit("ticket.edit", { id }); knocked += 1; }, label: "edit the ticket" },
    "escape": { to: () => b.navigate("#/board"), label: "back to the board" },
    "g b": { to: () => b.navigate("#/board"), label: "go to the board" },
  });

  /** Whether this ticket's newest revision names `field` (`deleted`, `undeleted`). */
  const revised = (field) => latest.rows.some((r) => r.ticket === id && Array.isArray(r.fields) && r.fields.includes(field));
  const tombstoned = $derived(ticket.rows.some((r) => Array.isArray(r.tombstones)));
  const deleted = $derived(revised("deleted") || (!revised("undeleted") && tombstoned));
  // The deleted ticket's title: its tombstone's, else the row's.
  const goneTitle = $derived.by(() => {
    let title = null;
    for (const r of ticket.rows) if (Array.isArray(r.tombstones)) for (const x of r.tombstones) if (x.id === id && x.title != null) title = x.title;
    return title ?? ticket.rows.find((r) => r.title != null)?.title ?? null;
  });
  const loading = $derived(sheet === "loading" || ticket.status === "pending" || (ticket.status === "resyncing" && ticket.rows.length === 0));
  const found = $derived(ticket.rows.some((r) => r.title != null));
  const ambiguous = $derived(ticket.rows.some((r) => Array.isArray(r.choices) || Array.isArray(r.candidates)));
</script>

<section class="page page-ticket">
  <div data-slot="top" data-here="ticket"></div>
  <article class="ticket-page" data-surface="ticket-page" data-ticket={id} data-edit-asked={knocked}>
    <nav class="crumbs" aria-hidden="true"><span class="keys" data-keys><kbd>e</kbd> edit<i class="sep"></i><kbd>esc</kbd> board<i class="sep"></i><kbd>[</kbd><kbd>]</kbd> siblings<i class="sep"></i><kbd>?</kbd> keys</span></nav>
    {#if deleted}
      <section class="tp">
        <div class="gone" data-deleted>
          <p class="gone-line"><span class="badge">deleted</span> <strong>{id}</strong>{" was deleted" + (goneTitle != null ? " — " + goneTitle : "") + ". Its body is kept 30 days; undelete brings it back."}</p>
          <div class="hole" data-mark="ticket-actions" data-id={id}><TicketActions {id}/></div>
        </div>
      </section>
    {:else if loading}
      <div class="tp skeleton" data-loading aria-busy="true">
        <div class="wide"><i class="bone short"></i><i class="bone title"></i><i class="bone third"></i></div>
        <div class="main"><i class="bone"></i><i class="bone"></i><i class="bone half"></i><i class="bone"></i></div>
        <div class="side"><i class="bone"></i><i class="bone half"></i><i class="bone"></i></div>
      </div>
    {:else if !found}
      {#if ambiguous}
        <div class="tp one"><TicketCollision {id}/></div>
      {:else}
        <p class="tp empty" data-empty data-not-found>{"No ticket " + id + " here" + (ticket.message == null ? "" : " — " + ticket.message) + "."}</p>
      {/if}
    {:else}
      <div class="tp">
        <div class="cell head"><TicketHead {id}/></div>
        <div class="cell main">
          <TicketBody {id}/>
          <TicketQuestions {id}/>
          <div class={knocked === 0 ? "hole" : knocked % 2 === 1 ? "hole heard" : "hole heard-again"} data-mark="ticket-edit" data-id={id}><TicketEdit {id}/></div>
          <TicketRelations {id}/>
        </div>
        <aside class="cell side">
          <div class="hole" data-mark="ticket-actions" data-id={id}><TicketActions {id}/></div>
          <TicketCommits {id}/>
          <TicketHistory {id}/>
        </aside>
        <div class="cell revs"><TicketRevisions {id}/></div>
      </div>
    {/if}
  </article>
  <div data-slot="palette"></div>
</section>

<style>
  .page-ticket { min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-ticket > [data-slot=top] { position: sticky; top: 0; z-index: 4; }
  .crumbs { display: flex; align-items: center; padding: 8px max(28px, calc((100% - 1284px) / 2)) 0; font-size: 11px; color: var(--muted); }
  .keys { display: flex; gap: 5px; align-items: center; margin-left: auto; }
  .sep { display: inline-block; width: 8px; }
  kbd { display: inline-block; min-width: 16px; padding: 0 4px; font-family: var(--font-text); font-size: 10px; line-height: 15px; text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  .tp { display: grid; grid-template-columns: minmax(0, 1fr) 400px; gap: 0 36px; align-items: start; max-width: 1340px; margin: 0 auto; padding: 16px 28px 48px; }
  .head { grid-column: 1 / -1; grid-row: 1; }
  .main { grid-column: 1; grid-row: 3 / span 2; min-width: 0; }
  .side { grid-column: 2; grid-row: 3; min-width: 0; }
  .revs { grid-column: 2; grid-row: 4; min-width: 0; }
  .revs:has(:global([data-open="true"])) { grid-column: 1 / -1; grid-row: 2; }
  .hole { margin-top: 14px; border-radius: var(--radius); }
  .side > .hole { margin-top: 0; }
  .hole.heard { animation: heard var(--motion-flash) var(--ease); }
  .hole.heard-again { animation: again var(--motion-flash) var(--ease); }
  @keyframes heard { from { background: var(--sel); } to { background: transparent; } }
  @keyframes again { from { background: var(--sel); } to { background: transparent; } }
  .skeleton .wide { grid-column: 1 / -1; padding-bottom: 14px; }
  .skeleton .main, .skeleton .side { grid-row: 2; }
  .bone { display: block; width: 100%; height: 10px; margin: 6px 0; background: var(--skeleton); border-radius: var(--radius-sm); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .bone.short { width: 90px; }
  .bone.title { width: 55%; height: 24px; margin: 10px 0; }
  .bone.third { width: 33%; }
  .bone.half { width: 50%; }
  @keyframes breathe { from { opacity: 1; } to { opacity: .45; } }
  .empty { display: block; color: var(--muted); }
  .tp.one { display: block; }
  .gone { grid-column: 1 / -1; padding: 14px 16px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .gone-line { margin: 0 0 10px; }
  .badge { padding: 0 6px; font-size: 10px; line-height: 16px; text-transform: uppercase; letter-spacing: .06em; color: var(--warn); border: var(--border) dashed var(--warn); border-radius: var(--radius-sm); }
  @media (max-width: 960px) {
    .tp { grid-template-columns: minmax(0, 1fr); padding: 12px 16px 40px; }
    .crumbs { display: none; }
    .head, .main, .side, .revs, .revs:has(:global([data-open="true"])) { grid-column: 1; grid-row: auto; }
  }
  @media (prefers-reduced-motion: reduce) {
    .bone, .hole.heard, .hole.heard-again { animation: none; }
  }
</style>
