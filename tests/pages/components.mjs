// COMPS-4: components defined in tickets, any tag in any ticket drawn by its component. The owner,
// 2026-11-05: "we should be able to refer to any component inside a ticket and define them in tickets,
// which would trigger a save to plexus ui!" — and frames "serve as foundation for rendering other peoples
// ui elements inside your tickets".
//
// - Define in ticket A (a ```component fence, saved by the editor's amend → ui.author), use in ticket B
//   (`<x-…>`); the owner holds ui.author on `*`, so it went into the live bundle and B draws it natively
//   (a bound mark the engine fills, in the page). Edit A: B, open in another page, redraws with the new
//   version, with no reload.
// - A non-editor (an editor of one scope, not `*`) defines one: it lands in their own namespace (signed,
//   never in the live bundle) and the owner's ticket B draws it in a frame — sandboxed, so its script
//   cannot read the page; its calls through the bridge are held to its uses (an html component has none:
//   `plexus.call` refused naming uses). An rhx one of theirs is framed from its kept version.
// - A compile error is said under the definition's block, with its line.
// - A tag nothing names keeps its words and says "no component named x" on hover.
// Run alone by `node tests/pages_browser.mjs --group components`.
//
// Marks: `[data-surface=component-def]` (`data-name`, `data-state=saving|authored|refused|unsaved`,
// `[data-field=line]`), `[data-surface=tag-component]` (`data-name`, `data-mode=native|framed`),
// `[data-tag-name]` on a drawn tag (`data-tag-drawn`, `data-unknown`).
import { makeKey, principalOf, readToken, writeSession } from "../../scripts/throwaway-login.mjs";

