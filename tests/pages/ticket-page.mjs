// SURFACE-3: the ticket page, whole — composed of its parts, each a svelte/1
// component the host fills into its bound mark; STUDIO-12: the page is
// page-ticket (SV-4: svelte/1), its root still `[data-surface=ticket-page]`.
// Marks: `[data-surface=ticket-page]` (`[data-loading]`, `[data-empty]` "No
// ticket …", `[data-deleted]`), `[data-surface=ticket-head]`
// `[data-field=id|title|kind|status|severity|labels|parent|address]`,
// `[data-badge=hidden|archived|frozen]` and `[data-sib=prev|next]`,
// `[data-surface=ticket-body]` `[data-field=body]` (SURFACE-16: drawn by `[data-surface=ticket-view]`, the
// rendering alone, its code inlined: props in, no call), `[data-surface=ticket-relations]`
// (`[data-ticket-child=<id>]`, `[data-field=blocked_by|unlocks]`), `[data-surface=ticket-subtasks]`
// (`[data-id]`, `[data-form=tickets.subtasks.create]`, `[data-refused]`), `[data-surface=ticket-commits]`
// (`[data-id=<ref>]`, `[data-form=tickets.commits.link]`, `[data-action=tickets.commits.unlink]`,
// `[data-refused]`), `[data-surface=ticket-history]` (`[data-transition]`, `[data-field=from|to|at|actor]`),
// `[data-surface=ticket-revisions]` (`[data-open]`, `li[data-id=<rev>]` `[data-ab=a|b]`, `[data-view=side|text|was]`,
// `[data-revision-open]`, `[data-diff]` `[data-side-by-side]` `[data-text]`, `[data-pick]`), and the other
// tickets' marks `[data-mark=X]` (carrying `data-id`) holding — or being — the bound mark
// `[data-slot=X][data-component=X]` for ticket-edit, ticket-actions (SURFACE-4; the page's
// wrapper around its ticket-edit mark, STUDIO-12) and work-blockers (SURFACE-7, ticket-relations' own).
import { bundle, push, sentCalls } from "../../scripts/ui-throwaway.mjs";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, groupsNow, fs, path, session, server, v1, ticketAt } = ctx;
  const trimmed = (t) => String(t ?? "").trim();
  const text = async (loc) => trimmed(await loc.first().textContent({ timeout: 1000 }));
  const scratch = path.dirname(session);
  // The page is page-ticket (rhx, STUDIO-12) at #/ticket/:id and its alias #/sv/ticket/:id.
  const ROUTE = "#/sv/ticket/";
  let n = 0;
  const root = () => groupsNow().filter((g) => g.root && !g.shortcode.startsWith("AAA")).at(-1);
  /** A fresh ticket under the last shortcode's root: its row, as `create` answers it. */
  function made(title, ...flags) {
    const g = root();
    const [m] = sy("tickets", "create", "--epic", g.epic, "--parent", g.root, "--title", `${title} ${++n}`, ...flags);
    assert(m?.ticket?.id, `made "${title}": ${JSON.stringify(m)}`);
    return m.ticket;
  }
  const body = (md) => {
    const f = path.join(scratch, `s3-body-${++n}.md`);
    fs.writeFileSync(f, md);
    return `@${f}`;
  };
  async function open(id) {
    const o = await openPage(`${ROUTE}${id}`);
    const tp = o.page.locator("[data-surface=ticket-page]");
    await eventually(`#/ticket/${id} draws its head`, async () => (await text(tp.locator("[data-surface=ticket-head] [data-field=title]"))).length > 0, 10_000);
    return { ...o, tp };
  }

  await test("the_page_draws_every_field", async () => {
    const parent = made("the kiln's parent");
    const blocker = made("the kiln's blocker");
    const t = made("the kiln, every field", "--severity", "High", "--blocked-by", blocker.id, "--body", body("The **glaze** fires at cone 6.\n\n- bisque\n- glaze\n"));
    sy("tickets", "set_parent", "--id", t.id, "--parent", parent.id);
    sy("tickets", "label", "--id", t.id, "--add", "needs-you");
    const { page, tp, context } = await open(t.id);
    try {
      const head = tp.locator("[data-surface=ticket-head]");
      const f = (name) => head.locator(`[data-field=${name}]`);
      assert((await text(f("id"))) === t.id, `id ${t.id}`);
      assert((await text(f("title"))) === t.title, `title ${t.title}`);
      assert((await text(f("kind"))) === t.kind, `kind ${t.kind}`);
      assert((await text(f("status"))) === t.status, `status ${t.status}`);
      assert((await f("status").getAttribute("data-status")) === t.status, "the status chip carries its status");
      await eventually("severity High", async () => (await f("severity").getAttribute("data-severity")) === "High");
      await eventually("the label", async () => (await f("labels").locator("[data-label=needs-you]").count()) === 1);
      await eventually("the parent chip", async () => (await f("parent").locator(`[data-chip=parent][data-chip-id="${parent.id}"]`).count()) === 1);
      assert((await text(f("address"))).includes(t.id), `the address names ${t.id}: ${await text(f("address"))}`);
      const b = tp.locator("[data-surface=ticket-body] [data-field=body]");
      assert((await text(b)).includes("fires at cone 6"), "the body");
      assert(trimmed(await b.locator("strong").textContent({ timeout: 1000 })) === "glaze" && (await b.locator("li").count()) === 2, "the body as markdown");
      const rel = tp.locator("[data-surface=ticket-relations]");
      assert((await rel.locator(`[data-field=blocked_by] [data-chip=blocker][data-chip-id="${blocker.id}"]`).count()) === 1, "the blocker chip");
      for (const s of ["ticket-head", "ticket-body", "ticket-relations", "ticket-subtasks", "ticket-commits", "ticket-history", "ticket-revisions"]) {
        assert((await tp.locator(`[data-surface=${s}]`).count()) === 1, `${s} is placed`);
      }
      await eventually("no skeleton once get answered", async () => (await tp.locator(".skeleton[data-loading]").count()) === 0);
      // Live: the badges and the title move without a reload.
      sy("tickets", "update", "--id", t.id, "--hidden", "true");
      await eventually("the hidden badge", async () => (await head.locator("[data-badge=hidden]").count()) === 1, 5_000);
      sy("tickets", "archive", "--ids", t.id);
      await eventually("the archived badge", async () => (await head.locator("[data-badge=archived]").count()) === 1, 5_000);
      sy("tickets", "restore", "--ids", t.id);
      await eventually("restored: no archived badge", async () => (await head.locator("[data-badge=archived]").count()) === 0, 5_000);
      sy("tickets", "amend", "--id", t.id, "--title", `${t.title} (amended)`);
      await eventually("the amended title", async () => (await text(f("title"))) === `${t.title} (amended)`, 5_000);
      // Both schemes: the tokens paint the chip.
      const chip = async (theme) => page.evaluate((th) => {
        document.documentElement.dataset.theme = th;
        const e = document.querySelector("[data-surface=ticket-head] [data-field=status]");
        return getComputedStyle(e).backgroundColor;
      }, theme);
      const shots = process.env.PAGES_SHOTS;
      const light = await chip("light");
      if (shots) await page.screenshot({ path: path.join(shots, "ticket-page-light.png"), fullPage: true });
      const dark = await chip("dark");
      if (shots) await page.screenshot({ path: path.join(shots, "ticket-page-dark.png"), fullPage: true });
      assert(light && dark && light !== dark, `the status chip's ground moves with the scheme: ${light} → ${dark}`);
    } finally {
      await context.close();
    }
  });

  await test("children_and_subtasks_are_live", async () => {
    const t = made("the kiln with children");
    const { tp, context } = await open(t.id);
    try {
      const rel = tp.locator("[data-surface=ticket-relations]");
      await eventually("no children yet", async () => (await rel.locator("[data-field=children] [data-empty]").count()) === 1);
      const [c] = sy("tickets", "create", "--epic", t.epic, "--parent", t.id, "--title", "a child made elsewhere");
      await eventually(`the child ${c.ticket.id} appears`, async () => (await rel.locator(`[data-ticket-child="${c.ticket.id}"]`).count()) === 1, 5_000);
      sy("tickets", "set_status", "--id", c.ticket.id, "--status", "Ready");
      await eventually("its status moves in place", async () => (await rel.locator(`[data-ticket-child="${c.ticket.id}"]`).getAttribute("data-status")) === "Ready", 5_000);
      const subs = tp.locator("[data-surface=ticket-subtasks]");
      const form = subs.locator("[data-form='tickets.subtasks.create']");
      await form.locator("[data-action='tickets.subtasks.create']").click({ timeout: 2000 });
      await eventually("a blank title is refused where it was typed", async () => (await text(subs.locator("[data-refused]"))).includes("not blank") && (await form.locator("input").getAttribute("aria-invalid")) === "true");
      await form.locator("input").fill("load the shelves");
      await eventually("the refusal goes as the title is typed", async () => (await subs.locator("[data-refused]").count()) === 0);
      await form.locator("input").press("Enter");
      await eventually(`the subtask ${t.id}.1 appears`, async () => (await subs.locator(`[data-id="${t.id}.1"]`).count()) === 1, 5_000);
      assert((await text(subs.locator(`[data-id="${t.id}.1"] [data-field=title]`))) === "load the shelves", "its title");
      assert((await form.locator("input").inputValue()) === "", "the field is cleared");
      assert((await rel.locator(`[data-ticket-child="${t.id}.1"]`).count()) === 0, "a subtask is not drawn twice as a child");
      const listed = sy("tickets", "subtasks", "list", "--parent", t.id).flatMap((e) => (e.type === "rows" ? e.items : [e])).filter((e) => (e.summary ?? e.ticket)?.id?.startsWith(`${t.id}.`));
      assert(listed.length === 1, `the service holds one subtask: ${JSON.stringify(listed)}`);
      sy("tickets", "subtasks", "create", "--parent", t.id, "--title", "glaze the shelves");
      await eventually(`one made elsewhere, ${t.id}.2, appears`, async () => (await subs.locator(`[data-id="${t.id}.2"]`).count()) === 1, 5_000);
    } finally {
      await context.close();
    }
    // The head links the siblings (the parent's children, subtasks passed over): ‹ prev, next ›.
    const [c2] = sy("tickets", "create", "--epic", t.epic, "--parent", t.id, "--title", "a second child");
    const kids = sy("tickets", "list", "--parent", t.id, "--summary", "true").flatMap((e) => (e.type === "rows" ? e.items : [e])).filter((e) => e.type === "summary").map((e) => e.summary.id).filter((x) => !x.includes("."));
    assert(kids.length === 2 && kids[1] === c2.ticket.id, `two children in order: ${kids}`);
    const { tp: tp2, context: ctx2 } = await open(kids[0]);
    try {
      const head = tp2.locator("[data-surface=ticket-head]");
      await eventually("next › the second child", async () => { const h = (await head.locator("[data-sib=next]").getAttribute("href", { timeout: 1000 })) ?? ""; return h.startsWith(`${ROUTE.replace("ticket/", "t/")}${c2.ticket.uid}/`) && ticketAt(h, kids[1]); }, 5_000);
      assert((await head.locator("[data-sib=prev]").count()) === 0, "no ‹ prev for the first");
    } finally {
      await ctx2.close();
    }
  });

  await test("commits_link_and_unlink", async () => {
    const t = made("the kiln's commits");
    const { tp, context } = await open(t.id);
    try {
      const box = tp.locator("[data-surface=ticket-commits]");
      const form = box.locator("[data-form='tickets.commits.link']");
      await eventually("none linked", async () => (await box.locator("[data-empty]").count()) === 1);
      await form.locator("input").fill("not a hash");
      await form.locator("[data-action='tickets.commits.link']").click({ timeout: 2000 });
      await eventually("a bad reference is refused in the service's words", async () => (await text(box.locator("[data-refused]"))).includes("is not a commit reference"));
      assert(!sy("tickets", "get", "--id", t.id).find((e) => e.type === "ticket").ticket.commits.length, "nothing was linked");
      await form.locator("input").fill("77df9a0");
      await form.locator("[data-action='tickets.commits.link']").click({ timeout: 2000 });
      await eventually("77df9a0 is listed", async () => (await box.locator('[data-id="77df9a0"]').count()) === 1, 5_000);
      await eventually("the service has it", async () => sy("tickets", "get", "--id", t.id).find((e) => e.type === "ticket").ticket.commits.includes("77df9a0"));
      sy("tickets", "commits", "link", "--id", t.id, "--commits", "menger@1a2b3c4");
      await eventually("one linked elsewhere appears", async () => (await box.locator('[data-id="menger@1a2b3c4"]').count()) === 1, 5_000);
      await box.locator('[data-id="77df9a0"] [data-action="tickets.commits.unlink"]').click({ timeout: 2000 });
      await eventually("unlinked: 77df9a0 goes", async () => (await box.locator('[data-id="77df9a0"]').count()) === 0, 5_000);
      const left = sy("tickets", "get", "--id", t.id).find((e) => e.type === "ticket").ticket.commits;
      assert(JSON.stringify(left) === '["menger@1a2b3c4"]', `the service keeps the other: ${JSON.stringify(left)}`);
    } finally {
      await context.close();
    }
  });

  await test("history_shows_a_status_move", async () => {
    const t = made("the kiln's history");
    sy("tickets", "set_status", "--id", t.id, "--status", "Pending");
    const { tp, context } = await open(t.id);
    try {
      const h = tp.locator("[data-surface=ticket-history]");
      const steps = h.locator("[data-transition=status]");
      // The birth (— → Draft) and the move made before the page opened.
      await eventually("the birth and the move before the page opened", async () => (await steps.count()) === 2);
      const moved = steps.first();
      assert((await text(moved.locator("[data-field=from]"))) === t.status && (await text(moved.locator("[data-field=to]"))) === "Pending", `${t.status} → Pending as chips`);
      assert(/^\d\d-\d\d \d\d:\d\d$/.test(await text(moved.locator("[data-field=at]"))), `when, as a date: ${await text(moved.locator("[data-field=at]"))}`);
      assert((await text(moved.locator("[data-field=actor]"))).length > 0, "who");
      sy("tickets", "set_status", "--id", t.id, "--status", "Ready", "--message", "promoted by the owner");
      await eventually("the new move appears live, newest first", async () => (await steps.count()) === 3 && (await text(steps.first().locator("[data-field=to]"))) === "Ready", 5_000);
      assert((await text(steps.first().locator(".why"))) === "promoted by the owner", "why");
      assert((await steps.first().locator("[data-field=to]").getAttribute("data-status")) === "Ready", "the status chip");
      const y = await steps.evaluateAll((els) => els.map((e) => e.getBoundingClientRect().top));
      assert(y[0] < y[1] && y[1] < y[2], `newest first on screen: ${y}`);
    } finally {
      await context.close();
    }
  });

  await test("revisions_list_open_and_diff", async () => {
    const t = made("the kiln's revisions", "--body", body("first line\nsecond line\n"));
    sy("tickets", "amend", "--id", t.id, "--body", body("first line\nsecond line, changed\n"));
    const { page, tp, context } = await open(t.id);
    try {
      const r = tp.locator("[data-surface=ticket-revisions]");
      const revs = sy("tickets", "revisions", "list", "--id", t.id).filter((e) => e.type === "revision").map((e) => e.revision.rev);
      await eventually(`every revision listed (${revs})`, async () => (await r.locator("li[data-id]").count()) === revs.length);
      const top = Math.max(...revs);
      // A row opens the diff wide: that revision against the one before it, side by side.
      await r.locator(`li[data-id="${top}"]`).click({ timeout: 2000 });
      await eventually("the diff is open wide", async () => (await r.getAttribute("data-open")) === "true");
      const d = r.locator(`[data-diff="${top - 1}..${top}"] [data-side-by-side]`);
      await eventually("the two compared side by side", async () => (await d.locator("[data-body=old] [data-op=del]").count()) >= 1 && (await d.locator("[data-body=new] [data-op=add]").count()) >= 1, 5_000);
      assert((await text(d.locator("[data-body=new] [data-op=add]"))).includes("changed"), "the new side holds the change");
      assert((await sentCalls(page)).some((c) => c.method === "tickets.revisions.diff" && c.params.text === true && c.params.from === top - 1 && c.params.to === top), "revisions.diff --text asked");
      const wide = await page.evaluate(() => { const e = document.querySelector("[data-surface=ticket-revisions]"); const m = document.querySelector("[data-surface=ticket-body]"); return [e.getBoundingClientRect().width, m.getBoundingClientRect().width]; });
      assert(wide[0] > wide[1], `open, it is wider than the body's column: ${wide}`);
      await r.locator("[data-view=text]").click({ timeout: 2000 });
      await eventually("the text form", async () => (await text(r.locator("[data-text] [data-op=add]"))).startsWith("+ "));
      // The b revision as it stood (revisions.get), then b moved to the one before.
      await r.locator("[data-view=was]").click({ timeout: 2000 });
      await eventually("rev top as it stood", async () => (await text(r.locator(`[data-revision-open="${top}"] [data-field=body]`))).includes("changed"), 5_000);
      await r.locator(`li[data-id="${top - 1}"] [data-ab=b]`).click({ timeout: 2000 });
      await eventually("the older revision as it stood", async () => { const b = await text(r.locator(`[data-revision-open="${top - 1}"] [data-field=body]`)); return b.includes("second line") && !b.includes("changed"); }, 5_000);
      assert((await sentCalls(page)).some((c) => c.method === "tickets.revisions.get" && c.params.rev === top - 1), "revisions.get asked");
      // Pick says ticket.revision_picked on the bus; its button shows it.
      await r.locator("[data-pick]").click({ timeout: 2000 });
      await eventually("picked", async () => (await r.locator("[data-pick]").getAttribute("aria-pressed")) === "true");
      await r.locator("[data-close]").click({ timeout: 2000 });
      await eventually("closed, back in its column", async () => (await r.getAttribute("data-open")) === "false");
      // Two revisions made while the page is open: both listed, the count the service's. V2-37
      // coalesces the same actor's writes within 120 s into its latest revision unless one says
      // why, so the first says why (it is kept apart from the amend's rev) and the second, after
      // a revision with a message, is kept as its own.
      sy("tickets", "update", "--id", t.id, "--severity", "Low", "--message", "the case's first");
      sy("tickets", "update", "--id", t.id, "--severity", "High");
      await eventually("both new revisions appear live", async () => (await r.locator(`li[data-id="${top + 1}"]`).count()) === 1 && (await r.locator(`li[data-id="${top + 2}"]`).count()) === 1, 5_000);
      // EDITOR-16: a third, soon and saying nothing, is coalesced into top+2 (`revised` says
      // top+2 again): no row is added, and top+2's row shows the fields of both live.
      sy("tickets", "update", "--id", t.id, "--title", `${t.title}, renamed`);
      await eventually("the coalesced write shows in top+2's row", async () => { const f = await text(r.locator(`li[data-id="${top + 2}"] [data-field=fields]`)); return f.includes("severity") && f.includes("title"); }, 5_000);
      const held = sy("tickets", "revisions", "list", "--id", t.id).filter((e) => e.type === "revision").map((e) => e.revision.rev);
      assert(Math.max(...held) === top + 2, `the third write was coalesced (${held})`);
      await eventually(`the list is the service's (${held})`, async () => JSON.stringify(await r.locator("li[data-id]").evaluateAll((els) => els.map((e) => Number(e.dataset.id)))) === JSON.stringify(held), 5_000);
      // Opened, the newest is compared with the one before it, which is listed.
      await r.locator(`li[data-id="${top + 2}"]`).click({ timeout: 2000 });
      await eventually("rev top+2 against top+1", async () => (await r.locator(`[data-diff="${top + 1}..${top + 2}"] [data-field=severity]`).count()) === 1, 5_000);
      // Escape closes the open diff first, and the page stays.
      await page.keyboard.press("Escape");
      await eventually("escape closed the diff", async () => (await r.getAttribute("data-open")) === "false");
      assert(ticketAt(await page.evaluate(() => location.hash), t.id), "the first escape stayed on the page");
    } finally {
      await context.close();
    }
  });

  // V2-24: a body's `![alt](media:<hash>)` draws from the shell's `/media/<hash>`:
  // the image put by synapse (`media put`), the body naming its hash, the page's
  // `img` sourced at this origin and drawn with its pixels (the route is behind
  // the session the page holds).
  await test("a_bodys_media_image_draws_from_the_shell", async () => {
    // A real 2×1 PNG (signature, IHDR, IDAT, IEND).
    const PNG_2X1 = "iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAAEUlEQVR42mP8z8DwnwEJMAIAKQgD/2zM2wkAAAAASUVORK5CYII=";
    const t = made("the kiln, with a picture");
    const png = path.join(scratch, `s3-png-${++n}.b64`);
    fs.writeFileSync(png, PNG_2X1);
    const [stored] = sy("media", "put", "--ticket", t.id, "--mime", "image/png", "--data", `@${png}`);
    assert(stored?.type === "stored" && stored.hash?.length === 64 && stored.width === 2, `media put: ${JSON.stringify(stored)}`);
    const [amended] = sy("tickets", "amend", "--id", t.id, "--body", body(`The curve:\n\n![the curve](media:${stored.hash.toUpperCase()})\n`));
    assert(amended?.type === "amended", `amend: ${JSON.stringify(amended)}`);
    const { tp, context } = await open(t.id);
    try {
      const img = tp.locator("[data-surface=ticket-body] [data-field=body] img");
      await eventually("the image is sourced at this origin's /media/<hash>", async () => (await img.count()) === 1 && (await img.first().getAttribute("src")) === `${server.origin}/media/${stored.hash}`, 10_000);
      assert((await img.first().getAttribute("alt")) === "the curve", "its alt text");
      await eventually("and is drawn with its pixels", async () => (await img.first().evaluate((e) => e.complete && e.naturalWidth)) === 2, 10_000);
    } finally {
      await context.close();
    }
  });

  await test("a_missing_ticket_says_so", async () => {
    const { page, context } = await openPage(`${ROUTE}NOPE-99999`);
    try {
      const tp = page.locator("[data-surface=ticket-page]");
      await eventually("[data-empty] says no such ticket", async () => (await text(tp.locator("[data-empty]"))).startsWith("No ticket NOPE-99999"), 10_000);
      assert((await tp.locator("[data-surface=ticket-head]").count()) === 0, "no head drawn");
    } finally {
      await context.close();
    }
    // A ticket deleted while its page is open says "deleted", and offers the actions mark.
    const t = made("the kiln to delete");
    const { tp, context: c2 } = await open(t.id);
    try {
      sy("tickets", "delete", "--ids", t.id);
      await eventually("the page says deleted", async () => (await text(tp.locator("[data-deleted]"))).includes("was deleted"), 5_000);
      assert((await tp.locator("[data-deleted] [data-mark=ticket-actions]").count()) === 1, "the actions mark is offered beside it");
      sy("tickets", "undelete", "--ids", t.id);
      await eventually("undeleted: the ticket again", async () => (await tp.locator("[data-deleted]").count()) === 0 && (await tp.locator("[data-surface=ticket-head]").count()) === 1, 5_000);
    } finally {
      await c2.close();
    }
    // A ticket deleted before its page opened says so too.
    const d = made("the kiln deleted before");
    sy("tickets", "delete", "--ids", d.id);
    const { page: p3, context: c3 } = await openPage(`${ROUTE}${d.id}`);
    try {
      const tp3 = p3.locator("[data-surface=ticket-page]");
      await eventually("the page says deleted, with its title", async () => (await text(tp3.locator("[data-deleted]"))).includes(`${d.id} was deleted — ${d.title}`), 10_000);
      assert((await tp3.locator("[data-empty]").count()) === 0 && (await tp3.locator("[data-surface=ticket-head]").count()) === 0, "not 'no such ticket', no head");
    } finally {
      await c3.close();
    }
  });

  await test("the_marks_for_edit_and_actions_are_there", async () => {
    const t = made("the kiln's marks");
    const { page, tp, context } = await open(t.id);
    try {
      // What the page sends: one `get` of the ticket for every part reading it, five watch streams.
      await eventually("every part drawn", async () => (await tp.locator("[data-surface=ticket-revisions] li[data-id]").count()) > 0 && (await tp.locator("[data-surface=ticket-history] [data-transition]").count()) > 0, 5_000);
      await sleep(300);
      const calls = await sentCalls(page);
      const by = calls.reduce((m, c) => ((m[c.method] = (m[c.method] ?? 0) + 1), m), {});
      console.log(`     sent: ${JSON.stringify(by)}`);
      assert(calls.filter((c) => c.method === "tickets.get" && c.params.id === t.id).length === 1, `one get of ${t.id}: ${JSON.stringify(by)}`);
      // STUDIO-12: the page carries the app's top (app-top, its count live on `watch { membership }`), apart from the six (V2-20: ticket-questions the sixth).
      const own = calls.filter((c) => c.method === "tickets.watch" && c.params?.membership !== true).length;
      assert(own <= 6, `at most six watches of the ticket's (${own}): ${JSON.stringify(by)}`);
      for (const [mark, where] of [["ticket-edit", tp], ["ticket-actions", tp], ["work-blockers", tp.locator("[data-surface=ticket-relations]")]]) {
        const m = where.locator(`[data-mark=${mark}]`);
        assert((await m.count()) === 1, `${mark}'s mark is placed`);
        assert((await m.getAttribute("data-id")) === t.id, `${mark}'s mark carries id=${t.id}`);
        const bound = m.locator(`xpath=descendant-or-self::*[@data-slot="${mark}" and @data-component="${mark}"]`);
        assert((await bound.count()) === 1, `${mark}'s mark is (or holds) its bound mark`);
        const filled = await bound.locator(`[data-surface=${mark}]`).count();
        console.log(`     ${mark}: ${filled ? "the component" : "a hole (its ticket not merged)"}`);
      }
    } finally {
      await context.close();
    }
  });

  await test("the_page_keys_e_escape_and_siblings", async () => {
    const t = made("the kiln's keys");
    const kids = [1, 2, 3].map((i) => sy("tickets", "create", "--epic", t.epic, "--parent", t.id, "--title", `a sibling ${i}`)[0].ticket.id);
    const { page, tp, context } = await open(kids[1]);
    try {
      // The host takes a bound key on the window's capture phase and stops it: the page below never sees it.
      await page.evaluate(() => {
        window.__keys = [];
        document.addEventListener("keydown", (e) => window.__keys.push(e.key), true);
      });
      const head = tp.locator("[data-surface=ticket-head]");
      await eventually("the siblings are known", async () => (await head.locator("[data-sib=next]").count()) === 1, 5_000);
      await page.keyboard.press("]");
      await eventually(`] goes to ${kids[2]}`, async () => ticketAt(await page.evaluate(() => location.hash), kids[2]), 5_000);
      await eventually(`${kids[2]} is drawn`, async () => (await text(page.locator("[data-surface=ticket-head] [data-field=id]"))) === kids[2], 5_000);
      await eventually("its siblings", async () => (await page.locator("[data-surface=ticket-head] [data-sib=prev]").count()) === 1, 5_000);
      await page.keyboard.press("[");
      await eventually(`[ goes back to ${kids[1]}`, async () => ticketAt(await page.evaluate(() => location.hash), kids[1]), 5_000);
      // STUDIO-12: an rhx page takes a new route by drawing again from its initial state (CONTRACT, rhx/1): count from its new draw.
      await eventually(`the page is ${kids[1]}'s`, async () => (await tp.getAttribute("data-ticket")) === kids[1], 5_000);
      // e says ticket.edit on the bus (SURFACE-4 hears it): the page counts it on its root and the edit mark flashes; no navigation.
      const asked = Number(await tp.getAttribute("data-edit-asked"));
      await page.keyboard.press("e");
      await eventually("e asked for the edit", async () => Number(await tp.getAttribute("data-edit-asked")) === asked + 1);
      assert(await tp.locator("[data-mark=ticket-edit]").evaluate((e) => e.classList.contains("heard")), "the edit mark heard it");
      assert(ticketAt(await page.evaluate(() => location.hash), kids[1]), "e moved nothing");
      await page.keyboard.press("x");
      await page.keyboard.press("Escape");
      await eventually("escape goes to the board", async () => !/\/(?:ticket|t)\//.test(await page.evaluate(() => location.hash)), 5_000);
      const seen = await page.evaluate(() => window.__keys);
      assert(seen.includes("x"), `an unbound key reaches the page: ${JSON.stringify(seen)}`);
      for (const k of ["]", "[", "e", "Escape"]) assert(!seen.includes(k), `${k} never reached the page: ${JSON.stringify(seen)}`);
      // g b, a sequence, goes to the board too.
      await page.evaluate((r) => (location.hash = r), `${ROUTE}${kids[0]}`);
      await eventually(`${kids[0]} is drawn`, async () => (await text(page.locator("[data-surface=ticket-head] [data-field=id]"))) === kids[0], 5_000);
      await page.keyboard.press("g");
      await page.keyboard.press("b");
      await eventually("g b goes to the board", async () => !/\/(?:ticket|t)\//.test(await page.evaluate(() => location.hash)), 5_000);
    } finally {
      await context.close();
    }
  });

  // SURFACE-16: ticket-body keeps its live spec and places ticket-view, a
  // sibling inlined in its module: the marks as before, no section of its own.
  await test("ticket_body_draws_through_ticket_view", async () => {
    const t = made("the view's ticket", "--body", body("The **glaze** fires at cone 6.\n"));
    const none = made("the view's empty ticket");
    const { page, tp, context, seen } = await open(t.id);
    try {
      const b = tp.locator("[data-surface=ticket-body] [data-surface=ticket-view] [data-field=body][data-ticket-body]");
      await eventually("the body, through the view", async () => (await text(b)).includes("fires at cone 6"));
      assert(trimmed(await b.locator("strong").textContent({ timeout: 1000 })) === "glaze", "the body as markdown");
      assert((await tp.locator("[data-surface=ticket-body] [data-field=title], [data-surface=ticket-body] [data-field=status]").count()) === 0, "the page's title and status stay ticket-head's");
      assert((await tp.locator("[data-surface=ticket-body] [data-slot]").count()) === 0, "the view is no section of ticket-body's");
      assert(!seen.requests.some((u) => /\/components\/ticket-view\.js$/.test(new URL(u).pathname)), "the view's code came inside ticket-body's module");
      sy("tickets", "amend", "--id", t.id, "--body", body("Now *bisque* first.\n"));
      await eventually("an amend redraws the body in place", async () => (await text(b)).includes("bisque"), 5_000);
      await page.evaluate((r) => (location.hash = r), `${ROUTE}${none.id}`);
      const empty = tp.locator("[data-surface=ticket-body] [data-surface=ticket-view] [data-field=body][data-empty]");
      await eventually("no body yet", async () => (await text(empty)) === "No body yet.", 5_000);
    } finally {
      await context.close();
    }
  });

  // SURFACE-16: a bundle of ticket-view and a page that places it, nothing else
  // of menger — by its sibling import and by its bound mark — draws from props
  // alone: `uses` empty, and no call of a service's on the wire. EDITOR-9: and
  // what draws its diagrams and code beside it: mermaid-diagram (Pele) and the
  // code card's components (code-card, code-paint, a painter per language), EDITOR-14: and its pages' components (m-mockup, m-live, m-page; COMPS-3), each
  // `uses` empty too.
  await test("ticket_view_draws_from_props_with_no_call", async () => {
    const live = () => sy("ui", "versions").find((e) => e.type === "version" && e.live)?.hash;
    const host = `<script module>
  export const uses = [];
</script>
<script>
  import View from "./ticket-view.svelte";
  import Bound from "plexus-ui:ticket-view";
  const md = "The **glaze** fires.\\n\\n<img src=x onerror=alert(1)><script>window.pwned = 1</scr" + "ipt>\\n";
</script>
<main id="host">
  <div id="inlined"><View body={md} title="KILN-1 the kiln" status="Ready"/></div>
  <div id="bare"><View body=""/></div>
  <div id="bound"><Bound body={md} title="KILN-2 the bound" status="Coding"/></div>
</main>
`;
    const alone = bundle(path.dirname(session), "view-alone", {
      engine: v1.engine,
      edit: (ui) => {
        for (const f of fs.readdirSync(path.join(ui, "src"))) if (f !== "ticket-view.svelte" && f !== "mermaid-diagram.svelte" && !["m-mockup.svelte", "m-live.svelte", "m-page.svelte", "m-details.svelte", "tag-components.svelte", "component-def.svelte"].includes(f) && f !== "ticket-links.svelte" && !f.startsWith("code-")) fs.rmSync(path.join(ui, "src", f));
        fs.writeFileSync(path.join(ui, "src", "view-host.svelte"), host);
        fs.writeFileSync(path.join(ui, "routes.json"), JSON.stringify({ "#/": "view-host" }));
        fs.writeFileSync(path.join(ui, "slots.json"), "{}");
      },
    });
    const m = JSON.parse(alone.manifest);
    // COMPS-4: and what draws its tags and says its definitions (tag-components, component-def); EDITOR-22: its folds' line (m-details).
    const drawers = Object.keys(m.components).filter((c) => c === "mermaid-diagram" || ["m-mockup", "m-live", "m-page", "m-details", "tag-components", "component-def"].includes(c) || c === "ticket-links" || c.startsWith("code-"));
    assert(JSON.stringify(Object.keys(m.components).filter((c) => !drawers.includes(c)).sort()) === JSON.stringify(["ticket-view", "view-host"]) && drawers.includes("mermaid-diagram") && ["m-mockup", "m-live", "m-page"].every((c) => drawers.includes(c)) && drawers.includes("code-card") && drawers.includes("code-rust"), `the bundle holds the view, what draws its diagrams and code, and its page: ${Object.keys(m.components)}`);
    // COMPS-4: nothing of menger's called — the only uses are the framework's ui service's (what a tag names,
    // asked only once a tag or a definition is on screen; a definition's save), which every tenant mounts.
    assert(Object.values(m.components).every((c) => c.uses.every((u) => u.startsWith("ui."))), `uses the ui service's alone, every one: ${JSON.stringify(Object.fromEntries(Object.entries(m.components).map(([k, c]) => [k, c.uses])))}`);
    const was = live();
    const answer = push(server, session, path.join(path.dirname(session), "signer.key"), alone);
    assert(answer?.type === "pushed", `pushed: ${JSON.stringify(answer)}`);
    await eventually("the view's bundle is live", async () => live() === alone.version);
    const { page, context, seen } = await openPage("#/");
    try {
      const v = (at) => page.locator(`#${at} [data-surface=ticket-view]`);
      // EDITOR-14: a body holding a script (raw HTML) is drawn whole in a sandboxed frame, its markdown inside it.
      const framed = async (at) => {
        const el = await v(at).locator("[data-surface=page-frame][data-kind=body] iframe[data-buffer=shown]").elementHandle({ timeout: 1000 }).catch(() => null);
        const f = await el?.contentFrame();
        return f ? f.locator("strong").count() : 0;
      };
      await eventually("the view draws", async () => (await framed("inlined")) === 1, 10_000);
      await eventually("the bound one draws", async () => (await framed("bound")) === 1, 10_000);
      assert((await text(v("inlined").locator("[data-field=title]"))) === "KILN-1 the kiln", "the title, from its prop");
      assert((await v("inlined").locator("[data-field=status]").getAttribute("data-status")) === "Ready", "the status, from its prop");
      assert((await page.locator("#bound [data-component=ticket-view][data-slot]").count()) === 1, "the bound one is a section");
      assert((await text(v("bare").locator("[data-field=body][data-empty]"))) === "No body yet.", "no body: says so");
      assert((await v("bare").locator("[data-field=title], [data-field=status]").count()) === 0, "no title, no status: neither drawn");
      assert((await page.locator("[data-ticket-body] script, [data-ticket-body] [onerror]").count()) === 0 && !(await page.evaluate(() => window.pwned)), "the body's script never reaches the app: framed, sandboxed");
      await sleep(300);
      const calls = (await sentCalls(page)).filter((c) => !String(c.method).startsWith("ui."));
      assert(calls.length === 0, `no call: ${calls.map((c) => c.method).join(", ")}`);
      const failed = seen.console.filter((l) => /pageerror/.test(l));
      assert(failed.length === 0, `no page error:\n${failed.join("\n")}`);
    } finally {
      await context.close();
      sy("ui", "rollback", "--to", was);
      await eventually("the version the runner left is live again", async () => live() === was);
    }
  });
};
