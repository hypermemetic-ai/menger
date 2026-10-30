<!-- SURFACE-3: a ticket's body, markdown through the host's render (the
     engine's sanitizer: no script, no style, no handler); live on the page's
     `get` spec, so an amend redraws it in place. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch"];
</script>

<script>
  import { live, markdown } from "plexus-ui/host";

  let { id } = $props();
  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket" } });
  const t = $derived(ticket.rows.find((r) => r.title != null));
</script>

<section class="ticket-body" data-surface="ticket-body">
  {#if t}
    {#if !t.body}
      <p class="empty" data-field="body" data-empty>No body yet.</p>
    {:else}
      <div class="md" data-field="body" data-ticket-body>{@html markdown(t.body)}</div>
    {/if}
  {/if}
</section>

<style>
  .ticket-body { padding-top: 14px; border-top: var(--border) solid var(--line); color: var(--ink); font-size: 14px; }
  .ticket-body .md { font-family: var(--sans); line-height: 1.55; overflow-wrap: anywhere; }
  .ticket-body .md :global(h1), .ticket-body .md :global(h2), .ticket-body .md :global(h3) { margin: 16px 0 4px; font-family: var(--font-display); font-size: 14px; font-weight: var(--weight-strong); }
  .ticket-body .md :global(p) { margin: 8px 0; }
  .ticket-body .md :global(ul), .ticket-body .md :global(ol) { margin: 6px 0; padding-left: 20px; }
  .ticket-body .md :global(a) { color: var(--accent); text-decoration: underline; }
  .ticket-body .md :global(code) { padding: 0 3px; font-family: var(--font-text); font-size: 12px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .ticket-body .md :global(pre) { overflow: auto; padding: 8px 10px; font-family: var(--font-text); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); }
  .ticket-body .md :global(pre code) { padding: 0; border: 0; background: none; }
  .ticket-body .md :global(table) { border-collapse: collapse; }
  .ticket-body .md :global(th), .ticket-body .md :global(td) { padding: 3px 8px; border: var(--border) solid var(--line); }
  .ticket-body .md :global(blockquote) { margin: 0; padding-left: 12px; color: var(--muted); border-left: 3px solid var(--line); }
  .ticket-body .empty { margin: 0; color: var(--muted); font-family: var(--font-text); font-size: 12px; }
</style>
