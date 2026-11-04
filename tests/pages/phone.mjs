// EDITOR-12: on a phone the doc never scrolls horizontally. The owner,
// 2026-11-04: "I would also like the mobile view to make sure for the doc
// component that there is no scroll … horizontally". One doc and one ticket
// stuffed with every wide thing — a 200-character code line, a 12-column
// table, a 300-character URL, a 2000 px image, a YouTube embed, a mermaid
// diagram, a long title, three open questions — drawn at 320, 375, 390 and
// 430 px on the editor (`#/doc/:id`, `#/edit/:id`) and the read-only ticket
// page (`#/ticket/:id`), with the caret outside every block and then inside
// each wide one. At each: `document.scrollingElement.scrollWidth` is the
// viewport's width, no element of the surface has `scrollWidth >
// clientWidth`, and nothing visible stands past the viewport's edges. Code
// wraps (a hanging indent, copy still the exact code), a table stacks its
// rows (each cell labelled by its column's header), media and the diagram
// fit the column, the questions sit under the text, the list is a drawer.
// At 1440 the code card keeps its own horizontal scroll, unwrapped.
// Run alone by `node tests/pages_browser.mjs --group phone`.
//
// Marks: `[data-surface=editor-screen]`, `[data-surface=editor-body]`,
// `[data-surface=ticket-page]`, `[data-surface=code-card]` (`[data-action=copy]`),
// `td[data-label]`, `[data-field=margin] [data-thread]`, `[data-field=documents]`,
// `[data-action=docs]`, `[data-surface=mermaid-diagram][data-state=drawn]`, `.mx-embed`.
import zlib from "node:zlib";

const EPIC = "PHON";
const WIDTHS = [320, 375, 390, 430];

