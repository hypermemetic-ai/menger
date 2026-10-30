<!-- SURFACE-4: what can be done to a ticket (svelte/1), placed in the ticket
     page by <ticket-actions id={id}/>, as the designer drew it in the side
     column: the lifecycle's moves from the status (set_status; the next steps
     first, every other move the store allows under "more…"; Canceled and
     Superseded ask why; leaving Canceled may restore what it canceled),
     labels (a chip with ×, a field to add), archive (a dry run first) and
     restore, rename (a dry run first), a revision put back (the rev from
     `ticket.revision_picked` on the bus, or typed), and the admin's
     freeze/unfreeze, reshortcode (a dry run first), delete (what its children
     and waiters do) and undelete. Each action is [data-action=<method>] with
     a message of its own; its answer is said beside it, a refusal in the
     store's words ([data-refused]); nothing is asked twice while a write is
     pending. An action the caller's roles on this ticket do not hold — the
     grants of members.mine that cover it — is drawn disabled, naming the
     roles `auth requires` says it needs. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "members.mine", "auth.requires",
    "tickets.set_status", "tickets.label", "tickets.archive", "tickets.restore", "tickets.rename", "tickets.revisions.restore",
    "tickets.freeze", "tickets.unfreeze", "tickets.reshortcode", "tickets.delete", "tickets.undelete"];
  // The designer's next steps from each status; the store allows any move but
  // Complete over open children and nothing on a doc, so the rest are under "more…".
  const LIFE = {
    Draft: ["Pending", "Superseded", "Canceled"],
    Pending: ["Ready", "Draft", "Superseded", "Canceled"],
    Ready: ["Coding", "Pending", "Blocked", "Superseded", "Canceled"],
    Coding: ["In Review", "Ready", "Blocked", "Superseded", "Canceled"],
    "In Review": ["Complete", "Coding", "Superseded", "Canceled"],
    Blocked: ["Ready", "Pending", "Superseded", "Canceled"],
    Epic: ["Complete", "Superseded", "Canceled"],
    Complete: ["Coding"],
    Superseded: ["Pending"],
    Canceled: ["Pending"],
  };
  const ALL = ["Draft", "Pending", "Ready", "Coding", "In Review", "Blocked", "Complete", "Epic", "Superseded", "Canceled"];
  const FIELDS = ["title", "body", "status", "kind", "severity", "parent", "blocked_by", "labels"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { id, sheet = null } = $props();
  const b = bus();

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", updated: "upsert:ticket",
                              labels_changed: "upsert:ticket", severity_changed: "upsert:ticket", frozen_changed: "upsert:ticket", hidden_changed: "upsert:ticket",
                              blockers_changed: "upsert:ticket", unlocks_changed: "upsert:ticket", unlocks_removed: "upsert:ticket", parent_changed: "upsert:ticket",
                              commits_linked: "upsert:ticket", commits_unlinked: "upsert:ticket", revision_restored: "upsert:ticket", deleted: "upsert:." } });
  // ticket-head's spec word for word: the page holds one `get` and one `watch` of the ticket, shared.
  // A delete is folded whole (`deleted { ids, tombstones }`); a ticket deleted before the page opened answers so too.
  const t = $derived(ticket.rows.find((r) => r.title != null));
  const tombed = $derived(ticket.rows.some((r) => Array.isArray(r.tombstones) && r.tombstones.some((x) => x.id === id)));
  // A delete asked here is known at its answer (the page itself draws a delete from elsewhere, and mounts
  // this component in its place, whose `get` answers `deleted`).
  let deletedHere = $state(false);
  let undone = $state(false); // undeleted here since
  const gone = $derived((tombed || deletedHere) && !undone);
  // An archive or a restore carries no ticket on the watch: after one asked here, `get` is asked again for its mark.
  let fresh = $state(null);
  async function regain() {
    const r = await b.call("tickets.get", { id });
    fresh = r.ok ? (r.events.find((e) => e?.type === "ticket")?.ticket ?? null) : null;
  }
  const archived = $derived(fresh?.id === id ? fresh.archived != null : t?.archived != null);

  // Who may: the grants that cover this ticket, and the roles each class of action needs.
  const mine = live({ snapshot: "members.mine", params: {}, key: "principal", on: { me: "upsert:.", grant: "upsert:grant" } });
  const needs = (method) => live({ snapshot: "auth.requires", params: { method }, key: "method" });
  const need = { edit: needs("tickets.set_status"), freeze: needs("tickets.freeze"), reshortcode: needs("tickets.reshortcode"), delete: needs("tickets.delete"), undelete: needs("tickets.undelete") };
  const me = $derived(mine.rows.find((r) => "admin_everywhere" in r));
  const grants = $derived(mine.rows.filter((r) => r.role && r.scope));
  // A grant `under X` covers the ticket when X is it or an ancestor: the chain asked once, when such a grant is held.
  let ancestors = $state([]);
  $effect(() => {
    if (!t || !grants.some((g) => g.scope.startsWith("under "))) return;
    let stop = false;
    (async () => {
      const chain = [];
      let at = t.parent;
      while (at && chain.length < 32 && !stop) {
        chain.push(at);
        const r = await b.call("tickets.get", { id: at });
        at = r.ok ? r.events.find((e) => e?.type === "ticket")?.ticket?.parent : null;
      }
      if (!stop) ancestors = chain;
    })();
    return () => (stop = true);
  });
  function covers(scope, x) {
    if (scope === "*") return true;
    const org = (x.address ?? "").split(":")[0];
    if (scope === org + ":" || scope === "org:" + org) return true;
    if (scope === x.shortcode || scope === x.epic || scope === `${org}:${x.shortcode}` || scope === x.id) return true;
    const under = /^under\s+(.+)$/.exec(scope)?.[1];
    return !!under && (under === x.id || ancestors.includes(under));
  }
  const held = $derived(me?.admin_everywhere ? ["admin"] : t ? [...new Set(grants.filter((g) => covers(g.scope, t)).map((g) => g.role))] : []);
  /** The roles `auth requires` names for a class, when the caller holds none of them here; else null. */
  function lacks(store) {
    const roles = store?.rows?.[0]?.roles;
    if (!me || !roles) return null;
    return held.some((r) => roles.includes(r)) ? null : roles;
  }
  const lack = $derived({ edit: lacks(need.edit), freeze: lacks(need.freeze), reshortcode: lacks(need.reshortcode), delete: lacks(need.delete), undelete: lacks(need.undelete) });
  const why = (roles) => `${roles.join(" or ")} · you hold ${held.join(", ") || "nothing"} here`;

  // A message per action.
  let msgs = $state({ status: "", archive: "", rename: "", reshortcode: "", delete: "", revision: "" });
  let ask = $state(""); // Canceled or Superseded, waiting for its why
  let moreMoves = $state(false);
  let restoreChildren = $state(false);
  let refusedOn = $state("");
  let label = $state("");
  let withChildren = $state(true);
  let dry = $state({ archive: null, rename: null, reshortcode: null }); // null, or { text } while confirming
  let renameTo = $state("");
  let rcOpen = $state(false);
  let rcTo = $state("");
  let dlOpen = $state(false);
  let dlChildren = $state("reparent");
  let dlUnblock = $state(true);
  let rev = $state("");
  let revFields = $state([]);
  let answers = $state({});
  let pending = $state(0);
  const busy = $derived(pending > 0);

  b.on("ticket.revision_picked", (data) => {
    if (data?.rev != null) {
      rev = String(data.rev);
      answers["tickets.revisions.restore"] = undefined;
    }
  });

  /** A write asked (`b.call("…", params)`, its method as written): FRAME-37's answer —
   *  `{ ok, events, message }`, the events array carrying the three. */
  async function settle(asked) {
    pending += 1;
    try {
      const r = await asked;
      return r && "ok" in r ? { ok: r.ok, events: r.events ?? r, message: r.message } : { ok: false, events: [], message: "no answer" };
    } finally {
      pending -= 1;
    }
  }
  /** Its answer said under `key` — `said(event)` or the refusal's words. */
  async function write(key, asked, said) {
    answers[key] = { asking: true };
    const r = await settle(asked);
    answers[key] = r.ok ? { said: said ? said(r.events[0] ?? {}) : "" } : { refused: r.message ?? "refused" };
    return r;
  }
  const withWhy = (key, p) => (msgs[key].trim() ? { ...p, message: msgs[key].trim() } : p);
  const moves = (e) => (e.moves ?? []).map((m) => `${m.from} → ${m.to}`).join(", ");

  // The lifecycle.
  async function move(to) {
    if (busy) return;
    if ((to === "Canceled" || to === "Superseded") && ask !== to) {
      ask = to;
      return;
    }
    if (ask === to && !msgs.status.trim()) return;
    ask = "";
    refusedOn = to;
    const p = withWhy("status", { id, status: to, source: "app" });
    if (t?.status === "Canceled" && restoreChildren) p.restore_children = true;
    const r = await write("tickets.set_status", b.call("tickets.set_status", p),
      (e) => `${e.previous} → ${e.ticket?.status}${p.message ? ` · “${p.message}”` : ""}`);
    if (r.ok) {
      refusedOn = "";
      msgs.status = "";
      restoreChildren = false;
    }
  }

  // Labels.
  function labelKeyed(ev) {
    if (ev.key !== "Enter") return;
    ev.preventDefault();
    const l = label.trim();
    if (!l || busy) return;
    label = "";
    write("tickets.label", b.call("tickets.label", { id, add: [l] }));
  }
  const unlabel = (l) => !busy && write("tickets.label", b.call("tickets.label", { id, remove: [l] }));

  // The archive.
  async function dryArchive() {
    if (busy) return;
    dry.archive = { text: `archives ${id} with its whole subtree` };
    answers["tickets.archive"] = undefined;
    const r = await settle(b.call("tickets.archive", { ids: [id], dry_run: true }));
    if (!r.ok) {
      dry.archive = null;
      answers["tickets.archive"] = { refused: r.message };
      return;
    }
    const e = r.events[0] ?? {};
    if (dry.archive) dry.archive = { text: `archives ${(e.ids ?? []).join(", ")} · ${(e.ids ?? []).length} out of every view${e.live?.length ? `, ${e.live.length} still live` : ""}` };
  }
  async function archive() {
    if (busy) return;
    dry.archive = null;
    const r = await write("tickets.archive", b.call("tickets.archive", withWhy("archive", { ids: [id] })), (e) => `archived ${(e.ids ?? []).join(", ")}`);
    if (r.ok) msgs.archive = "";
    await regain();
  }
  async function restore() {
    if (busy) return;
    const r = await write("tickets.archive", b.call("tickets.restore", withWhy("archive", { ids: [id], with_children: withChildren })),
      (e) => `restored ${(e.ids ?? []).join(", ")}${e.still_archived?.length ? ` · still archived: ${e.still_archived.join(", ")}` : ""}`);
    if (r.ok) msgs.archive = "";
    await regain();
  }

  // Rename and reshortcode: a dry run, then the move.
  async function dryRun(kind, method, asked) {
    dry[kind] = { text: "…" };
    answers[method] = undefined;
    const r = await settle(asked);
    if (!r.ok) {
      dry[kind] = null;
      answers[method] = { refused: r.message };
    } else if (dry[kind]) dry[kind] = { text: `would move ${moves(r.events[0] ?? {})}` };
  }
  async function moveTo(kind, method, asked) {
    dry[kind] = null;
    const r = await write(method, asked, (e) => `moved ${moves(e)}`);
    if (r.ok) msgs[kind] = "";
    const to = r.ok && r.events[0]?.moves?.[0]?.to;
    if (to) b.navigate("#/ticket/" + to);
  }
  const renameP = (dryRun) => withWhy("rename", { id, to: renameTo.trim(), ...(dryRun ? { dry_run: true } : {}) });
  const reshortP = (dryRun) => withWhy("reshortcode", { ids: [id], to: rcTo.trim(), ...(dryRun ? { dry_run: true } : {}) });

  // A revision put back.
  function toggleField(f, on) {
    revFields = on ? [...revFields.filter((x) => x !== f), f] : revFields.filter((x) => x !== f);
  }
  async function restoreRevision() {
    const n = Number(rev);
    if (!(n > 0) || busy) return;
    const p = withWhy("revision", { id, rev: n, source: "app" });
    if (revFields.length) p.fields = revFields;
    const r = await write("tickets.revisions.restore", b.call("tickets.revisions.restore", p),
      (e) => (e.changed?.length ? `from rev ${e.from_rev}: ${e.changed.join(", ")}` : `nothing differs from rev ${e.from_rev}`));
    if (r.ok) msgs.revision = "";
  }

  // The admin's.
  const freeze = (on) => !busy && write("tickets.freeze", on ? b.call("tickets.freeze", { id }) : b.call("tickets.unfreeze", { id }), () => (on ? "frozen" : "growing again"));
  async function del() {
    if (busy) return;
    dlOpen = false;
    answers["tickets.undelete"] = undefined;
    const p = withWhy("delete", { ids: [id], unblock: dlUnblock });
    if (dlChildren) p.children = dlChildren;
    const r = await write("tickets.delete", b.call("tickets.delete", p), (e) => `deleted ${(e.ids ?? []).join(", ")} · undelete within 30 days`);
    if (r.ok) {
      msgs.delete = "";
      deletedHere = true;
      undone = false;
    }
  }
  async function undelete() {
    if (busy) return;
    const r = await write("tickets.undelete", b.call("tickets.undelete", { ids: [id] }), (e) => `back: ${(e.ids ?? []).join(", ")}`);
    if (r.ok) {
      undone = true;
      deletedHere = false;
    }
  }
