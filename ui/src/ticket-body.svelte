<!-- SURFACE-3: a ticket's body, live on the page's `get` spec, so an amend
     redraws it in place. SURFACE-16: the rendering is ticket-view's, a sibling
     inlined here — no section for it, its markdown through the host's render
     (the engine's sanitizer: no script, no style, no handler). EDITOR-10: a
     lone file or URL in it drawn as its kind by media-draw, as the editor
     draws it. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import TicketView from "./ticket-view.svelte";
  // EDITOR-20: the bodies drawn below give their ticket ids as pills (ticket-pills).
  import PillTable, { providePills } from "./ticket-pills.svelte";
  providePills();
  import { drawMedia } from "./media-draw.svelte";

  let { id } = $props();
  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  const t = $derived(ticket.rows.find((r) => r.title != null));
  const b = bus();
  // EDITOR-4: the ticket page is "the ticket last opened" for the editor at #/ (editor-screen reads it).
  $effect(() => { try { if (id) localStorage.setItem("menger.editor.last", id); } catch { /* a private window */ } });
</script>

<section class="ticket-body" data-surface="ticket-body" use:drawMedia={{ bus: b, text: t?.body }}>
  <PillTable/>{#if t}<TicketView body={t.body}/>{/if}
</section>

<style>
  .ticket-body { padding-top: 14px; border-top: var(--border) solid var(--line); }
</style>
