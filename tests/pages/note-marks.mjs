// NOTES-5: what an agent leaves on a note — a mark (`notes.mark`) on a range
// of its text: the words underlined on every open page (in the accent when
// the mark carries a link, which a click opens), its line of state in the
// mono face under the block that holds it, with who left it, and a dot in
// that block's margin; the underline follows its words as the note is
// typed; every word deleted, it is drawn nowhere. The note's text is never
// written by a mark. Run alone by `node tests/pages_browser.mjs --group note-marks`.
//
// Marks: a drawn block `[data-marked]`; its underline `.nm-u[data-mark=<id>]`
// (`.nm-linked`, `[data-mark-link]`, `role=link`); a line of state
// `[data-mark-state=<id>]`; in the caret's block CodeMirror's own
// `.nm-u[data-mark]`, the line `.nm-dot` and `.nm-states`.
//
// And a component a mark places (`notes.mark { component, version, props }`),
// drawn under the block that holds the range, never in the text: a real
// component, node-legend, authored into the bundle `notes` (as
// tests/pages/slot.mjs authors graph-nodes), drawn on every open page and
// staying under its passage as the text moves; its props patched
// (`notes.mark_props`, by the component itself or an agent) reach every
// page's mounted component in place; typing in the passage above it moves
// no caret and mounts nothing again; its words deleted, it is drawn nowhere.
// Marks: `.nm-placed[data-placed=<mark>]` (`[data-field=component]`,
// `[data-field=placed-by]`), its bound mark `[data-slot=mark-<mark>]
// [data-component][data-version]`, and the component's own `[data-legend]
// [data-self]`, `[data-field=stopped]`, `[data-action=hollow]`.
//
// PAGES_SHOTS=DIR: a marked note at 1440 and 390, light and dark (`note-marked-*`),
// and one with a component under a passage (`note-placed-*`).
import { spawnSync } from "node:child_process";

