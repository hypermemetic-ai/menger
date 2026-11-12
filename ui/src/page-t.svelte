<!-- V2-42 (TICKETS-36 on v2): a ticket by its uid — `#/t/<uid>/<ID>-<slug>`,
     the link every part of the UI makes (ui/src/ticket-links.svelte). Only
     the uid is read: `get` takes it and answers the ticket whatever its id
     is now, so a moved ticket's old link still lands; `<ID>-<slug>` is the
     reader's, never trusted (ticket-head puts a stale one right in the
     address bar). The page drawn is the ticket's own, page-ticket, by the key
     `get` answered (`/edit` after the tail is page-editor's route, which
     resolves the uid itself). A deleted ticket's uid answers its tombstone:
     page-ticket draws it as deleted. A uid naming nothing says so.
     V2-50 (main's TICKETS-36): a route by uid names a uid, nothing else —
     `#/t/PERF-2/x` is not found, never read as a name (which `get` would
     resolve): the page draws the ticket only when the segment is 8 of
     Crockford's base32, as a uid is written. A live ticket is drawn through
     page-t-key, which keeps the original of two namesakes off the collision
     page (its key is a name that fits both). Each is placed by its bound
     mark, keyed by what it is given, so a new key is a new page. SV-4: real
     Svelte (svelte/1), page-t.rhx's port (ui/retired-rhx/). -->
<script module>
  export const uses = ["tickets.get"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import PageTicket from "plexus-ui:page-ticket";
  import PageTKey from "plexus-ui:page-t-key";

  let { uid = "", tail = null } = $props();
  const UID = /^[0-9a-hjkmnp-tv-z]{8}$/;
  const shaped = $derived(typeof uid === "string" && UID.test(uid.toLowerCase()));
  const t = live({ snapshot: "tickets.get", params: () => ({ id: uid }), key: "id" });
  const held = $derived(t.rows.filter((r) => r.title != null || Array.isArray(r.ids)));
  // A deleted ticket's tombstone answers `ids`: its key is the first.
  const gone = $derived(t.rows.filter((r) => Array.isArray(r.ids)).map((r) => r.ids[0]));
  const rows = $derived(t.rows.filter((r) => r.title != null && !Array.isArray(r.ids)));
  // The page stays while another uid is read (a link by uid followed from the ticket on screen): the ticket
  // last shown is kept until the new one answers, so the page is given a new ticket, never made again — its
  // nav, its top and what is open in them stay as they are.
  const reading = $derived(t.status === "pending" || t.status === "resyncing");
  let last = $state(null);
  $effect(() => { if (rows.length) last = { id: rows[0].id, address: rows[0].address }; });
  const shown = $derived(rows[0] ?? (reading ? last : null));
</script>

<section class="page-t" data-surface="ticket-by-uid" data-uid={uid}>
  {#if !shaped}
    <p class="empty" data-empty data-not-found>{"No ticket " + uid + " here — a link by uid names 8 letters and digits."}</p>
  {:else if gone.length}
    {#each gone as key (key)}
      <PageTicket id={key}/>
    {/each}
  {:else if shown}
    <PageTKey key={shown.id} address={shown.address}/>
  {:else if reading}
    <div class="wait" data-loading aria-busy="true"></div>
  {:else}
    <p class="empty" data-empty data-not-found>{"No ticket " + uid + " here" + (t.message == null ? "" : " — " + t.message) + "."}</p>
  {/if}
</section>

<style>
  .wait { min-height: 100vh; min-height: 100dvh; background: var(--bg); }
  .empty { margin: 0; padding: 28px; min-height: 100vh; color: var(--muted); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
</style>
