<!-- SURFACE-5: a new ticket (svelte/1) — `#/new`, and `#/new/:parent` with the
     parent filled. V2-25: no epic menu — the `epics` activation went; the
     shortcode is typed (bare, or `org:epic:CODE` for another epic's), or it
     is the parent's. Parent and blockers come from
     sibling `ticket-picker`s, each drawn as a `ticket-chip`. Create (or
     ctrl+enter, a key the host takes: FRAME-36) awaits `tickets.create`'s
     answer — `created { ticket }` goes to the ticket (its address, for one
     born outside the genesis epic), a refusal is said under the form.
     STUDIO-12: the board link and the title are page-new's. -->
<script module>
  export const uses = ["tickets.get", "tickets.create"];

  /** A call's answer as FRAME-37 says it, `{ ok, events, message }` — also
   *  from a host before it (the data events, or a rejection), and from none. */
  async function answer(call) {
    try {
      const r = await call;
      if (r === undefined || r === null) return { ok: false, events: [], message: "the call answered nothing" };
      const events = Array.isArray(r) ? r : (r.events ?? []);
      const error = events.find((e) => e.type === "error");
      const ok = (Array.isArray(r) ? true : r.ok !== false) && !error;
      return { ok, events, message: error?.message ?? (Array.isArray(r) ? null : r.message ?? null) };
    } catch (e) {
      return { ok: false, events: [], message: e?.message ?? String(e) };
    }
  }
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  // COMPS-5 (the owner, 2026-11-05: "all tickets are the same surface. doc or not. they all need the exact same
  // rendering capabilites"): a body is drawn by ticket-view, here as everywhere.
  import TicketView from "./ticket-view.svelte";
  import { ticketHref } from "./ticket-links.svelte";
  import Picker from "./ticket-picker.svelte";
  import Chip from "./ticket-chip-sv.svelte";

  let { parent = null, sheet = null } = $props();
  const b = bus();

  let code = $state("");
  let kind = $state("build");
  let severity = $state("");
  let title = $state("");
  let body = $state("");
  let picked = $state(null);
  let unparent = $state(false);
  let blocked = $state([]);
  let preview = $state(false);
  let sending = $state(false);
  let refused = $state(null);

  const par = live({ snapshot: "tickets.get", params: () => ({ id: parent }), key: "id", when: () => !!parent });

  const parentNow = $derived(picked ?? (unparent ? null : parent));
  const prefill = $derived(!picked && !unparent && parent && par.rows[0]?.id ? par.rows[0].epic : "");
  const shortcode = $derived(code.trim() || prefill || (parentNow ? parentNow.slice(0, parentNow.lastIndexOf("-")) : ""));
  const ready = $derived(title.trim() !== "" && shortcode !== "");

  function got({ name, id }) {
    if (name === "parent") {
      picked = id;
      unparent = false;
    } else if (name === "blocked_by" && !blocked.includes(id)) blocked = [...blocked, id];
  }
  function noParent() {
    picked = null;
    unparent = true;
  }

  async function create() {
    if (!ready || sending) return;
    const params = { epic: shortcode, title: title.trim(), kind, blocked_by: blocked };
    if (parentNow) params.parent = parentNow;
    if (severity) params.severity = severity;
    if (body) params.body = body;
    sending = true;
    refused = null;
    const r = await answer(b.call("tickets.create", params));
    sending = false;
    const made = r.events.find((x) => x.type === "created")?.ticket;
    if (r.ok && made) b.navigate(made.uid ? ticketHref(made) : ticketHref({ id: made.address?.split(":").length > 2 ? made.address : made.id }));
    else refused = r.message ?? "the ticket was not created";
  }

  b.keys({ "ctrl+enter": { to: () => create(), label: "create the ticket", in_inputs: true } });
</script>

<section class="create" data-surface="ticket-create" data-form="tickets.create">
  <div class="grid">
    <label class="f"><span class="l">shortcode</span>
      <input data-field="shortcode" type="text" spellcheck="false" autocomplete="off" placeholder="KILN, or org:epic:KILN" bind:value={() => code || prefill, (v) => (code = v)}/>
      {#if sheet === "loading"}<span class="skeleton" data-loading aria-hidden="true"></span>{/if}
    </label>
    <label class="f"><span class="l">kind</span>
      <select data-field="kind" bind:value={kind}>
        {#each ["build", "gap", "spike", "execution", "scope", "doc"] as k (k)}<option value={k}>{k}</option>{/each}
      </select>
    </label>
    <label class="f"><span class="l">severity</span>
      <select data-field="severity" bind:value={severity}>
        <option value="">none</option>
        {#each ["Critical", "High", "Medium", "Low"] as s (s)}<option value={s}>{s}</option>{/each}
      </select>
    </label>
  </div>
  <label class="f wide"><span class="l">title</span>
    <input data-field="title" type="text" autocomplete="off" placeholder="what it is, in a line" bind:value={title}/>
  </label>
  <div class="f wide"><span class="l">parent</span>
    <div class="chips" data-field-value="parent">
      {#if parentNow}
        <Chip id={parentNow} kind="parent"/><button type="button" class="x" data-unparent aria-label="no parent" onclick={noParent}>×</button>
      {/if}
    </div>
    <Picker name="parent" label="parent" onpicked={got}/>
  </div>
  <div class="f wide"><span class="l">blocked by</span>
    <div class="chips" data-field-value="blocked_by">
      {#each blocked as id (id)}
        <span class="chipped"><Chip {id} kind="blocker"/><button type="button" class="x" data-unblock={id} aria-label={"not blocked by " + id} onclick={() => (blocked = blocked.filter((x) => x !== id))}>×</button></span>
      {/each}
    </div>
    <Picker name="blocked_by" label="blocked by" onpicked={got}/>
  </div>
  <div class="f wide"><span class="l">body
      <button type="button" class="tab" data-preview-toggle aria-pressed={preview ? "true" : "false"} onclick={() => (preview = !preview)}>{preview ? "write" : "preview"}</button></span>
    {#if preview}
      <div class="preview" data-preview><TicketView {body}/></div>
      {#if body === ""}<p class="empty" data-empty>Nothing written yet.</p>{/if}
    {:else}
      <textarea data-field="body" rows="10" placeholder="markdown" bind:value={body}></textarea>
    {/if}
  </div>
  <div class="actions">
    <button type="button" class="go" data-action="tickets.create" disabled={!ready || sending} title={ready ? "create (ctrl+enter)" : "a title, and a shortcode or a parent"} onclick={create}>Create</button>
    {#if sending}<span class="hint" data-loading>creating…</span>{/if}
  </div>
  {#if refused}
    <p class="refused" data-refused role="alert">{refused}</p>
  {/if}
</section>

<style>
  .create { display: block; max-width: 760px; margin: 0 auto; padding: 16px 20px 40px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .grid { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 10px; }
  @media (max-width: 720px) { .grid { grid-template-columns: repeat(2, minmax(0, 1fr)); } }
  .f { display: flex; flex-direction: column; gap: 4px; min-width: 0; margin-bottom: 12px; }
  .l { display: flex; gap: 8px; align-items: center; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  input, select, textarea { width: 100%; padding: 5px 10px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  textarea { resize: vertical; font-family: var(--font-text); line-height: 1.5; }
  input:hover, select:hover, textarea:hover, input:global([data-sheet~=hover]), select:global([data-sheet~=hover]), textarea:global([data-sheet~=hover]) { border-color: var(--muted); }
  input:focus-visible, select:focus-visible, textarea:focus-visible, input:global([data-sheet~=focus]), select:global([data-sheet~=focus]), textarea:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .chips { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; min-height: 4px; }
  .chipped { display: inline-flex; gap: 2px; align-items: center; }
  .x { padding: 0 6px; font: inherit; line-height: 18px; color: var(--muted); background: none; border: 0; border-radius: var(--radius-pill); cursor: pointer; }
  .x:hover, .x:global([data-sheet~=hover]) { color: var(--warn); background: var(--hover); }
  .x:focus-visible, .x:global([data-sheet~=focus]) { outline: 2px solid var(--focus); }
  .tab { padding: 0 8px; font: inherit; font-size: 11px; text-transform: none; letter-spacing: 0; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  .tab:hover, .tab:global([data-sheet~=hover]) { color: var(--ink); }
  .tab[aria-pressed=true], .tab:global([data-sheet~=checked]) { color: var(--accent); border-color: var(--accent); background: var(--sel); }
  .tab:focus-visible, .tab:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .preview { min-height: 120px; padding: 8px 10px; font-family: var(--sans); line-height: 1.55; overflow-wrap: anywhere; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .preview :global(a) { color: var(--accent); }
  .preview :global(pre) { overflow: auto; padding: 8px 10px; font-family: var(--font-text); background: var(--bg); border-radius: var(--radius); }
  .empty, .hint { margin: 0; font-size: 11px; color: var(--muted); }
  .skeleton { display: block; height: 12px; background: var(--skeleton); border-radius: var(--radius-sm); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .actions { display: flex; gap: 10px; align-items: center; margin-top: 6px; }
  .go { padding: 5px 16px; font: inherit; font-weight: var(--weight-medium); color: var(--accent); background: var(--sel); border: var(--border) solid var(--accent); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .go:hover, .go:global([data-sheet~=hover]) { background: var(--row-sel); }
  .go:focus-visible, .go:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .go[disabled], .go:global([data-sheet~=disabled]) { color: var(--disabled); background: var(--card); border-color: var(--line); cursor: default; }
  .refused { margin: 10px 0 0; padding: 6px 10px; color: var(--warn); font-size: 12px; border-left: 2px solid var(--warn); background: var(--panel); }
  @keyframes breathe { from { opacity: 1; } to { opacity: .45; } }
  @media (prefers-reduced-motion: reduce) {
    input, select, textarea, .go { transition: none; }
    .skeleton { animation: none; }
  }
</style>
