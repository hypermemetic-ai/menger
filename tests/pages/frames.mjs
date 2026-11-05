// EDITOR-14: full pages in a body draw as sandboxed, interactive frames again.
// The owner, 2026-11-05: "we currently don't support the same level of tickets
// rendering full iframes. full pages inside. we need to support that".
// Bodies in each form the tracker stores (synthetic pages: the repository
// keeps no tracker data) — v1's `<m-mockup widths>…page…</m-mockup>` inside an
// `<m-step>` of an `<m-status>` (APP-32's shape), a ```mockup fence with
// `widths`, `title` and `description` (APP-110's), ```html marked as a page,
// a plain ```html (code, not a page), `<m-mockup ref>`, a `live` fence, and a
// body that is a whole `<!doctype html>` page with `menger-widths` (APP-31's) —
// drawn on the ticket page and in the editor. In each frame: its script ran,
// its height is the content's (said by the frame, only to its own
// `contentWindow` on its channel), its controls work, and it cannot read a
// cookie, storage, `parent.document`, move `top`, fetch or open a socket. A
// message forged from outside the frame is ignored. The caret entering the
// frame's block shows its source; leaving draws it again. At 390 the page has
// no horizontal scroll and a 1440 page is scaled into the column.
// `FRAMES_REAL=DIR`: each `*.md` there (real bodies, kept outside the
// repository) is filed and must draw a frame per page, at 390 without scroll.
// Run alone by `node tests/pages_browser.mjs --group frames`.
//
// Marks: `[data-surface=page-frame]` (`data-kind=page|live|ref`,
// `data-state=loading|drawn|failed|note`, `data-width`), `[data-action=width]`
// (`data-width`, `aria-pressed`), `[data-field=label|description|note]`,
// `[data-page]` on a drawn block.

const EPIC = "FRAM";

const PAGE = (title, extra = "") => `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title}</title>
<style>
  body { margin: 0; font: 16px/1.5 system-ui, sans-serif; background: #f6f3ec; color: #1d1b16; }

  header { padding: 24px; border-bottom: 2px solid #b4441c; }
  main { display: grid; gap: 16px; padding: 24px; grid-template-columns: repeat(auto-fit, minmax(220px, 1fr)); }
  .card { border: 1px solid currentColor; padding: 16px; border-radius: 6px; height: 120px; }
</style>
</head>
<body>
<header><h1>${title}</h1></header>

<main>
  <div class="card">Blank lines survive.</div>

  <div class="card">Scripts run: <span id="ran">not yet</span>.</div>
  <div class="card"><button id="more" type="button">more</button> <span id="count">0</span></div>
  <div class="card"><form id="f"><input id="q" value="cone 6"><button id="go">go</button></form> <span id="said"></span></div>
  <div class="card"><a id="jump" href="#below">below</a></div>
  <div class="card"><form id="real" action="https://example.com/away"><button id="realgo">leave</button></form></div>
  <div class="card" id="below">the end</div>
</main>
<script>
  document.getElementById("ran").textContent = "yes, at " + window.innerWidth + "px";
  document.getElementById("more").addEventListener("click", function () { var c = document.getElementById("count"); c.textContent = String(Number(c.textContent) + 1); });
  document.getElementById("f").addEventListener("submit", function (e) { e.preventDefault(); document.getElementById("said").textContent = "said " + document.getElementById("q").value; });
</script>
${extra}
</body>
</html>`;

const BODY = `The kiln's mockups, in every form a body keeps them.

<m-status data-field="status">

<m-step date="2026-11-05T06:00Z" status="In Review" by="builder">

The element itself:

<m-mockup widths="390,1440" title="The element" description="v1's form, as stored.">
${PAGE("The page sits in the ticket")}
</m-mockup>

</m-step>

</m-status>

## A fence

\`\`\`mockup widths=1440 title="The fence at 1440" description="One view, scaled into the column."
${PAGE("The fence")}
\`\`\`

## Marked html

\`\`\`html label="Marked html" height=300
<p id="marked">a fragment page</p><script>document.getElementById("marked").textContent = "a fragment page, run";</script>
\`\`\`

## Plain html is code

\`\`\`html
<p>only code</p>
\`\`\`

## A reference and a live view

<m-mockup ref="KILN-9"></m-mockup>

\`\`\`live route="#/board" widths=390,1440 title="the board on test"
\`\`\`

The last paragraph.
`;

