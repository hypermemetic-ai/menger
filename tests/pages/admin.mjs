// SURFACE-9: the admin pages — epics (#/epics), organizations (#/orgs), who
// I am (#/me) and the releases (#/releases). Run alone by
// `node tests/pages_browser.mjs --group admin`.
//
// Marks: every page's root `[data-surface=epics-page|orgs-list|me-page|ui-versions]`;
// rows `[data-id]` (an epic's name, an organization's name, a grant's scope,
// a team's name, a provider's name, a version's hash); fields `[data-field=…]`;
// actions `[data-action=epics.switch|epics.get|epics.dismiss|auth.requires|
// session.logout|ui.rollback|ui.preview]`; forms `[data-form=epics.list|
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
  /** A page signed in with `token` rather than the owner's. */
  async function openAs(token, route) {
    const { context, page, seen } = await openPage("", { signedIn: false });
    await context.clearCookies();
    await context.addCookies(sessionCookies(server.origin, token));
    await page.goto(`${server.origin}/app/${route}`);
    return { context, page, seen };
  }

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
      assert(ok(sy("epics", "current"), "current").epic.name === "s9-spring", "epics.current answers s9-spring");
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
      // A switch's answer reaches the page only through FRAME-37's answer fold.
      if (process.env.FRAME_37) {
        await root.locator("[data-id=s9-gone] [data-action='epics.switch']").click();
        await eventually("the refusal is said beside it", async () => (await text(root.locator("[data-id=s9-gone] [data-refused]"))).includes("s9-gone"));
        assert((await text(root.locator("[data-field=current]"))) === "s9-spring", "a refused switch leaves the top where it was");
      } else console.log("     skip a refused switch said beside it: waits on FRAME-37 (call's answer fold)");

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
      assert(!ok(sy("epics", "current"), "current").moved, "epics.current carries no moved after dismiss");

      // The archived toggle: s9-spring is listed only with it.
      await eventually("s9-spring's row says archived", async () => (await root.locator("[data-id=s9-spring][data-changed=archived]").count()) === 1);
      await root.locator("[data-form='epics.list'] input[type=checkbox]").check();
      await eventually("the archived list holds s9-spring as archived", async () => (await root.locator("[data-id=s9-spring][data-archived=true]").count()) === 1);
      await root.locator("[data-form='epics.list'] input[type=checkbox]").uncheck();
      // The live list again: s9-spring as the watch said it, not the archived list's card.
      await eventually("the live list again", async () => (await root.locator("[data-id=s9-spring][data-changed=archived]").count()) === 1 && (await root.locator("[data-id=s9-spring] [data-field=tickets]").count()) === 0 && (await root.locator("[data-id=genesis]").count()) === 1);
    } finally {
      await context.close();
      sy("epics", "switch", "--name", "genesis");
      sy("epics", "dismiss");
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
      assert((await accent()) === "rgb(142, 166, 255)", `the dark --accent: ${await accent()} (light ${light})`);
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
      await eventually("the grant on * as admin", async () => (await text(root.locator("[data-field=grants] [data-id='*'] [data-field=role]"))) === "admin");
      await eventually("the team", async () => (await root.locator("[data-field=teams] [data-id=s9-crew]").count()) === 1);
      await eventually("the door's provider", async () => (await root.locator("[data-field=providers] [data-id=key]").count()) === 1);

      // who may: ui.rollback is the admin's, epics.switch every reader's.
      const form = root.locator("[data-form='auth.requires']");
      await form.locator("input").fill("ui.rollback");
      await form.locator("[data-action='auth.requires']").click();
      await eventually("ui.rollback needs admin", async () => (await text(form.locator("[data-field=roles]"))) === "admin");
      assert((await text(form.locator("[data-field=may]"))) === "you may", "the owner may");
      await form.locator("input").fill("epics.switch");
      await form.locator("input").press("Enter");
      await eventually("epics.switch's roles", async () => (await text(form.locator("[data-field=roles]"))).includes("viewer"));
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
      const r = out.page.locator("[data-surface=me-page]");
      await eventually("the second session's page", async () => (await text(r.locator("[data-field=principal]"))) === who.principal);
      await r.locator("[data-action='session.logout']").click();
      await eventually("the door: /login", async () => new URL(out.page.url()).pathname === "/login");
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
      // ui.rollback's refusal (unknown_version, the gate's forbidden) said on its row is FRAME-37's answer fold.
      console.log("     skip a refused rollback said on its row: waits on FRAME-37 (call's answer fold)");
      await eventually("the answer is said", async () => (await text(root.locator("[data-field=answer]"))).includes(v2.version.slice(0, 12)));

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
    }
  });
};
