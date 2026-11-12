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
     its skeleton. EDITOR-24: the body is the editor (editor-body, the
     component a document's page writes on, `head={false}`: the title is
     ticket-head's), keyed by the ticket — a click in it puts the caret there
     and it saves as it is typed; this page is "the ticket last opened" for
     the editor at `#/` (`menger.editor.last`). SV-4: real Svelte (svelte/1), page-ticket.rhx's port
     (ui/retired-rhx/).
     The nav beside the ticket (the owner, 2026-11-12: "I want to see the nav next to the ticket … it should be
     collapsable"): a column left of the ticket holding the page's `nav` section (nav-tree-sv by
     ui/slots.json), under the top as the page scrolls, the window's height below it. Its button hides it and shows it; hidden, the section
     is not placed, so it reads nothing. Which it is is kept in this browser (`menger.ticket.nav`), shown
     unless it was hidden. On a narrow window it is not drawn. Marks: `[data-surface=ticket-nav]`
     (`data-nav=open|closed`), `[data-nav-hide]` (`[aria-expanded]`).
     The column right of the body collapses the same way (the owner: "the information section should also be
     collapsed"): the actions, the commits, the history and the revisions. `hide ›` above it takes the column
     away and gives the body the width; hidden, its parts are not placed, so they read nothing. Kept in this
     browser (`menger.ticket.side`), shown unless it was hidden. Marks: `.tp[data-side=open|closed]`,
     `[data-side-hide]` (`[aria-expanded]`). -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.revisions.list"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import TicketHead from "plexus-ui:ticket-head";
  import EditorBody from "plexus-ui:editor-body";
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

  // The nav beside the ticket: shown unless this browser hid it.
  const NAV = "menger.ticket.nav";
  let nav = $state((() => { try { return localStorage.getItem(NAV) !== "closed"; } catch { return true; } })());
  function toggleNav() {
    nav = !nav;
    try { localStorage.setItem(NAV, nav ? "open" : "closed"); } catch { /* a private window */ }
  }
  let topH = $state(0);
  // The column right of the body: shown unless this browser hid it.
  const SIDE = "menger.ticket.side";
  let side = $state((() => { try { return localStorage.getItem(SIDE) !== "closed"; } catch { return true; } })());
  function toggleSide() {
    side = !side;
    try { localStorage.setItem(SIDE, side ? "open" : "closed"); } catch { /* a private window */ }
  }

  // EDITOR-4: the ticket page is "the ticket last opened" for the editor at #/ (editor-screen reads it).
  $effect(() => { try { if (id) localStorage.setItem("menger.editor.last", id); } catch { /* a private window */ } });

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
  <div data-slot="top" data-here="ticket" bind:clientHeight={topH}></div>
  <div class="tp-frame">
  <aside class="tp-nav" data-surface="ticket-nav" data-nav={nav ? "open" : "closed"} style:top={`${topH}px`} style:height={nav ? `calc(100dvh - ${topH}px)` : null}>
    <button type="button" class="tp-nav-toggle" data-nav-hide aria-expanded={nav} aria-label={nav ? "Hide the nav" : "Show the nav"} title={nav ? "Hide the nav" : "Show the nav"} onclick={toggleNav}>{nav ? "‹ hide" : "›"}</button>
    {#if nav}<div class="tp-nav-slot" data-slot="nav"></div>{/if}
  </aside>
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
      <p class="tp-tools"><button type="button" class="tp-nav-toggle" data-side-hide aria-expanded={side} title={side ? "Hide the actions, commits and history" : "Show the actions, commits and history"} onclick={toggleSide}>{side ? "hide ›" : "‹ details"}</button></p>
      <div class="tp" data-side={side ? "open" : "closed"}>
        <div class="cell head"><TicketHead {id}/></div>
        <div class="cell main">
          <div class="body" data-surface="ticket-body">{#key id}<EditorBody {id} kind="ticket" head={false}/>{/key}</div>
          <TicketQuestions {id}/>
          <div class={knocked === 0 ? "hole" : knocked % 2 === 1 ? "hole heard" : "hole heard-again"} data-mark="ticket-edit" data-id={id}><TicketEdit {id}/></div>
          <TicketRelations {id}/>
        </div>
        {#if side}
        <aside class="cell side">
          <div class="hole" data-mark="ticket-actions" data-id={id}><TicketActions {id}/></div>
          <TicketCommits {id}/>
          <TicketHistory {id}/>
        </aside>
        <div class="cell revs"><TicketRevisions {id}/></div>
        {/if}
      </div>
    {/if}
  </article>
  </div>
  <div data-slot="palette"></div>
</section>

<style>
  .page-ticket { min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-ticket > [data-slot=top] { position: sticky; top: 0; z-index: 4; }
  .tp-frame { display: flex; align-items: flex-start; }
  .tp-frame > .ticket-page { flex: 1; min-width: 0; }
  .tp-nav { position: sticky; flex: none; width: 300px; box-sizing: border-box; overflow: auto; background: var(--panel); border-right: var(--border) solid var(--line); }
  .tp-nav[data-nav=closed] { width: auto; background: transparent; border-right: 0; }
  .tp-nav-toggle { display: block; margin: 6px 6px 2px auto; padding: 1px 8px; font: inherit; font-size: 11px; color: var(--muted); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-pill); cursor: pointer; }
  .tp-nav[data-nav=closed] .tp-nav-toggle { margin: 8px 0 0 8px; }
  .tp-nav-toggle:hover, .tp-nav-toggle:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .tp-nav-toggle:focus-visible, .tp-nav-toggle:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 2px; }
  .crumbs { display: flex; align-items: center; padding: 8px max(28px, calc((100% - 1284px) / 2)) 0; font-size: 11px; color: var(--muted); }
  .keys { display: flex; gap: 5px; align-items: center; margin-left: auto; }
  .sep { display: inline-block; width: 8px; }
  kbd { display: inline-block; min-width: 16px; padding: 0 4px; font-family: var(--font-text); font-size: 10px; line-height: 15px; text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  .tp { display: grid; grid-template-columns: minmax(0, 1fr) 400px; gap: 0 36px; align-items: start; max-width: 1340px; margin: 0 auto; padding: 16px 28px 48px; }
  .tp[data-side=closed] { grid-template-columns: minmax(0, 1fr); }
  .tp-tools { display: flex; justify-content: flex-end; margin: 0; padding: 4px max(28px, calc((100% - 1284px) / 2)) 0; }
  .tp-tools .tp-nav-toggle { margin: 0; }
  .head { grid-column: 1 / -1; grid-row: 1; }
  .main { grid-column: 1; grid-row: 3 / span 2; min-width: 0; }
  .side { grid-column: 2; grid-row: 3; min-width: 0; }
  .revs { grid-column: 2; grid-row: 4; min-width: 0; }
  .revs:has(:global([data-open="true"])) { grid-column: 1 / -1; grid-row: 2; }
  .body { padding-top: 14px; border-top: var(--border) solid var(--line); }
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
    .tp-nav { display: none; }
    .head, .main, .side, .revs, .revs:has(:global([data-open="true"])) { grid-column: 1; grid-row: auto; }
  }
  @media (prefers-reduced-motion: reduce) {
    .bone, .hole.heard, .hole.heard-again { animation: none; }
  }
</style>
