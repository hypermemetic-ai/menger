<!-- `<spec-outline>` in a ticket's body holds an OpenSpec change and draws it. Written in block form, its open
     line and its close line alone and the spec's markdown between them, it is the spec: a line of counts, its
     `##` sections as tabs, and the one chosen drawn under them. A section holding requirements draws each
     capability as a heading and each requirement as a line that opens to its statement, its code and its
     scenarios (a scenario's `- **WHEN** …` lines as steps); any other section is drawn as the body draws
     markdown (ticket-view: code cards, diagrams, tables), and a part of it holding tasks by spec-tasks, each
     box ticked in place. A requirement with no scenario is marked, since OpenSpec asks for at least one.
     What the body keeps outside the tag is what is not the spec: an overview, a diagram of how it fits.
     **Its parts are edited where they are**: `edit` on a part (or a double click on it) shows that part's
     markdown in a box — a section's words, a requirement with its heading, a scenario with its heading — and
     `save` (ctrl+enter) writes it back into the tag, alone; `cancel` (escape) leaves it. The tag is saved by
     the editor that drew it (`body.tag` on the bus, EDITOR-24: the editor replaces this tag in its text and
     saves the body), else by reading the ticket and amending it with this tag replaced, only while the body
     still holds the tag as it was drawn. A refusal is said under the box, `[data-refused]`.
     **A question** written as an input of the body (`<m-decision id>`, `<m-input id kind>`, block form) is
     drawn where it stands and answered in place: a choice's options (a click answers), a multi's boxes and
     a text's box with `save`, a why, what was answered and by whom, `clear` (`tickets.inputs.answer`,
     `tickets.inputs.clear`; live by `tickets.watch`'s `input_changed`). Until its row is read, or with no
     ticket, it is drawn as the body draws the mark. An open question is a line under the counts.
     **What builds it**: the ticket's children are read live (`tickets.list { parent }`). A requirement whose
     statement says `**Built by:** LPROTO-1, …` shows each as a link with its status on its line, and the
     children are listed under the counts, one that no requirement names marked `tied to no requirement`.
     Its words come from its own source (`source`, which a block-form tag is handed whole, so a fence's `<`
     stays as written), else `body` (how the sheet draws it; edits there change the drawing alone). Which tab
     and which requirements are open is kept for the page by the ticket and the tag's place. In an editor
     (`editor`) a click in it stays in it, and `edit source` enters the whole tag's markdown.
     Written empty (`<spec-outline></spec-outline>`) it is an outline of the ticket's whole body instead,
     read live: the counts, the sections and requirements as links that scroll the body to their headings.
     With no words, or no ticket known, it draws nothing.
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
     `[data-option=<value>]`, `[data-action]`, `[data-field=question|answer|recommendation]`),
     `[data-unit=<kind>]` with `[data-edit-unit]`, `[data-editing]` (its `textarea`, `[data-save]`,
     `[data-cancel]`, `[data-refused]`), `[data-built=<id>]` (`data-status`), `[data-kids]` with
     `[data-kid=<id>]` (`[data-untied]`). -->
