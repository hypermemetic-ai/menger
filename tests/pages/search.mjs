// SURFACE-8: search and the palette — `search-page` (svelte/1) at
// #/search/:q (#/search and #/search/:q/f/:filter too) and the Svelte
// board's palette, `jump-palette-sv`, with FRAME-36's keys and its `>`
// commands. The key cases are real: FRAME-36's probe, a window listener
// installed before the engine, reads each press's `defaultPrevented` and
// whether it reached the window's bubble phase.
//
// Marks: `[data-surface=search-page]` (`data-q`), its results
// `[data-result=find|search][data-id]` (each with `[data-field=title]`, a
// search row with `[data-field=snippet]`, the match in `<mark>`),
// `[data-empty]`, `[data-refused]`, the pager `[data-pager]` with
// `[data-field=matched]`, `[data-action=prev|next]`, the filter
// `[data-field=filter]` and `[data-action=everything]`, the jump button
// `[data-action="palette.open"]`; `[data-surface=jump-palette]`, its
// `input[data-palette-input]`, `[data-palette-hit=<id>]`,
// `[data-search-hit=<id>]`, `[data-command=<name>]`, the row
// `[data-selected]`, `[data-palette-need]`.
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

// Before the host (FRAME-36's own probe): every keydown the window's capture
// phase sees, read after the press for `defaultPrevented`, and every one
// that reaches the window's bubble phase.
const KEYS_PROBE = `(() => {
  window.__events = [];
  window.__reached = [];
  window.addEventListener("keydown", (e) => window.__events.push(e), true);
  window.addEventListener("keydown", (e) => window.__reached.push(e.key));
})();`;

