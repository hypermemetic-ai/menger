<!-- SURFACE-8: search — #/search/:q, and #/search/:q/f/:filter under the
     route's filter (FILTER-3's language; #/search alone asks nothing). Two
     reads side by side: `find` over ids and titles (the match marked where
     the title holds it), and `search` over titles and bodies (FTS5, best
     first), each hit's title and passage marked as the method marked them,
     paged by offset eight at a time — the `searched` event (matched, next)
     is the pager, kept as a row by `upsert:.`. A filter the service refuses
     is said in its words. STUDIO-12: the page is page-search.s — the
     board link and the jump button are app-top's in its `top` section, the
     palette its `palette` section's; this is the query's lists.
     ARCHIVE-8: `include` (#/search/:q/include/archived, and
     …/include/archived/f/:filter) is the archive switch: on, both lists ask
     under `(archived or not archived) and <the route's filter>` — a filter
     that names `archived` lifts the default that admits no archived ticket
     (Filter::names_archived), and this one admits every ticket either way —
     and an archived row says so; off, the route's filter alone, as before. -->
<script module>
  // PERF-15: a row is the four fields drawn, not the whole summary row.
  // ARCHIVE-8: and `archived`, the mark — a live row carries none, so
  // asking it costs nothing but the archived rows' own few bytes.
  const FIELDS = ["id", "uid", "title", "kind", "status", "archived"];
  /** ARCHIVE-8: the archive's filter — every ticket, archived or not. */
  const EITHER = "archived or not archived";
  const LIMIT = 8;
  export const uses = ["tickets.find", "tickets.search"];

  /** A title cut around the first case-blind match of q: [{ text, hit }]. */
  function marked(text, q) {
    const at = q ? String(text).toLowerCase().indexOf(String(q).toLowerCase()) : -1;
    if (at < 0) return [{ text, hit: false }];
    return [{ text: text.slice(0, at), hit: false }, { text: text.slice(at, at + q.length), hit: true }, { text: text.slice(at + q.length), hit: false }].filter((m) => m.text !== "");
  }
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import { ticketHref } from "./ticket-links.svelte";

  let { q = null, filter = null, include = null, sheet = null } = $props();
  const b = bus();
  // ARCHIVE-8: the archive switch, from the route; the filter both lists
  // ask under — first, so a display part (`· flat`) stays the text's last.
  const archived = $derived(include === "archived");
  const asks = $derived(archived ? (filter ? `(${EITHER}) and ${filter}` : EITHER) : filter);

  // The page's offset belongs to its query and filter: a route to another
  // (an update in place, no remount) starts at page one.
  const asked = $derived(`${q}\n${asks}`);
  let paged = $state({ asked: "", offset: 0 });
  const offset = $derived(paged.asked === asked ? paged.offset : 0);

  const found = live({ snapshot: "tickets.find", params: () => ({ query: q ?? "", filter: asks, limit: 12, fields: FIELDS }), key: "id", when: () => !!q });
  // `key` is the spec's, unused here: a snapshot with no changes stream
  // folds nothing by key. Each row is a whole event (`upsert:.`): a `hit`
  // (its id at `summary.id`, its title and passage marked) or the one
  // `searched` (matched, next, words), which has no id — the pager's.
  const hits = live({ snapshot: "tickets.search", params: () => ({ query: q ?? "", filter: asks, limit: LIMIT, offset }), key: "id",
                      on: { hit: "upsert:.", searched: "upsert:." }, when: () => !!q });
  const rows = $derived(hits.rows.filter((h) => h.summary));
  const page = $derived(hits.rows.find((h) => h.matched !== undefined));
  // SEARCH-2: a word the index does not know also matched the words a typo
  // away — said, so a mark on a word never typed is explained.
  const corrected = $derived((page?.words ?? []).filter((w) => w.also?.length));
  // One refusal: find and search are asked under the same filter.
  const refused = $derived(found.status === "error" || found.status === "refused" ? found.message ?? "this search is not taken"
    : hits.status === "error" || hits.status === "refused" ? hits.message ?? "this search is not taken" : null);

  function next() { paged = { asked, offset: offset + LIMIT }; }
  function prev() { paged = { asked, offset: Math.max(0, offset - LIMIT) }; }
  /** The route for this query: with the archive or not, under a filter or not. */
  const route = (on, f) => "#/search/" + encodeURIComponent(q) + (on ? "/include/archived" : "") + (f ? "/f/" + encodeURIComponent(f) : "");
  function everything() { b.navigate(route(archived, null)); }
  function toggle(e) { b.navigate(route(e.currentTarget.checked, filter)); }
</script>

