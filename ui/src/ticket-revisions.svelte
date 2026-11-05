<!-- SURFACE-3: a ticket's revisions (TICKETS-7) — `revisions list`, newest
     first; a row opens the diff wide (the page gives `[data-open=true]` the
     width): that revision against the one before it, any two by their a / b
     pickers, `revisions diff --text` side by side or as text, or the b
     revision as it stood (`revisions get`). Live: `watch { revisions: true }`
     says `revised`, folded whole into a collection keyed by ticket
     (`revisions list { limit: 1 }`: this ticket's newest rev); each newer rev
     moves the list's `before` past it, so the list is asked again and holds
     every revision, however many came at once. EDITOR-16: a write V2-37
     coalesces says `revised` with the newest rev's own number (the fields of
     both), so `before` does not move and nothing is asked again — the newest
     row takes the fields `revised` said, and its moment is "just now".
     Restoring is SURFACE-4's: "pick" says `ticket.revision_picked
     { id, rev }` on the bus for `ticket-actions`. `escape` closes the diff. -->
<script module>
  export const uses = ["tickets.revisions.list", "tickets.watch", "tickets.revisions.diff", "tickets.revisions.get"];
</script>

<script>
  import { bus, live, markdown } from "plexus-ui/host";

  let { id, sheet = null } = $props();
  const b = bus();
  let open = $state(false);
  let a = $state(0);
  let z = $state(0);
  let view = $state("side");
  let picked = $state(0);

  const latest = live({ snapshot: "tickets.revisions.list", params: () => ({ id, limit: 1 }), changes: "tickets.watch", changes_params: { revisions: true },
                        key: "ticket", on: { revised: "upsert:." } });
  const newest = $derived(latest.rows.filter((r) => r.ticket === id).reduce((m, r) => Math.max(m, r.rev ?? 0), 0));
  const revs = live({ snapshot: "tickets.revisions.list", params: () => (newest ? { id, limit: 50, before: newest + 1 } : { id, limit: 50 }), key: "rev" });
  const diff = live({ snapshot: "tickets.revisions.diff", params: () => ({ id, from: a, to: z, text: true }), key: "to", when: () => open && a !== z });
  const opened = live({ snapshot: "tickets.revisions.get", params: () => ({ id, rev: z }), key: "ticket", when: () => open && view === "was", on: { revision_at: "upsert:." } });

  const told = $derived(latest.rows.find((r) => r.ticket === id));
  const rows = $derived(revs.rows.map((r) => (told && r.rev === told.rev && String(r.fields ?? []) !== String(told.fields ?? []) ? { ...r, fields: told.fields, at_unix: null } : r)));
  const d = $derived(diff.rows.find((x) => x.from === a && x.to === z));
  const was = $derived(opened.rows.find((o) => o.ticket && o.revision?.rev === z));
  const changed = $derived(d ? [...d.fields, ...d.frontmatter] : []);
  const two = (n) => String(n).padStart(2, "0");
  const when = (s) => { if (s == null) return "just now"; const t = new Date(s * 1000); return `${two(t.getUTCMonth() + 1)}-${two(t.getUTCDate())} ${two(t.getUTCHours())}:${two(t.getUTCMinutes())}`; };
  const lines = (keep) => (d?.body?.hunks ?? []).flatMap((h) => h.lines.filter((l) => l.op !== keep));

  function show(rev) {
    if (open) { z = rev; return; }
    a = rev > 1 ? rev - 1 : rev;
    z = rev;
    open = true;
  }
  function close() { open = false; view = "side"; }
  function pick() {
    picked = a;
    b.emit("ticket.revision_picked", { id, rev: a });
  }
  let offKeys = null;
  $effect(() => {
    if (open && !offKeys) offKeys = b.keys({ escape: { to: close, label: "close the diff", only: true } });
    if (!open && offKeys) { offKeys(); offKeys = null; }
  });
</script>

<section class="ticket-revisions" data-surface="ticket-revisions" data-open={open ? "true" : "false"}>
  <header class="head">
    <h2 class="sec">Revisions <span class="n">{rows.length}</span></h2>
    {#if open}
      <span class="range" data-field="range"><b>rev {a}</b> → <b>rev {z}</b></span>
      <span class="tools">
        <span class="seg" role="radiogroup" aria-label="diff view">
          <button type="button" role="radio" data-view="side" aria-checked={view === "side"} onclick={() => (view = "side")}>side by side</button>
          <button type="button" role="radio" data-view="text" aria-checked={view === "text"} onclick={() => (view = "text")}>text</button>
          <button type="button" role="radio" data-view="was" data-action="tickets.revisions.get" aria-checked={view === "was"} onclick={() => (view = "was")}>rev {z} as it stood</button>
        </span>
        <button type="button" class="btn" data-pick={a} aria-pressed={picked === a} title="says ticket.revision_picked to the actions" onclick={pick}>{picked === a ? `picked rev ${a}` : `pick rev ${a} to restore`}</button>
        <button type="button" class="btn ghost" data-close onclick={close}>close <kbd>esc</kbd></button>
      </span>
    {/if}
  </header>
  {#if sheet === "loading" || revs.status === "pending"}<div data-loading aria-busy="true"><i class="bone"></i><i class="bone"></i></div>{/if}
  {#if revs.status === "error" || revs.status === "refused"}<p class="refused" data-refused>{revs.message}</p>{/if}
  <div class="grid">
    <ul class="revs" data-field="revisions">
      {#each rows as r (r.rev)}
        <li class="rev-row" tabindex="0" role="button" data-id={r.rev} data-rev={r.rev} data-action="tickets.revisions.diff"
            data-selected={open && (r.rev === a || r.rev === z) ? "true" : "false"}
            onclick={() => show(r.rev)} onkeydown={(e) => (e.key === "Enter" || e.key === " ") && (e.preventDefault(), show(r.rev))}>
          <span class="rev-n">rev {r.rev}</span>
          <span class="rev-at">{when(r.at_unix)}</span>
          <span class="rev-by">{r.name ?? (r.actor ? r.actor.slice(0, 10) + "…" : "")}</span>
          <span class="rev-f" data-field="fields">{#each r.fields?.length ? r.fields : [r.source === "baseline" ? "baseline" : "—"] as f (f)}<span>{f}</span>{/each}</span>
          {#if open}
            <span class="ab">
              <button type="button" data-ab="a" data-rev={r.rev} aria-pressed={r.rev === a} title="compare from" onclick={(e) => { e.stopPropagation(); a = r.rev; }}>a</button>
              <button type="button" data-ab="b" data-rev={r.rev} aria-pressed={r.rev === z} title="compare to" onclick={(e) => { e.stopPropagation(); z = r.rev; }}>b</button>
            </span>
          {:else}
            <span class="rev-go">diff ›</span>
          {/if}
        </li>
      {/each}
      {#if revs.status === "live" && rows.length === 0}<li class="empty" data-empty>no revisions kept</li>{/if}
    </ul>
    {#if open}
      <div class="diff" data-diff={`${a}..${z}`}>
        {#if view === "was"}
          {#if opened.status === "pending" || opened.status === "resyncing"}<div data-loading aria-busy="true"><i class="bone"></i></div>{/if}
          {#if opened.status === "error" || opened.status === "refused"}<p class="refused" data-refused>{opened.message}</p>{/if}
          {#if was}
            <div class="was" data-revision-open={was.revision.rev}>
              <p class="was-head"><span class="st" data-status={was.ticket.status}>{was.ticket.status}</span> <strong data-field="title">{was.ticket.title}</strong></p>
              <div class="md" data-field="body">{@html markdown(was.ticket.body ?? "")}</div>
            </div>
          {/if}
        {:else if a === z}
          <p class="empty" data-empty>rev {a} against itself: nothing differs</p>
        {:else}
          {#if diff.status === "pending" || diff.status === "resyncing"}<div data-loading aria-busy="true"><i class="bone"></i><i class="bone"></i></div>{/if}
          {#if diff.status === "error" || diff.status === "refused"}<p class="refused" data-refused>{diff.message}</p>{/if}
          {#if d}
            {#each changed as f (f.field)}
              <div class="field" data-field={f.field}><span class="k">{f.field}</span><span class="fl del">{f.from ?? "—"}</span><span class="fl add">{f.to ?? "—"}</span></div>
            {/each}
            {#if d.body && view === "side"}
              <div class="cols" data-side-by-side data-field="body">
                <div class="colhead">rev {d.from}</div>
                <div class="colhead">rev {d.to}</div>
                <div class="col" data-body="old">{#each lines("add") as l, i (i)}<div class="dl {l.op}" data-op={l.op}><span class="mk">{l.op === "del" ? "−" : ""}</span><span class="t">{l.text}</span></div>{/each}</div>
                <div class="col" data-body="new">{#each lines("del") as l, i (i)}<div class="dl {l.op}" data-op={l.op}><span class="mk">{l.op === "add" ? "+" : ""}</span><span class="t">{l.text}</span></div>{/each}</div>
              </div>
            {/if}
            {#if d.body && view === "text"}
              <div class="text" data-text data-field="body">{#each lines(null) as l, i (i)}<div class="tl {l.op}" data-op={l.op}>{(l.op === "add" ? "+ " : l.op === "del" ? "- " : "  ") + l.text}</div>{/each}</div>
            {/if}
            <p class="sum"><span class="dd">−{d.body?.removed ?? 0} lines</span><span class="aa">+{d.body?.added ?? 0} lines</span><span>{[...changed.map((f) => f.field), ...(d.body ? ["body"] : [])].join(", ")}</span></p>
            {#if !changed.length && !d.body}<p class="empty" data-empty>no difference</p>{/if}
          {/if}
        {/if}
      </div>
    {/if}
  </div>
</section>

<style>
  .ticket-revisions { color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .ticket-revisions[data-open=true] { margin: 4px 0 8px; padding: 0 0 12px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .ticket-revisions .head { display: flex; flex-wrap: wrap; gap: 10px; align-items: center; }
  .ticket-revisions[data-open=true] .head { padding: 8px 12px; border-bottom: var(--border) solid var(--line); }
  .ticket-revisions .sec { display: flex; gap: 7px; align-items: baseline; margin: 22px 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .ticket-revisions[data-open=true] .sec { margin: 0; }
  .ticket-revisions .n { color: var(--ink); font-variant-numeric: tabular-nums; text-transform: none; letter-spacing: 0; }
  .ticket-revisions .range { font-size: 12px; }
  .ticket-revisions .range b { color: var(--accent); font-weight: var(--weight-medium); }
  .ticket-revisions .tools { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin-left: auto; }
  .ticket-revisions .seg { display: inline-flex; overflow: hidden; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .ticket-revisions .seg button { padding: 2px 9px; font: inherit; font-size: 12px; line-height: 20px; color: var(--muted); background: none; border: 0; border-right: var(--border) solid var(--line); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .ticket-revisions .seg button:last-child { border-right: 0; }
  .ticket-revisions .seg button:hover, .ticket-revisions .seg button:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .ticket-revisions .seg button:focus-visible, .ticket-revisions .seg button:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: -2px; }
  .ticket-revisions .seg button[aria-checked=true], .ticket-revisions .seg button:global([data-sheet~=checked]) { color: var(--accent); background: var(--sel); }
  .ticket-revisions .btn { display: inline-flex; gap: 5px; align-items: center; padding: 1px 10px; font: inherit; font-size: 12px; line-height: 20px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .ticket-revisions .btn:hover, .ticket-revisions .btn:global([data-sheet~=hover]) { background: var(--hover); border-color: var(--muted); }
  .ticket-revisions .btn:focus-visible, .ticket-revisions .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .ticket-revisions .btn[aria-pressed=true], .ticket-revisions .btn:global([data-sheet~=checked]) { color: var(--accent); background: var(--sel); border-color: var(--accent); }
  .ticket-revisions .btn.ghost { color: var(--muted); background: none; border-color: transparent; }
  .ticket-revisions .btn.ghost:hover, .ticket-revisions .btn.ghost:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .ticket-revisions kbd { display: inline-block; min-width: 16px; padding: 0 4px; font-family: var(--font-text); font-size: 10px; line-height: 15px; text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  .ticket-revisions[data-open=true] .grid { display: grid; grid-template-columns: 330px minmax(0, 1fr); }
  .ticket-revisions .revs { list-style: none; margin: 0; padding: 0; border-top: var(--border) solid var(--line); }
  .ticket-revisions[data-open=true] .revs { border-top: 0; border-right: var(--border) solid var(--line); }
  .ticket-revisions .rev-row { position: relative; display: grid; grid-template-columns: 44px 82px 58px minmax(0, 1fr) auto; gap: 6px; align-items: center; height: var(--pitch); padding: 0 6px; font-size: 12px; border-bottom: var(--border) solid var(--line); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .ticket-revisions .rev-row:hover, .ticket-revisions .rev-row:global([data-sheet~=hover]) { background: var(--hover); }
  .ticket-revisions .rev-row:focus-visible, .ticket-revisions .rev-row:global([data-sheet~=focus]) { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .ticket-revisions .rev-row[data-selected=true], .ticket-revisions .rev-row:global([data-sheet~=selected]) { background: var(--row-sel); }
  .ticket-revisions .rev-row[data-selected=true]::before, .ticket-revisions .rev-row:global([data-sheet~=selected])::before { content: ""; position: absolute; left: 0; top: 3px; bottom: 3px; width: 3px; border-radius: 0 var(--radius-sm) var(--radius-sm) 0; background: var(--accent); }
  .ticket-revisions .rev-n { color: var(--accent); font-weight: var(--weight-medium); }
  .ticket-revisions .rev-at { font-size: 11px; color: var(--muted); font-variant-numeric: tabular-nums; }
  .ticket-revisions .rev-by { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-weight: var(--weight-medium); }
  .ticket-revisions .rev-f { display: flex; gap: 4px; overflow: hidden; }
  .ticket-revisions .rev-f span { padding: 0 5px; font-size: 10px; line-height: 15px; color: var(--chip-fg); background: var(--chip-bg); border-radius: var(--radius-sm); }
  .ticket-revisions .rev-go { opacity: 0; font-size: 11px; color: var(--accent); }
  .ticket-revisions .rev-row:hover .rev-go, .ticket-revisions .rev-row:focus-visible .rev-go, .ticket-revisions .rev-row:global([data-sheet~=hover]) .rev-go, .ticket-revisions .rev-row:global([data-sheet~=focus]) .rev-go { opacity: 1; }
  .ticket-revisions .ab { display: flex; gap: 3px; }
  .ticket-revisions .ab button { width: 19px; height: 17px; padding: 0; font: inherit; font-size: 10px; line-height: 15px; color: var(--muted); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-sm); cursor: pointer; }
  .ticket-revisions .ab button:hover, .ticket-revisions .ab button:global([data-sheet~=hover]) { color: var(--ink); border-color: var(--muted); }
  .ticket-revisions .ab button[aria-pressed=true], .ticket-revisions .ab button:global([data-sheet~=checked]) { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .ticket-revisions .diff { min-width: 0; padding: 10px 12px 0; }
  .ticket-revisions .field { display: grid; grid-template-columns: 52px minmax(0, 1fr) minmax(0, 1fr); gap: 0 10px; align-items: baseline; margin-bottom: 8px; font-size: 12px; }
  .ticket-revisions .k { font-size: 11px; color: var(--muted); }
  .ticket-revisions .fl { padding: 0 6px; }
  .ticket-revisions .cols { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); overflow: hidden; background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .ticket-revisions .colhead { padding: 4px 10px; font-size: 11px; color: var(--muted); background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .ticket-revisions .colhead + .colhead, .ticket-revisions .col + .col { border-left: var(--border) solid var(--line); }
  .ticket-revisions .dl { display: grid; grid-template-columns: 14px minmax(0, 1fr); min-height: 20px; padding: 0 6px; font-size: 12px; line-height: 20px; }
  .ticket-revisions .mk { color: var(--muted); }
  .ticket-revisions .t { white-space: pre-wrap; overflow-wrap: anywhere; }
  .ticket-revisions .del { background: var(--diff-del-bg); color: var(--diff-del-fg); }
  .ticket-revisions .add { background: var(--diff-add-bg); color: var(--diff-add-fg); }
  .ticket-revisions .same { color: var(--muted); }
  .ticket-revisions .text { padding: 8px 10px; font-size: 12px; line-height: 20px; white-space: pre-wrap; overflow-wrap: anywhere; background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .ticket-revisions .sum { display: flex; gap: 14px; margin: 8px 0 0; font-size: 11px; color: var(--muted); }
  .ticket-revisions .sum .aa { color: var(--diff-add-fg); }
  .ticket-revisions .sum .dd { color: var(--diff-del-fg); }
  .ticket-revisions .was-head { margin: 0 0 6px; }
  .ticket-revisions .md { font-family: var(--sans); line-height: 1.55; overflow-wrap: anywhere; }
  .ticket-revisions .md :global(a) { color: var(--accent); }
  .ticket-revisions .md :global(code) { font-family: var(--font-text); }
  .ticket-revisions .st { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .ticket-revisions .st[data-status=Pending] { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .ticket-revisions .st[data-status=Ready] { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .ticket-revisions .st[data-status=Coding] { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .ticket-revisions .st[data-status="In Review"] { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .ticket-revisions .st[data-status=Complete] { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .ticket-revisions .st[data-status=Canceled] { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .ticket-revisions .st[data-status="Draft"] { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .ticket-revisions .st[data-status="Blocked"] { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .ticket-revisions .st[data-status="Superseded"] { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .ticket-revisions .st[data-status="Epic"] { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .ticket-revisions .st[data-status="Doc"] { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  .ticket-revisions .empty { margin: 0; padding: 8px 6px; font-size: 12px; color: var(--muted); }
  .ticket-revisions .refused { margin: 4px 0; font-size: 11px; color: var(--warn); }
  .ticket-revisions .bone { display: block; height: 10px; margin: 6px 0; background: var(--skeleton); border-radius: var(--radius-sm); }
  @media (max-width: 960px) {
    .ticket-revisions[data-open=true] .grid { grid-template-columns: minmax(0, 1fr); }
    .ticket-revisions[data-open=true] .revs { border-right: 0; }
  }
  /* EDITOR-12: on a phone the row's columns narrow and a field chip shortens to an ellipsis, never past its edge. */
  @media (max-width: 640px) {
    .ticket-revisions .rev-row { grid-template-columns: 40px 66px 50px minmax(0, 1fr) auto; gap: 4px; }
    .ticket-revisions .rev-f { min-width: 0; }
    .ticket-revisions .rev-f span { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  }
  @media (prefers-reduced-motion: reduce) {
    .ticket-revisions .rev-row, .ticket-revisions .seg button, .ticket-revisions .btn { transition: none; }
  }
</style>
