<!-- SURFACE-3: the commits that finished a ticket (SDLC-9) — the ticket's own
     `commits`, live on the page's `get` spec (`commits_linked`,
     `commits_unlinked`); `commits link` from a field and `commits unlink`
     beside each, a refusal said under the form in the service's words
     (FRAME-37's `{ ok, events, message }`; until it lands, the call's events
     or its rejection). -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.commits.link", "tickets.commits.unlink"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { id } = $props();
  const b = bus();
  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket" } });
  const t = $derived(ticket.rows.find((r) => r.title != null));
  const commits = $derived(t?.commits ?? []);
  let commit = $state("");
  let refused = $state("");

  // FRAME-37's answer, or today's (before it lands, `ok` is undefined): the
  // events with an in-band error among them, or a rejection with its words.
  async function answer(call) {
    try {
      const a = await call;
      // FRAME-37: always resolves, `ok` false on a refusal or a drop, `message` its words (the answer is also the events' array).
      if (a && typeof a.ok === "boolean") return { ok: a.ok, events: a.events ?? [...a], message: String(a.message ?? "").replace(/^Invalid params: /, "") };
      const events = Array.isArray(a) ? a : [];
      const bad = events.find((e) => ["error", "refused", "not_found", "ambiguous", "frozen"].includes(e?.type));
      return bad ? { ok: false, events, message: bad.message ?? bad.reason ?? bad.type } : { ok: true, events };
    } catch (e) {
      return { ok: false, events: [], message: String(e?.message ?? e).replace(/^Invalid params: /, "") };
    }
  }

  async function link(ev) {
    ev.preventDefault();
    const c = commit.trim();
    const a = await answer(b.call("tickets.commits.link", { id, commits: [c] }));
    if (a.ok) { commit = ""; refused = ""; } else refused = a.message;
  }
  async function unlink(c) {
    const a = await answer(b.call("tickets.commits.unlink", { id, commits: [c] }));
    refused = a.ok ? "" : a.message;
  }
</script>

<section class="ticket-commits" data-surface="ticket-commits">
  <h2 class="sec">Commits <span class="n">{commits.length}</span></h2>
  <ul class="commits">
    {#each commits as c (c)}
      <li class="commit" data-id={c}>
        <code data-field="commit">{c}</code>
        <span class="repo">{c.includes("@") ? "in " + c.slice(0, c.indexOf("@")) : "menger"}</span>
        <button class="x" data-action="tickets.commits.unlink" data-commit={c} aria-label={"unlink " + c} title="unlink" onclick={() => unlink(c)}>×</button>
      </li>
    {/each}
    {#if t && commits.length === 0}<li class="empty" data-empty>no commit linked</li>{/if}
  </ul>
  <form class="add" data-form="tickets.commits.link" onsubmit={link}>
    <input class="in" name="commits" placeholder="hash or repo@hash" autocomplete="off" spellcheck="false" aria-invalid={refused ? "true" : "false"} bind:value={commit} oninput={() => (refused = "")}/>
    <button class="btn" data-action="tickets.commits.link" data-idle={commit === "" ? "true" : "false"}>link</button>
  </form>
  {#if refused}<p class="refused" data-refused role="alert">{refused}</p>{/if}
</section>

<style>
  .ticket-commits { color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .ticket-commits .sec { display: flex; gap: 7px; align-items: baseline; margin: 22px 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .ticket-commits .n { color: var(--ink); font-variant-numeric: tabular-nums; text-transform: none; letter-spacing: 0; }
  .ticket-commits .commits { list-style: none; margin: 0; padding: 0; border-top: var(--border) solid var(--line); }
  .ticket-commits .commit { display: grid; grid-template-columns: auto minmax(0, 1fr) auto; gap: 8px; align-items: center; min-height: var(--pitch); padding: 2px 4px 2px 6px; border-bottom: var(--border) solid var(--line); transition: background-color var(--motion-fast) var(--ease); }
  .ticket-commits .commit:hover { background: var(--hover); }
  .ticket-commits code { font-family: var(--font-text); font-size: 12px; color: var(--accent); }
  .ticket-commits .repo { font-size: 11px; color: var(--muted); }
  .ticket-commits .x { padding: 0 5px; font: inherit; font-size: 12px; line-height: 16px; color: var(--muted); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .ticket-commits .x:hover { color: var(--warn); background: var(--hover); }
  .ticket-commits .x:focus-visible { outline: 2px solid var(--focus); }
  .ticket-commits .empty { padding: 8px 6px; font-size: 12px; color: var(--muted); }
  .ticket-commits .add { display: flex; gap: 6px; margin: 8px 0 0; }
  .ticket-commits .in { flex: 1; min-width: 0; padding: 3px 8px; font: inherit; font-size: 12px; line-height: 18px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .ticket-commits .in::placeholder { color: var(--muted); }
  .ticket-commits .in:hover { border-color: var(--muted); }
  .ticket-commits .in:focus { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .ticket-commits .in[aria-invalid=true] { border-color: var(--warn); }
  .ticket-commits .btn { padding: 1px 10px; font: inherit; font-size: 12px; line-height: 20px; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease), border-color var(--motion-fast) var(--ease); }
  .ticket-commits .btn:hover { background: var(--hover); border-color: var(--muted); }
  .ticket-commits .btn:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .ticket-commits .btn[data-idle=true] { color: var(--disabled); }
  .ticket-commits .refused { margin: 5px 0 0; font-size: 11px; line-height: 1.4; color: var(--warn); overflow-wrap: anywhere; }
  .ticket-commits .refused::before { content: ""; display: inline-block; width: 6px; height: 6px; margin-right: 6px; vertical-align: 1px; border-radius: 50%; background: var(--warn); }
  @media (prefers-reduced-motion: reduce) {
    .ticket-commits .commit, .ticket-commits .in, .ticket-commits .btn { transition: none; }
  }
</style>
