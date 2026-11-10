<!-- The nav's row, v1's (AGS-12, AGS-13, AGS-25, AGS-29) in real Svelte
     (svelte/1): the toggle (▸/▾) when the node has anything under it, the
     kind's glyph, the id and the title as one link, a count — a shortcode's
     roll-up boxed (`done/total`), a ticket's children ringed — and the
     status as a tag (a dot where the nav is narrow). nav-group-sv draws a
     shortcode's row with it and nav-children-sv every ticket's; it draws
     the row alone, and the node's children are its importer's.

     A double-click on a row with children opens or closes it and opens
     nothing (AGS-29): a plain click on such a row waits out the double-click
     window before it follows the link; a leaf, a modified click, the toggle
     and the keys are immediate. -->
<script module>
  export const uses = [];
  /** AGS-13: a kind's glyph, one column of text. */
  export const GLYPH = { scope: "◆", build: "■", gap: "△", spike: "○", execution: "▶", doc: "¶" };
  /** AGS-29: how long a click on a row with children waits for a second. */
  const DOUBLE_CLICK_MS = 250;
</script>

<script>
  let { id, href, title = "", kind = null, status = null, frozen = false, expandable = false, open = false,
        children = null, rollup = null, held = null, mark = "row", ontoggle = () => {} } = $props();

  let pending = null;
  const follow = (e) => {
    if (!expandable || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey) return;
    e.preventDefault();
    if (pending != null) return;
    pending = setTimeout(() => { pending = null; location.hash = href.replace(/^#/, ""); }, DOUBLE_CLICK_MS);
  };
  const flip = (e) => {
    if (!expandable || e.target.closest?.("[data-nav-toggle]")) return;
    e.preventDefault();
    if (pending != null) { clearTimeout(pending); pending = null; }
    window.getSelection?.()?.removeAllRanges();
    ontoggle();
  };
</script>

<div class="nav-row" data-kind={kind} data-status={status} ondblclick={flip}>
  {#if expandable}
    <button class="toggle" data-nav-toggle={id} tabindex="-1" aria-label={open ? "close" : "open"} onclick={(e) => { if (e.detail < 2) ontoggle(); }}>{open ? "▾" : "▸"}</button>
  {:else}
    <span class="toggle"></span>
  {/if}
  <span class="glyph" title={kind ?? ""} aria-hidden="true">{kind ? GLYPH[kind] ?? "•" : "▣"}</span>
  <a class="link" data-nav-focus data-nav-row={mark === "row" ? id : undefined} data-nav-code={mark === "group" ? id : undefined}
     {href} title={title ? `${id} ${title}` : id} onclick={follow}>
    <span class="id">{id}</span>{#if title}<span class="title">{title}</span>{/if}{#if frozen}<span class="frozen" title="frozen">❄</span>{/if}
  </a>
  {#if held != null}<span class="held" data-nav-held title="matching the filter">{held}</span>{/if}
  {#if rollup}
    <span class="count boxed" title="{rollup.done} of {rollup.total} done"><span data-rollup-done>{rollup.done}</span><span class="of">/</span><span data-rollup-total>{rollup.total}</span></span>
  {:else if children}
    <span class="count ringed" title="{children} under it">{children}</span>
  {/if}
  {#if status}<span class="tag" data-tag={status}>{status}</span>{/if}
</div>

<style>
  .nav-row { display: grid; grid-template-columns: 16px 14px minmax(0, 1fr) auto auto auto; gap: 5px; align-items: center; height: var(--pitch); padding: 0 6px 0 2px; border-radius: var(--radius); user-select: none; transition: background-color var(--motion-fast) var(--ease); }
  .nav-row:hover, .nav-row:global([data-sheet~=hover]) { background: var(--hover); }
  .toggle { display: flex; align-items: center; justify-content: center; width: 16px; height: 100%; padding: 0; font-size: 10px; color: var(--muted); background: none; border: 0; cursor: pointer; }
  .toggle:hover, .toggle:global([data-sheet~=hover]) { color: var(--ink); }
  .glyph { text-align: center; font-size: 11px; line-height: 1; color: var(--muted); }
  .nav-row[data-kind=scope] .glyph { color: var(--kind-scope); }
  .nav-row[data-kind=build] .glyph { color: var(--kind-build); }
  .nav-row[data-kind=execution] .glyph { color: var(--kind-execution); }
  .nav-row[data-kind=gap] .glyph { color: var(--kind-gap); }
  .nav-row[data-kind=spike] .glyph { color: var(--kind-spike); }
  .nav-row[data-kind=doc] .glyph { color: var(--kind-doc); }
  .link { display: flex; gap: 6px; min-width: 0; overflow: hidden; white-space: nowrap; color: var(--ink); text-decoration: none; border-radius: var(--radius-sm); }
  .link:focus-visible, .link:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .id { flex: none; color: var(--accent); font-family: var(--mono); font-variant-numeric: tabular-nums; }
  .title { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .link:hover .title, .link:global([data-sheet~=hover]) .title { color: var(--accent); }
  .frozen { flex: none; color: var(--muted); }
  .held { font-size: 11px; color: var(--accent); font-variant-numeric: tabular-nums; }
  .count { font-size: 11px; color: var(--accent); font-variant-numeric: tabular-nums; white-space: nowrap; }
  .boxed { padding: 0 5px; line-height: 18px; border: var(--border) solid var(--line); border-radius: var(--radius-sm); background: var(--card); }
  .boxed .of { color: var(--muted); }
  .ringed { min-width: 18px; padding: 0 4px; line-height: 16px; text-align: center; border: var(--border) solid var(--line); border-radius: var(--radius-pill); }
  .tag { width: 76px; overflow: hidden; text-overflow: ellipsis; text-align: center; white-space: nowrap; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .tag[data-tag=Pending] { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .tag[data-tag=Ready] { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .tag[data-tag=Coding] { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .tag[data-tag="In Review"] { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .tag[data-tag=Complete] { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .tag[data-tag=Blocked] { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .tag[data-tag=Superseded] { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .tag[data-tag=Canceled] { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .tag[data-tag=Epic] { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .tag[data-tag=Doc] { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  /* A narrow nav (the board's side column) says a status as a dot of its colour. */
  @container nav-tree (max-width: 339px) {
    .tag { width: 8px; height: 8px; padding: 0; font-size: 0; line-height: 0; border-radius: 50%; background: var(--st-draft-fg); }
    .tag[data-tag=Pending] { background: var(--st-pending-fg); }
    .tag[data-tag=Ready] { background: var(--st-ready-fg); }
    .tag[data-tag=Coding] { background: var(--st-coding-fg); }
    .tag[data-tag="In Review"] { background: var(--st-in-review-fg); }
    .tag[data-tag=Complete] { background: var(--st-complete-fg); }
    .tag[data-tag=Blocked] { background: var(--st-blocked-fg); }
    .tag[data-tag=Superseded] { background: var(--st-superseded-fg); }
    .tag[data-tag=Canceled] { background: var(--st-canceled-fg); }
    .tag[data-tag=Epic] { background: var(--st-epic-fg); }
    .tag[data-tag=Doc] { background: var(--st-doc-fg); }
  }
  @media (max-width: 720px) {
    .nav-row { grid-template-columns: 28px 14px minmax(0, 1fr) auto auto auto; }
    .toggle { width: 28px; font-size: 13px; }
  }
  @media (prefers-reduced-motion: reduce) {
    .nav-row { transition: none; }
  }
</style>