/** A `w`×`h` PNG, one colour (RGB, no filter), as base64. */
function png(w, h) {
  const crcTable = Array.from({ length: 256 }, (_, n) => { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; return c >>> 0; });
  const crc = (buf) => { let c = 0xffffffff; for (const b of buf) c = crcTable[(c ^ b) & 0xff] ^ (c >>> 8); return (c ^ 0xffffffff) >>> 0; };
  const chunk = (type, data) => {
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const td = Buffer.concat([Buffer.from(type, "ascii"), data]);
    const c = Buffer.alloc(4); c.writeUInt32BE(crc(td));
    return Buffer.concat([len, td, c]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(w, 0); ihdr.writeUInt32BE(h, 4); ihdr[8] = 8; ihdr[9] = 2; ihdr[10] = 0; ihdr[11] = 0; ihdr[12] = 0;
  const row = Buffer.alloc(1 + w * 3);
  for (let x = 0; x < w; x++) { row[1 + x * 3] = 40 + (x % 200); row[2 + x * 3] = 120; row[3 + x * 3] = 200; }
  const raw = Buffer.concat(Array.from({ length: h }, () => row));
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk("IHDR", ihdr), chunk("IDAT", zlib.deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]).toString("base64");
}

const CODE_LINE = `    let fired = kiln.firings().filter(|f| f.cone >= 6 && f.hours > 9).map(|f| (f.date, f.glaze.name.clone(), f.notes.join("; "))).collect::<Vec<_>>(); // and the rest`;
const URL300 = "https://example.com/" + "kiln-firing-schedule-".repeat(13) + "end?q=" + "x".repeat(7);
const WORD = "Glazeformulationcalculationsthatnevertakeabreathbetweentheirsyllablesandneverwillatall";
const TITLE = "The kiln's long title, which says everything at once: " + "firingschedulesglazesconesandthewholeyearinoneword".repeat(2) + " and more";
const COLS = Array.from({ length: 12 }, (_, k) => `column ${k + 1}`);
const body = (hash) => `A paragraph with a long address in it, ${URL300}, and a word that never breaks on its own: ${WORD}.

\`\`\`rust
fn main() {
${CODE_LINE}
}
\`\`\`

| ${COLS.join(" | ")} |
| ${COLS.map(() => "--").join(" | ")} |
| ${COLS.map((_, k) => `cone ${k + 1}`).join(" | ")} |
| ${COLS.map((_, k) => `glaze number ${k + 1} of the year`).join(" | ")} |

<m-code lang="python" path="tools/fire_the_kiln_and_record_every_firing_in_the_log.py" why="how a firing is recorded, step by step">

\`\`\`python
def fire(kiln, cone=6):
    return kiln.record(cone)
\`\`\`

</m-code>

![a wide picture](media:${hash})

https://www.youtube.com/watch?v=dQw4w9WgXcQ

\`\`\`mermaid
graph LR
  A[the clay is dug] --> B[the clay is wedged] --> C[the pot is thrown] --> D[the pot is trimmed] --> E[the bisque firing] --> F[the glaze is mixed] --> G[the glaze firing] --> H[the pot is sold]
\`\`\`

The last paragraph.
`;

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, session, server } = ctx;
  const scratch = path.dirname(session);
  const file = (name, text) => { const f = path.join(scratch, `phone-${name}`); fs.writeFileSync(f, text); return `@${f}`; };
  const IMG = png(2000, 120);
  const put = (owner, id) => {
    const [stored] = sy("media", "put", `--${owner}`, id, "--mime", "image/png", "--data", file(`${owner}.b64`, IMG));
    assert(stored?.type === "stored" && stored.width === 2000, `media put: ${JSON.stringify(stored)}`);
    return stored.hash;
  };

  // The doc: its body, the picture its own, three open questions on its text.
  const docId = sy("docs", "create", "--title", TITLE, "--body", file("doc.md", "placeholder\n")).find((e) => e.type === "created").doc.id;
  const docBody = body(put("doc", docId));
  sy("docs", "save", "--id", docId, "--body", file("doc2.md", docBody));
  // The ticket: the same, its picture its own, three open questions.
  const root = sy("tickets", "create", "--epic", EPIC, "--kind", "scope", "--title", "the phone's tickets").find((e) => e.type === "created").ticket.id;
  const tid = sy("tickets", "create", "--epic", EPIC, "--parent", root, "--title", TITLE, "--body", file("t0.md", "placeholder\n")).find((e) => e.type === "created").ticket.id;
  const ticketBody = body(put("ticket", tid));
  sy("tickets", "amend", "--id", tid, "--body", file("t1.md", ticketBody));
  for (const q of ["Is cone 6 right for every glaze?", `And the address ${URL300}?`, `${WORD}?`]) sy("tickets", "inputs", "ask", "--ticket", tid, "--question", q);

  const BODY = "[data-surface=editor-body] [data-field=body]";
  const caretAt = (page, pos) => page.evaluate(({ s, pos }) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: pos }, scrollIntoView: true }); }, { s: BODY, pos });
  const blur = (page) => page.evaluate((s) => document.querySelector(s).editorView.contentDOM.blur(), BODY);
  /** Every horizontal overflow on the page: the document's, each element of `scope` wider inside than out,
   *  and each visible element of it standing past the viewport. */
  const overflow = (page, scope) => page.evaluate((scope) => {
    const vw = document.documentElement.clientWidth;
    const root = document.querySelector(scope);
    const name = (e) => `${e.localName}${e.className && typeof e.className === "string" ? "." + e.className.trim().split(/\s+/).join(".") : ""}${e.dataset?.field ? `[${e.dataset.field}]` : ""}`;
    const drawer = document.querySelector("[data-surface=editor-screen]:not([data-drawer=open]) [data-field=documents]");
    const out = { vw, page: document.scrollingElement.scrollWidth, wide: [], past: [] };
    if (!root) return { ...out, missing: true };
    for (const e of [root, ...root.querySelectorAll("*")]) {
      if (e instanceof SVGElement && !(e instanceof SVGSVGElement)) continue;
      if (drawer && drawer.contains(e)) continue; // the closed drawer stands off the left edge, by design
      // A truncation (overflow hidden, an ellipsis: a row's author in the history) cannot scroll; anything else wider inside than out can.
      const cs = getComputedStyle(e);
      const truncated = cs.textOverflow === "ellipsis" && (cs.overflowX === "hidden" || cs.overflowX === "clip");
      // A code block its viewer unwrapped scrolls inside its own box (`box:`), never the page.
      const box = e.closest("[data-surface=code-card][data-wrap=off] .scroll") ? "box:" : "";
      if (e.scrollWidth > e.clientWidth + 1 && !truncated) out.wide.push(`${box}${name(e)} ${e.scrollWidth}>${e.clientWidth}`);
      const r = e.getBoundingClientRect();
      if (r.width > 0 && r.height > 0 && (r.right > vw + 1 || r.left < -1) && getComputedStyle(e).visibility !== "hidden" && !e.closest(".scrim")) out.past.push(`${name(e)} ${Math.round(r.left)}..${Math.round(r.right)}`);
    }
    return out;
  }, scope);
  const fits = async (page, scope, what) => {
    // Every part of a tall document drawn: the editor's column scrolled through, measured at each stop.
    const stops = await page.evaluate(() => { const p = document.querySelector("[data-surface=editor-screen] .page"); return p ? Math.ceil(p.scrollHeight / 2000) : 0; });
    for (let k = 1; k < stops; k++) {
      await page.evaluate((y) => { document.querySelector("[data-surface=editor-screen] .page").scrollTop = y; }, k * 2000);
      await sleep(80);
      await fitsHere(page, scope, `${what} (scrolled ${k * 2000})`);
    }
    if (stops > 1) { await page.evaluate(() => { document.querySelector("[data-surface=editor-screen] .page").scrollTop = 0; }); await sleep(80); }
    await fitsHere(page, scope, what);
  };
  const fitsHere = async (page, scope, what) => {
    const o = await overflow(page, scope);
    assert(!o.missing, `${what}: ${scope} is on the page`);
    assert(o.page === o.vw, `${what}: the page is ${o.page} wide at ${o.vw}`);
    assert(o.wide.length === 0, `${what}: nothing scrolls sideways — ${o.wide.slice(0, 8).join(", ")}`);
    assert(o.past.length === 0, `${what}: nothing stands past the edge — ${o.past.slice(0, 8).join(", ")}`);
  };
  /** Everything wide drawn: the card, the stacked table, the picture loaded, the player, the diagram. */
  const drawnAll = (page, scope) => eventually(`${scope}: every wide thing drawn`, async () => {
    const missing = await page.evaluate((scope) => {
      const s = document.querySelector(scope);
      if (!s) return ["the surface"];
      const img = s.querySelector("img[alt='a wide picture']");
      const want = { "a code card": !!s.querySelector("[data-surface=code-card]"), "a labelled cell": !!s.querySelector("td[data-label]"),
        "the picture loaded": !!img?.complete && img.naturalWidth === 2000, "the player": !!s.querySelector(".mx-embed iframe"),
        "the diagram": !!s.querySelector("[data-surface=mermaid-diagram][data-state=drawn] svg") };
      return Object.entries(want).filter(([, ok]) => !ok).map(([k]) => k);
    }, scope);
    if (missing.length) throw new Error(`missing ${missing.join(", ")}`);
    return true;
  }, 15_000);
  const at = (text, needle, plus = 0) => { const k = text.indexOf(needle); assert(k >= 0, `${needle.slice(0, 20)} in the body`); return k + plus; };
  /** The caret inside each wide block, in turn: the long line, the table, the address, the picture, the player, the diagram. */
  const insides = (text) => [
    ["the code's long line", at(text, "let fired", 40)],
    ["the table", at(text, "glaze number 7", 3)],
    ["the address", at(text, URL300, 120)],
    ["the picture", at(text, "![a wide picture]", 4)],
    ["the player", at(text, "https://www.youtube.com", 10)],
    ["the diagram", at(text, "graph LR", 30)],
  ];
  const phone = async (route, w) => {
    const { context, page } = await openPage(route);
    // Tall, so CodeMirror draws the whole document at once (it draws its viewport and a margin): the width is what is measured.
    await page.setViewportSize({ width: w, height: 4000 });
    await context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: server.origin });
    return { context, page };
  };
  /** The drawn card wraps its long line under a hanging indent, and copies the code exactly. */
  const codeWraps = async (page, scope, what) => {
    const card = page.locator(`${scope} [data-surface=code-card]`).first();
    const r = await card.evaluate((c) => {
      const line = [...c.querySelectorAll(".ln")].find((l) => l.textContent.includes("let fired") && l.getClientRects().length > 0) ?? null;
      if (!line) return null;
      const cs = getComputedStyle(line);
      return { ws: getComputedStyle(line.closest("pre")).whiteSpace, h: line.getBoundingClientRect().height, lh: parseFloat(cs.lineHeight), indent: parseFloat(cs.textIndent), pad: parseFloat(cs.paddingLeft) };
    });
    assert(r, `${what}: the card draws its code line by line`);
    assert(r.ws === "pre-wrap", `${what}: the code wraps (${r.ws})`);
    assert(r.h > r.lh * 1.5, `${what}: the long line takes several rows (${r.h} at ${r.lh})`);
    assert(r.indent < 0 && r.pad > 0 && Math.abs(r.indent + r.pad) < 1, `${what}: a hanging indent (${r.indent}, ${r.pad})`);
    // Highlighting holds, wrapped: the painted rust's tokens, drawn.
    await eventually(`${what}: the rust painted`, async () => (await card.locator("[data-surface=code-paint][data-state=painted] .ln .tok-keyword").count()) > 0, 8_000);
    assert((await card.getAttribute("data-wrap")) === "on", `${what}: wrapped by default on a phone`);
    // The notes under the box, never in it: the box holds code only.
    const notes = await page.locator(`${scope} [data-surface=code-card]:has([data-field=notes])`).first().evaluate((c) => {
      const box = c.querySelector(".box"), n = c.querySelector("[data-field=notes]");
      return { inside: box.contains(n), below: n.getBoundingClientRect().top >= box.getBoundingClientRect().bottom - 0.5, path: n.querySelector("[data-field=path]")?.textContent, why: n.querySelector("[data-field=why]")?.textContent, boxText: box.textContent };
    });
    assert(!notes.inside && notes.below && notes.path?.startsWith("tools/fire") && notes.why?.startsWith("how a firing") && !notes.boxText.includes("tools/fire"), `${what}: path and why under the box: ${JSON.stringify(notes)}`);
    await card.locator("[data-action=copy]").click();
    await eventually(`${what}: copied`, async () => (await card.locator("[data-action=copy]").innerText()) === "copied", 3_000);
    const copied = await page.evaluate(() => navigator.clipboard.readText());
    assert(copied === `fn main() {\n${CODE_LINE}\n}`, `${what}: the exact code copied:\n${JSON.stringify(copied)}`);
  };
  /** The table stacks: a row a group, each cell labelled by its column's header. */
  const tableStacks = async (page, scope, what) => {
    const t = await page.locator(`${scope} table`).first().evaluate((tb) => ({
      labels: [...tb.querySelectorAll("tbody tr:last-child td")].map((td) => td.getAttribute("data-label")),
      display: getComputedStyle(tb.querySelector("tbody tr")).display,
      head: getComputedStyle(tb.querySelector("thead")).display,
      label: getComputedStyle(tb.querySelector("tbody td"), "::before").content,
    }));
    assert(JSON.stringify(t.labels) === JSON.stringify(COLS), `${what}: each cell labelled by its header (${t.labels})`);
    assert(t.display === "block" && t.head === "none", `${what}: a row is a stacked group, the header row gone (${t.display}, ${t.head})`);
    assert(t.label.includes("column 1"), `${what}: the label drawn beside the cell (${t.label})`);
  };
  /** The picture, the player and the diagram inside the column, the picture's and player's shapes kept. */
  const mediaFits = async (page, scope, what) => {
    const m = await page.evaluate((scope) => {
      const s = document.querySelector(scope);
      const box = (e) => e.getBoundingClientRect();
      const img = s.querySelector("img[alt='a wide picture']"), ifr = s.querySelector(".mx-embed iframe"), svg = s.querySelector("[data-surface=mermaid-diagram] svg");
      const col = img.closest(".blk, [data-block]") ?? img.parentElement;
      return { col: box(col).width, img: [box(img).width, box(img).height], ifr: [box(ifr).width, box(ifr).height], svg: box(svg).width };
    }, scope);
    assert(m.img[0] <= m.col + 1 && Math.abs(m.img[0] / m.img[1] - 2000 / 120) < 0.6, `${what}: the picture fits, its shape kept (${m.img} in ${m.col})`);
    assert(m.ifr[0] <= m.col + 1 && Math.abs(m.ifr[0] / m.ifr[1] - 16 / 9) < 0.05, `${what}: the player fits at 16:9 (${m.ifr})`);
    assert(m.svg <= m.col + 1, `${what}: the diagram scaled down (${m.svg} in ${m.col})`);
  };

  for (const w of WIDTHS) {
    await test(`the_doc_never_scrolls_sideways_at_${w}`, async () => {
      const { context, page } = await phone(`#/doc/${docId}`, w);
      try {
        const S = "[data-surface=editor-screen]";
        await eventually("the doc is drawn", async () => (await page.locator(`${BODY} .cm-content`).count()) === 1, 10_000);
        // Three questions on the doc's text, asked as the owner, live in the margin.
        if (w === WIDTHS[0]) for (const [on, q] of [["A paragraph", "Is it the kiln?"], ["column 3", `Which ${URL300}?`], ["The last", `${WORD}?`]])
          sy("docs", "ask", "--id", docId, "--from", String(docBody.indexOf(on)), "--to", String(docBody.indexOf(on) + on.length), "--text", q);
        await drawnAll(page, S);
        await eventually("three open questions in the margin", async () => (await page.locator(`${S} [data-field=margin] [data-thread][data-state=open]`).count()) === 3, 8_000);
        await blur(page);
        await sleep(200);
        await fits(page, S, `the doc at ${w}, every block drawn`);
        await codeWraps(page, S, `the doc at ${w}`);
        await tableStacks(page, S, `the doc at ${w}`);
        await mediaFits(page, S, `the doc at ${w}`);
        // The questions under the text, inside the column.
        const [margin, text] = await page.evaluate(() => [document.querySelector("[data-field=margin]").getBoundingClientRect().toJSON(), document.querySelector("[data-surface=editor-body] [data-field=body]").getBoundingClientRect().toJSON()]);
        assert(margin.top >= text.bottom - 1 && margin.right <= w, `the questions sit under the text (${JSON.stringify(margin)} under ${JSON.stringify(text)})`);
        // The title wraps.
        const title = await page.locator(`${S} [data-field=title]`).evaluate((e) => ({ h: e.getBoundingClientRect().height, lh: parseFloat(getComputedStyle(e).lineHeight) }));
        assert(title.h > title.lh * 1.5, `the title wraps (${title.h})`);
        // The list is a drawer: off the screen, then over it when asked for, inside the width.
        await page.locator(`${S} [data-action=docs]`).click();
        await eventually("the drawer opens", async () => (await page.locator(`${S}[data-drawer=open]`).count()) === 1, 3_000);
        await sleep(300);
        await fits(page, S, `the doc at ${w}, the drawer open`);
        await page.keyboard.press("Escape");
        await eventually("the drawer closes", async () => (await page.locator(`${S}[data-drawer=open]`).count()) === 0, 3_000);
        // The ask control on a selection, and the history's scrubber: inside the width too.
        await page.evaluate((s) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: 2, head: 40 } }); }, BODY);
        await page.locator(`${S} [data-action=ask-open]`).click();
        await eventually("the ask control open", async () => (await page.locator(`${S} [data-ask][data-state=open]`).count()) === 1, 3_000);
        await fits(page, S, `the doc at ${w}, asking`);
        await page.keyboard.press("Escape");
        await page.locator(`${S} [data-action=history]`).click();
        await eventually("the scrubber", async () => (await page.locator(`${S} [data-field=scrubber]`).count()) === 1, 5_000);
        await sleep(300);
        await fits(page, S, `the doc at ${w}, the history open`);
        await page.locator(`${S} [data-action=live]`).click();
        // The caret in each wide block: its source shows, and still nothing scrolls sideways.
        const text0 = await page.evaluate((s) => document.querySelector(s).editorView.state.doc.toString(), BODY);
        for (const [what, pos] of insides(text0)) {
          await caretAt(page, pos);
          await sleep(150);
          await fits(page, S, `the doc at ${w}, the caret in ${what}`);
        }
        // The caret's fence wraps under a hanging indent too.
        await caretAt(page, at(text0, "let fired", 40));
        const src = await page.evaluate(() => {
          const l = [...document.querySelectorAll(".cm-line.eb-fence")].find((x) => x.textContent.includes("let fired"));
          const cs = l && getComputedStyle(l);
          return l && { ws: cs.whiteSpace, h: l.getBoundingClientRect().height, lh: parseFloat(cs.lineHeight), indent: parseFloat(cs.textIndent) };
        });
        assert(src && src.ws === "pre-wrap" && src.h > src.lh * 1.5 && src.indent < 0, `the fence's source wraps, hanging: ${JSON.stringify(src)}`);
        await eventually("the fence's source highlighted", async () => (await page.locator(".cm-line.eb-fence .tok-keyword").count()) > 0, 5_000);
      } finally {
        await context.close();
      }
    });

    await test(`the_ticket_never_scrolls_sideways_at_${w}`, async () => {
      for (const [route, S] of [[`#/edit/${tid}`, "[data-surface=editor-screen]"], [`#/ticket/${tid}`, "[data-surface=ticket-page]"]]) {
        const { context, page } = await phone(route, w);
        try {
          await drawnAll(page, S);
          if (route.startsWith("#/edit/")) await blur(page);
          await eventually("the questions drawn", async () => (await page.locator(`${S} [data-surface=ticket-questions]`).innerText().catch(() => "")).includes("cone 6"), 8_000);
          await sleep(200);
          await fits(page, S, `${route} at ${w}`);
          await codeWraps(page, S, `${route} at ${w}`);
          await tableStacks(page, S, `${route} at ${w}`);
          await mediaFits(page, S, `${route} at ${w}`);
          // PAGES_SHOTS=DIR: the read-only page at 390, light and dark, from its body's top and from its code.
          if (process.env.PAGES_SHOTS && w === 390 && route.startsWith("#/ticket/")) {
            await page.setViewportSize({ width: 390, height: 844 });
            for (const theme of ["light", "dark"]) {
              await page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
              for (const [at, sel] of [["body", "[data-ticket-body]"], ["code", "[data-surface=code-card]"]]) {
                await page.locator(`${S} ${sel}`).first().evaluate((e) => e.scrollIntoView({ block: "start" }));
                await sleep(300);
                await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, `ticket-390-${theme}-${at}.png`) });
              }
            }
            await page.evaluate(() => (document.documentElement.dataset.theme = "light"));
            await page.setViewportSize({ width: 390, height: 4000 });
          }
          if (route.startsWith("#/edit/")) {
            const text0 = await page.evaluate((s) => document.querySelector(s).editorView.state.doc.toString(), BODY);
            for (const [what, pos] of insides(text0)) {
              await caretAt(page, pos);
              await sleep(150);
              await fits(page, S, `${route} at ${w}, the caret in ${what}`);
            }
          }
        } finally {
          await context.close();
        }
      }
    });
  }

  // The owner: "ideally code wrap is an option on code". The toggle, both ways, kept for this viewer.
  for (const w of [390, 1440]) await test(`wrap_is_a_choice_on_each_code_block_at_${w}`, async () => {
    const { context, page } = await phone(`#/ticket/${tid}`, w);
    const S = "[data-surface=ticket-page]";
    const card = page.locator(`${S} [data-surface=code-card][data-lang=rust]`).first();
    const look = () => card.evaluate((c) => {
      const sc = c.querySelector(".scroll"), line = [...c.querySelectorAll(".ln")].find((l) => l.textContent.includes("let fired") && l.getClientRects().length > 0);
      return { wrap: c.dataset.wrap, pressed: c.querySelector("[data-action=wrap]").getAttribute("aria-pressed"), sw: sc.scrollWidth, cw: sc.clientWidth, h: line?.getBoundingClientRect().height ?? 0, lh: parseFloat(getComputedStyle(line ?? sc).lineHeight), tokens: c.querySelectorAll(".painted .tok-keyword").length, page: document.scrollingElement.scrollWidth, vw: document.documentElement.clientWidth };
    });
    try {
      await drawnAll(page, S);
      await eventually("painted", async () => (await card.locator("[data-surface=code-paint][data-state=painted]").count()) === 1, 8_000);
      const first = await look();
      const phoneW = w < 641;
      assert(first.wrap === (phoneW ? "on" : "off") && first.pressed === String(phoneW), `the default at ${w}: ${JSON.stringify(first)}`);
      await card.locator("[data-action=wrap]").click();
      const flipped = await look();
      if (phoneW) assert(flipped.wrap === "off" && flipped.sw > flipped.cw && flipped.h <= flipped.lh * 1.5 && flipped.page === flipped.vw, `unwrapped at ${w}: the box scrolls, the page does not: ${JSON.stringify(flipped)}`);
      else assert(flipped.wrap === "on" && flipped.sw <= flipped.cw + 1 && flipped.h > flipped.lh * 1.5 && flipped.page === flipped.vw, `wrapped at ${w}: ${JSON.stringify(flipped)}`);
      assert(flipped.tokens > 0 && first.tokens > 0, `highlighted either way: ${first.tokens}, ${flipped.tokens}`);
      if (phoneW) { const o = await overflow(page, S); assert(o.page === o.vw && o.wide.length > 0 && o.wide.every((x) => x.startsWith("box:")), `only the code's box scrolls: ${o.wide}`); }
      // Kept for this viewer, in this browser: a reload keeps it; a second click puts it back.
      await page.reload();
      await drawnAll(page, S);
      const again = await look();
      assert(again.wrap === flipped.wrap, `kept across a reload: ${again.wrap}`);
      await card.locator("[data-action=wrap]").click();
      assert((await look()).wrap === first.wrap, "the toggle back");
      const kept = await page.evaluate(() => { try { return localStorage.getItem("menger.code.wrap"); } catch { return null; } });
      assert(kept && !kept.includes("fired"), `kept by the code's hash, not its text: ${kept}`);
    } finally {
      await context.close();
    }
  });

  await test("at_1440_code_keeps_its_own_sideways_scroll", async () => {
    for (const [route, S] of [[`#/doc/${docId}`, "[data-surface=editor-screen]"], [`#/ticket/${tid}`, "[data-surface=ticket-page]"]]) {
      const { context, page } = await openPage(route);
      try {
        await drawnAll(page, S);
        if (route.startsWith("#/doc/")) await blur(page);
        await sleep(200);
        const c = await page.locator(`${S} [data-surface=code-card]`).first().evaluate((card) => {
          const sc = card.querySelector(".scroll"), pre = card.querySelector("pre:not([hidden])");
          return { sw: sc.scrollWidth, cw: sc.clientWidth, ox: getComputedStyle(sc).overflowX, ws: getComputedStyle(pre).whiteSpace };
        });
        assert(c.ws === "pre" && c.ox === "auto" && c.sw > c.cw, `${route}: the card scrolls its own code, unwrapped: ${JSON.stringify(c)}`);
        const page0 = await page.evaluate(() => [document.scrollingElement.scrollWidth, document.documentElement.clientWidth]);
        assert(page0[0] === page0[1], `${route}: the page itself never scrolls sideways (${page0})`);
        const t = await page.locator(`${S} table`).first().evaluate((tb) => getComputedStyle(tb.querySelector("tbody tr")).display);
        assert(t === "table-row", `${route}: the table a table at 1440 (${t})`);
      } finally {
        await context.close();
      }
    }
  });
};
