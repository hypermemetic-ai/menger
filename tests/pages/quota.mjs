// V2-31: the quota pages — a person's usage (#/usage: a bar per source, used
// against the cap) and the admin's table of everyone (#/quota: sortable, the
// cap edited in place through quota.set). Run alone by
// `node tests/pages_browser.mjs --group quota`.
//
// Marks: `[data-surface=quota-usage]` with a row per source `[data-id=<source>]`
// (`[data-field=used|cap|from]`, `[data-bar][data-fill][data-state]`),
// `[data-field=stored]`, `[data-exempt]`; `[data-surface=quota-table]` with
// `[data-box][data-state]`, `[data-sort=<key>][aria-sort]`, a row
// `[data-id=<principal>]` and its cells `[data-cell=<source>]`
// (`[data-field=used]`, `[data-action=quota.set]`, `input[data-cap-input]`),
// `[data-total]`, `[data-refused]`; V2-70's `[data-surface=quota-history]` (`data-whose`)
// with `[data-range]`, `[data-view]`, `[data-chart=stored|day] [data-series=<source>]`
// (`data-points`, `data-last`), `[data-legend]`, `[data-tip]` (`data-day`), the table
// `[data-history-table] tr[data-day] [data-col]`; V2-68's `[data-surface=quota-total]` (on
// #/usage, an admin of *'s) with `[data-box]`, `[data-total]`
// (`[data-field=bytes|principals]`), a row per source `[data-source]`, the
// five storing the most `[data-top=<principal>]`, the link `[data-everyone]`.
//
// The owner (an admin of *, exempt) and an editor of its own, made here;
// every case leaves the editor's allowance as the default.
import path from "node:path";
import { makeKey, sessionCookies, writeSession } from "../../scripts/throwaway-login.mjs";
import { sentCalls, synapse } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, openPage, sy, server, session, fake } = ctx;
  const work = path.dirname(session);
  const text = async (loc) => String(await loc.first().textContent({ timeout: 2000 })).trim();
  const shots = process.env.QUOTA_SHOTS || null;

  // An editor on *, its own session.
  const keyFile = path.join(work, "quota-editor.key");
  const editor = makeKey(keyFile);
  const g = sy("members", "grant", "--principal", editor, "--scope", "*", "--role", "editor");
  assert(g.find((e) => e.type === "grant"), `editor granted: ${JSON.stringify(g)}`);
  const file = path.join(work, "quota-editor.session");
  const s = await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, file, "quota-editor");
  const as = (...a) => synapse(server, file, ...a);
  assert(sy("tickets", "create", "--epic", "QUOTA", "--kind", "scope", "--title", "quota pages").find((e) => e.type === "created"), "the scope");
  const body = path.join(work, "quota-body.md");
  (await import("node:fs")).writeFileSync(body, "x".repeat(4321));
  assert(as("tickets", "create", "--epic", "QUOTA", "--title", "an editor's ticket", "--body", `@${body}`).find((e) => e.type === "created"), "the editor writes");

  async function openAs(token, route) {
    const { context, page, seen } = await openPage("", { signedIn: false });
    await context.clearCookies();
    await context.addCookies(sessionCookies(server.origin, token));
    await page.goto(`${server.origin}/app/${route}`);
    return { context, page, seen };
  }
  async function shoot(page, name) {
    if (!shots) return;
    for (const [w, h] of [[1440, 900], [390, 844]]) {
      for (const theme of ["light", "dark"]) {
        await page.setViewportSize({ width: w, height: h });
        await page.emulateMedia({ colorScheme: theme });
        await page.evaluate((t) => document.querySelectorAll("[data-plexus-theme], quota-usage, quota-table, quota-total, quota-history").forEach((e) => e.setAttribute("data-plexus-theme", t)), theme);
        await page.waitForTimeout(150);
        await page.screenshot({ path: path.join(shots, `${name}-${w}-${theme}.png`), fullPage: true });
      }
    }
    await page.setViewportSize({ width: 1440, height: 900 });
  }

  // V2-68: the amount as the pages say it (bytes in powers of 1000).
  const words = (n) => {
    const units = ["B", "KB", "MB", "GB", "TB"];
    let v = Number(n), u = 0;
    while (v >= 1000 && u < units.length - 1) { v /= 1000; u += 1; }
    return u === 0 ? `${n} B` : `${(Math.round(v * 10) / 10).toString()} ${units[u]}`;
  };

  await test("usage_page_a_bar_per_source", async () => {
    const { page, context } = await openAs(s.token, "#/usage");
    const root = page.locator("[data-surface=quota-usage]");
    try {
      await eventually("a row per source", async () => (await root.locator("[data-id]").count()) === 9);
      const tickets = root.locator('[data-id="tickets.bytes"]');
      // V2-40: one 4,321-byte ticket is kept twice — its current body (the
      // tickets row) and its first revision's copy (revision_bodies) — 8.6 KB.
      assert((await text(tickets.locator("[data-field=used]"))) === "8.6 KB", `tickets used: ${await text(tickets.locator("[data-field=used]"))}`);
      assert((await text(tickets.locator("[data-field=cap]"))) === "50 MB", "the free tier's 50 MB");
      assert((await text(tickets.locator("[data-field=from]"))) === "the default", "from the default");
      assert((await text(root.locator('[data-id="tickets.count"] [data-field=cap]'))) === "unlimited", "a count unlimited");
      assert((await root.locator('[data-id="media.fetch.calls"]').getAttribute("data-window")) === "day", "a day's counter");
      // Capped near what is used: the bar fills and says so, live after a reload.
      sy("quota", "set", "--principal", editor, "--source", "tickets.bytes", "--cap", "10000");
      await page.reload();
      await eventually("the bar near its cap", async () => (await tickets.locator("[data-bar]").getAttribute("data-state")) === "near");
      assert(Number(await tickets.locator("[data-bar]").getAttribute("data-fill")) === 86, "8642 of 10000: 86%");
      assert((await text(tickets.locator("[data-field=from]"))) === "yours", "their own now");
      await shoot(page, "usage");
    } finally {
      sy("quota", "set", "--principal", editor, "--source", "tickets.bytes", "--cap", "clear");
      await context.close();
    }
  });

  await test("usage_page_the_owner_is_exempt", async () => {
    const { page, context } = await openPage("#/usage");
    try {
      const root = page.locator("[data-surface=quota-usage]");
      await eventually("the exempt mark", async () => (await root.locator("[data-exempt]").count()) === 1);
      assert((await text(root.locator('[data-id="media.bytes"] [data-field=cap]'))) === "unlimited", "never capped");
    } finally {
      await context.close();
    }
  });

  // V2-69: the owner's paths in the editor — a file pasted (media.put) and a
  // page's image pasted (media.fetch, which kept a file and charged no one's
  // storage before) — each charged to the owner, an admin of * (exempt:
  // counted, never capped): quota.usage and media.usage agree, and #/usage says it.
  await test("the_owners_pasted_media_is_counted", async () => {
    const mediaUsed = () => sy("quota", "usage").find((e) => e.type === "usage").sources.find((x) => x.source === "media.bytes").used;
    const kept = () => sy("media", "usage").find((e) => e.type === "usage").bytes;
    const before = mediaUsed();
    const made = sy("tickets", "create", "--epic", "QUOTA", "--title", "the owner's media");
    const id = made.find((e) => e.type === "created").ticket.id;
    const { page, context } = await openPage(`#/edit/${id}`);
    try {
      const content = page.locator("[data-surface=editor-body] .cm-content");
      await eventually("the editor", async () => (await content.count()) === 1, 10_000);
      // A PNG of its own bytes (a random tail), so no earlier test kept it.
      const tail = Array.from({ length: 4000 }, () => Math.floor(Math.random() * 256));
      await page.evaluate(({ tail }) => {
        const head = Uint8Array.from(atob("iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII="), (c) => c.charCodeAt(0));
        const dt = new DataTransfer();
        dt.items.add(new File([head, new Uint8Array(tail)], "owner.png", { type: "image/png" }));
        document.querySelector("[data-surface=editor-body] .cm-content").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
      }, { tail });
      const body = () => sy("tickets", "get", "--id", id).find((e) => e.type === "ticket").ticket.body;
      await eventually("the pasted file in the body", async () => /media:[0-9a-f]{64}/.test(body()), 8_000);
      const afterPut = mediaUsed();
      assert(afterPut >= before + 4000, `a put: ${before} → ${afterPut}`);
      // A page's image, pasted as HTML: fetched and kept by the server.
      await page.evaluate((src) => {
        const dt = new DataTransfer();
        dt.setData("text/html", `<p>a <b>picture</b></p><p><img src="${src}" alt="the owner's picture"></p>`);
        document.querySelector("[data-surface=editor-body] .cm-content").dispatchEvent(new ClipboardEvent("paste", { clipboardData: dt, bubbles: true, cancelable: true }));
      }, `${fake.origin}/picture.png`);
      await eventually("the fetched picture in the body", async () => (body().match(/media:[0-9a-f]{64}/g) ?? []).length === 2, 8_000);
      const after = mediaUsed();
      assert(after === kept(), `quota.usage's media.bytes (${after}) is media.usage's (${kept()})`);
      await page.goto(`${server.origin}/app/#/usage`);
      const row = page.locator('[data-surface=quota-usage] [data-id="media.bytes"]');
      await eventually("#/usage says what the owner keeps", async () => (await row.count()) === 1 && (await text(row.locator("[data-field=used]"))) === words(after), 5_000);
    } finally {
      await context.close();
    }
  });

  await test("quota_table_sorts_and_edits_a_cap_in_place", async () => {
    const { page, context } = await openPage("#/quota");
    const root = page.locator("[data-surface=quota-table]");
    try {
      await eventually("the editor's row", async () => (await root.locator(`[data-id="${editor}"]`).count()) === 1);
      assert(["ok", "warn", "full", "unknown"].includes(await root.locator("[data-box]").getAttribute("data-state")), "the box");
      assert((await root.locator("[data-sort=bytes]").getAttribute("aria-sort")) === "descending", "stored, most first, by default");
      await root.locator("[data-sort=principal]").click();
      await eventually("sorted by principal", async () => (await root.locator("[data-sort=principal]").getAttribute("aria-sort")) === "ascending");
      const ids = await root.locator(".tr[data-id]").evaluateAll((rs) => rs.map((r) => r.getAttribute("data-id")));
      assert(JSON.stringify(ids) === JSON.stringify([...ids].sort()), `in order: ${ids}`);
      const cell = root.locator(`[data-id="${editor}"] [data-cell="tickets.bytes"]`);
      assert((await text(cell.locator("[data-field=used]"))) === "8.6 KB", "the editor's tickets: the body and its revision's copy");
      await cell.locator("[data-action='quota.set']").click();
      const input = cell.locator("input[data-cap-input]");
      await input.fill("1 MB");
      await input.press("Enter");
      await eventually("the cap as set", async () => (await cell.locator("[data-action='quota.set']").count()) === 1 && (await text(cell.locator("[data-action='quota.set']"))) === "1 MB");
      const u = sy("quota", "usage", "--principal", editor).find((e) => e.type === "usage");
      const t = u.sources.find((x) => x.source === "tickets.bytes");
      assert(t.cap === 1000000 && t.from === "own", `quota.set landed: ${JSON.stringify(t)}`);
      // A cap that is no cap is refused, said, nothing set.
      await cell.locator("[data-action='quota.set']").click();
      await cell.locator("input[data-cap-input]").fill("lots");
      await cell.locator("input[data-cap-input]").press("Enter");
      await eventually("the refusal said", async () => (await root.locator("[data-refused]").count()) === 1);
      assert((await root.locator("[data-total]").count()) === 1, "the total");
      await page.keyboard.press("Escape");
      await shoot(page, "quota");
    } finally {
      sy("quota", "set", "--principal", editor, "--source", "tickets.bytes", "--cap", "clear");
      await context.close();
    }
  });

  await test("usage_page_shows_an_admin_the_whole_system", async () => {
    const { page, context } = await openPage("#/usage");
    const root = page.locator("[data-surface=quota-total]");
    try {
      await eventually("the system's total", async () => (await root.locator("[data-total]").count()) === 1, 5_000);
      const ev = sy("quota", "table");
      const total = ev.find((e) => e.type === "total");
      const rows = ev.filter((e) => e.type === "usage");
      assert((await text(root.locator("[data-total] [data-field=bytes]"))) === words(total.bytes), `stored: ${await text(root.locator("[data-total] [data-field=bytes]"))} vs ${words(total.bytes)}`);
      assert(Number(await text(root.locator("[data-total] [data-field=principals]"))) === total.principals, `principals: ${total.principals}`);
      assert((await root.locator("[data-source]").count()) === total.sources.length, "a row per source");
      const tb = total.sources.find((x) => x.source === "tickets.bytes");
      assert((await text(root.locator('[data-source="tickets.bytes"] [data-field=used]'))) === words(tb.used), "tickets' total used");
      assert((await root.locator('[data-source="media.fetch.calls"]').getAttribute("data-window")) === "day", "a day's counter said");
      const top = await root.locator("[data-top]").evaluateAll((rs) => rs.map((r) => r.getAttribute("data-top")));
      assert(JSON.stringify(top) === JSON.stringify(rows.slice(0, 5).map((r) => r.principal)), `the five storing the most: ${top}`);
      assert((await root.locator("[data-everyone]").getAttribute("href")) === "#/quota", "everyone's on #/quota");
      assert(["ok", "warn", "full", "unknown"].includes(await root.locator("[data-box]").getAttribute("data-state")), "the box");
      // Above the person's own.
      const above = await page.evaluate(() => {
        const t = document.querySelector("[data-surface=quota-total]"), u = document.querySelector("[data-surface=quota-usage]");
        return !!(t && u && t.compareDocumentPosition(u) & Node.DOCUMENT_POSITION_FOLLOWING);
      });
      assert(above, "above the person's own");
      // An admin looking at another person: the section too.
      await page.goto(`${server.origin}/app/#/usage/${encodeURIComponent(editor)}`);
      await eventually("the editor's usage", async () => (await page.locator("[data-surface=quota-usage] [data-id]").count()) === 9, 5_000);
      await eventually("and the system's", async () => (await root.locator("[data-total]").count()) === 1, 5_000);
      await shoot(page, "usage-admin");
    } finally {
      await context.close();
    }
  });

  await test("usage_page_shows_no_one_else_the_whole_system", async () => {
    const { page, context } = await openAs(s.token, "#/usage");
    try {
      await eventually("their own usage", async () => (await page.locator("[data-surface=quota-usage] [data-id]").count()) === 9, 5_000);
      await eventually("the roles asked", async () => (await sentCalls(page)).some((c) => c.method === "auth.requires" && c.params.method === "quota.table"), 5_000);
      await eventually("who they are asked", async () => (await sentCalls(page)).some((c) => c.method === "auth.whoami"), 5_000);
      await page.waitForTimeout(500);
      assert((await page.locator("[data-surface=quota-total]").count()) === 0, "no whole-system section");
      assert(!(await sentCalls(page)).some((c) => c.method === "quota.table"), "quota.table never asked");
    } finally {
      await context.close();
    }
  });

  // V2-70: consumption over time — the charts draw the points quota.history
  // answers (a line per source, a point per day, the last its value now), the
  // range moves them, the crosshair's tooltip says the day and every value,
  // the table view the same points; the whole system's for an admin.
  await test("history_charts_draw_what_quota_history_answers", async () => {
    const { page, context } = await openPage("#/usage");
    const root = page.locator("[data-surface=quota-history][data-whose=mine]");
    const last = (pts, source) => pts.filter((p) => p.type === "point" && p.source === source).at(-1);
    try {
      const line = (chart, source) => root.locator(`[data-chart=${chart}] [data-series="${source}"]`);
      await eventually("the stored chart", async () => (await line("stored", "tickets.bytes").count()) === 1, 10_000);
      const h = sy("quota", "history", "--days", "90");
      for (const [chart, source] of [["stored", "media.bytes"], ["stored", "tickets.bytes"], ["stored", "docs.bytes"], ["day", "media.fetch.bytes"], ["day", "media.egress.bytes"]]) {
        const l = line(chart, source);
        assert(Number(await l.getAttribute("data-points")) === 90, `${source}: a point a day`);
        assert(Number(await l.getAttribute("data-last")) === last(h, source).used, `${source}: its last is quota.history's (${last(h, source).used})`);
        assert((await root.locator(`[data-chart=${chart}] [data-legend="${source}"]`).count()) === 1, `${source} in the legend`);
      }
      assert((await root.locator("[data-range='90']").getAttribute("aria-pressed")) === "true", "90 days, unless said");
      await root.locator("[data-range='30']").click();
      await eventually("30 days", async () => Number(await line("stored", "media.bytes").getAttribute("data-points")) === 30, 5_000);
      // The crosshair: the right end is today, every series said.
      const svg = root.locator("[data-chart=stored] svg");
      const box = await svg.boundingBox();
      await page.mouse.move(box.x + box.width - 18, box.y + box.height / 2);
      const tip = root.locator("[data-chart=stored] [data-tip]");
      await eventually("the tooltip", async () => (await tip.count()) === 1, 3_000);
      assert((await tip.getAttribute("data-day")) === h.find((e) => e.type === "end").to, "today");
      assert((await tip.locator(".tip-row").count()) === 3, "every series at that day");
      await shoot(page, "history");
      // The keyboard finds the day too.
      await page.mouse.move(0, 0);
      await svg.focus();
      await page.keyboard.press("ArrowLeft");
      await eventually("a day back", async () => (await tip.getAttribute("data-day")) !== null && (await tip.getAttribute("data-day")) < h.find((e) => e.type === "end").to, 3_000);
      // The table: the same points.
      await root.locator("[data-view=table]").click();
      const today = h.find((e) => e.type === "end").to;
      const cell = root.locator(`[data-history-table] tr[data-day="${today}"] [data-col="media.bytes"]`);
      await eventually("the table", async () => (await cell.count()) === 1, 3_000);
      assert((await text(cell)) === words(last(h, "media.bytes").used), "today's files kept");
      assert((await root.locator("[data-history-table] tbody tr").count()) === 30, "a row a day");
      // The whole system's, for an admin: the system's sum.
      await root.locator("[data-view=chart]").click();
      const sys = page.locator("[data-surface=quota-history][data-whose=everyone]");
      await eventually("the system's history", async () => (await sys.locator('[data-chart=stored] [data-series="media.bytes"]').count()) === 1, 10_000);
      const all = sy("quota", "history", "--everyone", "true", "--days", "90");
      assert(Number(await sys.locator('[data-chart=stored] [data-series="media.bytes"]').getAttribute("data-last")) === last(all, "media.bytes").used, "the system's files kept");
      await page.setViewportSize({ width: 1440, height: 900 });
      await sys.scrollIntoViewIfNeeded();
      await shoot(page, "history-admin");
    } finally {
      await context.close();
    }
  });

  await test("a_viewer_cannot_read_anothers_history", async () => {
    const owner = sy("quota", "usage").find((e) => e.type === "usage").principal;
    for (const asked of [["--principal", owner], ["--everyone", "true"]]) {
      const r = as("quota", "history", ...asked);
      assert(r.some((e) => e.type === "error") && !r.some((e) => e.type === "point"), `${asked.join(" ")}: refused, nothing answered: ${JSON.stringify(r).slice(0, 200)}`);
    }
    assert(as("quota", "history", "--days", "7").filter((e) => e.type === "point").length === 7 * 9, "their own: every source, every day");
    const { page, context } = await openAs(s.token, `#/usage/${encodeURIComponent(owner)}`);
    try {
      await eventually("refused on the page", async () => (await page.locator("[data-surface=quota-history] [data-refused]").count()) === 1, 5_000);
      assert((await page.locator("[data-surface=quota-history] [data-series]").count()) === 0, "no line drawn");
    } finally {
      await context.close();
    }
  });

  await test("quota_table_is_an_admins", async () => {
    const { page, context } = await openAs(s.token, "#/quota");
    try {
      await eventually("refused", async () => (await page.locator("[data-surface=quota-table] [data-refused]").count()) === 1);
    } finally {
      await context.close();
    }
  });
};