<article class="search-page" data-surface="search-page" data-q={q}>
  {#if !q}
    <p class="sp-empty" data-empty>Type words to search: ids and titles on the left, bodies on the right.</p>
  {:else}
    <div class="sp-head">
      <h1>search <q data-field="q">{q}</q></h1>
      {#if filter}
        <span class="sp-filter" data-field="filter">honours the route's filter <code>{filter}</code> <button data-action="everything" onclick={everything}>search everything</button></span>
      {:else}
        <span class="sp-filter">every ticket</span>
      {/if}
      <label class="sp-arch" data-field="include">
        <input type="checkbox" data-action="archived" checked={archived} onchange={toggle}/>
        include archived
      </label>
    </div>
    {#if refused}<p class="sp-refused" data-refused role="alert">{refused}</p>{/if}
    {#if corrected.length}
      <p class="sp-corrected" data-corrected>also read as {#each corrected as w, i (i)}{i ? "; " : ""}“{w.word}” → {w.also.join(", ")}{/each}</p>
    {/if}
    <div class="sp-grid">
      <section class="panel sp-sec find" data-method="tickets.find">
        <h2>ids and titles <b data-count="find">{found.rows.length}</b></h2>
        {#if sheet === "loading" || found.status === "pending"}<div class="sp-loading" data-loading><i></i><i></i><i></i></div>{/if}
        {#each found.rows as f (f.id)}
          <a class="res" data-result="find" data-id={f.id} data-status={f.status} data-archived={f.archived ? "" : undefined} href={ticketHref(f)}>
            <i class="kmark" data-kind={f.kind}></i>
            <span class="row-id" data-field="id">{f.id}</span>
            <span class="row-title" data-field="title">{#each marked(f.title, q) as m, i (i)}{#if m.hit}<mark>{m.text}</mark>{:else}{m.text}{/if}{/each}</span>
            {#if f.archived}<span class="arch" data-field="archived">archived</span>{/if}
          </a>
        {/each}
        {#if found.status === "live" && found.rows.length === 0}<p class="sp-empty" data-empty>No id or title says “{q}”.</p>{/if}
      </section>
      <section class="panel sp-sec" data-method="tickets.search">
        <h2>in bodies {#if page}<b data-count="search">{page.matched}</b>{/if}<span class="m">best first</span></h2>
        {#if hits.status === "pending"}<div class="sp-loading" data-loading><i></i><i></i><i></i></div>{/if}
        {#each rows as h (h.summary.id)}
          <a class="res" data-result="search" data-id={h.summary.id} data-status={h.summary.status} data-corrected={h.corrected ? "" : undefined} data-archived={h.summary.archived ? "" : undefined} href={ticketHref(h.summary)}>
            <i class="kmark" data-kind={h.summary.kind}></i>
            <span class="row-id" data-field="id">{h.summary.id}</span>
            <span class="row-title" data-field="title">{#each h.title as m, i (i)}{#if m.hit}<mark>{m.text}</mark>{:else}{m.text}{/if}{/each}</span>
            {#if h.summary.archived}<span class="arch" data-field="archived">archived</span>{/if}
            <span class="row-status" data-field="status">{h.summary.status}{#if h.corrected}<span class="typo" title="matched a word a typo away"> ~</span>{/if}</span>
            <span class="snip" data-field="snippet">{#each h.snippet as m, i (i)}{#if m.hit}<mark>{m.text}</mark>{:else}{m.text}{/if}{/each}</span>
          </a>
        {/each}
        {#if page && page.matched === 0}
          <p class="sp-empty" data-empty>No body says “{q}”.</p>
        {:else if page}
          <div class="pager" data-pager>
            <span><b data-field="range">{offset + 1}–{offset + rows.length}</b> of <b data-field="matched">{page.matched}</b> · offset {offset}, limit {LIMIT}</span>
            <button class="btn" data-action="prev" disabled={offset === 0} onclick={prev}>prev</button>
            <button class="btn" data-action="next" disabled={page.next == null} onclick={next}>next</button>
          </div>
        {/if}
      </section>
    </div>
  {/if}
</article>

<style>
  .search-page { display: block; max-width: 1180px; margin: 0 auto; padding: 16px 20px 40px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .search-page .btn { padding: 3px 10px; font: inherit; font-size: 12px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: border-color var(--motion-fast) var(--ease), background-color var(--motion-fast) var(--ease); }
  .search-page .btn:hover:not(:disabled), .search-page .btn:global([data-sheet~=hover]):not(:disabled) { border-color: var(--muted); background: var(--hover); }
  .search-page .btn:focus-visible, .search-page .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .search-page .btn:disabled, .search-page .btn:global([data-sheet~=disabled]) { color: var(--disabled); cursor: default; }
  .search-page .panel { background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .search-page .sp-head { display: flex; align-items: baseline; gap: 10px; flex-wrap: wrap; margin: 0 0 4px; }
  .search-page .sp-head h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); }
  .search-page .sp-head h1 q { color: var(--accent); }
  .search-page .sp-filter { display: flex; gap: 6px; align-items: center; font-size: 11px; color: var(--muted); }
  .search-page .sp-filter code { padding: 0 6px; color: var(--ink); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .search-page .sp-filter button { padding: 0 6px; font: inherit; font-size: 11px; color: var(--muted); background: none; border: 0; cursor: pointer; text-decoration: underline; }
  .search-page .sp-filter button:hover, .search-page .sp-filter button:global([data-sheet~=hover]) { color: var(--ink); }
  .search-page .sp-filter button:focus-visible, .search-page .sp-filter button:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .search-page .sp-arch { display: inline-flex; gap: 5px; align-items: center; margin-left: auto; font-size: 12px; color: var(--muted); cursor: pointer; }
  .search-page .sp-arch input { margin: 0; accent-color: var(--accent); cursor: pointer; }
  .search-page .sp-arch input:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .search-page .sp-arch:hover { color: var(--ink); }
  /* ARCHIVE-8: the archived mark, a column of its own before the status (search) or after the title (find). */
  .search-page .arch { grid-column: 4; padding: 0 6px; font-size: 10px; line-height: 16px; color: var(--muted); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); white-space: nowrap; }
  .search-page .res[data-archived] .row-title { color: var(--muted); }
  .search-page .sp-refused { margin: 8px 0 0; font-size: 12px; color: var(--warn); }
  .search-page .sp-corrected { margin: 6px 0 0; font-size: 11px; color: var(--muted); }
  .search-page .sp-grid { display: grid; grid-template-columns: 340px minmax(0, 1fr); gap: 20px; align-items: start; margin-top: 12px; }
  @media (max-width: 860px) { .search-page .sp-grid { grid-template-columns: minmax(0, 1fr); } }
  .search-page .sp-sec { overflow: hidden; }
  .search-page .sp-sec h2 { display: flex; gap: 8px; align-items: baseline; margin: 0; padding: 8px 12px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; border-bottom: var(--border) solid var(--line); }
  .search-page .sp-sec h2 b { color: var(--ink); letter-spacing: 0; }
  .search-page .sp-sec h2 .m { margin-left: auto; text-transform: none; letter-spacing: 0; }
  .search-page .res { display: grid; grid-template-columns: 10px 92px minmax(0, 1fr) auto auto; gap: 4px 10px; align-items: center; padding: 7px 12px; color: var(--ink); text-decoration: none; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .search-page .res:last-of-type { border-bottom: 0; }
  .search-page .res:hover, .search-page .res:global([data-sheet~=hover]) { background: var(--hover); }
  .search-page .res:focus-visible, .search-page .res:global([data-sheet~=focus]) { outline: none; box-shadow: inset 0 0 0 2px var(--focus); }
  .search-page .res[data-selected], .search-page .res:global([data-sheet~=selected]) { background: var(--row-sel); box-shadow: inset 3px 0 0 var(--accent); }
  .search-page .find .res { grid-template-columns: 10px 84px minmax(0, 1fr) auto; }
  .search-page .row-id { color: var(--accent); font-variant-numeric: tabular-nums; overflow: hidden; text-overflow: ellipsis; }
  .search-page .row-title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .search-page .row-status { grid-column: 5; display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .search-page [data-status=Pending] .row-status { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .search-page [data-status=Ready] .row-status { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .search-page [data-status=Coding] .row-status { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .search-page [data-status="In Review"] .row-status { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .search-page [data-status=Complete] .row-status { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .search-page [data-status=Blocked] .row-status { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .search-page [data-status=Superseded] .row-status { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .search-page [data-status=Canceled] .row-status { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .search-page [data-status="Draft"] .row-status { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .search-page [data-status="Epic"] .row-status { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .search-page [data-status="Doc"] .row-status { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  .search-page .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .search-page .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .search-page .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .search-page .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .search-page .kmark[data-kind=doc] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--kind-doc); border-radius: var(--radius-xs); }
  .search-page .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .search-page .snip { grid-column: 3 / -1; color: var(--muted); font-family: var(--sans); line-height: 1.5; overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; }
  .search-page mark { color: var(--ink); background: var(--sel); box-shadow: inset 0 -2px 0 var(--accent); border-radius: var(--radius-xs); padding: 0 1px; }
  .search-page mark:empty { display: none; }
  .search-page .pager { display: flex; align-items: center; gap: 8px; padding: 8px 12px; border-top: var(--border) solid var(--line); font-size: 12px; color: var(--muted); }
  .search-page .pager b { color: var(--ink); font-variant-numeric: tabular-nums; }
  .search-page .pager .btn:first-of-type { margin-left: auto; }
  .search-page .sp-empty { padding: 14px 12px; margin: 0; color: var(--muted); }
  .search-page .sp-loading { display: flex; flex-direction: column; gap: 8px; padding: 10px 12px; }
  .search-page .sp-loading i { display: block; height: 14px; border-radius: var(--radius-sm); background: var(--skeleton); }
</style>
