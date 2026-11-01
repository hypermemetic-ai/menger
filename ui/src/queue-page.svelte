<!-- V2-20: the owner's queue, at #/queue — `tickets.queued`, most urgent
     first, then the oldest, as the service orders it; live by `tickets.watch`
     (`queued` upserts or moves a row, `snoozed` marks it, `unqueued` and
     `deleted` take it out; the head's count and the clock move with them).
     Each urgency is a band; a row says the ticket, why, how many
     questions it asks, how long it has waited, and until when it sleeps.
     Keys (through the bus's `keys`): `j` `k` move, `enter` opens, `s` snoozes
     an hour, `d` takes it out, `c` asks for changes (the row's form: its
     words go back to whoever queued it), `g b` the board. Past a page, `more` asks a
     longer one, to the service's 1,000; the head says how many there are.
     `sample` draws given rows, for the sheet: every urgency and a snooze. -->
<script module>
  export const uses = ["tickets.queued", "tickets.watch", "tickets.snooze", "tickets.unqueue", "tickets.request_changes"];
  export const BANDS = ["blocking", "now", "today", "whenever"];
  const STEP = 200, MOST = 1000;
  const ago = (from, now) => {
    const s = Math.max(0, now - from);
    if (s < 60) return "now";
    if (s < 3600) return `${Math.floor(s / 60)}m`;
    if (s < 86400) return `${Math.floor(s / 3600)}h`;
    return `${Math.floor(s / 86400)}d`;
  };
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { sheet = null, sample = null } = $props();
  const b = bus();
  const base = location.hash.startsWith("#/sv/") ? "#/sv/" : "#/";
  let limit = $state(STEP);
  const queue = live({ snapshot: "tickets.queued", params: () => ({ limit }), changes: "tickets.watch", key: "id",
                       on: { queue_row: "upsert:.", queued: "upsert:.", snoozed: "upsert:.", unqueued: "remove:id", deleted: "remove:ids", page: "upsert:.", queue_end: "upsert:." } });
  // A prop crosses a mark as text: the sheet's rows are JSON.
  const drawn = $derived(sample == null ? null : JSON.parse(sample));
  const rows = $derived((drawn ?? queue.rows.filter((r) => r.id != null)).slice().sort((x, y) =>
    BANDS.indexOf(x.entry.urgency) - BANDS.indexOf(y.entry.urgency) || x.entry.queued_unix - y.entry.queued_unix));
  const end = $derived(queue.rows.find((r) => r.count != null));
  const more = $derived(queue.rows.find((r) => r.id == null && "remaining" in r)?.remaining ?? 0);
  // The server's clock: the answer's, and every write heard since.
  const now = $derived(rows.reduce((at, r) => Math.max(at, r.entry.asked_unix ?? r.entry.queued_unix), end?.now_unix ?? 0));
  const count = $derived(rows.length + more);
  const asleep = $derived(rows.filter((r) => (r.entry.snoozed_until_unix ?? 0) > now).length);
  const loading = $derived(sheet === "loading" || (!drawn && queue.status === "pending"));
  let at = $state(0);
  const here = $derived(rows[Math.min(at, rows.length - 1)]);
  let refused = $state("");

  const said = (a) => { refused = a.ok ? "" : String(a.message ?? "refused").replace(/^Invalid params: /, ""); };
  const snooze = (id) => b.call("tickets.snooze", { id, minutes: 60 }).then(said);
  const done = (id) => b.call("tickets.unqueue", { id }).then(said);
  // The row whose change request is open, and its words.
  let asking = $state(null);
  let words = $state("");
  let form;
  function changes(id) {
    asking = id;
    words = "";
    queueMicrotask(() => form?.querySelector("textarea")?.focus());
  }
  function send(ev) {
    ev?.preventDefault?.();
    b.call("tickets.request_changes", { id: asking, body: words }).then((a) => {
      said(a);
      if (a.ok) asking = null;
    });
  }
  function typed(ev) {
    if (ev.key === "Enter" && ev.ctrlKey) send(ev);
    if (ev.key === "Escape") asking = null;
  }
  b.keys({
    j: { to: () => (at = Math.min(at + 1, rows.length - 1)), label: "next in the queue" },
    k: { to: () => (at = Math.max(at - 1, 0)), label: "previous in the queue" },
    enter: { to: () => here && b.navigate(base + "ticket/" + here.id), label: "open it" },
    s: { to: () => here && snooze(here.id), label: "snooze it an hour" },
    d: { to: () => here && done(here.id), label: "take it out of the queue" },
    c: { to: () => here && changes(here.id), label: "request changes" },
    "g b": { to: () => b.navigate(base), label: "the board" },
  });
  const questions = (e) => (e.inputs?.length ? e.inputs.length : e.input ? 1 : 0);
</script>

<section class="queue-page" data-surface="queue-page" data-state={queue.status}>
  <header class="q-head">
    <h1 class="q-title">queue</h1>
    <span class="q-count" data-field="count">{count}</span>
    {#if asleep}<span class="q-sleep" data-field="snoozed">{asleep} snoozed</span>{/if}
    <span class="q-keys" data-keys><kbd>j</kbd><kbd>k</kbd> move <kbd>enter</kbd> open <kbd>s</kbd> snooze <kbd>d</kbd> done <kbd>c</kbd> changes</span>
  </header>
  {#if refused}<p class="q-refused" data-refused role="alert">{refused}</p>{/if}
  <div class="q-rows" data-field="rows">
    {#if loading}
      <div data-loading aria-busy="true"><i class="bone"></i><i class="bone"></i><i class="bone"></i></div>
    {:else if rows.length === 0}
      <p class="q-empty" data-empty>nothing waits on you</p>
    {:else}
      {#each rows as r, i (r.id)}
        {#if i === 0 || rows[i - 1].entry.urgency !== r.entry.urgency}
          <h2 class="q-band" data-band={r.entry.urgency}>{r.entry.urgency}</h2>
        {/if}
        <div class="q-row" data-id={r.id} data-now={now} data-urgency={r.entry.urgency} data-selected={i === at ? "true" : "false"}
             data-snoozed={(r.entry.snoozed_until_unix ?? 0) > now ? "true" : "false"}>
          <i class="q-mark" aria-hidden="true"></i>
          <a class="q-id" href={base + "ticket/" + r.id}>{r.id}</a>
          <span class="q-what">
            <span class="q-name" data-field="title">{r.ticket.title}</span>
            <span class="q-why" data-field="reason">{r.entry.reason}</span>
          </span>
          {#if questions(r.entry)}<span class="q-asks" data-field="asks">{questions(r.entry)} asked</span>{/if}
          <span class="q-age" data-field="age">{ago(r.entry.queued_unix, now)}</span>
          <button class="q-act" data-action="tickets.snooze" onclick={() => snooze(r.id)}>snooze</button>
          <button class="q-act" data-action="tickets.unqueue" onclick={() => done(r.id)}>done</button>
          <button class="q-act" data-action="tickets.request_changes" onclick={() => changes(r.id)}>changes</button>
        </div>
        {#if asking === r.id}
          <form class="q-changes" data-form="tickets.request_changes" bind:this={form} onsubmit={send}>
            <textarea class="q-in" name="body" rows="2" spellcheck="false" placeholder="what to change" aria-label="what to change" bind:value={words} onkeydown={typed}></textarea>
            <button class="q-act" data-idle={words.trim() === "" ? "true" : "false"}>send</button>
          </form>
        {/if}
      {/each}
      {#if more > 0 && limit < MOST}
        <button class="q-more" data-action="more" onclick={() => (limit = Math.min(limit + STEP, MOST))}>{more} more</button>
      {/if}
    {/if}
  </div>
</section>

<style>
  .queue-page { display: flex; flex-direction: column; min-height: 0; flex: 1; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .queue-page .q-head { flex: none; display: flex; gap: 10px; align-items: baseline; padding: 8px 14px; background: var(--panel); border-bottom: var(--border) solid var(--line); }
  .queue-page .q-title { margin: 0; font-family: var(--font-display); font-size: 16px; font-weight: var(--weight-strong); }
  .queue-page .q-count { font-variant-numeric: tabular-nums; color: var(--muted); }
  .queue-page .q-sleep { color: var(--muted); font-size: 11px; }
  .queue-page .q-keys { margin-left: auto; display: flex; gap: 5px; align-items: center; font-size: 11px; color: var(--muted); }
  .queue-page kbd { display: inline-block; min-width: 16px; padding: 0 4px; font: 10px/15px var(--font-text); text-align: center; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-bottom-width: 2px; border-radius: var(--radius-sm); }
  .queue-page .q-refused { margin: 0; padding: 6px 14px; color: var(--warn); }
  .queue-page .q-rows { flex: 1; min-height: 0; overflow: auto; }
  .queue-page .q-band { position: sticky; top: 0; z-index: 1; margin: 0; padding: 6px 14px 4px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; background: var(--bg); border-bottom: var(--border) solid var(--line); }
  .queue-page .q-band[data-band=blocking], .queue-page .q-band[data-band=now] { color: var(--warn); }
  .queue-page .q-changes { display: flex; gap: 6px; align-items: flex-start; padding: 6px 14px 6px 98px; border-bottom: var(--border) solid var(--line); background: var(--panel); }
  .queue-page .q-in { flex: 1; min-width: 0; padding: 2px 6px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius-sm); resize: vertical; }
  .queue-page .q-act[data-idle=true] { opacity: .5; }
  .queue-page .q-row { display: grid; grid-template-columns: 4px 84px minmax(0, 1fr) auto 32px auto auto auto; gap: 10px; align-items: center; min-height: var(--pitch); padding: 3px 14px 3px 0; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .queue-page .q-row:hover, .queue-page .q-row:global([data-sheet~=hover]) { background: var(--hover); }
  .queue-page .q-row[data-selected=true], .queue-page .q-row:global([data-sheet~=selected]) { background: var(--sel); }
  .queue-page .q-row[data-snoozed=true] .q-what { opacity: .55; }
  .queue-page .q-mark { align-self: stretch; background: var(--line); }
  .queue-page .q-row[data-urgency=blocking] .q-mark, .queue-page .q-row[data-urgency=now] .q-mark { background: var(--warn); }
  .queue-page .q-row[data-urgency=today] .q-mark { background: var(--accent); }
  .queue-page .q-id { color: var(--ink); text-decoration: none; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .queue-page .q-id:focus-visible, .queue-page .q-id:global([data-sheet~=focus]) { outline: var(--border) solid var(--accent); outline-offset: 2px; }
  .queue-page .q-what { display: grid; min-width: 0; }
  .queue-page .q-name, .queue-page .q-why { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .queue-page .q-why { color: var(--muted); font-size: 11px; }
  .queue-page .q-asks, .queue-page .q-age { color: var(--muted); font-size: 11px; font-variant-numeric: tabular-nums; white-space: nowrap; }
  .queue-page .q-act, .queue-page .q-more { padding: 0 6px; font: inherit; font-size: 11px; color: var(--ink); background: none; border: var(--border) solid var(--line); border-radius: var(--radius-sm); cursor: pointer; }
  .queue-page .q-more { display: block; margin: 8px 14px; }
  .queue-page .q-empty { padding: 14px; color: var(--muted); }
  .queue-page .bone { display: block; height: 10px; margin: 12px 14px; background: var(--skeleton); border-radius: var(--radius-sm); }
  @media (max-width: 720px) {
    .queue-page .q-keys, .queue-page .q-asks { display: none; }
    .queue-page .q-row { grid-template-columns: 4px 72px minmax(0, 1fr) auto auto auto; padding-right: 10px; }
    .queue-page .q-changes { padding-left: 14px; }
    .queue-page .q-age { display: none; }
  }
</style>