<script module>
  export const uses = ["tickets.get", "tickets.watch", "tickets.list", "tickets.amend", "tickets.inputs.list", "tickets.inputs.answer", "tickets.inputs.clear"];

  const FENCE = /^ {0,3}(`{3,}|~{3,})/;
  const HEAD = /^(#{1,6})\s+(.+?)\s*#*\s*$/;
  const REQ = /^Requirement:\s*(.+)$/i;
  const SCN = /^Scenario:\s*(\S.*)$/i;
  const TASK = /^\s*[-*]\s+\[([ xX])\]\s+\S/;
  const ASK = /^<m-(decision|input)\b[^>]*\bid\s*=\s*(?:"([^"]+)"|'([^']+)'|([^\s>]+))/i;
  const BUILT = /\s*\*\*Built by:\*\*([^\n]*)/i;
  const ID = /[A-Z][A-Z0-9]*-\d+(?:\.\d+)*/g;

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
      const t = TASK.exec(line);
      if (t) { tasks.total++; if (t[1] !== " ") tasks.done++; continue; }
      const q = asking ? /^\s{0,3}(?:\d+[.)]|[-*])\s+(\S.*)$/.exec(line) : null;
      if (q) questions.push(lead(q[1]));
    }
    return { sections, groups, requirements: groups.reduce((n, g) => n + g.reqs.length, 0), scenarios, tasks, questions };
  }

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

  /** The tickets a statement says build it (`**Built by:** A-1, B-2`), and the statement without that. */
  export function built(md) {
    const m = BUILT.exec(md ?? "");
    return m ? { ids: [...new Set(m[1].match(ID) ?? [])], md: md.replace(BUILT, "").trim() } : { ids: [], md: md ?? "" };
  }

  /** A spec's own markdown as what is drawn, every part with its lines (`from` up to `to`, in `text`'s lines):
   *  `{ title, intro, sections }`. A section is `{ name, parts, end }` — each part `{ from, to, md }`, with
   *  `tasks` when it holds a task and `input` (its id) when it is an input of the body in block form — or,
   *  holding a requirement, `{ name, pre, groups: [{ name, intro, reqs: [{ title, from, to, md, ids,
   *  scenarios: [{ title, from, to, md, steps }] }] }] }`: a requirement's lines run from its heading to its
   *  first scenario, a scenario's from its heading; `md` is what follows the heading. A heading inside a
   *  requirement, deeper than it, is that requirement's words. */
  export function read(text) {
    const L = String(text ?? "").split("\n");
    const W = [...walk(L)];
    const tight = (from, to) => {
      while (from < to && !L[from].trim()) from++;
      while (to > from && !L[to - 1].trim()) to--;
      return { from, to, md: L.slice(from, to).join("\n") };
    };
    const parts = (from, end) => {
      const out = [];
      let run = from;
      const flush = (to) => { const u = tight(run, to); if (u.md) out.push({ ...u, tasks: L.slice(u.from, u.to).some((l, k) => !W[u.from + k].code && TASK.test(l)) }); };
      for (let i = from; i < end; i++) {
        const m = W[i].code ? null : ASK.exec(L[i]);
        if (!m) continue;
        const close = `</m-${m[1].toLowerCase()}>`;
        let j = i;
        while (j < end && !(L[j].includes(close) && !W[j].code)) j++;
        if (j >= end) break;
        flush(i);
        out.push({ from: i, to: j + 1, md: L.slice(i, j + 1).join("\n"), input: m[2] ?? m[3] ?? m[4] });
        run = j + 1;
        i = j;
      }
      flush(end);
      return out;
    };
    const requirements = (from, end) => {
      const marks = [];
      let inReq = false, level = 0;
      for (let i = from; i < end; i++) {
        const h = W[i].h;
        if (!h) continue;
        const r = REQ.exec(h[2]), s = SCN.exec(h[2]);
        if (r) { marks.push({ i, kind: "req", title: r[1] }); inReq = true; level = h[1].length; }
        else if (s && inReq) marks.push({ i, kind: "scn", title: s[1] });
        else if (!(inReq && h[1].length > level)) { marks.push({ i, kind: "group", title: h[2] }); inReq = false; }
      }
      const groups = [];
      let group = null, req = null;
      marks.forEach((m, k) => {
        const next = k + 1 < marks.length ? marks[k + 1].i : end;
        const own = tight(m.i, next), after = tight(m.i + 1, next);
        if (m.kind === "group") { groups.push(group = { name: m.title, intro: after, reqs: [] }); req = null; }
        else if (m.kind === "req") {
          if (!group) groups.push(group = { name: "Requirements", intro: { from: m.i, to: m.i, md: "" }, reqs: [] });
          const b = built(after.md);
          group.reqs.push(req = { title: m.title, from: own.from, to: own.to, md: b.md, ids: b.ids, scenarios: [] });
        } else req?.scenarios.push({ title: m.title, from: own.from, to: own.to, md: after.md, steps: steps(L.slice(after.from, after.to)) });
      });
      return { pre: tight(from, marks.length ? marks[0].i : end), groups };
    };
    let title = "", titleAt = -1;
    const heads = [];
    W.forEach((x, i) => {
      if (!x.h) return;
      if (x.h[1].length === 1 && !title && !heads.length) { title = x.h[2]; titleAt = i; }
      else if (x.h[1].length === 2) heads.push(i);
    });
    return {
      title,
      intro: tight(titleAt + 1, heads.length ? heads[0] : L.length),
      sections: heads.map((at, k) => {
        const end = k + 1 < heads.length ? heads[k + 1] : L.length;
        let holds = false;
        for (let i = at + 1; i < end && !holds; i++) holds = !!W[i].h && REQ.test(W[i].h[2]);
        return holds ? { name: W[at].h[2], ...requirements(at + 1, end) } : { name: W[at].h[2], parts: parts(at + 1, end), end };
      }),
    };
  }

  /** `text` with its lines `from` up to `to` replaced by `next` (none: taken out), a blank line kept each side. */
  export function spliced(text, from, to, next) {
    const L = String(text ?? "").split("\n");
    const trimmed = String(next ?? "").replace(/^\n+|\s+$/g, "");
    const mid = trimmed ? trimmed.split("\n") : [];
    const before = L.slice(0, from), after = L.slice(to);
    if (mid.length) {
      if (before.length && before[before.length - 1].trim()) mid.unshift("");
      if (after.length && after[0].trim()) mid.push("");
    }
    return [...before, ...mid, ...after].join("\n");
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
  import SpecTasks from "./spec-tasks.svelte";
  import { tagSpans, wroteOwn } from "./tag-source.svelte";
  import { ticketHref } from "./ticket-links.svelte";

  let { ticket = "", body = "", source = "", n = "0", editor = "" } = $props();
  const tagged = !!inner(source).trim();
  const first = (tagged ? inner(source) : body) || "";
  const holds = !!first.trim();
  // The spec's markdown now, and the tag as the body holds it (what a save must still find there).
  let text = $state(first);
  let drawnFrom = source;
  const saves = tagged && !!ticket;
  const b = holds && ticket ? bus() : null;

  const read_ = !holds && ticket ? live({ snapshot: "tickets.get", params: () => ({ id: ticket }), changes: "tickets.watch", key: "id",
                                          on: { amended: "upsert:ticket", updated: "upsert:ticket", revision_restored: "upsert:ticket" } }) : null;
  const row = $derived(read_?.rows.find((r) => r.body != null));
  const o = $derived(outline(holds ? text : row?.body));
  const spec = $derived(holds ? read(text) : null);
  const any = $derived(holds || o.requirements > 0 || o.tasks.total > 0 || o.questions.length > 0);

  // The questions the spec asks as inputs of its ticket's body, answered in place.
  const asked = $derived(holds ? spec.sections.flatMap((s) => (s.parts ?? []).filter((x) => x.input).map((x) => x.input)) : []);
  const inputs = b ? live({ snapshot: "tickets.inputs.list", params: () => ({ ticket }), changes: "tickets.watch", key: "id", on: { input_changed: "upsert:input" }, when: () => asked.length > 0 }) : null;
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

  // What builds it: the ticket's children, live; a requirement names the ones that build it.
  const kids = b ? live({ snapshot: "tickets.list", params: () => ({ parent: ticket, summary: true, limit: 200 }), changes: "tickets.watch", key: "id",
                          on: { created: "upsert:ticket", status_changed: "upsert:ticket", updated: "upsert:ticket", deleted: "remove:ids" } }) : null;
  const kid = (id) => kids?.rows.find((k) => k.id === id) ?? null;
  const tied = $derived(new Set(holds ? spec.sections.flatMap((s) => (s.groups ?? []).flatMap((g) => g.reqs.flatMap((r) => r.ids))) : []));
  const children = $derived((kids?.rows ?? []).filter((k) => k.id != null && k.kind !== "doc"));

  const place = `${ticket}\u0000${n}`;
  const was = kept.get(place);
  let tab = $state(was?.tab ?? 0);
  let open = $state(was?.open ?? {});
  $effect(() => { if (holds) kept.set(place, { tab, open: { ...open } }); });
  $effect(() => { if (holds && tab >= spec.sections.length) tab = 0; });
  let root = $state(null);

  // ── editing a part where it is ───────────────────────────────────────────
  let edit = $state(null);   // { from, to, value, error }
  let saving = $state(false);
  let flash = $state("");
  const editingAt = (u) => edit != null && edit.from === u.from && edit.to === u.to;
  function begin(u) {
    if (saving) return;
    edit = { from: u.from, to: u.to, value: text.split("\n").slice(u.from, u.to).join("\n"), error: "" };
    tick().then(() => root?.querySelector("[data-editing] textarea")?.focus());
  }
  let asks = 0;
  /** The save through the editor that drew this: it replaces the tag in its text and saves the body. */
  function throughEditor(next) {
    const token = `spec-${Math.random().toString(36).slice(2)}-${++asks}`;
    return new Promise((resolve) => {
      let off = null;
      const end = (r) => { clearTimeout(late); off?.(); resolve(r); };
      const late = setTimeout(() => end({ ok: false, message: "the editor did not answer: nothing saved" }), 15000);
      off = b.on("body.tag.done", (d) => { if (d?.token === token) end(d); });
      b.emit("body.tag", { kind: "ticket", id: ticket, name: "spec-outline", n: Number(n), was: drawnFrom, next, token });
    });
  }
  /** The save with no editor: the ticket read, this tag replaced in its body while it is still as drawn, amended. */
  async function direct(next) {
    const r = await b.call("tickets.get", { id: ticket });
    const t = r?.ok ? r.events?.find((e) => e?.type === "ticket")?.ticket : null;
    if (!t) return { ok: false, message: r?.message ?? `no ticket ${ticket}` };
    const s = tagSpans(t.body ?? "").filter((x) => x.name === "spec-outline")[Number(n)];
    if (!s) return { ok: false, message: "the body no longer holds this spec: nothing saved" };
    if (s.text !== drawnFrom) return { ok: false, message: "this was changed since it was drawn (someone else's edit): nothing saved" };
    wroteOwn(next);
    const w = await b.call("tickets.amend", { id: t.id, body: t.body.slice(0, s.from) + next + t.body.slice(s.to) });
    const bad = w?.events?.find((e) => e?.type === "error");
    return w?.ok && !bad ? { ok: true } : { ok: false, message: bad?.message ?? w?.message ?? "refused" };
  }
  /** The spec's markdown made `next`, and kept in its tag when it sits in a ticket's body. */
  async function persist(next) {
    if (!saves) { text = next; return { ok: true }; }
    const lines = drawnFrom.split("\n");
    const whole = [lines[0], ...next.split("\n"), lines[lines.length - 1]].join("\n");
    saving = true;
    try {
      let r;
      try { r = editor === "true" ? await throughEditor(whole) : await direct(whole); }
      catch (e) { r = { ok: false, message: e?.message ?? String(e) }; }
      if (r.ok) { drawnFrom = whole; text = next; }
      return r;
    } finally { saving = false; }
  }
  async function commit() {
    if (!edit || saving) return;
    const r = await persist(spliced(text, edit.from, edit.to, edit.value));
    if (r.ok) edit = null; else edit.error = String(r.message ?? "refused").replace(/^Invalid params: /, "");
  }
  /** A part's markdown made `next` with no box shown (a task ticked). */
  async function replace(u, next) {
    const r = await persist(spliced(text, u.from, u.to, next));
    flash = r.ok ? "" : String(r.message ?? "refused");
  }
  const editKey = (ev) => {
    if (ev.key === "Escape") { ev.preventDefault(); edit = null; }
    else if (ev.key === "Enter" && (ev.ctrlKey || ev.metaKey)) { ev.preventDefault(); commit(); }
  };

  const plural = (count, word) => `${count} ${word}${count === 1 ? "" : "s"}`;
  const smooth = () => (matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth");
  const heading = (scope, want_, skip) => {
    const want = String(want_).trim();
    for (const h of scope.querySelectorAll("h1, h2, h3, h4, h5, h6")) {
      if (skip?.contains(h)) continue;
      const says = h.textContent.trim();
      if (says === want || says.endsWith(want)) return h;
    }
    return null;
  };

  /** Outline alone: the body's heading saying `text` (or ending in it), scrolled to. */
  function jump(want) {
    const scope = root?.closest("[data-surface=ticket-body], [data-surface=editor-body]") ?? document;
    heading(scope, want, root)?.scrollIntoView({ behavior: smooth(), block: "start" });
  }
  /** Holding the spec: the section with the open questions shown, and the first of them scrolled to. */
  async function ask() {
    const id = waiting[0]?.id;
    const at = spec.sections.findIndex((s) => (id ? (s.parts ?? []).some((x) => x.input === id) : (s.parts ?? []).some((x) => /^#{1,6}\s+open questions\s*$/im.test(x.md)) || /^open questions$/i.test(s.name)));
    if (at < 0) return;
    tab = at;
    await tick();
    ((id && root?.querySelector(`[data-input="${CSS.escape(id)}"]`)) || heading(root, "Open Questions") || root)?.scrollIntoView({ behavior: smooth(), block: "nearest" });
  }
  const count = (s) => (s.groups ? s.groups.reduce((sum, g) => sum + g.reqs.length, 0) : /^tasks$/i.test(s.name) && o.tasks.total ? `${o.tasks.done}/${o.tasks.total}` : "");
  const keys = (s) => s.groups.flatMap((g) => g.reqs.map((r) => `${g.name}\u0000${r.title}`));
  const allOpen = (s) => keys(s).every((k) => open[k]);
  function openAll(s) { const to = !allOpen(s); for (const k of keys(s)) open[k] = to; }
  // In an editor a click on a drawn block enters its source; here it would turn the whole spec to markdown.
  // The focus of a click on its words is its own (`tabindex`), never the editor's around it: the editor, given
  // the focus, puts its caret at the document's start and scrolls there.
  const hold = (ev) => { if (editor === "true" && !ev.target?.closest?.("[data-edit]")) ev.stopPropagation(); };
  const typed = (ev) => { if (editor === "true") ev.stopPropagation(); };
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

{#snippet words(md)}{#if md}<div class="spec-md"><TicketView body={md}/></div>{/if}{/snippet}

{#snippet box()}
  <div class="spec-box" data-editing>
    <textarea rows={Math.min(40, Math.max(3, edit.value.split("\n").length + 1))} spellcheck="false" bind:value={edit.value} onkeydown={editKey} aria-label="This part's markdown"></textarea>
    <p class="spec-box-tools">
      <button type="button" class="spec-q-save" data-save disabled={saving} onclick={commit}>{saving ? "saving…" : "save"}</button>
      <button type="button" class="spec-pen" data-cancel onclick={() => (edit = null)}>cancel</button>
      <span class="spec-hint">ctrl+enter saves · escape cancels</span>
    </p>
    {#if edit.error}<p class="spec-q-refused" data-refused role="alert">{edit.error}</p>{/if}
  </div>
{/snippet}

{#snippet pen(u)}<button type="button" class="spec-pen" data-edit-unit title="Edit this part's markdown" onclick={() => begin(u)}>edit</button>{/snippet}

{#snippet part(u, kind)}
  {#if editingAt(u)}{@render box()}
  {:else if u.md || kind === "empty"}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="spec-unit" data-unit={kind} ondblclick={() => begin(u)}>
      {@render pen(u)}
      {#if kind === "tasks"}<SpecTasks text={u.md} onchange={(next) => replace(u, next)}/>
      {:else if kind === "empty"}<p class="spec-none">Nothing here yet.</p>
      {:else}{@render words(u.md)}{/if}
    </div>
  {/if}
{/snippet}

{#snippet question(x)}
  {@const i = input(x.input)}
  {#if !i || i.state === "removed"}{@render part(x, "words")}
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

{#snippet chip(id)}
  {@const k = kid(id)}
  <a class="spec-built tpill" data-built={id} data-status={k?.status ?? ""} href={ticketHref(k ?? { id })} title={k?.title ?? id}>{id}{#if k?.status}<span class="spec-status">{k.status}</span>{/if}</a>
{/snippet}

{#if any && holds}
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<section class="spec" data-surface="spec-outline" data-holds aria-label={spec.title || "Spec"} bind:this={root} onmousedown={hold} onkeydown={typed} tabindex="-1">
  {#if spec.title}<h2 class="spec-title">{spec.title}</h2>{/if}
  {@render stats()}
  {#if open_.length}
  <ul class="spec-questions">
    {#each open_ as q, i (i)}<li><button type="button" data-question onclick={ask}>{q}</button></li>{/each}
  </ul>
  {/if}
  {#if children.length}
  <ul class="spec-kids" data-kids aria-label="The tickets under this one">
    {#each children as k (k.id)}
      <li data-kid={k.id} data-untied={tied.has(k.id) ? undefined : ""}>
        <a class="spec-built tpill" data-status={k.status ?? ""} href={ticketHref(k)}>{k.id}<span class="spec-status">{k.status}</span></a>
        <span class="spec-kid-title">{k.title}</span>
        <span class="spec-kid-kind">{k.kind}</span>
        {#if !tied.has(k.id)}<span class="spec-warn">tied to no requirement</span>{/if}
      </li>
    {/each}
  </ul>
  {/if}
  {#if flash}<p class="spec-q-refused" data-refused role="alert">{flash}</p>{/if}
  {@render part(spec.intro, "words")}
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
        {@render part(s.pre, "words")}
        <p class="spec-tools"><button type="button" data-open-all onclick={() => openAll(s)}>{allOpen(s) ? "close all" : "open all"}</button></p>
        {#each s.groups as g, gi (gi)}
          {@const bare = g.reqs.filter((r) => !r.scenarios.length).length}
          <section class="spec-cap" data-group={g.name}>
            <h3><span class="spec-name">{g.name}</span> <span class="spec-count">{plural(g.reqs.length, "requirement")}{#if bare}, <span class="spec-warn">{bare} with no scenario</span>{/if}</span></h3>
            {@render part(g.intro, "words")}
            <ul class="spec-reqs">
              {#each g.reqs as r, ri (ri)}
                {@const k = `${g.name}\u0000${r.title}`}
                <li class:spec-on={open[k]}>
                  <div class="spec-req-line">
                    <button type="button" class="spec-req" data-req={r.title} data-bare={r.scenarios.length ? undefined : ""} aria-expanded={!!open[k]} onclick={() => (open[k] = !open[k])}>
                      <span class="spec-tri" aria-hidden="true"></span>
                      <span class="spec-name">{r.title}</span>
                      <span class="spec-count" class:spec-warn={!r.scenarios.length}>{r.scenarios.length ? plural(r.scenarios.length, "scenario") : "no scenario"}</span>
                    </button>
                    {#if r.ids.length}<span class="spec-builts" aria-label="Built by">{#each r.ids as id (id)}{@render chip(id)}{/each}</span>{/if}
                  </div>
                  {#if open[k]}
                  <div class="spec-req-body" data-req-body>
                    {#if editingAt(r)}{@render box()}
                    {:else}
                      <!-- svelte-ignore a11y_no_static_element_interactions -->
                      <div class="spec-unit" data-unit="requirement" ondblclick={() => begin(r)}>{@render pen(r)}{@render words(r.md)}</div>
                    {/if}
                    {#each r.scenarios as sc, si (si)}
                      {#if editingAt(sc)}{@render box()}
                      {:else}
                      <!-- svelte-ignore a11y_no_static_element_interactions -->
                      <div class="spec-scn spec-unit" data-unit="scenario" data-scenario={sc.title} ondblclick={() => begin(sc)}>
                        {@render pen(sc)}
                        <p class="spec-scn-name">{sc.title}</p>
                        {#if sc.steps}
                          <dl>{#each sc.steps as st, ti (ti)}<div data-step={st.word}><dt>{st.word}</dt><dd>{@html markdown(st.text)}</dd></div>{/each}</dl>
                        {:else}{@render words(sc.md)}{/if}
                      </div>
                      {/if}
                    {/each}
                  </div>
                  {/if}
                </li>
              {/each}
            </ul>
          </section>
        {/each}
      {:else if s.parts.length}
        {#each s.parts as x, xi (xi)}{#if x.input}{@render question(x)}{:else}{@render part(x, x.tasks ? "tasks" : "words")}{/if}{/each}
      {:else}
        {@render part({ from: s.end, to: s.end, md: "" }, "empty")}
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

  .spec-unit { position: relative; min-width: 0; }
  .spec .spec-pen { color: var(--muted); text-decoration: underline; font-size: 12px; }
  .spec .spec-pen:hover, .spec .spec-pen:global([data-sheet~=hover]) { color: var(--ink); }
  .spec-unit > .spec-pen { position: absolute; top: 0; right: 0; z-index: 1; padding: 0 4px; background: var(--panel); border-radius: var(--radius-xs); }
  .spec-on .spec-unit > .spec-pen { background: var(--bg); }
  .spec-box { display: grid; gap: 6px; min-width: 0; }
  .spec-box textarea { width: 100%; box-sizing: border-box; resize: vertical; font-family: var(--font-text); font-size: 13px; line-height: 1.5; color: var(--ink); background: var(--panel); border: var(--border) solid var(--focus); border-radius: var(--radius-sm); padding: 8px 10px; tab-size: 2; }
  .spec-box textarea:focus, .spec-box textarea:global([data-sheet~=focus]) { outline: 2px solid var(--sel); outline-offset: 0; }
  .spec-box-tools { display: flex; align-items: center; gap: 12px; }
  .spec-hint { color: var(--muted); font-size: 12px; }

  .spec-req-line { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  .spec-req-line > .spec-req { flex: 1; min-width: 0; width: auto; }
  .spec-builts { flex: none; display: inline-flex; flex-wrap: wrap; gap: 4px; padding-right: 4px; }
  .spec-built { display: inline-flex; align-items: baseline; gap: 5px; padding: 0 7px; font-size: 12px; line-height: 18px; color: var(--accent); text-decoration: none; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-pill); font-variant-numeric: tabular-nums; }
  .spec-built:hover, .spec-built:global([data-sheet~=hover]) { background: var(--hover); }
  .spec-built:focus-visible, .spec-built:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .spec-status { color: var(--muted); }
  .spec-built[data-status=Complete] .spec-status { color: var(--live); }
  .spec-kids { display: grid; gap: 3px; padding-top: 6px; border-top: var(--border) solid var(--line); }
  .spec-kids li { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  .spec-kid-title { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .spec-kid-kind { flex: none; color: var(--muted); font-size: 12px; }

  .spec-questions { display: grid; gap: 2px; }
  nav .spec-questions { border-top: var(--border) solid var(--line); padding-top: 6px; }
  .spec .spec-questions button { padding: 2px 4px; color: var(--st-pending-fg); border-radius: var(--radius-sm); }
  .spec .spec-questions button::before { content: "? "; font-weight: var(--weight-strong); }
  .spec .spec-questions button:hover, .spec .spec-questions button:global([data-sheet~=hover]) { background: var(--hover); }
  @media print { nav.spec { display: none; } }
</style>
