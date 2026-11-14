// SHELL.176-4 (the owner, 2026-11-14: "I want the nav to always show the
// ticket i'm open on"): the nav beside a ticket (page-ticket's `nav` section,
// nav-tree-sv, handed the open ticket as `data-current`) opens the way to the
// ticket, marks its row and scrolls it into the nav's column — by a link, by
// back and forward, past a node's first page — and leaves the scroll alone once
// the person has moved the nav; a ticket the tree does not hold is pinned
// above the rows, saying why — at once, from what was read (FRAME-67), never while a page on the way is out.
//
// Marks: `[data-surface=ticket-nav]` (the column, the scroller), in it the
// nav's row `[data-nav-current=<id>]` (its link `[aria-current=page]`), the
// pin `[data-nav-pinned=<id>]` with `[data-nav-pinned-why]`, `[data-nav-mode]`.
// NAV_SHOTS=DIR keeps a screenshot of each case there.
export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, groupsNow, summaries } = ctx;
  const shots = process.env.NAV_SHOTS || null;
  const shoot = async (page, name) => {
    if (!shots) return;
    await page.screenshot({ path: `${shots}/${name}.png` });
    await page.emulateMedia({ colorScheme: "dark" });
    await sleep(200);
    await page.screenshot({ path: `${shots}/${name}.dark.png` });
    await page.emulateMedia({ colorScheme: "light" });
  };
  const groups = groupsNow().filter((g) => g.root && !g.shortcode.startsWith("AAA"));
  const g = groups.at(-1);
  let n = 0;
  const make = (title, parent) => {
    const [m] = sy("tickets", "create", "--epic", g.epic, "--parent", parent, "--title", `${title} ${++n}`);
    assert(m?.ticket?.id, `made "${title}": ${JSON.stringify(m)}`);
    return m.ticket.id;
  };
  const navOf = (page) => page.locator("[data-surface=ticket-nav]");
  const marked = (page, id) => navOf(page).locator(`[data-nav] .nav-body [data-nav-current="${id}"]`);
  /** Whether the row is inside the nav column's visible box. */
  const inView = (page, id) => marked(page, id).first().evaluate((el) => {
    const sc = el.closest("[data-surface=ticket-nav]");
    const r = el.getBoundingClientRect(), b = sc.getBoundingClientRect();
    return r.top >= b.top - 1 && r.bottom <= b.bottom + 1;
  });
  async function openTicket(id) {
    const o = await openPage(`#/ticket/${id}`);
    await eventually("the nav is drawn beside the ticket", async () => (await navOf(o.page).locator("[data-nav-mode]").count()) > 0, 20_000);
    return o;
  }

  // A chain three deep under the last shortcode's root, which a fresh browser has closed.
  const a = make("the kiln shed", g.root);
  const bb = make("the kiln", a);
  const c = make("the third firing", bb);

  await test("the_nav_opens_the_way_to_the_open_ticket", async () => {
    const { page, context } = await openTicket(c);
    try {
      await eventually(`${c}'s row is drawn and marked`, async () => (await marked(page, c).count()) === 1, 20_000);
      assert((await marked(page, c).locator("[aria-current=page]").count()) === 1, "its link says aria-current=page");
      assert((await navOf(page).locator("[data-nav] [data-nav-current]").count()) === 1, "one row is marked");
      const opened = await page.evaluate(() => JSON.parse(localStorage.getItem("menger.nav.open") || "[]"));
      assert(opened.includes(g.shortcode) && opened.includes(a) && opened.includes(bb), `the shortcode and both ancestors are open: ${opened}`);
      assert(await inView(page, c), "the row is in the nav's view");
      await shoot(page, "deep");
    } finally { await context.close(); }
  });

  await test("back_and_forward_move_the_mark", async () => {
    const other = groups.at(-2);
    const d = make("the glaze shelf", g.root);
    const { page, context } = await openTicket(c);
    try {
      await eventually(`${c} marked`, async () => (await marked(page, c).count()) === 1, 20_000);
      // Another ticket in another shortcode, by a link from the page.
      await page.evaluate((id) => { location.hash = `#/ticket/${id}`; }, other.root);
      await eventually(`${other.root} marked, ${c} not`, async () => (await marked(page, other.root).count()) === 1 && (await marked(page, c).count()) === 0, 20_000);
      await page.goBack();
      await eventually(`back: ${c} marked again`, async () => (await marked(page, c).count()) === 1 && (await marked(page, other.root).count()) === 0, 20_000);
      await page.goForward();
      await eventually(`forward: ${other.root} marked again`, async () => (await marked(page, other.root).count()) === 1, 20_000);
      await page.evaluate((id) => { location.hash = `#/ticket/${id}`; }, d);
      await eventually(`${d} marked`, async () => (await marked(page, d).count()) === 1, 20_000);
      await page.reload();
      await eventually(`reloaded: ${d} marked`, async () => (await marked(page, d).count()) === 1, 20_000);
    } finally { await context.close(); }
  });

  // A child far past its parent's first page of 200: the node pages until it is drawn.
  const far = (() => {
    for (const x of [...groups].reverse()) {
      const kids = summaries("--parent", x.root, "--limit", "400", "--fields", "id").map((s) => s.id).filter((id) => !/\.\d+$/.test(id));
      if (kids.length >= 300) return kids[279];
    }
    return null;
  })();

  await test("past_the_first_page_the_row_is_drawn_and_in_view", async () => {
    assert(far, "a root with 300 children in the seed");
    const { page, context } = await openTicket(far);
    try {
      await eventually(`${far}, the 280th child, drawn and marked`, async () => (await marked(page, far).count()) === 1, 30_000);
      await eventually("and scrolled into the nav's view", () => inView(page, far), 5_000);
      assert((await navOf(page).evaluate((e) => e.scrollTop)) > 2000, "the nav scrolled down to it");
      await shoot(page, "far");
    } finally { await context.close(); }
  });

  await test("the_nav_leaves_the_scroll_to_the_person", async () => {
    assert(far, "a root with 300 children in the seed");
    // The node's second page (`limit: 400`, asked only on the way to the row) held back 1.5 s in the page, so
    // the person's wheel comes before the row, every run.
    const { page, context } = await openPage(`#/ticket/${far}`);
    try {
      await context.addInitScript(() => {
        const send = WebSocket.prototype.send;
        WebSocket.prototype.send = function (d) {
          if (typeof d === "string" && d.includes('"tickets.list"') && /"limit":\s*400\b/.test(d)) { setTimeout(() => send.call(this, d), 1500); return; }
          return send.call(this, d);
        };
      });
      await page.reload();
      const nav = navOf(page);
      await eventually("the shortcodes are drawn", async () => (await nav.locator("[data-nav-group]").count()) > 0, 20_000);
      const box = await nav.boundingBox();
      await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
      await page.mouse.wheel(0, 40);
      // FRAME-67: the pin is decided from what was read — while the page that holds the row is out, nothing is pinned.
      let pinned = 0;
      await eventually(`${far} drawn and marked`, async () => {
        pinned += await nav.locator("[data-nav-pinned]").count();
        return (await marked(page, far).count()) === 1;
      }, 30_000);
      assert(pinned === 0, "nothing pinned while the node's next page was out");
      await sleep(500);
      assert((await nav.locator("[data-nav-pinned]").count()) === 0, "nothing pinned once it is drawn");
      assert(!(await inView(page, far)), "the nav did not scroll to the row after the person wheeled it");
      assert((await nav.evaluate((e) => e.scrollTop)) < 1000, "the nav is where the person left it");
    } finally { await context.close(); }
  });

  await test("a_ticket_the_tree_does_not_hold_is_pinned", async () => {
    const e = make("the cracked saggar", bb);
    const out = sy("tickets", "archive", "--ids", e, "--message", "broken");
    assert(out.some((x) => x.type === "archived"), `archived: ${JSON.stringify(out)}`);
    const { page, context } = await openTicket(e);
    try {
      const pin = navOf(page).locator(`[data-nav-pinned="${e}"]`);
      await eventually(`${e} is pinned above the rows`, async () => (await pin.count()) === 1, 20_000);
      assert((await pin.locator("[data-nav-pinned-why]").textContent()).trim() === "archived", "it says why: archived");
      assert((await pin.locator("[aria-current=page]").count()) === 1, "the pinned row is marked");
      // Flat, one page of the store's order: the ticket made last is past it.
      await shoot(page, "pinned-archived");
    } finally { await context.close(); }
    const { page: p2, context: c2 } = await openTicket(c);
    try {
      await eventually(`${c} marked in Tree`, async () => (await marked(p2, c).count()) === 1, 20_000);
      // FRAME-67: a node on the way closed by the person — the row goes, and the ticket is pinned from that, at once.
      const toggle = navOf(p2).locator(`.nav-body [data-nav-toggle="${bb}"]`);
      await toggle.click();
      const closedPin = navOf(p2).locator(`[data-nav-pinned="${c}"]`);
      await eventually(`${bb} closed: ${c} is pinned`, async () => (await closedPin.count()) === 1, 2_000);
      assert((await closedPin.locator("[data-nav-pinned-why]").textContent()).trim() === "not in the tree", "it says why: not in the tree");
      await toggle.click();
      await eventually(`${bb} open again: the pin goes, the row is marked`, async () => (await closedPin.count()) === 0 && (await marked(p2, c).count()) === 1, 20_000);
      await navOf(p2).locator("[data-nav-mode=flat]").click();
      const pin = navOf(p2).locator(`[data-nav-pinned="${c}"]`);
      await eventually(`in Flat ${c} is pinned`, async () => (await pin.count()) === 1, 20_000);
      assert((await pin.locator("[data-nav-pinned-why]").textContent()).trim() === "past the first 200", "it says why: past the first 200");
      await navOf(p2).locator("[data-nav-mode=tree]").click();
      await eventually("back in Tree, the pin goes and the row is marked", async () => (await pin.count()) === 0 && (await marked(p2, c).count()) === 1, 20_000);
    } finally { await c2.close(); }
  });
};
