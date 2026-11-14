// SHELL.176-3: the board opens on Recent, and is filtered by chips —
// `node tests/pages_browser.mjs --group board-filter`.
//
// Marks:
//   board-rows-sv  `[data-board][data-order=changed]` under `· order changed`; each row's
//       `[data-field=changed]` (how long ago)
//   filter-chips (board-layout-sv's top bar)  `[data-surface=filter-chips]`; the views
//       `[data-view=recent|all]` (`aria-pressed`); a chip `[data-chip=<prop>][data-rule=<rule>]`
//       (`[data-chip=raw]` a rule the chips cannot say) with `[data-op]`, `[data-values]`,
//       `[data-remove]`; `[data-add-filter]` (the key `f`), the menu `[data-filter-menu=props|<prop>]`
//       with `input[data-menu-input]` and `[data-option=<id>]` (`[data-count]` its count,
//       `[data-on]` ticked); `[data-clear-all]`
export default async (ctx) => {
  const { test, assert, eventually, openPage, sy, summaries } = ctx;
  const RECENT = "changed within 7d · order changed";
  const trimmed = (t) => String(t ?? "").trim();
  const hash = (page) => page.evaluate(() => decodeURIComponent(location.hash));
  const chips = (page) => page.locator("[data-surface=filter-chips]");
  const held = (filter) => sy("tickets", "nav", "counts", "--filter", filter).filter((e) => e.type === "count").reduce((a, c) => a + c.held, 0);
  const count = async (page) => Number(trimmed(await page.locator(".board-count").textContent({ timeout: 500 })));
  const rowIds = (page) => page.locator(".board-row[data-id]").evaluateAll((els) => els.map((e) => e.getAttribute("data-id")));

  await test("the_board_opens_on_recent", async () => {
    // A seed's tickets are months old: touch three, so the last seven days hold some.
    for (const t of summaries("--limit", "3")) sy("tickets", "update", "--id", t.id, "--title", `${t.title} (touched)`);
    const { page, context } = await openPage("#/board");
    try {
      await eventually("the board is in the Recent order", async () => (await page.locator("[data-board][data-order=changed][data-drawn]").count()) === 1, 10_000);
      assert((await page.locator("input[data-filter-input]").inputValue()) === RECENT, `the line shows the default: ${await page.locator("input[data-filter-input]").inputValue()}`);
      assert((await chips(page).locator("[data-view=recent]").getAttribute("aria-pressed")) === "true", "Recent is pressed");
      assert((await chips(page).locator('[data-chip=changed][data-rule="changed within 7d"]').count()) === 1, "its one chip: changed within 7d");
      const want = summaries("--filter", RECENT, "--limit", "5").map((s) => s.id);
      await eventually("the rows are the service's newest first", async () => JSON.stringify((await rowIds(page)).slice(0, 5)) === JSON.stringify(want), 5000);
      assert((await count(page)) === held(RECENT), "the count is the filter's");
      assert((await page.locator(".board-row[data-id] [data-field=changed]").count()) > 0, "each row says how long ago it changed");

      // A ticket written moves to the top: live, then after a reload.
      const last = want.length ? (await rowIds(page)).at(-1) : null;
      assert(last, "a row to touch");
      sy("tickets", "update", "--id", last, "--title", "touched by SHELL.176-3's test");
      await eventually(`${last}, written, is the first row`, async () => (await rowIds(page))[0] === last, 5000);
      await page.reload();
      await eventually(`${last} is first after a reload`, async () => (await rowIds(page))[0] === last, 10_000);
      assert((await hash(page)) === "#/board", "the route stays #/board");
    } finally {
      await context.close();
    }
  });

  await test("a_chip_is_added_and_removed", async () => {
    const { page, context } = await openPage("#/board");
    try {
      await eventually("drawn", async () => (await page.locator("[data-board][data-drawn]").count()) === 1, 10_000);
      await page.locator(".board-count").click();
      await page.keyboard.press("f");
      const menu = chips(page).locator("[data-filter-menu]");
      await eventually("f opens the properties", async () => (await menu.getAttribute("data-filter-menu", { timeout: 500 })) === "props", 3000);
      await menu.locator("[data-option=status]").click();
      await eventually("the status values, counted", async () => (await menu.locator("[data-option] [data-count]").count()) > 1, 5000);
      const facets = sy("tickets", "nav", "facets").filter((e) => e.type === "facet" && e.field === "status");
      const shown = await menu.locator("[data-option]").evaluateAll((els) => els.map((e) => `${e.getAttribute("data-option")}=${e.querySelector("[data-count]")?.textContent.trim()}`));
      assert(JSON.stringify(shown) === JSON.stringify(facets.map((f) => `${f.value}=${f.held}`)), `the counts are nav.facets': ${shown} vs ${facets.map((f) => `${f.value}=${f.held}`)}`);
      const [a, b] = facets.map((f) => f.value);
      const q = (v) => (v.includes(" ") ? `"${v}"` : v);

      await menu.locator(`[data-option="${a}"]`).click();
      const one = `changed within 7d and status ${q(a)} · order changed`;
      await eventually("the chip's rule is in the route", async () => (await hash(page)) === `#/f/${one}`, 3000);
      const chip = chips(page).locator("[data-chip=status]");
      assert((await chip.getAttribute("data-rule")) === `status ${q(a)}`, "a chip status · is · A");
      assert(trimmed(await chip.locator("[data-op]").textContent()) === "is", "its operator is `is`");
      await eventually(`the board counts ${held(one)}`, async () => (await count(page)) === held(one), 5000);

      // A second value on the same chip: `in (A, B)`.
      if (b) {
        await eventually("the menu stays on the chip", async () => (await menu.count()) === 1 && (await menu.locator(`[data-option="${b}"]`).count()) === 1, 3000);
        await menu.locator(`[data-option="${b}"]`).click();
        const two = `changed within 7d and status in (${q(a)}, ${q(b)}) · order changed`;
        await eventually("two values are `in (…)`", async () => (await hash(page)) === `#/f/${two}`, 3000);
        await eventually("and the chip says is any of", async () => trimmed(await chip.locator("[data-op]").textContent({ timeout: 500 })) === "is any of", 3000);
        await page.keyboard.press("Escape");
        await eventually("escape closes the menu", async () => (await menu.count()) === 0, 2000);
        await chip.locator("[data-op]").click();
        await eventually("the operator negates the rule", async () => (await hash(page)) === `#/f/changed within 7d and not status in (${q(a)}, ${q(b)}) · order changed`, 3000);
      }

      await chips(page).locator("[data-chip=status] [data-remove]").click();
      await eventually("× takes the chip's rule out", async () => (await hash(page)) === `#/f/${RECENT}`, 3000);
      await eventually("the chip is gone", async () => (await chips(page).locator("[data-chip=status]").count()) === 0, 3000);
      await chips(page).locator("[data-clear-all]").click();
      await eventually("Clear all keeps the order alone", async () => (await hash(page)) === "#/f/· order changed", 3000);
      await eventually(`every ticket counted (${held("· order changed")})`, async () => (await count(page)) === held("· order changed"), 5000);
      await chips(page).locator("[data-view=all]").click();
      await eventually("All is the store's order", async () => (await hash(page)) === "#/f/· order code", 3000);
      await eventually("the board has no order of its own", async () => (await page.locator("[data-board][data-order]").count()) === 0 && (await chips(page).locator("[data-view=all]").getAttribute("aria-pressed")) === "true", 5000);
      await chips(page).locator("[data-view=recent]").click();
      await eventually("Recent is #/board again", async () => (await hash(page)) === "#/board", 3000);
    } finally {
      await context.close();
    }
  });

  await test("a_rule_the_chips_cannot_say_stays_in_the_line", async () => {
    const f = "kind build or kind gap";
    const { page, context } = await openPage(`#/f/${encodeURIComponent(f)}`);
    try {
      const raw = chips(page).locator("[data-chip=raw]");
      await eventually("one plain chip of its text", async () => (await raw.count()) === 1 && (await raw.getAttribute("data-rule")) === f, 5000);
      await eventually(`the board takes it: ${held(f)}`, async () => (await count(page)) === held(f), 5000);
      // A chip added beside it puts it in parentheses.
      await page.locator(".board-count").click();
      await page.keyboard.press("f");
      await chips(page).locator("[data-filter-menu] [data-option=ready]").click();
      await eventually("the flag is a chip beside it", async () => (await hash(page)) === `#/f/(${f}) and ready`, 3000);
      await eventually("and the board takes it", async () => (await page.locator("[data-filter-refused]").count()) === 0 && (await count(page)) === held(`(${f}) and ready`), 5000);
    } finally {
      await context.close();
    }
  });
};
