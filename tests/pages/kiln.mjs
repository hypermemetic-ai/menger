// LAYERS-20: kiln-live at #/kiln — plexus-examples' kiln (Rust, its log in SQLite) as wasm in a worker of the
// tab, reached through its generated client as frames; its generated part ui/src/kiln-activation.svelte
// (scripts/kiln-activation). Run alone by `node tests/pages_browser.mjs --group kiln --seed 200`.
// KILN_SHOT=<file.png> shoots the page at 1100 wide after a firing, a refusal and a heat.
//
// Marks: `[data-surface=kiln-live]` (`data-state`, `data-store`), `[data-field=…]`, `[data-action=fire|heat]`,
// `[data-refused]`, `[data-firing=<id>]`.
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, openPage } = ctx;
  const kiln = (page) => page.locator("[data-surface=kiln-live]");
  const errors = (seen) => seen.console.filter((l) => l.startsWith("error") || l.startsWith("pageerror"));
  const running = async (page, seen) => {
    await eventually("kiln-live running", async () => (await kiln(page).getAttribute("data-state", { timeout: 1000 })) === "running", 30_000).catch(async (e) => {
      const said = await kiln(page).textContent({ timeout: 500 }).catch(() => "(no component)");
      throw new Error(`${e.message}\nstate ${await kiln(page).getAttribute("data-state").catch(() => null)}: ${said}\nconsole: ${seen.console.slice(-10).join("\n")}`);
    });
  };
  const firingsOf = (page) => kiln(page).locator("[data-firing]").evaluateAll((els) => els.map((e) => e.textContent.trim()));
  async function fire(page, cone) {
    await kiln(page).locator("input[data-field=cone]").fill(String(cone));
    await kiln(page).locator("[data-action=fire]").click();
    await eventually(`fire cone ${cone} answered`, async () => (await kiln(page).locator("[data-action=fire]").textContent()) === "fire" && !(await kiln(page).locator("[data-action=fire]").isDisabled()));
  }

  let held = null; // the first tab: its worker holds the browser's file store

  await test("kiln_live_fires_refuses_and_heats_in_a_worker_with_no_console_error", async () => {
    const { context, page, seen } = await openPage("#/kiln");
    const kilnRequests = [];
    page.on("requestfinished", async (r) => { if (r.url().includes("kiln-activation")) kilnRequests.push({ url: r.url(), sizes: await r.sizes().catch(() => null) }); });
    const workers = [];
    page.on("worker", (w) => workers.push(w.url()));
    await running(page, seen);
    assert((await kiln(page).locator("[data-field=interface]").textContent()) === "abad598999eaf1bc", "the kiln's interface");
    assert((await kiln(page).getAttribute("data-store")) === "file", `the store: ${await kiln(page).locator("[data-field=store]").textContent()}`);
    assert(workers.length === 1 && /\/app\/[^/]+\/components\/kiln-activation\.js$/.test(workers[0]), `one worker, from the module's version path: ${JSON.stringify(workers)}`);

    await fire(page, 6);
    await eventually("the firing in the log", async () => (await firingsOf(page)).join("|") === "#1 · cone 6 · 2 h · by you");
    assert((await kiln(page).locator("[data-refused]").count()) === 0, "no refusal after cone 6");

    await fire(page, 11);
    const refused = kiln(page).locator("[data-refused]");
    await eventually("cone 11 refused", async () => (await refused.getAttribute("data-refused", { timeout: 500 })) === "too-hot");
    const said = await refused.textContent();
    assert(said.includes("too-hot") && said.includes("cone 11"), `the refusal shown plainly: ${said}`);
    assert((await firingsOf(page)).length === 1, "a refused firing is not in the log");

    await kiln(page).locator("input[data-field=cone]").fill("6");
    await kiln(page).locator("[data-action=heat]").click();
    await eventually("heat left after 12 readings", async () => (await kiln(page).locator("[data-field=heat-left]").count()) === 1, 15_000);
    const readings = await kiln(page).locator("[data-field=readings] li").evaluateAll((els) => els.map((e) => e.textContent.trim()));
    assert(readings.length === 12 && readings[0].startsWith("0′") && readings[0].includes("climbing"), `12 readings from minute 0: ${JSON.stringify(readings)}`);

    const frames = (await kiln(page).locator("[data-field=frames]").textContent()).split("\n");
    const open = frames.find((l) => l.includes('"kiln.heat"'));
    const call = open?.match(/^→ \[(\d+),/)?.[1];
    assert(call && frames.includes(`→ [${call},5,null]`), `the cancel frame crossed: ${frames.join(" / ")}`);

    const calls = (await sentCalls(page)).map((c) => c.method);
    assert(!calls.some((m) => m?.startsWith("kiln")), `no kiln call went to menger's server: ${calls.join(", ")}`);
    assert(errors(seen).length === 0, `no console error: ${errors(seen).join("\n")}`);
    console.log(`     worker ${workers[0]}\n     the module's fetches: ${JSON.stringify(kilnRequests)}`);

    if (process.env.KILN_SHOT) {
      await page.setViewportSize({ width: 1100, height: 900 });
      await page.waitForTimeout(200);
      await page.screenshot({ path: process.env.KILN_SHOT, fullPage: true });
      console.log(`     shot ${process.env.KILN_SHOT}`);
    }
    held = { context, page, seen };
  });

  await test("kiln_live_says_the_kiln_runs_here_and_menger_takes_no_part", async () => {
    assert(held, "the first case's page");
    const what = await kiln(held.page).locator(".what").textContent();
    for (const want of ["running in a worker of this tab", "menger's server takes no part", "kept by this browser only"]) {
      assert(what.replace(/\s+/g, " ").includes(want), `it says "${want}": ${what}`);
    }
  });

  await test("kiln_live_keeps_its_log_across_a_reload", async () => {
    assert(held, "the first case's page");
    const { page, seen } = held;
    await page.reload();
    await running(page, seen);
    assert((await kiln(page).getAttribute("data-store")) === "file", `the store after a reload: ${await kiln(page).locator("[data-field=store]").textContent()}`);
    assert((await firingsOf(page)).join("|") === "#1 · cone 6 · 2 h · by you", `the log after a reload: ${JSON.stringify(await firingsOf(page))}`);
    await fire(page, 7);
    await eventually("a second firing after the reload", async () => (await firingsOf(page)).length === 2);
    assert(errors(seen).length === 0, `no console error: ${errors(seen).join("\n")}`);
  });

  await test("kiln_live_with_no_file_store_says_the_log_will_not_outlive_the_page_and_still_works", async () => {
    assert(held, "the first case's page");
    // The first tab's worker holds the kiln's files in the browser's file store; a second tab's worker is
    // given none of them (NoModificationAllowedError), so it keeps the log in memory and says so.
    const page = await held.context.newPage();
    const seen = { console: [] };
    page.on("console", (m) => seen.console.push(`${m.type()}: ${m.text()}`));
    page.on("pageerror", (e) => seen.console.push(`pageerror: ${e.message}`));
    await page.goto(`${ctx.server.origin}/app/#/kiln`);
    await running(page, seen);
    assert((await kiln(page).getAttribute("data-store")) === "memory", "the second tab's store is memory");
    const store = await kiln(page).locator("[data-field=store]").textContent();
    assert(store.includes("will not outlive this page") && store.includes("NoModificationAllowedError"), `it says so, and why: ${store}`);
    assert((await firingsOf(page)).length === 0, "an empty log in memory");
    await fire(page, 5);
    await eventually("a firing in memory", async () => (await firingsOf(page)).join("|") === "#1 · cone 5 · 2 h · by you");
    assert(errors(seen).length === 0, `no console error: ${errors(seen).join("\n")}`);
    await page.close();
    await held.context.close();
  });
};