const WHOLE = `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="menger-view" content="mockup">
<meta name="menger-widths" content="390, 768, 1440">
<title>A whole page</title>
<style>body { margin: 0; font: 14px system-ui; } .tall { height: 1400px; background: linear-gradient(#fff, #ddd); }</style>
</head>
<body><p id="ran">not yet</p><div class="tall"></div><script>document.getElementById("ran").textContent = "whole, run";</script></body>
</html>
`;

// The owner, 2026-11-05: "also you know that v1 could just host raw html inside a frame right?" — a body
// holding raw HTML the page cannot draw is one frame, its markdown around the HTML.
const RAW = `A body with raw HTML in it.

<div class="demo" style="padding: 8px; border: 1px solid">
<button id="b" type="button" onclick="var n = document.getElementById('n'); n.textContent = String(Number(n.textContent) + 1)">count</button> <span id="n">0</span>
</div>

**Markdown** around it still renders, and [a link](https://example.com/away) opens a tab.

<script>document.getElementById("n").dataset.ran = "yes";</script>

<m-status data-field="status">

<m-step date="2026-11-05T06:00Z" status="Coding" by="builder">a step</m-step>

</m-status>

\`\`\`mockup title="nested" height=200
<p id="nested">nested page</p><script>document.getElementById("nested").textContent = "nested, run";</script>
\`\`\`

The end.
`;

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, fs, path, session } = ctx;
  const scratch = path.dirname(session);
  let n = 0;
  const file = (text) => { const f = path.join(scratch, `frames-${++n}.md`); fs.writeFileSync(f, text); return `@${f}`; };
  const made = (title, body, parent) => {
    const flags = parent ? ["--parent", parent] : ["--kind", "scope"];
    const [m] = sy("tickets", "create", "--epic", EPIC, ...flags, "--title", title, ...(body ? ["--body", file(body)] : []));
    assert(m?.ticket?.id, `made ${title}: ${JSON.stringify(m)}`);
    return m.ticket.id;
  };
  const root = made("the kiln's frames");
  const tid = made("every form of a page", BODY, root);
  const wid = made("a whole page", WHOLE, root);
  const rid = made("raw html", RAW, root);

  const PF = "[data-surface=page-frame]";
  /** The page frames under `scope`, each drawn (its page said its height), `count` of them. */
  const drawn = (page, scope, count) => eventually(`${count} frames drawn in ${scope}`, async () => (await page.locator(`${scope} ${PF}[data-kind=page][data-state=drawn]`).count()) >= count, 15_000);
  /** The Playwright frame of the `k`th page frame under `scope`. */
  const inner = async (page, scope, k) => {
    const el = await page.locator(`${scope} ${PF}[data-kind=page] iframe`).nth(k).elementHandle();
    return el.contentFrame();
  };
  const BODYSEL = "[data-surface=editor-body] [data-field=body]";
  const noSideways = async (page, what) => {
    const o = await page.evaluate(() => ({ page: document.scrollingElement.scrollWidth, vw: document.documentElement.clientWidth,
      past: [...document.querySelectorAll("[data-surface=page-frame], [data-surface=page-frame] *")].filter((e) => { const r = e.getBoundingClientRect(); return r.width > 0 && (r.right > document.documentElement.clientWidth + 1 || r.left < -1); }).map((e) => e.localName) }));
    assert(o.page === o.vw, `${what}: the page is ${o.page} wide at ${o.vw}`);
    assert(o.past.length === 0, `${what}: nothing of a frame stands past the edge (${o.past})`);
  };

  await test("a_body_draws_every_form_of_a_page_as_a_sandboxed_frame", async () => {
    const { context, page, seen } = await openPage(`#/ticket/${tid}`);
    const S = "[data-surface=ticket-page]";
    try {
      await drawn(page, S, 3);
      const frames = await page.locator(`${S} ${PF}`).evaluateAll((els) => els.map((e) => ({
        kind: e.dataset.kind, state: e.dataset.state, width: e.dataset.width ?? null,
        label: e.querySelector("[data-field=label]")?.textContent ?? null, about: e.querySelector("[data-field=description]")?.textContent ?? null,
        note: e.querySelector("[data-field=note]")?.textContent ?? null,
        sandbox: e.querySelector("iframe")?.getAttribute("sandbox") ?? null, src: e.querySelector("iframe")?.getAttribute("src") ?? null,
        srcdoc: e.querySelector("iframe")?.hasAttribute("srcdoc") ?? false,
        headAbove: (() => { const h = e.querySelector("figcaption"), f = e.querySelector("iframe"); return !h || !f || h.getBoundingClientRect().bottom <= f.getBoundingClientRect().top + 1; })(),
        widths: [...e.querySelectorAll("[data-action=width]")].map((b) => b.dataset.width),
      })));
      // The element, the fence, the marked html, the reference, the live view: five, in the body's order.
      assert(frames.length === 5, `five page blocks: ${JSON.stringify(frames)}`);
      const [el, fence, marked, ref, live] = frames;
      for (const f of [el, fence, marked]) {
        assert(f.kind === "page" && f.state === "drawn", `a page drawn: ${JSON.stringify(f)}`);
        assert(f.sandbox === "allow-scripts allow-forms", `sandboxed: scripts and forms (a real submit refused by form-action), never allow-same-origin: ${f.sandbox}`);
        assert(/^\/frame#f[0-9a-z]+$/.test(f.src) && !f.srcdoc, `handed over from /frame, not a srcdoc: ${f.src}`);
        assert(f.headAbove, `the label above the frame, never inside it: ${JSON.stringify(f)}`);
      }
      assert(el.label === "The element" && el.about === "v1's form, as stored." && JSON.stringify(el.widths) === '["390","1440"]' && el.width === "390", `the element's head and widths: ${JSON.stringify(el)}`);
      assert(fence.label === "The fence at 1440" && fence.width === "1440" && fence.widths.length === 0, `the fence: ${JSON.stringify(fence)}`);
      assert(marked.label === "Marked html", `the marked html: ${JSON.stringify(marked)}`);
      assert(ref.kind === "ref" && ref.note.includes("KILN-9"), `a reference is a link: ${JSON.stringify(ref)}`);
      assert(live.kind === "live" && live.state === "note" && /live origin/.test(live.note), `a live view with no --live-origin says so: ${JSON.stringify(live)}`);
      // Plain ```html stays code.
      assert((await page.locator(`${S} [data-surface=code-card]`).count()) === 1, "a plain html fence is a code card");
      assert(!(await page.locator(`${S} [data-ticket-body]`).innerText()).includes("Blank lines survive"), "no page's text leaks into the body as markdown");

      // Inside the element's page: its script ran at 390, its controls work, a link moves within the frame.
      const f0 = await inner(page, S, 0);
      assert((await f0.locator("#ran").textContent()) === "yes, at 390px", `the script ran at the device's width: ${await f0.locator("#ran").textContent()}`);
      await f0.locator("#more").click();
      await f0.locator("#more").click();
      assert((await f0.locator("#count").textContent()) === "2", "a button works");
      await f0.locator("#go").click();
      assert((await f0.locator("#said").textContent()) === "said cone 6", "a form a script handles works");
      await f0.locator("#jump").click();
      await sleep(200);
      assert((await f0.evaluate(() => location.hash)) === "#below", "a link moves within the frame");
      assert((await f0.locator("#ran").count()) === 1, "the frame still holds its page after the link");
      // The marked html fragment ran too.
      const f2 = await inner(page, S, 2);
      assert((await f2.locator("#marked").textContent()) === "a fragment page, run", "the fragment's script ran");

      // Its height is the content's: the frame grew past its start to the page's own height, and the fixed one kept its own.
      const h = await page.locator(`${S} ${PF}[data-kind=page]`).evaluateAll((els) => els.map((e) => ({ box: e.querySelector(".pf-box").getBoundingClientRect().height, frame: e.querySelector("iframe").getBoundingClientRect().height })));
      const doc0 = await f0.evaluate(() => document.documentElement.scrollHeight);
      assert(Math.abs(h[0].frame - doc0) <= 2 && doc0 > 844, `the element's frame is its content's height (${h[0].frame} for ${doc0})`);
      assert(Math.abs(h[2].frame - 300) <= 2, `height=300 is kept (${h[2].frame})`);

      // The sandbox: an opaque origin, nothing of the app's reachable.
      const probe = await f0.evaluate(async () => {
        const tryIt = (fn) => { try { const v = fn(); return `ok:${String(v).slice(0, 40)}`; } catch (e) { return `refused:${e.name}`; } };
        const out = {
          origin: String(window.origin),
          cookie: tryIt(() => document.cookie),
          storage: tryIt(() => localStorage.length),
          parentDoc: tryIt(() => parent.document.title),
          parentLocation: tryIt(() => parent.location.href),
          top: tryIt(() => { top.location.href = "https://example.com/"; return "moved"; }),
        };
        out.fetch = await fetch("/app/").then((r) => `ok:${r.status}`, (e) => `refused:${e.name}`);
        out.socket = await new Promise((res) => { try { const w = new WebSocket(`ws://${location.host || "127.0.0.1"}/ws`); w.onopen = () => res("open"); w.onerror = () => res("refused:error"); setTimeout(() => res("refused:timeout"), 3000); } catch (e) { res(`refused:${e.name}`); } });
        return out;
      });
      assert(probe.origin === "null", `an opaque origin: ${probe.origin}`);
      for (const k of ["cookie", "storage", "parentDoc", "parentLocation", "top", "fetch", "socket"]) assert(probe[k].startsWith("refused"), `${k} is refused in the frame: ${JSON.stringify(probe)}`);
      assert(page.url().includes(`#/ticket/${tid}`), `the app was not moved: ${page.url()}`);

      // A height said from anywhere but the frame's own window is ignored, its channel or not.
      const forged = await page.evaluate(() => {
        const f = document.querySelector("[data-surface=page-frame][data-kind=page] iframe");
        const channel = f.getAttribute("src").split("#")[1];
        const before = f.getBoundingClientRect().height;
        window.postMessage({ type: "menger:size", channel, height: 1999 }, "*");
        return new Promise((r) => setTimeout(() => r({ before, after: f.getBoundingClientRect().height }), 400));
      });
      assert(forged.after === forged.before, `a forged height is ignored: ${JSON.stringify(forged)}`);

      // A real submission never leaves: nothing is sent to its action (form-action 'none'), the app stays put.
      const sent = [];
      await context.route("https://example.com/**", (r) => { sent.push(r.request().url()); return r.fulfill({ contentType: "text/html", body: "<p>away</p>" }); });
      await f0.locator("#realgo").click();
      await sleep(600);
      const where = (await inner(page, S, 0)).url();
      console.log(`    after a real submit the frame is at ${where}`);
      assert(sent.length === 0 && !where.startsWith("https://example.com"), `a real submission went nowhere: ${JSON.stringify({ sent, where })}`);
      assert(page.url().includes(`#/ticket/${tid}`), `the app was not moved: ${page.url()}`);

      // The width switch: 1440, scaled into the column, its script now at 1440.
      const elPf = page.locator(`${S} ${PF}`).first();
      await elPf.locator("[data-action=width][data-width='1440']").click();
      // The page loads again at 1440 behind the one on screen, which stays until the new one says its height.
      const swap = [];
      for (let k = 0; k < 20; k++) { swap.push(await elPf.locator("iframe[data-buffer=shown]").count()); await sleep(50); }
      assert(swap.every((c) => c === 1), `a frame on screen all through the switch: ${swap}`);
      await eventually("drawn at 1440", async () => (await elPf.getAttribute("data-width")) === "1440" && (await elPf.locator("iframe").count()) === 1, 8_000);
      const at1440 = await elPf.evaluate((e) => ({ col: e.querySelector(".pf-stage").getBoundingClientRect().width, box: e.querySelector(".pf-box").getBoundingClientRect().width, frame: e.querySelector("iframe").offsetWidth, pressed: e.querySelector("[data-action=width][data-width='1440']").getAttribute("aria-pressed") }));
      assert(at1440.frame === 1440 && at1440.box <= at1440.col + 1 && at1440.pressed === "true", `1440 scaled into the column: ${JSON.stringify(at1440)}`);
      const f0b = await inner(page, S, 0);
      await eventually("the page ran again at 1440", async () => (await f0b.locator("#ran").textContent()) === "yes, at 1440px", 8_000);
      await noSideways(page, "the ticket at 1440");
      // The probe's own refusals (a fetch, a socket, a real submit) are expected; nothing else may be refused.
      const csp = seen.console.filter((l) => /Content Security Policy/i.test(l) && !/connect-src|Refused to connect|Fetch API cannot load|form-action|Refused to send form|Framing 'https:\/\/example\.com/.test(l));
      assert(csp.length === 0, `no policy refused a page's own script or style: ${csp.join("\n")}`);
    } finally {
      await context.close();
    }
  });

  await test("a_whole_page_body_is_one_frame", async () => {
    const { context, page } = await openPage(`#/ticket/${wid}`);
    const S = "[data-surface=ticket-page]";
    try {
      await drawn(page, S, 1);
      const f = await page.locator(`${S} ${PF}`).evaluateAll((els) => els.map((e) => ({ label: e.querySelector("[data-field=label]")?.textContent, widths: [...e.querySelectorAll("[data-action=width]")].map((b) => b.dataset.width), h: e.querySelector("iframe").getBoundingClientRect().height })));
      assert(f.length === 1 && f[0].label === "A whole page" && JSON.stringify(f[0].widths) === '["390","768","1440"]', `one frame, its title and widths: ${JSON.stringify(f)}`);
      const fr = await inner(page, S, 0);
      assert((await fr.locator("#ran").textContent()) === "whole, run", "its script ran");
      assert(f[0].h > 1400 && f[0].h <= 2000, `as tall as its content, capped: ${f[0].h}`);
    } finally {
      await context.close();
    }
  });

  await test("in_the_editor_a_page_is_a_widget_and_the_caret_shows_its_source", async () => {
    const { context, page } = await openPage(`#/edit/${tid}`);
    const S = "[data-surface=editor-screen]";
    try {
      // Tall, so CodeMirror draws every block at once (it draws its viewport and a margin).
      await page.setViewportSize({ width: 1440, height: 6000 });
      await eventually("the editor", async () => (await page.locator(`${BODYSEL} .cm-content`).count()) === 1, 10_000);
      await page.evaluate((s) => document.querySelector(s).editorView.contentDOM.blur(), BODYSEL);
      await drawn(page, S, 3);
      const f0 = await inner(page, S, 0);
      await eventually("the element's script ran in the editor", async () => (await f0.locator("#ran").textContent()) === "yes, at 390px", 8_000);
      await f0.locator("#more").click();
      assert((await f0.locator("#count").textContent()) === "1", "its controls work in the editor");
      const text = await page.evaluate((s) => document.querySelector(s).editorView.state.doc.toString(), BODYSEL);
      assert(text === BODY, "the document is the body, byte for byte");
      // The caret into the element's page: its source shows, the frame goes; out again: drawn again.
      const at = text.indexOf("Blank lines survive");
      await page.evaluate(({ s, at }) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: at }, scrollIntoView: true }); }, { s: BODYSEL, at });
      await eventually("the page's source shows", async () => page.evaluate(() => [...document.querySelectorAll(".cm-line")].some((l) => l.textContent.includes("Blank lines survive")) && [...document.querySelectorAll(".cm-line")].some((l) => l.textContent.startsWith("<m-mockup widths"))), 5_000);
      const frames = await page.locator(`${S} ${PF}[data-kind=page]`).count();
      assert(frames === 2, `the caret's page shows as source, the other two drawn (${frames})`);
      // Typing in it changes the page, and leaving draws the new one.
      await page.keyboard.type(" Edited.");
      const end = text.indexOf("The last paragraph");
      await page.evaluate(({ s, at }) => { const v = document.querySelector(s).editorView; v.dispatch({ selection: { anchor: at } }); }, { s: BODYSEL, at: end + 8 });
      await drawn(page, S, 3);
      const f0b = await inner(page, S, 0);
      await eventually("the edited page drawn", async () => (await f0b.locator("main .card").first().textContent()).includes("Edited."), 8_000);
      // A click on the frame's label enters its source; on the width switch it does not.
      await page.evaluate((s) => document.querySelector(s).editorView.contentDOM.blur(), BODYSEL);
      await sleep(200);
      await page.locator(`${S} ${PF} [data-action=width][data-width='1440']`).first().click();
      await eventually("1440 in the editor", async () => (await page.locator(`${S} ${PF}`).first().getAttribute("data-width")) === "1440", 5_000);
      await noSideways(page, "the editor at 1440");
    } finally {
      await context.close();
    }
  });

  for (const w of [390, 320]) await test(`at_${w}_a_page_fits_the_column`, async () => {
    for (const [route, S] of [[`#/ticket/${tid}`, "[data-surface=ticket-page]"], [`#/edit/${tid}`, "[data-surface=editor-screen]"], [`#/ticket/${wid}`, "[data-surface=ticket-page]"]]) {
      const { context, page } = await openPage(route);
      try {
        await page.setViewportSize({ width: w, height: 4000 });
        if (route.startsWith("#/edit/")) {
          await eventually("the editor", async () => (await page.locator(`${BODYSEL} .cm-content`).count()) === 1, 10_000);
          await page.evaluate((s) => document.querySelector(s).editorView.contentDOM.blur(), BODYSEL);
        }
        await drawn(page, S, route.includes(wid) ? 1 : 3);
        await sleep(300);
        await noSideways(page, `${route} at ${w}`);
        const fit = await page.locator(`${S} ${PF}[data-kind=page]`).evaluateAll((els) => els.map((e) => ({ col: e.querySelector(".pf-stage").getBoundingClientRect().width, box: e.querySelector(".pf-box").getBoundingClientRect().width, width: e.dataset.width })));
        for (const f of fit) assert(f.box <= f.col + 1, `${route} at ${w}: a frame fits its column: ${JSON.stringify(f)}`);
        const wide = route.includes(wid) ? { box: 0 } : fit.find((f) => f.width === "1440");
        assert(wide && wide.box < 400, `${route} at ${w}: a 1440 page scaled down: ${JSON.stringify(fit)}`);
      } finally {
        await context.close();
      }
    }
  });

  await test("a_body_holding_raw_html_is_one_frame_with_its_markdown_around_it", async () => {
    for (const [route, S] of [[`#/ticket/${rid}`, "[data-surface=ticket-page]"], [`#/edit/${rid}`, "[data-surface=editor-screen]"]]) {
      const { context, page } = await openPage(route);
      try {
        if (route.startsWith("#/edit/")) {
          await eventually("the editor", async () => (await page.locator(`${BODYSEL} .cm-content`).count()) === 1, 10_000);
          await page.evaluate((s) => document.querySelector(s).editorView.contentDOM.blur(), BODYSEL);
        }
        await eventually("the body framed", async () => (await page.locator(`${S} ${PF}[data-kind=body][data-state=drawn]`).count()) === 1, 15_000);
        assert((await page.locator(`${S} ${PF}`).count()) === 1, `${route}: one frame, the whole body`);
        const sandbox = await page.locator(`${S} ${PF} iframe`).getAttribute("sandbox");
        assert(sandbox === "allow-scripts allow-forms allow-popups allow-popups-to-escape-sandbox", `${route}: a body's sandbox, never same-origin: ${sandbox}`);
        const el = await page.locator(`${S} ${PF} iframe`).elementHandle();
        const fr = await el.contentFrame();
        await eventually("its script ran", async () => (await fr.locator("#n").getAttribute("data-ran")) === "yes", 5_000);
        await fr.locator("#b").click();
        assert((await fr.locator("#n").textContent()) === "1", `${route}: an inline handler works`);
        const look = await fr.evaluate(() => ({
          strong: document.querySelector("strong")?.textContent, link: document.querySelector("a[href='https://example.com/away']")?.textContent,
          base: document.querySelector("base")?.getAttribute("target"), step: !!document.querySelector("m-step[status=Coding]"),
          ink: getComputedStyle(document.documentElement).getPropertyValue("--ink").trim(),
          cookie: (() => { try { return document.cookie; } catch (e) { return `refused:${e.name}`; } })(), demo: getComputedStyle(document.querySelector(".demo")).paddingTop,
        }));
        const appInk = await page.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--ink").trim());
        assert(look.strong === "Markdown" && look.link === "a link" && look.base === "_blank" && look.step && look.demo === "8px", `${route}: markdown around the raw HTML, its own style kept: ${JSON.stringify(look)}`);
        assert(look.ink && look.ink === appInk, `${route}: the app's tokens in the frame (${look.ink} / ${appInk})`);
        assert(look.cookie.startsWith("refused"), `${route}: no cookie: ${look.cookie}`);
        // The nested page ran in its own frame inside.
        const kid = fr.childFrames()[0];
        assert(kid, `${route}: the page block is a nested frame`);
        await eventually("the nested page ran", async () => (await kid.locator("#nested").textContent()) === "nested, run", 5_000);
        // The theme flips: the frame takes the new tokens without a reload.
        const flipped = await page.evaluate(() => { const d = document.documentElement; const was = getComputedStyle(d).getPropertyValue("--bg").trim(); d.dataset.theme = d.dataset.theme === "dark" ? "light" : "dark"; return { was, now: getComputedStyle(d).getPropertyValue("--bg").trim() }; });
        assert(flipped.was !== flipped.now, `the app's theme flipped: ${JSON.stringify(flipped)}`);
        await eventually("the frame re-themed", async () => (await fr.evaluate(() => getComputedStyle(document.documentElement).getPropertyValue("--bg").trim())) === flipped.now, 3_000);
        assert((await fr.locator("#n").textContent()) === "1", `${route}: re-themed in place, not reloaded`);
        if (route.startsWith("#/edit/")) {
          // One widget; the caret in it shows the body's source.
          await page.evaluate(({ s, at }) => { const v = document.querySelector(s).editorView; v.focus(); v.dispatch({ selection: { anchor: at } }); }, { s: BODYSEL, at: RAW.indexOf("Markdown") });
          await eventually("the source shows", async () => page.evaluate(() => [...document.querySelectorAll(".cm-line")].some((l) => l.textContent.includes('<button id="b"'))), 5_000);
          assert((await page.locator(`${S} ${PF}`).count()) === 0, "the caret's body is its source");
        } else {
          // A link opens a tab (its `<base target="_blank">`); the app stays where it is.
          await context.route("https://example.com/**", (r) => r.fulfill({ contentType: "text/html", body: "<p>away</p>" }));
          const [tab] = await Promise.all([context.waitForEvent("page", { timeout: 5000 }), fr.locator("a[href='https://example.com/away']").click()]);
          await tab.waitForLoadState().catch(() => {});
          assert(tab.url().startsWith("https://example.com/away"), `the link opened a tab: ${tab.url()}`);
          assert(page.url().includes(`#/ticket/${rid}`), `the app was not moved: ${page.url()}`);
          await tab.close();
          // An amend reloads it behind the one on screen: never no frame.
          sy("tickets", "amend", "--id", rid, "--body", file(RAW.replace("The end.", "The end, amended.")));
          const seenShown = [];
          for (let k = 0; k < 30; k++) { seenShown.push(await page.locator(`${S} ${PF} iframe[data-buffer=shown]`).count()); await sleep(50); }
          assert(seenShown.every((c) => c === 1), `a frame on screen through the amend: ${seenShown}`);
          await eventually("the amended body drawn", async () => {
            const f = await (await page.locator(`${S} ${PF} iframe[data-buffer=shown]`).elementHandle()).contentFrame();
            return (await f.locator("body").textContent()).includes("The end, amended.");
          }, 8_000);
          sy("tickets", "amend", "--id", rid, "--body", file(RAW));
        }
      } finally {
        await context.close();
      }
    }
  });

  // PAGES_SHOTS=DIR: the ticket page with its frames, and the raw body, at 390 and 1440, light and dark.
  if (process.env.PAGES_SHOTS) await test("shots", async () => {
    for (const [name, id, n] of [["forms", tid, 3], ["raw", rid, 0]]) for (const w of [390, 1440]) for (const theme of ["light", "dark"]) {
      const { context, page } = await openPage(`#/ticket/${id}`);
      const S = "[data-surface=ticket-page]";
      try {
        await page.setViewportSize({ width: w, height: w < 641 ? 844 : 900 });
        await page.evaluate((th) => (document.documentElement.dataset.theme = th), theme);
        if (n) await drawn(page, S, n); else await eventually("framed", async () => (await page.locator(`${S} ${PF}[data-state=drawn]`).count()) === 1, 15_000);
        await page.locator(`${S} ${PF}`).first().evaluate((e) => e.scrollIntoView({ block: "start" }));
        await page.evaluate(() => window.scrollBy(0, -60));
        await sleep(500);
        await page.screenshot({ path: path.join(process.env.PAGES_SHOTS, `frames-${name}-${w}-${theme}.png`) });
      } finally {
        await context.close();
      }
    }
  });

  // FRAMES_REAL=DIR: real bodies, as stored, kept outside the repository.
  const real = process.env.FRAMES_REAL;
  if (real) for (const name of fs.readdirSync(real).filter((f) => f.endsWith(".md")).sort()) await test(`a_real_body_draws_its_pages_${name.replace(/\W+/g, "_")}`, async () => {
    const src = fs.readFileSync(path.join(real, name), "utf8");
    const id = made(`real ${name}`, src, root);
    for (const [route, S, width] of [[`#/ticket/${id}`, "[data-surface=ticket-page]", 1440], [`#/ticket/${id}`, "[data-surface=ticket-page]", 390], [`#/edit/${id}`, "[data-surface=editor-screen]", 390]]) {
      const { context, page } = await openPage(route);
      try {
        await page.setViewportSize({ width, height: 4000 });
        if (route.startsWith("#/edit/")) {
          await eventually("the editor", async () => (await page.locator(`${BODYSEL} .cm-content`).count()) === 1, 10_000);
          await page.evaluate((s) => document.querySelector(s).editorView.contentDOM.blur(), BODYSEL);
        }
        await eventually(`${name}: a frame`, async () => (await page.locator(`${S} ${PF}[data-kind=page]`).count()) >= 1, 15_000);
        const total = await page.locator(`${S} ${PF}[data-kind=page]`).count();
        await drawn(page, S, total);
        await sleep(300);
        await noSideways(page, `${name} ${route} at ${width}`);
        console.log(`    ${name}: ${total} page(s) drawn on ${route} at ${width}`);
      } finally {
        await context.close();
      }
    }
  });
};
