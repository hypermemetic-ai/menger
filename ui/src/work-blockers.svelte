<!-- SURFACE-7: what holds a ticket and what it holds — the ticket page's mark
     `<work-blockers id={id}/>` (SURFACE-3; #/blockers/:id mounts it alone).
     `work.blockers { id }` answers the blockers resolved at their statuses, a
     dangling id as its own, those out of reach counted (`hidden_blockers`).
     The set moves with the ticket (`get`, live on `blockers_changed` and the
     unlocks' events), and each chip is work-blocker, live on its own id: a
     blocker that completes drops out, and so does a held ticket that is
     done. -->
<script module>
  export const uses = ["tickets.work.blockers", "tickets.get", "tickets.watch"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import One from "./work-blocker.svelte";

  let { id } = $props();
  const blockers = live({ snapshot: "tickets.work.blockers", params: () => ({ id }), key: "id", on: { hidden_blockers: "upsert:." } });
  const me = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                    on: { blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket",
                          updated: "upsert:ticket", amended: "upsert:ticket", status_changed: "upsert:ticket" } });
  const ticket = $derived(me.rows.find((r) => r.id === id && r.title != null));
  // The ticket's own list once it is read (it moves); until then the method's answer.
  const held = $derived(ticket ? ticket.blocked_by ?? [] : blockers.rows.filter((r) => r.count == null && r.id !== id).map((r) => r.id));
  const holds = $derived(ticket ? ticket.unlocks ?? [] : []);
  const hidden = $derived(blockers.rows.find((r) => r.count != null)?.count ?? 0);
</script>

<section class="work-blockers" data-surface="work-blockers" data-ticket={id}>
  <div class="wb-side" data-side="blocker">
    <h3 class="wb-h">blocked by</h3>
    {#if blockers.status === "pending" && me.status === "pending"}
      <span class="wb-skel" data-loading aria-busy="true"></span>
    {:else}
      <div class="wb-chips">
        {#each held as b (b)}<One id={b} side="blocker"/>{/each}
      </div>
      <p class="wb-empty" data-empty>Nothing open holds {id}{hidden ? "" : " — it can start"}.</p>
    {/if}
    {#if hidden > 0}<p class="wb-hidden" data-field="hidden">{hidden} more out of your reach</p>{/if}
    {#if blockers.status === "error" || blockers.status === "refused"}<p class="wb-refused" data-refused>{blockers.message ?? "refused"}</p>{/if}
  </div>
  <div class="wb-side" data-side="unlocks">
    <h3 class="wb-h">blocks</h3>
    <div class="wb-chips">
      {#each holds as u (u)}<One id={u} side="unlocks"/>{/each}
    </div>
    <p class="wb-empty" data-empty>{id} holds nothing open.</p>
  </div>
</section>

<style>
  .work-blockers { display: grid; grid-template-columns: 1fr 1fr; gap: 6px 18px; margin: 8px 0; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .wb-h { margin: 0 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .wb-chips { display: flex; flex-wrap: wrap; gap: 6px; }
  /* A chip that is done draws nothing: the sentence shows when no chip is left. */
  .wb-chips:has(:global(.chip)) + .wb-empty { display: none; }
  .wb-empty { margin: 0; font-size: 12px; color: var(--muted); }
  .wb-hidden { margin: 6px 0 0; font-size: 11px; color: var(--muted); }
  .wb-refused { margin: 4px 0 0; font-size: 11px; color: var(--warn); }
  .wb-skel { display: inline-block; width: 160px; height: 18px; background: var(--skeleton); border-radius: var(--radius-pill); }
  @media (max-width: 720px) {
    .work-blockers { grid-template-columns: 1fr; }
  }
</style>
