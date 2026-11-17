<!-- EDITOR-8: the writing screen (svelte/1) — one note or ticket on one
     surface, and the notes beside it. The surface is editor-body (the body
     drawn as ticket-view draws it, edited in place), ticket-questions (V2-20)
     under it for a ticket; nothing is drawn twice. V2-29: the notes are the
     notes service's (NOTES-3: the docs service renamed; the owner, 2026-11-15:
     "we need documents renamed to notes"). The list, a narrow column (a
     drawer under 1024 px, `notes` in the bar): a search (`notes.list
     { query }`, by title), the notes and tickets opened here lately
     (`menger.editor.recent`, this browser's) and the notes you read, newest
     first (`notes.list`, live by `notes.watch`). `+ new` makes a note
     (`notes.create { title: "Untitled" }`) and opens it at `#/note/<id>`.
     `#/note/:note` opens a note on the surface (`#/doc/:doc`, its link before
     NOTES-3, the same), `#/edit/:id` a ticket, as before. Switching keeps
     autosave: the editor is keyed by what it edits, and the one left sends
     what it holds as it goes. `#/` opens the one last opened in this browser
     (`menger.editor.last`: a ticket's id, or `note:<id>` — `doc:<id>`, as a
     browser kept it before NOTES-3, read the same). Keys: `escape` (outside
     the text) and the bar's *board* go to `#/board`; `n` is *new*; `/` the
     search. NOTES-4: a note's page is the note on the left and its slot
     on the right (note-slot, placed by its bound mark), each scrolling on its
     own; under 760 px the two are tabs, the note first; the list of notes is
     a drawer at every width on a note. A ticket is drawn as before, with no
     slot. The page is flat on a note: hairlines, no shadow. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "notes.list", "notes.watch", "notes.create"];
  export const LAST = "menger.editor.last";
  export const RECENT = "menger.editor.recent";
  /** What `menger.editor.last` names: `note:<id>` a note (`doc:<id>` before NOTES-3), anything else a ticket's id. */
  export const lastOf = (v) => (!v ? null : /^(note|doc):/.test(v) ? { kind: "note", id: v.slice(v.indexOf(":") + 1) } : { kind: "ticket", id: v });
  /** A kind a browser kept: `doc` (before NOTES-3) is a note; none, a ticket. */
  export const kindOf = (k) => (k === "doc" || k === "note" ? "note" : "ticket");
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import { ticketHref, editHref, settle } from "./ticket-links.svelte";
  import Editor from "plexus-ui:editor-body";
  import Questions from "plexus-ui:ticket-questions";
  import NoteSlot from "plexus-ui:note-slot";

  let { id = null, note = null, uid = null, sheet = null } = $props();
  const b = bus();
  // V2-42: `#/t/<uid>/<ID>-<slug>/edit` names the ticket by its uid — `get`
  // takes it and answers the ticket whatever its id is now; the surface is
  // then the ticket by the key it answered, as `#/edit/<id>` is.
  const byUid = live({ snapshot: "tickets.get", params: () => ({ id: uid }), key: "id", when: () => uid != null && id == null && note == null });
  const resolved = $derived(uid != null && id == null && note == null ? (byUid.rows.find((r) => r.id && r.title != null)?.id ?? null) : null);
  const remembered = () => { try { return localStorage.getItem(LAST) || null; } catch { return null; } };
  const remember = (v) => { try { localStorage.setItem(LAST, v); } catch { /* a private window */ } };
  const readRecent = () => { try { const r = JSON.parse(localStorage.getItem(RECENT) || "[]"); return Array.isArray(r) ? r.filter((x) => x && typeof x.id === "string") : []; } catch { return []; } };
  // What is on screen: the route's (a note, or a ticket), else the last opened here.
  let last = $state(remembered());
  const route = $derived(note ? { kind: "note", id: note } : id ? { kind: "ticket", id } : resolved ? { kind: "ticket", id: resolved } : null);
  const cur = $derived(route ?? (uid != null && id == null && note == null ? null : lastOf(last)));
  const current = $derived(cur?.id ?? null);
  const isNote = $derived(cur?.kind === "note");
  const href = (r) => (kindOf(r.kind) === "note" ? "#/note/" + r.id : editHref(r));
  $effect(() => { if (route) { const v = route.kind === "note" ? "note:" + route.id : route.id; remember(v); last = v; } });

  // A ticket on the surface: its kind for the bar (the editor holds the same get and watch).
  const ticket = live({ snapshot: "tickets.get", params: () => ({ id: current }), changes: "tickets.watch", key: "id", when: () => current != null && !isNote,
                        on: { amended: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket", deleted: "upsert:." } });
  const t = $derived(isNote ? null : ticket.rows.find((r) => r.id === current && r.title != null));
  // V2-42: `#/edit/<id>` becomes `#/t/<uid>/<ID>-<slug>/edit` in place (`#/` stays `#/`).
  $effect(() => { if (route && t?.uid) settle(t); });

  // ── the notes ────────────────────────────────────────────────────────────
  const notes = live({ snapshot: "notes.list", params: { limit: 200 }, changes: "notes.watch", changes_params: {}, key: "id",
                      on: { created: "upsert:note", updated: "upsert:note", renamed: "upsert:note", deleted: "remove:id" } });
  const theirs = $derived(notes.rows.filter((r) => r.id && r.title != null).slice().sort((a, b) => (b.created_at ?? 0) - (a.created_at ?? 0)));
  const titled = $derived(new Map(notes.rows.filter((r) => r.id && r.title != null).map((r) => [r.id, r.title])));
  let recent = $state(readRecent());
  /** The one on the surface goes first in recent, by its title now. */
  $effect(() => {
    const title = isNote ? titled.get(current) : t?.title;
    if (!cur || title == null) return;
    const row = { id: cur.id, title, kind: cur.kind, ...(isNote || !t?.uid ? {} : { uid: t.uid }) };
    const next = [row, ...recent.filter((r) => r.id !== row.id)].slice(0, 12);
    if (JSON.stringify(next) === JSON.stringify(recent)) return;
    recent = next;
    try { localStorage.setItem(RECENT, JSON.stringify(next)); } catch { /* a private window */ }
  });
  let q = $state("");
  const found = live({ snapshot: "notes.list", params: () => ({ query: q.trim(), limit: 20 }), key: "id", when: () => q.trim().length > 0, debounce_ms: 150 });
  const hits = $derived(q.trim() ? found.rows.filter((r) => r.id && r.title != null) : []);
  let drawer = $state(false);
  // NOTES-4: under 760 px the note and its slot are two tabs, the note first.
  let tab = $state("note");
  $effect(() => { void current; tab = "note"; });
  let searchBox = $state(null);
  const open = (to) => { drawer = false; q = ""; if (to.id !== current || to.kind !== cur?.kind) b.navigate(href(to)); };

  let making = $state(false);
  let refused = $state("");
  async function create() {
    if (making) return;
    making = true;
    refused = "";
    const r = await b.call("notes.create", { title: "Untitled" });
    making = false;
    const made = r.events?.find((e) => e.type === "created")?.note;
    if (r.ok && made) { remember("note:" + made.id); drawer = false; b.navigate("#/note/" + made.id); }
    else refused = String(r.message ?? r.events?.[0]?.message ?? "refused").replace(/^Invalid params: /, "");
  }
  b.keys({
    escape: { to: () => (drawer ? (drawer = false) : b.navigate("#/board")), label: "the board" },
    n: { to: create, label: "a new note" },
    "/": { to: () => { drawer = true; searchBox?.focus(); }, label: "find a note" },
  });
</script>

<div class="editor-screen" data-surface="editor-screen" data-ticket={current ?? undefined} data-kind={cur?.kind} data-drawer={drawer ? "open" : undefined}>
  <div class="bar">
    <button type="button" class="btn notes" data-action="notes" aria-expanded={drawer} onclick={() => (drawer = !drawer)}>notes</button>
    {#if current && isNote}
      <span class="kind" data-field="kind"><i class="kmark" data-kind="note" aria-hidden="true"></i>note</span>
      <span class="id" data-field="current">{current}</span>
    {:else if current}
      <a class="id" href={ticketHref(t ?? { id: current })} data-field="current">{current}</a>
      {#if t}<span class="kind" data-field="kind"><i class="kmark" data-kind={t.kind} aria-hidden="true"></i>{t.kind}</span>{/if}
    {:else}
      <span class="none" data-empty>nothing open yet</span>
    {/if}
    <span class="right">
      <a class="board" href="#/board" data-action="board"><kbd>esc</kbd> board</a>
    </span>
    {#if refused}<p class="refused" data-refused role="alert">{refused}</p>{/if}
  </div>
  <div class="main">
    <nav class="list" data-field="notes" aria-label="notes">
      <button type="button" class="btn primary new" data-action="new" disabled={making || sheet === "loading"} onclick={create}>+ new note <kbd>n</kbd></button>
      <input class="find" data-field="find" type="search" placeholder="find a note…" aria-label="find a note" autocomplete="off" spellcheck="false" bind:value={q} bind:this={searchBox}/>
      {#if q.trim()}
        <p class="head">found</p>
        {#each hits as r (r.id)}
          <button type="button" class="note" data-note={r.id} data-current={r.id === current ? "" : undefined} onclick={() => open({ kind: "note", id: r.id })}>{r.title}</button>
        {:else}
          <p class="none">{found.status === "pending" ? "finding…" : "nothing found"}</p>
        {/each}
      {:else}
        {#if recent.length}
          <p class="head">recent</p>
          {#each recent as r (r.id)}
            <button type="button" class="note" data-note={r.id} data-recent data-current={r.id === current ? "" : undefined} onclick={() => open({ kind: kindOf(r.kind), id: r.id })}>{#if kindOf(r.kind) === "ticket"}<span class="did">{r.id}</span>{/if}{r.title}</button>
          {/each}
        {/if}
        <p class="head">notes</p>
        {#each theirs as r (r.id)}
          <button type="button" class="note" data-note={r.id} data-current={r.id === current ? "" : undefined} onclick={() => open({ kind: "note", id: r.id })}>{r.title}</button>
        {:else}
          <p class="none">{notes.status === "pending" ? "…" : "none yet"}</p>
        {/each}
      {/if}
    </nav>
    <button type="button" class="scrim" aria-label="close the notes" tabindex="-1" onclick={() => (drawer = false)}></button>
    <div class="page" data-two={current && isNote ? "" : undefined} data-tab={current && isNote ? tab : undefined}>
      {#if current && isNote}
        <div class="tabs" role="tablist" aria-label="the note and its slot">
          <button type="button" role="tab" class="tab" data-tab="note" aria-selected={tab === "note"} onclick={() => (tab = "note")}>note</button>
          <button type="button" role="tab" class="tab" data-tab="slot" aria-selected={tab === "slot"} onclick={() => (tab = "slot")}>slot</button>
        </div>
        <div class="col note-col" data-col="note">
          <div class="sheet">{#key `${cur.kind}:${current}`}<Editor id={current} kind={cur.kind} {sheet}/>{/key}</div>
        </div>
        <div class="col slot-col" data-col="slot">{#key current}<NoteSlot note={current}/>{/key}</div>
      {:else if current}
        <div class="sheet">
          {#key `${cur.kind}:${current}`}<Editor id={current} kind={cur.kind} {sheet}/>{/key}
          {#key current}<Questions id={current}/>{/key}
        </div>
      {:else}
        <div class="welcome" data-welcome>
          <p class="lead">Write a note here. It saves as you type.</p>
          <p class="muted">Open one from the list, a ticket from the <a href="#/board">board</a>, or make a new one.</p>
          <button type="button" class="btn primary" data-action="new" disabled={making} onclick={create}>+ new note <kbd>n</kbd></button>
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .editor-screen { display: flex; flex-direction: column; flex: 1; min-height: 0; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .bar { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; flex: none; min-height: 36px; padding: 4px 14px; font-size: 12px; color: var(--muted); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .id { color: var(--accent); text-decoration: none; }
  .id:hover, .id:global([data-sheet~=hover]) { text-decoration: underline; }
  .id:focus-visible, .id:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; border-radius: var(--radius-sm); }
  .kind { display: inline-flex; gap: 5px; align-items: center; }
  .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind="scope"] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind="gap"] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind="spike"] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind="execution"] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .kmark[data-kind="note"] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  .right { margin-left: auto; display: flex; gap: 10px; align-items: center; }
  .board { display: inline-flex; gap: 5px; align-items: center; color: var(--muted); text-decoration: none; }
  .board:hover, .board:global([data-sheet~=hover]) { color: var(--ink); }
  .board:focus-visible, .board:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; border-radius: var(--radius-sm); }
  .btn { display: inline-flex; gap: 5px; align-items: center; padding: 1px 10px; font-size: 12px; line-height: 20px; white-space: nowrap; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .btn:hover, .btn:global([data-sheet~=hover]) { background: var(--hover); border-color: var(--muted); }
  .btn:focus-visible, .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn:disabled, .btn:global([data-sheet~=disabled]) { color: var(--disabled); background: var(--card); border-color: var(--line); cursor: not-allowed; }
  .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .btn.primary:disabled, .btn.primary:global([data-sheet~=disabled]) { color: var(--disabled); background: var(--card); border-color: var(--line); }
  .notes { display: none; }
  .refused { flex-basis: 100%; margin: 0; color: var(--warn); }
  .main { position: relative; flex: 1; min-height: 0; display: grid; grid-template-columns: 232px minmax(0, 1fr); }
  .list { display: flex; flex-direction: column; gap: 2px; min-height: 0; overflow: auto; padding: 12px 10px 24px; background: var(--panel); border-right: var(--border) solid var(--line); }
  .new { justify-content: center; margin-bottom: 8px; }
  .find { margin-bottom: 6px; padding: 4px 8px; font: inherit; font-size: 12px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .find:focus-visible, .find:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 0; }
  .head { margin: 10px 4px 2px; font-size: 10px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  .note { display: block; width: 100%; padding: 4px 6px; overflow: hidden; font: inherit; font-size: 12px; line-height: 1.35; text-align: left; text-overflow: ellipsis; white-space: nowrap; color: var(--ink); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .note:hover, .note:global([data-sheet~=hover]) { background: var(--hover); }
  .note:focus-visible, .note:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .note[data-current], .note:global([data-sheet~=selected]) { background: var(--row-sel); }
  .did { margin-right: 6px; font-size: 11px; color: var(--muted); }
  .scrim { display: none; }
  .page { min-width: 0; min-height: 0; overflow: auto; }
  .sheet { max-width: 760px; margin: 0 auto; padding: 24px 28px 64px; }
  .welcome { padding: 40px 20px; text-align: center; }
  .lead { margin: 0 0 6px; font: var(--weight-strong) 18px var(--font-display); }
  .muted { margin: 0 0 14px; color: var(--muted); }
  .muted a { color: var(--accent); }
  .none { margin: 2px 6px; font-size: 12px; color: var(--muted); }
  kbd { display: inline-block; min-width: 16px; padding: 0 4px; font-family: var(--font-text); font-size: 10px; line-height: 15px; text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  @media (max-width: 1023px) {
    .notes { display: inline-flex; }
    .main { grid-template-columns: minmax(0, 1fr); }
    .list { position: absolute; z-index: 4; top: 0; bottom: 0; left: 0; width: min(280px, 82vw); transform: translateX(-100%); transition: transform var(--motion-fast) var(--ease); box-shadow: 0 6px 18px var(--shadow); padding-bottom: calc(24px + env(safe-area-inset-bottom)); }
    .editor-screen[data-drawer=open] .list { transform: none; }
    .editor-screen[data-drawer=open] .scrim { display: block; position: absolute; inset: 0; z-index: 3; padding: 0; background: var(--shadow); border: 0; }
    .sheet { padding: 16px 16px calc(48px + env(safe-area-inset-bottom)); }
    .board kbd { display: none; }
  }
  /* EDITOR-12: on a phone the drawer's titles wrap (two lines) rather than run past its edge. */
  @media (max-width: 640px) {
    .note { display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; white-space: normal; overflow-wrap: anywhere; }
    .bar { padding: 4px 12px; }
    .sheet { padding-left: 12px; padding-right: 12px; }
  }
  /* NOTES-4: a note's page — the note on --panel, its slot on --bg, each scrolling alone; the list a drawer at every width. */
  .editor-screen[data-kind=note] .notes { display: inline-flex; }
  .editor-screen[data-kind=note] .main { grid-template-columns: minmax(0, 1fr); }
  .editor-screen[data-kind=note] .list { position: absolute; z-index: 4; top: 0; bottom: 0; left: 0; width: min(280px, 82vw); transform: translateX(-100%); transition: transform var(--motion-fast) var(--ease); box-shadow: none; border-right: var(--border) solid var(--line); }
  .editor-screen[data-kind=note][data-drawer=open] .list { transform: none; }
  .editor-screen[data-kind=note][data-drawer=open] .scrim { display: block; position: absolute; inset: 0; z-index: 3; padding: 0; background: transparent; border: 0; }
  .page[data-two] { display: grid; grid-template-columns: minmax(0, 42fr) minmax(0, 58fr); overflow: hidden; }
  .page[data-two] .col { min-width: 0; min-height: 0; overflow: auto; }
  .note-col { container: note-col / inline-size; background: var(--panel); border-right: var(--border) solid var(--line); }
  .slot-col { background: var(--bg); }
  .slot-col > :global([data-slot=note-slot]) { min-height: 100%; }
  .page[data-two] .sheet { max-width: 720px; }
  .tabs { display: none; }
  @media (max-width: 759px) {
    .page[data-two] { grid-template-columns: minmax(0, 1fr); grid-template-rows: auto minmax(0, 1fr); }
    .tabs { display: flex; gap: 0; margin: 8px 12px; padding: 2px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); }
    .tab { flex: 1; padding: 3px 0; font: inherit; font-family: var(--mono); font-size: 13px; color: var(--muted); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
    .tab[aria-selected=true] { color: var(--ink); background: var(--bg); }
    .tab:focus-visible, .tab:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
    .page[data-tab=note] .slot-col, .page[data-tab=slot] .note-col { display: none; }
    .note-col { border-right: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .btn, .list { transition: none; }
  }
</style>
