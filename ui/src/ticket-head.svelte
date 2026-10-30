<!-- SURFACE-3: a ticket's head — its id, kind mark, kind, severity, address,
     the frozen / archived / hidden badges, the title, the status chip (the
     board's look), labels and the parent chip; live on the page's `get` spec
     (the same spec in every part reading the ticket: one snapshot, one
     stream); the archived badge from the archive's transitions (the spec
     ticket-history reads). ‹ › and `[` / `]` move to
     the previous / next sibling — the parent's children (`list { parent,
     fields: [id] }`), asked once the parent is known. The status menu stays
     until SURFACE-4's ticket-actions carries set_status (the board's ticket
     case selects it). -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.list", "tickets.set_status", "tickets.history"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";
  import Chip from "./ticket-chip-sv.svelte";

  let { id } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  const STATUSES = ["Draft", "Pending", "Ready", "Coding", "In Review", "Complete", "Blocked", "Superseded", "Canceled"];

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  // The archive's moves are transitions: the spec ticket-history reads, one collection.
  const history = live({ snapshot: "tickets.history", params: () => ({ id }), changes: "tickets.watch", key: "transition", on: { transition: "upsert:." } });
  const t = $derived(ticket.rows.find((r) => r.title != null));
  const parent = $derived(t?.parent ?? null);
  const siblings = live({ snapshot: "tickets.list", params: () => ({ parent, summary: true, limit: 1000, fields: ["id"] }), key: "id", when: () => parent != null });
  const sibs = $derived(parent == null ? [] : siblings.rows.map((r) => r.id).filter((x) => !x.includes(".")));
  const at = $derived(sibs.indexOf(id));
  const prev = $derived(at > 0 ? sibs[at - 1] : null);
  const next = $derived(at >= 0 && at + 1 < sibs.length ? sibs[at + 1] : null);
  // The last archive transition of this ticket, else get's mark.
  const moves = $derived(history.rows.map((r) => r.transition).filter((x) => x && x.ticket === id && x.field === "archive").sort((a, z) => a.id - z.id));
  const archived = $derived(moves.length ? moves.at(-1).to === "archived" : t?.archived != null);

  b.keys({
    "[": { to: () => prev && b.navigate(base + "ticket/" + prev), label: "previous sibling" },
    "]": { to: () => next && b.navigate(base + "ticket/" + next), label: "next sibling" },
  });

  function setStatus(ev) {
    const status = ev.currentTarget.value;
    // FRAME-37: the answer's `ok` (undefined before it lands); the chip moves on the watch.
    if (status !== "") b.call("tickets.set_status", { id, status }).then((r) => r?.ok === false && console.warn(`set_status: ${r.message}`), (e) => console.error(`set_status: ${e?.message ?? e}`));
  }
</script>

<header class="ticket-head" data-surface="ticket-head">
  {#if t}
    <div class="top">
      <i class="kmark" data-kind={t.kind} aria-hidden="true"></i>
      <span class="code" data-field="id" data-ticket-id>{t.id}</span>
      <span data-field="kind" data-ticket-kind>{t.kind}</span>
      <span class="sev" data-field="severity" data-severity={t.severity ?? "none"}>
        <span class="bars" aria-hidden="true"><i></i><i></i><i></i></span>{t.severity ? t.severity.toLowerCase() : "no severity"}
      </span>
      <span class="address" data-field="address" title="organization : epic : shortcode">{t.address}</span>
      {#if t.frozen}<span class="badge" data-badge="frozen">frozen</span>{/if}
      {#if archived}<span class="badge" data-badge="archived">archived</span>{/if}
      {#if t.hidden}<span class="badge" data-badge="hidden">hidden</span>{/if}
      {#if prev || next}
        <span class="sib">
          {#if prev}<a href={base + "ticket/" + prev} data-sib="prev">‹ {prev}</a>{/if}
          {#if next}<a href={base + "ticket/" + next} data-sib="next">{next} ›</a>{/if}
        </span>
      {/if}
    </div>
    <h1 class="title" data-field="title" data-ticket-title>{t.title}</h1>
    <div class="meta">
      <span class="status" data-field="status" data-status={t.status} data-ticket-status>{t.status}</span>
      <span class="labels" data-field="labels">
        {#each t.labels ?? [] as l (l)}<span class="label" data-label={l}>{l}</span>{/each}
      </span>
      <span data-field="parent">
        {#if t.parent != null}<Chip id={t.parent} kind="parent"/>{:else}<span class="none">a root</span>{/if}
      </span>
      <label class="status-menu">set status
        <select data-status-menu onchange={setStatus}>
          <option value="">…</option>
          {#each STATUSES as s}<option value={s}>{s}</option>{/each}
        </select>
      </label>
    </div>
  {/if}
</header>

<style>
  .ticket-head { padding-bottom: 12px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .ticket-head .top { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; font-size: 12px; color: var(--muted); }
  .ticket-head .code { color: var(--accent); font-weight: var(--weight-medium); }
  .ticket-head .address { color: var(--muted); }
  .ticket-head .address::before { content: "·"; margin-right: 8px; }
  .ticket-head .title { margin: 6px 0 10px; font-family: var(--font-display); font-size: 24px; line-height: 1.25; font-weight: var(--weight-strong); overflow-wrap: anywhere; }
  .ticket-head .meta { display: flex; flex-wrap: wrap; gap: 8px; align-items: center; font-size: 12px; color: var(--muted); }
  .ticket-head .status { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .ticket-head .status[data-status=Pending] { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .ticket-head .status[data-status=Ready] { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .ticket-head .status[data-status=Coding] { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .ticket-head .status[data-status="In Review"] { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .ticket-head .status[data-status=Complete] { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .ticket-head .status[data-status=Blocked] { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .ticket-head .status[data-status=Superseded] { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .ticket-head .status[data-status=Canceled] { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .ticket-head .kmark { display: inline-block; width: 8px; height: 8px; flex: none; background: var(--kind-build); border-radius: 50%; }
  .ticket-head .kmark[data-kind=scope] { background: var(--kind-scope); border-radius: var(--radius-xs); transform: rotate(45deg) scale(.85); }
  .ticket-head .kmark[data-kind=gap] { background: var(--kind-gap); border-radius: 0; clip-path: polygon(50% 0, 100% 100%, 0 100%); }
  .ticket-head .kmark[data-kind=spike] { background: none; border: 2px solid var(--kind-spike); }
  .ticket-head .kmark[data-kind=execution] { background: var(--kind-execution); border-radius: var(--radius-xs); }
  .ticket-head .kmark[data-kind=doc] { width: 7px; height: 9px; background: none; border: var(--border) solid var(--muted); border-radius: var(--radius-xs); }
  .ticket-head .sev { display: inline-flex; gap: 5px; align-items: center; }
  .ticket-head .bars { display: inline-flex; gap: 2px; align-items: flex-end; width: 14px; height: 12px; }
  .ticket-head .bars i { display: block; width: 3px; background: var(--line); border-radius: var(--radius-xs); }
  .ticket-head .bars i:nth-child(1) { height: 5px; }
  .ticket-head .bars i:nth-child(2) { height: 8px; }
  .ticket-head .bars i:nth-child(3) { height: 11px; }
  .ticket-head .sev[data-severity=Low] .bars i:nth-child(1) { background: var(--sev-low); }
  .ticket-head .sev[data-severity=Medium] .bars i:nth-child(-n+2) { background: var(--sev-medium); }
  .ticket-head .sev[data-severity=High] .bars i { background: var(--sev-high); }
  .ticket-head .sev[data-severity=Critical] .bars i { background: var(--sev-critical); }
  .ticket-head .badge { padding: 0 6px; font-size: 10px; line-height: 16px; text-transform: uppercase; letter-spacing: .06em; color: var(--muted); border: var(--border) dashed var(--muted); border-radius: var(--radius-sm); }
  .ticket-head .sib { display: flex; gap: 10px; margin-left: auto; }
  .ticket-head .sib a { color: var(--muted); text-decoration: none; border-radius: var(--radius-sm); transition: color var(--motion-fast) var(--ease); }
  .ticket-head .sib a:hover { color: var(--accent); }
  .ticket-head .sib a:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .ticket-head .labels { display: inline-flex; flex-wrap: wrap; gap: 5px; }
  .ticket-head .label { display: inline-flex; align-items: center; padding: 0 8px; font-size: 11px; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .ticket-head .none { font-size: 11px; color: var(--disabled); }
  .ticket-head .status-menu { display: flex; gap: 6px; align-items: center; margin-left: auto; font-size: 11px; }
  .ticket-head .status-menu select { padding: 1px 4px; color: var(--ink); font: inherit; background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .ticket-head .status-menu select:hover { border-color: var(--muted); }
  .ticket-head .status-menu select:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  @media (prefers-reduced-motion: reduce) {
    .ticket-head .sib a, .ticket-head .status-menu select { transition: none; }
  }
</style>