</script>

{#snippet answer(method)}
  {#if answers[method]?.refused}<p class="refused" data-refused data-for={method}>{answers[method].refused}</p>{/if}
  {#if answers[method]?.said}<p class="landed" data-answer data-for={method}>{answers[method].said}</p>{/if}
  {#if answers[method]?.asking}<p class="hint" data-loading data-for={method}>…</p>{/if}
{/snippet}
{#snippet needing(roles, method)}
  <span class="why" data-needs={method}>{why(roles)}</span>
{/snippet}
{#snippet note(key)}
  <input class="in" data-field="message" placeholder="a message, if any" autocomplete="off" bind:value={msgs[key]}/>
{/snippet}
{#snippet moveButton(s)}
  <button type="button" class="btn" data-action="tickets.set_status" data-to={s} disabled={!!lack.edit || busy}
          data-refused-on={answers["tickets.set_status"]?.refused && refusedOn === s ? "true" : "false"}
          aria-pressed={ask === s} onclick={() => move(s)}>{s}{s === "Superseded" || s === "Canceled" ? "…" : ""}</button>
{/snippet}

<section class="ticket-actions" data-surface="ticket-actions" data-ticket={id}>
  {#if sheet === "loading" || ticket.status === "pending"}
    <div class="skel" data-loading><span class="bar"></span><span class="bar short"></span><span class="bar"></span></div>
  {/if}
  {#if t && !gone}
    <div data-group="status">
      <h2 class="sec">Status
        {#if me}<span class="aside ta-who" data-field="who">as <b>{me.name ?? me.principal}</b> · {me.admin_everywhere ? "admin" : held.join(", ") || "nothing"} here</span>{/if}
      </h2>
      <div class="ta-row" data-field="status-moves">
        <span class="tag status" data-status={t.status} data-field="status">{t.status}</span>
        {#if t.kind === "doc"}
          <span class="hint" data-empty>a doc has no lifecycle</span>
        {:else}
          <span class="ar">→</span>
          {#each LIFE[t.status] ?? [] as s (s)}{@render moveButton(s)}{/each}
          {#if moreMoves}
            {#each ALL.filter((s) => s !== t.status && !(LIFE[t.status] ?? []).includes(s)) as s (s)}{@render moveButton(s)}{/each}
          {/if}
          <button type="button" class="btn ghost" data-action="more-moves" aria-pressed={moreMoves} onclick={() => (moreMoves = !moreMoves)}>{moreMoves ? "fewer" : "more…"}</button>
        {/if}
      </div>
      {#if lack.edit}<p class="why" data-needs="tickets.set_status">{why(lack.edit)}</p>{/if}
      {#if t.status === "Canceled" && !lack.edit}
        <label class="hint check"><input type="checkbox" data-field="restore-children" bind:checked={restoreChildren}/> put back what its cancel canceled</label>
      {/if}
      {#if ask}
        <div class="ta-line" data-ask={ask}>
          <input class="in" data-field="message" placeholder={"why " + ask + " (required)"} autocomplete="off" bind:value={msgs.status}/>
          <button type="button" class="btn primary" data-action="tickets.set_status" data-confirm={ask} disabled={busy} onclick={() => move(ask)}>{ask}</button>
          <button type="button" class="btn ghost" data-action="unask" onclick={() => (ask = "")}>cancel</button>
        </div>
      {:else if !lack.edit}
        <div class="ta-line">{@render note("status")}</div>
      {/if}
      {@render answer("tickets.set_status")}
    </div>

    <div data-group="labels">
      <h2 class="sec">Labels</h2>
      <div class="ta-row" data-field="labels">
        {#each t.labels ?? [] as l (l)}
          <span class="label" data-label={l}>{l}{#if !lack.edit}<button type="button" class="x" data-action="tickets.label" data-remove={l} aria-label={"remove " + l} disabled={busy} onclick={() => unlabel(l)}>×</button>{/if}</span>
        {/each}
        {#if lack.edit}
          {#if !(t.labels ?? []).length}<span class="hint" data-empty>none</span>{/if}
        {:else}
          <span class="ta-line grow" data-form="tickets.label">
            <input class="in" data-field="label" placeholder="+ label" autocomplete="off" spellcheck="false" bind:value={label}
                   aria-invalid={answers["tickets.label"]?.refused ? "true" : "false"} onkeydown={labelKeyed}/>
          </span>
        {/if}
      </div>
      {@render answer("tickets.label")}
    </div>

    <div data-group="archive">
      <h2 class="sec">Archive</h2>
      {#if archived}
        <div class="ta-row">
          <span class="hint" data-archived>archived · out of every view</span>
          <button type="button" class="btn" data-action="tickets.restore" disabled={!!lack.edit || busy} onclick={restore}>restore</button>
          <label class="hint check"><input type="checkbox" data-field="with-children" bind:checked={withChildren}/> with children</label>
        </div>
        {#if !lack.edit}<div class="ta-line">{@render note("archive")}</div>{/if}
      {:else if dry.archive}
        <p class="ta-dry" data-dry-answer="tickets.archive">{dry.archive.text}</p>
        <div class="ta-line">
          {@render note("archive")}
          <button type="button" class="btn primary" data-action="tickets.archive" disabled={busy} onclick={archive}>archive</button>
          <button type="button" class="btn ghost" data-action="undry" onclick={() => (dry.archive = null)}>cancel</button>
        </div>
      {:else}
        <div class="ta-row"><button type="button" class="btn" data-action="tickets.archive" data-dry disabled={!!lack.edit || busy} onclick={dryArchive}>archive…</button><span class="hint">a dry run first</span></div>
      {/if}
      {@render answer("tickets.archive")}
    </div>

    <div data-group="rename">
      <h2 class="sec">Rename</h2>
      <div class="ta-line">
        <input class="in" data-field="rename-to" placeholder={(t.parent ?? t.id) + ".1"} spellcheck="false" autocomplete="off" bind:value={renameTo} oninput={() => (dry.rename = null)}/>
        <button type="button" class="btn" data-action="tickets.rename" data-dry disabled={!!lack.edit || busy || !renameTo.trim()} onclick={() => dryRun("rename", "tickets.rename", b.call("tickets.rename", renameP(true)))}>dry run</button>
      </div>
      {#if dry.rename}
        <p class="ta-dry" data-dry-answer="tickets.rename">{dry.rename.text}</p>
        <div class="ta-line">
          {@render note("rename")}
          <button type="button" class="btn primary" data-action="tickets.rename" disabled={busy} onclick={() => moveTo("rename", "tickets.rename", b.call("tickets.rename", renameP(false)))}>rename</button>
          <button type="button" class="btn ghost" data-action="undry" onclick={() => (dry.rename = null)}>cancel</button>
        </div>
      {/if}
      {@render answer("tickets.rename")}
    </div>

    <div data-group="revision">
      <h2 class="sec">Restore a revision</h2>
      {#if !rev}
        <div class="ta-line">
          <span class="hint" data-empty>pick one in Revisions, or</span>
          <input class="in rev" data-field="rev" placeholder="rev" inputmode="numeric" autocomplete="off" onchange={(ev) => (rev = ev.currentTarget.value.trim())}/>
        </div>
      {:else}
        <div class="ta-rev" data-field="revision"><span class="rev-n">rev {rev}</span><button type="button" class="x" data-action="unpick" aria-label="clear" onclick={() => (rev = "")}>×</button></div>
        <div class="ta-checks" data-field="fields">
          {#each FIELDS as f (f)}
            <label><input type="checkbox" data-rf={f} checked={revFields.includes(f)} onchange={(ev) => toggleField(f, ev.currentTarget.checked)}/> {f}</label>
          {/each}
        </div>
        <div class="ta-line">
          {@render note("revision")}
          <button type="button" class="btn" data-action="tickets.revisions.restore" disabled={!!lack.edit || busy} onclick={restoreRevision}>restore rev {rev}</button>
        </div>
        <p class="hint">{revFields.length ? revFields.join(", ") : "every field it keeps"}</p>
      {/if}
      {@render answer("tickets.revisions.restore")}
    </div>

    <div data-group="admin">
      <h2 class="sec">Admin</h2>
      <div class="ta-grid">
        {#if t.frozen}
          <button type="button" class="btn" data-action="tickets.unfreeze" disabled={!!lack.freeze || busy} onclick={() => freeze(false)}>unfreeze</button>
        {:else}
          <button type="button" class="btn" data-action="tickets.freeze" disabled={!!lack.freeze || busy} onclick={() => freeze(true)}>freeze</button>
        {/if}
        {#if lack.freeze}{@render needing(lack.freeze, "tickets.freeze")}{:else}<span class="hint" data-frozen={t.frozen ? "true" : "false"}>{t.frozen ? "frozen · no new children or numbers" : "no new children or numbers"}</span>{/if}

        <button type="button" class="btn" data-action="tickets.reshortcode" data-open disabled={!!lack.reshortcode} aria-pressed={rcOpen} onclick={() => { rcOpen = !rcOpen; dry.reshortcode = null; }}>reshortcode…</button>
        {#if lack.reshortcode}
          {@render needing(lack.reshortcode, "tickets.reshortcode")}
        {:else}
          <span class="ta-line tight">
            {#if rcOpen}
              <input class="in" data-field="reshortcode-to" placeholder="a shortcode" spellcheck="false" autocomplete="off" bind:value={rcTo} oninput={() => (dry.reshortcode = null)}/>
              <button type="button" class="btn" data-action="tickets.reshortcode" data-dry disabled={busy || !rcTo.trim()} onclick={() => dryRun("reshortcode", "tickets.reshortcode", b.call("tickets.reshortcode", reshortP(true)))}>dry run</button>
            {/if}
          </span>
        {/if}
        {#if dry.reshortcode}
          <span></span>
          <span>
            <p class="ta-dry" data-dry-answer="tickets.reshortcode">{dry.reshortcode.text}</p>
            <span class="ta-line">
              {@render note("reshortcode")}
              <button type="button" class="btn primary" data-action="tickets.reshortcode" disabled={busy} onclick={() => moveTo("reshortcode", "tickets.reshortcode", b.call("tickets.reshortcode", reshortP(false)))}>move</button>
            </span>
          </span>
        {/if}

        <button type="button" class="btn" data-action="tickets.delete" data-open disabled={!!lack.delete} aria-pressed={dlOpen} onclick={() => (dlOpen = !dlOpen)}>delete…</button>
        {#if lack.delete}{@render needing(lack.delete, "tickets.delete")}<span></span>{/if}
        <span class="ta-opts" class:off={!!lack.delete}>
          <span>children</span>
          {#each ["reparent", "delete"] as c (c)}
            <label><input type="radio" name={"children-" + id} data-field="children" data-children={c} value={c} disabled={!!lack.delete} bind:group={dlChildren}/> {c}</label>
          {/each}
          <label><input type="checkbox" data-field="unblock" disabled={!!lack.delete} bind:checked={dlUnblock}/> unblock</label>
        </span>
        {#if dlOpen && !lack.delete}
          <span></span>
          <span class="ta-line tight">
            <span class="hint warn">deletes {t.id} for good; a tombstone stays 30 days</span>
            {@render note("delete")}
            <button type="button" class="btn danger" data-action="tickets.delete" data-confirm disabled={busy} onclick={del}>delete</button>
          </span>
        {/if}
      </div>
      {@render answer("tickets.freeze")}
      {@render answer("tickets.reshortcode")}
      {@render answer("tickets.delete")}
    </div>
  {/if}

  <!-- A deleted ticket offers its undelete; a ticket that is not there draws nothing (the page says so). -->
  {#if gone}
    <div data-group="undelete">
      <h2 class="sec">Deleted</h2>
      {@render answer("tickets.delete")}
      <div class="ta-row">
        <button type="button" class="btn" data-action="tickets.undelete" disabled={!!lack.undelete || busy} onclick={undelete}>undelete</button>
        {#if lack.undelete}{@render needing(lack.undelete, "tickets.undelete")}{/if}
      </div>
    </div>
  {/if}
  {@render answer("tickets.undelete")}
</section>

<style>
  .ticket-actions { display: block; margin-top: 2px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .sec { display: flex; gap: 7px; align-items: baseline; margin: 22px 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  [data-group]:first-child > .sec { margin-top: 0; }
  .aside { margin-left: auto; text-transform: none; letter-spacing: 0; }
  .ta-who { display: flex; gap: 6px; align-items: center; font-size: 11px; color: var(--muted); }
  .ta-who b { color: var(--ink); font-weight: var(--weight-medium); }
  .ta-row { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .ar { color: var(--muted); }
  .ta-line { display: flex; gap: 6px; align-items: center; margin-top: 6px; }
  .ta-line .in { flex: 1; }
  .ta-line.grow { flex: 1; margin: 0; }
  .ta-line.tight { flex-wrap: wrap; margin: 0; }
  .ta-dry { margin: 6px 0 0; padding: 6px 8px; font-size: 11px; line-height: 1.5; color: var(--ink); background: var(--sel); border-radius: var(--radius); }
  .ta-grid { display: grid; grid-template-columns: auto 1fr; gap: 6px 10px; align-items: center; }
  .why { justify-self: start; margin: 4px 0 0; font-size: 11px; color: var(--disabled); }
  .ta-grid .why { margin: 0; }
  .ta-opts { display: flex; flex-wrap: wrap; gap: 10px; font-size: 11px; color: var(--muted); white-space: nowrap; }
  .ta-opts.off { color: var(--disabled); }
  .ta-opts label { display: flex; gap: 4px; align-items: center; }
  .ta-rev { display: flex; gap: 8px; align-items: center; font-size: 12px; }
  .rev-n { color: var(--accent); }
  .ta-checks { display: flex; flex-wrap: wrap; gap: 6px 12px; margin-top: 6px; font-size: 12px; }
  .ta-checks label { display: flex; gap: 4px; align-items: center; }
  .ta-checks input, .check input, .ta-opts input { accent-color: var(--accent); }
  .check { display: inline-flex; gap: 4px; align-items: center; }
  .in.rev { flex: 0 0 7ch; }
  .tag { display: inline-block; padding: 0 7px; font-size: 11px; line-height: 18px; border-radius: var(--radius-md); background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .tag[data-status="Pending"] { background: var(--st-pending-bg); color: var(--st-pending-fg); }
  .tag[data-status="Ready"] { background: var(--st-ready-bg); color: var(--st-ready-fg); }
  .tag[data-status="Coding"] { background: var(--st-coding-bg); color: var(--st-coding-fg); }
  .tag[data-status="In Review"] { background: var(--st-in-review-bg); color: var(--st-in-review-fg); }
  .tag[data-status="Complete"] { background: var(--st-complete-bg); color: var(--st-complete-fg); }
  .tag[data-status="Blocked"] { background: var(--st-blocked-bg); color: var(--st-blocked-fg); }
  .tag[data-status="Superseded"] { background: var(--st-superseded-bg); color: var(--st-superseded-fg); }
  .tag[data-status="Canceled"] { background: var(--st-canceled-bg); color: var(--st-canceled-fg); }
  .tag[data-status="Draft"] { background: var(--st-draft-bg); color: var(--st-draft-fg); }
  .tag[data-status="Epic"] { background: var(--st-epic-bg); color: var(--st-epic-fg); }
  .tag[data-status="Doc"] { background: var(--st-doc-bg); color: var(--st-doc-fg); }
  .btn { display: inline-flex; gap: 5px; align-items: center; padding: 1px 10px; font-size: 12px; line-height: 20px; white-space: nowrap; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .btn:hover, .btn:global([data-sheet~=hover]) { background: var(--hover); border-color: var(--muted); }
  .btn:focus-visible, .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .btn.primary:hover, .btn.primary:global([data-sheet~=hover]) { opacity: .88; }
  .btn.danger { color: var(--bg); background: var(--warn); border-color: var(--warn); }
  .btn.danger:hover, .btn.danger:global([data-sheet~=hover]) { opacity: .88; }
  .btn:disabled, .btn:disabled:hover, .btn:global([data-sheet~=disabled]), .btn:disabled:global([data-sheet~=hover]), .btn:global([data-sheet~=disabled]):hover, .btn:global([data-sheet~=disabled]):global([data-sheet~=hover]) { color: var(--disabled); background: var(--card); border-color: var(--line); cursor: not-allowed; opacity: 1; }
  .btn[aria-pressed="true"], .btn:global([data-sheet~=checked]) { color: var(--accent); background: var(--sel); border-color: var(--accent); }
  .btn[data-refused-on="true"] { color: var(--warn); border-color: var(--warn); }
  .btn.ghost { color: var(--muted); background: none; border-color: transparent; }
  .btn.ghost:hover, .btn.ghost:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .x { margin-left: 2px; padding: 0 3px; font-size: 12px; line-height: 14px; color: var(--muted); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .x:hover, .x:global([data-sheet~=hover]) { color: var(--warn); background: var(--hover); }
  .x:focus-visible, .x:global([data-sheet~=focus]) { outline: 2px solid var(--focus); }
  .in { min-width: 0; padding: 3px 8px; font-size: 12px; line-height: 18px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .in::placeholder { color: var(--muted); }
  .in:hover, .in:global([data-sheet~=hover]) { border-color: var(--muted); }
  .in:focus, .in:global([data-sheet~=focus]) { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
  .in[aria-invalid="true"] { border-color: var(--warn); }
  .label { display: inline-flex; align-items: center; padding: 0 8px; font-size: 11px; line-height: 18px; color: var(--chip-fg); background: var(--chip-bg); border: var(--border) solid var(--chip-line); border-radius: var(--radius-pill); }
  .refused { margin: 5px 0 0; font-size: 11px; line-height: 1.4; color: var(--warn); }
  .refused::before { content: ""; display: inline-block; width: 6px; height: 6px; margin-right: 6px; vertical-align: 1px; border-radius: 50%; background: var(--warn); }
  .landed { margin: 5px 0 0; font-size: 11px; color: var(--live); }
  .landed::before { content: "\2713  "; }
  .hint { font-size: 11px; color: var(--muted); }
  .hint.warn { color: var(--warn); }
  .empty { padding: 8px 6px; font-size: 12px; color: var(--muted); }
  .skel { display: flex; flex-direction: column; gap: 8px; }
  .skel .bar { display: block; width: 70%; height: 14px; background: var(--skeleton); border-radius: var(--radius-sm); animation: breathe var(--motion-pulse) var(--ease) infinite alternate; }
  .skel .bar.short { width: 40%; }
  @keyframes breathe { from { opacity: 1; } to { opacity: .55; } }
  @media (prefers-reduced-motion: reduce) {
    .btn, .in { transition: none; }
  }
</style>
