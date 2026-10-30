<!-- SURFACE-5: a ticket picker (svelte/1). Typing asks `find` (nothing until
     there is a query; 150 ms of quiet in the host's registry, the newest query
     only); ↑↓ choose a hit, Enter or a click picks it, Escape closes. A pick
     goes to `onpicked({ name, id })` when the importer gives one — a sibling
     shares its importer's member of the bus, and a component never hears its
     own emit — and always on the bus as `picked { name, id }`, `name` the
     picker's prop, for any other component that places it.
     While the debounce runs, or the new `find` is resyncing, the host keeps
     the last query's rows: a hit is drawn, chosen or taken only when its id
     or title holds the query as typed now — Enter takes the hit ↑↓ chose,
     else the one whose id is the query, else the first that holds it, else
     nothing. The keys are the field's own: they act only while it has the
     focus. -->
<script module>
  const FIELDS = ["id", "title", "kind", "status"];
  export const uses = ["tickets.find"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { name, label = null, onpicked = null } = $props();
  const b = bus();

  let q = $state("");
  let open = $state(false);
  let cur = $state(null);
  const lq = $derived(q.trim().toLowerCase());
  const found = live({ snapshot: "tickets.find", params: () => ({ query: q, limit: 8, fields: FIELDS }), key: "id",
                       when: () => q.length > 0, debounce_ms: 150 });
  const holds = (f) => f.id.toLowerCase().includes(lq) || (f.title ?? "").toLowerCase().includes(lq);
  const hits = $derived(found.rows.filter(holds));

  /** `text` split around the first place it holds the query: [before, match, after], or null. */
  function parts(text) {
    const at = lq ? String(text ?? "").toLowerCase().indexOf(lq) : -1;
    if (at < 0) return null;
    return [text.slice(0, at), text.slice(at, at + lq.length), text.slice(at + lq.length)];
  }
  function pick(id) {
    const data = { name, id };
    onpicked?.(data);
    b.emit("picked", data);
    q = "";
    open = false;
    cur = null;
  }
  function typed(v) {
    q = v;
    open = v !== "";
    cur = null;
  }
  function keyed(ev) {
    const ids = hits.map((f) => f.id);
    const at = ids.indexOf(cur);
    if (ev.key === "ArrowDown" && ids.length) {
      ev.preventDefault();
      cur = ids[(at + 1) % ids.length];
    } else if (ev.key === "ArrowUp" && ids.length) {
      ev.preventDefault();
      cur = ids[at <= 0 ? ids.length - 1 : at - 1];
    } else if (ev.key === "Enter") {
      ev.preventDefault();
      const id = at >= 0 ? ids[at] : ids.find((i) => i.toLowerCase() === lq) ?? ids[0];
      if (open && id) pick(id);
    } else if (ev.key === "Escape") {
      q = "";
      open = false;
      cur = null;
    }
  }
</script>

<div class="picker" data-surface="ticket-picker" data-picker={name}>
  <input class="picker-input" data-field={name} type="text" spellcheck="false" autocomplete="off" role="combobox"
         aria-label={label ?? name} aria-expanded={open ? "true" : "false"}
         placeholder="an id or words in a title" bind:value={() => q, typed} onkeydown={keyed}/>
  {#if open}
    <ul class="picker-hits" role="listbox" data-picker-hits>
      {#each hits as f (f.id)}
        <li class="hit" role="option" aria-selected={f.id === cur ? "true" : "false"}>
          <button type="button" class="pick" data-pick={f.id} data-id={f.id} data-status={f.status} onclick={() => pick(f.id)}>
            <i class="kmark" data-kind={f.kind} aria-hidden="true"></i>
            {#if parts(f.id)}
              {@const p = parts(f.id)}
              <span class="pick-id">{p[0]}<mark class="match" data-match>{p[1]}</mark>{p[2]}</span>
            {:else}
              <span class="pick-id">{f.id}</span>
            {/if}
            {#if parts(f.title)}
              {@const p = parts(f.title)}
              <span class="pick-title">{p[0]}<mark class="match" data-match>{p[1]}</mark>{p[2]}</span>
            {:else}
              <span class="pick-title">{f.title}</span>
            {/if}
            <span class="pick-status">{f.status}</span>
          </button>
        </li>
      {/each}
      {#if found.status === "pending"}<li class="picker-skeleton" data-loading aria-hidden="true"></li>{/if}
      {#if found.status === "live" && hits.length === 0}<li class="picker-empty" data-empty>No ticket's id or title holds “{q}”.</li>{/if}
      {#if found.status === "error" || found.status === "refused"}<li class="picker-refused" data-refused role="alert">{found.message}</li>{/if}
    </ul>
  {/if}
</div>

<style>
  .picker { display: block; position: relative; min-width: 0; }
  .picker-input { width: 100%; padding: 5px 10px; color: var(--ink); font: inherit; background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .picker-input:hover, .picker-input:global([data-sheet~=hover]) { border-color: var(--muted); }
  .picker-input:focus-visible, .picker-input:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .picker-hits { position: absolute; z-index: 5; left: 0; right: 0; top: calc(100% + 4px); list-style: none; margin: 0; padding: 4px; max-height: 260px; overflow: auto; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-lg); box-shadow: 0 10px 30px var(--shadow); }
  .pick { display: grid; grid-template-columns: 10px auto minmax(0, 1fr) auto; gap: 8px; align-items: center; width: 100%; height: var(--pitch); padding: 0 8px; font: inherit; text-align: left; color: var(--ink); white-space: nowrap; background: none; border: 0; border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .pick > * { pointer-events: none; }
  .pick:hover, .pick:global([data-sheet~=hover]) { background: var(--hover); }
  .pick:focus-visible, .pick:global([data-sheet~=focus]) { outline: none; background: var(--row-sel); box-shadow: inset 0 0 0 2px var(--focus); }
  .hit[aria-selected="true"] .pick, .hit:global([data-sheet~=selected]) .pick { background: var(--row-sel); box-shadow: inset 3px 0 0 var(--accent); }
  .match { color: inherit; background: var(--sel); border-radius: var(--radius-xs); box-shadow: 0 1px 0 var(--accent); }
  .pick-id { color: var(--accent); font-variant-numeric: tabular-nums; }
  .pick-title { overflow: hidden; text-overflow: ellipsis; }
  .pick-status { padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .pick[data-status="Pending"] .pick-status { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .pick[data-status="Ready"] .pick-status { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .pick[data-status="Coding"] .pick-status { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .pick[data-status="In Review"] .pick-status { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .pick[data-status="Complete"] .pick-status { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .pick[data-status="Blocked"] .pick-status { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .pick[data-status="Superseded"] .pick-status { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .pick[data-status="Canceled"] .pick-status { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .pick[data-status="Draft"] .pick-status { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .pick[data-status="Epic"] .pick-status { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .pick[data-status="Doc"] .pick-status { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  .kmark { display: inline-block; width: 8px; height: 8px; background: var(--kind-build); border-radius: 50%; }
  .kmark[data-kind="scope"] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .kmark[data-kind="gap"] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .kmark[data-kind="spike"] { background: none; border: 2px solid var(--kind-spike); }
  .kmark[data-kind="doc"] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  .kmark[data-kind="execution"] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .picker-skeleton { height: 12px; margin: 8px; background: var(--skeleton); border-radius: var(--radius-sm); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .picker-empty { padding: 6px 8px; color: var(--muted); }
  .picker-refused { padding: 6px 8px; color: var(--warn); font-size: 11px; }
  @keyframes breathe { from { opacity: 1; } to { opacity: .45; } }
  @media (prefers-reduced-motion: reduce) {
    .pick, .picker-input { transition: none; }
    .picker-skeleton { animation: none; }
  }
</style>
