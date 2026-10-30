// SURFACE-7: the order of work as svelte/1 components — `work-ready` (#/ready,
// #/ready/:filter), `work-waves` (#/waves, #/waves/in/:epics, #/waves/:id,
// #/waves/to/:to) and `work-blockers` (the ticket page's mark
// `<work-blockers id={id}/>`, SURFACE-3; until it is placed there, the route
// #/blockers/:id mounts it alone).
//
// Marks: `[data-surface=work-ready]` with rows `.board-row[data-id]`
// (`[data-field=id|title|kind|status]`), `[data-empty]`, `[data-loading]`,
// `[data-refused]`; `[data-surface=work-waves]` with lanes
// `[data-wave=N]`, chips `.chip[data-id]`, lines `.edge[data-from][data-to]`;
// `[data-surface=work-blockers]` with `[data-side=blocker|unlocks]` and its
// chips `[data-surface=work-blocker] [data-id]`.
//
// The data is synthetic, made here in shortcodes of the cases' own.
export default async (ctx) => {
  const { test, assert, eventually, openPage, sy } = ctx;

  const made = (ev) => {
    const t = ev.find((e) => e.type === "created")?.ticket;
    if (!t) throw new Error(`create: ${JSON.stringify(ev)}`);
    return t.id;
  };
  const create = (epic, title, ...more) => made(sy("tickets", "create", "--epic", epic, "--title", title, ...more));
  const status = (id, s) => {
    const ev = sy("tickets", "set_status", "--id", id, "--status", s);
    if (ev.some((e) => e.type === "error" || e.type === "refused")) throw new Error(`set_status ${id} ${s}: ${JSON.stringify(ev)}`);
  };
  const ids = (loc) => loc.evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
  const enc = (s) => encodeURIComponent(s);

  await test("ready_lists_and_is_live", async () => {
    const root = create("WRKA", "the ready probe", "--kind", "scope");
    const a = create("WRKA", "a ready build", "--kind", "build", "--parent", root, "--severity", "High");
    const held = create("WRKA", "a blocker still coding", "--kind", "build", "--parent", root);
    const b = create("WRKA", "ready but held", "--kind", "build", "--parent", root, "--blocked-by", held);
    status(a, "Ready");
    status(held, "Coding");
    status(b, "Ready");
    const { page, context } = await openPage(`#/ready/${enc("in WRKA")}`);
    try {
      const W = page.locator("[data-surface=work-ready]");
      const rows = W.locator(".board-row[data-id]");
      await eventually("work-ready draws its rows", async () => (await ids(rows)).includes(a), 15_000);
      let now = await ids(rows);
      assert(!now.includes(b) && !now.includes(held) && !now.includes(root), `only the ready set: ${now}`);
      const title = await W.locator(`.board-row[data-id="${a}"] [data-field=title]`).textContent();
      assert(title.includes("a ready build"), `the row's title: ${title}`);
      assert((await W.locator(`.board-row[data-id="${a}"] [data-field=status]`).textContent()).trim() === "Ready", "the row's status chip");
      // STUDIO-12: the chip is a bound mark inside the page's own bound mark — waited for, as every mark is.
      await eventually("the parent as a ticket-chip", async () => (await W.locator(`.board-row[data-id="${a}"] [data-component=ticket-chip] [data-chip-id="${root}"]`).count()) === 1, 5_000);
      assert((await W.locator(`.board-row[data-id="${a}"] .sev[data-sev=High]`).count()) === 1, "the severity bars");
      // A blocker completing lets what it held arrive; a ticket taken leaves.
      status(held, "Complete");
      await eventually(`${b} arrives when its blocker completes`, async () => (await ids(rows)).includes(b), 10_000);
      await eventually(`${b}'s row is titled`, async () => ((await W.locator(`.board-row[data-id="${b}"] [data-field=title]`).textContent({ timeout: 500 })) ?? "").includes("ready but held"), 5_000);
      status(a, "Coding");
      await eventually(`${a} leaves when it is taken`, async () => !(await ids(rows)).includes(a), 10_000);
      const c = create("WRKA", "a new one, made ready", "--kind", "gap", "--parent", root);
      status(c, "Ready");
      await eventually(`${c} arrives when it enters Ready`, async () => (await ids(rows)).includes(c), 10_000);
      // The count says the rows; a filter nothing matches is the empty sentence.
      assert((await W.locator("[data-field=count]").textContent()).trim() === String((await ids(rows)).length), "the head's count");
      await page.evaluate((r) => (location.hash = r), `#/ready/${enc("in WRKA and kind spike")}`);
      await eventually("an empty ready set says so", async () => (await W.locator("[data-empty]").count()) === 1, 10_000);
      // A filter the language refuses is said where it was asked.
      await page.evaluate((r) => (location.hash = r), `#/ready/${enc("kind nonsense")}`);
      await eventually("a refused filter is [data-refused]", async () => (await W.locator("[data-refused]").count()) === 1, 10_000);
      // The unfiltered set draws rows too, and the dark scheme paints them.
      await page.evaluate(() => (location.hash = "#/ready"));
      await eventually("#/ready draws rows", async () => (await rows.count()) > 0, 15_000);
      // Past 200 the head says the cut: the first 200 by id, of how many.
      await eventually("the cut-off is said with the count", async () => Number(await W.locator("[data-approx]").getAttribute("data-approx", { timeout: 500 })) > 200, 10_000);
      const light = await rows.first().locator("[data-field=status]").evaluate((e) => getComputedStyle(e).backgroundColor);
      await page.evaluate(() => document.documentElement.setAttribute("data-theme", "dark"));
      const dark = await rows.first().locator("[data-field=status]").evaluate((e) => getComputedStyle(e).backgroundColor);
      assert(light !== dark, `the dark scheme repaints the status chip (${light} → ${dark})`);
    } finally {
      await context.close();
    }
  });

  await test("waves_draw_lanes_in_order", async () => {
    const root = create("WRKB", "the waves probe", "--kind", "scope");
    const A = create("WRKB", "wave one", "--kind", "build", "--parent", root);
    const B = create("WRKB", "wave two", "--kind", "build", "--parent", root, "--blocked-by", A);
    const C = create("WRKB", "wave three", "--kind", "build", "--parent", root, "--blocked-by", B);
    const { page, context } = await openPage(`#/waves/in/WRKB`);
    try {
      const W = page.locator("[data-surface=work-waves]");
      const lane = (n) => W.locator(`[data-wave="${n}"]`);
      await eventually("three lanes", async () => (await W.locator("[data-wave]").count()) === 3, 15_000);
      const order = await W.locator("[data-wave]").evaluateAll((els) => els.map((e) => [e.getAttribute("data-wave"), e.getBoundingClientRect().left]));
      assert(order.map((o) => o[0]).join(",") === "1,2,3", `lanes in order: ${JSON.stringify(order)}`);
      assert(order[0][1] < order[1][1] && order[1][1] < order[2][1], `left to right: ${JSON.stringify(order)}`);
      for (const [n, id] of [[1, A], [2, B], [3, C]]) {
        assert((await lane(n).locator(`.chip[data-id="${id}"]`).count()) === 1, `${id} in wave ${n}`);
      }
      // Each blocker a line from the earlier lane into the later one, its
      // ends at the two chips.
      const edges = await W.locator(".edge").evaluateAll((els) => els.map((e) => `${e.getAttribute("data-from")}>${e.getAttribute("data-to")}`));
      assert(edges.includes(`${A}>${B}`) && edges.includes(`${B}>${C}`) && edges.length === 2, `the lines: ${edges}`);
      const geo = await page.evaluate(([a, b]) => {
        const r = (s) => document.querySelector(s).getBoundingClientRect();
        const e = document.querySelector(`[data-surface=work-waves] .edge[data-from="${a}"][data-to="${b}"]`).getBoundingClientRect();
        return { from: r(`[data-surface=work-waves] .chip[data-id="${a}"]`), to: r(`[data-surface=work-waves] .chip[data-id="${b}"]`), e };
      }, [A, B]);
      assert(Math.abs(geo.e.left - geo.from.right) < 4 && Math.abs(geo.e.right - geo.to.left) < 6, `the line runs from ${A}'s right edge to ${B}'s left: ${JSON.stringify(geo)}`);
      // A hovered chip lights its lines.
      await W.locator(`.chip[data-id="${B}"]`).hover();
      await eventually("the hovered chip's lines light", async () => (await W.locator('.edge[data-on="1"]').count()) === 2, 3_000);
      // A click opens the ticket.
      await W.locator(`.chip[data-id="${C}"]`).click();
      await eventually("a chip opens its ticket", async () => (await page.evaluate(() => location.hash)) === `#/ticket/${C}`, 5_000);
      // No shortcode named: the epic's shortcodes to choose from.
      await page.evaluate(() => (location.hash = "#/waves"));
      await eventually("#/waves offers the shortcodes", async () => (await W.locator('[data-param=epics] a[data-epic="WRKB"]').count()) === 1, 10_000);
      await W.locator('[data-param=epics] a[data-epic="WRKB"]').click();
      await eventually("choosing one draws its waves", async () => (await W.locator("[data-wave]").count()) === 3, 10_000);
      // The kinds toggle: no build, no lanes.
      await W.locator('[data-param=kinds] button[data-kind="build"]').click();
      await eventually("kinds without build: empty", async () => (await W.locator("[data-empty]").count()) === 1, 10_000);
    } finally {
      await context.close();
    }
  });

  await test("waves_under_and_to_by_route", async () => {
    const root = create("WRKC", "the under probe", "--kind", "scope");
    const A = create("WRKC", "first", "--kind", "build", "--parent", root);
    const B = create("WRKC", "second", "--kind", "build", "--parent", root, "--blocked-by", A);
    const other = create("WRKD", "elsewhere", "--kind", "scope");
    const X = create("WRKD", "waits across", "--kind", "build", "--parent", other, "--blocked-by", B);
    const lone = create("WRKD", "waits on nothing", "--kind", "build", "--parent", other);
    const { page, context } = await openPage(`#/waves/${root}`);
    try {
      const W = page.locator("[data-surface=work-waves]");
      const chips = W.locator(".chip[data-id]");
      await eventually(`waves under ${root}`, async () => (await ids(chips)).sort().join() === [A, B].sort().join(), 15_000);
      assert((await W.locator(`[data-wave="2"] .chip[data-id="${B}"]`).count()) === 1, `${B} in wave 2 under ${root}`);
      assert((await W.locator("[data-mode]").getAttribute("data-mode")) === "under", "the mode is said");
      await page.evaluate((r) => (location.hash = r), `#/waves/to/${X}`);
      await eventually(`waves to ${X}: its blockers, then it, alone and last`, async () => {
        const lanes = await W.locator("[data-wave]").evaluateAll((els) => els.map((e) => [...e.querySelectorAll(".chip[data-id]")].map((c) => c.getAttribute("data-id")).join("+")));
        return lanes.join("|") === `${A}|${B}|${X}`;
      }, 15_000);
      assert(!(await ids(chips)).includes(lone), "only what must happen first");
      assert((await W.locator("[data-mode]").getAttribute("data-mode")) === "to", "the mode is said");
      await page.evaluate(() => (location.hash = "#/waves/NOPE-404"));
      await eventually("an unknown ticket is its own sentence", async () => (await W.locator('[data-empty][data-not-found="NOPE-404"]').count()) === 1, 10_000);
    } finally {
      await context.close();
    }
  });

  await test("blockers_in_the_ticket_page", async () => {
    const root = create("WRKE", "the blockers probe", "--kind", "scope");
    const b1 = create("WRKE", "first blocker", "--kind", "build", "--parent", root);
    const b2 = create("WRKE", "second blocker", "--kind", "build", "--parent", root);
    const t = create("WRKE", "the one held", "--kind", "build", "--parent", root, "--blocked-by", `${b1},${b2}`);
    const after = create("WRKE", "what it holds", "--kind", "build", "--parent", root, "--blocked-by", t);
    // SURFACE-3 places the mark in the ticket page (`[data-mark=work-blockers]`):
    // where the page draws it, it must be filled; only a page without it (v2
    // before SURFACE-3) falls back to the component's own route.
    const { page, context } = await openPage(`#/ticket/${t}`);
    try {
      await eventually("the ticket page is drawn", async () => (await page.locator("[data-ticket-title]").count()) > 0, 15_000);
      let W = page.locator("[data-surface=work-blockers]");
      if ((await page.locator("[data-mark=work-blockers]").count()) > 0) {
        await eventually("the ticket page's work-blockers mark is filled", async () => (await page.locator("[data-mark=work-blockers] [data-surface=work-blockers]").count()) === 1, 10_000);
        W = page.locator("[data-mark=work-blockers] [data-surface=work-blockers]");
      } else {
        console.log("     the ticket page draws no [data-mark=work-blockers] (SURFACE-3 places it): mounted by #/blockers/:id");
        await page.evaluate((r) => (location.hash = r), `#/blockers/${t}`);
      }
      const side = (s) => W.locator(`[data-side=${s}] [data-surface=work-blocker] .chip[data-id]`);
      await eventually("both blockers drawn", async () => (await ids(side("blocker"))).sort().join() === [b1, b2].sort().join(), 15_000);
      assert(!(await W.locator("[data-side=blocker] [data-empty]").isVisible()), "the empty sentence hidden while blockers are drawn");
      await eventually("what it blocks drawn", async () => (await ids(side("unlocks"))).join() === after, 5_000);
      await eventually("each chip titled", async () => ((await side("blocker").filter({ hasText: "first blocker" }).count()) === 1), 5_000);
      // A blocker completed through synapse drops out, live.
      status(b1, "Complete");
      await eventually(`${b1} drops out when it completes`, async () => (await ids(side("blocker"))).join() === b2, 10_000);
      // A blocker added after the page opened comes in.
      const b3 = create("WRKE", "a late blocker", "--kind", "gap", "--parent", root);
      sy("tickets", "update", "--id", t, "--block", b3);
      await eventually(`${b3} comes in when it is added`, async () => (await ids(side("blocker"))).includes(b3), 10_000);
      // Nothing waits: the sentence.
      status(b2, "Complete");
      status(b3, "Canceled");
      await eventually("no open blockers: the empty sentence", async () => W.locator("[data-side=blocker] [data-empty]").isVisible(), 10_000);
      // Each live chip holds a get and a watch: past 24 the rest are read once, said as "+N".
      const many = Array.from({ length: 26 }, (_, i) => create("WRKE", `blocker ${i + 1} of many`, "--kind", "build", "--parent", root));
      const big = create("WRKE", "held by many", "--kind", "build", "--parent", root, "--blocked-by", many.join(","));
      await page.evaluate((r) => (location.hash = r), `#/blockers/${big}`);
      const chips = page.locator("[data-surface=work-blockers] [data-side=blocker] [data-surface=work-blocker] .chip[data-id]");
      await eventually("all 26 drawn", async () => (await chips.count()) === 26, 15_000);
      const live = await chips.evaluateAll((els) => els.map((e) => e.getAttribute("data-live")).join(""));
      assert(live === "1".repeat(24) + "00", `the first 24 live, the rest once: ${live}`);
      assert((await page.locator("[data-surface=work-blockers] [data-field=more]").getAttribute("data-approx")) === "2", "+2 not live, said");
    } finally {
      await context.close();
    }
  });
};
