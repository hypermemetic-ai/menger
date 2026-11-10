// SURFACE-4: the ticket page's writes — ticket-edit and ticket-actions —
// on the throwaway, through their test routes (#/form/:id → ticket-edit,
// #/actions/:id → ticket-actions) until SURFACE-3's ticket page places them.
//
// Marks: ticket-edit `[data-surface=ticket-edit]`, `[data-action=edit|cancel|tickets.update|tickets.amend]`,
// `[data-form=tickets.update|tickets.amend]`, `[data-field=title|kind|severity|hidden|parent|blocked_by|block|body|preview]`,
// `[data-kind-pick]`, `[data-sev-pick]`, `[data-unblock]`; ticket-actions `[data-surface=ticket-actions]`,
// `[data-group=status|labels|archive|rename|revision|admin|undelete]`, `[data-action=tickets.<method>|more-moves]`
// (`[data-to=<status>]` on a move, `[data-confirm]`, `[data-dry]`, `[data-open]`, `[data-remove]`),
// `[data-field=status|message|label|labels|restore-children|with-children|rename-to|reshortcode-to|rev|children|unblock|who]`,
// `[data-rf]`, `[data-ask]`, `[data-needs]`, `[data-archived]`, `[data-dry-answer]`, `[data-refused-on]`; both:
// `[data-for=<method>]` on an answer — `[data-answer]`, `[data-refused]`, `[data-loading]` — and `[data-empty]`.
// A write's answer is FRAME-37's `await bus.call(…)`: `{ ok, events, message }`.
import fs from "node:fs";
import { makeKey, readToken, sessionCookies, writeSession } from "../../scripts/throwaway-login.mjs";
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, server, path, ticketAt } = ctx;
  const EPIC = "WRIT";
  const get = (id) => sy("tickets", "get", "--id", id).find((e) => e.type === "ticket")?.ticket;
  const created = (out, what) => {
    const c = out.find((e) => e.type === "created");
    if (!c) throw new Error(`create ${what}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const make = (title, ...flags) => created(sy("tickets", "create", "--epic", EPIC, "--title", title, ...flags), title);
  const root = make("the writes' shortcode", "--kind", "scope");
  const child = (title, ...flags) => make(title, "--parent", root, ...flags);
  /** A scope rooting a shortcode of its own, under the writes' root. */
  const scopeOf = (epic, title) => created(sy("tickets", "create", "--epic", epic, "--kind", "scope", "--parent", root, "--title", title), title);
  const said = (loc, method, kind) => loc.locator(`[data-for="${method}"][data-${kind}]`);
  const text = async (loc) => (await loc.first().textContent({ timeout: 1000 })).trim();
  /** The frames the page sent for `method`. */
  const sent = async (page, method) => (await sentCalls(page)).filter((c) => c.method === method);
  /** A page signed in as a fresh key granted `role` on *, the grant taken back by `done()`. The page is
   *  opened signed in as the owner first, then its cookies swapped and a page of the new session opened
   *  (an unsigned first load asks for what only a session may fetch). */
  const as = async (role, route) => {
    const keyFile = path.join(server.work, `${role}-writes.key`);
    const principal = makeKey(keyFile);
    const g = sy("members", "grant", "--principal", principal, "--scope", "*", "--role", role);
    assert(g.some((e) => e.type === "grant"), `${role} granted: ${JSON.stringify(g)}`);
    const file = path.join(server.work, `${role}-writes.session`);
    await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, file, role);
    const { context, page: first } = await openPage("");
    await first.close();
    await context.clearCookies();
    await context.addCookies(sessionCookies(server.origin, readToken(file)));
    const page = await context.newPage();
    await page.goto(`${server.origin}/app/${route}`);
    const done = async () => {
      await context.close();
      sy("members", "revoke", "--principal", principal, "--scope", "*");
    };
    return { page, done };
  };
  /** `id` typed into the `name` picker (SURFACE-5's) and taken with Enter once it is a hit. */
  const pick = async (loc, name, id) => {
    const input = loc.locator(`[data-picker=${name}] input`);
    await input.fill(id);
    await eventually(`${id} found by the ${name} picker`, async () => (await loc.locator(`[data-picker=${name}] [data-pick="${id}"]`).count()) === 1);
    await input.press("Enter");
  };
  const opened = async (page) => {
    const edit = page.locator("[data-surface=ticket-edit]");
    await eventually("the edit button is drawn", async () => (await edit.locator("[data-action=edit]").count()) === 1);
    await edit.locator("[data-action=edit]").click();
    await eventually("the form is open", async () => (await edit.locator("[data-open]").count()) === 1);
    return edit;
  };

  await test("amend_saves_the_body_and_the_page_shows_it", async () => {
    const id = child("a body to write");
    const { context, page } = await openPage(`#/form/${id}`);
    try {
      const edit = await opened(page);
      const form = edit.locator("[data-form='tickets.amend']");
      assert(await form.locator("[data-action='tickets.amend']").isDisabled(), "save body waits for an edit");
      await form.locator("textarea[data-field=body]").fill("# The heading\n\nsome *words* and a [link](https://example.com)\n\n<script>window.__pwned = 1</script>");
      // COMPS-5 (the owner: "all tickets are the same surface"): the preview is ticket-view's, as the page
      // draws the body — a body holding raw HTML (the script) is drawn whole in its sandboxed frame there too.
      const inPreviewFrame = async (sel) => {
        const f = await form.locator("[data-preview] iframe").first().elementHandle({ timeout: 500 }).then((h) => h?.contentFrame()).catch(() => null);
        return f ? await f.locator(sel).first().textContent({ timeout: 500 }).catch(() => null) : null;
      };
      await eventually("the preview draws the markdown, in the page's frame for raw HTML", async () => (await inPreviewFrame("h1")) === "The heading");
      assert((await inPreviewFrame("em")) === "words", "the preview's emphasis");
      assert((await form.locator("[data-preview] script").count()) === 0, "the preview puts no script in the page");
      // FRAME-30: the form's sheet is the theme's, light and dark.
      const ground = (theme) => page.evaluate((theme) => {
        document.querySelector("[data-surface=ticket-edit]").parentElement.setAttribute("data-plexus-theme", theme);
        return getComputedStyle(document.querySelector("[data-surface=ticket-edit] [data-open]")).backgroundColor;
      }, theme);
      const light = await ground("light");
      const dark = await ground("dark");
      assert(light === "rgb(255, 255, 255)" && dark === "rgb(28, 27, 25)", `the sheet is --panel in both schemes: ${light} / ${dark}`);
      if (process.env.PAGES_SHOTS) await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, "ticket-edit-dark.png"), fullPage: true });
      await ground("light");
      if (process.env.PAGES_SHOTS) await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, "ticket-edit-light.png"), fullPage: true });
      // Asked twice in one tick: one write, the button held while it is pending.
      await form.locator("[data-action='tickets.amend']").evaluate((b) => { b.click(); b.click(); });
      await eventually("the store holds the body", async () => get(id).body.startsWith("# The heading"));
      await eventually("the amend's answer under the body", async () => (await text(said(form, "tickets.amend", "answer"))) === "saved the body");
      await eventually("the watch brings it back: save body waits for the next edit", async () => form.locator("[data-action='tickets.amend']").isDisabled());
      assert((await page.evaluate(() => window.__pages.__pwned ?? window.__pwned)) === undefined, "no script ran");
      const amends = await sent(page, "tickets.amend");
      assert(amends.length === 1 && amends[0].params.id === id, `one amend sent: ${JSON.stringify(amends)}`);
      assert((await sent(page, "tickets.update")).length === 0, "the body alone: no update sent");
      await page.evaluate((id) => (location.hash = `#/ticket/${id}`), id);
      // EDITOR-18: a body holding raw HTML (the `<script>`) is drawn whole in a sandboxed frame (EDITOR-14):
      // its markdown inside the frame, its script never in the page.
      const shown = page.locator("[data-ticket-body] [data-surface=page-frame][data-kind=body] iframe[data-buffer=shown]");
      const inFrame = async () => { const f = await (await shown.elementHandle({ timeout: 500 }).catch(() => null))?.contentFrame(); return f ? (await f.locator("h1").textContent({ timeout: 500 }).catch(() => null)) : null; };
      await eventually("the ticket page frames the new body, its heading inside", async () => (await inFrame()) === "The heading", 10_000);
      const sandbox = (await shown.getAttribute("sandbox")) ?? "";
      assert(sandbox.split(/\s+/).includes("allow-scripts") && !sandbox.includes("allow-same-origin"), `sandboxed, an opaque origin: "${sandbox}"`);
      assert((await page.locator("[data-ticket-body] script, [data-ticket-body] h1").count()) === 0, "neither the script nor the markdown is drawn in the page");
      assert((await page.evaluate(() => window.__pwned)) === undefined, "the body's script never ran in the page");
      // The markdown path: a body with no raw HTML is drawn in the page itself, live.
      const plain = path.join(server.work, `writes-plain-${id}.md`);
      fs.writeFileSync(plain, "# The heading, plain\n\nsome *words* and a [link](https://example.com)\n");
      sy("tickets", "amend", "--id", id, "--body", `@${plain}`);
      await eventually("the ticket page draws the plain body's markdown in the page", async () => (await page.locator("[data-ticket-body] h1").textContent({ timeout: 500 }).catch(() => null)) === "The heading, plain", 5_000);
      assert((await page.locator("[data-ticket-body] em").first().textContent()) === "words", "the body's emphasis, in the page");
      assert((await page.locator("[data-ticket-body] [data-surface=page-frame][data-kind=body]").count()) === 0, "no frame for a body without raw HTML");
    } finally {
      await context.close();
    }
  });

  await test("update_changes_severity_and_blockers", async () => {
    const a = child("waits on b");
    const b = child("b, waited on");
    const { context, page } = await openPage(`#/form/${a}`);
    try {
      const edit = await opened(page);
      const form = edit.locator("[data-form='tickets.update']");
      const save = form.locator("[data-action='tickets.update']");
      assert(await save.isDisabled(), "save fields waits for a change");
      await form.locator("[data-sev-pick=High]").click();
      await pick(form, "blocker", b);
      await eventually("the blocker is a chip", async () => (await form.locator(`[data-field=blocked_by] [data-blocker="${b}"]`).count()) === 1);
      await form.locator("[data-field=title]").fill("waits on b, retitled");
      await eventually("the button counts the fields", async () => (await text(save)) === "save 3 fields");
      await save.click();
      await eventually("the store moved", async () => { const t = get(a); return t.severity === "High" && t.blocked_by.join() === b && t.title === "waits on b, retitled"; });
      await eventually("the update's answer", async () => /^saved .*severity/.test(await text(said(form, "tickets.update", "answer"))));
      const updates = await sent(page, "tickets.update");
      assert(updates.length === 1 && updates[0].params.source === "app" && updates[0].params.kind === undefined, `one update, from the app: ${JSON.stringify(updates)}`);
      // Cleared: severity none, the blocker's ×.
      await form.locator("[data-sev-pick=none]").click();
      await form.locator(`[data-unblock="${b}"]`).click();
      await save.click();
      await eventually("cleared in the store", async () => { const t = get(a); return t.severity == null && t.blocked_by.length === 0; });
      // A cycle: b waiting on a while a waits on b — refused under the form, nothing written.
      sy("tickets", "update", "--id", b, "--block", a);
      await pick(form, "blocker", b);
      await eventually("save offered again", async () => !(await save.isDisabled()));
      await save.click();
      await eventually("the cycle refused under the form, in the store's words", async () => /cycle/.test(await text(said(form, "tickets.update", "refused"))));
      assert(get(a).blocked_by.length === 0, "the cycle wrote nothing");
      await form.locator(`[data-unblock="${b}"]`).click();
      // The kind rides the one update, under set_kind's rules: doc refused for a ticket a blocker waits on.
      await form.locator("[data-kind-pick=gap]").click();
      await save.click();
      await eventually("the kind moved", async () => get(a).kind === "gap");
      const last = (await sent(page, "tickets.update")).at(-1);
      assert(last.params.kind === "gap" && (await sent(page, "tickets.set_kind")).length === 0, `the kind in the update: ${JSON.stringify(last)}`);
      await form.locator("[data-kind-pick=doc]").click();
      await save.click();
      await eventually("doc refused under the form", async () => /doc/.test(await text(said(form, "tickets.update", "refused"))));
      assert(get(a).kind === "gap", "nothing written");
    } finally {
      await context.close();
    }
  });

  await test("set_parent_by_the_picker", async () => {
    const other = child("another parent", "--kind", "build");
    const moved = child("to be moved", "--kind", "build");
    const below = make("under the moved one", "--parent", moved);
    const { context, page } = await openPage(`#/form/${moved}`);
    try {
      const edit = await opened(page);
      const form = edit.locator("[data-form='tickets.update']");
      // STUDIO-12: the picker is a bound mark filled once the form draws it — waited for, as every mark is.
      await eventually("one parent field: the picker's", async () => (await form.locator("input[data-field=parent]").count()) === 1, 5_000);
      // Its own child as its parent: the store refuses the cycle, said under the form.
      await pick(form, "parent", below);
      await eventually("the pick is the draft", async () => (await form.locator(`[data-field=parent-chosen] [data-chip-id="${below}"]`).count()) === 1);
      await form.locator("[data-action='tickets.update']").click();
      await eventually("its own descendant refused under the form, in the store's words", async () => (await said(form, "tickets.update", "refused").count()) === 1);
      assert(get(moved).parent === root, "nothing written");
      await pick(form, "parent", other);
      await form.locator("[data-action='tickets.update']").click();
      await eventually("the store moved it", async () => get(moved).parent === other);
      await eventually("said", async () => /^saved .*parent/.test(await text(said(form, "tickets.update", "answer"))));
      const last = (await sent(page, "tickets.update")).at(-1);
      assert(last.params.parent === other && (await sent(page, "tickets.set_parent")).length === 0, `the parent rides the update: ${JSON.stringify(last.params)}`);
    } finally {
      await context.close();
    }
  });

  // FRAME-36: the form's keys — ctrl+enter saves (in a field too, never a newline in the body), escape cancels.
  await test("the_edit_forms_keys", async () => {
    const id = child("saved by a key");
    const { context, page } = await openPage(`#/form/${id}`);
    try {
      const edit = await opened(page);
      const body = edit.locator("textarea[data-field=body]");
      await body.fill("by the keyboard");
      await edit.locator("[data-field=title]").fill("saved by ctrl+enter");
      await body.focus();
      await page.keyboard.press("Control+Enter");
      await eventually("both saves by one chord", async () => { const t = get(id); return t.title === "saved by ctrl+enter" && t.body === "by the keyboard"; });
      assert((await body.inputValue()) === "by the keyboard", "the chord typed nothing into the body");
      await page.keyboard.press("Escape");
      await eventually("escape closes the form", async () => (await edit.locator("[data-open]").count()) === 0 && (await edit.locator("[data-action=edit]").count()) === 1);
    } finally {
      await context.close();
    }
  });

  await test("set_status_offers_the_lifecycle_and_moves", async () => {
    const id = child("a lifecycle");
    const { context, page } = await openPage(`#/actions/${id}`);
    try {
      const status = page.locator("[data-surface=ticket-actions] [data-group=status]");
      const offered = () => status.locator("[data-action='tickets.set_status'][data-to]").evaluateAll((els) => els.map((e) => e.dataset.to));
      await eventually("the moves are drawn", async () => (await offered()).length > 0);
      assert(JSON.stringify(await offered()) === JSON.stringify(["Pending", "Superseded", "Canceled"]), `from Draft, the next steps: ${await offered()}`);
      await status.locator("[data-action=more-moves]").click();
      await eventually("more… offers every other move the store allows", async () =>
        JSON.stringify(await offered()) === JSON.stringify(["Pending", "Superseded", "Canceled", "Ready", "Coding", "In Review", "Blocked", "Complete", "Epic"]));
      await status.locator("[data-action=more-moves]").click();
      // Two clicks in one tick: one write.
      await status.locator("[data-to=Pending]").evaluate((b) => { b.click(); b.click(); });
      await eventually("the chip moves on the watch", async () => (await text(status.locator("[data-field=status]"))) === "Pending");
      await eventually("its answer", async () => (await text(said(status, "tickets.set_status", "answer"))) === "Draft → Pending");
      assert((await sent(page, "tickets.set_status")).length === 1, "one write for two clicks");
      // Canceled asks why first, and sends nothing until it is said.
      await status.locator("[data-to=Canceled]").click();
      await eventually("Canceled asks why", async () => (await status.locator("[data-ask=Canceled]").count()) === 1);
      await status.locator("[data-ask=Canceled] [data-confirm=Canceled]").click();
      await sleep(200);
      assert((await sent(page, "tickets.set_status")).length === 1, "nothing sent before the why");
      await status.locator("[data-ask=Canceled] [data-field=message]").fill("folded into another");
      await status.locator("[data-ask=Canceled] [data-confirm=Canceled]").click();
      await eventually("canceled", async () => get(id).status === "Canceled");
      const history = sy("tickets", "history", "--id", id).filter((e) => e.type === "transition").map((e) => e.transition);
      assert(history.some((h) => h.to === "Canceled" && h.message === "folded into another" && h.source === "app"), `the why and the app in the history: ${JSON.stringify(history.slice(-1))}`);
      for (const to of ["Pending", "Ready", "Pending", "Ready"]) {
        await eventually(`${to} offered`, async () => (await status.locator(`[data-to="${to}"]:not([disabled])`).count()) === 1);
        await status.locator(`[data-to="${to}"]`).click();
        await eventually(to, async () => get(id).status === to);
      }
      // A write is sent once: the socket dropped and back, nothing is sent again.
      await sleep(300);
      const before = (await sent(page, "tickets.set_status")).length;
      assert(before === 6, `six moves, six writes: ${before}`);
      await page.evaluate(() => window.__pages.sockets.forEach((s) => s.close()));
      await eventually("the page is back on a new socket", async () => (await page.evaluate(() => window.__pages.sockets.filter((s) => s.readyState === 1).length)) === 1, 15_000);
      await sleep(1_000);
      assert((await sent(page, "tickets.set_status")).length === before, "no write re-sent on the reconnect");
    } finally {
      await context.close();
    }
  });

  await test("leaving_canceled_restores_children", async () => {
    const top = child("canceled with its child", "--kind", "build");
    const kid = make("canceled along", "--parent", top);
    sy("tickets", "set_status", "--id", kid, "--status", "Ready");
    sy("tickets", "set_status", "--id", top, "--status", "Canceled", "--message", "for the case");
    assert(get(kid).status === "Canceled", "the cancel cascaded");
    const { context, page } = await openPage(`#/actions/${top}`);
    try {
      const status = page.locator("[data-surface=ticket-actions] [data-group=status]");
      await eventually("leaving Canceled offers the children back", async () => (await status.locator("[data-field=restore-children]").count()) === 1);
      await status.locator("[data-field=restore-children]").check();
      await status.locator("[data-to=Pending]").click();
      await eventually("its child back as it was", async () => get(top).status === "Pending" && get(kid).status === "Ready");
      assert((await sent(page, "tickets.set_status")).at(-1).params.restore_children === true, "restore_children asked");
    } finally {
      await context.close();
    }
  });

  await test("a_refusal_is_said_beside_the_action", async () => {
    const parent = child("a parent with open work", "--kind", "build");
    const kid = make("still open", "--parent", parent);
    sy("tickets", "set_status", "--id", parent, "--status", "In Review");
    const { context, page } = await openPage(`#/actions/${parent}`);
    try {
      const status = page.locator("[data-surface=ticket-actions] [data-group=status]");
      await eventually("Complete offered from In Review", async () => (await status.locator("[data-to=Complete]").count()) === 1);
      await status.locator("[data-to=Complete]").click();
      const refused = said(status, "tickets.set_status", "refused");
      await eventually("the refusal beside the moves, in the store's words", async () => /cannot be Complete/.test(await text(refused)) && (await text(refused)).includes(kid));
      assert((await status.locator("[data-to=Complete]").getAttribute("data-refused-on")) === "true", "the refused button is marked");
      assert(get(parent).status === "In Review" && (await text(status.locator("[data-field=status]"))) === "In Review", "nothing moved");
      sy("tickets", "set_status", "--id", kid, "--status", "Complete");
      await status.locator("[data-to=Complete]").click();
      await eventually("Complete once the child is", async () => get(parent).status === "Complete");
      await eventually("the refusal gives way to the answer", async () => (await refused.count()) === 0 && (await said(status, "tickets.set_status", "answer").count()) === 1);
    } finally {
      await context.close();
    }
  });

  await test("labels_add_and_remove", async () => {
    const id = child("labelled");
    const { context, page } = await openPage(`#/actions/${id}`);
    try {
      const labels = page.locator("[data-surface=ticket-actions] [data-group=labels]");
      const add = async (l) => {
        await labels.locator("[data-field=label]").fill(l);
        await labels.locator("[data-field=label]").press("Enter");
      };
      await eventually("the label field", async () => (await labels.locator("[data-field=label]").count()) === 1);
      await add("needs-you");
      await eventually("the chip", async () => (await labels.locator(".label[data-label=needs-you]").count()) === 1);
      assert(JSON.stringify(get(id).labels) === '["needs-you"]', "the store");
      await labels.locator("[data-action='tickets.label'][data-remove=needs-you]").click();
      await eventually("the chip goes", async () => (await labels.locator(".label[data-label=needs-you]").count()) === 0);
      await add("needs-you");
      await eventually("back at once", async () => (await labels.locator(".label[data-label=needs-you]").count()) === 1);
      await add("not a label!");
      await eventually("a label refused beside the field, in the store's words", async () => /label/.test(await text(said(labels, "tickets.label", "refused"))));
      assert(JSON.stringify(get(id).labels) === '["needs-you"]', "the bad label wrote nothing");
    } finally {
      await context.close();
    }
  });

  await test("archive_and_restore", async () => {
    const top = child("archived with its child", "--kind", "build");
    const kid = make("archived along", "--parent", top);
    const { context, page } = await openPage(`#/actions/${top}`);
    try {
      const acts = page.locator("[data-surface=ticket-actions]");
      const group = acts.locator("[data-group=archive]");
      // A message for the status line is the status line's, never the archive's.
      await eventually("drawn", async () => (await acts.locator("[data-group=status] [data-field=message]").count()) === 1);
      await acts.locator("[data-group=status] [data-field=message]").fill("for the status alone");
      await group.locator("[data-action='tickets.archive'][data-dry]").click();
      await eventually("the dry run counts the subtree", async () => (await text(group.locator("[data-dry-answer]"))).includes(kid));
      assert(!get(top).archived, "a dry run writes nothing");
      await group.locator("[data-field=message]").fill("first season fired");
      await group.locator("[data-action='tickets.archive']:not([data-dry])").click();
      await eventually("archived with its child", async () => !!get(top).archived && !!get(kid).archived);
      await eventually("said", async () => (await text(said(group, "tickets.archive", "answer"))).startsWith("archived"));
      const asked = (await sent(page, "tickets.archive")).at(-1);
      assert(asked.params.message === "first season fired", `the archive's own message: ${JSON.stringify(asked.params)}`);
      await eventually("restore offered", async () => (await group.locator("[data-archived]").count()) === 1);
      await group.locator("[data-action='tickets.restore']").click();
      await eventually("restored with its child", async () => !get(top).archived && !get(kid).archived);
      await eventually("archive… offered again", async () => (await group.locator("[data-action='tickets.archive'][data-dry]").count()) === 1);
    } finally {
      await context.close();
    }
  });

  await test("rename_dry_runs_then_moves", async () => {
    const parent = child("a parent of subtasks", "--kind", "build");
    const id = make("to become a subtask", "--parent", parent);
    const { context, page } = await openPage(`#/actions/${id}`);
    try {
      const group = page.locator("[data-surface=ticket-actions] [data-group=rename]");
      await eventually("drawn", async () => (await group.locator("[data-field=rename-to]").count()) === 1);
      await group.locator("[data-field=rename-to]").fill(`${root}.1`);
      await group.locator("[data-action='tickets.rename'][data-dry]").click();
      await eventually("outside its parent, refused in the store's words", async () => /is not/.test(await text(said(group, "tickets.rename", "refused"))));
      await group.locator("[data-field=rename-to]").fill(`${parent}.1`);
      await group.locator("[data-action='tickets.rename'][data-dry]").click();
      await eventually("the dry run says the move", async () => (await text(group.locator("[data-dry-answer]"))) === `would move ${id} → ${parent}.1`);
      assert(get(id).id === id, "a dry run writes nothing");
      await group.locator("[data-action='tickets.rename']:not([data-dry])").click();
      await eventually("moved, and the page follows it", async () => page.evaluate(() => location.hash).then((h) => ticketAt(h, `${parent}.1`)));
      assert(get(`${parent}.1`)?.title === "to become a subtask", "the store has it under its new id");
    } finally {
      await context.close();
    }
  });

  await test("reshortcode_dry_runs_then_moves", async () => {
    const target = scopeOf("WRITX", "where it moves");
    const id = child("to move shortcodes");
    const { context, page } = await openPage(`#/actions/${id}`);
    try {
      const admin = page.locator("[data-surface=ticket-actions] [data-group=admin]");
      await eventually("reshortcode… offered to the admin", async () => (await admin.locator("[data-action='tickets.reshortcode'][data-open]:not([disabled])").count()) === 1);
      await admin.locator("[data-action='tickets.reshortcode'][data-open]").click();
      await admin.locator("[data-field=reshortcode-to]").fill("WRITX");
      await admin.locator("[data-action='tickets.reshortcode'][data-dry]").click();
      await eventually("the dry run names the new id", async () => /^would move .* → WRITX-\d+$/.test(await text(admin.locator("[data-dry-answer]"))));
      const to = (await text(admin.locator("[data-dry-answer]"))).split(" → ")[1];
      await admin.locator("[data-action='tickets.reshortcode']:not([data-dry]):not([data-open])").click();
      await eventually("moved, the page follows it", async () => page.evaluate(() => location.hash).then((h) => ticketAt(h, to)));
      assert(get(to)?.title === "to move shortcodes" && get(to).parent === target, `the store: ${JSON.stringify(get(to)?.parent)}`);
    } finally {
      await context.close();
    }
  });

  await test("a_revision_is_put_back", async () => {
    const id = child("the first title");
    sy("tickets", "amend", "--id", id, "--title", "a second title");
    const revs = sy("tickets", "revisions", "list", "--id", id).filter((e) => e.type === "revision").map((e) => e.revision);
    const first = Math.min(...revs.map((r) => r.rev));
    const { context, page } = await openPage(`#/actions/${id}`);
    try {
      const group = page.locator("[data-surface=ticket-actions] [data-group=revision]");
      await eventually("a rev to type", async () => (await group.locator("[data-field=rev]").count()) === 1);
      await group.locator("[data-field=rev]").fill(String(first));
      await group.locator("[data-field=rev]").press("Enter");
      await eventually("the rev picked", async () => (await text(group.locator("[data-field=revision]"))).includes(`rev ${first}`));
      await group.locator("[data-rf=title]").check();
      await group.locator("[data-field=message]").fill("the first was right");
      await group.locator("[data-action='tickets.revisions.restore']").click();
      await eventually("the title is back", async () => get(id).title === "the first title");
      await eventually("said", async () => (await text(said(group, "tickets.revisions.restore", "answer"))) === `from rev ${first}: title`);
      const p = (await sent(page, "tickets.revisions.restore")).at(-1).params;
      assert(JSON.stringify(p.fields) === '["title"]' && p.message === "the first was right", `its fields and its message: ${JSON.stringify(p)}`);
    } finally {
      await context.close();
    }
  });

  // SURFACE-22: the group lists the revisions itself — a row picks one, and one made while the page is open is listed.
  await test("a_revision_is_picked_from_its_list", async () => {
    const id = child("the listed title");
    sy("tickets", "update", "--id", id, "--title", "a later title", "--message", "kept as its own");
    const revs = sy("tickets", "revisions", "list", "--id", id).filter((e) => e.type === "revision").map((e) => e.revision.rev);
    const first = Math.min(...revs);
    const { context, page } = await openPage(`#/actions/${id}`);
    try {
      const group = page.locator("[data-surface=ticket-actions] [data-group=revision]");
      await eventually(`every revision listed (${revs})`, async () => (await group.locator("[data-pick-rev]").count()) === Math.min(revs.length, 5));
      const order = await group.locator("[data-pick-rev]").evaluateAll((els) => els.map((e) => Number(e.dataset.pickRev)));
      assert(JSON.stringify(order) === JSON.stringify([...order].sort((a, b) => b - a)), `newest first: ${order}`);
      assert((await group.locator("[data-field=rev]").count()) === 1, "a rev can still be typed");
      // One made while the page is open: its row appears, on top.
      sy("tickets", "update", "--id", id, "--title", "a third title", "--message", "kept as its own too");
      const top = Math.max(...sy("tickets", "revisions", "list", "--id", id).filter((e) => e.type === "revision").map((e) => e.revision.rev));
      await eventually("the new revision is listed live", async () => (await group.locator(`[data-pick-rev="${top}"]`).count()) === 1, 5_000);
      // A row picks it; restore sends that rev.
      await group.locator(`[data-pick-rev="${first}"]`).click();
      await eventually("the rev picked", async () => (await text(group.locator("[data-field=revision]"))).includes(`rev ${first}`));
      assert((await group.locator(`[data-pick-rev="${first}"]`).getAttribute("aria-pressed")) === "true", "its row is marked");
      if (process.env.PAGES_SHOTS) await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, "ticket-actions-revisions.png"), fullPage: true });
      await group.locator("[data-rf=title]").check();
      await group.locator("[data-field=message]").fill("the first was right");
      await group.locator("[data-action='tickets.revisions.restore']").click();
      await eventually("the title is back", async () => get(id).title === "the listed title");
      const p = (await sent(page, "tickets.revisions.restore")).at(-1).params;
      assert(p.rev === first && JSON.stringify(p.fields) === '["title"]', `the rev its row named: ${JSON.stringify(p)}`);
    } finally {
      await context.close();
    }
  });

  await test("freeze_and_unfreeze_a_scope", async () => {
    const scope = scopeOf("WRITF", "a scope to freeze");
    const { context, page } = await openPage(`#/actions/${scope}`);
    try {
      const admin = page.locator("[data-surface=ticket-actions] [data-group=admin]");
      await eventually("freeze offered", async () => (await admin.locator("[data-action='tickets.freeze']:not([disabled])").count()) === 1);
      await admin.locator("[data-action='tickets.freeze']").click();
      await eventually("frozen, and the page says so", async () => get(scope).frozen === true && (await admin.locator("[data-frozen=true]").count()) === 1);
      await admin.locator("[data-action='tickets.unfreeze']").click();
      await eventually("growing again", async () => !get(scope).frozen && (await admin.locator("[data-action='tickets.freeze']").count()) === 1);
    } finally {
      await context.close();
    }
  });

  await test("delete_undelete_as_admin", async () => {
    const top = child("deleted with a child", "--kind", "build");
    const kid = make("its child", "--parent", top);
    const waiter = child("waits on the deleted one");
    sy("tickets", "update", "--id", waiter, "--block", top);
    const { context, page } = await openPage(`#/actions/${top}`);
    try {
      const acts = page.locator("[data-surface=ticket-actions]");
      const admin = acts.locator("[data-group=admin]");
      await eventually("delete… offered to the admin", async () => (await admin.locator("[data-action='tickets.delete'][data-open]:not([disabled])").count()) === 1);
      assert(await admin.locator("[data-field=children][data-children=reparent]").isChecked(), "its children go to its parent by default");
      // Children go too, the waits kept: refused, naming the waiter.
      await admin.locator("[data-field=children][data-children=delete]").check();
      await admin.locator("[data-field=unblock]").uncheck();
      await admin.locator("[data-action='tickets.delete'][data-open]").click();
      await admin.locator("[data-action='tickets.delete'][data-confirm]").click();
      await eventually("refused while one waits on it, in the store's words", async () => (await text(said(admin, "tickets.delete", "refused"))).includes(waiter));
      assert(get(top) && get(kid), "nothing deleted");
      await admin.locator("[data-field=unblock]").check();
      await admin.locator("[data-action='tickets.delete'][data-open]").click();
      await admin.locator("[data-action='tickets.delete'][data-confirm]").click();
      await eventually("gone with its child, the wait dropped", async () => !get(top) && !get(kid) && get(waiter).blocked_by.length === 0);
      const p = (await sent(page, "tickets.delete")).at(-1).params;
      assert(p.children === "delete" && p.unblock === true, `the choices asked: ${JSON.stringify(p)}`);
      await eventually("the page says it is gone and offers undelete", async () => (await acts.locator("[data-group=undelete] [data-action='tickets.undelete']").count()) === 1);
      await acts.locator("[data-action='tickets.undelete']").click();
      await eventually("back in the store", async () => !!get(top));
      await eventually("undelete said", async () => (await text(said(acts, "tickets.undelete", "answer"))) === `back: ${top}`);
      await eventually("the page draws it again from the watch", async () => (await acts.locator("[data-group=status]").count()) === 1);
    } finally {
      await context.close();
    }
  });

  await test("a_viewer_sees_admin_actions_disabled", async () => {
    const scope = scopeOf("WRITV", "a scope a viewer looks at");
    // An editor: the lifecycle open, the admin's actions disabled, naming the role.
    {
      const { page, done } = await as("editor", `#/actions/${scope}`);
      try {
        const acts = page.locator("[data-surface=ticket-actions]");
        await eventually("the editor's moves are live", async () => (await acts.locator("[data-group=status] [data-to=Pending]:not([disabled])").count()) === 1);
        await eventually("delete disabled for the editor, needing admin", async () =>
          (await acts.locator("[data-action='tickets.delete'][disabled]").count()) === 1 && (await text(acts.locator("[data-needs='tickets.delete']"))) === "admin · you hold editor here");
        assert((await acts.locator("[data-action='tickets.freeze'][disabled]").count()) === 1 && (await text(acts.locator("[data-needs='tickets.freeze']"))) === "admin · you hold editor here", "freeze disabled, needing admin");
        await acts.locator("[data-group=rename] [data-field=rename-to]").fill(`${scope}.1`);
        assert((await acts.locator("[data-group=rename] [data-action='tickets.rename'][data-dry]:not([disabled])").count()) === 1, "rename is the editor's");
        assert((await text(acts.locator("[data-field=who]"))).includes("editor"), "who is asking, said");
        if (process.env.PAGES_SHOTS) await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, "ticket-actions-editor.png"), fullPage: true });
        await acts.locator("[data-group=status] [data-to=Pending]").click();
        await eventually("the editor's move lands", async () => get(scope).status === "Pending");
      } finally {
        await done();
      }
    }
    // A viewer: every write disabled, the roles named.
    {
      const { page, done } = await as("viewer", `#/actions/${scope}`);
      try {
        const acts = page.locator("[data-surface=ticket-actions]");
        await eventually("the viewer's moves are disabled, naming the roles", async () =>
          (await acts.locator("[data-group=status] [data-to]:not([disabled])").count()) === 0 && (await acts.locator("[data-group=status] [data-to][disabled]").count()) > 0
          && (await text(acts.locator("[data-group=status] [data-needs]"))) === "editor or admin · you hold viewer here");
        assert((await acts.locator("[data-action='tickets.delete'][disabled]").count()) === 1, "delete disabled for the viewer");
        assert((await acts.locator("[data-group=labels] [data-field=label]").count()) === 0, "no label field for the viewer");
        assert((await sent(page, "tickets.set_status")).length === 0, "the viewer's page asked no write");
      } finally {
        await done();
      }
    }
    // A viewer everywhere who edits under one ticket alone: judged on each ticket's scope.
    {
      const other = child("another subtree");
      const keyFile = path.join(server.work, "scoped-writes.key");
      const principal = makeKey(keyFile);
      sy("members", "grant", "--principal", principal, "--scope", "*", "--role", "viewer");
      sy("members", "grant", "--principal", principal, "--scope", `under ${other}`, "--role", "editor");
      const file = path.join(server.work, "scoped-writes.session");
      await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, file, "scoped");
      const { context, page: first } = await openPage("");
      await first.close();
      await context.clearCookies();
      await context.addCookies(sessionCookies(server.origin, readToken(file)));
      const page = await context.newPage();
      try {
        await page.goto(`${server.origin}/app/#/actions/${other}`);
        const acts = page.locator("[data-surface=ticket-actions]");
        await eventually("an editor under it may move it", async () => (await acts.locator("[data-group=status] [data-to=Pending]:not([disabled])").count()) === 1);
        await page.evaluate((s) => (location.hash = `#/actions/${s}`), root);
        await eventually("not elsewhere: the writes disabled, the viewer's role said", async () =>
          (await acts.locator("[data-group=status] [data-to]:not([disabled])").count()) === 0 && (await text(acts.locator("[data-group=status] [data-needs]"))) === "editor or admin · you hold viewer here");
      } finally {
        await context.close();
        sy("members", "revoke", "--principal", principal, "--scope", `under ${other}`);
        sy("members", "revoke", "--principal", principal, "--scope", "*");
      }
    }
  });
};
