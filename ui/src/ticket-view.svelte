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
     'self'`) admits the origin's own. -->
<script module>
  export const uses = [];
</script>

<script>
  import { markdown } from "plexus-ui/host";

  let { body = "", title = null, status = null } = $props();

  // V2-24: `](media:<64 hex>)` → `](<origin>/media/<hash>)`, the hash lower-cased.
  const MEDIA = /\]\(\s*media:([0-9a-fA-F]{64})\s*\)/g;
  const origin = () => (typeof location === "object" && location?.origin) || "";
  const mediaLinks = (md) => String(md ?? "").replace(MEDIA, (_, hash) => `](${origin()}/media/${hash.toLowerCase()})`);
  const drawn = $derived(markdown(mediaLinks(body)));
</script>

<article class="ticket-view" data-surface="ticket-view">
  {#if title != null || status != null}
    <header class="head">
      {#if title != null}<h3 class="title" data-field="title">{title}</h3>{/if}
      {#if status != null}<span class="status" data-field="status" data-status={status}>{status}</span>{/if}
    </header>
  {/if}
  {#if !body}
    <p class="empty" data-field="body" data-empty>No body yet.</p>
  {:else}
    <div class="md" data-field="body" data-ticket-body>{@html drawn}</div>
  {/if}
</article>

<style>
  .ticket-view { color: var(--ink); font-size: 14px; }
  .head { display: flex; align-items: baseline; gap: 8px; margin-bottom: 6px; }
  .title { margin: 0; min-width: 0; font-family: var(--font-display); font-size: 14px; font-weight: var(--weight-strong); overflow-wrap: anywhere; }
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
  .md { font-family: var(--sans); line-height: 1.55; overflow-wrap: anywhere; }
  .md :global(h1), .md :global(h2), .md :global(h3) { margin: 16px 0 4px; font-family: var(--font-display); font-size: 14px; font-weight: var(--weight-strong); }
  .md :global(p) { margin: 8px 0; }
  .md :global(ul), .md :global(ol) { margin: 6px 0; padding-left: 20px; }
  .md :global(a) { color: var(--accent); text-decoration: underline; }
  .md :global(code) { padding: 0 3px; font-family: var(--font-text); font-size: 12px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .md :global(pre) { overflow: auto; padding: 8px 10px; font-family: var(--font-text); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .md :global(pre code) { padding: 0; border: 0; background: none; }
  .md :global(table) { border-collapse: collapse; }
  .md :global(th), .md :global(td) { padding: 3px 8px; border: var(--border) solid var(--line); }
  .md :global(blockquote) { margin: 0; padding-left: 12px; color: var(--muted); border-left: 3px solid var(--line); }
  .md :global(img) { max-width: 100%; height: auto; border-radius: var(--radius); }
  .empty { margin: 0; color: var(--muted); font-family: var(--font-text); font-size: 12px; }
</style>
