<!-- V2-20: a ticket's questions, answered in place — `tickets.inputs.list`
     live by `tickets.watch`'s `input_changed`. A decision is its options, a
     click answers; a multi its boxes and `save`; a text its box and `save`
     (`ctrl+enter`); each with a why. An answered one says what and who, and
     can be cleared or closed; a closed one reopened; a withdrawn or removed
     one says so and takes nothing. Under them, `ask` files a new question,
     and the queue: the ticket's entry (`tickets.queued { id }`), or a form
     that puts it in front of the owner, with the urgency and why.
     Keys (through the bus's `keys`): `a` the first open question, `q` the
     queue's form. A refusal is the service's words, `data-refused`.
     `sample` draws given inputs, for the sheet: every kind in every state. -->
<script module>
  export const uses = ["tickets.inputs.list", "tickets.inputs.ask", "tickets.inputs.answer", "tickets.inputs.clear", "tickets.inputs.close",
                       "tickets.inputs.reopen", "tickets.queued", "tickets.queue", "tickets.unqueue", "tickets.watch"];
  const URGENCIES = ["whenever", "today", "now", "blocking"];
  const said = (v) => (v?.type === "choice" ? v.option : v?.type === "multi" ? (v.options.join(", ") || "none") : v?.text ?? "");
</script>

<script>
  import { bus, live } from "plexus-ui/host";

  let { id, sheet = null, sample = null } = $props();
  const b = bus();
  const inputs = live({ snapshot: "tickets.inputs.list", params: () => ({ ticket: id }), changes: "tickets.watch", key: "id", on: { input_changed: "upsert:input" } });
  // One watch for the section (the page is held to a count of them): the
  // entry is read once, and again when an input moves — closing the last it
  // names takes it out — or a write here moves it; `limit` is what changes,
  // and names nothing with `id`.
  let reads = $state(1);
  const moved = $derived(inputs.rows.reduce((n, i) => n + (i.history?.length ?? 0) + 1, 0));
  const queued = live({ snapshot: "tickets.queued", params: () => ({ id, limit: 1 + (reads + moved) % 999 }), key: "id",
                        on: { queue_row: "upsert:.", page: "upsert:.", queue_end: "upsert:." } });
  // A prop crosses a mark as text: the sheet's inputs are JSON.
  const drawn = $derived(sample == null ? null : JSON.parse(sample));
  const list = $derived((drawn ?? inputs.rows).filter((i) => i.id != null && i.state !== "removed"));
  const open = $derived(list.filter((i) => i.state === "open"));
  const entry = $derived(queued.rows.find((r) => r.id === id)?.entry ?? null);
  // What is being written, by input id: its ticks or text, and why.
  let ticks = $state({});
  let texts = $state({});
  let whys = $state({});
  let asked = $state("");
  let urgency = $state("today");
  let reason = $state("");
  let names = $state({});
  let refused = $state("");
  let landed = $state("");
  let root;

  // FRAME-37: the answer always resolves — `ok`, `events`, `message`.
  function heard(a, then) {
    if (a.ok) {
      refused = "";
      then?.(a);
    } else {
      refused = String(a.message ?? "refused").replace(/^Invalid params: /, "");
      landed = "";
    }
    return a;
  }
  const answer = (i, value) => {
    const rationale = (whys[i.id] ?? "").trim();
    return b.call("tickets.inputs.answer", { ticket: id, input: i.id, value, ...(rationale ? { rationale } : {}) }).then((a) => heard(a, () => {
      landed = `answered ${i.id}`;
      whys[i.id] = "";
    }));
  };
  const tick = (i, value) => {
    const had = ticks[i.id] ?? i.answer?.value?.options ?? [];
    ticks[i.id] = had.includes(value) ? had.filter((v) => v !== value) : [...had, value];
  };
  const ticked = (i, value) => (ticks[i.id] ?? i.answer?.value?.options ?? []).includes(value);
  const reply = (i) => answer(i, { type: "text", text: texts[i.id] ?? "" }).then((a) => a.ok && (texts[i.id] = ""));
  const textKey = (ev, i) => ev.key === "Enter" && ev.ctrlKey && (ev.preventDefault(), reply(i));
  const clear = (i) => b.call("tickets.inputs.clear", { ticket: id, input: i.id }).then((a) => heard(a));
  const close = (i) => b.call("tickets.inputs.close", { ticket: id, input: i.id }).then((a) => heard(a));
  const reopen = (i) => b.call("tickets.inputs.reopen", { ticket: id, input: i.id }).then((a) => heard(a));
  function ask(ev) {
    ev?.preventDefault?.();
    b.call("tickets.inputs.ask", { ticket: id, question: asked }).then((a) => heard(a, () => {
      landed = `asked ${a.events.find((e) => e.type === "input_changed")?.input?.id ?? ""}`;
      asked = "";
    }));
  }
  function queue(ev) {
    ev?.preventDefault?.();
    const input = Object.keys(names).filter((k) => names[k]);
    b.call("tickets.queue", { id, urgency, reason, ...(input.length ? { input } : {}) }).then((a) => heard(a, () => {
      landed = "queued";
      reason = "";
      reads += 1;
    }));
  }
  const unqueue = () => b.call("tickets.unqueue", { id }).then((a) => heard(a, () => (reads += 1)));
  const focus = (sel) => root?.querySelector(sel)?.focus();
  b.keys({
    a: { to: () => focus(".tq-item[data-state=open] button, .tq-item[data-state=open] input, .tq-item[data-state=open] textarea"), label: "the first open question" },
    q: { to: () => focus("[data-form='tickets.queue'] input[name=reason]"), label: "queue this ticket" },
  });
</script>

<section class="ticket-questions" data-surface="ticket-questions" bind:this={root} data-asking={open.length}>
  <h2 class="sec">Questions <span class="n">{list.length}</span></h2>
  {#if sheet === "loading" || (!drawn && inputs.status === "pending")}<div data-loading aria-busy="true"><i class="bone"></i></div>{/if}
  {#each list as i (i.id)}
    <article class="tq-item" data-input={i.id} data-kind={i.kind} data-state={i.state}>
      <p class="tq-q" data-field="question">{i.question ?? i.id}</p>
      {#if i.state === "open" || i.state === "answered"}
        {#if i.kind === "choice"}
          <div class="tq-options" role="group" aria-label={i.question ?? i.id}>
            {#each i.options as o (o.value)}
              <button class="tq-opt" data-option={o.value} data-recommended={o.recommended ? "true" : "false"}
                      aria-pressed={i.answer?.value?.option === o.value ? "true" : "false"} onclick={() => answer(i, { type: "choice", option: o.value })}>{o.label}</button>
            {/each}
          </div>
        {:else if i.kind === "multi"}
          <div class="tq-options">
            {#each i.options as o (o.value)}
              <label class="tq-tick"><input type="checkbox" data-option={o.value} checked={ticked(i, o.value)} onchange={() => tick(i, o.value)}/> {o.label}</label>
            {/each}
            <button class="btn" data-action="tickets.inputs.answer" onclick={() => answer(i, { type: "multi", options: ticks[i.id] ?? i.answer?.value?.options ?? [] })}>save</button>
          </div>
        {:else}
          <div class="tq-reply">
            <textarea class="in" name="reply" rows="2" spellcheck="false" placeholder="your reply" bind:value={texts[i.id]} onkeydown={(ev) => textKey(ev, i)}></textarea>
            <button class="btn" data-action="tickets.inputs.answer" onclick={() => reply(i)}>save</button>
          </div>
        {/if}
        {#if i.kind !== "text"}<input class="in tq-why" name="why" autocomplete="off" placeholder="why (optional)" bind:value={whys[i.id]}/>{/if}
        {#if i.recommendation && i.state === "open"}<p class="tq-rec" data-field="recommendation">{i.recommendation}</p>{/if}
      {/if}
      {#if i.answer}
        <p class="tq-said" data-field="answer">{said(i.answer.value)}{#if i.answer.rationale}<span class="tq-whysaid"> — {i.answer.rationale}</span>{/if}{#if i.answer.name}<span class="tq-who"> · {i.answer.name}</span>{/if}</p>
      {/if}
      {#if i.state === "withdrawn"}<p class="tq-said" data-field="withdrawn">withdrawn: its ticket is canceled</p>{/if}
      <div class="tq-tools">
        <span class="tq-state" data-field="state">{i.state}</span>
        {#if i.state === "answered"}<button class="link" data-action="tickets.inputs.clear" onclick={() => clear(i)}>clear</button>{/if}
        {#if i.state === "open" || i.state === "answered"}<button class="link" data-action="tickets.inputs.close" onclick={() => close(i)}>close</button>{/if}
        {#if i.state === "closed"}<button class="link" data-action="tickets.inputs.reopen" onclick={() => reopen(i)}>reopen</button>{/if}
      </div>
    </article>
  {/each}
  {#if !drawn && inputs.status === "live" && list.length === 0}<p class="empty" data-empty>nothing asked</p>{/if}
  <form class="add" data-form="tickets.inputs.ask" onsubmit={ask}>
    <input class="in" name="question" autocomplete="off" spellcheck="false" placeholder="ask the owner" bind:value={asked}/>
    <button class="btn" data-action="tickets.inputs.ask" data-idle={asked.trim() === "" ? "true" : "false"}>ask</button>
  </form>
  {#if entry}
    <p class="tq-queued" data-field="queued" data-urgency={entry.urgency}>
      <span class="tq-urgency">{entry.urgency}</span> {entry.reason}
      <button class="link" data-action="tickets.unqueue" onclick={unqueue}>take out</button>
    </p>
  {:else}
    <form class="add" data-form="tickets.queue" onsubmit={queue}>
      <select class="in tq-urg" name="urgency" aria-label="urgency" bind:value={urgency}>
        {#each URGENCIES as u (u)}<option value={u}>{u}</option>{/each}
      </select>
      <input class="in" name="reason" autocomplete="off" spellcheck="false" placeholder="why the owner should look" bind:value={reason}/>
      {#if open.length}
        <span class="tq-names">
          {#each open as i (i.id)}<label class="tq-tick"><input type="checkbox" name="input" data-input-name={i.id} bind:checked={names[i.id]}/> {i.id}</label>{/each}
        </span>
      {/if}
      <button class="btn" data-action="tickets.queue" data-idle={reason.trim() === "" ? "true" : "false"}>queue</button>
    </form>
  {/if}
  {#if refused}<p class="refused" data-refused role="alert">{refused}</p>{/if}
  {#if landed}<p class="landed">{landed}</p>{/if}
</section>

<style>
  .ticket-questions { color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .ticket-questions .sec { display: flex; gap: 7px; align-items: baseline; margin: 22px 0 6px; font-size: 11px; font-weight: var(--weight-medium); color: var(--muted); text-transform: uppercase; letter-spacing: .06em; }
  .ticket-questions .n { color: var(--ink); font-variant-numeric: tabular-nums; text-transform: none; letter-spacing: 0; }
  .ticket-questions .tq-item { padding: 10px 0; border-top: var(--border) solid var(--line); }
  .ticket-questions .tq-item[data-state=open] .tq-q { font-weight: var(--weight-medium); }
  .ticket-questions .tq-item[data-state=closed], .ticket-questions .tq-item[data-state=withdrawn] { color: var(--muted); }
  .ticket-questions .tq-q { margin: 0 0 6px; }
  .ticket-questions .tq-options { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; }
  .ticket-questions .tq-opt { padding: 2px 10px; font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-sm); cursor: pointer; transition: background-color var(--motion-fast) var(--ease); }
  .ticket-questions .tq-opt[data-recommended=true] { border-color: var(--accent); }
  .ticket-questions .tq-opt[aria-pressed=true], .ticket-questions .tq-opt:global([data-sheet~=selected]), .ticket-questions .tq-opt:global([data-sheet~=checked]) { background: var(--sel); border-color: var(--accent); }
  .ticket-questions .tq-opt:hover, .ticket-questions .tq-opt:global([data-sheet~=hover]) { background: var(--hover); }
  .ticket-questions .tq-opt:focus-visible, .ticket-questions .tq-opt:global([data-sheet~=focus]) { outline: var(--border) solid var(--accent); outline-offset: 1px; }
  .ticket-questions .tq-tick { display: inline-flex; gap: 4px; align-items: center; }
  .ticket-questions .tq-reply { display: flex; gap: 6px; align-items: flex-start; }
  .ticket-questions .tq-reply textarea { flex: 1; min-width: 0; resize: vertical; }
  .ticket-questions .tq-why { display: block; width: 100%; margin-top: 6px; box-sizing: border-box; }
  .ticket-questions .tq-rec, .ticket-questions .tq-said { margin: 6px 0 0; font-size: 12px; color: var(--muted); }
  .ticket-questions .tq-said { color: var(--ink); }
  .ticket-questions .tq-whysaid, .ticket-questions .tq-who { color: var(--muted); }
  .ticket-questions .tq-tools { display: flex; gap: 10px; margin-top: 6px; font-size: 11px; color: var(--muted); }
  .ticket-questions .link { padding: 0; font: inherit; color: var(--accent); background: none; border: 0; cursor: pointer; }
  .ticket-questions .in { padding: 2px 6px; font: inherit; color: var(--ink); background: var(--bg); border: var(--border) solid var(--line); border-radius: var(--radius-sm); }
  .ticket-questions .in:focus-visible, .ticket-questions .in:global([data-sheet~=focus]) { outline: var(--border) solid var(--accent); outline-offset: 1px; }
  .ticket-questions .btn { padding: 2px 10px; font: inherit; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-sm); cursor: pointer; }
  .ticket-questions .btn[data-idle=true] { opacity: .5; }
  .ticket-questions .add { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; margin-top: 10px; }
  .ticket-questions .add input[name=question], .ticket-questions .add input[name=reason] { flex: 1; min-width: 140px; }
  .ticket-questions .tq-names { display: inline-flex; flex-wrap: wrap; gap: 6px; font-size: 11px; color: var(--muted); }
  .ticket-questions .tq-queued { margin: 10px 0 0; }
  .ticket-questions .tq-urgency { padding: 0 6px; font-size: 10px; line-height: 16px; text-transform: uppercase; letter-spacing: .06em; color: var(--accent); border: var(--border) solid var(--accent); border-radius: var(--radius-sm); }
  .ticket-questions .tq-queued[data-urgency=blocking] .tq-urgency, .ticket-questions .tq-queued[data-urgency=now] .tq-urgency { color: var(--warn); border-color: var(--warn); }
  .ticket-questions .refused { margin: 6px 0 0; color: var(--warn); }
  .ticket-questions .landed, .ticket-questions .empty { margin: 6px 0 0; color: var(--muted); }
  .ticket-questions .bone { display: block; height: 10px; margin: 8px 0; background: var(--skeleton); border-radius: var(--radius-sm); }
</style>