/** node-legend: running and (when its props say `hollow`) stopped; its button patches its own props. */
const LEGEND = `<script module>
  export const uses = ["notes.mark_props"];
</script>

<script>
  import { bus } from "plexus-ui/host";
  let { note = "", mark = "", props = "{}" } = $props();
  const b = bus();
  const self = Math.random().toString(36).slice(2, 10);
  const p = $derived.by(() => { try { return JSON.parse(props || "{}"); } catch { return {}; } });
  const hollow = $derived(p.hollow === true);
  const toggle = () => b.call("notes.mark_props", { id: note, mark, patch: { hollow: !hollow } });
</script>

<div class="legend" data-legend data-self={self}>
  <span><i></i>running</span>
  {#if hollow}<span data-field="stopped"><i class="hollow"></i>stopped</span>{/if}
  <button type="button" data-action="hollow" aria-pressed={hollow} onclick={toggle}>show stopped</button>
</div>

<style>
  .legend { display: flex; gap: 20px; align-items: center; padding: 12px 14px; font: 14px var(--sans); color: var(--ink); }
  span { display: inline-flex; gap: 7px; align-items: center; }
  i { display: inline-block; width: 12px; height: 12px; background: var(--ink); border-radius: 50%; }
  i.hollow { background: transparent; border: 1.5px dashed var(--ink); }
  button { margin-left: auto; padding: 3px 9px; font: 12px var(--mono); color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  button[aria-pressed=true] { color: var(--panel); background: var(--ink); border-color: var(--ink); }
</style>
`;

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, session, ticketAt, server } = ctx;
  const BODY = "[data-surface=editor-body] [data-field=body]";
  const scratch = path.dirname(session);
  const file = (name, text) => { const f = path.join(scratch, `note-marks-${name}`); fs.writeFileSync(f, text); return `@${f}`; };
  const note = (title, body) => sy("notes", "create", "--title", title, "--body", file(title.replace(/\W+/g, "-"), body)).find((e) => e.type === "created").note.id;
  const get = (id) => sy("notes", "get", "--id", id).find((e) => e.type === "note")?.note;
  const marks = (id) => sy("notes", "marks", "--id", id).filter((e) => e.type === "mark").map((e) => e.mark);
  const mark = (id, from, to, ...flags) => {
    const out = sy("notes", "mark", "--id", id, "--from", String(from), "--to", String(to), ...flags);
    const m = out.find((e) => e.type === "marked");
    if (!m) throw new Error(`notes.mark: ${JSON.stringify(out)}`);
    return m.mark;
  };
  const openNote = async (id) => {
    const { context, page } = await openPage(`#/note/${id}`);
    const ed = page.locator(`[data-surface=editor-body][data-kind=note][data-ticket="${id}"]`);
    await eventually("the note is drawn", async () => (await ed.locator(".cm-content[data-field=body-source]").count()) === 1, 10_000);
    return { context, page, ed };
  };
  const caretAt = (page, pos) => page.evaluate(({ s, pos }) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: pos } }); }, { s: BODY, pos });
  const blur = (page) => page.evaluate((s) => document.querySelector(s).editorView.contentDOM.blur(), BODY);
  const underline = (ed, m) => ed.locator(`.nm-u[data-mark="${m}"]`);
  const underlined = async (ed, m) => (await underline(ed, m).allInnerTexts()).join("");

  const SERVICE_MAP = "Draw every service as a node on a graph.\n\nLet me connect two nodes by clicking one, then the other; clicking a line takes it away.\n\nThe names come from the service registry, so a new service shows up without anyone drawing it.\n\nA stopped service stays, drawn hollow.\n";
  const at = (text, words) => [SERVICE_MAP.indexOf(words), SERVICE_MAP.indexOf(words) + words.length];

  await test("a_sentence_became_a_ticket_underlined_on_every_page_and_its_link_opens", async () => {
    const ticket = sy("tickets", "create", "--epic", "KILN", "--kind", "scope", "--title", "the service map draws every service as a node").find((e) => e.type === "created").ticket.id;
    const id = note("The service map", SERVICE_MAP);
    const one = await openNote(id);
    const two = await openNote(id);
    try {
      await sleep(500);
      const before = get(id);
      const [a, z] = at(SERVICE_MAP, "Draw every service as a node on a graph.");
      const sent = Date.now();
      const m = mark(id, a, z, "--link", ticket, "--label", `filed ${ticket}`);
      for (const [k, { ed }] of [one, two].entries()) {
        await eventually(`page ${k + 1}: the words underlined`, async () => (await underlined(ed, m.mark)) === "Draw every service as a node on a graph.", 2_000);
      }
      assert(Date.now() - sent < 2_500, `on every open page within two seconds (${Date.now() - sent} ms)`);
      for (const { ed } of [one, two]) {
        const u = underline(ed, m.mark).first();
        assert((await u.getAttribute("data-mark-link")) === ticket && (await u.getAttribute("role")) === "link", "linked");
        const [colour, accent] = await u.evaluate((e) => [getComputedStyle(e).color, getComputedStyle(document.documentElement).getPropertyValue("--accent")]);
        assert(colour !== "" && accent !== "", `in the accent: ${colour}`);
        const state = ed.locator(`[data-mark-state="${m.mark}"]`);
        assert((await state.innerText()).includes(`filed ${ticket}`) && (await state.innerText()).includes(m.by), `its line of state, with who left it: ${await state.innerText()}`);
        assert((await state.evaluate((e) => getComputedStyle(e).fontFamily)).includes("Mono"), "in the mono face");
        assert((await ed.locator("[data-marked]").count()) === 1, "a dot in the margin of the block that holds it");
        assert((await state.evaluate((e) => e.parentElement.hasAttribute("data-marked"))), "under the block that holds it");
      }
      // Three words linked to an address of the app, and a line of state alone.
      const [ra, rz] = at(SERVICE_MAP, "the service registry");
      const reg = mark(id, ra, rz, "--link", "#/code/plexus-registry");
      const [sa, sz] = at(SERVICE_MAP, "A stopped service stays, drawn hollow.");
      const plain = mark(id, sa, sz, "--label", "built node-legend v1");
      await eventually("three words linked", async () => (await underlined(one.ed, reg.mark)) === "the service registry", 3_000);
      await eventually("an underline with no link", async () => (await underlined(one.ed, plain.mark)) === "A stopped service stays, drawn hollow.", 3_000);
      assert(!(await underline(one.ed, plain.mark).first().getAttribute("class")).includes("nm-linked"), "no link: in the ink");
      await sleep(100);
      const offs = await one.ed.locator("[data-marked]").evaluateAll((els) => els.map((el) => {
        const d = el.querySelector(":scope > .nm-mdot")?.getBoundingClientRect();
        const walk = document.createTreeWalker(el, NodeFilter.SHOW_TEXT, { acceptNode: (n) => (n.data.trim() ? NodeFilter.FILTER_ACCEPT : NodeFilter.FILTER_SKIP) });
        const r = document.createRange(); const t = walk.nextNode(); r.setStart(t, 0); r.setEnd(t, 1);
        const line = r.getClientRects()[0];
        return d ? Math.abs(d.top + d.height / 2 - (line.top + line.height / 2)) : 99;
      }));
      assert(offs.length === 3 && offs.every((d) => d <= 2), `three dots, each level with its block's first line: off by ${JSON.stringify(offs)} px`);
      // The text is not touched.
      const after = get(id);
      assert(after.body === before.body && after.revision === before.revision, `the body and revision as they were: ${before.revision} → ${after.revision}`);
      if (process.env.PAGES_SHOTS) {
        for (const [w, h] of [[1440, 900], [390, 844]]) for (const theme of ["light", "dark"]) {
          await one.page.setViewportSize({ width: w, height: h });
          await one.page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
          await sleep(400);
          await one.page.screenshot({ path: path.join(process.env.PAGES_SHOTS, `note-marked-${w}-${theme}.png`) });
        }
        await one.page.setViewportSize({ width: 1440, height: 900 });
        await one.page.evaluate(() => (document.documentElement.dataset.theme = "light"));
        await sleep(300);
      }
      // A click on the underline opens the ticket.
      await underline(one.ed, m.mark).first().click();
      await eventually("the ticket opened", async () => ticketAt(await one.page.evaluate(() => location.hash), ticket), 5_000);
      // An address of the app, by the keyboard.
      await underline(two.ed, reg.mark).first().focus();
      await two.page.keyboard.press("Enter");
      await eventually("the app's address opened", async () => (await two.page.evaluate(() => location.hash)).startsWith("#/code/plexus-registry"), 5_000);
    } finally {
      await one.context.close();
      await two.context.close();
    }
  });

  await test("a_mark_follows_its_words_while_the_note_is_typed", async () => {
    const id = note("The text moves", SERVICE_MAP);
    const { context, page, ed } = await openNote(id);
    try {
      const [a, z] = at(SERVICE_MAP, "Let me connect two nodes");
      const m = mark(id, a, z, "--label", "built graph-nodes v3");
      await eventually("underlined", async () => (await underlined(ed, m.mark)) === "Let me connect two nodes", 3_000);
      // A paragraph written above it, typed.
      await caretAt(page, 0);
      await page.keyboard.type("A paragraph above.\n\n");
      // A word added inside it, typed in its own block: CodeMirror's underline in the caret's block.
      const inside = SERVICE_MAP.indexOf("two nodes") + "A paragraph above.\n\n".length;
      await caretAt(page, inside);
      await page.keyboard.type("more ");
      await eventually("underlined in the caret's block, the new word included", async () => (await underlined(ed, m.mark)) === "Let me connect more two nodes", 5_000);
      assert((await ed.locator(".cm-line.nm-dot").count()) === 1, "the caret's block keeps its dot");
      assert((await ed.locator(".nm-states [data-mark-state]").innerText()).includes("built graph-nodes v3"), "and its line of state");
      // Away from it, drawn: still on the same words.
      await blur(page);
      await eventually("drawn on the same words", async () => (await underlined(ed, m.mark)) === "Let me connect more two nodes" && (await ed.locator(`[data-marked] [data-mark-state="${m.mark}"]`).count()) === 1, 5_000);
      // notes.marks says where they are now.
      await eventually("notes.marks says where they are now", async () => {
        const now = marks(id).find((x) => x.mark === m.mark);
        const body = get(id).body;
        return now && now.words === "Let me connect more two nodes" && body.slice(now.from, now.to) === now.words && now.from === a + 20;
      }, 5_000);
    } finally {
      await context.close();
    }
  });

  // ── NOTES-5's third requirement: a component under a passage ──────────────
  /** `ui author` into the bundle `notes`, its params a file (`-p`, synapse's global option): the version. */
  function author(name, source) {
    const params = path.join(scratch, `note-marks-author-${name}-${Date.now()}.json`);
    fs.writeFileSync(params, JSON.stringify({ bundle: "notes", sources: [{ name, profile: "svelte/1", source }] }));
    const r = spawnSync("synapse", ["--no-hold", "--token-file", session, "-j", "-p", `@${params}`, "-P", String(server.ws), "menger", "ui", "author"],
      { env: { ...process.env, XDG_CACHE_HOME: path.join(server.work, "cache") }, encoding: "utf8" });
    const authored = (r.stdout ?? "").split("\n").filter(Boolean).map((l) => { try { return JSON.parse(l); } catch { return null; } }).find((i) => i?.content?.type === "authored");
    assert(authored, `ui author ${name}: ${r.stdout} ${r.stderr}`);
    return authored.content.version;
  }
  const legendV = author("node-legend", LEGEND);
  const LAST = "A stopped service stays, drawn hollow.";
  const place = (id, ...flags) => { const [a, z] = at(SERVICE_MAP, LAST); return mark(id, a, z, "--component", "node-legend", "--version", legendV, ...flags); };
  const card = (page, m) => page.locator(`.nm-placed[data-placed="${m}"]`);
  const legend = (page, m) => page.locator(`.nm-placed[data-placed="${m}"] [data-legend]`);
  /** Where the card stands against its passage's words: the gap from the words' bottom to the card's top, and whether it is outside every block of text. */
  const under = (page, m) => page.evaluate((m) => {
    const words = [...document.querySelectorAll(`.nm-u[data-mark="${m}"]`)].map((e) => e.getBoundingClientRect());
    const c = document.querySelector(`.nm-placed[data-placed="${m}"]`);
    if (!words.length || !c) return null;
    const bottom = Math.max(...words.map((r) => r.bottom));
    return { gap: c.getBoundingClientRect().top - bottom, outside: !c.closest("[data-block], .cm-line"), words: words.length };
  }, m);

  await test("a_component_under_a_passage_on_every_page_stays_as_the_text_moves", async () => {
    const id = note("A component under a passage", SERVICE_MAP);
    const one = await openNote(id);
    const two = await openNote(id);
    try {
      await sleep(500);
      const before = get(id);
      const sent = Date.now();
      const m = place(id, "--props", JSON.stringify({ hollow: false }));
      for (const [k, { page }] of [one, two].entries()) await eventually(`page ${k + 1}: the component drawn`, async () => (await legend(page, m.mark).count()) === 1, 3_000);
      assert(Date.now() - sent < 3_500, `on every open page (${Date.now() - sent} ms)`);
      for (const { page } of [one, two]) {
        const fill = card(page, m.mark).locator(`[data-slot="mark-${m.mark}"]`);
        assert((await fill.getAttribute("data-component")) === "node-legend" && (await fill.getAttribute("data-version")) === legendV, "a bound mark pinned to the version");
        assert((await card(page, m.mark).locator("[data-field=component]").innerText()) === "node-legend", "its name in the card's head");
        assert((await card(page, m.mark).locator("[data-field=placed-by]").innerText()).includes(m.by), "and who placed it");
        const u = await under(page, m.mark);
        assert(u && u.outside && u.gap >= 0 && u.gap < 80, `under the passage, never in the text: ${JSON.stringify(u)}`);
      }
      const self = await legend(two.page, m.mark).getAttribute("data-self");
      // The text moves: a paragraph written above, on the first page.
      await caretAt(one.page, 0);
      await one.page.keyboard.type("A paragraph written above.\n\n");
      await blur(one.page);
      await eventually("the second page has the paragraph", async () => (await two.page.evaluate((s) => document.querySelector(s).editorView.state.doc.toString(), BODY)).startsWith("A paragraph written above."), 5_000);
      await sleep(300);
      const u = await under(two.page, m.mark);
      assert(u && u.outside && u.gap >= 0 && u.gap < 80, `still under its passage: ${JSON.stringify(u)}`);
      assert((await legend(two.page, m.mark).getAttribute("data-self")) === self && (await legend(two.page, m.mark).count()) === 1, "the same component, not mounted again");
      const after = get(id);
      assert(after.body === "A paragraph written above.\n\n" + before.body, "only the typing wrote the text");
      if (process.env.PAGES_SHOTS) {
        for (const [w, h] of [[1440, 900], [390, 844]]) for (const theme of ["light", "dark"]) {
          await one.page.setViewportSize({ width: w, height: h });
          await one.page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
          await sleep(400);
          await one.page.screenshot({ path: path.join(process.env.PAGES_SHOTS, `note-placed-${w}-${theme}.png`) });
        }
        await one.page.setViewportSize({ width: 1440, height: 900 });
        await one.page.evaluate(() => (document.documentElement.dataset.theme = "light"));
      }
    } finally {
      await one.context.close();
      await two.context.close();
    }
  });

  await test("its_props_change_on_every_page_in_place", async () => {
    const id = note("Its props change", SERVICE_MAP);
    const m = place(id);
    const one = await openNote(id);
    const two = await openNote(id);
    try {
      for (const { page } of [one, two]) await eventually("drawn", async () => (await legend(page, m.mark).count()) === 1, 8_000);
      const selves = await Promise.all([one, two].map(({ page }) => legend(page, m.mark).getAttribute("data-self")));
      // The component patches its own props (notes.mark_props, its mark's id handed to it).
      await legend(one.page, m.mark).locator("[data-action=hollow]").click();
      for (const [k, { page }] of [one, two].entries()) await eventually(`page ${k + 1}: shown`, async () => (await legend(page, m.mark).locator("[data-field=stopped]").count()) === 1, 3_000);
      // An agent patches them too.
      sy("notes", "mark_props", "--id", id, "--mark", m.mark, "--patch", JSON.stringify({ hollow: false }));
      for (const [k, { page }] of [one, two].entries()) await eventually(`page ${k + 1}: the agent's patch`, async () => (await legend(page, m.mark).locator("[data-field=stopped]").count()) === 0, 3_000);
      for (const [k, { page }] of [one, two].entries()) {
        assert((await legend(page, m.mark).getAttribute("data-self")) === selves[k] && (await legend(page, m.mark).count()) === 1, `page ${k + 1}: the same instance, not mounted again`);
      }
      const kept = marks(id).find((x) => x.mark === m.mark);
      assert(kept?.props?.hollow === false, `kept: ${JSON.stringify(kept?.props)}`);
    } finally {
      await one.context.close();
      await two.context.close();
    }
  });

  await test("writing_goes_on_in_the_passage_above_a_component", async () => {
    const id = note("Writing goes on", SERVICE_MAP);
    const m = place(id);
    const { context, page } = await openNote(id);
    try {
      await eventually("drawn", async () => (await legend(page, m.mark).count()) === 1, 8_000);
      const self = await legend(page, m.mark).getAttribute("data-self");
      // The caret in the passage the component is under, then typing.
      const at0 = SERVICE_MAP.indexOf("drawn hollow.");
      await caretAt(page, at0);
      const typed = "always ";
      for (let k = 0; k < typed.length; k++) {
        await page.keyboard.type(typed[k]);
        const head = await page.evaluate((s) => document.querySelector(s).editorView.state.selection.main.head, BODY);
        assert(head === at0 + k + 1, `the caret did not jump: at ${head}, want ${at0 + k + 1}`);
      }
      await eventually("written", async () => get(id).body.includes("stays, always drawn hollow."), 5_000);
      await sleep(400);
      assert((await legend(page, m.mark).getAttribute("data-self")) === self && (await legend(page, m.mark).count()) === 1, "not mounted again");
      assert(await page.evaluate((s) => document.activeElement?.closest(".cm-content") != null, BODY), "the focus stays in the text");
      // Out of the passage: still the same instance, under it.
      await blur(page);
      await sleep(300);
      assert((await legend(page, m.mark).getAttribute("data-self")) === self, "drawn again around it, the component is the same");
      const u = await under(page, m.mark);
      assert(u && u.outside && u.gap >= 0 && u.gap < 80, `under its passage: ${JSON.stringify(u)}`);
      // Its words deleted: drawn nowhere.
      sy("notes", "save", "--id", id, "--body", file("placed-cut", get(id).body.replace("A stopped service stays, always drawn hollow.\n", "")));
      await eventually("drawn nowhere", async () => (await card(page, m.mark).count()) === 0 && (await page.locator("[data-legend]").count()) === 0, 5_000);
      assert(marks(id).find((x) => x.mark === m.mark)?.detached === true, "listed detached");
    } finally {
      await context.close();
    }
  });

  await test("a_mark_whose_words_are_deleted_is_drawn_nowhere", async () => {
    const id = note("The words go", SERVICE_MAP);
    const { context, page, ed } = await openNote(id);
    try {
      const [a, z] = at(SERVICE_MAP, "A stopped service stays, drawn hollow.");
      const m = mark(id, a, z, "--label", "built node-legend v1");
      await eventually("underlined", async () => (await underline(ed, m.mark).count()) > 0, 3_000);
      sy("notes", "save", "--id", id, "--body", file("cut", SERVICE_MAP.replace("A stopped service stays, drawn hollow.\n", "")));
      await eventually("drawn nowhere", async () => (await underline(ed, m.mark).count()) === 0 && (await ed.locator(`[data-mark-state="${m.mark}"]`).count()) === 0 && (await ed.locator("[data-marked]").count()) === 0, 5_000);
      const now = marks(id).find((x) => x.mark === m.mark);
      assert(now?.detached === true && now.from == null, `listed as detached: ${JSON.stringify(now)}`);
    } finally {
      await context.close();
    }
  });
};
