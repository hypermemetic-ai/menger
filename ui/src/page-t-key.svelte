<!-- V2-50 (main's TICKETS-36): the page page-t draws for a ticket it found by
     its uid — page-ticket by the key `get` answered (`key`), unless that key
     is also a name of another ticket: the original of two namesakes is keyed
     `CLX-2`, which as a name fits both (`get --id CLX-2` answers `ambiguous`),
     so its page would be the collision page. Then the page is drawn by the
     ticket's full address (`address`), which names it alone. The key is
     asked once (no watch): a namesake made later is heard on the next open.
     SV-4: real Svelte (svelte/1), page-t-key.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = ["tickets.get"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import PageTicket from "plexus-ui:page-ticket";

  let { key, address = null } = $props();
  const named = live({ snapshot: "tickets.get", params: () => ({ id: key }), key: "id" });
  const ambiguous = $derived(named.rows.some((r) => Array.isArray(r.choices) || Array.isArray(r.candidates)));
  const shown = $derived(ambiguous && address != null ? address : key);
</script>

<div class="page-t-key" data-surface="ticket-by-key" data-key={key}>
  {#if named.status === "pending"}
    <div class="wait" data-loading aria-busy="true"></div>
  {:else}
    {#key shown}<PageTicket id={shown}/>{/key}
  {/if}
</div>

<style>
  .wait { min-height: 100vh; min-height: 100dvh; background: var(--bg); }
</style>
