<!-- V2-50 (main's TICKETS-36): a ticket by an address of several segments —
     `#/ticket/hypermemetic:PLEXUS/PERF-19`, `#/ticket/CLA/CLB/CLX-2`, what the
     shell sends `/hypermemetic:PLEXUS/PERF-19` to. A route's `:name` is one
     segment, so `#/ticket/:id` (page-ticket) takes a bare id or `org:ID`, and
     the routes with two to four segments draw page-named, which places this
     (ui/routes.json). The
     name is read whole from the address bar (ticket-links' `named`) and
     asked once: one ticket — the address replaced in place by its uid route
     (`location.replace`: no history entry), which page-t draws; several —
     the collision page (ticket-collision); none — not found; a deleted one —
     it says so. Marks: `[data-surface=ticket-by-name][data-name]`,
     `[data-loading]`, `[data-not-found]`, `[data-deleted]`. -->
<script module>
  export const uses = ["tickets.get"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import { named, ticketHref, baseOf } from "./ticket-links.svelte";
  import Collision from "./ticket-collision.svelte";

  const name = named(location.hash)?.name ?? "";
  const got = live({ snapshot: "tickets.get", params: () => ({ id: name }), key: "id" });
  const t = $derived(got.rows.find((r) => r.title != null && r.uid));
  const choices = $derived(got.rows.some((r) => Array.isArray(r.choices) || Array.isArray(r.candidates)));
  const tomb = $derived(got.rows.flatMap((r) => (Array.isArray(r.tombstones) ? r.tombstones : []))[0]);
  $effect(() => {
    if (t) {
      const [, query] = location.hash.split(/\?(.*)/s);
      location.replace(ticketHref(t, baseOf()) + (query ? "?" + query : ""));
    }
  });
</script>

<section class="page-named" data-surface="ticket-by-name" data-name={name}>
  {#if choices}
    <div class="wrap"><Collision id={name}/></div>
  {:else if tomb}
    <p class="empty" data-deleted><span class="badge">deleted</span> <strong>{tomb.id}</strong> was deleted{tomb.title ? " — " + tomb.title : ""}.</p>
  {:else if got.status === "pending" || got.status === "resyncing" || t}
    <div class="wait" data-loading aria-busy="true"></div>
  {:else}
    <p class="empty" data-empty data-not-found>No ticket {name} here.</p>
  {/if}
</section>

<style>
  .page-named { display: block; min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .wrap { max-width: 1340px; margin: 0 auto; padding: 16px 28px 48px; }
  .wait { min-height: 100vh; background: var(--bg); }
  .empty { margin: 0; padding: 28px; color: var(--muted); }
  .badge { padding: 0 6px; font-size: 10px; line-height: 16px; text-transform: uppercase; letter-spacing: .06em; color: var(--warn); border: var(--border) dashed var(--warn); border-radius: var(--radius-sm); }
  @media (max-width: 960px) { .wrap { padding: 12px 16px 40px; } }
</style>
