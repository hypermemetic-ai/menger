<!-- SURFACE-4: a ticket's edit form (svelte/1), placed in the ticket page by
     <ticket-edit id={id}/> and opened by its button or `ticket.edit` on the
     bus (the page's `e`). Two saves, as the designer drew them: the fields —
     title, severity, blockers and hidden through one `update`, the kind
     through `set_kind`, the parent through `set_parent` (SURFACE-5's
     ticket-picker in a bound mark, heard as `picked`, or an id typed) — and
     the body through `amend`, its markdown previewed beside it by the host's
     render. Each write's answer is said under its field, a refusal in the
     store's words ([data-refused]); the ticket itself moves on the watch.
     ctrl+enter saves, escape cancels (FRAME-36's keys, while the form is
     open). -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.amend", "tickets.update", "tickets.set_kind", "tickets.set_parent"];
</script>

<script>
  import { bus, live, markdown } from "plexus-ui/host";
  import Chip from "./ticket-chip-sv.svelte";

  let { id } = $props();
  const b = bus();
  const KINDS = ["scope", "execution", "build", "gap", "spike", "doc"];
  const SEVERITIES = ["none", "Critical", "High", "Medium", "Low"];

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", labels_changed: "upsert:ticket",
                              severity_changed: "upsert:ticket", hidden_changed: "upsert:ticket", blockers_changed: "upsert:ticket",
                              parent_changed: "upsert:ticket", frozen_changed: "upsert:ticket", deleted: "remove:ids" } });
  const t = $derived($ticket.rows[0]);

  let open = $state(false);
  let draft = $state({ title: "", body: "", kind: "", severity: "none", parent: "", hidden: false, blockers: [], block: "" });
  // Each write's last answer: { said } or { refused }, by its method.
  let answers = $state({});

  const sevOf = (x) => x?.severity ?? "none";
  const parentOf = (x) => x?.parent ?? "";
  const same = (a, c) => a.length === c.length && a.every((v, i) => v === c[i]);
  const dirty = $derived(t ? {
    title: draft.title !== t.title,
    kind: draft.kind !== t.kind,
    severity: draft.severity !== sevOf(t),
    hidden: draft.hidden !== (t.hidden === true),
    parent: draft.parent.trim() !== "" && draft.parent.trim() !== parentOf(t),
    blockers: !same(draft.blockers, t.blocked_by ?? []),
    body: draft.body !== t.body,
  } : {});
  const fields = $derived(["title", "kind", "severity", "hidden", "parent", "blockers"].filter((k) => dirty[k]).length);

  function edit() {
    if (!t) return;
    draft = { title: t.title, body: t.body, kind: t.kind, severity: sevOf(t), parent: parentOf(t), hidden: t.hidden === true, blockers: [...(t.blocked_by ?? [])], block: "" };
    answers = {};
    open = true;
  }
  function cancel() {
    open = false;
  }
  b.on("ticket.edit", (data) => {
    if (!open && (data?.id == null || data.id === id)) edit();
  });
  b.on("picked", (data) => {
    if (data?.name === "parent" && data.id) draft.parent = data.id;
  });
  $effect(() => {
    if (!open) return;
    return b.keys({
      "ctrl+enter": { to: save, label: "save the edit", in_inputs: true },
      escape: { to: cancel, label: "cancel the edit", in_inputs: true },
    });
  });

  /** A write, its answer as FRAME-37 hands it — `{ ok, events, message }` —
   *  or, before it lands, made so from the events (an in-band error, a
   *  not_found) or the refusal thrown. */
  async function ask(asked, params) {
    let r;
    try {
      r = await asked;
    } catch (e) {
      return { ok: false, events: [], message: e?.message ?? String(e) };
    }
    if (r && !Array.isArray(r) && typeof r === "object" && "ok" in r) return r;
    const events = Array.isArray(r) ? r : [];
    const bad = events.find((e) => e?.type === "error" || e?.type === "not_found" || e?.type === "refused");
    return { ok: !bad, events, message: bad ? (bad.message ?? bad.reason ?? `no ticket ${params.id ?? ""}`) : undefined };
  }
  async function write(key, asked, params, said) {
    answers[key] = { asking: true };
    const r = await ask(asked, params);
    answers[key] = r.ok ? { said: said(r.events[0] ?? {}) } : { refused: r.message ?? "refused" };
  }

  function saveFields() {
    if (!open || !t) return;
    const p = { id, source: "app" };
    if (dirty.title) p.title = draft.title;
    if (dirty.severity) p.severity = draft.severity;
    if (dirty.hidden) p.hidden = draft.hidden;
    const block = draft.blockers.filter((x) => !(t.blocked_by ?? []).includes(x));
    const unblock = (t.blocked_by ?? []).filter((x) => !draft.blockers.includes(x));
    if (block.length) p.block = block;
    if (unblock.length) p.unblock = unblock;
    if (Object.keys(p).length > 2)
      write("tickets.update", b.call("tickets.update", p), p, (e) => (e.changed?.length ? "saved " + e.changed.join(", ") : "nothing changed"));
    if (dirty.kind)
      write("tickets.set_kind", b.call("tickets.set_kind", { id, kind: draft.kind, source: "app" }), {}, (e) => `${e.previous} → ${e.ticket?.kind}`);
    if (dirty.parent)
      write("tickets.set_parent", b.call("tickets.set_parent", { id, parent: draft.parent.trim() }), { id }, (e) => `under ${e.ticket?.parent}`);
  }
  function saveBody() {
    if (!open || !t || !dirty.body) return;
    write("tickets.amend", b.call("tickets.amend", { id, body: draft.body }), { id }, (e) => (e.changed?.length ? "saved the body" : "nothing changed"));
  }
  function save() {
    saveFields();
    saveBody();
  }
  function blockKeyed(ev) {
    if (ev.key !== "Enter") return;
    ev.preventDefault();
    const x = draft.block.trim();
    if (x && !draft.blockers.includes(x)) draft.blockers.push(x);
    draft.block = "";
  }
  function unblock(x) {
    draft.blockers = draft.blockers.filter((v) => v !== x);
  }
