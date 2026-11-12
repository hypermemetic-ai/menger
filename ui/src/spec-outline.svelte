<!-- `<spec-outline></spec-outline>` in a ticket's body draws that body's outline, read as an OpenSpec change:
     how many requirements, scenarios, tasks and open questions it holds, its sections as links, and each
     group of requirements as a line that opens to list them. A click on a section or a requirement scrolls
     the body to its heading. A requirement with no scenario is marked, since OpenSpec asks for at least one.
     It is placed by its tag (a native tag is handed the `ticket` it sits in), reads that ticket's body and
     follows its changes, and writes nothing. A tag written with words of its own outlines those instead
     (`body`, which is how the sheet draws it). In a document with no words, or with no ticket known, it
     draws nothing.
     What it reads from the body (`outline`):
     - a heading `Requirement: <name>` is a requirement, grouped under the nearest heading before it that
       is neither a requirement nor a scenario (a capability's name, or `Requirements`);
     - a heading `Scenario: <name>` belongs to the requirement before it;
     - a `- [ ]` or `- [x]` line is a task;
     - a list item under the heading `Open Questions` is a question;
     - a `##` heading is a section. Fenced code is skipped.
     Marks: `[data-surface=spec-outline]`, `[data-stat=requirements|scenarios|tasks|questions]`,
     `[data-section]`, `[data-group=<name>]` (`[aria-expanded]`), `[data-req=<name>]` (`[data-bare]` when it
     has no scenario), `[data-question]`. -->
<script module>
  export const uses = ["tickets.get", "tickets.watch"];

  /** A question's first words: its bold lead, else its first sentence. */
  const lead = (text) => (/^\*\*(.+?)\*\*/.exec(text)?.[1] ?? /^[^.?!]*[.?!]?/.exec(text)?.[0] ?? text).replace(/[.:]$/, "").trim();

  /** `body` read as an OpenSpec change: `{ sections, groups: [{ name, reqs: [{ title, heading, scenarios }] }],
   *  requirements, scenarios, tasks: { done, total }, questions }`. */
  export function outline(body) {
    const sections = [], groups = [], questions = [];
    const tasks = { done: 0, total: 0 };
    let fence = null, group = null, req = null, last = "", asking = false, scenarios = 0;
    for (const line of String(body ?? "").split("\n")) {
      const f = /^ {0,3}(`{3,}|~{3,})/.exec(line);
      if (f) { fence = fence ? (f[1][0] === fence ? null : fence) : f[1][0]; continue; }
      if (fence) continue;
      const h = /^(#{1,6})\s+(.+?)\s*#*\s*$/.exec(line);
      if (h) {
        const text = h[2];
        const r = /^Requirement:\s*(.+)$/i.exec(text);
        asking = /^open questions$/i.test(text);
        if (r) {
          if (!group || group.name !== last) groups.push(group = { name: last || "Requirements", reqs: [] });
          group.reqs.push(req = { title: r[1], heading: text, scenarios: 0 });
        } else if (/^Scenario:\s*\S/i.test(text)) {
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
</script>

<script>
  import { live } from "plexus-ui/host";

  let { ticket = "", body = "" } = $props();
  const id = () => (body ? "" : ticket);
  const read = id() ? live({ snapshot: "tickets.get", params: () => ({ id: ticket }), changes: "tickets.watch", key: "id",
                             on: { amended: "upsert:ticket", updated: "upsert:ticket", revision_restored: "upsert:ticket" } }) : null;
  const row = $derived(read?.rows.find((r) => r.body != null));
  const o = $derived(outline(body || row?.body));
  const any = $derived(o.requirements > 0 || o.tasks.total > 0 || o.questions.length > 0);

  let open = $state({});
  let root = $state(null);

  /** The body's heading saying `text` (or ending in it), scrolled to. */
  function jump(text) {
    const scope = root?.closest("[data-surface=ticket-body], [data-surface=editor-body]") ?? document;
    const want = String(text).trim();
    for (const h of scope.querySelectorAll("h1, h2, h3, h4, h5, h6")) {
      if (root?.contains(h)) continue;
      const said = h.textContent.trim();
      if (said !== want && !said.endsWith(want)) continue;
      h.scrollIntoView({ behavior: matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth", block: "start" });
      return;
    }
  }
  const plural = (n, word) => `${n} ${word}${n === 1 ? "" : "s"}`;
</script>

{#if any}
<nav class="spec" data-surface="spec-outline" aria-label="Outline" bind:this={root}>
  <p class="spec-stats">
    <span data-stat="requirements"><b>{o.requirements}</b> {o.requirements === 1 ? "requirement" : "requirements"}</span>
    <span data-stat="scenarios"><b>{o.scenarios}</b> {o.scenarios === 1 ? "scenario" : "scenarios"}</span>
    {#if o.tasks.total}<span data-stat="tasks"><b>{o.tasks.done}/{o.tasks.total}</b> tasks
      <span class="spec-bar" aria-hidden="true"><span style:width={`${(100 * o.tasks.done) / o.tasks.total}%`}></span></span></span>{/if}
    {#if o.questions.length}<span data-stat="questions" class="spec-open"><b>{o.questions.length}</b> open {o.questions.length === 1 ? "question" : "questions"}</span>{/if}
  </p>
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
        <ul class="spec-reqs">
          {#each g.reqs as r (r.heading)}
            <li><button type="button" data-req={r.title} data-bare={r.scenarios ? undefined : ""} onclick={() => jump(r.heading)}>
              <span class="spec-name">{r.title}</span>
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
  .spec { display: grid; gap: 8px; padding: 12px 14px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius); font-family: var(--font-text); }
  .spec p, .spec ul { margin: 0; padding: 0; list-style: none; }
  .spec button { font: inherit; color: inherit; background: none; border: 0; padding: 0; cursor: pointer; text-align: left; }
  .spec button:focus-visible, .spec button:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 2px; border-radius: var(--radius-xs); }

  .spec-stats { display: flex; flex-wrap: wrap; align-items: center; gap: 4px 18px; color: var(--muted); }
  .spec-stats b { color: var(--ink); font-weight: var(--weight-strong); font-variant-numeric: tabular-nums; }
  .spec-stats .spec-open b, .spec-stats .spec-open { color: var(--st-pending-fg); }
  .spec-bar { display: inline-block; vertical-align: middle; width: 56px; height: 4px; margin-left: 4px; background: var(--line); border-radius: var(--radius-pill); overflow: hidden; }
  .spec-bar span { display: block; height: 100%; background: var(--live); }

  .spec-sections { display: flex; flex-wrap: wrap; gap: 4px 6px; }
  .spec .spec-sections button { padding: 1px 8px; color: var(--muted); border: var(--border) solid var(--line); border-radius: var(--radius-pill); }
  .spec .spec-sections button:hover, .spec .spec-sections button:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }

  .spec-groups { border-top: var(--border) solid var(--line); padding-top: 6px; }
  .spec .spec-group, .spec .spec-reqs button { display: flex; align-items: baseline; gap: 8px; width: 100%; padding: 3px 4px; border-radius: var(--radius-sm); }
  .spec .spec-group:hover, .spec .spec-reqs button:hover, .spec .spec-group:global([data-sheet~=hover]), .spec .spec-reqs button:global([data-sheet~=hover]) { background: var(--hover); }
  .spec-tri { flex: none; align-self: center; width: 0; height: 0; border-top: 4px solid transparent; border-bottom: 4px solid transparent; border-left: 6px solid var(--muted); transition: transform var(--motion-fast) var(--ease); }
  [aria-expanded=true] > .spec-tri { transform: rotate(90deg); }
  .spec-group .spec-name { font-weight: var(--weight-medium); }
  .spec-name { min-width: 0; }
  .spec-count { flex: none; margin-left: auto; color: var(--muted); font-variant-numeric: tabular-nums; }
  .spec-warn { color: var(--warn); }
  .spec .spec-reqs { padding-left: 14px; }
  .spec-reqs .spec-name { color: var(--accent); }

  .spec-questions { display: grid; gap: 2px; border-top: var(--border) solid var(--line); padding-top: 6px; }
  .spec .spec-questions button { padding: 2px 4px; color: var(--st-pending-fg); border-radius: var(--radius-sm); }
  .spec .spec-questions button::before { content: "? "; font-weight: var(--weight-strong); }
  .spec .spec-questions button:hover, .spec .spec-questions button:global([data-sheet~=hover]) { background: var(--hover); }
  @media print { .spec { display: none; } }
</style>
