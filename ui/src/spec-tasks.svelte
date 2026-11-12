<!-- A task list written in markdown, drawn as tasks: how many are done, and each `- [ ]` / `- [x]` line a box
     that is ticked in place. A sibling of spec-outline, which places it for the part of a spec that holds its
     tasks; it calls nothing. `text` is the markdown; `onchange(next)` is told the whole markdown with one line
     changed (a box ticked or cleared, a task added at the end), and with none given the boxes only show.
     A task's leading number (`1.`) is drawn as its number; an indented task sits under the one before it; any
     other line is drawn as markdown where it stands. Fenced code holds no task.
     Marks: `[data-surface=spec-tasks]` (`data-done`, `data-total`), `[data-task=<line>]` (`[aria-checked]`),
     `[data-form=add]`. -->
<script module>
  export const uses = [];

  const FENCE = /^ {0,3}(`{3,}|~{3,})/;
  const TASK = /^(\s*)([-*])\s+\[([ xX])\]\s+(\S.*)$/;

  /** `text` as rows, in order: a task `{ i, indent, done, num, label }` (`i` its line) or words `{ md }`; with
   *  `done` and `total`. */
  export function tasks(text) {
    const lines = String(text ?? "").split("\n");
    const rows = [];
    let done = 0, total = 0, fence = null, run = [];
    const flush = () => { const md = run.join("\n").replace(/^\n+|\s+$/g, ""); if (md) rows.push({ md }); run = []; };
    lines.forEach((line, i) => {
      const f = FENCE.exec(line);
      if (f) { fence = fence ? (f[1][0] === fence ? null : fence) : f[1][0]; run.push(line); return; }
      const t = fence ? null : TASK.exec(line);
      if (!t) { run.push(line); return; }
      flush();
      const n = /^(\d+)[.)]\s+(.*)$/.exec(t[4]);
      const row = { i, indent: t[1].replace(/\t/g, "  ").length, done: t[3] !== " ", num: n ? n[1] : "", label: n ? n[2] : t[4] };
      total++;
      if (row.done) done++;
      rows.push(row);
    });
    flush();
    return { rows, done, total };
  }

  /** `text` with the box of line `i` ticked, or cleared when it was. */
  export function toggled(text, i) {
    const lines = String(text ?? "").split("\n");
    const t = TASK.exec(lines[i] ?? "");
    if (t) lines[i] = `${t[1]}${t[2]} [${t[3] === " " ? "x" : " "}] ${t[4]}`;
    return lines.join("\n");
  }

  /** `text` with a task added after its last one (at its end when it holds none), numbered when they are. */
  export function added(text, label) {
    const lines = String(text ?? "").replace(/\s+$/, "").split("\n");
    const { rows } = tasks(text);
    const last = rows.filter((r) => r.i != null).pop();
    const num = last?.num ? `${Number(last.num) + 1}. ` : "";
    const line = `- [ ] ${num}${String(label).trim()}`;
    if (last) lines.splice(last.i + 1, 0, line); else lines.push(...(lines.at(-1)?.trim() ? ["", line] : [line]));
    return lines.join("\n");
  }
</script>

<script>
  import { markdown } from "plexus-ui/host";

  let { text = "", onchange = null } = $props();
  const read = $derived(tasks(text));
  let fresh = $state("");
  function add(ev) {
    ev.preventDefault();
    if (!fresh.trim() || !onchange) return;
    onchange(added(text, fresh));
    fresh = "";
  }
</script>

<section class="tasks" data-surface="spec-tasks" data-done={read.done} data-total={read.total}>
  {#if read.total}
  <p class="tasks-count"><b>{read.done}/{read.total}</b> done
    <span class="tasks-bar" aria-hidden="true"><span style:width={`${(100 * read.done) / read.total}%`}></span></span></p>
  {/if}
  {#each read.rows as r, k (k)}
    {#if r.md != null}
      <div class="tasks-md">{@html markdown(r.md)}</div>
    {:else}
      <div class="tasks-row" class:tasks-done={r.done} style:padding-left={`${Math.min(r.indent, 8) * 9}px`}>
        <button type="button" class="tasks-box" role="checkbox" aria-checked={r.done ? "true" : "false"} data-task={r.i} disabled={!onchange}
                aria-label={r.label} onclick={() => onchange?.(toggled(text, r.i))}></button>
        {#if r.num}<span class="tasks-num">{r.num}.</span>{/if}
        <span class="tasks-label">{@html markdown(r.label)}</span>
      </div>
    {/if}
  {/each}
  {#if onchange}
  <form class="tasks-add" data-form="add" onsubmit={add}>
    <input name="task" autocomplete="off" placeholder="add a task" bind:value={fresh}/>
    <button type="submit" data-idle={fresh.trim() === "" ? "true" : "false"}>add</button>
  </form>
  {/if}
</section>

<style>
  .tasks { display: grid; gap: 4px; min-width: 0; font-family: var(--font-text); color: var(--ink); }
  .tasks p { margin: 0; }
  .tasks-count { display: flex; align-items: center; gap: 8px; color: var(--muted); margin-bottom: 4px; }
  .tasks-count b { color: var(--ink); font-weight: var(--weight-strong); font-variant-numeric: tabular-nums; }
  .tasks-bar { display: inline-block; width: 120px; height: 4px; background: var(--line); border-radius: var(--radius-pill); overflow: hidden; }
  .tasks-bar span { display: block; height: 100%; background: var(--live); }
  .tasks-row { display: flex; align-items: baseline; gap: 8px; min-width: 0; }
  .tasks-box { flex: none; align-self: center; width: 16px; height: 16px; padding: 0; background: var(--panel); border: var(--border) solid var(--muted); border-radius: var(--radius-xs); cursor: pointer; position: relative; }
  .tasks-box:hover, .tasks-box:global([data-sheet~=hover]) { border-color: var(--ink); }
  .tasks-box:focus-visible, .tasks-box:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 2px; }
  .tasks-box[aria-checked=true], .tasks-box:global([data-sheet~=checked]) { background: var(--live); border-color: var(--live); }
  .tasks-box[aria-checked=true]::after, .tasks-box:global([data-sheet~=checked])::after { content: ""; position: absolute; left: 4px; top: 1px; width: 4px; height: 8px; border: solid var(--panel); border-width: 0 2px 2px 0; transform: rotate(45deg); }
  .tasks-box:disabled, .tasks-box:global([data-sheet~=disabled]) { cursor: default; opacity: 0.7; }
  .tasks-num { flex: none; color: var(--muted); font-variant-numeric: tabular-nums; }
  .tasks-label { min-width: 0; }
  .tasks-done .tasks-label { color: var(--muted); text-decoration: line-through; }
  .tasks-label :global(p), .tasks-md :global(p) { margin: 0; }
  .tasks-md { color: var(--muted); }
  .tasks-add { display: flex; gap: 6px; margin-top: 6px; }
  .tasks-add input { flex: 1; min-width: 0; font: inherit; color: var(--ink); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-sm); padding: 3px 8px; }
  .tasks-add input:focus, .tasks-add input:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .tasks-add button { font: inherit; color: var(--ink); background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-sm); padding: 3px 10px; cursor: pointer; }
  .tasks-add button:hover, .tasks-add button:global([data-sheet~=hover]) { background: var(--hover); }
  .tasks-add button[data-idle=true] { color: var(--muted); }
</style>
