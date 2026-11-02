// SURFACE-5: create, pick and bulk — `node tests/pages_browser.mjs --group create`.
//
// Marks: ticket-create `[data-surface=ticket-create][data-form="tickets.create"]`, its fields
// `[data-field=shortcode|kind|severity|title|body]` (V2-25: no epic menu), the picked values
// `[data-field-value=parent|blocked_by] [data-chip-id]`, `[data-unblock=<id>]`, `[data-preview-toggle]`,
// `[data-preview]`, the write `[data-action="tickets.create"]`, its refusal `[data-refused]`;
// ticket-picker `[data-surface=ticket-picker][data-picker=<name>]`, its field `input[data-field=<name>]`,
// its hits `[data-picker-hits]`, each `[data-pick=<id>]` with its matched word `mark[data-match]`, the hit ↑↓
// chose `li[aria-selected=true]`; bulk-bar `[data-surface=bulk-bar]`, its count
// `[data-bulk-count]`, `[data-field=filter|action|value|max]`, `[data-action=dry-run]`, the dry run's
// `[data-dry-count=<n>]`, the writes `[data-action="tickets.update_many"]` and `[data-action="tickets.archive"]`,
// the answer's `[data-changed=<n>]` and its rows `[data-id]` (past 24, `[data-more]`), refusals `[data-refused]`.
//
// The board's rows do not emit `board.selected` yet (SURFACE-2), so `bulk_moves_the_selection`
// pushes a fixture bundle — ui/ plus `bulk-stub` at `#/bulk-stub/:ids`, a button that emits
// `board.selected { ids }` on the bus, the bulk bar and a `<board-rows filter="in CRTB"/>` — and
// rolls back to the runner's v2 after.
//
// V2-25: the epic menu went with the `epics` activation; a ticket is born in its shortcode's epic
// (`genesis` on the throwaway), and `create_in_another_epic` went with `epics create`.
import { bundle, push, sentCalls } from "../../scripts/ui-throwaway.mjs";

const STUB = `<script>
  // SURFACE-5's fixture: the rows' selection, until board-rows emits it (SURFACE-2).
  prop("ids");
  fn send(state, ev) { emit("board.selected", #{ ids: ev.props.ids.split(",") }); state }
</script>

<div class="stub" data-surface="bulk-stub">
  <button type="button" data-stub-select on:click={send}>select</button>
  <bulk-bar/>
  <div class="rows"><board-rows filter="in CRTB"/></div>
</div>

<style>
  :scope { display: block; }
  .rows { display: flex; height: 320px; }
</style>
`;

