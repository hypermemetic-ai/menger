// SURFACE-9: the admin pages — epics (#/epics), organizations (#/orgs), who
// I am (#/me) and the releases (#/releases). Run alone by
// `node tests/pages_browser.mjs --group admin`.
//
// Marks: every page's root `[data-surface=epics-page|orgs-list|me-page|ui-versions]`;
// rows `[data-id]` (an epic's name, an organization's name, a grant's `role@scope`,
// a team's name, a provider's name, a version's hash); fields `[data-field=…]`;
// actions `[data-action=epics.switch|epics.get|epics.dismiss|auth.requires|
// session.logout|ui.rollback|ui.preview|ui.tag|ui.undo|ui.redo]`; FRAME-52: a
// row's `[data-field=label]` and `[data-tag]`s, its `[data-tag-editor]`; the
// history's `[data-move][data-how]`; forms `[data-form=epics.list|
// auth.requires]`; a refusal `[data-refused]`; `[data-empty]`.
//
// The owner's session (`token`) is an admin on the throwaway; every case
// leaves the owner where it found it — on genesis, nothing previewed, v2
// live, no section set — so the groups after this one see the board as the
// runner made it.
import path from "node:path";
import { makeKey, sessionCookies, writeSession } from "../../scripts/throwaway-login.mjs";
import { bundle, push, synapse } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, openPage, sy, server, session, v1, v2, fs } = ctx;
  const work = path.dirname(session);
  const text = async (loc) => String(await loc.first().textContent({ timeout: 1000 })).trim();
  const ok = (events, type) => events.find((e) => e.type === type);
  /** A session of another key: `{ file, token, principal }`, `role` granted on `*` when named. */
  async function another(name, role) {
    const keyFile = path.join(work, `${name}.key`);
    const principal = makeKey(keyFile);
    if (role) {
      const g = sy("members", "grant", "--principal", principal, "--scope", "*", "--role", role);
      assert(ok(g, "grant"), `${role} granted to ${name}: ${JSON.stringify(g)}`);
    }
    const file = path.join(work, `${name}.session`);
    const s = await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, file, name);
    return { file, token: s.token, principal };
  }
  /** A page signed in with `token` rather than the owner's; `sent` every frame its sockets send. */
  async function openAs(token, route) {
    const { context, page, seen } = await openPage("", { signedIn: false });
    await context.clearCookies();
    await context.addCookies(sessionCookies(server.origin, token));
    const sent = [];
    page.on("websocket", (ws) => ws.on("framesent", (f) => sent.push(String(f.payload))));
    await page.goto(`${server.origin}/app/${route}`);
    return { context, page, seen, sent };
  }
  /** Revokes `principal`'s grant on `scope`. */
  const revoke = (principal, scope) => sy("members", "revoke", "--principal", principal, "--scope", scope);

  await test("epics_list_switch_and_the_top_follows", async () => {
    for (const n of ["s9-spring", "s9-summer"]) sy("epics", "create", "--name", n);
    const { page, context } = await openPage("#/epics");
    const root = page.locator("[data-surface=epics-page]");
    try {
      await eventually("the epics are listed", async () => (await root.locator("[data-id=genesis]").count()) === 1 && (await root.locator("[data-id=s9-spring]").count()) === 1);
      await eventually("the current epic is genesis", async () => (await text(root.locator("[data-field=current]"))) === "genesis");
      assert((await root.locator("[data-id=genesis][data-current=true]").count()) === 1, "genesis's row is marked current");
      const tickets = ok(sy("epics", "get", "--name", "genesis"), "epic").epic.tickets;
      assert(Number(await text(root.locator("[data-id=genesis] [data-field=tickets]"))) === tickets, `genesis's row says its ${tickets} tickets`);

      // switch: the answer is the top's epic, and the server took it.
      await root.locator("[data-id=s9-spring] [data-action='epics.switch']").click();
      await eventually("the top says s9-spring", async () => (await text(root.locator("[data-field=current]"))) === "s9-spring");
      await eventually("the row is current", async () => (await root.locator("[data-id=s9-spring][data-current=true]").count()) === 1);
      await eventually("epics.current answers s9-spring", async () => ok(sy("epics", "current"), "current").epic.name === "s9-spring");
      // The app's top follows on the board when SURFACE-2's app-top is there.
      const top = await openPage("#/");
      try {
        const field = top.page.locator("[data-surface=app-top] [data-field=epic]");
        await top.page.waitForTimeout(1500);
        if ((await field.count()) === 0) console.log("     skip the board's top: no app-top in this bundle (SURFACE-2)");
        else await eventually("the board's top says s9-spring", async () => (await text(field)).includes("s9-spring"));
      } finally {
        await top.context.close();
      }

      // A refused switch says why beside the action: an epic deleted under the page.
      sy("epics", "create", "--name", "s9-gone");
      await eventually("s9-gone appears live (epics.watch)", async () => (await root.locator("[data-id=s9-gone]").count()) === 1);
      assert((await root.locator("[data-id=s9-gone][data-changed=created]").count()) === 1, "a row the watch brought says what changed");
      sy("epics", "delete", "--name", "s9-gone");
      await eventually("the deletion is said on its row", async () => (await root.locator("[data-id=s9-gone][data-changed=deleted]").count()) === 1);
      assert((await root.locator("[data-id=s9-gone] [data-action='epics.switch']").count()) === 0, "a deleted epic offers no switch");
      // A refused switch says why beside the action: a second page whose socket
      // never hears the deletion (the watch's frame dropped on its way in) still
      // offers s9-gone, and the switch's answer is not_found.
      sy("epics", "create", "--name", "s9-gone2");
      const deaf = await context.newPage();
      await deaf.routeWebSocket(/.*/, (ws) => {
        const up = ws.connectToServer();
        ws.onMessage((m) => up.send(m));
        up.onMessage((m) => { if (!(String(m).includes("epic_changed") && String(m).includes("s9-gone2"))) ws.send(m); });
      });
      await deaf.goto(`${server.origin}/app/#/epics`);
      const d = deaf.locator("[data-surface=epics-page]");
      await eventually("the deaf page lists s9-gone2", async () => (await d.locator("[data-id=s9-gone2] [data-action='epics.switch']").count()) === 1);
      sy("epics", "delete", "--name", "s9-gone2");
      await d.locator("[data-id=s9-gone2] [data-action='epics.switch']").click();
      await eventually("the refusal is said beside it", async () => (await text(d.locator("[data-id=s9-gone2] [data-refused]"))).includes("s9-gone2"));
      await eventually("a refused switch leaves the top where it was", async () => (await text(d.locator("[data-field=current]"))) === "s9-spring");
      assert(ok(sy("epics", "current"), "current").epic.name === "s9-spring", "the server stayed on s9-spring");
      await deaf.close();

      // get: one epic's card.
      await root.locator("[data-id=s9-summer] [data-action='epics.get']").click();
      await eventually("the card is s9-summer", async () => (await text(root.locator("[data-field=detail] [data-field=name]"))) === "s9-summer");

      // An archive moves the owner (on s9-spring) to s9-summer: the notice, then dismiss.
      const a = sy("epics", "archive", "--name", "s9-spring");
      assert(ok(a, "archived")?.to === "s9-summer", `s9-spring archived, its people to s9-summer: ${JSON.stringify(a)}`);
      await eventually("the move is said", async () => (await text(root.locator("[data-field=moved]"))).includes("s9-summer"));
      await eventually("the top follows the move", async () => (await text(root.locator("[data-field=current]"))) === "s9-summer");
      await root.locator("[data-action='epics.dismiss']").click();
      await eventually("the notice is gone", async () => (await root.locator("[data-field=moved]").count()) === 0);
      await eventually("epics.current carries no moved after dismiss", async () => !ok(sy("epics", "current"), "current").moved);

      // The archived toggle: s9-spring is listed only with it.
      await eventually("s9-spring's row says archived", async () => (await root.locator("[data-id=s9-spring][data-changed=archived]").count()) === 1);
      await root.locator("[data-form='epics.list'] input[type=checkbox]").check();
      await eventually("the archived list holds s9-spring as archived", async () => (await root.locator("[data-id=s9-spring][data-archived=true]").count()) === 1);
      await root.locator("[data-form='epics.list'] input[type=checkbox]").uncheck();
      // The live list again: s9-spring as the watch said it, not the archived list's card.
      await eventually("the live list again", async () => (await root.locator("[data-id=s9-spring][data-changed=archived]").count()) === 1 && (await root.locator("[data-id=s9-spring] [data-field=tickets]").count()) === 0 && (await root.locator("[data-id=genesis]").count()) === 1);
    } finally {
      // The owner back on genesis, the case's epics gone, before the page goes.
      sy("epics", "switch", "--name", "genesis");
      sy("epics", "dismiss");
      for (const n of ["s9-summer", "s9-spring"]) {
        sy("epics", "archive", "--name", n);
        sy("epics", "delete", "--name", n);
      }
      await context.close();
    }
  });

  await test("orgs_list", async () => {
    const orgs = sy("orgs", "list").filter((e) => e.type === "org").map((e) => e.org);
    assert(orgs.length > 0, "the owner reads an organization");
    const { page, context } = await openPage("#/orgs");
    const root = page.locator("[data-surface=orgs-list]");
    try {
      await eventually("every organization is a row", async () => (await root.locator("[data-id]").count()) === orgs.length);
      for (const o of orgs) {
        const row = root.locator(`[data-id="${o.name}"]`);
        assert(Number(await text(row.locator("[data-field=tickets]"))) === o.tickets, `${o.name}: ${o.tickets} tickets`);
        assert(Number(await text(row.locator("[data-field=shortcodes]"))) === o.shortcodes, `${o.name}: ${o.shortcodes} shortcodes`);
      }
      // Both schemes are the tokens': the name in the accent, light then dark under the wrapper.
      const accent = () => root.locator("[data-field=name]").first().evaluate((e) => getComputedStyle(e).color);
      const light = await accent();
      await page.locator("orgs-list").evaluate((e) => e.setAttribute("data-plexus-theme", "dark"));
      await eventually("the dark accent", async () => (await accent()) !== light);
      // The token itself, as the wrapper resolves it in dark: the name wears it.
      const token = await page.locator("orgs-list").evaluate((w) => {
        const probe = document.createElement("span");
        probe.style.color = getComputedStyle(w).getPropertyValue("--accent").trim();
        document.body.appendChild(probe);
        const c = getComputedStyle(probe).color;
        probe.remove();
        return c;
      });
      assert((await accent()) === token, `the name wears the dark --accent: ${await accent()} (the token ${token}; light ${light})`);
    } finally {
      await context.close();
    }
  });

  await test("me_shows_my_grants_and_requires_answers", async () => {
    const who = ok(sy("session", "whoami"), "identity");
    sy("members", "teams", "create", "--name", "s9-crew");
    sy("members", "teams", "add", "--team", "s9-crew", "--principal", who.principal);
    const { page, context } = await openPage("#/me");
    const root = page.locator("[data-surface=me-page]");
    try {
      await eventually("the principal", async () => (await text(root.locator("[data-field=principal]"))) === who.principal);
      assert((await text(root.locator("[data-field=provider]"))) === who.provider, `the provider is ${who.provider}`);
      assert((await text(root.locator("[data-field=name]"))) === who.name, `the name is ${who.name}`);
      await eventually("the grant on * as admin", async () => (await text(root.locator("[data-field=grants] [data-id='admin@*'] [data-field=role]"))) === "admin");
      await eventually("the team", async () => (await root.locator("[data-field=teams] [data-id=s9-crew]").count()) === 1);
      await eventually("the door's provider", async () => (await root.locator("[data-field=providers] [data-id=key]").count()) === 1);

      // who may: ui.rollback is the admin's, epics.switch every reader's.
      const form = root.locator("[data-form='auth.requires']");
      await form.locator("input").fill("ui.rollback");
      await form.locator("[data-action='auth.requires']").click();
      await eventually("ui.rollback needs admin", async () => (await text(form.locator("[data-field=roles]"))) === "admin");
      assert((await text(form.locator("[data-field=may]"))).startsWith("may, on * as admin"), `the owner may, on * as admin: ${await text(form.locator("[data-field=may]"))}`);
      await form.locator("input").fill("epics.switch");
      await form.locator("input").press("Enter");
      await eventually("epics.switch's roles", async () => (await text(form.locator("[data-field=roles]"))).includes("viewer"));
      assert((await form.locator("[data-sugg='ui.rollback']").count()) === 1, "the method asked before is a recent one");
      await form.locator("input").fill("nope.nada");
      await form.locator("input").press("Enter");
      await eventually("an unknown method is refused in place", async () => (await form.locator("[data-refused]").count()) === 1);
    } finally {
      await context.close();
      sy("members", "teams", "delete", "--team", "s9-crew");
    }

    // logout: a second session of the owner's key, signed out through the shell's door.
    const keyFile = path.join(work, "admin.key");
    const file = path.join(work, "s9-logout.session");
    const s = await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, file, "owner");
    const out = await openAs(s.token, "#/me");
    try {
      // At once, before anything the page reads has answered.
      const r = out.page.locator("[data-surface=me-page]");
      await r.locator("[data-action='session.logout']").click();
      await eventually("the door: /login", async () => new URL(out.page.url()).pathname === "/login");
      // A browser's session signs out by the door alone: no session.logout over the socket,
      // which would leave /logout no session to end and its backend and provider untold.
      const overSocket = out.sent.filter((f) => f.includes("session.logout"));
      assert(overSocket.length === 0, `no session.logout frame from a browser session: ${overSocket.join(" | ")}`);
      // The ended session's token is refused at the upgrade (401), or answers as no one.
      let after;
      try {
        after = synapse(server, file, "session", "whoami");
      } catch (e) {
        assert(/\b401\b/.test(e.message), `the ended session is refused at the upgrade: ${e.message.split("\n")[0]}`);
        after = [{ type: "refused", message: "401" }];
      }
      assert(!ok(after, "identity")?.principal?.startsWith("key:"), `the session is over: ${JSON.stringify(after)}`);
      assert(ok(sy("session", "whoami"), "identity").principal === who.principal, "the owner's own session stands");
    } finally {
      await out.context.close();
    }
  });

  await test("releases_list_live_and_rollback_as_admin", async () => {
    const live = () => sy("ui", "versions").find((e) => e.type === "version" && e.live)?.hash;
    assert(live() === v2.version, "v2 is live as the runner left it");
    const tickets = ok(sy("ui", "contract", "--uses", "tickets.list"), "contract").contract;
    sy("ui", "slots", "set", "--layout", "board-layout", "--slot", "nav", "--component", "nav-flat");
    const { page, context } = await openPage("#/releases");
    const root = page.locator("[data-surface=ui-versions]");
    let v3;
    try {
      await eventually("both versions listed", async () => (await root.locator(`[data-id="${v1.version}"]`).count()) === 1 && (await root.locator(`[data-id="${v2.version}"]`).count()) === 1);
      assert((await root.locator(`[data-id="${v2.version}"][data-live=true]`).count()) === 1, "v2 is marked live");
      assert((await root.locator(`[data-id="${v1.version}"][data-live=true]`).count()) === 0, "v1 is not");
      await eventually("the tickets contract", async () => (await text(root.locator("[data-field=contract]"))) === tickets);
      await eventually("the section set is a row", async () => (await text(root.locator("[data-field=slots] [data-id='board-layout/nav'] [data-field=component]"))) === "nav-flat");
      assert((await root.locator("[data-field=routes] [data-id], [data-field=routes] [data-empty]").count()) > 0, "the routes table is drawn");

      // A second push: the list moves, the new one live.
      v3 = bundle(work, "s9-v3", { engine: v1.engine, edit: (ui) => fs.appendFileSync(path.join(ui, "src", "hello-list.rhx"), '<p class="s9-v3">v3</p>\n') });
      const pushed = push(server, session, path.join(work, "signer.key"), v3);
      assert(pushed?.type === "pushed" && pushed.version === v3.version, `v3 pushed: ${JSON.stringify(pushed)}`);
      await eventually("v3 appears, live", async () => (await root.locator(`[data-id="${v3.version}"][data-live=true]`).count()) === 1);
      assert((await root.locator(`[data-id="${v2.version}"][data-live=true]`).count()) === 0, "v2 no longer marked live");

      // rollback to v2: a confirm, then the answer.
      const row = root.locator(`[data-id="${v2.version}"]`);
      await row.locator("[data-action='ui.rollback']").click();
      await row.locator("[data-action='ui.rollback'][data-confirm]").click();
      await eventually("v2 live again on the page", async () => (await root.locator(`[data-id="${v2.version}"][data-live=true]`).count()) === 1);
      await eventually("and on the server", async () => live() === v2.version);
      await eventually("the answer is said", async () => (await text(root.locator("[data-field=answer]"))).includes(v2.version.slice(0, 8)));

      // Back to v3, a version already moved to: the mark follows (the pointer is the page's own).
      const row3 = root.locator(`[data-id="${v3.version}"]`);
      await row3.locator("[data-action='ui.rollback']").click();
      await row3.locator("[data-action='ui.rollback'][data-confirm]").click();
      await eventually("v3 live again on the page", async () => (await root.locator(`[data-id="${v3.version}"][data-live=true]`).count()) === 1 && (await root.locator("[data-live=true]").count()) === 1);
      await eventually("and on the server", async () => live() === v3.version);
      // And a move made elsewhere, to v2 a second time: heard from the watch.
      sy("ui", "rollback", "--to", v2.version);
      await eventually("the watch's move to v2 marks it", async () => (await root.locator(`[data-id="${v2.version}"][data-live=true]`).count()) === 1 && (await root.locator("[data-live=true]").count()) === 1);

      // preview v1: /app/ serves it to this session alone; then cleared.
      const served = () => page.evaluate(() => fetch("/app/").then((r) => r.text()));
      await root.locator(`[data-id="${v1.version}"] [data-action='ui.preview']`).click();
      await eventually("/app/ serves v1 to the owner", async () => (await served()).includes(v1.version));
      assert((await root.locator("[data-field=preview] a[href='/app/']").count()) === 1, "a link to the preview");
      await root.locator("[data-action='ui.preview'][data-clear]").click();
      await eventually("/app/ serves v2 again", async () => (await served()).includes(v2.version));
    } finally {
      await context.close();
      sy("ui", "preview");
      if (live() !== v2.version) sy("ui", "rollback", "--to", v2.version);
      sy("ui", "slots", "clear", "--layout", "board-layout", "--slot", "nav");
    }

    // An admin of one shortcode is offered rollback, and the service refuses it on `*`: said on the row.
    const code = ctx.groupsNow().find((g) => g.root)?.shortcode;
    const shelf = await another("s9-shelf", null);
    const g = sy("members", "grant", "--principal", shelf.principal, "--scope", code, "--role", "admin");
    assert(ok(g, "grant"), `admin on ${code} granted: ${JSON.stringify(g)}`);
    const sa = await openAs(shelf.token, "#/releases");
    try {
      const r = sa.page.locator("[data-surface=ui-versions]");
      const row = r.locator(`[data-id="${v1.version}"]`);
      await eventually("the shortcode admin's list", async () => (await row.locator("[data-action='ui.rollback']:not([disabled])").count()) === 1);
      await row.locator("[data-action='ui.rollback']").click();
      await row.locator("[data-action='ui.rollback'][data-confirm]").click();
      const said = await eventually("an answer on the row", async () => {
        if ((await row.locator("[data-refused]").count()) === 1) return "refused";
        if ((await row.locator("[data-answer=rolled_back]").count()) === 1) return "rolled";
        return null;
      });
      // FRAME-43: the UI has no scope — a grant on one shortcode moves no pointer, and the row says so.
      if (said === "rolled") sy("ui", "rollback", "--to", v2.version);
      assert(said === "refused", `the service refuses ui.rollback to an admin on ${code} alone (FRAME-43); it answered: ${said}`);
      const words = await text(row.locator("[data-refused]"));
      assert(words.includes("*"), `the refusal names \`*\`: ${words}`);
      assert(live() === v2.version, "the pointer stayed on v2");
      assert((await r.locator(`[data-id="${v2.version}"][data-live=true]`).count()) === 1, "and the page's mark with it");
    } finally {
      await sa.context.close();
      revoke(shelf.principal, code);
    }

    // A viewer sees the list and rollback disabled, the role named.
    const viewer = await another("s9-viewer", "viewer");
    const as = await openAs(viewer.token, "#/releases");
    try {
      const r = as.page.locator("[data-surface=ui-versions]");
      await eventually("the viewer's list", async () => (await r.locator(`[data-id="${v2.version}"]`).count()) === 1);
      await eventually("rollback disabled", async () => (await r.locator(`[data-id="${v1.version}"] [data-action='ui.rollback'][disabled]`).count()) === 1);
      assert((await r.locator(`[data-id="${v1.version}"] [data-action='ui.preview'][disabled]`).count()) === 1, "preview disabled");
      await eventually("the role named", async () => (await text(r.locator("[data-field=needs]"))).includes("admin"));
    } finally {
      await as.context.close();
      revoke(viewer.principal, "*");
    }
  });

  // FRAME-52: the releases page names and tags a version and undoes the last move.
  await test("releases_label_tag_and_undo", async () => {
    const live = () => sy("ui", "versions").find((e) => e.type === "version" && e.live)?.hash;
    assert(live() === v2.version, "v2 is live as the runner left it");
    const { page, context } = await openPage("#/releases");
    const root = page.locator("[data-surface=ui-versions]");
    try {
      const row = root.locator(`[data-id="${v1.version}"]`);
      await eventually("v1 listed, tag offered", async () => (await row.locator("[data-action='ui.tag']:not([disabled])").count()) === 1);
      await row.locator("[data-action='ui.tag']").first().click();
      await row.locator("[data-tag-label]").fill("s52 first");
      await row.locator("[data-tag-tags]").fill("demo, s52");
      await row.locator("[data-action='ui.tag'][data-save]").click();
      await eventually("the label on the row", async () => (await row.locator("[data-field=label]").count()) === 1 && (await text(row.locator("[data-field=label]"))) === "s52 first");
      assert((await row.locator("[data-tag=demo]").count()) === 1 && (await row.locator("[data-tag=s52]").count()) === 1, "the tags on the row");
      const kept = sy("ui", "versions", "--tag", "s52").filter((e) => e.type === "version");
      assert(kept.length === 1 && kept[0].hash === v1.version && kept[0].label === "s52 first", `the server keeps them: ${JSON.stringify(kept)}`);

      // FRAME-52b: a tag made elsewhere (an agent) appears on the row from the watch;
      // the label moved there leaves v1's row.
      sy("ui", "tag", "--version", v2.version, "--label", "s52 first", "--tags", "s52b");
      const row2 = root.locator(`[data-id="${v2.version}"]`);
      await eventually("the agent's tag on v2's row", async () => (await row2.locator("[data-tag=s52b]").count()) === 1 && (await row2.locator("[data-field=label]").count()) === 1);
      await eventually("the label gone from v1's row", async () => (await row.locator("[data-field=label]").count()) === 0);
      sy("ui", "tag", "--version", v1.version, "--label", "s52 first");
      await eventually("and back on v1's", async () => (await row.locator("[data-field=label]").count()) === 1 && (await row2.locator("[data-field=label]").count()) === 0);

      // A move by label from elsewhere: heard, and the history says it.
      const back = sy("ui", "rollback", "--to", "s52 first");
      assert(ok(back, "live")?.version === v1.version, `rollback --to a label: ${JSON.stringify(back)}`);
      const history = root.locator("[data-field=history]");
      await eventually("the rollback heads the history", async () => (await history.locator("[data-move]").first().getAttribute("data-how")) === "rollback");
      await eventually("v1 marked live", async () => (await root.locator(`[data-id="${v1.version}"][data-live=true]`).count()) === 1);

      // Undo: the pointer back to v2, kept as an undo; redo offered.
      await eventually("undo offered", async () => (await history.locator("[data-action='ui.undo']:not([disabled])").count()) === 1);
      await history.locator("[data-action='ui.undo']").click();
      // FRAME-52b: by the seq the page showed — one move undone.
      await eventually("v2 live on the server", async () => live() === v2.version);
      await eventually("and on the page", async () => (await root.locator(`[data-id="${v2.version}"][data-live=true]`).count()) === 1 && (await root.locator("[data-live=true]").count()) === 1);
      await eventually("the answer said", async () => (await history.locator("[data-answer=undo]").count()) === 1);
      await eventually("the undo heads the history", async () => (await history.locator("[data-move]").first().getAttribute("data-how")) === "undo");
      const h = sy("ui", "history", "--limit", "2").filter((e) => e.type === "move");
      assert(h[0]?.how === "undo" && h[0].version === v2.version && h[1]?.how === "rollback", `ui.history: ${JSON.stringify(h)}`);
      await eventually("redo offered", async () => (await history.locator("[data-action='ui.redo']:not([disabled])").count()) === 1);
    } finally {
      await context.close();
      sy("ui", "tag", "--version", v1.version, "--untag", "demo,s52", "--label", "");
      sy("ui", "tag", "--version", v2.version, "--untag", "s52b");
      if (live() !== v2.version) sy("ui", "rollback", "--to", v2.version);
    }
  });
};
