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
// `[data-action="palette.open"]`; `[data-surface=jump-palette-sv]`, its
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

export default async ({ test, assert, eventually, sleep, openPage, sy, groupsNow, attrs, BOARDS, ticketAt }) => {
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
  const pal = (page) => page.locator("[data-surface=jump-palette-sv]").first();
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
    await eventually(`${titled}'s page`, async () => ticketAt(await hash(page), titled), 3_000);
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
    // Closed, the palette binds neither Enter nor Escape (its overlay rows: the_open_keys_come_and_go_with_the_palette);
    // on the board they are the rows' (SURFACE-2: open the selected, clear the selection), so they leave it shut.
    await pressed(page, "Enter");
    await pressed(page, "Escape");
    assert((await palInput(page).count()) === 0, "closed, enter and escape leave the palette shut");
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
    await eventually(`enter opens ${order[1]}`, async () => ticketAt(await hash(page), order[1]), 3_000);

    // Escape closes; the bus's palette.open opens (the page's top's jump button emits it; STUDIO-12: app-top's, the one on the page).
    await page.evaluate(() => (location.hash = "#/search/qorvex"));
    await eventually("the search page draws", async () => (await sp(page).count()) === 1);
    await page.locator('[data-surface=app-top] [data-action="palette.open"]').click();
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
    // An argument is one segment, encoded whole: an address holds `/`, and `?` `#` would be cut.
    await run(">waves to MENGER/ORG-3", "#/waves/to/MENGER%2FORG-3", "waves to");
    await run(">search a/b?c#d", "#/search/a%2Fb%3Fc%23d", "search");
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
    // A click on it does the same: says so and stays (never #/waves/to).
    await palInput(page).fill(">waves t");
    await eventually("waves to listed", async () => (await pal(page).locator('[data-command="waves to"]').count()) === 1, 2_000);
    await pal(page).locator('[data-command="waves to"]').click();
    await eventually("a click asks for its id", async () => trimmed(await pal(page).locator("[data-palette-need]").textContent({ timeout: 500 })).includes("waves to"), 2_000);
    assert((await hash(page)) === SV.route && (await palInput(page).count()) === 1, `a click without its id stays: ${await hash(page)}`);
    const calls = (await sentCalls(page)).filter((c) => c.method === "tickets.find" && String(c.params.query).startsWith(">"));
    assert(calls.length === 0, `a command never asks find: ${JSON.stringify(calls)}`);
  });

  await test("the_slash_is_typed_in_the_palettes_field_and_does_not_reopen_it", async () => {
    const { page } = await probed(SV.route);
    await eventually("the board's palette is mounted", async () => (await pal(page).count()) === 1);
    await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
    // On the page `/` is the board's (the filter line's, SURFACE-2): the palette stays shut.
    taken(await pressed(page, "/"), "/ on the page");
    await sleep(200);
    assert((await palInput(page).count()) === 0, "/ does not open the palette");
    taken(await pressed(page, "Control+k"), "ctrl+k");
    await eventually("ctrl+k opens the palette, its field focused", async () => palInput(page).evaluate((e) => e === document.activeElement), 2_000);
    // In a field `/` is a character: not the host's, typed, and the palette stays as it is.
    free(await pressed(page, "/"), "/ in the field");
    await eventually("the slash is typed", async () => (await palInput(page).inputValue()) === "/", 2_000);
    assert((await palInput(page).count()) === 1, "one palette, open");
  });

  await test("the_open_keys_come_and_go_with_the_palette", async () => {
    const { page } = await probed(SV.route);
    await eventually("the board's palette is mounted", async () => (await pal(page).count()) === 1);
    const blank = () => page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
    // The host's `?` overlay: the palette's rows, by chord.
    const chords = async () => {
      await page.keyboard.press("?");
      const sec = page.locator("[data-engine-keys] [data-component=jump-palette-sv]");
      await eventually("the ? overlay", async () => (await page.locator("[data-engine-keys]").count()) === 1, 2_000);
      const got = (await attrs(sec.locator("[data-chord]"), "data-chord")).sort();
      await page.keyboard.press("Escape");
      await eventually("the overlay closes", async () => (await page.locator("[data-engine-keys]").count()) === 0, 2_000);
      return got;
    };
    await blank();
    const closed = await chords();
    assert(JSON.stringify(closed) === JSON.stringify(["ctrl+k"]), `closed, the palette holds ctrl+k alone: ${JSON.stringify(closed)}`);
    // Ten opens and closes: nothing left behind.
    for (let i = 0; i < 10; i++) {
      taken(await pressed(page, "Control+k"), `ctrl+k (${i + 1})`);
      await eventually("open", async () => (await palInput(page).count()) === 1, 2_000);
      taken(await pressed(page, "Escape"), `escape (${i + 1})`);
      await eventually("closed", async () => (await palInput(page).count()) === 0, 2_000);
    }
    await blank();
    const after = await chords();
    assert(JSON.stringify(after) === JSON.stringify(closed), `after ten opens the closed palette holds the same keys: ${JSON.stringify(after)}`);
    await pressed(page, "Enter"); // the rows' on the board (SURFACE-2), never the palette's once it is closed
    assert((await palInput(page).count()) === 0, "enter after ten opens leaves the palette shut");
    // Open, out of the field: the open-only keys are listed, once each.
    await page.keyboard.press("Control+k");
    await eventually("open", async () => (await palInput(page).count()) === 1, 2_000);
    await pal(page).locator(".palette-foot").click();
    const open = await chords();
    assert(JSON.stringify(open) === JSON.stringify(["arrowdown", "arrowup", "ctrl+k", "enter", "escape"]), `open, its five keys once each: ${JSON.stringify(open)}`);
  });

  await test("a_corrected_query_says_so", async () => {
    const typo = "qorvax";
    const { searched, ids } = searchOf(typo);
    const also = (searched?.words ?? []).filter((w) => w.also?.length);
    assert(ids.includes(bodied) && also.length > 0, `search reads ${typo} as a typo: ${JSON.stringify(searched?.words)} (${ids})`);
    const { page } = await openPage(`#/search/${typo}`);
    const note = sp(page).locator("p[data-corrected]");
    await eventually("the correction is said", async () => trimmed(await note.textContent({ timeout: 1000 })).includes(also[0].also[0]), 5_000);
    assert(trimmed(await note.textContent()).includes(typo), "it names the word typed");
    await eventually("the corrected rows are marked", async () => (await sp(page).locator(`[data-result=search][data-id="${bodied}"][data-corrected]`).count()) === 1, 5_000);
    // A query the index knows is not corrected.
    await page.evaluate(() => (location.hash = "#/search/qorvex"));
    const known = searchOf("qorvex");
    assert(!(known.searched?.words ?? []).some((w) => w.also?.length), `qorvex is no typo: ${JSON.stringify(known.searched?.words)}`);
    await eventually("qorvex's rows, uncorrected", async () => JSON.stringify(await resultIds(page, "search")) === JSON.stringify(known.ids) && (await sp(page).locator("[data-result][data-corrected]").count()) === 0, 5_000);
    await eventually("no correction for a known word", async () => (await note.count()) === 0, 5_000);
  });

  await test("a_title_is_text_never_markup", async () => {
    const id = make('<img src=x onerror="window.__xss=1">qorvex', "<b>bold</b> qorvex body");
    const { page } = await openPage("#/search/qorvex");
    await eventually("the row is drawn", async () => (await sp(page).locator(`[data-result=find][data-id="${id}"]`).count()) === 1, 5_000);
    await eventually("and in search", async () => (await sp(page).locator(`[data-result=search][data-id="${id}"]`).count()) === 1, 5_000);
    assert((await sp(page).locator("img, [data-result] [data-field=title] b, [data-field=snippet] b").count()) === 0, "no element from a title or a body");
    const title = trimmed(await sp(page).locator(`[data-result=find][data-id="${id}"] [data-field=title]`).textContent());
    assert(title.startsWith("<img"), `the title is its text: ${title}`);
    await sleep(300);
    assert(!(await page.evaluate(() => window.__xss)), "nothing ran");
    // The palette draws it as text too.
    await page.evaluate((r) => (location.hash = r), SV.route);
    // SV-4: the board's own palette, in board-frame's section, and the search page's gone with its page.
    await eventually("the board's palette", async () => (await page.locator("[data-surface=jump-palette-sv]").count()) === 1 && (await page.locator("[data-layout=board-frame][data-slot=palette] [data-surface=jump-palette-sv]").count()) === 1);
    await page.keyboard.press("Control+k");
    await eventually("open", async () => (await palInput(page).count()) === 1, 2_000);
    await palInput(page).fill("qorvex");
    await eventually("the hit is drawn", async () => (await pal(page).locator(`[data-palette-hit="${id}"]`).count()) === 1, 3_000);
    assert((await pal(page).locator("img").count()) === 0 && !(await page.evaluate(() => window.__xss)), "the palette draws no element from a title");
  });

  // ARCHIVE-8 (the owner, 2026-11-13: "we need search to optionally search through archived"): the page's
  // archive switch, `[data-action=archived]`, off by default. Off, nothing archived is found, as ever; on
  // (#/search/:q/include/archived, and …/f/:filter after it), both lists ask under `(archived or not
  // archived)` and an archived row says so (`[data-archived]`, `[data-field=archived]`). The choice is the
  // address's: a reload keeps it. An agent says the same with the filter.
  await test("the_archive_switch_finds_archived_tickets_and_says_so", async () => {
    const EITHER = "archived or not archived";
    const live = make("the zembla shelf", "a shelf that is still in use");
    const goneTitle = make("the zembla kiln", "a kiln that was sold");
    const goneBody = make("an old firing note", "the zembla slip crazed on every pot, so the batch was thrown out");
    const archived = sy("tickets", "archive", "--ids", `${goneTitle},${goneBody}`, "--message", "the first season");
    assert(!archived.some((e) => e.type === "refused" || e.type === "error"), `archived: ${JSON.stringify(archived)}`);
    // The service: off by default, on by the filter (find and search alike).
    assert(JSON.stringify(findIds("zembla")) === JSON.stringify([live]), `find alone answers the live one: ${findIds("zembla")}`);
    assert(searchOf("zembla").ids.every((id) => id !== goneBody && id !== goneTitle), `search alone answers nothing archived: ${searchOf("zembla").ids}`);
    const found = findIds("zembla", "--filter", EITHER), searched = searchOf("zembla", "--filter", EITHER).ids;
    assert(found.includes(live) && found.includes(goneTitle), `find under the filter answers both titles: ${found}`);
    assert(searched.includes(goneBody), `search under the filter answers the archived body: ${searched}`);

    const { page } = await openPage("#/search/zembla");
    const box = sp(page).locator("input[data-action=archived]");
    await eventually("off: the live one alone", async () => JSON.stringify(await resultIds(page, "find")) === JSON.stringify([live]));
    assert(!(await box.isChecked()), "the switch is off by default");
    assert((await sp(page).locator("[data-archived]").count()) === 0 && !(await resultIds(page, "search")).includes(goneBody), "nothing archived is drawn");
    assert((await sp(page).locator("label[data-field=include]").innerText()).trim() === "include archived", "the switch is named");

    // On: the address says so, both lists find the archived, each archived row is marked.
    await box.focus();
    await page.keyboard.press(" ");
    await eventually("the address holds the choice", async () => (await hash(page)) === "#/search/zembla/include/archived", 3_000);
    await eventually("find's rows, the archived too", async () => JSON.stringify(await resultIds(page, "find")) === JSON.stringify(found), 5_000);
    await eventually("search's rows, the archived too", async () => JSON.stringify(await resultIds(page, "search")) === JSON.stringify(searched), 5_000);
    assert(await box.isChecked(), "the switch is on");
    for (const [kind, id] of [["find", goneTitle], ["search", goneBody]]) {
      const row = sp(page).locator(`[data-result=${kind}][data-id="${id}"]`);
      assert((await row.getAttribute("data-archived")) === "" && trimmed(await row.locator("[data-field=archived]").textContent()) === "archived", `${kind}: ${id}'s row says it is archived`);
    }
    assert((await sp(page).locator(`[data-result=find][data-id="${live}"][data-archived]`).count()) === 0 && (await sp(page).locator(`[data-result=find][data-id="${live}"] [data-field=archived]`).count()) === 0, "a live row carries no mark");
    assert(await page.evaluate(() => document.scrollingElement.scrollWidth <= document.scrollingElement.clientWidth), "the page does not scroll sideways");

    // A reload keeps it; under a filter too; off again is as before.
    await page.reload();
    await eventually("after a reload, still on", async () => (await box.isChecked()) && (await resultIds(page, "find")).includes(goneTitle), 10_000);
    await page.evaluate(() => (location.hash = "#/search/zembla/include/archived/f/" + encodeURIComponent("status Draft")));
    await eventually("under the route's filter, the archived too", async () => (await resultIds(page, "find")).includes(goneTitle) && trimmed(await sp(page).locator("[data-field=filter] code").textContent()) === "status Draft", 5_000);
    await box.click();
    await eventually("off keeps the filter", async () => (await hash(page)) === "#/search/zembla/f/" + encodeURIComponent("status Draft"), 3_000);
    await eventually("off: the live one alone again", async () => JSON.stringify(await resultIds(page, "find")) === JSON.stringify([live]), 5_000);
    sy("tickets", "restore", "--ids", `${goneTitle},${goneBody}`);
  });
};