export default async (ctx) => {
  const { test, assert, eventually, openPage, marked, sameDocument, sy, server, session, v1, v2, path, fs } = ctx;
  const one = (events, type) => events.find((e) => e.type === type);
  const ticket = (id) => one(sy("tickets", "get", "--id", id), "ticket")?.ticket;
  const make = (...a) => {
    const t = one(sy("tickets", "create", ...a), "created")?.ticket;
    assert(t, `created: ${a.join(" ")}`);
    return t;
  };
  const hash = (page) => page.evaluate(() => location.hash);

  // The tickets the cases work on: CRTA for create and pick, CRTB for the bulk bar.
  const crta = make("--epic", "CRTA", "--title", "the create cases", "--kind", "scope");
  const zephyr = make("--epic", "CRTA", "--title", "zephyr kiln shelf", "--parent", crta.id);
  const quartz = make("--epic", "CRTA", "--title", "quartz kiln door", "--parent", crta.id);
  const crtb = make("--epic", "CRTB", "--title", "the bulk cases", "--kind", "scope");
  const bulk = [1, 2, 3, 4].map((i) => make("--epic", "CRTB", "--title", `bulk row ${i}`, "--parent", crtb.id));

  const form = (page) => page.locator('[data-surface=ticket-create][data-form="tickets.create"]');
  // V2-25: no epic menu (the `epics` activation went); the shortcode field is the form's first.
  const formUp = (page) => eventually("ticket-create draws its form", async () =>
    (await form(page).count()) === 1 && (await form(page).locator("[data-field=shortcode]").count()) === 1);

  await test("create_fills_and_navigates", async () => {
    const { page, context } = await openPage("#/new");
    try {
      await formUp(page);
      const origin = await marked(page);
      const f = form(page);
      assert((await f.locator("[data-field=epic]").count()) === 0, "V2-25: no epic menu");
      // The disabled state: no title, no shortcode.
      assert(await f.locator('[data-action="tickets.create"]').isDisabled(), "Create waits for a title and a shortcode");
      await f.locator("[data-field=shortcode]").fill("CRTA");
      await f.locator("[data-field=title]").fill("a firing log, made from the form");
      await f.locator("[data-field=kind]").selectOption("gap");
      await f.locator("[data-field=severity]").selectOption("High");
      await f.locator("[data-field=body]").fill("the **kiln** log");
      await f.locator("[data-preview-toggle]").click();
      await eventually("the preview draws the body's markdown", async () => (await f.locator("[data-preview] strong").textContent({ timeout: 500 })) === "kiln");
      await f.locator("[data-preview-toggle]").click();
      assert((await f.locator("[data-field=body]").inputValue()) === "the **kiln** log", "back to writing, the body kept");
      // Both schemes: the tokens on a dark wrapper — a field paints the dark `--bg` the bundle's theme.css declares.
      const seen = await page.evaluate(async () => {
        // UI-19: the theme by its version's path (the page asks it by its content URL).
        const v = document.querySelector('meta[name="menger-ui-version"]')?.content;
        const at = v ? `/app/${v}/theme.css` : null;
        const css = at ? await (await fetch(at)).text() : "";
        const block = css.slice(css.indexOf('[data-plexus-theme="dark"]'));
        const want = (block.match(/--bg:\s*([^;]+);/) || [])[1]?.trim();
        const probe = document.createElement("i");
        probe.style.color = want ?? "";
        document.body.append(probe);
        const wanted = getComputedStyle(probe).color;
        probe.remove();
        const el = document.querySelector("ticket-create");
        const field = el.querySelector("[data-field=title]");
        const light = getComputedStyle(field).backgroundColor;
        el.setAttribute("data-plexus-theme", "dark");
        const dark = getComputedStyle(field).backgroundColor;
        el.removeAttribute("data-plexus-theme");
        return { want, wanted, light, dark };
      });
      assert(seen.want && seen.dark === seen.wanted && seen.dark !== seen.light, `a field on the dark wrapper paints the dark --bg (${seen.want}): ${JSON.stringify(seen)}`);
      await f.locator('[data-action="tickets.create"]').click();
      const at = await eventually("create moves the page to the ticket made", async () => {
        const h = await hash(page);
        return /^#\/ticket\/CRTA-\d+$/.test(h) && h;
      });
      const id = at.replace("#/ticket/", "");
      await eventually("the ticket page draws it", async () => (await page.locator("[data-ticket-title]").textContent({ timeout: 500 })) === "a firing log, made from the form");
      await sameDocument(page, origin, "after the create");
      const t = ticket(id);
      assert(t.kind === "gap" && t.severity === "High" && t.body === "the **kiln** log" && t.parent === crta.id && t.status === "Draft",
        `the ticket as the form said (parent: the shortcode's root): ${JSON.stringify({ kind: t.kind, severity: t.severity, body: t.body, parent: t.parent, status: t.status })}`);
    } finally {
      await context.close();
    }
  });

  await test("the_picker_finds_and_picks", async () => {
    const { page, context } = await openPage("#/new");
    try {
      await formUp(page);
      const f = form(page);
      const blockers = f.locator("[data-surface=ticket-picker][data-picker=blocked_by]");
      const field = blockers.locator("input[data-field=blocked_by]");
      // Enter takes the first hit.
      await field.fill("zephyr");
      await eventually("find answers the picker", async () => (await blockers.locator(`[data-picker-hits] [data-pick="${zephyr.id}"]`).count()) === 1);
      await field.press("Enter");
      await eventually("picked { name: blocked_by } fills the blockers", async () => (await f.locator(`[data-field-value=blocked_by] [data-chip-id="${zephyr.id}"]`).count()) === 1);
      assert((await blockers.locator("[data-picker-hits]").count()) === 0, "the pick closes the hits");
      assert((await blockers.locator("input[data-field=blocked_by]").inputValue()) === "", "and empties the field");
      assert((await f.locator("[data-field-value=parent] [data-chip-id]").count()) === 0, "the parent picker heard nothing: picked carries its name");
      // A click takes the one clicked.
      await blockers.locator("input[data-field=blocked_by]").fill("kiln");
      await eventually("find's hits for kiln", async () => (await blockers.locator(`[data-pick="${quartz.id}"]`).count()) === 1);
      const marks = await blockers.locator("[data-pick] mark[data-match]").allTextContents();
      assert(marks.length >= 2 && marks.every((m) => m === "kiln"), `the matched word is marked in each hit: ${JSON.stringify(marks)}`);
      // ↑↓ choose the hit Enter takes: down twice is the second, up once the first, down again the second.
      const hits = await blockers.locator("[data-pick]").evaluateAll((els) => els.map((e) => e.getAttribute("data-pick")));
      const input = blockers.locator("input[data-field=blocked_by]");
      for (const k of ["ArrowDown", "ArrowDown", "ArrowUp", "ArrowDown"]) await input.press(k);
      await eventually("↑↓ mark the chosen hit", async () => (await blockers.locator(`li[aria-selected=true] [data-pick="${hits[1]}"]`).count()) === 1);
      await input.press("Enter");
      await eventually("Enter takes the chosen hit", async () => (await f.locator(`[data-field-value=blocked_by] [data-chip-id="${hits[1]}"]`).count()) === 1);
      if (hits[1] !== quartz.id) await f.locator(`[data-unblock="${hits[1]}"]`).click();
      await blockers.locator("input[data-field=blocked_by]").fill("kiln");
      await eventually("find's hits for kiln again", async () => (await blockers.locator(`[data-pick="${quartz.id}"]`).count()) === 1);
      await blockers.locator(`[data-pick="${quartz.id}"]`).click();
      await eventually("a click picks", async () => (await f.locator(`[data-field-value=blocked_by] [data-chip-id="${quartz.id}"]`).count()) === 1);
      assert((await hash(page)) === "#/new", "a pick never navigates");
      // Escape closes; nothing found is said.
      await blockers.locator("input[data-field=blocked_by]").fill("no-ticket-holds-this");
      await eventually("nothing found is said", async () => (await blockers.locator("[data-empty]").count()) === 1);
      await blockers.locator("input[data-field=blocked_by]").press("Escape");
      await eventually("Escape closes the hits", async () => (await blockers.locator("[data-picker-hits]").count()) === 0);
      // A blocker taken off; the parent by its own picker.
      await f.locator(`[data-unblock="${zephyr.id}"]`).click();
      await eventually("a blocker comes off", async () => (await f.locator(`[data-field-value=blocked_by] [data-chip-id="${zephyr.id}"]`).count()) === 0);
      const parent = f.locator("[data-surface=ticket-picker][data-picker=parent]");
      await parent.locator("input[data-field=parent]").fill(crta.id);
      await eventually("the parent picker finds the root", async () => (await parent.locator(`[data-pick="${crta.id}"]`).count()) === 1);
      await parent.locator("input[data-field=parent]").press("Enter");
      await eventually("picked { name: parent } fills the parent", async () => (await f.locator(`[data-field-value=parent] [data-chip-id="${crta.id}"]`).count()) === 1);
      await f.locator("[data-field=title]").fill("picked, then made");
      await f.locator('[data-action="tickets.create"]').click();
      const at = await eventually("created", async () => { const h = await hash(page); return /^#\/ticket\/CRTA-\d+$/.test(h) && h; });
      const t = ticket(at.replace("#/ticket/", ""));
      assert(JSON.stringify(t.blocked_by) === JSON.stringify([quartz.id]) && t.parent === crta.id, `the picks are the ticket's: ${JSON.stringify({ blocked_by: t.blocked_by, parent: t.parent })}`);
    } finally {
      await context.close();
    }
  });

  await test("the_picker_takes_the_exact_match_at_once", async () => {
    const { page, context } = await openPage("#/new");
    try {
      await formUp(page);
      const f = form(page);
      const blockers = f.locator("[data-surface=ticket-picker][data-picker=blocked_by]");
      const input = () => blockers.locator("input[data-field=blocked_by]");
      // The last query's rows held: CRTA-'s, the root first.
      await input().fill("CRTA-");
      await eventually("find's hits for CRTA-", async () => (await blockers.locator(`[data-pick="${crta.id}"]`).count()) === 1 && (await blockers.locator(`[data-pick="${quartz.id}"]`).count()) === 1);
      // Typed and entered inside the debounce: the exact id, never the held first row.
      await input().fill(quartz.id);
      await input().press("Enter");
      await eventually("Enter at once takes the exact match", async () => (await f.locator(`[data-field-value=blocked_by] [data-chip-id]`).count()) === 1);
      const chips = await f.locator("[data-field-value=blocked_by] [data-chip-id]").evaluateAll((els) => els.map((e) => e.getAttribute("data-chip-id")));
      assert(JSON.stringify(chips) === JSON.stringify([quartz.id]), `the chip is the exact match: ${JSON.stringify(chips)}`);
      // Words no held row holds: Enter at once never takes a held row — it takes quartz once find has
      // answered it, or nothing until then.
      await f.locator(`[data-unblock="${quartz.id}"]`).click();
      await input().fill("zephyr");
      await eventually("find's hit for zephyr", async () => (await blockers.locator(`[data-pick="${zephyr.id}"]`).count()) === 1);
      await input().fill("quartz");
      await input().press("Enter");
      await page.waitForTimeout(400);
      assert((await f.locator(`[data-field-value=blocked_by] [data-chip-id="${zephyr.id}"]`).count()) === 0, "a held row that does not hold the query is never taken");
      if ((await f.locator(`[data-field-value=blocked_by] [data-chip-id="${quartz.id}"]`).count()) === 0) {
        await eventually("find answers quartz", async () => (await blockers.locator(`[data-pick="${quartz.id}"]`).count()) === 1);
        assert((await blockers.locator(`[data-pick="${zephyr.id}"]`).count()) === 0, "a held row that does not hold the query is not drawn");
        await input().press("Enter");
      }
      await eventually("quartz taken", async () => (await f.locator(`[data-field-value=blocked_by] [data-chip-id="${quartz.id}"]`).count()) === 1);
    } finally {
      await context.close();
    }
  });

  await test("create_with_a_parent_route_prefills", async () => {
    const { page, context } = await openPage(`#/new/${zephyr.id}`);
    try {
      await formUp(page);
      const f = form(page);
      await eventually("the parent's chip", async () => (await f.locator(`[data-field-value=parent] [data-chip-id="${zephyr.id}"]`).count()) === 1);
      await eventually("the shortcode is the parent's", async () => (await f.locator("[data-field=shortcode]").inputValue()) === "CRTA");
      await f.locator("[data-field=title]").fill("a child of zephyr");
      // FRAME-36: ctrl+enter, declared with keys(), creates from the field — the host takes the press.
      await f.locator("[data-field=title]").evaluate((el) => {
        window.__s5_reached = false;
        el.addEventListener("keydown", (e) => { if (e.key === "Enter") window.__s5_reached = true; });
      });
      await f.locator("[data-field=title]").press("Control+Enter");
      const at = await eventually("created", async () => { const h = await hash(page); return /^#\/ticket\/CRTA-\d+$/.test(h) && h; });
      assert((await page.evaluate(() => window.__s5_reached)) === false, "ctrl+enter never reached the field: the host took it");
      const t = ticket(at.replace("#/ticket/", ""));
      assert(t.parent === zephyr.id && t.title === "a child of zephyr", `filed under the route's parent: ${t.parent}`);
    } finally {
      await context.close();
    }
  });

  await test("a_create_refusal_is_said", async () => {
    const { page, context } = await openPage("#/new");
    try {
      await formUp(page);
      const f = form(page);
      await f.locator("[data-field=shortcode]").fill("NOSUCHX");
      await f.locator("[data-field=title]").fill("a build where no scope is");
      await f.locator('[data-action="tickets.create"]').click();
      const said = await eventually("the in-band refusal under the form", async () => (await f.locator("[data-refused]").textContent({ timeout: 500 })) || false);
      assert(/must be kind 'scope'/.test(said), `the refusal in the method's words: ${said}`);
      assert((await hash(page)) === "#/new", "a refusal stays on the form");
      assert(one(sy("tickets", "get", "--id", "NOSUCHX-1"), "ticket") == null, "nothing was made");
    } finally {
      await context.close();
    }
  });

  await test("bulk_moves_the_selection", async () => {
    const work = path.dirname(v1.tar);
    const fixture = bundle(work, "create-fixture", {
      engine: v1.engine,
      edit: (ui) => {
        fs.writeFileSync(path.join(ui, "src", "bulk-stub.rhx"), STUB);
        const routes = JSON.parse(fs.readFileSync(path.join(ui, "routes.json"), "utf8"));
        routes["#/bulk-stub/:ids"] = "bulk-stub";
        fs.writeFileSync(path.join(ui, "routes.json"), JSON.stringify(routes));
      },
    });
    const pushed = push(server, session, path.join(work, "signer.key"), fixture);
    assert(pushed?.type === "pushed", `the fixture bundle pushed: ${JSON.stringify(pushed)}`);
    const picked = [bulk[0].id, bulk[1].id];
    const { page, context } = await openPage(`#/bulk-stub/${picked.join(",")}`);
    try {
      const bar = page.locator("[data-surface=bulk-stub] [data-surface=bulk-bar]");
      const row = (id) => page.locator(`[data-surface=bulk-stub] .board-row[data-id="${id}"]`);
      await eventually("the stub draws the bar and the rows", async () => (await bar.count()) === 1 && (await row(bulk[0].id).count()) === 1);
      assert((await bar.locator("[data-bulk-count]").textContent()) === "0", "nothing selected yet");
      await page.locator("[data-stub-select]").click();
      await eventually("board.selected { ids } heard: the count", async () => (await bar.locator("[data-bulk-count]").textContent()) === "2");
      await bar.locator("[data-field=value]").selectOption("Ready");
      await bar.locator("[data-action=dry-run]").click();
      await eventually("the dry run's count first", async () => (await bar.locator('[data-dry-count="2"]').count()) === 1);
      assert(ticket(bulk[0].id).status === "Draft", "the dry run wrote nothing");
      await bar.locator('[data-action="tickets.update_many"]').click();
      await eventually("update_many's answer said: changed 2", async () => (await bar.locator('[data-changed="2"]').count()) === 1);
      for (const id of picked) {
        await eventually(`${id}'s chip moves live to Ready`, async () => (await row(id).getAttribute("data-status", { timeout: 500 })) === "Ready");
        assert(ticket(id).status === "Ready", `${id} is Ready`);
      }
      assert((await row(bulk[2].id).getAttribute("data-status")) === "Draft" && ticket(bulk[2].id).status === "Draft", "a row not selected stays");
      // Past max, the bar refuses before anything is sent.
      const sentBefore = (await sentCalls(page)).filter((c) => c.method === "tickets.update_many").length;
      await page.locator("[data-stub-select]").click();
      await bar.locator("[data-field=value]").selectOption("Pending");
      await bar.locator("[data-field=max]").fill("1");
      await bar.locator("[data-field=max]").dispatchEvent("change");
      await bar.locator("[data-action=dry-run]").click();
      const over = await eventually("more than max is refused", async () => (await bar.locator("[data-dry-run] [data-refused]").textContent({ timeout: 500 })) || false);
      assert(/more than max 1/.test(over) && (await bar.locator('[data-action="tickets.update_many"]').count()) === 0, `refused, no write offered: ${over}`);
      await page.waitForTimeout(200);
      assert((await sentCalls(page)).filter((c) => c.method === "tickets.update_many").length === sentBefore, "nothing sent");
      assert(ticket(bulk[0].id).status === "Ready", "nothing written");
      await bar.locator("[data-field=max]").fill("500");
      await bar.locator("[data-field=max]").dispatchEvent("change");
      // A refusal said: a doc never waits nor is waited on, so a kind of doc for two that block each other is refused.
      await page.locator("[data-stub-select]").click();
      await bar.locator("[data-field=action]").selectOption("kind");
      await bar.locator("[data-field=value]").selectOption("doc");
      await sy("tickets", "update", "--id", bulk[1].id, "--block", bulk[0].id);
      await bar.locator("[data-action=dry-run]").click();
      await bar.locator('[data-action="tickets.update_many"]').click();
      const said = await eventually("update_many's refusal is said", async () => (await bar.locator("[data-refused]").textContent({ timeout: 500 })) || false);
      assert(said.length > 0 && ticket(bulk[0].id).kind === "build", `refused, nothing written: ${said}`);
    } finally {
      await context.close();
      const back = sy("ui", "rollback", "--to", v2.version);
      assert(back.some((e) => e.type !== "error"), `rolled back to v2: ${JSON.stringify(back)}`);
    }
  });

  await test("bulk_by_filter_dry_runs_first", async () => {
    const { page, context } = await openPage("#/bulk");
    try {
      const bar = page.locator("[data-surface=bulk-bar]");
      await eventually("the bar", async () => (await bar.count()) === 1);
      assert(await bar.locator("[data-action=dry-run]").isDisabled(), "nothing to ask until a selection or a filter");
      const filter = `in CRTB and kind build and status Draft`;
      const n = sy("tickets", "list", "--filter", filter, "--summary", "true").filter((e) => e.type === "summary").length;
      assert(n >= 1, `the filter admits some: ${n}`);
      await bar.locator("[data-field=filter]").fill(filter);
      await eventually(`nav.counts says ${n}`, async () => (await bar.locator("[data-bulk-count]").textContent()) === String(n));
      await bar.locator("[data-field=action]").selectOption("label");
      await bar.locator("[data-field=value]").fill("kilned");
      await bar.locator("[data-action=dry-run]").click();
      await eventually("the dry run's count", async () => (await bar.locator(`[data-dry-count="${n}"]`).count()) === 1);
      const labelled = () => sy("tickets", "list", "--filter", "label kilned", "--summary", "true").filter((e) => e.type === "summary").length;
      assert(labelled() === 0, "the dry run labelled nothing");
      await bar.locator('[data-action="tickets.update_many"]').click();
      await eventually(`changed ${n}`, async () => (await bar.locator(`[data-changed="${n}"]`).count()) === 1);
      assert(labelled() === n, `labelled ${n}`);
      // Archive: max refused by the service's own dry run, then the dry run, then the write.
      await bar.locator("[data-field=action]").selectOption("archive");
      if (n > 1) {
        await bar.locator("[data-field=max]").fill("1");
        await bar.locator("[data-field=max]").dispatchEvent("change");
        await bar.locator("[data-action=dry-run]").click();
        const said = await eventually("archive's dry run refuses past max", async () => (await bar.locator("[data-dry-run] [data-refused]").textContent({ timeout: 500 })) || false);
        assert(/over the 1/.test(said), `the refusal in the method's words: ${said}`);
        await bar.locator("[data-field=max]").fill("500");
        await bar.locator("[data-field=max]").dispatchEvent("change");
      }
      await bar.locator("[data-action=dry-run]").click();
      await eventually("archive's dry run answers its ids", async () => (await bar.locator(`[data-dry-count="${n}"]`).count()) === 1);
      const archivedNow = () => sy("tickets", "list", "--filter", "archived and in CRTB", "--summary", "true").filter((e) => e.type === "summary").length;
      assert(archivedNow() === 0, "the dry run archived nothing");
      await bar.locator('[data-action="tickets.archive"]').click();
      await eventually(`archived ${n}`, async () => (await bar.locator(`[data-changed="${n}"]`).count()) === 1);
      assert(archivedNow() === n, `${n} archived`);
    } finally {
      await context.close();
    }
  });
};
