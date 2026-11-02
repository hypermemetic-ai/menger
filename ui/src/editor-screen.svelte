<!-- EDITOR-4: the first screen (svelte/1) — editor-body on the left, the
     ticket as everyone else sees it on the right: ticket-head, ticket-body
     (SURFACE-16's view, live on the shared `get`) and ticket-questions with
     its queue line (V2-20), each a bound mark. `#/` opens the ticket last
     opened in this browser (`menger.editor.last`, written here and by the
     ticket page's body); `#/edit/:id` a given one, which becomes the last.
     With none, the screen offers *new*: `tickets.create { epic, title }` in
     the shortcode of the last ticket, else the first `nav groups` lists, and
     `#/edit/<id>` opens. At 1024 and wider the halves sit side by side;
     narrower, one pane at a time and a switch below it, padded by the
     safe-area inset. Keys: `escape` (in the editor too) and the header's
     *board* go to `#/board`; `n` is *new*. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.create", "tickets.nav.groups"];
  export const LAST = "menger.editor.last";
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import Editor from "plexus-ui:editor-body";
  import Head from "plexus-ui:ticket-head";
  import Body from "plexus-ui:ticket-body";
  import Questions from "plexus-ui:ticket-questions";

  let { id = null, sheet = null } = $props();
  const b = bus();
  const remembered = () => { try { return localStorage.getItem(LAST) || null; } catch { return null; } };
  const remember = (v) => { try { localStorage.setItem(LAST, v); } catch { /* a private window */ } };
  // The ticket on screen: the route's, else the last opened here.
  let last = $state(remembered());
  const current = $derived(id ?? last);
  $effect(() => { if (id) { remember(id); last = id; } });

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id: current }), changes: "tickets.watch", key: "id", when: () => current != null,
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  // The page's spec word for word: one get and one watch of the ticket, shared with every part.
  const t = $derived(ticket.rows.find((r) => r.id === current && r.title != null));
  // Where *new* creates: the current ticket's shortcode, else the first group (asked only when there is no ticket).
  const groups = live({ snapshot: "tickets.nav.groups", params: {}, key: "shortcode", when: () => current == null, on: {} });
  const shortcode = $derived(t?.epic ?? groups.rows.find((g) => g.shortcode)?.shortcode ?? null);

  let pane = $state("editor");
  let making = $state(false);
  let refused = $state("");
  async function create() {
    if (making || !shortcode) return;
    making = true;
    refused = "";
    const r = await b.call("tickets.create", { epic: shortcode, title: "Untitled" });
    making = false;
    const made = r.events?.find((e) => e.type === "created")?.ticket;
    if (r.ok && made) { remember(made.id); b.navigate("#/edit/" + made.id); }
    else refused = String(r.message ?? "refused").replace(/^Invalid params: /, "");
  }
  b.keys({
    escape: { to: () => b.navigate("#/board"), label: "the board", in_inputs: true },
    n: { to: create, label: "a new ticket" },
  });
</script>

<div class="editor-screen" data-surface="editor-screen" data-ticket={current ?? undefined} data-pane={pane}>
  <div class="bar">
    {#if current}
      <a class="id" href={"#/ticket/" + current} data-field="current">{current}</a>
      {#if t}<span class="kind" data-field="kind"><i class="kmark" data-kind={t.kind} aria-hidden="true"></i>{t.kind}</span>{/if}
    {:else}
      <span class="none" data-empty>no ticket open yet</span>
    {/if}
    <span class="right">
      <span class="keys"><kbd>esc</kbd> board</span>
      <button type="button" class="btn" data-action="new" disabled={making || !shortcode || sheet === "loading"} onclick={create}>{shortcode ? `new in ${shortcode}` : "new"} <kbd>n</kbd></button>
    </span>
    {#if refused}<p class="refused" data-refused role="alert">{refused}</p>{/if}
  </div>
  {#if current}
    <div class="halves">
      <section class="half editor" data-half="editor"><Editor id={current} {sheet}/></section>
      <section class="half rendered" data-half="rendered">
        <Head id={current}/>
        <Body id={current}/>
        <Questions id={current}/>
      </section>
    </div>
    <div class="switch" role="tablist" aria-label="pane">
      <button type="button" role="tab" data-action="pane" data-to="editor" aria-selected={pane === "editor"} onclick={() => (pane = "editor")}>editor</button>
      <button type="button" role="tab" data-action="pane" data-to="rendered" aria-selected={pane === "rendered"} onclick={() => (pane = "rendered")}>rendered</button>
    </div>
  {:else}
    <div class="welcome" data-welcome>
      <p class="lead">Write a ticket here. It lands as you type.</p>
      <p class="muted">Open one from the <a href="#/board">board</a>, or make a new one{#if shortcode} in {shortcode}{/if}.</p>
      <button type="button" class="btn primary" data-action="new" disabled={making || !shortcode} onclick={create}>new ticket <kbd>n</kbd></button>
    </div>
  {/if}
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
  .kmark[data-kind="doc"] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  .right { margin-left: auto; display: flex; gap: 10px; align-items: center; }
  .keys { display: inline-flex; gap: 5px; align-items: center; }
  .btn { display: inline-flex; gap: 5px; align-items: center; padding: 1px 10px; font-size: 12px; line-height: 20px; white-space: nowrap; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .btn:hover, .btn:global([data-sheet~=hover]) { background: var(--hover); border-color: var(--muted); }
  .btn:focus-visible, .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn:disabled, .btn:global([data-sheet~=disabled]) { color: var(--disabled); background: var(--card); border-color: var(--line); cursor: not-allowed; }
  .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .refused { flex-basis: 100%; margin: 0; color: var(--warn); }
  .halves { flex: 1; min-height: 0; display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); }
  .half { min-width: 0; min-height: 0; overflow: auto; padding: 14px 20px 40px; }
  .half.rendered { background: var(--panel); border-left: var(--border) solid var(--line); }
  .switch { display: none; }
  .welcome { padding: 40px 20px; text-align: center; }
  .lead { margin: 0 0 6px; font: var(--weight-strong) 18px var(--font-display); }
  .muted { margin: 0 0 14px; color: var(--muted); }
  .muted a { color: var(--accent); }
  .none { color: var(--muted); }
  kbd { display: inline-block; min-width: 16px; padding: 0 4px; font-family: var(--font-text); font-size: 10px; line-height: 15px; text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  @media (max-width: 1023px) {
    .halves { grid-template-columns: minmax(0, 1fr); }
    .half.rendered { border-left: 0; }
    .editor-screen[data-pane="editor"] .half.rendered, .editor-screen[data-pane="rendered"] .half.editor { display: none; }
    .switch { display: flex; gap: 6px; flex: none; padding: 6px 14px calc(6px + env(safe-area-inset-bottom)); background: var(--panel); border-top: var(--border) solid var(--line); }
    .switch button { flex: 1; padding: 6px 0; font: inherit; font-size: 12px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
    .switch button[aria-selected="true"], .switch button:global([data-sheet~=selected]) { color: var(--accent); background: var(--sel); border-color: var(--accent); }
    .keys { display: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .btn { transition: none; }
  }
</style>
