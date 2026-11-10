<!-- EDITOR-22: `<m-details summary="One short sentence.">…</m-details>` in a body is one line that opens to
     show its details. The owner, 2026-11-10: "we need a component which allows you to write into a document a
     short sentence about the work, and then it should open up and show more details".
     - **Made after the sanitizer, from the kept mark**: ticket-view's enhance asks `foldDetails(node)` for the
       marks its drawn block holds — the mark is the engine's, its `summary` and `open` kept as
       `data-prop-summary` / `data-prop-open`, its `status` and `ticket` as themselves — and mounts this
       component as each one's line; what the mark held moves, as it was drawn, into `.det-body` under it,
       `hidden="until-found"` while it is closed (find-in-page opens it: `beforematch`), shown in print. No
       HTML is pasted: the sentence is the host's `markdown()` (its sanitizer), so an id in it is a pill and
       `code` is code.
     - **The line**: a real `button` (the disclosure mark, `aria-expanded`, Tab, Enter and Space), an optional
       status dot (`done`, `running`, `next`, `blocked`, `needs-you`, painted with the stage tokens), the
       sentence, and `ticket`'s id at the line's end — words the pill pass makes a pill.
     - **What is open is this page's**: kept in this module (`OPEN`, by the sentence, its ticket and its
       place among its namesakes in the block), so a live update of the body, a hot-swap and the editor's
       redraw of a block keep it; a reload forgets it, and nothing is written to the body. `open` on the mark
       starts it open.
     - **In the editor** the line's sentence is the block's text (a click there puts the caret into its
       source, as on every block); the mark alone opens it, the caret staying where it was. -->
<script module>
  export const uses = [];

  /** The statuses a line's dot says (anything else: no dot). */
  export const STATUSES = ["done", "running", "next", "blocked", "needs-you"];
  /** What is open on this page, by a mark's key (`keyOf`): forgotten on a reload, never written. */
  const OPEN = new Map();

  /** A drawn body's `.det-body` shown or folded (`hidden="until-found"`), its mark saying which. */
  export function show(body, open) {
    if (open) body.removeAttribute("hidden");
    else body.setAttribute("hidden", "until-found");
    body.parentElement?.toggleAttribute("data-det-open", open);
  }

  const attr = (el, ...names) => { for (const n of names) { const v = el.getAttribute(n); if (v != null) return v; } return null; };

  /** The `m-details` of `node` (a drawn block) folded: each one's children moved into its `.det-body`, an
   *  empty `.det-line` before it for the line. Answers `[{ at, props }]`, the line's place and this
   *  component's props, for the caller to mount. A mark with no sentence stays a plain block; so does the
   *  mark a block is only a piece of (EDITOR-9's open, middle and close pieces, when it holds a page). */
  export function foldDetails(node) {
    const out = [];
    const seen = new Map();
    const piece = node.parentElement?.matches?.(".h-open, .h-mid, .h-close") ?? false;
    for (const el of node.querySelectorAll("m-details")) {
      if (piece && el.parentElement === node) continue;
      const summary = (attr(el, "data-prop-summary", "data-summary") ?? "").trim();
      if (!summary || el.querySelector(":scope > .det-line")) continue;
      const ticket = (attr(el, "ticket", "data-ticket") ?? "").trim();
      const said = (attr(el, "status", "data-status") ?? "").trim().toLowerCase();
      const status = STATUSES.includes(said) ? said : "";
      let body = el.querySelector(":scope > .det-body");
      if (!body) {
        body = document.createElement("div");
        body.className = "det-body";
        body.append(...el.childNodes);
        el.append(body);
      }
      const base = `${summary}\u0000${ticket}`;
      const n = seen.get(base) ?? 0;
      seen.set(base, n + 1);
      const key = `${base}\u0000${n}`;
      const flag = attr(el, "data-prop-open", "data-open");
      const open = OPEN.has(key) ? OPEN.get(key) : flag != null && flag.trim().toLowerCase() !== "false";
      el.setAttribute("data-folds", "");
      show(body, open);
      const at = document.createElement("div");
      at.className = "det-line";
      el.prepend(at);
      out.push({ at, props: { summary, status, ticket, key, body, open } });
    }
    return out;
  }

  /** The sentence as words, for the button's name (the markdown's marks aside). */
  const plain = (s) => String(s ?? "").replace(/[`*_~]/g, "").replace(/\[([^\]]*)\]\([^)]*\)/g, "$1");
</script>

<script>
  import { markdown } from "plexus-ui/host";

  let { summary = "", status = "", ticket = "", key = "", body = null, open: start = false } = $props();
  const begun = () => start;
  let open = $state(begun());

  function set(v) {
    open = v;
    if (key) OPEN.set(key, v);
    if (body) show(body, v);
  }
  // A click on the sentence opens it too, except on a link (a pill) and in the editor, where the sentence is
  // the block's text and a click there is the caret's.
  function said(ev) {
    if (ev.target?.closest?.("a[href]") || ev.currentTarget?.closest?.(".cm-editor")) return;
    set(!open);
  }
  // Find-in-page reaching words inside a closed one opens it.
  $effect(() => {
    if (!body) return;
    const found = () => set(true);
    body.addEventListener("beforematch", found);
    return () => body.removeEventListener("beforematch", found);
  });
</script>

<div class="det-row" data-surface="m-details" data-open={open ? "" : undefined}>
  <button type="button" class="det-mark" aria-expanded={open} aria-label={plain(summary)} onclick={() => set(!open)}><span class="det-tri" aria-hidden="true"></span></button>
  {#if status}<span class="det-dot" data-status={status} title={status} aria-label={status}></span>{/if}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <span class="det-sum" onclick={said}>{@html markdown(summary)}</span>
  {#if ticket}<span class="det-ticket">{ticket}</span>{/if}
</div>

<style>
  .det-row { display: flex; align-items: baseline; gap: 6px; min-width: 0; }
  .det-mark { flex: none; align-self: center; display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 18px; margin: 0 0 0 -2px; padding: 0; color: var(--muted); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .det-mark:hover, .det-mark:global([data-sheet~=hover]) { color: var(--ink); }
  .det-mark:focus-visible, .det-mark:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .det-tri { width: 0; height: 0; border-top: 4px solid transparent; border-bottom: 4px solid transparent; border-left: 6px solid currentColor; transition: transform 0.12s ease; }
  [data-open] .det-tri { transform: rotate(90deg); }
  .det-dot { flex: none; align-self: center; width: 7px; height: 7px; border-radius: 50%; background: var(--muted); }
  .det-dot[data-status=done] { background: var(--st-complete-fg); }
  .det-dot[data-status=running] { background: var(--st-coding-fg); }
  .det-dot[data-status=next] { background: var(--st-ready-fg); }
  .det-dot[data-status=blocked] { background: var(--st-blocked-fg); }
  .det-dot[data-status=needs-you] { background: var(--st-pending-fg); }
  .det-sum { flex: 1 1 auto; min-width: 0; cursor: pointer; }
  .det-sum :global(:is(p, h1, h2, h3, h4, h5, h6, ul, ol)) { display: inline; margin: 0; padding: 0; font: inherit; }
  .det-ticket { flex: none; margin-left: auto; padding-left: 8px; }
  @media print { .det-mark { display: none; } }
</style>
