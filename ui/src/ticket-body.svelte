<!-- SURFACE-3: a ticket's body, live on the page's `get` spec, so an amend
     redraws it in place. SURFACE-16: the rendering is ticket-view's, a sibling
     inlined here — no section for it, its markdown through the host's render
     (the engine's sanitizer: no script, no style, no handler). -->
<script module>
  export const uses = ["tickets.get", "tickets.watch"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import TicketView from "./ticket-view.svelte";

  let { id } = $props();
  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  const t = $derived(ticket.rows.find((r) => r.title != null));
</script>

<section class="ticket-body" data-surface="ticket-body">
  {#if t}<TicketView body={t.body}/>{/if}
</section>

<style>
  .ticket-body { padding-top: 14px; border-top: var(--border) solid var(--line); }
</style>
