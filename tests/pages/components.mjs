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
    const { context, page, seen } = await openPage(`#/ticket/${id}`, { as });
    await eventually(`#/ticket/${id} draws its body`, async () => (await page.locator("[data-surface=ticket-view] [data-ticket-body]").count()) > 0, 15_000);
    return { context, page, seen };
  };
  // COMPS-5: a page's effects never loop — the tag table's listeners ran inside its effect, and the
  // definition line's `tick++` made that effect depend on what it wrote (`effect_update_depth_exceeded`,
  // Svelte's flush aborted, the effects queued behind it dropped: a tag left undrawn by the order things mounted).
  const noLoop = (seen, where) => {
    const looped = seen.console.filter((l) => /effect_update_depth_exceeded/.test(l));
    assert(!looped.length, `${where}: no effect loops (${looped.length} effect_update_depth_exceeded)`);
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

  // COMPS-5: v1's format — a fence with no `html` whose source is v1's custom element (a <template>, its
  // <style>, a <script> run with the element as `this`), words after the name (v1's preview attributes) — in
  // a body written straight to the store, as v1's were, and registered as the server's backfill does (one
  // `ui.author` of plexus-ui's element/1, own: true). Its ticket and another using its tag draw it in
  // plexus-ui's frame: an open shadow root, its attribute a prop, the page's theme through
  // `this.menger.on('theme')`, and its cleanup run before it draws again.
  const FILM = "<template>\n<div class=\"box\" part=\"box\"><b class=\"tok\">-</b> <i class=\"who\">-</i></div>\n</template>\n"
    + "<style>:host{display:block}.box{background:var(--panel);color:var(--ink);padding:8px;border:1px solid var(--line);font:13px var(--font-text)}</style>\n"
    + "<docs><summary>a v1 component, for COMPS-5</summary></docs>\n<script>\n"
    + "const tok = this.root.querySelector('.tok'), who = this.root.querySelector('.who');\n"
    + "const paint = () => { tok.textContent = getComputedStyle(this).getPropertyValue('--bg').trim() + '|' + this.menger.theme.scheme; };\n"
    + "who.textContent = 'label=' + this.attr('label');\n"
    + "this.menger.on('theme', paint);\npaint();\nwindow.__drawn = (window.__drawn || 0) + 1;\n"
    + "return () => { window.__cleaned = (window.__cleaned || 0) + 1; };\n</script>\n";

  await test("a_v1_definition_draws_in_plexus_uis_frame_and_follows_the_theme", async () => {
    const a = make("defines x-film in v1's format", "The film, defined the v1 way.\n\n```component x-film data-field=film\n" + FILM + "```\n\nAnd used:\n\n<x-film label=\"Kiln\"></x-film>\n");
    const b = make("uses x-film", "Elsewhere:\n\n<x-film label=\"Glaze\"></x-film>\n");
    const params = path.join(path.dirname(session), "comp-film-author.json");
    fs.writeFileSync(params, JSON.stringify({ sources: [{ name: "x-film", source: FILM, profile: "element/1" }], own: true }));
    // `-p` is synapse's global option, before the service's name: the call `sy` makes, with it there.
    const { spawnSync } = await import("node:child_process");
    const r = spawnSync("synapse", ["--no-hold", "--token-file", session, "-j", "-p", `@${params}`, "-P", String(server.ws), "menger", "ui", "author"],
      { env: { ...process.env, XDG_CACHE_HOME: path.join(server.work, "cache") }, encoding: "utf8" });
    assert(/"type":\s*"authored"/.test(r.stdout ?? ""), `registered as element/1: ${r.stdout} ${r.stderr}`);
    const view = await ticketPage(a);
    const said = await defSays(view.page, "x-film", "authored");
    assert(/drawn framed/.test(said), `its block says it is saved, framed: ${said}`);
    assert((await view.page.locator("[data-surface=component-def][data-name=x-film]").getAttribute("data-v1")) != null, "read as v1's element");
    const host = view.page.locator("[data-surface=tag-component][data-name=x-film]");
    await eventually("x-film is framed", async () => (await host.getAttribute("data-mode")) === "framed" && (await host.locator("iframe").count()) >= 1, 20_000);
    const film = async (page) => {
      for (const f of page.frames()) if (await f.locator("x-film").count().catch(() => 0)) return f;
      return null;
    };
    const text = async (page, sel) => (await (await film(page))?.locator(sel).textContent({ timeout: 300 }).catch(() => null)) ?? null;
    await eventually("its template drew in its shadow root, its attribute given", async () => (await text(view.page, ".who")) === "label=Kiln", 15_000);
    const f = await film(view.page);
    assert(await f.evaluate(() => document.querySelector("x-film").shadowRoot?.mode === "open"), "an open shadow root");
    assert(await f.evaluate(() => !document.querySelector("docs") && !document.querySelector("x-film").shadowRoot.querySelector("docs")), "its <docs> drawn nowhere");
    const lightBg = await view.page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--bg").trim());
    await eventually("it painted with the page's light tokens", async () => (await text(view.page, ".tok")) === `${lightBg}|light`, 10_000);
    await view.page.evaluate(() => (document.documentElement.dataset.theme = "dark"));
    const darkBg = await view.page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--bg").trim());
    assert(darkBg !== lightBg, `the page's --bg moved: ${lightBg} → ${darkBg}`);
    await eventually("on('theme') heard the flip and it repainted", async () => (await text(view.page, ".tok")) === `${darkBg}|dark`, 10_000);
    await view.page.evaluate(() => (document.documentElement.dataset.theme = "light"));
    await eventually("and back", async () => (await text(view.page, ".tok")) === `${lightBg}|light`, 10_000);
    await f.evaluate(() => document.querySelector("x-film").setAttribute("label", "Clay"));
    await eventually("it drew again with the new attribute", async () => (await text(view.page, ".who")) === "label=Clay", 5_000);
    const runs = await f.evaluate(() => ({ drawn: window.__drawn, cleaned: window.__cleaned }));
    assert(runs.drawn === 2 && runs.cleaned === 1, `the cleanup ran before the second draw: ${JSON.stringify(runs)}`);
    assert(await view.page.evaluate(() => !document.querySelector(".tok")), "nothing of it is in the page");
    noLoop(view.seen, "the defining ticket");
    await shoot(view.page, "comps5-v1");
    await view.context.close();
    const other = await ticketPage(b);
    await eventually("another ticket's tag draws it too", async () => (await text(other.page, ".who")) === "label=Glaze", 20_000);
    noLoop(other.seen, "the ticket using it");
    await other.context.close();
  });

  // COMPS-5, the owner (2026-11-05): "all tickets are the same surface. doc or not. they all need the exact same
  // rendering capabilites". One body, opened as a doc (the docs service's, `#/doc/<id>`, drawn by the editor)
  // and as a ticket (`#/ticket/<id>`): both draw the same things — the house's card, a code card, a definition's
  // line (v1's element), the tag drawn framed by plexus-ui, raw HTML — through the one renderer, ticket-view.
  await test("a_body_draws_the_same_as_a_doc_and_as_a_ticket", async () => {
    const SAME = "Intro.\n\n<m-summary>\n\n<m-answer data-field=\"answer\">**One surface.**</m-answer>\n\n</m-summary>\n\n"
      + "```rust\nfn main() {}\n```\n\n```component x-film\n" + FILM + "```\n\n<x-film label=\"Same\"></x-film>\n\nA line with <b>raw HTML</b> the sanitizer keeps.\n";
    const t = make("one body, as a ticket", SAME);
    const docId = sy("docs", "create", "--title", "one body, as a doc", "--body", bodyFile("same-doc", SAME)).find((e) => e.type === "created")?.doc.id;
    assert(docId, "the doc made");
    const drawn = async (page) => page.evaluate(() => ({
      card: document.querySelectorAll("m-summary").length > 0,
      code: [...document.querySelectorAll("[data-surface=code-card]")].map((c) => c.dataset.lang).sort(),
      defs: [...document.querySelectorAll("[data-surface=component-def]")].map((d) => `${d.dataset.name}:${d.hasAttribute("data-v1") ? "v1" : "-"}`).sort(),
      tags: [...document.querySelectorAll("[data-surface=tag-component]")].map((h) => `${h.dataset.name}:${h.dataset.mode}:${h.querySelectorAll("iframe").length}`).sort(),
      raw: [...document.querySelectorAll("[data-surface=ticket-view] b, [data-surface=editor-body] b")].some((b) => b.textContent === "raw HTML"),
    }));
    const settled = async (page, what) => {
      let got = null;
      await eventually(`${what} draws the film`, async () => { got = await drawn(page); return got.tags.length === 1 && got.tags[0] === "x-film:framed:1"; }, 20_000)
        .catch((e) => { throw new Error(`${e.message}: drew ${JSON.stringify(got)}`); });
      return got;
    };
    const asTicket = await ticketPage(t);
    const ticketDrew = await settled(asTicket.page, "the ticket");
    noLoop(asTicket.seen, "the ticket");
    const asDoc = await openPage(`#/doc/${docId}`);
    await eventually("the doc's editor is drawn", async () => (await asDoc.page.locator(`${BODY} .cm-content`).count()) === 1, 15_000);
    const docDrew = await settled(asDoc.page, "the doc");
    noLoop(asDoc.seen, "the doc");
    assert(JSON.stringify(ticketDrew) === JSON.stringify(docDrew), `the same drawing: ticket ${JSON.stringify(ticketDrew)} · doc ${JSON.stringify(docDrew)}`);
    assert(ticketDrew.card && ticketDrew.raw && ticketDrew.code.includes("rust") && ticketDrew.defs.includes("x-film:v1"), `every capability drawn: ${JSON.stringify(ticketDrew)}`);
    await asTicket.context.close();
    await asDoc.context.close();
  });

  // COMPS-8, the owner (2026-11-05): "we want to hide the code of the definition of a component." A
  // definition's code card is closed: its line names it, says its save, and `source` opens and closes it;
  // any other fence's card stays drawn, and the tag draws as before — on a ticket and on a doc alike.
  await test("a_definitions_source_is_hidden_until_asked", async () => {
    const BODY8 = "Before.\n\n```rust\nfn main() {}\n```\n\n```component x-film\n" + FILM + "```\n\n<x-film label=\"Hidden\"></x-film>\n\nAfter.\n";
    const t = make("a definition, hidden", BODY8);
    const docId = sy("docs", "create", "--title", "a definition, hidden, as a doc", "--body", bodyFile("hidden-doc", BODY8)).find((e) => e.type === "created")?.doc.id;
    assert(docId, "the doc made");
    const SRC = "[data-surface=component-def][data-name=x-film] [data-field=source]";
    const sourceShown = (page) => page.evaluate(() => [...document.querySelectorAll(".code.def-src")].some((c) => c.offsetParent !== null));
    const check = async (page, what) => {
      await eventually(`${what}: x-film's line drawn`, async () => (await page.locator(SRC).count()) === 1, 20_000);
      await eventually(`${what}: the film drawn`, async () => (await page.locator("[data-surface=tag-component][data-name=x-film] iframe").count()) === 1, 20_000);
      assert(!(await sourceShown(page)), `${what}: the definition's source is hidden on load`);
      assert(!(await page.evaluate(() => document.body.innerText.includes("this.root.querySelector"))), `${what}: none of its code shows`);
      assert(await page.evaluate(() => [...document.querySelectorAll("[data-surface=code-card]")].some((c) => c.dataset.lang === "rust" && c.offsetParent !== null)), `${what}: another fence's card still drawn`);
      assert((await page.locator(SRC).getAttribute("aria-expanded")) === "false", `${what}: closed`);
      await page.locator(SRC).click();
      await eventually(`${what}: source opens it`, async () => await sourceShown(page), 5_000);
      assert((await page.locator(SRC).getAttribute("aria-expanded")) === "true", `${what}: open`);
      await page.locator(SRC).click();
      await eventually(`${what}: and closes it`, async () => !(await sourceShown(page)), 5_000);
    };
    const asTicket = await ticketPage(t);
    await check(asTicket.page, "the ticket");
    noLoop(asTicket.seen, "the ticket");
    await shoot(asTicket.page, "comps8-hidden");
    await asTicket.context.close();
    const asDoc = await openPage(`#/doc/${docId}`);
    await eventually("the doc's editor is drawn", async () => (await asDoc.page.locator(`${BODY} .cm-content`).count()) === 1, 15_000);
    await check(asDoc.page, "the doc");
    await asDoc.context.close();
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
