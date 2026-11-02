<!-- V2-31: a person's usage (#/usage), svelte/1 — `quota.usage`, one
     `usage { principal, exempt, bytes, sources }`: a bar per source, used
     (and in flight) against the cap — the cap's source said beside it (own,
     a team's, the default) — or, for an unlimited one, the amount alone. A
     day's counter says so. What the person stores is the sum on top.
     Marks: `[data-surface=quota-usage]`, a row per source `[data-id=<source>]`
     with `[data-field=used|cap|from]` and its bar `[data-bar]` (`data-fill`,
     0–100; `data-state=ok|near|over`), `[data-field=stored]`, `[data-exempt]`,
     a refusal `[data-refused]`. `principal`, for an admin's look at another. -->
<script module>
  export const uses = ["quota.usage"];
</script>

<script>
  import { live } from "plexus-ui/host";
  let { principal = null, sheet = null } = $props();

  const usage = live({ snapshot: "quota.usage", params: principal ? { principal } : {}, key: "principal", on: {} });
  const me = $derived(usage.rows.find((r) => r.type === "usage" || r.sources) ?? null);
  const NAMES = {
    "media.bytes": "files kept", "media.fetch.bytes": "fetched today", "media.fetch.calls": "fetches today",
    "media.egress.bytes": "served today", "docs.bytes": "documents", "docs.count": "documents made",
    "tickets.bytes": "tickets", "tickets.count": "tickets made", "questions.count": "questions asked",
  };
  /** An amount in words: bytes as 1.9 GB (powers of 1000, as the caps are set), counts as the number. */
  function amount(unit, n) {
    if (n == null) return "unlimited";
    if (unit !== "bytes") return Number(n).toLocaleString("en");
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = Number(n), u = 0;
    while (v >= 1000 && u < units.length - 1) { v /= 1000; u += 1; }
    return u === 0 ? `${n} B` : `${(Math.round(v * 10) / 10).toString()} ${units[u]}`;
  }
  const fill = (s) => (s.cap == null ? 0 : s.cap === 0 ? 100 : Math.min(100, Math.round(((s.used + s.held) / s.cap) * 100)));
  const state = (s) => (s.cap == null ? "ok" : s.used + s.held >= s.cap ? "over" : fill(s) >= 80 ? "near" : "ok");
  const from = (s) => (s.from === "own" ? "yours" : s.from === "default" ? "the default" : s.from === "exempt" ? "admin: no cap" : s.from.replace(/^team:/, "team "));
</script>

<section class="quota-usage" data-surface="quota-usage">
  <div class="page-head">
    <span class="sub">{principal ? principal : "what you keep and send, against your allowance"}</span>
    {#if usage.status !== "live"}<span class="state" data-loading={usage.status}>{usage.status}</span>{/if}
  </div>
  {#if me}
    <p class="stored">You store <b data-field="stored">{amount("bytes", me.bytes)}</b>{#if me.exempt}<span class="exempt" data-exempt="">admin of everything: counted, never capped</span>{/if}</p>
    <div class="panel">
      {#each me.sources as s (s.source)}
        <div class="tr" data-id={s.source} data-window={s.window}>
          <span class="name">{NAMES[s.source] ?? s.source}<code>{s.source}</code></span>
          <span class="bar" data-bar="" data-fill={fill(s)} data-state={state(s)} aria-hidden={s.cap == null ? "true" : undefined}>
            {#if s.cap != null}<i style={`width: ${fill(s)}%`}></i>{/if}
          </span>
          <span class="nums"><b data-field="used">{amount(s.unit, s.used)}</b>{#if s.held > 0}<span class="held" data-field="held">+{amount(s.unit, s.held)} in flight</span>{/if} of <b data-field="cap">{amount(s.unit, s.cap)}</b>{#if s.window === "day"}<span class="day">a day</span>{/if}</span>
          <span class="from" data-field="from">{from(s)}</span>
        </div>
      {/each}
    </div>
  {:else if sheet === "loading" || usage.status === "pending"}
    <div class="panel"><div class="tr"><span data-loading="">media.bytes</span></div></div>
  {/if}
  {#if usage.status === "error" || usage.status === "refused"}<p class="empty" data-refused="">refused: {usage.message}</p>{/if}
</section>

<style>
  .quota-usage { max-width: 980px; margin: 0 auto; padding: 18px 24px 40px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .page-head { display: flex; align-items: baseline; gap: 10px; margin: 0 0 10px; }
  .sub { color: var(--muted); font-size: 12px; overflow-wrap: anywhere; }
  .state { margin-left: auto; font-size: 11px; color: var(--muted); }
  .stored { margin: 0 0 14px; color: var(--muted); }
  .stored b { color: var(--ink); font-variant-numeric: tabular-nums; }
  .exempt { margin-left: 10px; padding: 0 8px; font-size: 11px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .panel { overflow: hidden; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .tr { display: grid; grid-template-columns: minmax(150px, 1.1fr) minmax(120px, 2fr) minmax(170px, 1.3fr) 110px; gap: 0 14px; align-items: center; min-height: 40px; padding: 6px 12px; border-bottom: var(--border) solid var(--line); }
  .tr:last-child { border-bottom: 0; }
  .name { display: flex; flex-direction: column; font-weight: var(--weight-medium); }
  .name code { font-size: 11px; font-weight: normal; color: var(--muted); font-family: var(--font-text); }
  .bar { position: relative; height: 8px; background: var(--hover); border-radius: var(--radius-pill); overflow: hidden; }
  .bar[aria-hidden] { background: repeating-linear-gradient(90deg, var(--line) 0 6px, transparent 6px 10px); opacity: 0.6; }
  .bar i { position: absolute; inset: 0 auto 0 0; background: var(--accent); border-radius: var(--radius-pill); transition: width var(--motion-fast) var(--ease); }
  .bar[data-state=near] i { background: var(--warn); }
  .bar[data-state=over] i { background: var(--warn); }
  .nums { color: var(--muted); font-size: 12px; font-variant-numeric: tabular-nums; }
  .nums b { color: var(--ink); font-weight: var(--weight-medium); }
  .held { color: var(--accent); }
  .day, .held { margin-left: 0.3em; }
  .from { color: var(--muted); font-size: 11px; text-align: right; }
  [data-loading] { color: transparent; background: var(--skeleton); border-radius: var(--radius-sm); }
  .empty { margin: 8px 10px; color: var(--muted); }
  [data-refused] { color: var(--warn); }
  @media (prefers-reduced-motion: reduce) { .bar i { transition: none; } }
  @media (max-width: 720px) { .tr { grid-template-columns: 1fr auto; } .bar { grid-column: 1 / -1; grid-row: 2; } .from { display: none; } }
</style>
