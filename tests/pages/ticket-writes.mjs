// SURFACE-4: the ticket page's writes — ticket-edit and ticket-actions —
// on the throwaway, through their test routes (#/edit/:id → ticket-edit,
// #/actions/:id → ticket-actions) until SURFACE-3's ticket page places them.
//
// Marks: ticket-edit `[data-surface=ticket-edit]`, `[data-action=edit|cancel|tickets.update|tickets.amend]`,
// `[data-form=tickets.update|tickets.amend]`, `[data-field=title|kind|severity|hidden|parent|blocked_by|block|body|preview]`,
// `[data-kind-pick]`, `[data-sev-pick]`, `[data-unblock]`; ticket-actions `[data-surface=ticket-actions]`,
// `[data-group=status|labels|archive|rename|revision|admin|undelete]`, `[data-action=tickets.<method>]`
// (`[data-to=<status>]` on a move, `[data-confirm]`, `[data-dry]`, `[data-open]`, `[data-remove]`),
// `[data-field=status|message|label|labels|with-children|rename-to|rev|children|unblock]`, `[data-ask]`,
// `[data-needs]`, `[data-archived]`, `[data-dry-answer]`; both: `[data-for=<method>]` on an answer —
// `[data-answer]`, `[data-refused]` — `[data-loading]`, `[data-empty]`.
//
// FRAME-37: a write's answer is `await bus.call(…)` — `{ ok, events, message }` once FRAME-37 lands,
// the events (or the refusal thrown) before; the components read either. The answers and refusals
// said beside an action are checked unless FRAME37=0; every case checks what the write moved in the
// store and on the watch.
import { makeKey, readToken, sessionCookies, writeSession } from "../../scripts/throwaway-login.mjs";
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

