// SHELL.176-5 (the owner, 2026-11-14: "the quick search should show the
// siblings and recents before any typing"): the quick search (jump-palette-sv,
// the page's `palette` section) opened with nothing typed lists the open
// ticket's siblings in their order, the open one marked, then the tickets
// opened in this browser most recently, newest first; typing replaces them
// with the hits and clearing brings them back; with no ticket open, recent
// alone.
//
// Marks: `[data-surface=jump-palette-sv]`, `input[data-palette-input]`,
// `[data-palette-siblings] [data-palette-sibling=<id>]` (the open one
// `[data-current]`), `[data-palette-recent] [data-palette-recent-hit=<id>]`,
// `[data-selected]`, `[data-palette-hit]`. NAV_SHOTS=DIR keeps a screenshot of
// each case there.
export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, groupsNow, attrs, ticketAt } = ctx;
  const shots = process.env.NAV_SHOTS || null;
  const shoot = async (page, name) => {
    if (!shots) return;
    await page.screenshot({ path: `${shots}/${name}.png` });
    await page.emulateMedia({ colorScheme: "dark" });
    await sleep(200);
    await page.screenshot({ path: `${shots}/${name}.dark.png` });
    await page.emulateMedia({ colorScheme: "light" });
  };
  const g = groupsNow().filter((x) => x.root && !x.shortcode.startsWith("AAA")).at(-1);
  let n = 0;
  const make = (title, parent) => {
    const [m] = sy("tickets", "create", "--epic", g.epic, "--parent", parent, "--title", `${title} ${++n}`);
    assert(m?.ticket?.id, `made "${title}": ${JSON.stringify(m)}`);
    return m.ticket.id;
  };
  const pal = (page) => page.locator("[data-surface=jump-palette-sv]").first();
  const input = (page) => pal(page).locator("input[data-palette-input]");
  const sibIds = (page) => attrs(pal(page).locator("[data-palette-siblings] [data-palette-sibling]"), "data-palette-sibling");
  const recentIds = (page) => attrs(pal(page).locator("[data-palette-recent] [data-palette-recent-hit]"), "data-palette-recent-hit");
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  async function visit(page, id) {
    await page.evaluate((x) => { location.hash = `#/ticket/${x}`; }, id);
    await eventually(`${id}'s page draws its head`, async () => (await page.locator(`[data-surface=ticket-page][data-ticket="${id}"] [data-surface=ticket-head] [data-field=title]`).count()) === 1, 20_000);
    // The palette hears the ticket once the page has it.
    await eventually(`${id} is first in this browser's record`, async () => (await page.evaluate(() => JSON.parse(localStorage.getItem("menger.recent") || "[]")[0]?.id)) === id, 10_000);
  }
  async function openPalette(page) {
    await page.locator("body").click({ position: { x: 5, y: 5 } }).catch(() => {});
    await page.keyboard.press("Control+k");
    await eventually("the palette's field is focused", async () => input(page).evaluate((e) => e === document.activeElement), 5_000);
  }

  // A parent with four children; then three of them opened in turn and the third child last.
  const p = make("the kiln yard", g.root);
  const kids = [make("the first shelf", p), make("the second shelf", p), make("the third shelf", p), make("the fourth shelf", p)];
  const [s1, s2, s3, s4] = kids;
  const opened = await openPage(`#/ticket/${s1}`);
  const { page, context } = opened;
  try {
    await test("the_empty_quick_search_lists_the_siblings_then_the_recent", async () => {
      await visit(page, s1);
      await visit(page, s2);
      await visit(page, s4);
      await visit(page, s3);
      await openPalette(page);
      await eventually("the siblings are listed in their order", async () => same(await sibIds(page), kids), 10_000);
      const cur = await attrs(pal(page).locator("[data-palette-sibling][data-current]"), "data-palette-sibling");
      assert(same(cur, [s3]), `the open one is marked: ${cur}`);
      await eventually("the recent follow, newest first, the open one left out", async () => same((await recentIds(page)).slice(0, 3), [s4, s2, s1]), 10_000);
      assert(!(await recentIds(page)).includes(s3), "the open ticket is not among the recent");
      const sel = await attrs(pal(page).locator("[data-selected]"), "data-palette-sibling");
      assert(same(sel, [s1]), `the first row that is not the open one is selected: ${sel}`);
      await shoot(page, "quick-on-a-ticket");
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      const moved = await attrs(pal(page).locator("[data-selected]"), "data-palette-sibling");
      assert(same(moved, [s3]), `the arrows move over the siblings: ${moved}`);
      await page.keyboard.press("ArrowDown");
      await page.keyboard.press("ArrowDown");
      const into = await attrs(pal(page).locator("[data-selected]"), "data-palette-recent-hit");
      assert(same(into, [s4]), `and on into the recent: ${into}`);
      await page.keyboard.press("Enter");
      await eventually(`enter opens ${s4}`, async () => ticketAt(await page.evaluate(() => location.hash), s4), 10_000);
    });

    await test("typing_replaces_them_and_clearing_brings_them_back", async () => {
      await visit(page, s3);
      await openPalette(page);
      await eventually("the siblings are listed", async () => (await sibIds(page)).length === 4, 10_000);
      await input(page).fill("kiln yard");
      await eventually("the hits replace both groups", async () => (await pal(page).locator("[data-palette-hit]").count()) > 0
        && (await pal(page).locator("[data-palette-siblings], [data-palette-recent]").count()) === 0, 10_000);
      await shoot(page, "quick-typed");
      await input(page).fill("");
      await eventually("cleared, both groups come back", async () => same(await sibIds(page), kids) && (await recentIds(page)).length > 0, 10_000);
      await page.keyboard.press("Escape");
    });

    await test("with_no_ticket_open_the_recent_alone", async () => {
      await page.evaluate(() => { location.hash = "#/board"; });
      await eventually("the board draws", async () => (await page.locator("[data-board-layout]").count()) === 1, 20_000);
      await sleep(300);
      await openPalette(page);
      await eventually("the recent are listed, newest first", async () => same((await recentIds(page)).slice(0, 2), [s3, s4]), 10_000);
      assert((await pal(page).locator("[data-palette-siblings]").count()) === 0, "no siblings with no ticket open");
      await shoot(page, "quick-on-the-board");
    });
  } finally { await context.close(); }
};
