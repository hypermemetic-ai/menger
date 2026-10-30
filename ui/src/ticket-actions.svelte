<!-- SURFACE-4: what can be done to a ticket (svelte/1), placed in the ticket
     page by <ticket-actions id={id}/>, as the designer drew it in the side
     column: the moves the lifecycle allows from the status (set_status;
     Canceled and Superseded ask why), labels (a chip with ×, a field to
     add), archive (a dry run first) and restore, rename (a dry run first), a
     revision put back (the rev from `ticket.revision_picked` on the bus, or
     typed), and the admin's freeze/unfreeze, reshortcode (a dry run first),
     delete (what its children and waiters do) and undelete. Each action is
     [data-action=<method>]; its answer is said beside it, a refusal in the
     store's words ([data-refused]); an action the caller's roles do not hold
     is drawn disabled, naming the role `auth requires` says it needs. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "auth.whoami", "auth.requires",
    "tickets.set_status", "tickets.label", "tickets.archive", "tickets.restore", "tickets.rename", "tickets.revisions.restore",
    "tickets.freeze", "tickets.unfreeze", "tickets.reshortcode", "tickets.delete", "tickets.undelete"];
  // The lifecycle's moves from each status, the next step first (the designer's).
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
  const FIELDS = ["title", "body", "status", "kind", "severity", "parent", "blocked_by", "labels"];
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { id } = $props();
  const b = bus();

  const ticket = live({ snapshot: "tickets.get", params: () => ({ id }), changes: "tickets.watch", key: "id",
                        on: { amended: "upsert:ticket", status_changed: "upsert:ticket", kind_changed: "upsert:ticket", labels_changed: "upsert:ticket",
                              severity_changed: "upsert:ticket", hidden_changed: "upsert:ticket", blockers_changed: "upsert:ticket",
                              parent_changed: "upsert:ticket", frozen_changed: "upsert:ticket", deleted: "remove:ids" } });
  const t = $derived($ticket.rows[0]);
  // Who may: the caller's memberships, and the roles each class of action needs.
  const who = live({ snapshot: "auth.whoami", params: {}, key: "principal" });
  const needs = (method) => live({ snapshot: "auth.requires", params: { method }, key: "method" });
  const need = { edit: needs("tickets.set_status"), freeze: needs("tickets.freeze"), reshortcode: needs("tickets.reshortcode"), delete: needs("tickets.delete"), undelete: needs("tickets.undelete") };
  const me = $derived($who.rows[0]);
  const held = $derived((me?.memberships ?? []).map((m) => m.role));
  /** The roles `auth requires` names for a class, when the caller holds none of them; else null. */
  function lacks(store) {
    const roles = store?.rows?.[0]?.roles;
    if (!me || !roles) return null;
    return held.some((r) => roles.includes(r)) ? null : roles;
  }
  const lack = $derived({ edit: lacks(need.edit), freeze: lacks(need.freeze), reshortcode: lacks(need.reshortcode), delete: lacks(need.delete), undelete: lacks(need.undelete) });
  const why = (roles) => `${roles.join(" or ")} · you hold ${held.join(", ") || "nothing"}`;

  let msg = $state("");
  let ask = $state(""); // Canceled or Superseded, waiting for its why
  let refusedOn = $state("");
  let label = $state("");
  let arch = $state(""); // "" the store's word, else "archived" / "live" after a write here
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

  b.on("ticket.revision_picked", (data) => {
    if (data?.rev != null) {
      rev = String(data.rev);
      answers["tickets.revisions.restore"] = undefined;
    }
  });

  /** A write, its answer as FRAME-37 hands it — `{ ok, events, message }` —
   *  or, before it lands, made so from the events (an in-band error, a
   *  not_found) or the refusal thrown. */
  async function settle(asked, params) {
    let r;
    try {
      r = await asked;
    } catch (e) {
      return { ok: false, events: [], message: e?.message ?? String(e) };
    }
    if (r && !Array.isArray(r) && typeof r === "object" && "ok" in r) return r;
    const events = Array.isArray(r) ? r : [];
    const bad = events.find((e) => e?.type === "error" || e?.type === "not_found" || e?.type === "refused");
    return { ok: !bad, events, message: bad ? (bad.message ?? bad.reason ?? `no ticket ${id}`) : undefined };
  }
  /** A write asked (`b.call("…", params)`, its method as written); its answer said under `key` — `said(event)` or the refusal. */
  async function write(key, asked, params, said) {
    answers[key] = { asking: true };
    const r = await settle(asked, params);
    answers[key] = r.ok ? { said: said ? said(r.events[0] ?? {}, params) : "" } : { refused: r.message ?? "refused" };
    return r;
  }
  const withWhy = (p) => (msg.trim() ? { ...p, message: msg.trim() } : p);
  const moves = (e) => (e.moves ?? []).map((m) => `${m.from} → ${m.to}`).join(", ");

  // The lifecycle.
  async function move(to) {
    if ((to === "Canceled" || to === "Superseded") && ask !== to) {
      ask = to;
      return;
    }
    if (ask === to && !msg.trim()) return;
    ask = "";
    refusedOn = to;
    const p = withWhy({ id, status: to, source: "app" });
    const r = await write("tickets.set_status", b.call("tickets.set_status", p), p,
      (e, p) => `${e.previous} → ${e.ticket?.status}${p.message ? ` · “${p.message}”` : ""}`);
    if (r.ok) {
      refusedOn = "";
      msg = "";
    }
  }

  // Labels.
  function labelKeyed(ev) {
    if (ev.key !== "Enter") return;
    ev.preventDefault();
    const l = label.trim();
    if (!l) return;
    label = "";
    const p = { id, add: [l] };
    write("tickets.label", b.call("tickets.label", p), p);
  }
  const unlabel = (l) => { const p = { id, remove: [l] }; return write("tickets.label", b.call("tickets.label", p), p); };

  // The archive.
  async function dryArchive() {
    dry.archive = { text: `archives ${id} with its whole subtree` };
    const r = await settle(b.call("tickets.archive", { ids: [id], dry_run: true }), {});
    if (!r.ok) {
      dry.archive = null;
      answers["tickets.archive"] = { refused: r.message };
      return;
    }
    const e = r.events[0] ?? {};
    if (dry.archive) dry.archive = { text: `archives ${(e.ids ?? []).join(", ")} · ${(e.ids ?? []).length} out of every view${e.live?.length ? `, ${e.live.length} still live` : ""}` };
  }
  async function archive() {
    dry.archive = null;
    arch = "archived";
    const p = withWhy({ ids: [id] });
    const r = await write("tickets.archive", b.call("tickets.archive", p), p, (e) => `archived ${(e.ids ?? []).join(", ")}`);
    if (!r.ok) arch = "";
  }
  async function restore() {
    arch = "live";
    const p = withWhy({ ids: [id], with_children: withChildren });
    const r = await write("tickets.archive", b.call("tickets.restore", p), p,
      (e) => `restored ${(e.ids ?? []).join(", ")}${e.still_archived?.length ? ` · still archived: ${e.still_archived.join(", ")}` : ""}`);
    if (!r.ok) arch = "";
  }
  const archived = $derived(arch === "" ? t?.archived != null : arch === "archived");

  // Rename and reshortcode: a dry run, then the move.
  async function dryRun(kind, method, asked) {
    dry[kind] = { text: "…" };
    answers[method] = undefined;
    const r = await settle(asked, {});
    if (!r.ok) {
      dry[kind] = null;
      answers[method] = { refused: r.message };
    } else if (dry[kind]) dry[kind] = { text: `would move ${moves(r.events[0] ?? {})}` };
  }
  async function moveTo(kind, method, asked) {
    dry[kind] = null;
    const r = await write(method, asked, {}, (e) => `moved ${moves(e)}`);
    const to = r.ok && r.events[0]?.moves?.[0]?.to;
    if (to) b.navigate("#/ticket/" + to);
  }
  const renameP = (dry) => withWhy({ id, to: renameTo.trim(), ...(dry ? { dry_run: true } : {}) });
  const reshortP = (dry) => withWhy({ ids: [id], to: rcTo.trim(), ...(dry ? { dry_run: true } : {}) });

  // A revision put back.
  function toggleField(f, on) {
    revFields = on ? [...revFields.filter((x) => x !== f), f] : revFields.filter((x) => x !== f);
  }
  function restoreRevision() {
    const n = Number(rev);
    if (!(n > 0)) return;
    const p = withWhy({ id, rev: n, source: "app" });
    if (revFields.length) p.fields = revFields;
    write("tickets.revisions.restore", b.call("tickets.revisions.restore", p), p,
      (e) => (e.changed?.length ? `from rev ${e.from_rev}: ${e.changed.join(", ")}` : `nothing differs from rev ${e.from_rev}`));
  }

  // The admin's.
  const freeze = (on) => write("tickets.freeze", on ? b.call("tickets.freeze", { id }) : b.call("tickets.unfreeze", { id }), { id }, () => (on ? "frozen" : "growing again"));
  async function del() {
    dlOpen = false;
    answers["tickets.undelete"] = undefined;
    const p = withWhy({ ids: [id], unblock: dlUnblock });
    if (dlChildren) p.children = dlChildren;
    await write("tickets.delete", b.call("tickets.delete", p), p, (e) => `deleted ${(e.ids ?? []).join(", ")} · undelete within 30 days`);
  }
  const undelete = () => write("tickets.undelete", b.call("tickets.undelete", { ids: [id] }), { ids: [id] }, (e) => `back: ${(e.ids ?? []).join(", ")}`);