export default async ({ test, assert, eventually, sleep, openPage, sy, groupsNow, attrs, BOARDS }) => {
  // The palette is the Svelte board's (jump-palette-sv), the default board.
  const SV = BOARDS.sv;
  const trimmed = (t) => String(t ?? "").trim();
  const root = groupsNow().filter((g) => g.root).at(-1);
  const other = groupsNow().filter((g) => g.root).at(-2);
  const make = (title, body, under = root) => {
    const [made] = sy("tickets", "create", "--epic", under.epic, "--parent", under.root, "--title", title, "--body", body);
    assert(made?.ticket?.id, `${title} was made: ${JSON.stringify(made)}`);
    return made.ticket.id;
  };
  const findIds = (q, ...f) => sy("tickets", "find", "--query", q, "--limit", "12", ...f).filter((e) => e.type === "summary").map((e) => e.summary.id);
  const searchOf = (q, ...f) => {
    const out = sy("tickets", "search", "--query", q, "--limit", "8", ...f);
    return { ids: out.filter((e) => e.type === "hit").map((e) => e.summary.id), hits: out.filter((e) => e.type === "hit"), searched: out.find((e) => e.type === "searched") };
  };
  const sp = (page) => page.locator("[data-surface=search-page]");
  const resultIds = (page, kind) => attrs(sp(page).locator(`[data-result=${kind}][data-id]`), "data-id");
  const hash = (page) => page.evaluate(() => location.hash);
  async function pressed(page, combo) {
    const before = await page.evaluate(() => window.__reached.length);
    await page.keyboard.press(combo);
    return page.evaluate((b) => {
      const e = window.__events[window.__events.length - 1];
      return { key: e.key, prevented: e.defaultPrevented, reached: window.__reached.slice(b).includes(e.key) };
    }, before);
  }
  const taken = (p, combo) => assert(p.prevented && !p.reached, `${combo} (${p.key}) is the host's alone: prevented ${p.prevented}, reached ${p.reached}`);
  const free = (p, combo) => assert(!p.prevented && p.reached, `${combo} (${p.key}) is the browser's: prevented ${p.prevented}, reached ${p.reached}`);
  /** The board with the keys probe installed before the engine. */
  async function probed(route = "") {
    const opened = await openPage(route);
    await opened.context.addInitScript(KEYS_PROBE);
    await opened.page.reload();
    return opened;
  }
  const pal = (page) => page.locator("[data-surface=jump-palette]").first();
  const palInput = (page) => pal(page).locator("input[data-palette-input]");

  // The tickets the cases read: one titled `qorvex`, one with it in its body.
  const titled = make("the qorvex kettle", "a kettle for the tea room");
  const bodied = make("a firing note", "the glaze ran where the qorvex wash was thin, so the shelf needs grinding");

  await test("find_and_search_both_answer", async () => {
    const found = findIds("qorvex");
    const { ids, hits } = searchOf("qorvex");
    assert(found.includes(titled) && ids.includes(bodied), `find answers the title (${found}) and search the body (${ids})`);
    const { page } = await openPage("#/search/qorvex");
    await eventually("the search page draws", async () => (await sp(page).count()) === 1 && (await sp(page).getAttribute("data-q")) === "qorvex");
    await eventually("find's rows, in find's order", async () => JSON.stringify(await resultIds(page, "find")) === JSON.stringify(found));
    await eventually("search's rows, in search's order", async () => JSON.stringify(await resultIds(page, "search")) === JSON.stringify(ids));
    // The match highlighted: find's title, search's snippet as the method marked it.
    const t = sp(page).locator(`[data-result=find][data-id="${titled}"] [data-field=title] mark`);
    assert(trimmed(await t.textContent()).toLowerCase() === "qorvex", `the title's match is marked: ${await t.textContent()}`);
    const marked = hits.find((h) => h.summary.id === bodied).snippet.filter((m) => m.hit).map((m) => m.text);
    const drawn = await sp(page).locator(`[data-result=search][data-id="${bodied}"] [data-field=snippet] mark`).allTextContents();
    assert(marked.length > 0 && JSON.stringify(drawn) === JSON.stringify(marked), `the snippet's marks are search's: ${JSON.stringify(drawn)} vs ${JSON.stringify(marked)}`);
    const snippet = trimmed(await sp(page).locator(`[data-result=search][data-id="${bodied}"] [data-field=snippet]`).textContent());
    assert(snippet.includes("qorvex wash"), `the passage is drawn: ${snippet}`);
    // A row opens its ticket.
    await sp(page).locator(`[data-result=find][data-id="${titled}"]`).click();
    await eventually(`#/ticket/${titled}`, async () => (await hash(page)) === `#/ticket/${titled}`, 3_000);
  });

  await test("search_pages_through_results", async () => {
    // Eleven in one root and three in another: `under` the first is the filter.
    const made = [];
    for (let i = 1; i <= 11; i++) made.push(make(`plumbix note ${i}`, `the plumbix line number ${i} of the batch`));
    for (let i = 1; i <= 3; i++) make(`plumbix elsewhere ${i}`, `plumbix outside the root, ${i}`, other);
    const filter = `under ${root.root}`;
    const all = sy("tickets", "search", "--query", "plumbix", "--filter", filter, "--limit", "100").filter((e) => e.type === "hit").map((e) => e.summary.id);
    const p1 = searchOf("plumbix", "--filter", filter);
    const p2 = searchOf("plumbix", "--filter", filter, "--offset", "8");
    assert(p1.searched.matched === all.length && all.length >= 11 && p2.ids.length > 0, `the method pages (${all.length}; ${p1.ids.length} + ${p2.ids.length})`);
    const { page } = await openPage(`#/search/plumbix/f/${encodeURIComponent(filter)}`);
    await eventually("page one: search's first eight", async () => JSON.stringify(await resultIds(page, "search")) === JSON.stringify(p1.ids));
    assert(trimmed(await sp(page).locator("[data-field=filter]").textContent()).includes(filter), "the page says the filter it honours");
    assert(Number(trimmed(await sp(page).locator("[data-pager] [data-field=matched]").textContent())) === all.length, "the pager says how many match");
    assert(await sp(page).locator("[data-pager] [data-action=prev]").isDisabled(), "prev is disabled on the first page");
    let calls = await sentCalls(page);
    assert(calls.some((c) => c.method === "tickets.search" && c.params.filter === filter && c.params.offset === 0 && c.params.limit === 8), `search asks under the filter, offset 0: ${JSON.stringify(calls.filter((c) => c.method === "tickets.search").map((c) => c.params))}`);
    assert(calls.some((c) => c.method === "tickets.find" && c.params.filter === filter), "find asks under the filter");
    const asked = calls.length;
    await sp(page).locator("[data-pager] [data-action=next]").click();
    await eventually("page two: offset 8", async () => JSON.stringify(await resultIds(page, "search")) === JSON.stringify(p2.ids));
    calls = (await sentCalls(page)).slice(asked);
    assert(calls.some((c) => c.method === "tickets.search" && c.params.offset === 8), `next asks offset 8: ${JSON.stringify(calls.map((c) => c.params))}`);
    assert(await sp(page).locator("[data-pager] [data-action=next]").isDisabled() === (p2.searched.next == null), "next is disabled on the last page");
    await sp(page).locator("[data-pager] [data-action=prev]").click();
    await eventually("back to page one", async () => JSON.stringify(await resultIds(page, "search")) === JSON.stringify(p1.ids));
    // Search everything: the route without the filter.
    await sp(page).locator("[data-action=everything]").click();
    await eventually("the route drops the filter", async () => (await hash(page)) === "#/search/plumbix", 3_000);
    const wide = searchOf("plumbix");
    await eventually("everything's first page", async () => JSON.stringify(await resultIds(page, "search")) === JSON.stringify(wide.ids));
  });

  await test("an_empty_query_says_so", async () => {
    const { page } = await openPage("#/search");
    await eventually("#/search says to type", async () => (await sp(page).locator("[data-empty]").count()) === 1);
    const calls = (await sentCalls(page)).filter((c) => c.method === "tickets.find" || c.method === "tickets.search");
    assert(calls.length === 0, `an empty query asks nothing: ${JSON.stringify(calls)}`);
    await page.evaluate(() => (location.hash = "#/search/zzqxwvnothing"));
    await eventually("both columns say none", async () => (await sp(page).locator("[data-empty]").count()) === 2, 5_000);
    assert((await sp(page).locator("[data-result]").count()) === 0, "no rows");
    // A filter the service refuses is said where it was asked, in its words.
    await page.evaluate(() => (location.hash = "#/search/qorvex/f/text%20foo"));
    await eventually("the refusal is drawn", async () => trimmed(await sp(page).locator("[data-refused]").first().textContent({ timeout: 1000 })).length > 0, 5_000);
    // Dark: the tokens do it (no literal colour) — the refusal's colour moves with the wrapper.
    const refused = sp(page).locator("[data-refused]").first();
    const light = await refused.evaluate((e) => getComputedStyle(e).color);
    await sp(page).evaluate((e) => e.setAttribute("data-plexus-theme", "dark"));
    const dark = await refused.evaluate((e) => getComputedStyle(e).color);
    assert(light !== dark, `--warn has its dark value under data-plexus-theme=dark: ${light} vs ${dark}`);
  });

  await test("the_palette_opens_on_ctrl_k_and_enter_opens", async () => {
    const { page } = await probed(SV.route);
    await eventually("the board's palette is mounted", async () => (await pal(page).count()) === 1);
    await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
    // Closed, the palette's keys are not bound: Enter and Escape stay the page's.
    free(await pressed(page, "Enter"), "enter (closed)");
    free(await pressed(page, "Escape"), "escape (closed)");
    taken(await pressed(page, "Control+k"), "ctrl+k");
    await eventually("ctrl+k focuses the palette's field", async () => palInput(page).evaluate((e) => e === document.activeElement), 2_000);
    // ctrl+k in the field is still the host's (a ctrl chord).
    taken(await pressed(page, "Control+k"), "ctrl+k in the field");
    const found = findIds("qorvex");
    const { ids } = searchOf("qorvex");
    await palInput(page).fill("qorvex");
    await eventually("the find hits are drawn", async () => JSON.stringify(await attrs(pal(page).locator("[data-palette-hit]"), "data-palette-hit")) === JSON.stringify(found), 3_000);
    await eventually("the search hits below", async () => JSON.stringify(await attrs(pal(page).locator("[data-search-hit]"), "data-search-hit")) === JSON.stringify(ids), 3_000);
    const selected = async () => (await pal(page).locator("[data-selected]").first().getAttribute("data-palette-hit")) ?? (await pal(page).locator("[data-selected]").first().getAttribute("data-search-hit"));
    assert((await pal(page).locator("[data-selected]").count()) === 1 && (await selected()) === found[0], "the first hit is selected");
    const order = [...found, ...ids];
    taken(await pressed(page, "ArrowDown"), "arrowdown");
    await eventually("arrowdown moves the selection", async () => (await selected()) === order[1], 2_000);
    taken(await pressed(page, "ArrowDown"), "arrowdown");
    taken(await pressed(page, "ArrowUp"), "arrowup");
    await eventually("arrowup moves it back", async () => (await selected()) === order[1], 2_000);
    taken(await pressed(page, "Enter"), "enter");
    await eventually(`enter opens ${order[1]}`, async () => (await hash(page)) === `${SV.ticketRoute}${order[1]}`, 3_000);

    // Escape closes; the bus's palette.open opens (the search page's jump button emits it).
    await page.evaluate(() => (location.hash = "#/search/qorvex"));
    await eventually("the search page draws", async () => (await sp(page).count()) === 1);
    await sp(page).locator('[data-action="palette.open"]').click();
    await eventually("palette.open on the bus opens it", async () => (await palInput(page).count()) === 1, 2_000);
    await palInput(page).focus();
    taken(await pressed(page, "Escape"), "escape");
    await eventually("escape closes it", async () => (await palInput(page).count()) === 0, 2_000);
    free(await pressed(page, "Enter"), "enter (closed again)");
  });

  await test("a_command_routes", async () => {
    const { page } = await probed(SV.route);
    await eventually("the board's palette is mounted", async () => (await pal(page).count()) === 1);
    const run = async (typed, route, first) => {
      await page.evaluate((r) => (location.hash = r), SV.route);
      await eventually("the board's palette", async () => (await pal(page).count()) === 1);
      await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
      taken(await pressed(page, "Control+k"), "ctrl+k");
      await eventually("open", async () => (await palInput(page).count()) === 1, 2_000);
      await palInput(page).fill(typed);
      await eventually(`${typed}: ${first} is the selected command`, async () => (await pal(page).locator("[data-command][data-selected]").getAttribute("data-command", { timeout: 500 })) === first, 2_000);
      taken(await pressed(page, "Enter"), "enter");
      await eventually(`${typed} → ${route}`, async () => (await hash(page)) === route, 2_000);
    };
    await run(">ready", "#/ready", "ready");
    await run(">waves", "#/waves", "waves");
    await run(">new", "#/new", "new");
    await run(">waves to GLAZE-12", "#/waves/to/GLAZE-12", "waves to");
    await run(">search qorvex", "#/search/qorvex", "search");
    // `>wa` lists the three waves commands; arrows move among them.
    await page.evaluate((r) => (location.hash = r), SV.route);
    await eventually("the board's palette", async () => (await pal(page).count()) === 1);
    await page.keyboard.press("Control+k");
    await eventually("open", async () => (await palInput(page).count()) === 1, 2_000);
    await palInput(page).fill(">wa");
    await eventually("three commands", async () => JSON.stringify(await attrs(pal(page).locator("[data-command]"), "data-command")) === JSON.stringify(["waves", "waves under", "waves to"]), 2_000);
    assert((await pal(page).locator("[data-palette-hit], [data-search-hit]").count()) === 0, "a command asks no tickets");
    await page.keyboard.press("ArrowDown");
    await eventually("waves under selected", async () => (await pal(page).locator("[data-command][data-selected]").getAttribute("data-command", { timeout: 500 })) === "waves under", 2_000);
    // A command that needs an argument says so and stays.
    await page.keyboard.press("Enter");
    await eventually("it asks for its id", async () => trimmed(await pal(page).locator("[data-palette-need]").textContent({ timeout: 500 })).includes("waves under"), 2_000);
    assert((await hash(page)) === SV.route, "no route without its id");
    const calls = (await sentCalls(page)).filter((c) => c.method === "tickets.find" && String(c.params.query).startsWith(">"));
    assert(calls.length === 0, `a command never asks find: ${JSON.stringify(calls)}`);
  });
};