const ANSWERS = process.env.FRAME37 !== "0"; // FRAME-37: bus.call answers; FRAME37=0 skips the answers' cases

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, server, path } = ctx;
  const EPIC = "WRIT";
  const get = (id) => sy("tickets", "get", "--id", id).find((e) => e.type === "ticket")?.ticket;
  const make = (title, ...flags) => {
    const [c] = sy("tickets", "create", "--epic", EPIC, "--title", title, ...flags);
    if (c?.type !== "created") throw new Error(`create ${title}: ${JSON.stringify(c)}`);
    return c.ticket.id;
  };
  const root = make("the writes' shortcode", "--kind", "scope");
  const child = (title, ...flags) => make(title, "--parent", root, ...flags);
  const said = (loc, method, kind) => loc.locator(`[data-for="${method}"][data-${kind}]`);
  const text = async (loc) => (await loc.first().textContent({ timeout: 1000 })).trim();
  const waits = (what) => console.log(`     skip ${what}: waits on FRAME-37`);
  /** The frames the page sent for `method`. */
  const sent = async (page, method) => (await sentCalls(page)).filter((c) => c.method === method);
  /** A session of a fresh key granted `role` on *, and a page signed in as it. */
  const as = async (role, route) => {
    const keyFile = path.join(server.work, `${role}-writes.key`);
    const principal = makeKey(keyFile);
    const g = sy("members", "grant", "--principal", principal, "--scope", "*", "--role", role);
    assert(g.some((e) => e.type === "grant"), `${role} granted: ${JSON.stringify(g)}`);
    const file = path.join(server.work, `${role}-writes.session`);
    await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, file, role);
    const { context, page } = await openPage("", { signedIn: false });
    await context.clearCookies();
    await context.addCookies(sessionCookies(server.origin, readToken(file)));
    await page.goto(`${server.origin}/app/${route}`);
    return { context, page };
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
    const { context, page } = await openPage(`#/edit/${id}`);
    try {
      const edit = await opened(page);
      const form = edit.locator("[data-form='tickets.amend']");
      assert(await form.locator("[data-action='tickets.amend']").isDisabled(), "save body waits for an edit");
      await form.locator("textarea[data-field=body]").fill("# The heading\n\nsome *words* and a [link](https://example.com)\n\n<script>window.__pwned = 1</script>");
      await eventually("the preview draws the markdown", async () => (await form.locator("[data-preview] h1").textContent({ timeout: 500 })) === "The heading");
      assert((await form.locator("[data-preview] em").textContent()) === "words", "the preview's emphasis");
      assert((await form.locator("[data-preview] script").count()) === 0, "the preview draws no script");
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
      await form.locator("[data-action='tickets.amend']").click();
      await eventually("the store holds the body", async () => get(id).body.startsWith("# The heading"));
      await eventually("the watch brings it back: save body waits for the next edit", async () => form.locator("[data-action='tickets.amend']").isDisabled());
      if (ANSWERS) await eventually("the amend's answer under the body", async () => (await text(said(form, "tickets.amend", "answer"))) === "saved the body");
      else waits("the amend's answer under the body");
      assert((await page.evaluate(() => window.__pwned)) === undefined, "no script ran");
      await sleep(300);
      const amends = await sent(page, "tickets.amend");
      assert(amends.length === 1 && amends[0].params.id === id, `one amend sent: ${JSON.stringify(amends)}`);
      assert((await sent(page, "tickets.update")).length === 0, "the body alone: no update sent");
      await page.evaluate((id) => (location.hash = `#/ticket/${id}`), id);
      await eventually("the ticket page's body is the new one", async () => (await page.locator("[data-ticket-body] h1").textContent({ timeout: 500 })) === "The heading");
    } finally {
      await context.close();
    }
  });

  await test("update_changes_severity_and_blockers", async () => {
    const a = child("waits on b");
    const b = child("b, waited on");
    const { context, page } = await openPage(`#/edit/${a}`);
    try {
      const edit = await opened(page);
      const form = edit.locator("[data-form='tickets.update']");
      const save = form.locator("[data-action='tickets.update']");
      assert(await save.isDisabled(), "save fields waits for a change");
      await form.locator("[data-sev-pick=High]").click();
      await form.locator("[data-field=block]").fill(b);
      await form.locator("[data-field=block]").press("Enter");
      await eventually("the blocker is a chip", async () => (await form.locator(`[data-field=blocked_by] [data-blocker="${b}"]`).count()) === 1);
      await form.locator("[data-field=title]").fill("waits on b, retitled");
      await eventually("the button counts the fields", async () => (await text(save)) === "save 3 fields");
      await save.click();
      await eventually("the store moved", async () => { const t = get(a); return t.severity === "High" && t.blocked_by.join() === b && t.title === "waits on b, retitled"; });
      const updates = await sent(page, "tickets.update");
      assert(updates.length === 1 && updates[0].params.source === "app" && updates[0].params.kind === undefined, `one update, from the app: ${JSON.stringify(updates)}`);
      if (ANSWERS) await eventually("the update's answer", async () => /saved .*severity/.test(await text(said(form, "tickets.update", "answer"))));
      else waits("the update's answer");
      // Cleared: severity none, the blocker's ×.
      await form.locator("[data-sev-pick=none]").click();
      await form.locator(`[data-unblock="${b}"]`).click();
      await save.click();
      await eventually("cleared in the store", async () => { const t = get(a); return t.severity == null && t.blocked_by.length === 0; });
      // A cycle: b waiting on a while a waits on b.
      sy("tickets", "update", "--id", b, "--block", a);
      await form.locator("[data-field=block]").fill(b);
      await form.locator("[data-field=block]").press("Enter");
      await save.click();
      await eventually("the update was asked", async () => (await sent(page, "tickets.update")).length === 3);
      await sleep(300);
      assert(get(a).blocked_by.length === 0, "the cycle wrote nothing");
      if (ANSWERS) await eventually("the cycle refused under the form, in the store's words", async () => /cycle/.test(await text(said(form, "tickets.update", "refused"))));
      else waits("the cycle's refusal under the form");
      await form.locator(`[data-unblock="${b}"]`).click();
      // The kind, by set_kind; doc refused for a ticket that waits or is waited on.
      await form.locator("[data-kind-pick=gap]").click();
      await save.click();
      await eventually("the kind moved", async () => get(a).kind === "gap");
      assert((await sent(page, "tickets.set_kind")).length === 1, "one set_kind");
      if (ANSWERS) {
        await form.locator("[data-kind-pick=doc]").click();
        sy("tickets", "update", "--id", a, "--block", root);
        await save.click();
        await eventually("doc refused under the kind", async () => /doc/.test(await text(said(form, "tickets.set_kind", "refused"))));
      } else waits("the kind's refusal under the kind");
    } finally {
      await context.close();
    }
  });

  await test("set_parent_by_an_id_typed", async () => {
    const other = child("another parent", "--kind", "build");
    const moved = child("to be moved");
    const { context, page } = await openPage(`#/edit/${moved}`);
    try {
      const edit = await opened(page);
      // The picker is SURFACE-5's mark (it is empty until that lands); the id is typed.
      if (ANSWERS) {
        await edit.locator("[data-field=parent]").fill(`${EPIC}-99999`);
        await edit.locator("[data-action='tickets.update']").click();
        await eventually("a parent that is not there, refused under the field", async () => /no ticket/.test(await text(said(edit, "tickets.set_parent", "refused"))));
      } else waits("an unknown parent's refusal under the field");
      await edit.locator("[data-field=parent]").fill(other);
      await edit.locator("[data-action='tickets.update']").click();
      await eventually("the store moved it", async () => get(moved).parent === other);
      assert((await sent(page, "tickets.set_parent")).length === (ANSWERS ? 2 : 1), "set_parent asked");
      assert((await sent(page, "tickets.update")).length === 0, "the parent alone: no update");
    } finally {
      await context.close();
    }
  });

  // FRAME-36: the form's keys — ctrl+enter saves (in a field too, never a newline in the body), escape cancels.
  await test("the_edit_forms_keys", async () => {
    const id = child("saved by a key");
    const { context, page } = await openPage(`#/edit/${id}`);
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
      assert(JSON.stringify(await offered()) === JSON.stringify(["Pending", "Superseded", "Canceled"]), `from Draft: ${await offered()}`);
      await status.locator("[data-to=Pending]").click();
      await eventually("the chip moves on the watch", async () => (await text(status.locator("[data-field=status]"))) === "Pending");
      await eventually("and the moves with it", async () => JSON.stringify(await offered()) === JSON.stringify(["Ready", "Draft", "Superseded", "Canceled"]));
      assert(get(id).status === "Pending", "the store");
      if (ANSWERS) await eventually("its answer", async () => (await text(said(status, "tickets.set_status", "answer"))) === "Draft → Pending");
      else waits("the move's answer");
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
      // Back and forth at once: every move is its own write.
      for (const to of ["Pending", "Ready", "Pending", "Ready"]) {
        await eventually(`${to} offered`, async () => (await status.locator(`[data-to="${to}"]`).count()) === 1);
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
      const after = (await sent(page, "tickets.set_status")).length;
      assert(after === before, `no write re-sent on the reconnect: ${before} → ${after}`);
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
      await eventually("the move was asked", async () => (await sent(page, "tickets.set_status")).length === 1);
      await sleep(300);
      assert(get(parent).status === "In Review", "the lifecycle refused it: nothing moved");
      assert((await text(status.locator("[data-field=status]"))) === "In Review", "the chip stays");
      if (!ANSWERS) return waits("a_refusal_is_said_beside_the_action's words beside the button");
      const refused = said(status, "tickets.set_status", "refused");
      await eventually("the refusal beside the moves, in the store's words", async () => /cannot be Complete/.test(await text(refused)) && (await text(refused)).includes(kid));
      assert((await status.locator("[data-to=Complete]").getAttribute("data-refused-on")) === "true", "the refused button is marked");
      sy("tickets", "set_status", "--id", kid, "--status", "Complete");
      await status.locator("[data-to=Complete]").click();
      await eventually("Complete once the child is", async () => get(parent).status === "Complete");
      await eventually("the refusal gives way to the answer", async () => (await refused.count()) === 0);
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
      await eventually("the bad label asked", async () => (await sent(page, "tickets.label")).length === 4);
      await sleep(300);
      assert(JSON.stringify(get(id).labels) === '["needs-you"]', "the bad label wrote nothing");
      if (ANSWERS) await eventually("a label refused beside the field", async () => (await said(labels, "tickets.label", "refused").count()) === 1);
      else waits("the bad label's refusal beside the field");
    } finally {
      await context.close();
    }
  });

  await test("archive_and_restore", async () => {
    const top = child("archived with its child", "--kind", "build");
    const kid = make("archived along", "--parent", top);
    const { context, page } = await openPage(`#/actions/${top}`);
    try {
      const group = page.locator("[data-surface=ticket-actions] [data-group=archive]");
      await eventually("archive… offered", async () => (await group.locator("[data-action='tickets.archive'][data-dry]").count()) === 1);
      await group.locator("[data-action='tickets.archive'][data-dry]").click();
      await eventually("the dry run asked, nothing written", async () => (await sent(page, "tickets.archive")).some((c) => c.params.dry_run === true));
      assert(!get(top).archived, "a dry run writes nothing");
      await eventually("its line", async () => (await group.locator("[data-dry-answer='tickets.archive']").count()) === 1);
      if (ANSWERS) await eventually("the dry run counts the subtree", async () => (await text(group.locator("[data-dry-answer]"))).includes(kid));
      else waits("the dry run's count");
      await group.locator("[data-action='tickets.archive']:not([data-dry])").click();
      await eventually("archived with its child", async () => !!get(top).archived && !!get(kid).archived);
      await eventually("restore offered", async () => (await group.locator("[data-archived]").count()) === 1);
      await group.locator("[data-action='tickets.restore']").click();
      await eventually("restored with its child", async () => !get(top).archived && !get(kid).archived);
      await eventually("archive… offered again", async () => (await group.locator("[data-action='tickets.archive'][data-dry]").count()) === 1);
    } finally {
      await context.close();
    }
  });

  await test("delete_undelete_as_admin", async () => {
    const top = child("deleted with a child", "--kind", "build");
    const kid = make("its child", "--parent", top);
    const { context, page } = await openPage(`#/actions/${top}`);
    try {
      const acts = page.locator("[data-surface=ticket-actions]");
      const admin = acts.locator("[data-group=admin]");
      await eventually("delete… offered to the admin", async () => (await admin.locator("[data-action='tickets.delete'][data-open]:not([disabled])").count()) === 1);
      await admin.locator("[data-action='tickets.delete'][data-open]").click();
      assert(await admin.locator("[data-field=children][data-children=reparent]").isChecked(), "its children go to its parent by default");
      await admin.locator("[data-action='tickets.delete'][data-confirm]").click();
      await eventually("gone, its child moved up", async () => !get(top) && get(kid).parent === root);
      await eventually("the page says it is gone and offers undelete", async () => (await acts.locator("[data-group=undelete] [data-action='tickets.undelete']").count()) === 1);
      await acts.locator("[data-action='tickets.undelete']").click();
      await eventually("back in the store", async () => !!get(top));
      if (ANSWERS) await eventually("undelete said", async () => (await text(said(acts, "tickets.undelete", "answer"))) === `back: ${top}`);
      else waits("the undelete's answer");
    } finally {
      await context.close();
    }
  });

  await test("a_viewer_sees_admin_actions_disabled", async () => {
    const scope = sy("tickets", "create", "--epic", "WRITV", "--kind", "scope", "--parent", root, "--title", "a scope a viewer looks at").find((e) => e.type === "created").ticket.id;
    // An editor: the lifecycle open, the admin's actions disabled, naming the role.
    {
      const { context, page } = await as("editor", `#/actions/${scope}`);
      try {
        const acts = page.locator("[data-surface=ticket-actions]");
        await eventually("the editor's moves are live", async () => (await acts.locator("[data-group=status] [data-to=Pending]:not([disabled])").count()) === 1);
        await eventually("delete disabled for the editor, needing admin", async () =>
          (await acts.locator("[data-action='tickets.delete'][disabled]").count()) === 1 && (await text(acts.locator("[data-needs='tickets.delete']"))) === "admin · you hold editor");
        assert((await acts.locator("[data-action='tickets.freeze'][disabled]").count()) === 1 && (await text(acts.locator("[data-needs='tickets.freeze']"))) === "admin · you hold editor", "freeze disabled, needing admin");
        await acts.locator("[data-group=rename] [data-field=rename-to]").fill(`${scope}.1`);
        assert((await acts.locator("[data-group=rename] [data-action='tickets.rename'][data-dry]:not([disabled])").count()) === 1, "rename is the editor's");
        assert((await text(acts.locator("[data-field=who]"))).includes("editor"), "who is asking, said");
        if (process.env.PAGES_SHOTS) await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, "ticket-actions-editor.png"), fullPage: true });
        await acts.locator("[data-group=status] [data-to=Pending]").click();
        await eventually("the editor's move lands", async () => get(scope).status === "Pending");
      } finally {
        await context.close();
      }
    }
    // A viewer: every write disabled, the roles named.
    {
      const { context, page } = await as("viewer", `#/actions/${scope}`);
      try {
        const acts = page.locator("[data-surface=ticket-actions]");
        await eventually("the viewer's moves are disabled, naming the roles", async () =>
          (await acts.locator("[data-group=status] [data-to]:not([disabled])").count()) === 0 && (await acts.locator("[data-group=status] [data-to][disabled]").count()) > 0
          && (await text(acts.locator("[data-group=status] [data-needs]"))) === "editor or admin · you hold viewer");
        assert((await acts.locator("[data-action='tickets.delete'][disabled]").count()) === 1, "delete disabled for the viewer");
        assert((await acts.locator("[data-group=labels] [data-field=label]").count()) === 0, "no label field for the viewer");
        assert((await sent(page, "tickets.set_status")).length === 0, "the viewer's page asked no write");
      } finally {
        await context.close();
      }
    }
  });
};