const EPIC = "COMP";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, session, server } = ctx;
  const created = (out, what) => {
    const c = out.find((e) => e.type === "created");
    if (!c) throw new Error(`create ${what}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const root = created(sy("tickets", "create", "--epic", EPIC, "--kind", "scope", "--title", "components in tickets"), "root");
  const bodyFile = (name, text) => { const f = path.join(path.dirname(session), `comp-${name}.md`); fs.writeFileSync(f, text); return `@${f}`; };
  let n = 0;
  const make = (title, body) => created(sy("tickets", "create", "--epic", EPIC, "--parent", root, "--title", title, "--body", bodyFile(`${++n}`, body)), title);
  const BODY = "[data-surface=editor-body] [data-field=body]";
  const shots = process.env.PAGES_SHOTS;
  const shoot = async (page, name) => {
    if (!shots) return;
    for (const [w, h] of [[1440, 900], [390, 844]]) for (const theme of ["light", "dark"]) {
      await page.setViewportSize({ width: w, height: h });
      await page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
      await sleep(400);
      await page.screenshot({ path: path.join(shots, `${name}-${w}-${theme}.png`), fullPage: true });
    }
    await page.setViewportSize({ width: 1440, height: 900 });
    await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
  };

  /** The editor on `id` (as `as`, a session token, else the owner), its document replaced by `text`, saved. */
  async function writeInEditor(id, text, as = null) {
    const { context, page } = await openPage(`#/edit/${id}`, { as });
    await eventually("the editor is drawn", async () => (await page.locator(`${BODY} .cm-content`).count()) === 1, 15_000);
    await sleep(300);
    await page.evaluate(({ s, text }) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text }, selection: { anchor: 0 } }); }, { s: BODY, text });
    await eventually("the body saved", async () => (await page.locator("[data-field=write-state]").getAttribute("data-state")) === "saved", 10_000);
    return { context, page };
  }
  /** A definition's line under its block, once it says `state`. */
  const defSays = async (page, name, state, ms = 20_000) => {
    const def = page.locator(`[data-surface=component-def][data-name="${name}"]`).first();
    await eventually(`${name}'s block says ${state}`, async () => (await def.count()) > 0 && (await def.getAttribute("data-state")) === state, ms);
    return (await def.locator("[data-field=line]").textContent()) ?? "";
  };
  const ticketPage = async (id, as = null) => {
    const { context, page } = await openPage(`#/ticket/${id}`, { as });
    await eventually(`#/ticket/${id} draws its body`, async () => (await page.locator("[data-surface=ticket-view] [data-ticket-body]").count()) > 0, 15_000);
    return { context, page };
  };

  const hello = (w) => "```component x-hello\n<script>\n  prop(\"words\");\n</script>\n<p class=\"x-hello\">{\"" + w + " \" + words}</p>\n```\n";

  await test("a_definition_in_one_ticket_draws_in_another_and_redraws_when_edited", async () => {
    const a = make("defines x-hello", "The greeting, defined here.\n");
    const b = make("uses x-hello", "Here it is:\n\n<x-hello words=\"from B\"></x-hello>\n");
    const ed = await writeInEditor(a, `The greeting, defined here.\n\n${hello("one")}`);
    const said = await defSays(ed.page, "x-hello", "authored");
    assert(/live bundle/.test(said), `the owner's definition went into the live bundle: ${said}`);
    const viewB = await ticketPage(b);
    const host = viewB.page.locator("[data-surface=tag-component][data-name=x-hello]");
    await eventually("B draws x-hello natively, its prop given", async () => (await host.getAttribute("data-mode")) === "native" && /one from B/.test((await host.locator(".x-hello").textContent({ timeout: 1000 }).catch(() => "")) ?? ""), 20_000);
    await shoot(viewB.page, "comps4-native");
    const origin = await viewB.page.evaluate(() => ((window.__comps_marker = "kept"), performance.timeOrigin));
    // Edit A's definition: B redraws with the new version, no reload.
    await ed.page.evaluate(({ s }) => { const v = document.querySelector(s).editorView; const t = v.state.doc.toString(); const at = t.indexOf("\"one \""); v.dispatch({ changes: { from: at, to: at + 6, insert: "\"two \"" } }); }, { s: BODY });
    await eventually("A saved again", async () => (await ed.page.locator("[data-field=write-state]").getAttribute("data-state")) === "saved", 10_000);
    await eventually("B redraws with the new version", async () => /two from B/.test((await host.locator(".x-hello").textContent({ timeout: 1000 }).catch(() => "")) ?? ""), 30_000);
    const now = await viewB.page.evaluate(() => ({ m: window.__comps_marker, o: performance.timeOrigin }));
    assert(now.m === "kept" && now.o === origin, "B was not reloaded");
    await ed.context.close();
    await viewB.context.close();
  });

  // An editor of the root alone: ui.author held on a scope, never `*`.
  const keyFile = path.join(path.dirname(session), "scoped.key");
  const scoped = makeKey(keyFile);
  sy("members", "grant", "--principal", scoped, "--scope", `under ${root}`, "--role", "editor");
  const scopedSession = path.join(path.dirname(session), "scoped.session");
  await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, scopedSession, "scoped");
  const scopedToken = readToken(scopedSession);

  const CARD = "```component x-card html\n<div id=\"card\">not yet</div>\n<p id=\"page\">-</p>\n<p id=\"call\">-</p>\n<script>\n"
    + "  document.getElementById('card').textContent = 'ran: ' + (window.props && window.props.label);\n"
    + "  try { document.getElementById('page').textContent = 'read the page: ' + parent.document.title; } catch (e) { document.getElementById('page').textContent = 'page blocked'; }\n"
    + "  plexus.call('tickets.get', { id: (window.props && window.props.ticket) || '' }).then(function (a) { document.getElementById('call').textContent = 'call: ' + (a.ok ? 'answered' : a.message); }, function (e) { document.getElementById('call').textContent = 'call: ' + e; });\n"
    + "</script>\n```\n";
  const NOTE = "```component x-note\n<p class=\"x-note\">a note, framed</p>\n```\n";

  await test("a_non_editors_component_is_framed_and_held_to_its_uses", async () => {
    const a = make("their components", "Theirs.\n");
    const ed = await writeInEditor(a, `Theirs.\n\n${CARD}\n${NOTE}`, scopedToken);
    for (const name of ["x-card", "x-note"]) {
      const said = await defSays(ed.page, name, "authored");
      assert(/your namespace/.test(said), `${name} went to their own namespace: ${said}`);
    }
    const rows = sy("ui", "components", "--names", "x-card", "--names", "x-note");
    assert(rows.length === 2 && rows.every((r) => r.from === "authored" && r.native === false && r.author === scoped), `authored by the scoped editor, framed: ${JSON.stringify(rows)}`);
    await ed.context.close();

    const b = make("uses theirs", `Their card:\n\n<x-card label="Kiln" ticket="${a}"></x-card>\n\nTheir note:\n\n<x-note></x-note>\n`);
    const view = await ticketPage(b);
    const card = view.page.locator("[data-surface=tag-component][data-name=x-card]");
    await eventually("x-card is framed", async () => (await card.getAttribute("data-mode")) === "framed" && (await card.locator("iframe").count()) >= 1, 20_000);
    const sandbox = await card.locator("iframe").first().getAttribute("sandbox");
    assert(sandbox && !sandbox.includes("allow-same-origin"), `sandboxed, an opaque origin: ${sandbox}`);
    const inFrame = async (sel) => {
      for (const f of view.page.frames()) {
        const t = await f.locator(sel).textContent({ timeout: 300 }).catch(() => null);
        if (t != null) return t;
      }
      return null;
    };
    await eventually("its script ran with its props", async () => (await inFrame("#card")) === "ran: Kiln", 15_000);
    assert((await inFrame("#page")) === "page blocked", `it cannot read the page: ${await inFrame("#page")}`);
    await eventually("its call is held to its uses", async () => /uses does not name/.test((await inFrame("#call")) ?? ""), 15_000);
    const note = view.page.locator("[data-surface=tag-component][data-name=x-note]");
    await eventually("x-note (rhx) is framed from its kept version", async () => (await note.getAttribute("data-mode")) === "framed" && (await inFrame(".x-note")) === "a note, framed", 20_000);
    assert(await view.page.evaluate(() => !document.querySelector(".x-note") && !document.getElementById("card")), "nothing of theirs is in the page");
    await shoot(view.page, "comps4-framed");
    await view.context.close();
  });

  await test("a_compile_error_is_said_under_the_block", async () => {
    const a = make("a broken component", "Broken.\n");
    const ed = await writeInEditor(a, "Broken.\n\n```component x-broken\n<div>\n  {#if}\n</div>\n```\n\nAfter.\n");
    const said = await defSays(ed.page, "x-broken", "refused");
    assert(/compile_failed/.test(said) && /line \d+/.test(said), `the compiler's refusal, with its line: ${said}`);
    const view = await ticketPage(a);
    // On the ticket page, after the save on this page: the store holds no x-broken.
    const s2 = await defSays(view.page, "x-broken", "unsaved");
    assert(/not saved/.test(s2), s2);
    await shoot(ed.page, "comps4-refused");
    await view.context.close();
    await ed.context.close();
  });

  await test("a_tag_nothing_names_keeps_its_words", async () => {
    const a = make("an unknown tag", "<x-nobody-here>the words stay</x-nobody-here>\n");
    const view = await ticketPage(a);
    const el = view.page.locator("[data-tag-name=x-nobody-here]");
    await eventually("marked unknown", async () => (await el.getAttribute("data-unknown")) != null, 15_000);
    assert((await el.getAttribute("title")) === "no component named x-nobody-here", `the hover: ${await el.getAttribute("title")}`);
    assert(/the words stay/.test(await el.textContent()), "its words drawn");
    await view.context.close();
  });
};
