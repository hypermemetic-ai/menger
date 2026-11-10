<!-- V2-31: a person's usage page — #/usage (yours), #/usage/:principal (an admin's look at another): the app's top in its `top` section, the palette in its `palette` section (ui/slots.json), quota-usage between. SV-4: real Svelte (svelte/1), page-usage.rhx's port (ui/retired-rhx/).
     V2-68: above it, for an admin of `*` — `quota.table`'s roles (`auth.requires`) held on `*`
     (`auth.whoami`'s memberships), as app-top decides an editor — quota-total, the whole system's;
     anyone else's page places nothing and asks no `quota.table`.
     V2-70: below the person's own, quota-history — their consumption over time (the system's is in quota-total). -->
<script module>
  export const uses = ["auth.requires", "auth.whoami"];
</script>

<script>
  import { live } from "plexus-ui/host";
  import QuotaUsage from "plexus-ui:quota-usage";
  import QuotaTotal from "plexus-ui:quota-total";
  import QuotaHistory from "plexus-ui:quota-history";
  let { principal = null } = $props();

  // V2-68: who may see everyone's — quota.table's roles, one held on *.
  const needs = live({ snapshot: "auth.requires", params: { method: "quota.table" }, key: "method", on: {} });
  const door = live({ snapshot: "auth.whoami", params: {}, key: "principal", on: {} });
  const admin = $derived.by(() => {
    const roles = needs.rows.find((r) => r.method === "quota.table")?.roles ?? [];
    return (door.rows[0]?.memberships ?? []).some((m) => m.scope === "*" && roles.includes(m.role));
  });
</script>

<section class="page page-usage">
  <div data-slot="top" data-here="usage"></div>
  <header class="page-title"><h1>usage</h1></header>
  {#if admin}<QuotaTotal/>{/if}
  <QuotaUsage {principal}/>
  <QuotaHistory {principal}/>
  <div data-slot="palette"></div>
</section>

<style>
  .page-usage { padding-bottom: 40px; min-height: 100vh; min-height: 100dvh; color: var(--ink); background: var(--bg); font-family: var(--font-text); font-size: var(--text-size); }
  .page-title { max-width: 980px; margin: 0 auto; padding: 16px 24px 0; }
  .page-title h1 { margin: 0; font-family: var(--font-display); font-size: 20px; font-weight: var(--weight-strong); color: var(--ink); }
</style>