</script>

{#snippet answer(method)}
  {#if answers[method]?.refused}<p class="refused" data-refused data-for={method}>{answers[method].refused}</p>{/if}
  {#if answers[method]?.said}<p class="landed" data-answer data-for={method}>{answers[method].said}</p>{/if}
  {#if answers[method]?.asking}<p class="hint" data-loading data-for={method}>…</p>{/if}
{/snippet}
{#snippet needing(roles, method)}
  <span class="why" data-needs={method}>{why(roles)}</span>
{/snippet}

<section class="ticket-actions" data-surface="ticket-actions" data-ticket={id}>
  {#if $ticket.status === "pending"}
    <div class="skel" data-loading><span class="bar"></span><span class="bar short"></span><span class="bar"></span></div>
  {/if}
  {#if t}
    <div data-group="status">
      <h2 class="sec">Status
        {#if me}<span class="aside ta-who" data-field="who">as <b>{me.name}</b> · {(me.memberships ?? []).map((m) => m.role + (m.scope === "*" ? "" : " on " + m.scope)).join(", ")}</span>{/if}
      </h2>
      <div class="ta-row" data-field="status-moves">
        <span class="tag status" data-status={t.status} data-field="status">{t.status}</span>
        {#if t.kind === "doc"}
          <span class="hint" data-empty>a doc has no lifecycle</span>
        {:else}
          <span class="ar">→</span>
          {#each LIFE[t.status] ?? [] as s (s)}
            <button type="button" class="btn" data-action="tickets.set_status" data-to={s} disabled={!!lack.edit}
                    data-refused-on={answers["tickets.set_status"]?.refused && refusedOn === s ? "true" : "false"}
                    aria-pressed={ask === s} onclick={() => move(s)}>{s}{s === "Superseded" || s === "Canceled" ? "…" : ""}</button>
          {/each}
        {/if}
      </div>
      {#if lack.edit}<p class="why" data-needs="tickets.set_status">{why(lack.edit)}</p>{/if}
      {#if ask}
        <div class="ta-line" data-ask={ask}>
          <input class="in" data-field="message" placeholder={"why " + ask + " (required)"} autocomplete="off" bind:value={msg}/>
          <button type="button" class="btn primary" data-action="tickets.set_status" data-confirm={ask} onclick={() => move(ask)}>{ask}</button>
          <button type="button" class="btn ghost" data-action="unask" onclick={() => (ask = "")}>cancel</button>
        </div>
      {:else if !lack.edit}
        <div class="ta-line"><input class="in" data-field="message" placeholder="a message, if any" autocomplete="off" bind:value={msg}/></div>
      {/if}
      {@render answer("tickets.set_status")}
    </div>

    <div data-group="labels">
      <h2 class="sec">Labels</h2>
      <div class="ta-row" data-field="labels">
        {#each t.labels ?? [] as l (l)}
          <span class="label" data-label={l}>{l}{#if !lack.edit}<button type="button" class="x" data-action="tickets.label" data-remove={l} aria-label={"remove " + l} onclick={() => unlabel(l)}>×</button>{/if}</span>
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
          <button type="button" class="btn" data-action="tickets.restore" disabled={!!lack.edit} onclick={restore}>restore</button>
          <label class="hint check"><input type="checkbox" data-field="with-children" bind:checked={withChildren}/> with children</label>
        </div>
      {:else if dry.archive}
        <p class="ta-dry" data-dry-answer="tickets.archive">{dry.archive.text}</p>
        <div class="ta-line">
          <button type="button" class="btn primary" data-action="tickets.archive" onclick={archive}>archive</button>
          <button type="button" class="btn ghost" data-action="undry" onclick={() => (dry.archive = null)}>cancel</button>
        </div>
      {:else}
        <div class="ta-row"><button type="button" class="btn" data-action="tickets.archive" data-dry disabled={!!lack.edit} onclick={dryArchive}>archive…</button><span class="hint">a dry run first</span></div>
      {/if}
      {@render answer("tickets.archive")}
    </div>

    <div data-group="rename">
      <h2 class="sec">Rename</h2>
      <div class="ta-line">
        <input class="in" data-field="rename-to" placeholder={(t.parent ?? t.id) + ".1"} spellcheck="false" autocomplete="off" bind:value={renameTo} oninput={() => (dry.rename = null)}/>
        <button type="button" class="btn" data-action="tickets.rename" data-dry disabled={!!lack.edit || !renameTo.trim()} onclick={() => dryRun("rename", "tickets.rename", b.call("tickets.rename", renameP(true)))}>dry run</button>
      </div>
      {#if dry.rename}
        <p class="ta-dry" data-dry-answer="tickets.rename">{dry.rename.text}</p>
        <div class="ta-line">
          <button type="button" class="btn primary" data-action="tickets.rename" onclick={() => moveTo("rename", "tickets.rename", b.call("tickets.rename", renameP(false)))}>rename</button>
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
          <span class="hint">{revFields.length ? revFields.join(", ") : "every field it keeps"}</span>
          <button type="button" class="btn" data-action="tickets.revisions.restore" disabled={!!lack.edit} onclick={restoreRevision}>restore rev {rev}</button>
        </div>
      {/if}
      {@render answer("tickets.revisions.restore")}
    </div>

    <div data-group="admin">
      <h2 class="sec">Admin</h2>
      <div class="ta-grid">
        {#if t.frozen}
          <button type="button" class="btn" data-action="tickets.unfreeze" disabled={!!lack.freeze} onclick={() => freeze(false)}>unfreeze</button>
        {:else}
          <button type="button" class="btn" data-action="tickets.freeze" disabled={!!lack.freeze} onclick={() => freeze(true)}>freeze</button>
        {/if}
        {#if lack.freeze}{@render needing(lack.freeze, "tickets.freeze")}{:else}<span class="hint" data-frozen={t.frozen ? "true" : "false"}>{t.frozen ? "frozen · no new children or numbers" : "no new children or numbers"}</span>{/if}

        <button type="button" class="btn" data-action="tickets.reshortcode" data-open disabled={!!lack.reshortcode} aria-pressed={rcOpen} onclick={() => { rcOpen = !rcOpen; dry.reshortcode = null; }}>reshortcode…</button>
        {#if lack.reshortcode}
          {@render needing(lack.reshortcode, "tickets.reshortcode")}
        {:else}
          <span class="ta-line tight">
            {#if rcOpen}
              <input class="in" data-field="reshortcode-to" placeholder="a shortcode" spellcheck="false" autocomplete="off" bind:value={rcTo} oninput={() => (dry.reshortcode = null)}/>
              <button type="button" class="btn" data-action="tickets.reshortcode" data-dry disabled={!rcTo.trim()} onclick={() => dryRun("reshortcode", "tickets.reshortcode", b.call("tickets.reshortcode", reshortP(true)))}>dry run</button>
            {/if}
          </span>
        {/if}
        {#if dry.reshortcode}
          <span></span>
          <span>
            <p class="ta-dry" data-dry-answer="tickets.reshortcode">{dry.reshortcode.text}</p>
            <span class="ta-line"><button type="button" class="btn primary" data-action="tickets.reshortcode" onclick={() => moveTo("reshortcode", "tickets.reshortcode", b.call("tickets.reshortcode", reshortP(false)))}>move</button></span>
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
            <button type="button" class="btn danger" data-action="tickets.delete" data-confirm onclick={del}>delete</button>
          </span>
        {/if}
      </div>
      {@render answer("tickets.freeze")}
      {@render answer("tickets.reshortcode")}
      {@render answer("tickets.delete")}
    </div>
  {/if}

  {#if $ticket.status === "live" && !t}
    <div data-group="undelete">
      <h2 class="sec">Gone</h2>
      <p class="empty" data-empty>No ticket {id} here — deleted, moved or never filed.</p>
      {@render answer("tickets.delete")}
      <div class="ta-row">
        <button type="button" class="btn" data-action="tickets.undelete" disabled={!!lack.undelete} onclick={undelete}>undelete</button>
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
  .btn { display: inline-flex; gap: 5px; align-items: center; padding: 1px 10px; font-size: 12px; line-height: 20px; white-space: nowrap; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .btn:hover { background: var(--hover); border-color: var(--muted); }
  .btn:focus-visible { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .btn.primary:hover { opacity: .88; }
  .btn.danger { color: var(--bg); background: var(--warn); border-color: var(--warn); }
  .btn.danger:hover { opacity: .88; }
  .btn:disabled, .btn:disabled:hover { color: var(--disabled); background: var(--card); border-color: var(--line); cursor: not-allowed; opacity: 1; }
  .btn[aria-pressed="true"] { color: var(--accent); background: var(--sel); border-color: var(--accent); }
  .btn[data-refused-on="true"] { color: var(--warn); border-color: var(--warn); }
  .btn.ghost { color: var(--muted); background: none; border-color: transparent; }
  .btn.ghost:hover { color: var(--ink); background: var(--hover); }
  .x { margin-left: 2px; padding: 0 3px; font-size: 12px; line-height: 14px; color: var(--muted); background: none; border: 0; border-radius: var(--radius-sm); cursor: pointer; }
  .x:hover { color: var(--warn); background: var(--hover); }
  .x:focus-visible { outline: 2px solid var(--focus); }
  .in { min-width: 0; padding: 3px 8px; font-size: 12px; line-height: 18px; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius); transition: border-color var(--motion-fast) var(--ease); }
  .in::placeholder { color: var(--muted); }
  .in:hover { border-color: var(--muted); }
  .in:focus { outline: none; border-color: var(--focus); box-shadow: 0 0 0 2px var(--sel); }
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
