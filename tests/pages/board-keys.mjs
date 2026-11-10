// SURFACE-2: the app's top and the board's keys — `node tests/pages_browser.mjs --group board-keys`.
//
// The Svelte board (owner, 2026-10-30: the default, `#/`): board-frame's sections.
//
// Marks:
//   [data-layout=board-frame][data-slot=top] app-top   `[data-surface=app-top]`, `[data-field=health]` (`[data-health-count]`, live;
//                       V2-25: no `[data-field=epic]` — the switcher went with the `epics` activation),
//                       `[data-form=search] input` (enter → #/search/:q); STUDIO-30: `[data-pages]`, the pages dropdown —
//                       `[data-action="pages.open"]` (its toggle, `[data-pages-here]` the page you are on), open:
//                       `[data-pages-menu]` with `input[data-pages-filter]` and `[data-page=<name>]` (href the pattern,
//                       `[data-active]` the arrows' option, `aria-current=page` the one you are on);
//                       `[data-action="palette.open"]` (emits palette.open); STUDIO-11: for an editor
//                       (auth.requires ui.author, held on *) `[data-action="page.edit"]` — the pencil, ctrl+e —
//                       to /studio/#/page/<the hash, encodeURIComponent'd>; a viewer has neither
//   board-rows-sv       `.board-row[data-id]` with `[data-selected]` (one) and `[data-checked]` (any), the check
//                       box `[data-check=<id>]` (`[data-action=check]`), `[data-field=checked]` (how many)
//   keys-help           `[data-surface=keys-help]` (the `keys` section) with `[data-chord]` rows; it says what it
//                       heard on the bus: `data-selected-ids` (board.selected's ids, comma-joined) and
//                       `data-focus` (board.focus's id)
//   the host's overlay  `[data-engine-keys]` (FRAME-36), `[data-component=board-rows-sv]`, `[data-chord]`
//
// The keys are FRAME-36's: a window listener the test adds before the page's
// scripts (an init script) records each press and reads `defaultPrevented`
// after the host had it; a document listener says whether it reached the page.
import { makeKey, readToken, sessionCookies, writeSession } from "../../scripts/throwaway-login.mjs";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, marked, sameDocument, sy, main, section, boardUp, BOARDS, server, path, fs, ticketAt } = ctx;
  const B = BOARDS.sv;

  const SPY = () => {
    window.__keys = [];
    addEventListener(
      "keydown",
      (e) => {
        const rec = { key: e.key, ctrl: e.ctrlKey, prevented: null, reached: false };
        window.__keys.push(rec);
        setTimeout(() => (rec.prevented = e.defaultPrevented), 0);
      },
      true,
    );
    document.addEventListener("keydown", () => {
      const r = window.__keys.at(-1);
      if (r) r.reached = true;
    }, true);
  };

  /** The board, with the spy in before the page's scripts, focus on the board's head (no input). */
  async function board(route = "#/board") {
    const { context, page, seen } = await openPage(route);
    await context.addInitScript(SPY);
    await page.reload();
    await boardUp(page, B);
    await calm(page);
    return { context, page, seen };
  }
  /** Focus off every field: a click on the board's head. */
  const calm = (page) => main(page, B).locator(".board-head").click({ timeout: 2000 });
  const lastKey = async (page) => {
    await sleep(30);
    return page.evaluate(() => window.__keys.at(-1));
  };
  const rows = (page) => main(page, B).locator(".board-row[data-id]");
  const ids = (page) => rows(page).evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
  const selected = (page) => main(page, B).locator(".board-row[data-selected]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
  const checked = (page) => main(page, B).locator(".board-row[data-checked]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
  const layoutSays = (page, attr) => section(page, B, "keys").locator("[data-surface=keys-help]").getAttribute(attr);
  const hash = (page) => page.evaluate(() => location.hash);
  const top = (page) => section(page, B, "top").locator("[data-surface=app-top]");

  await test("the_top_shows_the_health_and_no_epic", async () => {
    const health = () => sy("tickets", "health").find((e) => e.type === "health").ticket_count;
    const { page } = await board();
    const t = top(page);
    await eventually("app-top is drawn in the layout's top section", async () => (await t.count()) === 1, 5_000);
    // V2-25: no epic switcher — the board is the organization's whole set.
    assert((await t.locator("[data-field=epic]").count()) === 0, "no epic field in the top");
    const n0 = health();
    const shown = async () => Number((await t.locator("[data-field=health] [data-health-count]").textContent()).replace(/[^0-9]/g, ""));
    await eventually(`the health says ${n0}`, async () => (await shown()) === n0, 5_000);
    // STUDIO-30: the fixed links went for the pages dropdown; it says the board, and lists the board's pages.
    assert((await t.locator("[data-link]").count()) === 0, "STUDIO-30: no fixed links");
    assert((await t.locator("[data-pages-here]").textContent()).trim() === "board", "the dropdown says the board");
    await t.locator('[data-action="pages.open"]').click();
    await eventually("the dropdown lists the board's pages", async () => (await t.locator("[data-page]").count()) >= 6, 5_000);
    for (const [link, href] of [["board", "#/board"], ["queue", "#/queue"], ["ready", "#/ready"], ["waves", "#/waves"], ["filters", "#/filters"], ["new", "#/new"]]) {
      assert((await t.locator(`[data-page="${link}"]`).getAttribute("href")) === href, `the ${link} page is ${href}`);
    }
    assert((await t.locator('[data-page="me"]').count()) === 0, "V2-25: no me page");
    assert((await t.locator('[data-page="board"]').getAttribute("aria-current")) === "page", "the board is the current page");
    await page.keyboard.press("Escape");
    await eventually("escape closes the dropdown", async () => (await t.locator("[data-pages-menu]").count()) === 0, 2_000);
    await t.locator('[data-action="palette.open"]').click();
    await eventually("the palette's button opens the palette (palette.open on the bus)", async () => (await section(page, B, "palette").locator("input[data-palette-input]").count()) === 1, 2_000);
    await page.keyboard.press("Escape");
    await eventually("escape closes it", async () => (await section(page, B, "palette").locator("input[data-palette-input]").count()) === 0, 2_000);

    // Live: a ticket made elsewhere is counted, one deleted is taken off.
    const epic = ctx.extentsNow()[0].epic;
    const [made] = sy("tickets", "create", "--epic", epic, "--title", "the top's kettle");
    assert(made?.ticket?.id, `made: ${JSON.stringify(made)}`);
    await eventually(`the health counts the new ticket (${n0 + 1})`, async () => (await shown()) === n0 + 1, 5_000);
    sy("tickets", "delete", "--ids", made.ticket.id, "--message", "the top's test");
    await eventually(`the health takes the deleted one off (${n0})`, async () => (await shown()) === n0, 5_000);
    assert(health() === n0, "the service agrees");

    // Dark, once: the top on --panel's dark value.
    const bg = (theme) => page.evaluate((theme) => {
      document.documentElement.dataset.theme = theme;
      return getComputedStyle(document.querySelector("[data-surface=app-top]")).backgroundColor;
    }, theme);
    assert((await bg("light")) === "rgb(255, 255, 255)", "light: the top on --panel");
    assert((await bg("dark")) === "rgb(28, 27, 25)", "dark: the top on --panel's dark");
    const help = page.locator("[data-surface=keys-help]");
    assert((await help.locator("[data-chord]").count()) >= 8, "keys-help lists the board's chords at the nav's foot");
  });

  // STUDIO-30: every argument-free page in one dropdown, read live from the routes.
  await test("the_top_lists_every_page_live", async () => {
    // What the live bundle routes (ui/routes.json, no row over it yet): no `:param`, no `#/rhx/` or `#/sv/`
    // alias, one entry per component; the board's pages first, then the rest by name.
    const routes = JSON.parse(fs.readFileSync(path.join(path.dirname(new URL(import.meta.url).pathname), "../../ui/routes.json"), "utf8"));
    const FIRST = ["board", "queue", "ready", "waves", "filters", "search", "new", "bulk"];
    const nameOf = (p) => p.replace(/^#\//, "").replace(/\/$/, "") || "home";
    const free = Object.entries(routes).filter(([p]) => !p.includes("/:") && !/^#\/(rhx|sv)\//.test(p));
    const rest = free.map(([p]) => nameOf(p)).filter((n) => !FIRST.includes(n)).sort();
    const want = [...FIRST, ...rest];
    assert(new Set(free.map(([, c]) => c)).size === free.length, `today's argument-free routes are one per component: ${JSON.stringify(free)}`);
    const hrefOf = Object.fromEntries(free.map(([p]) => [nameOf(p), p]));
    const listed = (t) => t.locator("[data-page]").evaluateAll((els) => els.map((e) => [e.getAttribute("data-page"), e.getAttribute("href")]));

    const { page } = await board();
    const origin = await marked(page);
    const t = top(page);
    await eventually("app-top drawn", async () => (await t.count()) === 1, 5_000);
    const toggle = t.locator('[data-action="pages.open"]');
    await toggle.click();
    await eventually(`the dropdown lists ${want.join(", ")}`, async () => JSON.stringify((await listed(t)).map(([n]) => n)) === JSON.stringify(want), 5_000);
    for (const [n, href] of await listed(t)) {
      assert(href === hrefOf[n], `${n} goes to ${hrefOf[n]}, not ${href}`);
      assert(!href.includes(":") && !/^#\/(rhx|sv)\//.test(href), `no alias or argument: ${href}`);
      assert((await t.locator(`[data-page="${n}"] .top-page-href`).textContent()) === href, `${n} shows its address`);
    }
    assert((await t.locator("[data-pages-filter]").evaluate((e) => e === document.activeElement)), "opened, the filter has the focus");

    // A click on one goes there.
    await t.locator('[data-page="queue"]').click();
    await eventually("a click on queue goes to #/queue", async () => (await hash(page)) === "#/queue", 3_000);
    await eventually("closed after it", async () => (await t.count()) === 0 || (await top(page).locator("[data-pages-menu]").count()) === 0, 2_000);

    // The keyboard: Enter opens, typing filters, the arrows move, Enter goes; Space opens, Escape closes.
    await page.evaluate(() => (location.hash = "#/board"));
    await boardUp(page, B);
    const tt = top(page);
    await eventually("the dropdown says the board again", async () => (await tt.locator("[data-pages-here]").textContent()).trim() === "board", 3_000);
    await tt.locator('[data-action="pages.open"]').focus();
    await page.keyboard.press("Enter");
    await eventually("enter opens it", async () => (await tt.locator("[data-pages-menu]").count()) === 1, 2_000);
    assert((await tt.locator("[data-page][data-active]").getAttribute("data-page")) === "board", "the arrows start on the page you are on");
    await page.keyboard.press("ArrowDown");
    assert((await tt.locator("[data-page][data-active]").getAttribute("data-page")) === "queue", "down moves to queue");
    await page.keyboard.press("ArrowUp");
    assert((await tt.locator("[data-page][data-active]").getAttribute("data-page")) === "board", "up moves back");
    await page.keyboard.type("re");
    await eventually("typing filters", async () => JSON.stringify((await listed(tt)).map(([n]) => n)) === JSON.stringify(want.filter((n) => n.includes("re") || hrefOf[n].includes("re"))), 2_000);
    await page.keyboard.type("lea");
    await eventually("…to releases alone", async () => JSON.stringify((await listed(tt)).map(([n]) => n)) === JSON.stringify(["releases"]), 2_000);
    await page.keyboard.press("Enter");
    await eventually("enter goes to #/releases", async () => (await hash(page)) === "#/releases", 3_000);
    const rt = page.locator("[data-surface=app-top]");
    await eventually("on releases the dropdown says releases", async () => (await rt.count()) === 1 && (await rt.locator("[data-pages-here]").textContent()).trim() === "releases", 5_000);
    await rt.locator('[data-action="pages.open"]').focus();
    await page.keyboard.press(" ");
    await eventually("space opens it", async () => (await rt.locator("[data-pages-menu]").count()) === 1, 2_000);
    await page.keyboard.press("Escape");
    await eventually("escape closes it", async () => (await rt.locator("[data-pages-menu]").count()) === 0, 2_000);
    assert(await rt.locator('[data-action="pages.open"]').evaluate((e) => e === document.activeElement), "the focus is back on the toggle");
    assert((await hash(page)) === "#/releases", "escape went nowhere");
    // ctrl+k is unchanged: the palette's button is still there.
    assert((await rt.locator('[data-action="palette.open"]').count()) === 1, "the palette's button stays");

    // A route the studio sets appears in the open page's dropdown, no reload; cleared, it goes.
    const set = sy("ui", "routes", "set", "--pattern", "#/kiln-log", "--component", "page-thread");
    assert(set.some((e) => e.type === "route_set"), `route set: ${JSON.stringify(set)}`);
    try {
      await rt.locator('[data-action="pages.open"]').click();
      const withKiln = [...FIRST, ...[...rest, "kiln-log"].sort()];
      await eventually("kiln-log appears in its place, without a reload", async () => JSON.stringify((await listed(rt)).map(([n]) => n)) === JSON.stringify(withKiln), 5_000);
      assert((await rt.locator('[data-page="kiln-log"]').getAttribute("href")) === "#/kiln-log", "it goes to #/kiln-log");
      await sameDocument(page, origin, "the route arrived on ui.watch");
    } finally {
      const cleared = sy("ui", "routes", "clear", "--pattern", "#/kiln-log");
      assert(cleared.some((e) => e.type === "route_cleared"), `route cleared: ${JSON.stringify(cleared)}`);
    }
    await eventually("cleared, kiln-log goes", async () => JSON.stringify((await listed(rt)).map(([n]) => n)) === JSON.stringify(want), 5_000);
    await sameDocument(page, origin, "the clear arrived on ui.watch");
  });

  await test("search_from_the_top_routes", async () => {
    const { page } = await board();
    const input = top(page).locator("[data-form=search] input");
    await input.fill("kettle");
    await input.press("Enter");
    await eventually("enter in the top's search goes to #/search/kettle", async () => (await hash(page)) === "#/search/kettle", 3_000);
    await page.evaluate(() => (location.hash = "#/board"));
    await boardUp(page, B);
    const again = top(page).locator("[data-form=search] input");
    await again.fill("glaze/ash #2");
    await again.press("Enter");
    await eventually("a query's / and # are escaped (the space is the browser's %20)", async () => (await hash(page)).replace(/%20/g, " ") === "#/search/glaze%2Fash %232", 3_000);
    // Under the board's filter, the search carries it (SURFACE-8's #/search/:q/f/:filter).
    await page.evaluate(() => (location.hash = "#/f/ready"));
    await boardUp(page, B);
    const under = top(page).locator("[data-form=search] input");
    await under.fill("kettle");
    await under.press("Enter");
    await eventually("under #/f/ready the search goes to #/search/kettle/f/ready", async () => (await hash(page)) === "#/search/kettle/f/ready", 3_000);
  });

  await test("j_k_move_the_selection_and_enter_opens", async () => {
    const { page } = await board();
    const first = await ids(page);
    assert(first.length > 5, `rows drawn: ${first.length}`);
    assert((await selected(page)).length === 0, "nothing selected at first");
    const press = async (key, want) => {
      await page.keyboard.press(key);
      await eventually(`${key} selects ${want}`, async () => JSON.stringify(await selected(page)) === JSON.stringify([want]), 2_000);
      const k = await lastKey(page);
      assert(k.prevented === true && k.reached === false, `${key} is the host's: ${JSON.stringify(k)}`);
    };
    await press("j", first[0]);
    await press("j", first[1]);
    await press("j", first[2]);
    await press("k", first[1]);
    await press("ArrowDown", first[2]);
    await press("ArrowUp", first[1]);
    await eventually("board.focus heard on the bus", async () => (await layoutSays(page, "data-focus")) === first[1], 2_000);

    // Scrolled into view: forty rows down is below the fold, and in view.
    for (let i = 0; i < 40; i++) await page.keyboard.press("j");
    await eventually(`j × 40 selects ${first[41]}`, async () => JSON.stringify(await selected(page)) === JSON.stringify([first[41]]), 3_000);
    const seen = await main(page, B).evaluate((m) => {
      const s = m.querySelector(".board-scroll").getBoundingClientRect();
      const r = m.querySelector(".board-row[data-selected]").getBoundingClientRect();
      return r.top >= s.top - 1 && r.bottom <= s.bottom + 1;
    });
    assert(seen, "the selected row is scrolled into view");
    for (let i = 0; i < 40; i++) await page.keyboard.press("k");
    await eventually(`k × 40 is back on ${first[1]}`, async () => JSON.stringify(await selected(page)) === JSON.stringify([first[1]]), 3_000);

    // Kept across a window move: far down, and back.
    const scroller = main(page, B).locator(".board-scroll");
    await scroller.evaluate((el) => (el.scrollTop = 28 * 5_000));
    await eventually("the window moved", async () => !(await ids(page)).includes(first[1]), 5_000);
    await scroller.evaluate((el) => (el.scrollTop = 0));
    await eventually("the window is back", async () => (await ids(page)).includes(first[1]), 5_000);
    await eventually(`${first[1]} is still selected`, async () => JSON.stringify(await selected(page)) === JSON.stringify([first[1]]), 3_000);

    await calm(page);
    await page.keyboard.press("Enter");
    await eventually(`enter opens ${first[1]}'s page`, async () => ticketAt(await hash(page), first[1]), 3_000);
  });

  await test("x_checks_rows_and_board_selected_is_emitted", async () => {
    const { page } = await board();
    const r = await ids(page);
    // By click (no key): the box checks its row and selects it.
    await main(page, B).locator(`[data-check="${r[4]}"]`).click();
    await eventually("a click on the box checks the row", async () => JSON.stringify(await checked(page)) === JSON.stringify([r[4]]), 2_000);
    assert(JSON.stringify(await selected(page)) === JSON.stringify([r[4]]), "… and selects it");
    await eventually("board.selected { ids } heard", async () => (await layoutSays(page, "data-selected-ids")) === r[4], 2_000);
    await main(page, B).locator(`[data-check="${r[4]}"]`).click();
    await eventually("a second click unchecks it", async () => (await checked(page)).length === 0, 2_000);
    await eventually("board.selected { ids: [] } heard", async () => (await layoutSays(page, "data-selected-ids")) === "", 2_000);

    await calm(page);
    await page.keyboard.press("Escape");
    for (const k of ["j", "x", "j", "j", "x"]) await page.keyboard.press(k);
    await eventually("x checked the first and the third", async () => JSON.stringify((await checked(page)).sort()) === JSON.stringify([r[0], r[2]].sort()), 2_000);
    await eventually("board.selected says both", async () => (await layoutSays(page, "data-selected-ids")) === `${r[0]},${r[2]}`, 2_000);
    const k = await lastKey(page);
    assert(k.prevented === true && k.reached === false, `x is the host's: ${JSON.stringify(k)}`);
    await page.keyboard.press("j");
    await page.keyboard.press("j");
    await page.keyboard.press("Shift+X");
    await eventually("shift+x checks the range from the last checked to here", async () => JSON.stringify((await checked(page)).sort()) === JSON.stringify([r[0], r[2], r[3], r[4]].sort()), 2_000);
    assert(Number((await main(page, B).locator("[data-field=checked]").textContent()).replace(/[^0-9]/g, "")) === 4, "the head says 4 checked");
    await page.keyboard.press("Escape");
    await eventually("escape clears the selection and the checks", async () => (await checked(page)).length === 0 && (await selected(page)).length === 0, 2_000);
    await eventually("board.selected { ids: [] } heard", async () => (await layoutSays(page, "data-selected-ids")) === "", 2_000);
  });

  await test("slash_focuses_the_filter_and_escape_clears", async () => {
    const { page } = await board();
    await page.keyboard.press("/");
    const k = await lastKey(page);
    assert(k.prevented === true && k.reached === false, `/ is the host's: ${JSON.stringify(k)}`);
    assert((await page.locator("input[data-palette-input]").count()) === 0, "/ does not open the palette (ctrl+k does, the palette's own)");
    // `/` says filter.focus on the bus, and the filter line (filter-line-sv, SURFACE-6) takes it.
    await eventually("/ focuses the filter line", async () => page.evaluate(() => document.activeElement?.matches?.("input[data-filter-input]") ?? false), 2_000);
    const typed = await page.locator("input[data-filter-input]").inputValue();
    assert(typed === "", `the / was typed nowhere: ${JSON.stringify(typed)}`);
    // Out of the line, so the board's keys are the board's again.
    await page.evaluate(() => document.activeElement?.blur?.());
    // escape clears the selection.
    await page.keyboard.press("j");
    await eventually("j selected a row", async () => (await selected(page)).length === 1, 2_000);
    await page.keyboard.press("Escape");
    await eventually("escape clears it", async () => (await selected(page)).length === 0, 2_000);
    // c and the g chords route.
    for (const [keys, want] of [[["c"], "#/new"], [["g", "r"], "#/ready"], [["g", "w"], "#/waves"], [["g", "b"], "#/board"]]) {
      await page.evaluate(() => (location.hash = "#/f/ready"));
      await boardUp(page, B);
      await calm(page);
      for (const key of keys) await page.keyboard.press(key);
      await eventually(`${keys.join(" ")} goes to ${want}`, async () => (await hash(page)) === want, 3_000);
    }
  });

  await test("a_bound_key_is_prevented_on_the_board", async () => {
    const { page } = await board();
    for (const key of ["/", "Control+k"]) {
      await calm(page);
      await page.keyboard.press(key);
      const k = await lastKey(page);
      assert(k.prevented === true && k.reached === false, `${key} is prevented and never reaches the page: ${JSON.stringify(k)}`);
    }
    // ctrl+k opened the palette (its own key): escape in its field closes it.
    await eventually("ctrl+k opened the palette", async () => (await section(page, B, "palette").locator("input[data-palette-input]").count()) === 1, 2_000);
    await page.keyboard.press("Escape");
    await eventually("escape closed it", async () => (await section(page, B, "palette").locator("input[data-palette-input]").count()) === 0, 2_000);
    await calm(page);
    await page.keyboard.press("?");
    const q = await lastKey(page);
    assert(q.prevented === true && q.reached === false, `? is the host's: ${JSON.stringify(q)}`);
    const overlay = page.locator("[data-engine-keys]");
    await eventually("? opens the host's overlay", async () => (await overlay.count()) === 1, 2_000);
    const mine = overlay.locator("[data-component=board-rows-sv]");
    assert((await mine.count()) === 1, "the overlay has board-rows-sv's section");
    const chords = await mine.locator("[data-chord]").evaluateAll((els) => els.map((e) => [e.getAttribute("data-chord"), e.querySelector("[data-label]")?.textContent ?? ""]));
    for (const c of ["j", "k", "arrowdown", "arrowup", "enter", "x", "shift+x", "c", "/", "g b", "g r", "g q", "g w", "escape"]) {
      const row = chords.find(([chord]) => chord === c);
      assert(row && row[1].trim() && row[1] !== c, `the overlay lists ${c} with its label: ${JSON.stringify(chords)}`);
    }
    await page.keyboard.press("Escape");
    await eventually("escape closes the overlay", async () => (await overlay.count()) === 0, 2_000);
    await calm(page);
    await page.keyboard.press("t");
    const t = await lastKey(page);
    assert(t.prevented === false && t.reached === true, `t, unbound, is the page's: ${JSON.stringify(t)}`);
  });

  await test("ctrl_e_edits_this_page_for_an_editor", async () => {
    const { page } = await board("#/f/ready");
    const pencil = top(page).locator('[data-action="page.edit"]');
    await eventually("the pencil is lit for an editor", async () => (await pencil.count()) === 1, 5_000);
    const want = "/studio/#/page/" + encodeURIComponent("#/f/ready");
    assert(((await pencil.getAttribute("href")) ?? "") === want, `the pencil goes to ${want}: ${await pencil.getAttribute("href")}`);
    assert(((await pencil.textContent()) ?? "").includes(want.replace("/studio/", "")) || ((await pencil.getAttribute("title")) ?? "").includes("#/f/ready"), "its tooltip names where it goes");
    // The host's overlay lists it under app-top.
    await calm(page);
    await page.keyboard.press("?");
    const overlay = page.locator("[data-engine-keys]");
    await eventually("? opens the overlay", async () => (await overlay.count()) === 1, 2_000);
    const mine = await overlay.locator("[data-component=app-top] [data-chord]").evaluateAll((els) => els.map((e) => [e.getAttribute("data-chord"), e.querySelector("[data-label]")?.textContent ?? ""]));
    assert(mine.some(([c, l]) => c === "ctrl+e" && l.includes("edit this page")), `the overlay lists ctrl+e · edit this page under app-top: ${JSON.stringify(mine)}`);
    await page.keyboard.press("Escape");
    await calm(page);
    await page.keyboard.press("Control+e");
    await eventually(`ctrl+e goes to ${want}`, async () => {
      const u = new URL(page.url());
      return u.pathname + u.hash === want;
    }, 5_000);
  });

  await test("ctrl_e_does_nothing_for_a_viewer", async () => {
    const keyFile = path.join(server.work, "viewer-edit.key");
    const principal = makeKey(keyFile);
    const g = sy("members", "grant", "--principal", principal, "--scope", "*", "--role", "viewer");
    assert(g.some((e) => e.type === "grant"), `viewer granted: ${JSON.stringify(g)}`);
    const file = path.join(server.work, "viewer-edit.session");
    await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, file, "viewer");
    const { context, page: first } = await openPage("");
    await first.close();
    await context.clearCookies();
    await context.addCookies(sessionCookies(server.origin, readToken(file)));
    await context.addInitScript(SPY);
    const page = await context.newPage();
    try {
      await page.goto(`${server.origin}/app/#/f/ready`);
      await boardUp(page, B);
      await eventually("app-top drawn", async () => (await top(page).count()) === 1, 5_000);
      await sleep(1_000);
      assert((await top(page).locator('[data-action="page.edit"]').count()) === 0, "a viewer has no pencil");
      await calm(page);
      await page.keyboard.press("Control+e");
      await sleep(500);
      const u = new URL(page.url());
      assert(u.pathname === "/app/" && u.hash === "#/f/ready", `ctrl+e moved a viewer to ${page.url()}`);
    } finally {
      await context.close();
      sy("members", "revoke", "--principal", principal, "--scope", "*");
    }
  });
};
