// EDITOR-20: a ticket id written in a body is a pill that links to the ticket — on the ticket page, in the
// editor's drawn blocks, on a document and in its margin — made by ticket-view's pass (ticket-pills) from the
// text after the sanitizer: only where the id is prose (never in code, a link, a URL, a fence or a mark's
// attribute), only for a ticket the reader may see (an unknown or unseen id stays its plain words), every id
// of a page in one batched `tickets.list`, live on one `tickets.watch` for the page; the stored body never
// rewritten. Run alone by `node tests/pages_browser.mjs --group pills`.
//
// Marks: `a.tpill[data-pill=<id as asked>][data-status][data-stage]` (its `href` the link helper's, its
// `title` "<id> · <status> — <title>"), `m-ticket[data-pilled]`, ticket-view's `[data-ticket-body]` and
// `.blk`, the editor's `[data-surface=editor-body]`, doc-margin's `[data-thread] .md`.
import { makeKey, readToken, writeSession } from "../../scripts/throwaway-login.mjs";
import { sentCalls } from "../../scripts/ui-throwaway.mjs";

const EPIC = "PILL";
const PAUSE = 600;

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, summaries, fs, path, session, server, ticketAt } = ctx;
  const bodyFile = (name, text) => { const f = path.join(path.dirname(session), `pills-${name}.md`); fs.writeFileSync(f, text); return `@${f}`; };
  const createdId = (out, what) => {
    const c = out.find((e) => e.type === "created" || e.ticket?.id);
    if (!c) throw new Error(`create ${what}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const get = (id) => sy("tickets", "get", "--id", id).find((e) => e.type === "ticket")?.ticket;
  const root = createdId(sy("tickets", "create", "--epic", EPIC, "--kind", "scope", "--title", "the pills' tickets"), "root");
  const make = (title, body = "") => createdId(sy("tickets", "create", "--epic", EPIC, "--parent", root, "--title", title, ...(body ? ["--body", bodyFile(title.replace(/\W+/g, "-"), body)] : [])), title);
  const a = make("the kiln");
  const b = make("the glaze");
  const sub = createdId(sy("tickets", "subtasks", "create", "--parent", b, "--title", "load the shelves"), "a subtask");
  // A scope rooting a shortcode of its own under the pills' root: an address names it `PILL/PNEST-1`.
  const c = createdId(sy("tickets", "create", "--epic", "PNEST", "--kind", "scope", "--parent", root, "--title", "the shelves"), "a nested scope");
  // An address of c: the shortcode it is hosted under, then its id.
  const cPath = String(get(c).address).replace(/^[^:]*:/, "").split("/");
  const cAddr = `${cPath[cPath.length - 2]}/${c}`;
  // Outside the pills' tree: what a viewer of that tree alone cannot see.
  const secret = createdId(sy("tickets", "create", "--epic", "PSEC", "--kind", "scope", "--title", "kept apart"), "the secret");

  /** The body every case draws: ids in prose (a bare one, a subtask, an address, an m-ticket), and the same
   *  ids where they are not prose — code, a link's text and target, a URL, a fence, a mermaid fence, a mark's
   *  attribute — and words shaped like ids that name no ticket. */
  const PROSE = [a, sub, cAddr, secret];
  const BODY = `The kiln is ${a}; its shelves are ${sub}, and ${cAddr} holds the glaze. See ${secret}.

Not tickets: UTF-8, SHA-256, ISO-8601 and NOPE-404.

In code \`${a}\`, in [a link to ${b}](https://example.com/${b}), and a bare https://example.com/${b} URL.

\`\`\`text
${a} in a fence
\`\`\`

\`\`\`mermaid
graph TD
  X["${a}"] --> Y["${b}"]
\`\`\`

<m-status data-field="status">

<m-step date="2026-11-10T09:00Z" status="Ready" by="${b}">a step whose author attribute is an id</m-step>

</m-status>

<m-field data-field="detail">A mark: <m-ticket ref="${b}"></m-ticket> and <m-ticket ref="NOPE-404"></m-ticket>.</m-field>
`;
  const host = make("names the others", BODY);

  /** The pills a scope holds: each `{ pill, text, href, status, title }`. */
  const pillsIn = (page, scope) => page.locator(`${scope} a.tpill[data-pill]`).evaluateAll((els) => els.map((e) => ({
    pill: e.getAttribute("data-pill"), text: e.firstChild?.textContent ?? "", href: e.getAttribute("href"), status: e.getAttribute("data-status"), title: e.getAttribute("title"),
    mark: e.parentElement?.localName === "m-ticket", where: e.closest("code, pre, a:not(.tpill), m-step") ? "inside" : "prose",
  })));
  const ticketPage = async (id, as = null) => {
    const opened = await openPage(`#/ticket/${id}`, { as });
    await eventually(`#/ticket/${id} draws its body`, async () => (await opened.page.locator("[data-surface=ticket-body] [data-ticket-body]").count()) > 0, 15_000);
    return opened;
  };
  const listCalls = async (page) => (await sentCalls(page)).filter((x) => x.method === "tickets.list" && /\bticket \S/.test(String(x.params.filter ?? "")) && !String(x.params.filter).includes("00000000"));
  const shots = process.env.PILLS_SHOTS || process.env.PAGES_SHOTS;
  const shoot = async (page, name) => {
    if (!shots) return;
    fs.mkdirSync(shots, { recursive: true });
    for (const [w, h] of [[1440, 900], [390, 844]]) for (const theme of ["light", "dark"]) {
      await page.setViewportSize({ width: w, height: h });
      await page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
      await sleep(300);
      await page.screenshot({ path: path.join(shots, `${name}-${w}-${theme}.png`), fullPage: true });
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
  };

  await test("a_ticket_id_in_prose_is_a_pill_and_nowhere_else", async () => {
    const { context, page, seen } = await ticketPage(host);
    try {
      const scope = "[data-surface=ticket-body]";
      let got = [];
      await eventually("the prose ids and the m-ticket are pills", async () => { got = await pillsIn(page, scope); return got.length === PROSE.length + 1; }, 10_000)
        .catch((e) => { throw new Error(`${e.message}: ${JSON.stringify(got)}`); });
      await sleep(300); // nothing more comes
      got = await pillsIn(page, scope);
      const prose = got.filter((p) => !p.mark).map((p) => p.text);
      assert(JSON.stringify(prose) === JSON.stringify(PROSE), `pills for the prose ids alone, in order: ${JSON.stringify(prose)}`);
      assert(got.every((p) => p.where === "prose"), `no pill inside code, a link, a fence or a mark: ${JSON.stringify(got)}`);
      const mark = got.find((p) => p.mark);
      assert(mark?.pill === b && (await page.locator(`${scope} m-ticket[data-pilled]`).count()) === 1, "the m-ticket the reader may see is the same pill, the unknown one stays its ref");
      // The href is the link helper's: by uid, `#/t/<uid>/<ID>-<slug>`.
      for (const p of got) {
        const t = get(p.pill.split("/").pop());
        assert(new RegExp(`^#/t/${t.uid}/${t.id}(-[a-z0-9-]+)?$`).test(p.href), `${p.text}'s href is by its uid: ${p.href}`);
        assert(p.status === t.status && p.title === `${t.id} · ${t.status} — ${t.title}`, `${p.text} says its stage and title: ${JSON.stringify(p)}`);
      }
      // Not prose, and not a ticket: plain words, and nothing said about the lookup.
      const text = await page.locator(`${scope} [data-ticket-body]`).innerText();
      for (const w of ["UTF-8", "SHA-256", "ISO-8601", "NOPE-404"]) assert(text.includes(w), `${w} stays in the text`);
      assert((await page.locator(`${scope} a.tpill[data-pill=NOPE-404], ${scope} [data-refused]`).count()) === 0, "an unknown id makes no pill and nothing refused");
      assert((await page.locator(`${scope} code a.tpill, ${scope} pre a.tpill, ${scope} a:not(.tpill) a.tpill`).count()) === 0, "nothing in code or a link");
      const calls = await listCalls(page);
      assert(calls.length === 1, `one batched read for the body: ${JSON.stringify(calls.map((x) => x.params.filter))}`);
      assert(!seen.console.some((l) => /^(error|pageerror)/.test(l)), `no error on the page: ${seen.console.filter((l) => /^(error|pageerror)/.test(l)).join(" | ")}`);
      // The stored body is as it was.
      assert(get(host).body === BODY, "the stored body untouched");
      await shoot(page, "pills-ticket");
      // A pill goes to its ticket.
      await page.locator(`${scope} a.tpill[data-pill="${a}"]`).first().click();
      await eventually(`the pill opens ${a}`, async () => ticketAt(await page.evaluate(() => location.hash), a), 5_000);
    } finally {
      await context.close();
    }
  });

  await test("an_id_the_reader_cannot_see_stays_plain", async () => {
    // A viewer of the pills' tree alone: the secret, outside it, is words; the rest are pills.
    const keyFile = path.join(path.dirname(session), "pills-viewer.key");
    const viewer = makeKey(keyFile);
    sy("members", "grant", "--principal", viewer, "--scope", `under ${root}`, "--role", "viewer");
    const viewerSession = path.join(path.dirname(session), "pills-viewer.session");
    await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, viewerSession, "viewer");
    const { context, page } = await ticketPage(host, readToken(viewerSession));
    try {
      const scope = "[data-surface=ticket-body]";
      await eventually("the seen ids are pills", async () => (await pillsIn(page, scope)).length === PROSE.length, 10_000);
      await sleep(300);
      const got = (await pillsIn(page, scope)).map((p) => p.text);
      assert(!got.includes(secret) && got.length === PROSE.length, `the unseen ${secret} is no pill: ${JSON.stringify(got)}`);
      assert((await page.locator(`${scope} [data-ticket-body]`).innerText()).includes(`See ${secret}.`), "its words stay");
    } finally {
      await context.close();
    }
  });

  await test("a_pill_follows_its_ticket_live", async () => {
    const live = make("one that moves", `Watch ${c} move.\n`);
    const { context, page } = await ticketPage(live);
    try {
      const scope = "[data-surface=ticket-body]";
      const pill = (id) => page.locator(`${scope} a.tpill[data-pill="${id}"]`);
      await eventually("the pill", async () => (await pill(c).count()) === 1, 10_000);
      const watches = async () => (await sentCalls(page)).filter((x) => x.method === "tickets.watch").length;
      const before = await watches();
      const was = await pill(c).getAttribute("data-status");
      sy("tickets", "set_status", "--id", c, "--status", "Pending");
      await eventually(`${c}'s pill says Pending (was ${was})`, async () => (await pill(c).getAttribute("data-status")) === "Pending", 5_000);
      sy("tickets", "amend", "--id", c, "--title", "the shelves, renamed");
      await eventually(`${c}'s pill says its new title`, async () => /the shelves, renamed$/.test((await pill(c).getAttribute("title")) ?? ""), 5_000);
      assert((await watches()) === before, "no new stream for a pill's change");
      assert((await listCalls(page)).length === 1, "and no new read");
    } finally {
      await context.close();
    }
  });

  await test("sixty_ids_cost_one_read_and_one_watch", async () => {
    // A document holding a table of sixty ids, and ten paragraphs naming one each (ten more blocks, each its
    // own ticket-view in the editor): every id of the page in one read.
    const ids = summaries("--limit", "200", "--fields", "id,title,kind,status").map((s) => s.id).filter((id) => !id.includes(".")).slice(0, 60);
    assert(ids.length === 60, `sixty tickets to name: ${ids.length}`);
    const text = ids.slice(0, 10).map((id, k) => `Paragraph ${k + 1} names ${id}.`).join("\n\n") + "\n\n| # | ticket |\n| -- | -- |\n" + ids.map((id, k) => `| ${k + 1} | ${id} |`).join("\n") + "\n";
    const docId = sy("docs", "create", "--title", "sixty names", "--body", bodyFile("sixty", text)).find((e) => e.type === "created")?.doc.id;
    // The same page with one id: the watches it holds are the page's own, and sixty ids hold no more.
    const oneId = sy("docs", "create", "--title", "one name", "--body", bodyFile("one", `Names ${ids[0]}.\n`)).find((e) => e.type === "created")?.doc.id;
    const watchesOf = async (page) => (await sentCalls(page)).filter((x) => x.method === "tickets.watch").length;
    const one = await openPage(`#/doc/${oneId}`);
    await eventually("one pill", async () => (await one.page.locator("[data-surface=editor-body] a.tpill[data-pill]").count()) === 1, 15_000);
    await sleep(400);
    const w1 = await watchesOf(one.page);
    await one.context.close();
    const { context, page } = await openPage(`#/doc/${docId}`);
    try {
      const scope = "[data-surface=editor-body]";
      await eventually("seventy pills", async () => (await page.locator(`${scope} a.tpill[data-pill]`).count()) === 70, 15_000);
      await sleep(400);
      const calls = await listCalls(page);
      assert(calls.length === 1, `one batched read for sixty ids in sixty blocks: ${calls.length}`);
      const all = await sentCalls(page);
      const watches = all.filter((x) => x.method === "tickets.watch");
      // EDITOR-21: the read is the collection's snapshot, the watch its changes: sixty ids hold what one does.
      assert(watches.length === w1, `one watch for the page's pills, never one a pill: ${watches.length} watches (one id: ${w1})`);
    } finally {
      await context.close();
    }
  });

  await test("a_ticket_and_a_doc_with_one_body_draw_the_same", async () => {
    const docId = sy("docs", "create", "--title", "the same names", "--body", bodyFile("same", BODY)).find((e) => e.type === "created")?.doc.id;
    /** Each drawn block's HTML, Svelte's anchors and a bound mark's fill aside. */
    const drawnHtml = (page, scope) => page.locator(`${scope} .blk`).evaluateAll((els) => els.map((e) => {
      const c = e.cloneNode(true);
      for (const m of c.querySelectorAll("[data-component]")) { m.removeAttribute("data-layout"); m.replaceChildren(); }
      return c.innerHTML.replace(/<!--[^]*?-->/g, "");
    }));
    const settle = async (page, scope) => {
      await eventually(`${scope}: every pill`, async () => (await page.locator(`${scope} a.tpill[data-pill]`).count()) === PROSE.length + 1, 15_000);
      await eventually(`${scope}: the diagram drawn`, async () => (await page.locator(`${scope} [data-surface=mermaid-diagram][data-state=drawn]`).count()) === 1, 15_000);
      await sleep(300);
      return drawnHtml(page, scope);
    };
    const t = await ticketPage(host);
    const asTicket = await settle(t.page, "[data-surface=ticket-body]");
    await t.context.close();
    const d = await openPage(`#/doc/${docId}`);
    try {
      const asDoc = await settle(d.page, "[data-surface=editor-body]");
      assert(asTicket.length === asDoc.length && asTicket.length > 5, `as many blocks: ${asTicket.length} · ${asDoc.length}`);
      for (let k = 0; k < asTicket.length; k++) assert(asTicket[k] === asDoc[k], `block ${k} the same:\n  ticket ${asTicket[k].slice(0, 300)}\n  doc    ${asDoc[k].slice(0, 300)}`);
      await shoot(d.page, "pills-doc");
    } finally {
      await d.context.close();
    }
  });

  await test("in_the_editor_the_id_stays_text_where_the_caret_is", async () => {
    const id = make("edits around a pill", `First names ${a}.\n\nSecond names ${b}.\n`);
    const { context, page } = await openPage(`#/edit/${id}`);
    try {
      const ed = page.locator("[data-surface=editor-body]");
      await eventually("both pills drawn", async () => (await ed.locator("a.tpill[data-pill]").count()) === 2, 15_000);
      // The caret into the first block: its line is the markdown, the id plain text; the second still a pill.
      await page.evaluate(() => { const v = document.querySelector("[data-surface=editor-body] [data-field=body]").editorView; v.focus(); v.dispatch({ selection: { anchor: 3 } }); });
      await eventually("the caret's block is its source", async () => (await ed.locator(".cm-line").first().innerText()).includes(`First names ${a}.`), 5_000);
      assert((await ed.locator(".cm-line a.tpill").count()) === 0, "no pill in the caret's line");
      assert((await ed.locator(`.blk a.tpill[data-pill="${b}"]`).count()) === 1, "the other block's id is still a pill");
      await sleep(PAUSE + 200);
      const amends = (await sentCalls(page)).filter((x) => x.method === "tickets.amend");
      assert(amends.length === 0 && get(id).body === `First names ${a}.\n\nSecond names ${b}.\n`, "the stored markdown never rewritten");
    } finally {
      await context.close();
    }
  });

  await test("in_the_editor_a_click_on_a_pill_opens_its_ticket", async () => {
    // EDITOR-23: a drawn block takes a click to put the caret in it — but a pill's click follows its link.
    const text = `First names ${a}.\n\nSecond has [a link](https://example.invalid/) in it.\n`;
    const id = make("opens from the editor", text);
    const { context, page } = await openPage(`#/edit/${id}`);
    try {
      const ed = page.locator("[data-surface=editor-body]");
      await eventually("the pill drawn", async () => (await ed.locator(`.blk a.tpill[data-pill="${a}"]`).count()) === 1, 15_000);
      // Any other link is as it was: the click enters its block, and nothing is followed.
      const here = await page.evaluate(() => location.hash);
      await ed.locator(".blk a[href^='https://example.invalid']").click();
      await eventually("the link's block is its source", async () => (await ed.locator(".cm-line").evaluateAll((ls) => ls.some((l) => l.innerText.includes("[a link]")))), 5_000);
      assert((await page.evaluate(() => location.hash)) === here, "a link in a block is not followed");
      await ed.locator(`.blk a.tpill[data-pill="${a}"]`).click();
      await eventually(`the pill opens ${a}`, async () => ticketAt(await page.evaluate(() => location.hash), a), 5_000);
      await sleep(PAUSE + 200);
      assert(get(id).body === text, "the stored markdown never rewritten");
    } finally {
      await context.close();
    }
  });

  await test("a_margin_question_draws_its_ids_as_pills", async () => {
    const text = "The kiln fires twice a week.\n";
    const docId = sy("docs", "create", "--title", "asked about", "--body", bodyFile("margin", text)).find((e) => e.type === "created")?.doc.id;
    sy("docs", "ask", "--id", docId, "--from", "4", "--to", "8", "--text", `Is this ${a}, or \`${b}\`?`);
    const { context, page } = await openPage(`#/doc/${docId}`);
    try {
      const card = page.locator("[data-field=margin] [data-thread] .md").first();
      await eventually("the question's id is a pill", async () => (await card.locator(`a.tpill[data-pill="${a}"]`).count()) === 1, 15_000);
      assert((await card.locator("a.tpill").count()) === 1, "the id in code is not");
    } finally {
      await context.close();
    }
  });
};
