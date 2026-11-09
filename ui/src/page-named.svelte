<!-- V2-50 (main's TICKETS-36): the page for a ticket named by an address of
     several segments (`#/ticket/CLA/CLB/CLX-2`,
     `#/ticket/hypermemetic:PLEXUS/PERF-19`) — a route's `:name` is one
     segment, so these routes come here. ticket-by-name reads the name whole
     from the address bar and resolves it: the uid route, the collision page
     or not found. SV-4: real Svelte (svelte/1), page-named.rhx's port
     (ui/retired-rhx/). -->
<script module>
  export const uses = [];
</script>

<script>
  import TicketByName from "plexus-ui:ticket-by-name";
  let { id = null } = $props();
</script>

<section class="page page-named">
  <div data-slot="top" data-here="ticket"></div>
  <TicketByName {id}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-named { min-height: 100vh; min-height: 100dvh; background: var(--bg); }
  .page-named > [data-slot=top] { position: sticky; top: 0; z-index: 4; }
</style>
