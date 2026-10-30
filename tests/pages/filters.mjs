// SURFACE-6: the saved filters, the facets, the counts and the filter line's
// check — `node tests/pages_browser.mjs --group filters`.
//
// Marks:
//   filters-saved (#/filters, #/filters/:filter)  `[data-surface=filters-saved]`; the form
//       `[data-form="tickets.filters.save"]` with `input[data-field=name]`, `input[data-field=filter]`,
//       `[data-action="tickets.filters.save"]`, its check `[data-check=ok|refused|pending]`
//       (`[data-refused]` the refusal's words, `[data-field=count]` when ok); rows `[data-id=<name>]`
//       (`[data-selected]` the picked one), `[data-pick]` picks, `[data-action=open]` opens on the board,
//       the picked row's `[data-field=detail] [data-field=filter]` (filters.get), `input[data-field=to]`,
//       `[data-action="tickets.filters.rename"]`, `[data-action="tickets.filters.delete"]`, `[data-refused]`
//   nav-facets (#/facets/:filter)  `[data-surface=nav-facets]`, chips `[data-facet=<field>][data-value=<v>]`
//       with `[data-held]`, `[data-selected]` when the filter holds the chip's `data-rule`
//   nav-counts (#/counts/:filter)  `[data-surface=nav-counts] [data-field=count]`, `[data-refused]`
//   filters-saved, nav-facets, nav-counts are svelte/1 (owner, 2026-10-30); the marks are the profile's own business
//   filter-line (the top bar; filter-line.rhx on the rhx board, filter-line-sv on the Svelte one — `#/rhx/` and
//       `#/` once SURFACE-2 makes the Svelte board the default, `#/` and `#/sv/` before)
//       `input[data-filter-input]`, `[data-filter-check=ok|refused]`
//       (`[data-refused]`), saved names offered as `[data-complete=<name>]`
//   PERF-22: the board's `[data-board][data-surface=board-rows]` gains `[data-drawn]` once its first
//       answer is drawn (and says `board.drawn` on the bus); the nav's facets and saved filters ask then —
//       FRAME-50: because the frame places them with `after="board.drawn"`, never from a look at the page
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, openPage, marked, sameDocument, sy, summaries } = ctx;
  const trimmed = (t) => String(t ?? "").trim();
  const one = (events, type) => events.find((e) => e.type === type);
  const got = (name) => sy("tickets", "filters", "get", "--name", name);
  const hash = (page) => page.evaluate(() => decodeURIComponent(location.hash));
  const saved = (page) => page.locator("[data-surface=filters-saved]");
  const facets = (page) => page.locator("[data-surface=nav-facets]");
  // SURFACE-2 makes the Svelte board the default (`#/`, `#/f/:filter` → board-frame; the rhx
  // board at `#/rhx/…`); before it, the rhx board is at `#/` and the Svelte one at `#/sv/`.
  const routes = JSON.parse(ctx.fs.readFileSync(new URL("../../ui/routes.json", import.meta.url), "utf8"));
  const svelteFirst = routes["#/f/:filter"] === "board-frame";
  const boardLayout = svelteFirst ? "board-frame" : "board-layout";
  const held = (filter) => sy("tickets", "nav", "counts", "--filter", filter).filter((e) => e.type === "count").reduce((a, c) => a + c.held, 0);

  // PERF-22: the rows are what the person waits for. On the board the nav's
  // facets (nav.facets) and saved filters (filters.list) ask once the board's
  // first rows are drawn — after the first tickets.list answer, never beside
  // it; on a page of their own, at once. FRAME-50: the frame says so
  // (`after="board.drawn"` on their slots); a page of their own passes no
  // `after` — so it holds whether the sections mount in one task or in turns.
  await test("the_navs_sections_ask_after_the_first_rows", async () => {
    const mark = (page, name) => page.evaluate((name) => performance.getEntriesByType("mark").find((e) => e.name === name)?.startTime ?? null, name);
    {
      const { page, context } = await openPage("");
      try {
        await eventually("the board's rows", async () => (await page.locator(".board-row[data-id]").count()) > 0, 10000);
        await eventually("the facets' chips and the saved filters drawn", async () =>
          (await facets(page).locator("[data-facet]").count()) > 0 && (await saved(page).locator('[data-id="Needs you"]').count()) === 1, 5000);
        assert((await page.locator("[data-board][data-surface=board-rows][data-drawn]").count()) === 1, "the board is marked [data-drawn]");
        const answered = await mark(page, "plexus-ui:answered:tickets.list");
        assert(answered != null, "the host marks the list's first answer");
        const calls = await sentCalls(page);
        const list = calls.find((c) => c.method === "tickets.list");
        const at = (m) => calls.find((c) => c.method === m)?.t;
        for (const m of ["tickets.nav.facets", "tickets.filters.list"]) {
          assert(at(m) != null, `${m} is asked on the board`);
          assert(at(m) > list.t && at(m) >= answered, `${m} asked after the first tickets.list answer (sent +${at(m)?.toFixed(1)}, the list sent +${list.t.toFixed(1)}, answered +${answered.toFixed(1)})`);
        }
        assert(calls.filter((c) => c.method === "tickets.nav.facets").length === 1 && calls.filter((c) => c.method === "tickets.filters.list").length === 1, "each asked once");
      } finally {
        await context.close();
      }
    }
    for (const [route, method, drawn] of [["#/filters", "tickets.filters.list", '[data-surface=filters-saved] [data-id="Needs you"]'],
                                         ["#/facets/kind%20build", "tickets.nav.facets", "[data-surface=nav-facets] [data-facet]"],
                                         ["#/counts/kind%20build", "tickets.nav.counts", "[data-surface=nav-counts] [data-field=count]"]]) {
      const { page, context } = await openPage(route);
      try {
        await eventually(`${route} drawn`, async () => (await page.locator(drawn).count()) > 0, 5000);
        const calls = await sentCalls(page);
        const first = await mark(page, "plexus-ui:first-call");
        const t = calls.find((c) => c.method === method)?.t;
        assert(t != null && first != null && t - first < 50, `${route}: ${method} asked at once, no board to wait for (+${(t - first).toFixed(1)} ms after the first call)`);
      } finally {
        await context.close();
      }
    }
  });

  await test("a_filter_is_saved_and_listed", async () => {
    const { page, context } = await openPage("#/filters/status%20Ready");
    try {
      const fs = saved(page);
      await eventually("the list is drawn (Needs you, the default)", async () => (await fs.locator('[data-id="Needs you"]').count()) === 1);
      const form = fs.locator('[data-form="tickets.filters.save"]');
      assert((await form.locator("input[data-field=filter]").inputValue()) === "status Ready", "the board's filter is handed in to be saved");
      assert((await form.locator('[data-action="tickets.filters.save"][disabled]').count()) === 1, "no name: the save is disabled");
      await form.locator("input[data-field=name]").fill("S6 ready");
      await form.locator('[data-action="tickets.filters.save"]:not([disabled])').click({ timeout: 2000 });
      const row = fs.locator('[data-id="S6 ready"]');
      await eventually("the saved filter is listed, picked", async () => (await row.count()) === 1 && (await row.getAttribute("data-selected")) !== null, 5000);
      const answer = one(got("S6 ready"), "filter_saved");
      assert(answer?.filter?.filter === "status Ready", `filters.get has it: ${JSON.stringify(answer)}`);
      await eventually("its detail is filters.get's: the filter and the count", async () =>
        trimmed(await row.locator("[data-field=detail] [data-field=filter]").textContent({ timeout: 500 })) === "status Ready" &&
        trimmed(await row.locator("[data-field=detail]").textContent()).includes(`${answer.filter.count} tickets`), 5000);
      // Saved elsewhere, it is listed without a reload (watch's filter_saved).
      const origin = await marked(page);
      sy("tickets", "filters", "save", "--name", "S6 elsewhere", "--filter", "kind spike");
      await eventually("a filter saved elsewhere is listed", async () => (await fs.locator('[data-id="S6 elsewhere"]').count()) === 1, 5000);
      await sameDocument(page, origin, "a filter saved elsewhere");
    } finally {
      await context.close();
    }
  });

  await test("a_saved_filter_opens_the_board_on_it", async () => {
    const text = "kind build and status Ready";
    sy("tickets", "filters", "save", "--name", "S6 builds", "--filter", text);
    const { page, context } = await openPage("#/filters");
    try {
      const row = saved(page).locator('[data-id="S6 builds"]');
      await eventually("the saved filter is listed", async () => (await row.count()) === 1);
      await row.locator("[data-action=open]").click();
      await eventually('the route is #/f/saved "S6 builds"', async () => (await hash(page)) === '#/f/saved "S6 builds"', 3000);
      await eventually(`the board (${boardLayout}) is drawn on the saved filter`, async () =>
        (await page.locator(`[data-layout=${boardLayout}]`).count()) > 0 &&
        (await page.locator("[data-board-layout]").getAttribute("data-filter", { timeout: 500 })) === 'saved "S6 builds"', 5000);
      const count = one(got("S6 builds"), "filter_saved").filter.count;
      const expected = summaries("--filter", text, "--limit", "200").map((t) => t.id);
      await eventually(`the board counts ${count} and shows the filter's rows`, async () => {
        const n = Number(trimmed(await page.locator(".board-count").textContent({ timeout: 500 })));
        const shown = await page.locator(".board-row[data-id]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));
        return n === count && shown.length > 0 && JSON.stringify(shown) === JSON.stringify(expected.slice(0, shown.length));
      }, 5000);
    } finally {
      await context.close();
    }
  });

  await test("rename_and_delete", async () => {
    sy("tickets", "filters", "save", "--name", "S6 old", "--filter", "kind gap");
    sy("tickets", "filters", "save", "--name", "S6 other", "--filter", "kind scope");
    const { page, context } = await openPage("#/filters");
    try {
      const fs = saved(page);
      const old = fs.locator('[data-id="S6 old"]');
      await eventually("S6 old is listed", async () => (await old.count()) === 1);
      await old.locator("[data-pick]").click();
      await eventually("picked: its detail from filters.get", async () => (await old.getAttribute("data-selected")) !== null && trimmed(await old.locator("[data-field=detail] [data-field=filter]").textContent({ timeout: 500 })) === "kind gap", 5000);
      // A name another filter holds: refused beside the action, nothing written.
      await old.locator("input[data-field=to]").fill("S6 other");
      await old.locator('[data-action="tickets.filters.rename"]').click();
      await eventually("the clash is said beside the rename", async () => /another saved filter is named S6 other/.test(trimmed(await old.locator("[data-refused]").textContent({ timeout: 500 }))), 3000);
      assert(one(got("S6 old"), "filter_saved")?.filter?.filter === "kind gap", "nothing renamed");
      // Renamed.
      await old.locator("input[data-field=to]").fill("S6 new");
      await old.locator('[data-action="tickets.filters.rename"]').click();
      const renamed = fs.locator('[data-id="S6 new"]');
      await eventually("S6 new is listed and S6 old is gone", async () => (await renamed.count()) === 1 && (await fs.locator('[data-id="S6 old"]').count()) === 0, 5000);
      assert(one(got("S6 new"), "filter_saved")?.filter?.filter === "kind gap", "filters.get finds it under the new name");
      assert(one(got("S6 old"), "error"), "and not under the old");
      // Renamed elsewhere: the row moves without a reload.
      sy("tickets", "filters", "rename", "--name", "S6 other", "--to", "S6 other2");
      await eventually("a rename elsewhere moves the row", async () => (await fs.locator('[data-id="S6 other2"]').count()) === 1 && (await fs.locator('[data-id="S6 other"]').count()) === 0, 5000);
      // Renamed back (the service says the old name gone after each rename,
      // so a row keyed by name follows it both ways), and a save under a
      // name renamed away is listed again.
      sy("tickets", "filters", "rename", "--name", "S6 new", "--to", "S6 old");
      await eventually("renamed back: S6 old again, S6 new gone", async () => (await fs.locator('[data-id="S6 old"]').count()) === 1 && (await fs.locator('[data-id="S6 new"]').count()) === 0, 5000);
      sy("tickets", "filters", "rename", "--name", "S6 old", "--to", "S6 new");
      sy("tickets", "filters", "save", "--name", "S6 old", "--filter", "kind spike");
      await eventually("a new filter under the name renamed away is listed", async () => (await fs.locator('[data-id="S6 old"]').count()) === 1 && (await fs.locator('[data-id="S6 new"]').count()) === 1, 5000);
      sy("tickets", "filters", "delete", "--name", "S6 old");
      // A case-only rename is a rename, not a clash with itself.
      if ((await renamed.getAttribute("data-selected")) === null) await renamed.locator("[data-pick]").click();
      await eventually("S6 new picked", async () => (await renamed.getAttribute("data-selected")) !== null, 3000);
      await renamed.locator("input[data-field=to]").fill("S6 NEW");
      await renamed.locator('[data-action="tickets.filters.rename"]').click();
      const upper = fs.locator('[data-id="S6 NEW"]');
      await eventually("S6 NEW is listed, S6 new gone", async () => (await upper.count()) === 1 && (await fs.locator('[data-id="S6 new"]').count()) === 0, 5000);
      await upper.locator("input[data-field=to]").fill("S6 new");
      await upper.locator('[data-action="tickets.filters.rename"]').click();
      await eventually("and back to S6 new", async () => (await renamed.count()) === 1 && (await upper.count()) === 0, 5000);
      // Deleted.
      await eventually("S6 new stays picked", async () => (await renamed.getAttribute("data-selected")) !== null, 3000);
      await renamed.locator('[data-action="tickets.filters.delete"]').click();
      await eventually("S6 new is gone", async () => (await fs.locator('[data-id="S6 new"]').count()) === 0, 5000);
      assert(one(got("S6 new"), "error"), "filters.get finds it no more");
      // Deleted elsewhere.
      sy("tickets", "filters", "delete", "--name", "S6 other2");
      await eventually("a delete elsewhere takes the row", async () => (await fs.locator('[data-id="S6 other2"]').count()) === 0, 5000);
    } finally {
      await context.close();
    }
  });

  await test("check_says_the_refusal", async () => {
    const bad = "kind nonsense";
    const said = one(sy("tickets", "filters", "check", "--filter", bad), "error")?.message;
    assert(said && /unknown kind/.test(said), `filters.check refuses ${bad}: ${said}`);
    const good = one(sy("tickets", "filters", "check", "--filter", "kind spike"), "filter_checked");
    // The saved filters' form.
    {
      const { page, context } = await openPage("#/filters");
      try {
        const form = saved(page).locator('[data-form="tickets.filters.save"]');
        await form.locator("input[data-field=name]").fill("S6 never");
        await form.locator("input[data-field=filter]").fill(bad);
        await eventually("the refusal's words under the form", async () => trimmed(await form.locator("[data-check=refused][data-refused]").textContent({ timeout: 500 })) === said, 3000);
        assert((await form.locator('[data-action="tickets.filters.save"][disabled]').count()) === 1, "a refused filter is not saved");
        await form.locator("input[data-field=filter]").fill("kind spike");
        await eventually("ok and the count", async () => trimmed(await form.locator("[data-check=ok] [data-field=count]").textContent({ timeout: 500 })) === String(good.count), 3000);
      } finally {
        await context.close();
      }
    }
    // The filter line: checked on each pause, saved names completed, Escape clears.
    sy("tickets", "filters", "save", "--name", "S6 spikes", "--filter", "kind spike");
    // On both boards: the rhx line (filter-line.rhx) and the Svelte one (filter-line-sv, board-layout-sv's).
    for (const L of [
      // the rhx line's Escape goes to `#/` (it cannot know its board's route), the Svelte line's to its board's root
      svelteFirst
        ? { name: "rhx", route: "#/rhx/", f: "#/rhx/f/", home: ["#/rhx/", "#/"], focuses: false }
        : { name: "rhx", route: "", f: "#/f/", home: ["", "#/"], focuses: false },
      svelteFirst
        ? { name: "sv", route: "", f: "#/f/", home: ["", "#/"], focuses: true, slash: true }
        : { name: "sv", route: "#/sv/", f: "#/sv/f/", home: ["#/sv/", "#/sv/"], focuses: true },
    ]) {
      {
        const { page, context } = await openPage(L.route);
        try {
          const input = () => page.locator("input[data-filter-input]");
          if (L.slash) {
            // SURFACE-2's board-rows-sv says `filter.focus` on `/`; the Svelte line takes the focus.
            await eventually("the board is up", async () => (await page.locator(".board-row[data-id]").count()) > 0, 10000);
            await page.evaluate(() => document.activeElement?.blur?.());
            await page.keyboard.press("/");
            await eventually("`/` focuses the filter line (filter.focus)", async () => page.evaluate(() => document.activeElement?.matches("input[data-filter-input]") ?? false), 3000);
          }
          const check = page.locator("[data-surface=filter-line] [data-filter-check]");
          await input().fill("k");
          await page.waitForTimeout(120);
          assert((await page.locator('[data-surface=filter-line] [data-filter-check=refused]').count()) === 0, "the first key waits its pause too: no one-letter refusal flashes");
          await input().fill(bad);
          await eventually(`${L.name}: the filter line says the refusal's words`, async () => (await check.getAttribute("data-filter-check", { timeout: 500 })) === "refused" && trimmed(await check.textContent()) === said, 3000);
          assert((await check.getAttribute("data-refused")) !== null, "marked [data-refused]");
          await input().fill("kind spike");
          await eventually("ok, and how many", async () => (await check.getAttribute("data-filter-check", { timeout: 500 })) === "ok" && trimmed(await check.locator("[data-field=count]").textContent()) === String(good.count), 3000);
          await input().fill('kind build or saved "S6 sp');
          const offer = page.locator('[data-surface=filter-line] [data-complete="S6 spikes"]');
          await eventually("an open saved \"… offers the saved names", async () => (await offer.count()) === 1, 3000);
          await offer.click();
          assert(L.home.includes(await hash(page)), `${L.name}: a click on a completion applies nothing (Enter applies, a blur does not)`);
          await eventually("the name completes the line", async () => (await input().inputValue()) === 'kind build or saved "S6 spikes"', 3000);
          // The rhx line draws the field again to set its text (the engine sets `value` as an
          // attribute) and cannot focus it: `autofocus` on a later-inserted field is the
          // browser's to ignore, and it does, now and then — FRAME-39's gap. The Svelte line
          // binds the value and keeps the focus.
          const focused = await page.evaluate(() => document.activeElement?.matches("input[data-filter-input]") ?? false);
          if (L.focuses) assert(focused, `${L.name}: the field keeps the focus after a completion`);
          else console.log(`     the ${L.name} field after a completion: ${focused ? "focused" : "not focused (FRAME-39: the engine cannot focus)"}`);
          await eventually("and the completed line checks ok", async () => (await check.getAttribute("data-filter-check", { timeout: 500 })) === "ok", 3000);
          await input().press("Escape");
          await eventually("Escape clears the line", async () => (await input().inputValue()) === "" && (await check.count()) === 0, 3000);
          assert(L.home.includes(await hash(page)), `${L.name}: nothing applied, no route moved`);
        } finally {
          await context.close();
        }
      }
      {
        const { page, context } = await openPage(`${L.f}kind%20spike`);
        try {
          const input = page.locator("input[data-filter-input]");
          await eventually("the applied filter is on the line", async () => (await input.inputValue({ timeout: 500 })) === "kind spike", 5000);
          await input.press("Escape");
          await eventually("Escape on an applied filter clears the route too", async () => (await hash(page)) === L.home[1] && (await page.locator("input[data-filter-input]").inputValue()) === "", 3000);
        } finally {
          await context.close();
        }
      }
    }
  });

  // A write's own in-band refusal, beside it: filters-saved awaits its
  // `bus.call` (FRAME-37's `{ ok, events, message }`, or today's events with
  // their `error`). The cycle is one filters.check cannot see — `saved "S6
  // cyc-b"` reads fine alone — and filters.save refuses.
  await test("a_refused_write_is_said_beside_it", async () => {
    sy("tickets", "filters", "save", "--name", "S6 cyc-a", "--filter", "kind build");
    sy("tickets", "filters", "save", "--name", "S6 cyc-b", "--filter", 'saved "S6 cyc-a"');
    const refusal = one(sy("tickets", "filters", "save", "--name", "S6 cyc-a", "--filter", 'saved "S6 cyc-b"'), "error")?.message;
    assert(refusal, "filters.save refuses the cycle");
    const { page, context } = await openPage("#/filters");
    try {
      const form = saved(page).locator('[data-form="tickets.filters.save"]');
      await form.locator("input[data-field=name]").fill("S6 cyc-a");
      await form.locator("input[data-field=filter]").fill('saved "S6 cyc-b"');
      await eventually("the check says ok (it cannot see the cycle)", async () => (await form.locator("[data-check=ok]").count()) === 1, 3000);
      await form.locator('[data-action="tickets.filters.save"]:not([disabled])').click();
      await eventually("the save's refusal, in its words, beside it", async () => trimmed(await form.locator("[data-refused]").last().textContent({ timeout: 500 })) === refusal, 3000);
      assert(one(got("S6 cyc-a"), "filter_saved")?.filter?.filter === "kind build", "nothing written");
    } finally {
      await context.close();
    }
  });

  await test("facets_narrow_the_board", async () => {
    const base = "kind build";
    const expected = sy("tickets", "nav", "facets", "--filter", base, "--base", "not category done").filter((e) => e.type === "facet");
    assert(expected.some((f) => f.field === "status" && f.value === "Ready"), `facets for ${base} hold status Ready: ${JSON.stringify(expected.slice(0, 5))}`);
    {
      const { page, context } = await openPage("#/facets/kind%20build");
      try {
        const nf = facets(page);
        await eventually("every facet drawn", async () => (await nf.locator("[data-facet]").count()) === expected.length, 5000);
        const drawn = await nf.locator("[data-facet]").evaluateAll((els) => els.map((e) => [e.getAttribute("data-facet"), e.getAttribute("data-value"), e.querySelector("[data-held]").textContent.trim()].join("=")));
        const want = expected.map((f) => `${f.field}=${f.value}=${f.held}`);
        assert(JSON.stringify([...drawn].sort()) === JSON.stringify([...want].sort()), `the chips are nav.facets' values and counts:\n${drawn.join(" ")}\n${want.join(" ")}`);
        assert((await nf.locator("[data-facet][data-selected]").count()) === 1 && (await nf.locator("[data-facet=kind][data-value=build][data-selected]").count()) === 1, "the filter's own rule, kind build, is the one chip selected");
        // The look on the tokens, both schemes.
        const chip = (theme) => page.evaluate((theme) => new Promise((done) => {
          document.documentElement.dataset.theme = theme;
          // past the chip's transition (--motion-fast)
          setTimeout(() => done(getComputedStyle(document.querySelector("[data-surface=nav-facets] [data-facet]")).borderTopColor), 400);
        }), theme);
        const light = await chip("light");
        const dark = await chip("dark");
        assert(light !== dark, `a chip's edge moves with the scheme: ${light} → ${dark}`);
        await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
        await nf.locator('[data-facet=status][data-value=Ready]').click();
        const narrowed = "kind build and status Ready";
        await eventually("the chip narrows the route's filter", async () => (await hash(page)) === `#/f/${narrowed}`, 3000);
        const n = held(narrowed);
        await eventually(`the board counts ${n}`, async () => Number(trimmed(await page.locator(".board-count").textContent({ timeout: 500 }))) === n, 5000);
      } finally {
        await context.close();
      }
    }
    {
      const { page, context } = await openPage("#/facets/kind%20build%20and%20status%20Ready");
      try {
        const nf = facets(page);
        const ready = nf.locator("[data-facet=status][data-value=Ready]");
        await eventually("status Ready is selected", async () => (await ready.getAttribute("data-selected", { timeout: 500 })) !== null, 5000);
        assert((await nf.locator("[data-facet=kind][data-value=build][data-selected]").count()) === 1, "kind build is selected too");
        assert((await nf.locator("[data-facet=status][data-value=Pending][data-selected]").count()) === 0, "Pending is not");
        await nf.locator("[data-facet=status][data-value=Pending]").click();
        await eventually("another status replaces the field's rule", async () => (await hash(page)) === "#/f/kind build and status Pending", 3000);
      } finally {
        await context.close();
      }
    }
    {
      // A filter in parentheses is never cut: the chip puts it whole beside its rule.
      const { page, context } = await openPage(`#/facets/${encodeURIComponent("(kind build and status Ready)")}`);
      try {
        const nf = facets(page);
        await eventually("drawn", async () => (await nf.locator("[data-facet]").count()) > 0, 5000);
        assert((await nf.locator("[data-facet][data-selected]").count()) === 0, "no chip is drawn selected on a filter that is not cut");
        // its status and kind are the group's own (only Ready, only build): narrow by a severity
        const chip = nf.locator("[data-facet=severity]").first();
        const rule = await chip.getAttribute("data-rule");
        await chip.click();
        const whole = `((kind build and status Ready)) and ${rule}`;
        await eventually("the filter whole beside the rule", async () => (await hash(page)) === `#/f/${whole}`, 3000);
        const n = held(whole);
        await eventually(`the board takes it (no refusal) and counts ${n}`, async () => (await page.locator("[data-filter-refused]").count()) === 0 && Number(trimmed(await page.locator(".board-count").textContent({ timeout: 500 }))) === n, 5000);
      } finally {
        await context.close();
      }
    }
    {
      const { page, context } = await openPage("#/facets/kind%20build%20and%20status%20Ready");
      try {
        const ready = facets(page).locator("[data-facet=status][data-value=Ready][data-selected]");
        await eventually("selected", async () => (await ready.count()) === 1, 5000);
        await ready.click();
        await eventually("a selected chip takes its rule out", async () => (await hash(page)) === "#/f/kind build", 3000);
      } finally {
        await context.close();
      }
    }
  });

  await test("counts_are_live", async () => {
    const epic = "S6CN";
    const root = one(sy("tickets", "create", "--epic", epic, "--kind", "scope", "--title", "the counts' shortcode"), "created")?.ticket;
    assert(root?.id, "a shortcode for the counts");
    const filter = `in ${epic} and status Draft`;
    const { page, context } = await openPage(`#/counts/${encodeURIComponent(filter)}`);
    try {
      const count = page.locator("[data-surface=nav-counts] [data-field=count]");
      await eventually("the count is nav.counts'", async () => trimmed(await count.textContent({ timeout: 500 })) === String(held(filter)) && held(filter) === 1, 5000);
      const origin = await marked(page);
      const child = one(sy("tickets", "create", "--epic", epic, "--parent", root.id, "--title", "a child the count takes"), "created")?.ticket;
      await eventually("a created ticket moves the count", async () => trimmed(await count.textContent()) === "2", 5000);
      sy("tickets", "set_status", "--id", child.id, "--status", "Ready");
      await eventually("a ticket leaving the filter moves it back", async () => trimmed(await count.textContent()) === "1", 5000);
      await sameDocument(page, origin, "the count moved");
      assert(held(filter) === 1, "nav.counts agrees");
    } finally {
      await context.close();
    }
    {
      const { page, context } = await openPage("#/counts/kind%20nonsense");
      try {
        await eventually("a filter not taken is said, refused", async () => /unknown kind/.test(trimmed(await page.locator("[data-surface=nav-counts] [data-refused]").textContent({ timeout: 500 }))), 5000);
      } finally {
        await context.close();
      }
    }
  });
};
