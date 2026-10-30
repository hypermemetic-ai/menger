<!-- SURFACE-6: the facets under the board's nav, svelte/1 — `nav facets
     { filter, base: "not category done" }`, one chip row per field (status,
     category, kind, severity, in, ready, blocked, has commits, menger doc:
     the fields the method answers, in its order), each value with its count.
     A chip narrows the route's filter (`#/f/<filter> and <rule>`, the
     field's other rule replaced); one the filter already holds is
     `[data-selected]` and a click takes it out. A filter that is not a flat
     `and` of plain rules — a `(`, an `or`, a quote but a status's — is never
     cut: a chip puts it whole beside its rule, `(<filter>) and <rule>`, and
     no chip is drawn selected on it. Asked again when the route's filter
     moves (the prop `filter`). Placed by a mark, `<nav-facets
     filter={filter}/>`, or on its own at `#/facets/:filter`. PERF-22, FRAME-50: given
     `after` (a bus topic: the board's frame passes "board.drawn") it asks once
     it is heard; without it, at once. -->
<script module>
  export const uses = ["tickets.nav.facets"];
  const FIELDS = ["status", "category", "kind", "severity", "in", "ready", "blocked", "has commits", "menger doc"];
  const FLAGS = new Set(["blocked", "ready", "has commits", "menger doc"]);

  /** The rule a chip stands for, in the filter language's own spelling. */
  export function ruleOf(field, value) {
    if (FLAGS.has(field)) return value === "yes" ? field : "not " + field;
    return value.includes(" ") ? `${field} "${value}"` : `${field} ${value}`;
  }
  /** The filter's rules when it is a flat `and` of plain ones, else null. */
  function flatParts(now) {
    if (now === "") return [];
    if (now.includes("(") || now.includes(" or ")) return null;
    const parts = now.split(" and ");
    return parts.every((p) => !p.includes('"') || p.startsWith('status "')) ? parts : null;
  }
  /** The filter a chip's click leaves. */
  export function narrowed(now, field, rule) {
    now = (now ?? "").trim();
    const parts = flatParts(now);
    if (parts === null) return `(${now}) and ${rule}`;
    const had = parts.includes(rule);
    const same = (p) => p.startsWith(field + " ") || (FLAGS.has(field) && (p === field || p === "not " + field));
    const kept = parts.filter((p) => !same(p) && p !== rule);
    if (!had) kept.push(rule);
    return kept.join(" and ");
  }
  export function selected(now, rule) {
    const parts = flatParts((now ?? "").trim());
    return parts !== null && parts.includes(rule);
  }
</script>

<script>
  import { untrack } from "svelte";
  import { bus, live } from "plexus-ui/host";

  let { filter = null, after = null } = $props();
  const b = bus();
  // PERF-22, FRAME-50: `after` names a bus topic to hear before the first
  // call — the board's frame passes "board.drawn", the rows being what the
  // person waits for; none (a page of its own, the sheet's tiles): at once.
  // A section cannot assume its siblings are mounted, so it never guesses
  // there is no board. FRAME-53: the topic is sticky (board-rows-sv says
  // `board.drawn` with `{ sticky: true }`), so `{ last: true }` hears it at
  // once when it was said before this mounted — a remount after the board
  // drew (a hot-swap, a section set) asks at once; nothing looks at the page.
  const wait = untrack(() => after);
  let drawn = $state(!wait);
  if (wait) b.on(wait, () => (drawn = true), { last: true });
  const facets = live({ snapshot: "tickets.nav.facets", params: () => ({ filter: filter ?? null, base: "not category done" }), key: "field", when: () => drawn });
  const byField = $derived(FIELDS.map((f) => [f, facets.rows.filter((r) => r.field === f)]).filter(([, rows]) => rows.length > 0));

  function narrow(field, rule) {
    const t = narrowed(filter, field, rule).replaceAll("%", "%25").replaceAll("/", "%2F").replaceAll("#", "%23");
    b.navigate(t === "" ? "#/" : "#/f/" + t);
  }
</script>

<section class="nav-facets" data-surface="nav-facets" data-filter={filter} aria-label="facets">
  <h2 class="nf-head">Facets {#if filter != null}<span class="nf-of" title={filter}>of {filter}</span>{/if}</h2>
  {#if facets.status === "pending"}
    <div class="nf-skel-wrap" data-loading aria-busy="true"><span class="nf-skel"></span><span class="nf-skel"></span><span class="nf-skel"></span></div>
  {:else}
    {#if facets.status === "error" || facets.status === "refused"}
      <p class="nf-note" data-refused role="status">{facets.message ?? "these facets are not answered for this filter"}</p>
    {:else if facets.rows.length === 0}
      <p class="nf-note" data-empty>Nothing open matches — no facet to narrow by.</p>
    {/if}
    {#each byField as [fld, rows] (fld)}
      <div class="nf-field" data-field={fld}>
        <span class="nf-name">{fld}</span>
        <div class="nf-chips">
          {#each rows as f (f.value)}
            {@const rule = ruleOf(fld, f.value)}
            {@const on = selected(filter, rule)}
            <button class="nf-chip" type="button" data-selected={on ? "" : undefined} aria-pressed={on ? "true" : "false"}
                    data-facet={fld} data-value={f.value} data-rule={rule} title={(on ? "take out: " : "narrow: ") + rule}
                    onclick={() => narrow(fld, rule)}><span class="nf-v">{f.value}</span><span class="nf-n" data-held>{f.held}</span></button>
          {/each}
        </div>
      </div>
    {/each}
  {/if}
</section>

<style>
  .nav-facets { display: block; padding: 8px 10px 12px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); background: var(--panel); }
  .nf-head { display: flex; gap: 6px; align-items: baseline; margin: 0 0 8px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .nf-of { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-transform: none; letter-spacing: 0; color: var(--accent); }
  .nf-field { margin-bottom: 8px; }
  .nf-name { display: block; margin-bottom: 3px; font-size: 11px; color: var(--muted); }
  .nf-chips { display: flex; flex-wrap: wrap; gap: 4px; }
  .nf-chip { display: inline-flex; gap: 5px; align-items: center; padding: 0 3px 0 8px; font: inherit; font-size: 11px; line-height: 20px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); cursor: pointer; transition: background-color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease), color var(--motion-fast) var(--ease); }
  .nf-chip:hover, .nf-chip:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); border-color: var(--muted); }
  .nf-chip:focus-visible, .nf-chip:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .nf-chip[data-selected], .nf-chip:global([data-sheet~=selected]) { color: var(--accent); background: var(--sel); border-color: var(--accent); }
  .nf-n { padding: 0 6px; line-height: 16px; color: var(--muted); background: var(--card); border-radius: var(--radius-pill); font-variant-numeric: tabular-nums; }
  .nf-chip[data-selected] .nf-n, .nf-chip:global([data-sheet~=selected]) .nf-n { color: var(--accent); }
  .nf-field[data-field=status] .nf-chip[data-value=Draft] .nf-v { color: var(--st-draft-fg); }
  .nf-field[data-field=status] .nf-chip[data-value=Pending] .nf-v { color: var(--st-pending-fg); }
  .nf-field[data-field=status] .nf-chip[data-value=Ready] .nf-v { color: var(--st-ready-fg); }
  .nf-field[data-field=status] .nf-chip[data-value=Coding] .nf-v { color: var(--st-coding-fg); }
  .nf-field[data-field=status] .nf-chip[data-value="In Review"] .nf-v { color: var(--st-in-review-fg); }
  .nf-field[data-field=status] .nf-chip[data-value=Complete] .nf-v { color: var(--st-complete-fg); }
  .nf-field[data-field=status] .nf-chip[data-value=Blocked] .nf-v { color: var(--st-blocked-fg); }
  .nf-field[data-field=status] .nf-chip[data-value=Superseded] .nf-v { color: var(--st-superseded-fg); }
  .nf-field[data-field=status] .nf-chip[data-value=Canceled] .nf-v { color: var(--st-canceled-fg); }
  .nf-field[data-field=status] .nf-chip[data-value=Epic] .nf-v { color: var(--st-epic-fg); }
  .nf-field[data-field=status] .nf-chip[data-value=Doc] .nf-v { color: var(--st-doc-fg); }
  .nf-note { margin: 4px 0; font-size: 11px; color: var(--muted); }
  .nf-note[data-refused] { color: var(--warn); }
  .nf-skel-wrap { display: flex; flex-direction: column; gap: 6px; }
  .nf-skel { display: block; height: 12px; border-radius: var(--radius-sm); background: var(--skeleton); animation: nf-pulse var(--motion-pulse) var(--ease) infinite alternate; }
  .nf-skel:nth-child(2) { width: 70%; }
  .nf-skel:nth-child(3) { width: 45%; }
  @keyframes nf-pulse { from { opacity: .55; } to { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) {
    .nf-chip { transition: none; }
    .nf-skel { animation: none; }
  }
</style>
