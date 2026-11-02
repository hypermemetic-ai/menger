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
// `[data-total]`, `[data-refused]`.
//
// The owner (an admin of *, exempt) and an editor of its own, made here;
// every case leaves the editor's allowance as the default.
import path from "node:path";
import { makeKey, sessionCookies, writeSession } from "../../scripts/throwaway-login.mjs";
import { synapse } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, openPage, sy, server, session } = ctx;
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
        await page.evaluate((t) => document.querySelectorAll("[data-plexus-theme], quota-usage, quota-table").forEach((e) => e.setAttribute("data-plexus-theme", t)), theme);
        await page.waitForTimeout(150);
        await page.screenshot({ path: path.join(shots, `${name}-${w}-${theme}.png`), fullPage: true });
      }
    }
    await page.setViewportSize({ width: 1440, height: 900 });
  }

  await test("usage_page_a_bar_per_source", async () => {
    const { page, context } = await openAs(s.token, "#/usage");
    const root = page.locator("[data-surface=quota-usage]");
    try {
      await eventually("a row per source", async () => (await root.locator("[data-id]").count()) === 9);
      const tickets = root.locator('[data-id="tickets.bytes"]');
      assert((await text(tickets.locator("[data-field=used]"))) === "4.3 KB", `tickets used: ${await text(tickets.locator("[data-field=used]"))}`);
      assert((await text(tickets.locator("[data-field=cap]"))) === "50 MB", "the free tier's 50 MB");
      assert((await text(tickets.locator("[data-field=from]"))) === "the default", "from the default");
      assert((await text(root.locator('[data-id="tickets.count"] [data-field=cap]'))) === "unlimited", "a count unlimited");
      assert((await root.locator('[data-id="media.fetch.calls"]').getAttribute("data-window")) === "day", "a day's counter");
      // Capped near what is used: the bar fills and says so, live after a reload.
      sy("quota", "set", "--principal", editor, "--source", "tickets.bytes", "--cap", "5000");
      await page.reload();
      await eventually("the bar near its cap", async () => (await tickets.locator("[data-bar]").getAttribute("data-state")) === "near");
      assert(Number(await tickets.locator("[data-bar]").getAttribute("data-fill")) === 86, "4321 of 5000: 86%");
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
      assert((await text(cell.locator("[data-field=used]"))) === "4.3 KB", "the editor's tickets");
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

  await test("quota_table_is_an_admins", async () => {
    const { page, context } = await openAs(s.token, "#/quota");
    try {
      await eventually("refused", async () => (await page.locator("[data-surface=quota-table] [data-refused]").count()) === 1);
    } finally {
      await context.close();
    }
  });
};
