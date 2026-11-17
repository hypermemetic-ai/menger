// NOTES-4: a note's page — the note on the left, its slot on the right
// (note-slot, placed by editor-screen) — on a throwaway whose ui.author
// compiles svelte/1 (`--ui-svelte-compiler`). A real component, graph-nodes,
// is authored into the bundle `notes` over synapse (`ui author`, its params a
// file), and the slot filled with it (`notes fill`): it is drawn beside a
// note being typed in within two seconds, the caret and the scroll where they
// were; an earlier fill picked is drawn on that page alone; the component
// keeps its lines in the slot's props (`notes.props`), which reach a second
// page's mounted component in place and a reload; a build said shows its
// steps with the component still usable, a failure its refusal under it; at
// 390 px the note and the slot are two tabs and nothing scrolls sideways; a
// ticket on the surface has no slot. Run alone by
// `node tests/pages_browser.mjs --group slot`.
//
// Marks: `[data-field=slot][data-state=empty|filled|building|failed]`,
// `[data-slot=fill][data-component][data-version]` (the pinned mark),
// `[data-field=steps] [data-step=…][data-state=reached|failed|next]`,
// `[data-field=refusal]`, `[data-field=fills] [data-fill=N]`,
// `[data-field=which]`; editor-screen's `.page[data-two][data-tab]`,
// `[data-col=note|slot]`, `[role=tab][data-tab=note|slot]`; the component's
// own `[data-graph][data-self]`, `[data-node=NAME]`, `[data-line=I]`,
// `[data-field=count]`.
import { spawnSync } from "node:child_process";

/** graph-nodes: nodes on a ring; click one then another to connect them, a line to take it away.
 *  Its lines are the slot's props (`notes.props`), so every page and a reload draw them. */
