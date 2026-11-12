<!-- `<spec-outline>` in a ticket's body holds an OpenSpec change and draws it. Written in block form, its open
     line and its close line alone and the spec's markdown between them, it is the spec: a line of counts, its
     `##` sections as tabs, and the one chosen drawn under them. A section holding requirements draws each
     capability as a heading and each requirement as a line that opens to its statement, its code and its
     scenarios (a scenario's `- **WHEN** …` lines as steps); any other section is drawn as the body draws
     markdown (ticket-view: code cards, diagrams, tables). A requirement with no scenario is marked, since
     OpenSpec asks for at least one. An open question is a line under the counts that goes to its section.
     A question written as an input of the body (`<m-decision id>`, `<m-input id kind>`, block form) is drawn
     where it stands as that question, answered in place: a choice's options (a click answers), a multi's
     boxes and a text's box with `save`, a why, what was answered and by whom, `clear`
     (`tickets.inputs.answer`, `tickets.inputs.clear`; live by `tickets.watch`'s `input_changed`). A refusal
     is the service's words, `[data-refused]`. Until its row is read, or with no ticket, it is drawn as the
     body draws the mark.
     What the body keeps outside the tag is what is not the spec: an overview, a diagram of how it fits.
     Its words come from its own source (`source`, which a block-form tag is handed whole, so a fence's `<`
     stays as written), else `body` (how the sheet draws it). Which tab and which requirements are open is
     kept for the page by the ticket and the tag's place, so an amend redraws it where it was. In an editor
     (`editor`) a click in it stays in it, and `edit source` enters the tag's markdown.
     Written empty (`<spec-outline></spec-outline>`) it is an outline of the ticket's whole body instead,
     read live: the counts, the sections and requirements as links that scroll the body to their headings.
     It writes nothing. With no words, or no ticket known, it draws nothing.
     What it reads (`outline`, `read`):
     - a heading `Requirement: <name>` is a requirement, grouped under the nearest heading before it that
       is neither a requirement nor a scenario (a capability's name, or `Requirements`);
     - a heading `Scenario: <name>` belongs to the requirement before it;
     - a `- [ ]` or `- [x]` line is a task;
     - a list item under the heading `Open Questions` is a question;
     - a `##` heading is a section; a first `#` heading the title. Fenced code is skipped.
     Marks: `[data-surface=spec-outline]` (`[data-holds]` when it holds the spec),
     `[data-stat=requirements|scenarios|tasks|questions]`, `[data-section]` (`[aria-selected]` when a tab),
     `[data-panel=<section>]`, `[data-group=<name>]`, `[data-req=<name>]` (`[aria-expanded]`; `[data-bare]`
     when it has no scenario), `[data-req-body]`, `[data-scenario=<name>]`, `[data-step=<WORD>]`,
     `[data-question]`, `[data-open-all]`, `[data-edit]`, `[data-input=<id>]` (`[data-kind]`, `[data-state]`;
     `[data-option=<value>]`, `[data-action]`, `[data-field=question|answer|recommendation]`). -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.inputs.list", "tickets.inputs.answer", "tickets.inputs.clear"];

  const FENCE = /^ {0,3}(`{3,}|~{3,})/;
  const HEAD = /^(#{1,6})\s+(.+?)\s*#*\s*$/;
  const REQ = /^Requirement:\s*(.+)$/i;
  const SCN = /^Scenario:\s*(\S.*)$/i;

  /** A question's first words: its bold lead, else its first sentence. */
  const lead = (text) => (/^\*\*(.+?)\*\*/.exec(text)?.[1] ?? /^[^.?!]*[.?!]?/.exec(text)?.[0] ?? text).replace(/[.:]$/, "").trim();

  /** `lines`, each with the heading it is outside a fence (`h`: `[, hashes, text]`, else null). */
  function* walk(lines) {
    let fence = null;
    for (const line of lines) {
      const f = FENCE.exec(line);
      if (f) { fence = fence ? (f[1][0] === fence ? null : fence) : f[1][0]; yield { line, h: null, code: true }; continue; }
      yield { line, h: fence ? null : HEAD.exec(line), code: !!fence };
    }
  }

  /** `body` read as an OpenSpec change: `{ sections, groups: [{ name, reqs: [{ title, heading, scenarios }] }],
   *  requirements, scenarios, tasks: { done, total }, questions }`. */
  export function outline(body) {
    const sections = [], groups = [], questions = [];
    const tasks = { done: 0, total: 0 };
    let group = null, req = null, last = "", asking = false, scenarios = 0;
    for (const { line, h, code } of walk(String(body ?? "").split("\n"))) {
      if (code) continue;
      if (h) {
        const text = h[2];
        const r = REQ.exec(text);
        asking = /^open questions$/i.test(text);
        if (r) {
          if (!group || group.name !== last) groups.push(group = { name: last || "Requirements", reqs: [] });
          group.reqs.push(req = { title: r[1], heading: text, scenarios: 0 });
        } else if (SCN.test(text)) {
          if (req) { req.scenarios++; scenarios++; }
        } else {
          last = text;
          req = null;
          if (h[1].length === 2) sections.push(text);
        }
        continue;
      }
      const t = /^\s*[-*]\s+\[([ xX])\]\s+\S/.exec(line);
      if (t) { tasks.total++; if (t[1] !== " ") tasks.done++; continue; }
      const q = asking ? /^\s{0,3}(?:\d+[.)]|[-*])\s+(\S.*)$/.exec(line) : null;
      if (q) questions.push(lead(q[1]));
    }
    return { sections, groups, requirements: groups.reduce((n, g) => n + g.reqs.length, 0), scenarios, tasks, questions };
  }

  const md = (lines) => lines.join("\n").replace(/^\n+|\s+$/g, "");

  /** A scenario's lines as steps, `[{ word, text }]` (`- **WHEN** it rains`), or null when any line is not one. */
  function steps(lines) {
    const out = [];
    for (const line of lines) {
      if (!line.trim()) continue;
      const m = /^\s*[-*]\s+\*\*([A-Za-z]+)\*\*\s*(.*)$/.exec(line);
      if (!m) return null;
      out.push({ word: m[1].toUpperCase(), text: m[2] });
    }
    return out.length ? out : null;
  }

  /** A section's lines that hold requirements: `{ md, groups: [{ name, md, reqs: [{ title, md, scenarios:
   *  [{ title, md, steps }] }] }] }` (`md` the words before the first group; a group's, before its first
   *  requirement). A heading inside a requirement, at its level or deeper, is that requirement's words. */
  function requirements(lines) {
    const pre = [], groups = [];
    let group = null, req = null, into = pre, level = 0;
    const close = () => { for (const g of groups) for (const r of g.reqs) for (const s of r.scenarios) if (s.lines) { s.steps = steps(s.lines); s.md = md(s.lines); delete s.lines; } };
    for (const { line, h } of walk(lines)) {
      const r = h && REQ.exec(h[2]), s = h && SCN.exec(h[2]);
      if (r) {
        if (!group) groups.push(group = { name: "Requirements", lines: [], reqs: [] });
        group.reqs.push(req = { title: r[1], lines: [], scenarios: [] });
        level = h[1].length;
        into = req.lines;
      } else if (s && req) {
        const scn = { title: s[1], lines: [] };
        req.scenarios.push(scn);
        into = scn.lines;
      } else if (h && !(req && h[1].length > level)) {
        groups.push(group = { name: h[2], lines: [], reqs: [] });
        req = null;
        into = group.lines;
      } else {
        into.push(line);
      }
    }
    close();
    return { md: md(pre), groups: groups.map((g) => ({ name: g.name, md: md(g.lines), reqs: g.reqs.map((q) => ({ title: q.title, md: md(q.lines), scenarios: q.scenarios })) })) };
  }

  const ASK = /^<m-(decision|input)\b[^>]*\bid\s*=\s*(?:"([^"]+)"|'([^']+)'|([^\s>]+))/i;

  /** Markdown lines as parts, in order: `{ md }`, and `{ input, md }` for each input of the body written in
   *  block form (its id; `md` its own lines, open to close). */
  export function parts(lines) {
    const out = [];
    let run = [], ask = null;
    const flush = () => { const text = md(run); if (text) out.push({ md: text }); run = []; };
    for (const { line, code } of walk(lines)) {
      if (ask) {
        ask.lines.push(line);
        if (!code && line.includes(`</m-${ask.tag}>`)) { out.push({ input: ask.id, md: ask.lines.join("\n") }); ask = null; }
        continue;
      }
      const m = code ? null : ASK.exec(line);
      if (!m) { run.push(line); continue; }
      flush();
      const tag = m[1].toLowerCase(), id = m[2] ?? m[3] ?? m[4];
      if (line.includes(`</m-${tag}>`)) out.push({ input: id, md: line });
      else ask = { tag, id, lines: [line] };
    }
    if (ask) run.push(...ask.lines);
    flush();
    return out;
  }

  /** A spec's own markdown as what is drawn: `{ title, md, sections: [{ name, md, parts?, groups? }] }` — `md` the
   *  words before the first `##`; a section with a requirement in it has `groups` (`requirements`), any other
   *  its `parts`. */
  export function read(text) {
    let title = "", cur = null;
    const intro = [], sections = [];
    for (const { line, h } of walk(String(text ?? "").split("\n"))) {
      if (h && h[1].length === 1 && !title && !cur) { title = h[2]; continue; }
      if (h && h[1].length === 2) { sections.push(cur = { name: h[2], lines: [] }); continue; }
      (cur ? cur.lines : intro).push(line);
    }
    return {
      title, md: md(intro),
      sections: sections.map((s) => {
        const holds = [...walk(s.lines)].some((x) => x.h && REQ.test(x.h[2]));
        return holds ? { name: s.name, ...requirements(s.lines) } : { name: s.name, md: md(s.lines), parts: parts(s.lines) };
      }),
    };
  }

  /** A block-form tag's own words: its source between its open line and its close line. */
  export const inner = (source) => String(source ?? "").split("\n").slice(1, -1).join("\n");

  // What is open, by the ticket and the tag's place: an amend draws the tag again, and it opens where it was.
  const kept = new Map();
</script>

<script>
  import { tick } from "svelte";
  import { bus, live, markdown } from "plexus-ui/host";
  import TicketView from "./ticket-view.svelte";

  let { ticket = "", body = "", source = "", n = "0", editor = "" } = $props();
  const own = (inner(source).trim() ? inner(source) : body) || "";
  const holds = !!own.trim();
  const read_ = !holds && ticket ? live({ snapshot: "tickets.get", params: () => ({ id: ticket }), changes: "tickets.watch", key: "id",
                                          on: { amended: "upsert:ticket", updated: "upsert:ticket", revision_restored: "upsert:ticket" } }) : null;
  const row = $derived(read_?.rows.find((r) => r.body != null));
  const o = $derived(outline(holds ? own : row?.body));
  const spec = holds ? read(own) : null;
  // The questions the spec asks as inputs of its ticket's body, answered in place.
  const asked = holds ? spec.sections.flatMap((s) => (s.parts ?? []).filter((x) => x.input).map((x) => x.input)) : [];
  const b = asked.length && ticket ? bus() : null;
  const inputs = b ? live({ snapshot: "tickets.inputs.list", params: () => ({ ticket }), changes: "tickets.watch", key: "id", on: { input_changed: "upsert:input" } }) : null;
  const input = (id) => inputs?.rows.find((i) => i.id === id) ?? null;
  const waiting = $derived(asked.map(input).filter((i) => i?.state === "open"));
  const said = (v) => (v?.type === "choice" ? v.option : v?.type === "multi" ? (v.options.join(", ") || "none") : v?.text ?? "");
  let ticks = $state({}), texts = $state({}), whys = $state({}), refused = $state({});
  const heard = (i, a) => { refused[i.id] = a.ok ? "" : String(a.message ?? "refused").replace(/^Invalid params: /, ""); return a; };
  const answer = (i, value) => {
    const rationale = (whys[i.id] ?? "").trim();
    return b.call("tickets.inputs.answer", { ticket, input: i.id, value, ...(rationale ? { rationale } : {}) }).then((a) => { if (heard(i, a).ok) { whys[i.id] = ""; texts[i.id] = ""; } return a; });
  };
  const clear = (i) => b.call("tickets.inputs.clear", { ticket, input: i.id }).then((a) => heard(i, a));
  const ticked = (i, value) => (ticks[i.id] ?? i.answer?.value?.options ?? []).includes(value);
  const tickOne = (i, value) => { const had = ticks[i.id] ?? i.answer?.value?.options ?? []; ticks[i.id] = had.includes(value) ? had.filter((v) => v !== value) : [...had, value]; };
  // What is still asked: the open inputs once they are read, with the questions written as a plain list.
  const open_ = $derived(asked.length && inputs?.status === "live" ? [...waiting.map((i) => i.question ?? i.id), ...o.questions] : o.questions);

  const any = $derived(holds || o.requirements > 0 || o.tasks.total > 0 || o.questions.length > 0);

  const place = `${ticket}\u0000${n}`;
  const was = kept.get(place);
  let tab = $state(was?.tab != null && was.tab < (spec?.sections.length ?? 0) ? was.tab : 0);
  let open = $state(was?.open ?? {});
  $effect(() => { if (holds) kept.set(place, { tab, open: { ...open } }); });
  let root = $state(null);

  const plural = (count, word) => `${count} ${word}${count === 1 ? "" : "s"}`;
  const smooth = () => (matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth");
  const heading = (scope, text, skip) => {
    const want = String(text).trim();
    for (const h of scope.querySelectorAll("h1, h2, h3, h4, h5, h6")) {
      if (skip?.contains(h)) continue;
      const said = h.textContent.trim();
      if (said === want || said.endsWith(want)) return h;
    }
    return null;
  };

  /** Outline alone: the body's heading saying `text` (or ending in it), scrolled to. */
  function jump(text) {
    const scope = root?.closest("[data-surface=ticket-body], [data-surface=editor-body]") ?? document;
    heading(scope, text, root)?.scrollIntoView({ behavior: smooth(), block: "start" });
  }
  /** Holding the spec: the section with the open questions shown, and their heading scrolled to. */
  async function ask() {
    const first = waiting[0]?.id;
    const at = spec.sections.findIndex((s) => (first ? (s.parts ?? []).some((x) => x.input === first) : /^#{1,6}\s+open questions\s*$/im.test(s.md ?? "") || /^open questions$/i.test(s.name)));
    if (at < 0) return;
    tab = at;
    await tick();
    ((first && root?.querySelector(`[data-input="${CSS.escape(first)}"]`)) || heading(root, "Open Questions") || root)?.scrollIntoView({ behavior: smooth(), block: "nearest" });
  }
  const count = (s) => (s.groups ? s.groups.reduce((sum, g) => sum + g.reqs.length, 0) : /^tasks$/i.test(s.name) && o.tasks.total ? `${o.tasks.done}/${o.tasks.total}` : "");
  const keys = (s) => s.groups.flatMap((g) => g.reqs.map((r) => `${g.name}\u0000${r.title}`));
  const allOpen = (s) => keys(s).every((k) => open[k]);
  function openAll(s) { const to = !allOpen(s); for (const k of keys(s)) open[k] = to; }
  // In an editor a click on a drawn block enters its source; here it would turn the whole spec to markdown.
  // The focus of a click on its words is its own (`tabindex`), never the editor's around it: the editor, given
  // the focus, puts its caret at the document's start and scrolls there.
  const hold = (ev) => { if (editor === "true" && !ev.target?.closest?.("[data-edit]")) ev.stopPropagation(); };
</script>

{#snippet stats()}
  <p class="spec-stats">
    <span data-stat="requirements"><b>{o.requirements}</b> {o.requirements === 1 ? "requirement" : "requirements"}</span>
    <span data-stat="scenarios"><b>{o.scenarios}</b> {o.scenarios === 1 ? "scenario" : "scenarios"}</span>
    {#if o.tasks.total}<span data-stat="tasks"><b>{o.tasks.done}/{o.tasks.total}</b> tasks
      <span class="spec-bar" aria-hidden="true"><span style:width={`${(100 * o.tasks.done) / o.tasks.total}%`}></span></span></span>{/if}
    {#if open_.length}<span data-stat="questions" class="spec-open"><b>{open_.length}</b> open {open_.length === 1 ? "question" : "questions"}</span>{/if}
    {#if holds && editor === "true"}<span class="spec-edit" data-edit role="button" tabindex="0" title="Show this spec's markdown, to edit it">edit source</span>{/if}
  </p>
{/snippet}

{#snippet words(text)}{#if text}<div class="spec-md"><TicketView body={text}/></div>{/if}{/snippet}

{#snippet question(x)}
  {@const i = input(x.input)}
  {#if !i || i.state === "removed"}{@render words(x.md)}
  {:else}
  <article class="spec-q" data-input={i.id} data-kind={i.kind} data-state={i.state}>
    <p class="spec-q-ask" data-field="question">{i.question ?? i.id}</p>
    {#if i.state === "open" || i.state === "answered"}
      {#if i.kind === "choice"}
        <div class="spec-q-opts" role="group" aria-label={i.question ?? i.id}>
          {#each i.options as op (op.value)}
            <button type="button" class="spec-q-opt" data-option={op.value} data-recommended={op.recommended ? "true" : "false"} data-action="tickets.inputs.answer"
                    aria-pressed={i.answer?.value?.option === op.value ? "true" : "false"} onclick={() => answer(i, { type: "choice", option: op.value })}>{op.label}{#if op.recommended} <span class="spec-q-star">recommended</span>{/if}</button>
          {/each}
        </div>
      {:else if i.kind === "multi"}
        <div class="spec-q-opts">
          {#each i.options as op (op.value)}
            <label class="spec-q-tick"><input type="checkbox" data-option={op.value} checked={ticked(i, op.value)} onchange={() => tickOne(i, op.value)}/> {op.label}</label>
          {/each}
          <button type="button" class="spec-q-save" data-action="tickets.inputs.answer" onclick={() => answer(i, { type: "multi", options: ticks[i.id] ?? i.answer?.value?.options ?? [] })}>save</button>
        </div>
      {:else}
        <div class="spec-q-reply">
          <textarea name="reply" rows="2" spellcheck="false" placeholder="your answer" bind:value={texts[i.id]}></textarea>
          <button type="button" class="spec-q-save" data-action="tickets.inputs.answer" onclick={() => answer(i, { type: "text", text: texts[i.id] ?? "" })}>save</button>
        </div>
      {/if}
      {#if i.kind !== "text"}<input class="spec-q-why" name="why" autocomplete="off" placeholder="why (optional)" bind:value={whys[i.id]}/>{/if}
      {#if i.recommendation && i.state === "open"}<p class="spec-q-rec" data-field="recommendation">Recommended: {i.recommendation}</p>{/if}
    {/if}
    {#if i.answer}
      <p class="spec-q-said" data-field="answer"><b>{said(i.answer.value)}</b>{#if i.answer.rationale} — {i.answer.rationale}{/if}{#if i.answer.name} <span class="spec-q-who">· {i.answer.name}</span>{/if}
        {#if i.state === "answered"}<button type="button" class="spec-q-clear" data-action="tickets.inputs.clear" onclick={() => clear(i)}>clear</button>{/if}</p>
    {:else if i.state !== "open"}<p class="spec-q-said" data-field="state">{i.state}</p>{/if}
    {#if refused[i.id]}<p class="spec-q-refused" data-refused role="alert">{refused[i.id]}</p>{/if}
  </article>
  {/if}
{/snippet}

{#if any && holds}
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="spec" data-surface="spec-outline" data-holds aria-label={spec.title || "Spec"} bind:this={root} onmousedown={hold} tabindex="-1">
  {#if spec.title}<h2 class="spec-title">{spec.title}</h2>{/if}
  {@render stats()}
  {#if open_.length}
  <ul class="spec-questions">
    {#each open_ as q, i (i)}<li><button type="button" data-question onclick={ask}>{q}</button></li>{/each}
  </ul>
  {/if}
  {@render words(spec.md)}
  {#if spec.sections.length}
  <div class="spec-sections" role="tablist">
    {#each spec.sections as s, i (i)}
      <button type="button" role="tab" data-section={s.name} aria-selected={tab === i} onclick={() => (tab = i)}>{s.name}{#if count(s) !== ""} <span class="spec-n">{count(s)}</span>{/if}</button>
    {/each}
  </div>
  {#each spec.sections as s, i (i)}
    {#if tab === i}
    <div class="spec-panel" role="tabpanel" data-panel={s.name}>
      {#if s.groups}
        {@render words(s.md)}
        <p class="spec-tools"><button type="button" data-open-all onclick={() => openAll(s)}>{allOpen(s) ? "close all" : "open all"}</button></p>
        {#each s.groups as g, gi (gi)}
          {@const bare = g.reqs.filter((r) => !r.scenarios.length).length}
          <section class="spec-cap" data-group={g.name}>
            <h3><span class="spec-name">{g.name}</span> <span class="spec-count">{plural(g.reqs.length, "requirement")}{#if bare}, <span class="spec-warn">{bare} with no scenario</span>{/if}</span></h3>
            {@render words(g.md)}
            <ul class="spec-reqs">
              {#each g.reqs as r, ri (ri)}
                {@const k = `${g.name}\u0000${r.title}`}
                <li class:spec-on={open[k]}>
                  <button type="button" class="spec-req" data-req={r.title} data-bare={r.scenarios.length ? undefined : ""} aria-expanded={!!open[k]} onclick={() => (open[k] = !open[k])}>
                    <span class="spec-tri" aria-hidden="true"></span>
                    <span class="spec-name">{r.title}</span>
                    <span class="spec-count" class:spec-warn={!r.scenarios.length}>{r.scenarios.length ? plural(r.scenarios.length, "scenario") : "no scenario"}</span>
                  </button>
                  {#if open[k]}
                  <div class="spec-req-body" data-req-body>
                    {@render words(r.md)}
                    {#each r.scenarios as sc, si (si)}
                      <div class="spec-scn" data-scenario={sc.title}>
                        <p class="spec-scn-name">{sc.title}</p>
                        {#if sc.steps}
                          <dl>{#each sc.steps as st, ti (ti)}<div data-step={st.word}><dt>{st.word}</dt><dd>{@html markdown(st.text)}</dd></div>{/each}</dl>
                        {:else}{@render words(sc.md)}{/if}
                      </div>
                    {/each}
                  </div>
                  {/if}
                </li>
              {/each}
            </ul>
          </section>
        {/each}
      {:else if s.parts?.length}
        {#each s.parts as x, xi (xi)}{#if x.input}{@render question(x)}{:else}{@render words(x.md)}{/if}{/each}
      {:else}
        <p class="spec-none">Nothing here yet.</p>
      {/if}
    </div>
    {/if}
  {/each}
  {/if}
</section>
{:else if any}
<nav class="spec" data-surface="spec-outline" aria-label="Outline" bind:this={root}>
  {@render stats()}
  {#if o.sections.length}
  <p class="spec-sections">
    {#each o.sections as s (s)}<button type="button" data-section={s} onclick={() => jump(s)}>{s}</button>{/each}
  </p>
  {/if}
  {#if o.groups.length}
  <ul class="spec-groups">
    {#each o.groups as g (g.name)}
      {@const bare = g.reqs.filter((r) => !r.scenarios).length}
      <li>
        <button type="button" class="spec-group" data-group={g.name} aria-expanded={!!open[g.name]} onclick={() => (open[g.name] = !open[g.name])}>
          <span class="spec-tri" aria-hidden="true"></span>
          <span class="spec-name">{g.name}</span>
          <span class="spec-count">{plural(g.reqs.length, "requirement")}{#if bare}, <span class="spec-warn">{bare} with no scenario</span>{/if}</span>
        </button>
        {#if open[g.name]}
        <ul class="spec-reqs spec-in">
          {#each g.reqs as r (r.heading)}
            <li><button type="button" class="spec-req" data-req={r.title} data-bare={r.scenarios ? undefined : ""} onclick={() => jump(r.heading)}>
              <span class="spec-name spec-link">{r.title}</span>
              <span class="spec-count" class:spec-warn={!r.scenarios}>{r.scenarios ? plural(r.scenarios, "scenario") : "no scenario"}</span>
            </button></li>
          {/each}
        </ul>
        {/if}
      </li>
    {/each}
  </ul>
  {/if}
  {#if o.questions.length}
  <ul class="spec-questions">
    {#each o.questions as q, i (i)}<li><button type="button" data-question onclick={() => jump("Open Questions")}>{q}</button></li>{/each}
  </ul>
  {/if}
</nav>
{/if}

<style>
  .spec { display: grid; gap: 8px; padding: 12px 14px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); font-family: var(--font-text); min-width: 0; }
  .spec p, .spec ul, .spec dl, .spec dd, .spec h2, .spec h3 { margin: 0; padding: 0; list-style: none; }
  .spec button { font: inherit; color: inherit; background: none; border: 0; padding: 0; cursor: pointer; text-align: left; }
  .spec button:focus-visible, .spec button:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 2px; border-radius: var(--radius-xs); }
  section.spec { outline: none; }
  .spec-title { font-family: var(--font-display); font-size: 18px; font-weight: var(--weight-strong); color: var(--ink); }

  .spec-stats { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 18px; color: var(--muted); }
  .spec-stats b { color: var(--ink); font-weight: var(--weight-strong); font-variant-numeric: tabular-nums; }
  .spec-stats .spec-open b, .spec-stats .spec-open { color: var(--st-pending-fg); }
  .spec-bar { display: inline-block; vertical-align: middle; width: 56px; height: 4px; margin-left: 4px; background: var(--line); border-radius: var(--radius-pill); overflow: hidden; }
  .spec-bar span { display: block; height: 100%; background: var(--live); }
  .spec-edit { margin-left: auto; color: var(--muted); cursor: pointer; text-decoration: underline; }
  .spec-edit:hover, .spec-edit:global([data-sheet~=hover]) { color: var(--ink); }

  .spec-sections { display: flex; flex-wrap: wrap; gap: 4px 6px; }
  .spec .spec-sections button { padding: 1px 8px; color: var(--muted); border: var(--border) solid var(--line); border-radius: var(--radius-pill); }
  .spec .spec-sections button:hover, .spec .spec-sections button:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .spec .spec-sections button[aria-selected=true], .spec .spec-sections button:global([data-sheet~=selected]) { color: var(--ink); background: var(--sel); border-color: var(--focus); font-weight: var(--weight-medium); }
  .spec-n { margin-left: 5px; color: var(--muted); font-variant-numeric: tabular-nums; }

  .spec-panel { display: grid; gap: 10px; min-width: 0; border-top: var(--border) solid var(--line); padding-top: 10px; }
  .spec-md { min-width: 0; }
  .spec-none { color: var(--muted); }
  .spec-tools { display: flex; justify-content: flex-end; }
  .spec .spec-tools button { color: var(--muted); text-decoration: underline; }
  .spec .spec-tools button:hover, .spec .spec-tools button:global([data-sheet~=hover]) { color: var(--ink); }
  .spec-cap { display: grid; gap: 4px; min-width: 0; }
  .spec-cap h3 { display: flex; align-items: baseline; gap: 8px; padding: 0 4px; font-size: 13px; font-weight: inherit; color: var(--ink); }
  .spec-cap h3 .spec-name { font-weight: var(--weight-strong); }

  .spec-groups { border-top: var(--border) solid var(--line); padding-top: 6px; }
  .spec .spec-group, .spec .spec-req { display: flex; align-items: baseline; gap: 8px; width: 100%; padding: 3px 4px; border-radius: var(--radius-sm); }
  .spec .spec-group:hover, .spec .spec-req:hover, .spec .spec-group:global([data-sheet~=hover]), .spec .spec-req:global([data-sheet~=hover]) { background: var(--hover); }
  .spec-tri { flex: none; align-self: center; width: 0; height: 0; border-top: 4px solid transparent; border-bottom: 4px solid transparent; border-left: 6px solid var(--muted); transition: transform var(--motion-fast) var(--ease); }
  [aria-expanded=true] > .spec-tri { transform: rotate(90deg); }
  .spec-group .spec-name { font-weight: var(--weight-medium); }
  .spec-name { min-width: 0; }
  .spec-count { flex: none; margin-left: auto; color: var(--muted); font-variant-numeric: tabular-nums; }
  .spec-warn { color: var(--warn); }
  .spec .spec-in { padding-left: 14px; }
  .spec-link { color: var(--accent); }

  .spec-reqs > li { min-width: 0; border-radius: var(--radius-sm); }
  .spec-reqs > li.spec-on { background: var(--bg); border: var(--border) solid var(--line); margin: 2px 0; }
  .spec-on > .spec-req .spec-name { font-weight: var(--weight-medium); }
  .spec-req-body { display: grid; gap: 8px; min-width: 0; padding: 4px 12px 10px 18px; }
  .spec-scn { display: grid; gap: 2px; padding-left: 10px; border-left: 2px solid var(--line); }
  .spec-scn-name { color: var(--muted); font-size: 12px; }
  .spec-scn dl { display: grid; gap: 2px; }
  .spec-scn dl > div { display: grid; grid-template-columns: 52px minmax(0, 1fr); gap: 8px; align-items: baseline; }
  .spec-scn dt { font-size: 11px; font-weight: var(--weight-strong); letter-spacing: 0.04em; color: var(--muted); }
  .spec-scn [data-step=THEN] dt { color: var(--live); }
  .spec-scn dd :global(p) { margin: 0; }

  .spec-q { display: grid; gap: 6px; min-width: 0; padding: 10px 12px; background: var(--bg); border: var(--border) solid var(--line); border-left: 3px solid var(--st-pending-fg); border-radius: var(--radius-sm); }
  .spec-q[data-state=answered] { border-left-color: var(--live); }
  .spec-q-ask { font-weight: var(--weight-medium); color: var(--ink); }
  .spec-q-opts { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .spec .spec-q-opt, .spec .spec-q-save { padding: 3px 10px; border: var(--border) solid var(--line); border-radius: var(--radius-sm); background: var(--panel); }
  .spec .spec-q-opt:hover, .spec .spec-q-save:hover, .spec .spec-q-opt:global([data-sheet~=hover]), .spec .spec-q-save:global([data-sheet~=hover]) { background: var(--hover); }
  .spec .spec-q-opt[aria-pressed=true], .spec .spec-q-opt:global([data-sheet~=checked]) { background: var(--sel); border-color: var(--focus); font-weight: var(--weight-medium); }
  .spec-q-star { margin-left: 6px; color: var(--muted); font-size: 11px; }
  .spec-q-tick { display: inline-flex; align-items: center; gap: 4px; }
  .spec-q-reply { display: grid; grid-template-columns: minmax(0, 1fr) auto; gap: 6px; align-items: end; }
  .spec-q textarea, .spec-q-why { font: inherit; color: var(--ink); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-sm); padding: 4px 8px; min-width: 0; }
  .spec-q textarea:focus, .spec-q-why:focus, .spec-q textarea:global([data-sheet~=focus]), .spec-q-why:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .spec-q-rec, .spec-q-who { color: var(--muted); }
  .spec-q-said b { font-weight: var(--weight-strong); }
  .spec .spec-q-clear { margin-left: 8px; color: var(--muted); text-decoration: underline; }
  .spec .spec-q-clear:hover, .spec .spec-q-clear:global([data-sheet~=hover]) { color: var(--ink); }
  .spec-q-refused { color: var(--warn); }

  .spec-questions { display: grid; gap: 2px; }
  nav .spec-questions { border-top: var(--border) solid var(--line); padding-top: 6px; }
  .spec .spec-questions button { padding: 2px 4px; color: var(--st-pending-fg); border-radius: var(--radius-sm); }
  .spec .spec-questions button::before { content: "? "; font-weight: var(--weight-strong); }
  .spec .spec-questions button:hover, .spec .spec-questions button:global([data-sheet~=hover]) { background: var(--hover); }
  @media print { nav.spec { display: none; } }
</style>