</script>

{#snippet answer(method)}
  {#if answers[method]?.refused}<p class="refused" data-refused data-for={method}>{answers[method].refused}</p>{/if}
  {#if answers[method]?.said}<p class="landed" data-answer data-for={method}>{answers[method].said}</p>{/if}
  {#if answers[method]?.asking}<p class="hint" data-loading data-for={method}>saving…</p>{/if}
{/snippet}

<div class="ticket-edit" data-surface="ticket-edit" data-ticket={id}>
  {#if $ticket.status === "pending"}<div class="skel" data-loading><span class="bar"></span></div>{/if}
  {#if $ticket.status === "live" && !t}<p class="empty" data-empty>No ticket {id} to edit.</p>{/if}
  {#if t}
    {#if !open}
      <div class="closed">
        <button type="button" class="btn" data-action="edit" onclick={edit}>edit <kbd>e</kbd></button>
      </div>
    {:else}
      <div class="sheet" data-open>
        <div class="te-fields" data-form="tickets.update">
          <div class="te-grid">
            <label class="te-f wide">
              <span class="te-l">title{#if dirty.title}<i class="dirty" title="changed"></i>{/if}</span>
              <input class="in title" data-field="title" autocomplete="off" bind:value={draft.title}/>
            </label>
            <div class="te-f">
              <span class="te-l">kind{#if dirty.kind}<i class="dirty" title="changed"></i>{/if}</span>
              <span class="seg" role="radiogroup" aria-label="kind" data-field="kind" data-action="tickets.set_kind">
                {#each KINDS as k (k)}
                  <button type="button" role="radio" data-kind-pick={k} aria-checked={draft.kind === k} onclick={() => (draft.kind = k)}><i class="kmark" data-kind={k} aria-hidden="true"></i>{k}</button>
                {/each}
              </span>
              {@render answer("tickets.set_kind")}
            </div>
            <div class="te-f">
              <span class="te-l">severity{#if dirty.severity}<i class="dirty" title="changed"></i>{/if}</span>
              <span class="seg" role="radiogroup" aria-label="severity" data-field="severity">
                {#each SEVERITIES as s (s)}
                  <button type="button" role="radio" data-sev-pick={s} aria-checked={draft.severity === s} onclick={() => (draft.severity = s)}>{#if s !== "none"}<span class="sev" data-severity={s.toLowerCase()} aria-hidden="true"><i></i><i></i><i></i></span>{/if}{s.toLowerCase()}</button>
                {/each}
              </span>
            </div>
            <label class="te-f te-check">
              <input type="checkbox" data-field="hidden" bind:checked={draft.hidden}/>
              hidden from the nav{#if dirty.hidden}<i class="dirty" title="changed"></i>{/if}
            </label>
            <div class="te-f">
              <span class="te-l">parent{#if dirty.parent}<i class="dirty" title="changed"></i>{/if}</span>
              <span class="te-chips">
                {#if t.parent}<Chip id={t.parent} kind="parent"/>{/if}
                <input class="in" data-field="parent" data-action="tickets.set_parent" placeholder="an id" spellcheck="false" autocomplete="off"
                       aria-invalid={answers["tickets.set_parent"]?.refused ? "true" : "false"} bind:value={draft.parent}/>
              </span>
              <!-- SURFACE-5's picker, as `import Picker from "plexus-ui:ticket-picker"` draws it — a bound
                   mark the host fills; the import refuses the build until ticket-picker is in src/, so the
                   mark is written as the import resolves it (the merge turns it into the import). -->
              <div class="picker" data-slot="ticket-picker" data-component="ticket-picker" data-name="parent"></div>
              {@render answer("tickets.set_parent")}
            </div>
            <div class="te-f span2">
              <span class="te-l">blocked by{#if dirty.blockers}<i class="dirty" title="changed"></i>{/if}</span>
              <span class="te-chips" data-field="blocked_by">
                {#each draft.blockers as x (x)}
                  <span class="te-chip" data-blocker={x}><Chip id={x} kind="blocker"/><button type="button" class="x" data-unblock={x} aria-label={"unblock " + x} onclick={() => unblock(x)}>×</button></span>
                {/each}
                <input class="in" data-field="block" placeholder="+ an id" spellcheck="false" autocomplete="off" bind:value={draft.block} onkeydown={blockKeyed}/>
              </span>
            </div>
          </div>
          <div class="te-act">
            <button type="button" class="btn primary" data-action="tickets.update" disabled={fields === 0} onclick={saveFields}>{fields === 0 ? "save fields" : `save ${fields} field${fields === 1 ? "" : "s"}`}</button>
            {@render answer("tickets.update")}
          </div>
        </div>

        <div class="te-bodyform" data-form="tickets.amend">
          <div class="te-split">
            <div class="te-pane">
              <div class="te-pane-h">markdown{#if dirty.body}<span class="edited"> · edited</span>{/if}</div>
              <textarea class="te-text" data-field="body" spellcheck="false" bind:value={draft.body}></textarea>
            </div>
            <div class="te-pane">
              <div class="te-pane-h">preview</div>
              <div class="te-prev md" data-field="preview" data-preview>{@html markdown(draft.body)}</div>
            </div>
          </div>
          <div class="te-act">
            <button type="button" class="btn primary" data-action="tickets.amend" disabled={!dirty.body} onclick={saveBody}>save body</button>
            <button type="button" class="btn ghost" data-action="cancel" onclick={cancel}>cancel</button>
            {@render answer("tickets.amend")}
            <span class="hint push"><kbd>ctrl</kbd><kbd>enter</kbd> save <kbd>esc</kbd> cancel</span>
          </div>
        </div>
      </div>
    {/if}
  {/if}
</div>

<style>
  .ticket-edit { display: block; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .closed { display: flex; gap: 8px; align-items: center; margin: 14px 0 0; }
  .sheet { margin: 4px 0 14px; padding: 12px 14px 14px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .te-grid { display: grid; grid-template-columns: minmax(0, auto) minmax(0, auto) minmax(0, 1fr); gap: 12px 20px; align-items: start; }
  .te-f { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .te-f.wide { grid-column: 1 / -1; }
  .te-f.span2 { grid-column: span 2; }
  .te-l { display: flex; gap: 6px; align-items: center; font-size: 11px; color: var(--muted); }
  .dirty { display: inline-block; width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .te-check { flex-direction: row; align-items: center; align-self: end; gap: 6px; font-size: 12px; color: var(--ink); }
  .te-check input { accent-color: var(--accent); }
  .te-chips { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .te-chips .in { width: 150px; }
  .te-chip { display: inline-flex; align-items: center; gap: 0; }
  .picker:empty { display: none; }
  .te-act { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; margin-top: 12px; }
  .te-act .refused, .te-act .landed, .te-act .hint { margin: 0; }
  .push { display: flex; gap: 4px; align-items: center; margin-left: auto; }
  .te-split { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 0; margin-top: 14px; overflow: hidden; background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .te-pane + .te-pane { border-left: var(--border) solid var(--line); }
  .te-pane-h { display: flex; gap: 8px; padding: 4px 10px; font-size: 11px; color: var(--muted); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .edited { color: var(--accent); }
  .te-text { display: block; width: 100%; height: 300px; padding: 10px 12px; font-family: var(--font-text); font-size: 12px; line-height: 1.6; color: var(--ink); background: var(--bg); border: 0; resize: none; }
  .te-text:focus { outline: none; box-shadow: inset 0 0 0 2px var(--sel); }
  .te-prev { height: 300px; overflow: auto; padding: 10px 14px; font-size: 14px; }
  .md { font-family: var(--sans); line-height: 1.55; overflow-wrap: anywhere; }
  .md :global(h1), .md :global(h2), .md :global(h3) { margin: 16px 0 4px; font-family: var(--font-display); font-size: 14px; font-weight: var(--weight-strong); color: var(--ink); }
  .md :global(p) { margin: 8px 0; }
  .md :global(ul) { margin: 6px 0; padding-left: 20px; }
  .md :global(a) { color: var(--accent); text-decoration: underline; }
  .md :global(code) { padding: 0 3px; font-family: var(--font-text); font-size: 12px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .md :global(pre) { overflow: auto; padding: 8px 10px; font-family: var(--font-text); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .md :global(pre code) { padding: 0; border: 0; background: none; }
  .md :global(> :first-child) { margin-top: 0; }
  .seg { display: inline-flex; flex-wrap: wrap; align-self: flex-start; overflow: hidden; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .seg button { display: inline-flex; gap: 5px; align-items: center; padding: 2px 9px; font-size: 12px; line-height: 20px; color: var(--muted); background: none; border: 0; border-right: var(--border) solid var(--line); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .seg button:last-child { border-right: 0; }
  .seg button:hover { color: var(--ink); background: var(--hover); }
  .seg button:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
  .seg button[aria-checked="true"] { color: var(--accent); background: var(--sel); }
  .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind="scope"] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind="gap"] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind="spike"] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind="execution"] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .kmark[data-kind="doc"] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--muted); border-radius: 1px; }
  .sev { display: inline-flex; gap: 2px; align-items: flex-end; width: 14px; height: 12px; flex: none; }
  .sev i { display: block; width: 3px; background: var(--line); border-radius: 1px; }
  .sev i:nth-child(1) { height: 5px; }
  .sev i:nth-child(2) { height: 8px; }
  .sev i:nth-child(3) { height: 11px; }
  .sev[data-severity="low"] i:nth-child(1) { background: var(--sev-low); }
  .sev[data-severity="medium"] i:nth-child(-n+2) { background: var(--sev-medium); }
  .sev[data-severity="high"] i { background: var(--sev-high); }
  .sev[data-severity="critical"] i { background: var(--sev-critical); }
  .btn { display: inline-flex; gap: 5px; align-items: center; padding: 1px 10px; font-size: 12px; line-height: 20px; white-space: nowrap; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .btn:hover { background: var(--hover); border-color: var(--muted); }
  .btn:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .btn.primary:hover { opacity: .88; }
  .btn:disabled, .btn.primary:disabled { color: var(--disabled); background: var(--card); border-color: var(--line); cursor: not-allowed; opacity: 1; }
  .btn.ghost { color: var(--muted); background: none; border-color: transparent; }
  .btn.ghost:hover { color: var(--ink); background: var(--hover); }
  .x { margin-left: 2px; padding: 0 3px; font-size: 12px; line-height: 14px; color: var(--muted); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .x:hover { color: var(--warn); background: var(--hover); }
  .x:focus-visible { outline: 2px solid var(--focus); }
  .in { min-width: 0; padding: 3px 8px; font-size: 12px; line-height: 18px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .in::placeholder { color: var(--muted); }
  .in:hover { border-color: var(--muted); }
  .in:focus { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .in[aria-invalid="true"] { border-color: var(--warn); }
  .in.title { font-family: var(--font-display); font-size: 16px; line-height: 24px; font-weight: var(--weight-strong); }
  .refused { margin: 5px 0 0; font-size: 11px; line-height: 1.4; color: var(--warn); }
  .refused::before { content: ""; display: inline-block; width: 6px; height: 6px; margin-right: 6px; vertical-align: 1px; border-radius: 50%; background: var(--warn); }
  .landed { margin: 5px 0 0; font-size: 11px; color: var(--live); }
  .landed::before { content: "\2713  "; }
  .hint { font-size: 11px; color: var(--muted); }
  kbd { display: inline-block; min-width: 16px; padding: 0 4px; font-family: var(--font-text); font-size: 10px; line-height: 15px; text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  .empty { padding: 8px 6px; font-size: 12px; color: var(--muted); }
  .skel .bar { display: block; width: 72px; height: 22px; margin-top: 14px; background: var(--skeleton); border-radius: var(--radius); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  @keyframes breathe { from { opacity: 1; } to { opacity: .55; } }
  @media (max-width: 720px) {
    .te-grid { grid-template-columns: minmax(0, 1fr); }
    .te-f.span2 { grid-column: auto; }
    .te-split { grid-template-columns: minmax(0, 1fr); }
    .te-pane + .te-pane { border-top: var(--border) solid var(--line); border-left: 0; }
  }
  @media (prefers-reduced-motion: reduce) {
    .btn, .seg button, .in { transition: none; }
  }
</style>