export const GRAPH = (look) => `<script module>
  export const uses = ["notes.props"];
</script>

<script>
  import { bus } from "plexus-ui/host";
  let { note = "", props = "{}" } = $props();
  const b = bus();
  const self = Math.random().toString(36).slice(2, 10);
  const p = $derived.by(() => { try { return JSON.parse(props || "{}"); } catch { return {}; } });
  const names = ["tickets", "ui", "media", "notes", "quota", "logs"];
  const lines = $derived(Array.isArray(p.lines) ? p.lines : []);
  const at = (name) => { const i = names.indexOf(name); const a = (i / names.length) * 2 * Math.PI - Math.PI / 2; return [260 + 170 * Math.cos(a), 190 + 140 * Math.sin(a)]; };
  let first = $state(null);
  async function pick(name) {
    if (first == null) { first = name; return; }
    const from = first;
    first = null;
    if (from !== name) await b.call("notes.props", { id: note, patch: { lines: [...lines, [from, name]] } });
  }
  const drop = (i) => b.call("notes.props", { id: note, patch: { lines: lines.filter((_, j) => j !== i) } });
</script>

<div class="graph" data-graph data-self={self} data-look="${look}">
  <svg viewBox="0 0 520 380" role="img" aria-label="the services">
    {#each lines as l, i (i)}
      {@const a = at(l[0])}{@const z = at(l[1])}
      <line data-line={i} x1={a[0]} y1={a[1]} x2={z[0]} y2={z[1]} onclick={() => drop(i)}></line>
    {/each}
    {#each names as n (n)}
      {@const c = at(n)}
      <g data-node={n} data-picked={first === n ? "" : undefined} onclick={() => pick(n)}>
        <circle cx={c[0]} cy={c[1]} r="12"></circle>
        <text x={c[0]} y={c[1] - 22}>{n}</text>
      </g>
    {/each}
  </svg>
  <p class="foot"><span data-field="count">{names.length} nodes · {lines.length} lines</span> <span>click a node, then another, to connect them · click a line to remove it</span></p>
</div>

<style>
  .graph { display: flex; flex-direction: column; width: 100%; height: 100%; color: var(--ink); }
  svg { flex: 1; width: 100%; min-height: 280px; }
  circle { fill: var(--ink); cursor: pointer; }
  g[data-picked] circle { fill: var(--accent); }
  text { fill: var(--muted); font: 13px var(--mono); text-anchor: middle; }
  line { stroke: var(--muted); stroke-width: 2; cursor: pointer; }
  line:hover { stroke: var(--warn); }
  .foot { display: flex; flex-wrap: wrap; gap: 12px; margin: 0; padding: 8px 12px; font: 11px var(--mono); color: var(--muted); border-top: var(--border) solid var(--line); }
</style>
`;

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, marked, sameDocument, sy, server, session, path, fs, summaries } = ctx;

  /** `ui author` into the bundle `notes`, its params a file (`-p`, synapse's global option): the version. */
  function author(name, source) {
    const params = path.join(path.dirname(session), `slot-author-${name}-${Date.now()}.json`);
    fs.writeFileSync(params, JSON.stringify({ bundle: "notes", sources: [{ name, profile: "svelte/1", source }] }));
    const r = spawnSync("synapse", ["--no-hold", "--token-file", session, "-j", "-p", `@${params}`, "-P", String(server.ws), "menger", "ui", "author"],
      { env: { ...process.env, XDG_CACHE_HOME: path.join(server.work, "cache") }, encoding: "utf8" });
    const authored = (r.stdout ?? "").split("\n").filter(Boolean).map((l) => { try { return JSON.parse(l); } catch { return null; } }).find((i) => i?.content?.type === "authored");
    assert(authored, `ui author ${name}: ${r.stdout} ${r.stderr}`);
    return authored.content.version;
  }
  const note = (title, body) => sy("notes", "create", "--title", title, "--body", body).find((e) => e.type === "created").note.id;
  const fill = (id, version, say) => {
    const r = sy("notes", "fill", "--id", id, "--component", "graph-nodes", "--version", version, "--say", say);
    assert(r[0]?.type === "filled", `notes fill: ${JSON.stringify(r)}`);
    return r[0].slot;
  };
  const slotOf = (page) => page.locator("[data-field=slot]");
  const mark = (page) => page.locator("[data-slot=fill][data-component=graph-nodes]");
  const graph = (page) => page.locator("[data-slot=fill] [data-graph]");
  const lines = async (page) => Number(((await page.locator("[data-slot=fill] [data-field=count]").textContent({ timeout: 1000 }).catch(() => "")) ?? "").match(/(\d+) lines/)?.[1] ?? -1);
  const editorUp = async (page) => eventually("the note's editor", async () => (await page.locator("[data-surface=editor-body] .cm-content").count()) === 1, 15_000);

  const v1 = author("graph-nodes", GRAPH("v1"));
  const v2 = author("graph-nodes", GRAPH("v2"));
  assert(v1 !== v2, "two versions");

  await test("a_note_and_its_slot_side_by_side_filled_while_typing", async () => {
    const id = note("The service map", "Draw every service as a node on a graph.\n\n" + "A line of the note.\n\n".repeat(60));
    const { page, context } = await openPage(`#/note/${id}`);
    await editorUp(page);
    await eventually("the slot is empty", async () => (await slotOf(page).getAttribute("data-state")) === "empty", 10_000);
    // The note on the left, its slot on the right, each scrolling alone.
    const cols = await page.evaluate(() => ["note", "slot"].map((c) => { const e = document.querySelector(`[data-col=${c}]`); const r = e.getBoundingClientRect(); return { x: r.x, w: r.width, scroll: getComputedStyle(e).overflowY }; }));
    assert(cols[0].x < cols[1].x && cols[0].w > 300 && cols[1].w > 500, `the note left, the slot right: ${JSON.stringify(cols)}`);
    assert(cols.every((c) => c.scroll === "auto"), `each scrolls alone: ${JSON.stringify(cols)}`);
    assert((await page.locator("[data-surface=editor-screen] .list").evaluate((e) => e.getBoundingClientRect().right <= 0)), "the list of notes is a drawer, closed");

    // Typing, scrolled down, the caret in the text.
    const noteCol = page.locator("[data-col=note]");
    await page.evaluate(() => { const v = document.querySelector("[data-surface=editor-body] [data-field=body]").editorView; v.focus(); v.dispatch({ selection: { anchor: v.state.doc.line(31).to } }); });
    await page.keyboard.type("typing here ");
    await noteCol.evaluate((e) => (e.scrollTop = 400));
    await sleep(300);
    const before = await page.evaluate(() => ({ scroll: document.querySelector("[data-col=note]").scrollTop, caret: document.querySelector("[data-surface=editor-body] [data-field=body]")?.editorView?.state.selection.main.head, focus: document.activeElement?.closest(".cm-content") != null }));
    const origin = await marked(page);
    const t0 = Date.now();
    fill(id, v1, "nodes on a ring");
    await eventually("the component is drawn beside the note", async () => (await graph(page).count()) === 1, 2_000);
    const took = Date.now() - t0;
    const after = await page.evaluate(() => ({ scroll: document.querySelector("[data-col=note]").scrollTop, caret: document.querySelector("[data-surface=editor-body] [data-field=body]")?.editorView?.state.selection.main.head, focus: document.activeElement?.closest(".cm-content") != null }));
    assert(took <= 2_000, `drawn within two seconds: ${took} ms`);
    assert(typeof before.caret === "number" && before.scroll > 0 && after.scroll === before.scroll && after.caret === before.caret && after.focus && before.focus, `the caret and the scroll stay: ${JSON.stringify({ before, after })}`);
    await page.keyboard.type("and on");
    await eventually("the typing went on where it was", async () => (sy("notes", "get", "--id", id).find((e) => e.type === "note")?.note.body ?? "").includes("typing here and on"), 5_000);
    await sameDocument(page, origin, "the fill");
    assert((await mark(page).getAttribute("data-version")) === v1, "pinned to its version");
    assert(/v1 of 1/.test(await page.locator("[data-field=which]").textContent()), "which fill, said above it");
    await context.close();
  });

  await test("an_earlier_fill_is_drawn_for_this_page_alone", async () => {
    const id = note("Two versions", "Lines between them.");
    fill(id, v1, "nodes on a ring");
    fill(id, v2, "the lines between them");
    const a = await openPage(`#/note/${id}`);
    const b = await openPage(`#/note/${id}`);
    for (const p of [a.page, b.page]) await eventually("v2 drawn", async () => (await mark(p).getAttribute("data-version").catch(() => null)) === v2, 10_000);
    await a.page.locator("[data-field=fills] [data-fill='1']").click();
    await eventually("v1 drawn on the first page", async () => (await mark(a.page).getAttribute("data-version")) === v1 && (await a.page.locator("[data-graph][data-look=v1]").count()) === 1, 5_000);
    await sleep(500);
    assert((await mark(b.page).getAttribute("data-version")) === v2, "the other page still draws the newest");
    const kept = sy("notes", "slot", "--id", id).find((e) => e.type === "slot").slot;
    assert(kept.fill.n === 2 && kept.fill.version === v2, `notes.slot is unchanged: ${JSON.stringify(kept.fill)}`);
    // › steps back to the newest.
    await a.page.locator("[data-action=later]").click();
    await eventually("› to v2", async () => (await mark(a.page).getAttribute("data-version")) === v2, 5_000);
    await a.context.close();
    await b.context.close();
  });

  await test("props_patched_on_one_page_reach_the_component_on_another_in_place", async () => {
    const id = note("Props", "Connect them.");
    fill(id, v2, "click two nodes to connect");
    const a = await openPage(`#/note/${id}`);
    const b = await openPage(`#/note/${id}`);
    for (const p of [a.page, b.page]) await eventually("the graph", async () => (await graph(p).count()) === 1, 10_000);
    const self = await graph(b.page).getAttribute("data-self");
    assert((await lines(b.page)) === 0, "no lines yet");
    await a.page.locator("[data-slot=fill] [data-node=tickets]").click();
    await a.page.locator("[data-slot=fill] [data-node=ui]").click();
    await eventually("the line on the second page", async () => (await lines(b.page)) === 1, 5_000);
    assert((await graph(b.page).getAttribute("data-self")) === self && (await graph(b.page).count()) === 1, "the same component, not mounted again");
    await b.page.reload();
    await eventually("a reload starts from the props as they were left", async () => (await lines(b.page)) === 1, 15_000);
    await a.context.close();
    await b.context.close();
  });

  await test("the_slot_shows_the_building_and_a_refusal_under_the_component", async () => {
    const id = note("Building", "And let me drag a node.");
    fill(id, v2, "click two nodes to connect");
    const { page, context } = await openPage(`#/note/${id}`);
    await eventually("the graph", async () => (await graph(page).count()) === 1, 10_000);
    sy("notes", "building", "--id", id, "--component", "graph-nodes", "--step", "written", "--say", "drag a node");
    await eventually("the steps, written reached", async () => (await page.locator("[data-field=steps] [data-step=written]").getAttribute("data-state").catch(() => null)) === "reached", 5_000);
    assert((await page.locator("[data-field=steps] [data-step=compiled]").getAttribute("data-state")) === "next", "compiled is still to come");
    // The component that was there still works.
    await page.locator("[data-slot=fill] [data-node=notes]").click();
    await page.locator("[data-slot=fill] [data-node=quota]").click();
    await eventually("still usable while building", async () => (await lines(page)) === 1, 5_000);
    const why = "graph-nodes.svelte:9:54 · `drag` is not defined";
    sy("notes", "building", "--id", id, "--component", "graph-nodes", "--step", "written", "--failed", "true", "--say", why);
    await eventually("the refusal under the component", async () => ((await page.locator("[data-field=refusal]").textContent().catch(() => "")) ?? "").includes("`drag` is not defined"), 5_000);
    assert((await page.locator("[data-field=steps] [data-step=compiled]").getAttribute("data-state")) === "failed", "the step that failed");
    assert((await graph(page).count()) === 1, "the component stays drawn");
    fill(id, v1, "back to the ring");
    await eventually("a fill ends the build", async () => (await page.locator("[data-field=steps]").count()) === 0 && (await mark(page).getAttribute("data-version")) === v1, 5_000);
    await context.close();
  });

  await test("at_390_the_note_and_its_slot_are_two_tabs", async () => {
    const id = note("Narrow", "One sentence.");
    fill(id, v2, "click two nodes to connect");
    const { page, context } = await openPage(`#/note/${id}`);
    await page.setViewportSize({ width: 390, height: 844 });
    await editorUp(page);
    await eventually("two tabs, the note first", async () => (await page.locator("[role=tab][data-tab=note]").isVisible()) && (await page.locator("[role=tab][data-tab=note]").getAttribute("aria-selected")) === "true", 5_000);
    assert(!(await page.locator("[data-col=slot]").isVisible()), "the slot is the other tab");
    await page.locator("[role=tab][data-tab=slot]").click();
    await eventually("the slot's tab draws the component", async () => (await page.locator("[data-col=slot]").isVisible()) && (await graph(page).isVisible()), 5_000);
    assert(!(await page.locator("[data-col=note]").isVisible()), "the note is the other tab");
    const wide = await page.evaluate(() => ({ doc: document.documentElement.scrollWidth, view: document.documentElement.clientWidth, body: document.body.scrollWidth }));
    assert(wide.doc <= wide.view && wide.body <= wide.view, `nothing scrolls sideways: ${JSON.stringify(wide)}`);
    await context.close();
  });

  await test("a_ticket_on_the_surface_has_no_slot", async () => {
    const [t] = summaries("--limit", "1");
    const { page, context } = await openPage(`#/edit/${t.id}`);
    await editorUp(page);
    await sleep(500);
    assert((await slotOf(page).count()) === 0 && (await page.locator(".page[data-two]").count()) === 0, "no slot beside a ticket");
    await context.close();
  });
};
