<!-- SURFACE-16: a ticket's rendering, its input the ticket itself — `body`,
     optional `title` and `status`, drawn from props: no `live`, no call,
     `uses` empty, the markdown through the host's render (the engine's
     sanitizer), the look on the tokens. It sits in any bundle with nothing
     else of menger. A parent holding the ticket imports it as a sibling,
     `./ticket-view.svelte`: its code inlined, no section per instance
     (perf/notes.md "SURFACE-16" has what a bound mark costs instead).
     V2-24: a body names a kept image `![alt](media:<hash>)` (the media
     service's `media.put` answers the hash); the page draws it from the
     shell's `/media/<hash>`, behind the session, so the link is rewritten
     to that path on this origin before the render — the sanitizer keeps an
     image's `src` only when it is http(s), and the page policy (`img-src
     'self'`) admits the origin's own.
     EDITOR-8: one surface. The body is drawn block by block — `blocks()`
     below splits it where a blank line ends a block, keeping a fence, a
     house mark from its open line to its close (`<m-summary>` … `</m-summary>`),
     a comment and a list's continuation whole — each block through
     one `{@html markdown(…)}` (the body's references and media links with it). The editor (editor-body) draws the same blocks with the
     same component: each drawn block there is this one mounted with `block`
     (the block's text as `body`, the body's references as `defs`), drawing
     that block alone through the same `{@html markdown(…)}`, so a block the
     caret is not in is drawn there exactly as here; the caret's block shows
     its markdown. `edit` (an action given the body's element) and
     `titleEdit` (one given the title's) are how the editor sits inside. The house's marks (`m-*`, kept by
     the sanitizer since EDITOR-8) are drawn as cards by the rules below,
     one card per kind, the tokens' colours. -->
<script module>

  export const uses = [];

  // V2-24: `](media:<64 hex>)` → `](<origin>/media/<hash>)`, the hash lower-cased.
  const MEDIA = /\]\(\s*media:([0-9a-fA-F]{64})\s*\)/g;
  const origin = () => (typeof location === "object" && location?.origin) || "";
  export const mediaLinks = (md) => String(md ?? "").replace(MEDIA, (_, hash) => `](${origin()}/media/${hash.toLowerCase()})`);

  const FENCE = /^ {0,3}(`{3,}|~{3,})/;
  const HOUSE_OPEN = /^<(m-[a-z0-9-]+)(?:\s[^>]*)?>\s*$/;
  const LIST = /^ {0,3}(?:[-*+]|\d{1,9}[.)])(?:\s|$)/;
  const REF = /^ {0,3}\[[^\]]+\]:\s*\S/;
  const blank = (l) => l.trim() === "";

  /** The line after the fence opened at `i` closes (or the end). */
  function fenceEnd(lines, i) {
    const m = FENCE.exec(lines[i]);
    const ch = m[1][0], len = m[1].length;
    for (let k = i + 1; k < lines.length; k++) {
      const c = /^ {0,3}(`{3,}|~{3,})\s*$/.exec(lines[k]);
      if (c && c[1][0] === ch && c[1].length >= len) return k + 1;
    }
    return lines.length;
  }
  /** The line after the close of the house mark opened at `i`, or -1 when it never closes. */
  function houseEnd(lines, i, tag) {
    const open = new RegExp(`^<${tag}(?:\\s[^>]*)?>\\s*$`), close = new RegExp(`^</${tag}>\\s*$`);
    let depth = 0;
    for (let k = i; k < lines.length; k++) {
      if (k > i && FENCE.test(lines[k])) { k = fenceEnd(lines, k) - 1; continue; }
      if (open.test(lines[k])) depth++;
      else if (close.test(lines[k]) && --depth === 0) return k + 1;
    }
    return -1;
  }

  /** `src` as its blocks: `[{ from, to, text }]`, offsets into `src`, `to` the end of the block's last
   *  line (the blank lines between blocks belong to none). A block is what a blank line ends, but a fence
   *  is whole, a house mark is whole from its open line to its close, a comment to its `-->`, and a line
   *  indented, or a list's next item, after blank lines joins the block before it — merging never
   *  changes what the markdown draws; splitting could. */
  export function blocks(src) {
    const text = String(src ?? "");
    const lines = text.split("\n");
    const at = [0];
    for (const l of lines) at.push(at[at.length - 1] + l.length + 1);
    const out = [];
    let i = 0;
    while (i < lines.length) {
      if (blank(lines[i])) { i++; continue; }
      const start = i;
      let end;
      const house = HOUSE_OPEN.exec(lines[i]);
      if (FENCE.test(lines[i])) end = fenceEnd(lines, i);
      else if (house && houseEnd(lines, i, house[1]) > 0) end = houseEnd(lines, i, house[1]);
      else if (/^ {0,3}<!--/.test(lines[i]) && !lines[i].includes("-->")) {
        end = i + 1;
        while (end < lines.length && !lines[end - 1].includes("-->")) end++;
      } else {
        end = i;
        while (end < lines.length && !blank(lines[end])) {
          if (end > i && FENCE.test(lines[end])) { end = fenceEnd(lines, end); continue; }
          end++;
        }
      }
      // A continuation after blank lines: an indented line, or the next item of a list this block began.
      for (;;) {
        let k = end;
        while (k < lines.length && blank(lines[k])) k++;
        if (k === end || k >= lines.length) break;
        const indented = /^(?: {2,}|\t)\S/.test(lines[k]);
        const item = LIST.test(lines[start]) && LIST.test(lines[k]) && !house;
        if (!indented && !item) break;
        end = k;
        while (end < lines.length && !blank(lines[end])) {
          if (FENCE.test(lines[end].trimStart()) && end > k) { end = fenceEnd(lines, end); continue; }
          end++;
        }
      }
      out.push({ from: at[start], to: at[end] - 1, text: text.slice(at[start], at[end] - 1) });
      i = end;
    }
    return out;
  }

  /** The link reference definitions of `src` (`[name]: url`), outside fences: each block is drawn with them. */
  export function refDefs(src) {
    const lines = String(src ?? "").split("\n"), defs = [];
    for (let i = 0; i < lines.length; i++) {
      if (FENCE.test(lines[i])) { i = fenceEnd(lines, i) - 1; continue; }
      if (REF.test(lines[i])) defs.push(lines[i]);
    }
    return defs.length ? "\n\n" + defs.join("\n") + "\n" : "";
  }
</script>

<script>
  import { markdown } from "plexus-ui/host";

  let { body = "", title = null, status = null, edit = null, titleEdit = null, block = false, defs: given = null } = $props();
  const parts = $derived(edit || block ? [] : blocks(body));
  const defs = $derived(block ? given ?? "" : edit ? "" : refDefs(body));
</script>

{#snippet drawn(text)}{@html markdown(mediaLinks(text) + defs)}{/snippet}

{#if block}{@render drawn(body)}{:else}
<article class="ticket-view" data-surface="ticket-view" data-editing={edit ? "" : undefined}>
  {#if title != null || status != null || titleEdit}
    <header class="head">
      {#if titleEdit}<h3 class="title" data-field="title" use:titleEdit></h3>
      {:else if title != null}<h3 class="title" data-field="title">{title}</h3>{/if}
      {#if status != null}<span class="status" data-field="status" data-status={status}>{status}</span>{/if}
    </header>
  {/if}
  {#if edit}
    <div class="md" data-field="body" data-ticket-body use:edit></div>
  {:else if !body}
    <p class="empty" data-field="body" data-empty>No body yet.</p>
  {:else}
    <div class="md" data-field="body" data-ticket-body>{#each parts as p (p.from)}<div class="blk" data-block>{@render drawn(p.text)}</div>{/each}</div>
  {/if}
</article>{/if}

<style>
  .ticket-view { color: var(--ink); font-size: 14px; }
  .head { display: flex; align-items: baseline; gap: 8px; margin-bottom: 6px; }
  .title { margin: 0; min-width: 0; flex: 1; font-family: var(--font-display); font-size: 14px; font-weight: var(--weight-strong); overflow-wrap: anywhere; }
  [data-editing] .title { font-size: 20px; line-height: 1.3; }
  .title[contenteditable] { padding: 2px 4px; margin-left: -5px; border: var(--border) solid transparent; border-radius: var(--radius); outline: none; }
  .title[contenteditable]:hover, .title[contenteditable]:global([data-sheet~=hover]) { border-color: var(--line); }
  .title[contenteditable]:focus, .title[contenteditable]:global([data-sheet~=focus]) { border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .title[contenteditable]:empty::before { content: "Untitled"; color: var(--muted); }
  .status { flex: none; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .status[data-status=Pending] { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .status[data-status=Ready] { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .status[data-status=Coding] { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .status[data-status="In Review"] { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .status[data-status=Complete] { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .status[data-status=Blocked] { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .status[data-status=Superseded] { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .status[data-status=Canceled] { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .status[data-status=Epic] { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .status[data-status=Doc] { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  /* The body: each block one box, drawn the same here and in the editor (where these rules reach
     inside the editor's own lines and widgets). */
  .md { font-family: var(--sans); line-height: 1.55; overflow-wrap: anywhere; }
  .md :global(.blk) { display: flow-root; }
  .md :global(.blk > :first-child) { margin-top: 0; }
  .md :global(.blk > :last-child) { margin-bottom: 0; }
  .md :global(.blk + .blk), .md :global(.cm-line + .blk), .md :global(.blk + .cm-line) { padding-top: 10px; }
  .md :global(.blk) { white-space: normal; }
  .md :global(h1), .md :global(h2), .md :global(h3), .md :global(h4) { margin: 16px 0 4px; font-family: var(--font-display); font-weight: var(--weight-strong); line-height: 1.3; }
  .md :global(h1) { font-size: 20px; }
  .md :global(h2) { font-size: 16px; }
  .md :global(h3), .md :global(h4), .md :global(h5), .md :global(h6) { font-size: 14px; }
  .md :global(p) { margin: 8px 0; }
  .md :global(ul), .md :global(ol) { margin: 6px 0; padding-left: 20px; }
  .md :global(a) { color: var(--accent); text-decoration: underline; }
  .md :global(code) { padding: 0 3px; font-family: var(--font-text); font-size: 12px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .md :global(pre) { overflow: auto; padding: 8px 10px; font-family: var(--font-text); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .md :global(pre code) { padding: 0; border: 0; background: none; }
  .md :global(pre:has(> code.language-mermaid)) { border-left: 3px solid var(--kind-scope); }
  .md :global(pre:has(> code.language-mermaid))::before { content: "mermaid"; display: block; margin-bottom: 4px; font-size: 10px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  .md :global(table) { border-collapse: collapse; }
  .md :global(th), .md :global(td) { padding: 3px 8px; border: var(--border) solid var(--line); }
  .md :global(blockquote) { margin: 0; padding-left: 12px; color: var(--muted); border-left: 3px solid var(--line); }
  .md :global(img) { max-width: 100%; height: auto; border-radius: var(--radius); }
  .md :global(hr) { border: 0; border-top: var(--border) solid var(--line); }
  /* EDITOR-8: the house's marks, as cards. A container mark (summary, decision, input, cases, beat,
     status, diagram, mockup, field with a body, …) is a card with its kind in the corner; a line mark
     (stat, option, step, question, answer, …) a row inside it. Every name is the mark's own text. */
  .md :global(:is(m-summary, m-decision, m-input, m-cases, m-beat, m-status, m-diagram, m-mockup, m-results, m-walk, m-code, m-compare, m-callout, m-acceptance, m-cards, m-card, m-dag, m-details, m-provides, m-consumes)) { position: relative; display: block; margin: 8px 0; padding: 12px 14px 10px; background: var(--card); border: var(--border) solid var(--line); border-left: 3px solid var(--accent); border-radius: var(--radius-lg); }
  .md :global(:is(m-summary, m-decision, m-input, m-cases, m-beat, m-status, m-diagram, m-mockup, m-results, m-walk, m-code, m-compare, m-callout, m-acceptance, m-cards, m-card, m-dag, m-details, m-provides, m-consumes))::before { position: absolute; top: 5px; right: 10px; font-family: var(--font-text); font-size: 10px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); }
  .md :global(m-summary)::before { content: "summary"; }
  .md :global(m-decision)::before { content: "decision " attr(data-id); }
  .md :global(m-input)::before { content: "input " attr(data-id); }
  .md :global(m-cases)::before { content: attr(label); }
  .md :global(m-beat)::before { content: attr(kicker); }
  .md :global(m-status)::before { content: "status"; }
  .md :global(m-diagram)::before { content: "diagram " attr(label); }
  .md :global(m-mockup)::before { content: "mockup " attr(title); }
  .md :global(m-results)::before { content: attr(label); }
  .md :global(m-walk)::before { content: attr(kicker); }
  .md :global(m-code)::before { content: attr(lang) " " attr(path); }
  .md :global(m-callout)::before { content: "note"; }
  .md :global(m-acceptance)::before { content: "acceptance"; }
  .md :global(m-decision) { border-left-color: var(--kind-spike); }
  .md :global(m-input) { border-left-color: var(--kind-spike); }
  .md :global(m-status) { border-left-color: var(--kind-build); }
  .md :global(m-diagram), .md :global(m-mockup) { border-left-color: var(--kind-scope); }
  .md :global(m-cases), .md :global(m-results) { border-left-color: var(--kind-execution); }
  .md :global(m-summary p:has(> m-field[data-field=standing])), .md :global(m-summary > m-field[data-field=standing]) { margin: 0 0 8px; padding-right: 70px; font-family: var(--font-display); font-size: 15px; line-height: 1.4; }
  .md :global(:is(m-field, m-answer, m-question, m-recommendation, m-option, m-choice, m-step, m-stat, m-ticket, m-badge, m-cite, m-commit, m-chip)) { display: inline; }
  .md :global(m-field[label])::before { content: attr(label); display: block; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .md :global(m-field:not([label]))::before { content: none; }
  .md :global(m-stat) { display: inline-flex; flex-direction: column; min-width: 100px; margin: 2px 6px 2px 0; padding: 5px 9px; vertical-align: top; font-family: var(--font-text); font-size: 11px; color: var(--muted); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .md :global(m-stat)::before { content: attr(value); font-family: var(--font-display); font-size: 15px; font-weight: var(--weight-strong); color: var(--ink); }
  .md :global(m-stat[tone=pass])::before { color: var(--live); }
  .md :global(m-stat[tone=warn])::before, .md :global(m-stat[tone=fail])::before { color: var(--warn); }
  .md :global(m-question) { display: block; margin: 0 0 8px; padding-right: 90px; font-family: var(--font-display); font-size: 15px; font-weight: var(--weight-strong); line-height: 1.4; }
  .md :global(m-option), .md :global(m-choice) { display: block; margin: 4px 0; padding: 4px 10px; font-size: 13px; background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .md :global(m-option)::before { content: attr(name); font-weight: var(--weight-strong); color: var(--ink); }
  .md :global(m-option[cost])::after { content: "cost: " attr(cost) " · benefit: " attr(benefit); display: block; font-size: 11px; color: var(--muted); }
  .md :global(:is(m-option, m-choice)[recommended]), .md :global(:is(m-option, m-choice)[picked]) { border-color: var(--accent); box-shadow: inset 3px 0 0 var(--accent); }
  .md :global(m-recommendation), .md :global(m-answer) { display: block; margin: 8px 0 0; font-size: 13px; }
  .md :global(m-recommendation)::before { content: "recommended" " · " attr(by) " "; font-size: 11px; color: var(--accent); }
  .md :global(m-answer[by])::before { content: "answer · " attr(by) " "; font-size: 11px; color: var(--live); }
  .md :global(m-step) { display: block; margin: 6px 0; padding: 4px 0 4px 12px; font-size: 13px; border-left: 2px solid var(--line); }
  .md :global(m-step)::before { content: attr(status) " · " attr(by) " · " attr(date); display: block; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .md :global(m-step[status=Complete]) { border-left-color: var(--live); }
  .md :global(m-step[status="In Review"]), .md :global(m-step[status=Coding]) { border-left-color: var(--accent); }
  .md :global(m-ticket)::before { content: attr(ref); padding: 0 5px; font-family: var(--font-text); font-size: 12px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-sm); }
  .md :global(m-badge) { padding: 0 6px; font-size: 11px; color: var(--chip-fg); background: var(--chip-bg); border-radius: var(--radius-pill); }
  .empty { margin: 0; color: var(--muted); font-family: var(--font-text); font-size: 12px; }
</style>
