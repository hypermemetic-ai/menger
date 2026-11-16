// CODE-3: code-ide — a repository read through the code service (CODE-2), drawn in a body by its tag and at
// #/code/… by page-code. The throwaway's code service reads a stand-in for GitHub (tests/code-github.mjs,
// the hidden --code-github-at): `hypermemetic-ai/kiln` (main, glazes, feature/slash-name, the tag v1.2.0;
// a 20,000-line src/big.rs, a binary assets/logo.png; main moves to a tip without src/log.rs),
// `hypermemetic-ai/menger` (main and v2), and `hypermemetic-ai/secret`, which GitHub would not show.
// Run alone by `node tests/pages_browser.mjs --group code-ide` (`--seed 200` is plenty). With
// CODE_IDE_SHOTS=<dir> the tag's page is shot at 390 and 1440 wide, light and dark.
//
// Marks: code-ide's `[data-surface=code-ide]` (`data-state`, `data-commit`, `data-pinned`, `data-drawer`,
// `data-full`), `[data-field=commit|status|marked|path]`, `[data-action=…]`, `[data-notice=…][data-reason]`;
// code-ide-tree's `input[data-finder]`, `[data-dir][aria-expanded]`, `[data-file]`; code-ide-tabs'
// `[data-tab][aria-selected]`, `[data-close]`; code-ide-view's `[data-surface=code-ide-view]` and
// CodeMirror's `.cm-line` (`.cm-marked`) and `.cm-gutterElement`.
import { INIT, sentCalls, synapse } from "../../scripts/ui-throwaway.mjs";
import { makeKey, readToken, sessionCookies, writeSession } from "../../scripts/throwaway-login.mjs";

export default async (ctx) => {
  const { test, assert, eventually, sleep, openPage, sy, server, path, fs, github, token, sameDocument, marked: markPage } = ctx;
  const EPIC = "CODEIDE";
  const KILN = "hypermemetic-ai/kiln";
  const MAIN = github.hashOf("kiln main"), MOVED = github.hashOf("kiln main moved"), V120 = github.hashOf("kiln v1.2.0");
  let k = 0;
  const bodyFile = (body) => {
    const f = path.join(server.work, `code-ide-${++k}.md`);
    fs.writeFileSync(f, body);
    return f;
  };
  const make = (title, body = null, ...flags) => {
    const out = sy("tickets", "create", "--epic", EPIC, "--title", title, ...(body == null ? [] : ["--body", `@${bodyFile(body)}`]), ...flags);
    const c = out.find((e) => e.type === "created");
    if (!c) throw new Error(`create ${title}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const root = make("code in bodies", null, "--kind", "scope");
  const withTag = (title, tag) => make(title, `A repository, where the tag says.\n\n${tag}\n\nWords after it.\n`, "--parent", root, "--kind", "build");

  const TAG = `<code-ide repo="${KILN}" ref="main" dir="src" open="src/cones.rs#L5-L10, src/log.rs"></code-ide>`;
  const ide = (page) => page.locator("[data-surface=tag-component][data-name=code-ide] [data-surface=code-ide]").first();
  const state = (loc) => loc.getAttribute("data-state", { timeout: 1000 }).catch(() => null);
  async function ready(page, loc, seen, what = "code-ide ready") {
    await eventually(what, async () => (await state(loc)) === "ready", 30_000).catch(async (e) => {
      const notice = await loc.locator("[data-notice]").textContent({ timeout: 500 }).catch(() => "(none)");
      throw new Error(`${e.message}\nstate ${await state(loc)}; notice: ${notice}\nconsole: ${seen?.console.slice(-8).join("\n") ?? ""}`);
    });
  }
  const tabsOf = (loc) => loc.locator("[data-tab]").evaluateAll((els) => els.map((e) => `${e.dataset.tab}${e.getAttribute("aria-selected") === "true" ? "*" : ""}`));
  const shownText = (loc) => loc.locator("[data-surface=code-ide-view] .cm-content").textContent({ timeout: 1000 }).catch(() => "");
  const marked = (loc) => loc.locator("[data-surface=code-ide-view] .cm-line.cm-marked").count();
  const codeCalls = async (page) => (await sentCalls(page)).filter((c) => c.method?.startsWith("code."));
  /** Whether the line `n` of the shown file is inside the view's scroller, on screen. */
  const lineOnScreen = (loc, n) => loc.locator("[data-surface=code-ide-view]").evaluate((el, n) => {
    const v = el.codeView;
    const line = v.state.doc.line(n);
    const top = v.lineBlockAt(line.from).top, s = v.scrollDOM;
    return top >= s.scrollTop - 1 && top < s.scrollTop + s.clientHeight;
  }, n);
  const gutter = (loc, n) => loc.locator(".cm-lineNumbers .cm-gutterElement").filter({ hasText: new RegExp(`^${n}$`) }).first();
  const clip = (page) => page.evaluate(() => navigator.clipboard.readText());
  const grantClip = (context) => context.grantPermissions(["clipboard-read", "clipboard-write"], { origin: server.origin });

  // ── a viewer, and a commit nobody fetched ───────────────────────────────
  await test("code_ide_a_viewer_is_told_someone_must_fetch_it", async () => {
    const id = withTag("menger's main, read by a viewer", `<code-ide repo="hypermemetic-ai/menger" ref="main" open="README.md"></code-ide>`);
    const keyFile = path.join(server.work, "code-viewer.key");
    const principal = makeKey(keyFile);
    const g = sy("members", "grant", "--principal", principal, "--scope", "*", "--role", "viewer");
    assert(g.some((e) => e.type === "grant"), `viewer granted: ${JSON.stringify(g)}`);
    const file = path.join(server.work, "code-viewer.session");
    await writeSession(`ws://127.0.0.1:${server.ws}`, keyFile, file, "viewer");
    const { context, page, seen } = await openPage(`#/ticket/${id}`, { as: readToken(file) });
    try {
      const c = ide(page);
      await eventually("the view says nobody fetched it", async () => (await state(c)) === "unfetched", 30_000);
      const words = await c.locator("[data-notice=unfetched]").textContent();
      assert(/not kept by menger yet/.test(words) && /must open this view once/.test(words), `said in place: ${words}`);
      await sleep(500);
      const calls = (await sentCalls(page)).map((x) => x.method);
      assert(!calls.includes("code.fetch") && !calls.includes("code.tree") && !calls.includes("code.read"), `nothing it would be refused, nothing not kept: ${calls.join(", ")}`);
      assert(calls.includes("code.open") && calls.includes("auth.requires") && calls.includes("auth.whoami"), `asked what it may: ${calls.join(", ")}`);
    } finally {
      await context.close();
      sy("members", "revoke", "--principal", principal, "--scope", "*");
    }
  });

  // ── the tag's scenario, and what a person does in it ────────────────────
  await test("code_ide_the_tag_opens_the_ref_the_dir_and_the_tabs", async () => {
    const id = withTag("the kiln, where the tag says", TAG);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    await grantClip(context);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      assert((await page.locator("[data-surface=tag-component][data-name=code-ide]").getAttribute("data-mode")) === "native", "drawn natively, a bundle component");
      assert((await c.getAttribute("data-commit")) === MAIN && (await c.getAttribute("data-pinned")) === "false", "main's tip");
      assert((await c.locator("[data-field=commit]").textContent()).startsWith(MAIN.slice(0, 8)), "the head shows the commit");
      assert((await c.locator("[data-action=refresh]").count()) === 1, "a branch: refresh");
      assert((await c.locator("[data-dir=src]").getAttribute("aria-expanded")) === "true", "src/ expanded");
      assert((await c.locator("[data-file='src/cones.rs']").count()) === 1 && (await c.locator("[data-dir=ui]").getAttribute("aria-expanded")) === "false", "src's files drawn, ui/ closed");
      assert(JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs*", "src/log.rs"]), `two tabs, cones.rs shown: ${await tabsOf(c)}`);
      await eventually("cones.rs drawn", async () => (await shownText(c)).includes("pub fn known"));
      assert((await marked(c)) === 6, `lines 5 to 10 marked: ${await marked(c)}`);
      assert((await lineOnScreen(c, 5)) && (await lineOnScreen(c, 10)), "the marked lines in view");
      assert((await c.locator("[data-field=marked]").textContent()).includes("L5–L10"), "the path line says so");
      assert((await c.locator("[data-action=github]").getAttribute("href")) === `https://github.com/${KILN}/blob/${MAIN}/src/cones.rs#L5-L10`, "GitHub at the commit and the lines");
      // Painted by its language, once its grammar's module came.
      await eventually("painted as Rust", async () => (await c.locator("[data-surface=code-ide-view]").getAttribute("data-painted")) === "yes" && (await c.locator(".cm-content .tok-keyword").count()) > 0, 10_000);

      // A tab shown again asks nothing: log.rs read once, then ten switches.
      await c.locator("[data-tab='src/log.rs']").click();
      await eventually("log.rs drawn", async () => (await shownText(c)).includes("cones::known"));
      const before = (await codeCalls(page)).length;
      for (let i = 0; i < 10; i++) {
        await c.locator(`[data-tab='${i % 2 ? "src/log.rs" : "src/cones.rs"}']`).click();
        await eventually("switched", async () => (await shownText(c)).includes(i % 2 ? "cones::known" : "pub fn known"), 2_000);
      }
      const after = await codeCalls(page);
      assert(after.length === before, `ten switches, no call: ${after.slice(before).map((x) => `${x.method} ${JSON.stringify(x.params)}`).join("; ")}`);
      const reads = after.filter((x) => x.method === "code.read").map((x) => x.params.file);
      assert(new Set(reads).size === reads.length, `each file read once: ${reads.join(", ")}`);
      assert(after.filter((x) => x.method === "code.tree").length === 1, "the tree asked once");

      // A file under the pointer 150 ms is read ahead; opened, it asks nothing more.
      const readsOf = async (file) => (await codeCalls(page)).filter((x) => x.method === "code.read" && x.params.file === file).length;
      await c.locator("[data-file='src/lib.rs']").hover();
      await eventually("lib.rs read ahead", async () => (await readsOf("src/lib.rs")) === 1, 3_000);
      // Worked by the keyboard: a row by Enter, a tab closed by Delete.
      await c.locator("[data-file='src/lib.rs']").focus();
      await page.keyboard.press("Enter");
      await eventually("lib.rs opened by Enter", async () => (await tabsOf(c)).includes("src/lib.rs*") && (await shownText(c)).includes("pub mod store"));
      assert((await readsOf("src/lib.rs")) === 1, "read once: the read-ahead's answer drawn");
      await c.locator("[data-tab='src/lib.rs']").focus();
      await page.keyboard.press("Delete");
      await eventually("lib.rs closed by Delete, its neighbour shown", async () => JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs", "src/log.rs*"]));

      // The finder: `stor`, Enter.
      await c.locator("[data-tab='src/cones.rs']").click();
      const box = c.locator("input[data-finder]");
      await box.fill("stor");
      await eventually("matches drawn", async () => (await c.locator("[data-match]").first().getAttribute("data-match")) === "src/store.rs");
      await box.press("Enter");
      await eventually("store.rs is the shown tab", async () => JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs", "src/log.rs", "src/store.rs*"]));
      assert((await box.inputValue()) === "", "the finder left");

      // The view as a tag: three files, the first's marks.
      await c.locator("[data-action=copy-tag]").click();
      const tag = await eventually("copied", async () => { const t = await clip(page); return t.startsWith("<code-ide") ? t : null; });
      assert(tag === `<code-ide repo="${KILN}" ref="main" dir="src" open="src/cones.rs#L5-L10, src/log.rs, src/store.rs"></code-ide>`, `the tag: ${tag}`);
      // A link, and one that never moves.
      await c.locator("[data-action=copy-link]").click();
      const link = await eventually("link copied", async () => { const t = await clip(page); return t.includes("#/code/") ? t : null; });
      const view = "dir=src;open=src/cones.rs#L5-L10,src/log.rs,src/store.rs";
      assert(link === `${server.origin}/app/#/code/${KILN}/main/${encodeURIComponent(view).replace(/%3D/g, "=").replace(/%2C/g, ",").replace(/%3B/g, ";")}`, `the link: ${link}`);
      await c.locator("[data-action=copy-permalink]").click();
      const perma = await eventually("permalink copied", async () => { const t = await clip(page); return t.includes(`/${MAIN}/`) ? t : null; });
      assert(perma.startsWith(`${server.origin}/app/#/code/${KILN}/${MAIN}/`), `the permanent link: ${perma}`);

      // ctrl+p from inside the view: the finder.
      await c.locator("[data-surface=code-ide-view] .cm-line").first().click();
      await page.keyboard.press("Control+p");
      await eventually("ctrl+p focuses the finder", async () => page.evaluate(() => document.activeElement?.matches?.("input[data-finder]")), 2_000);
      await page.keyboard.press("Escape");

      // Line marks: a click, then a shift-click.
      await c.locator("[data-tab='src/cones.rs']").click();
      await eventually("cones shown", async () => (await shownText(c)).includes("pub fn known"));
      await gutter(c, 3).click();
      await eventually("line 3 marked", async () => (await marked(c)) === 1 && (await c.locator("[data-field=marked]").textContent()).trim() === "L3 marked");
      await gutter(c, 7).click({ modifiers: ["Shift"] });
      await eventually("lines 3 to 7 marked", async () => (await marked(c)) === 5 && (await c.locator("[data-field=marked]").textContent()).includes("L3–L7"));
      assert((await c.locator("[data-action=github]").getAttribute("href")).endsWith("/src/cones.rs#L3-L7"), "GitHub follows the marks");

      // Tabs close: by ×, the neighbour shown; by a middle click.
      await c.locator("[data-tab='src/log.rs']").click();
      await c.locator("[data-close='src/log.rs']").click();
      await eventually("log.rs closed, its neighbour shown", async () => JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs", "src/store.rs*"]));
      await c.locator("[data-tab='src/store.rs']").click({ button: "middle" });
      await eventually("store.rs closed by a middle click", async () => JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs*"]));

      // Fill the window, and Escape puts it back.
      await c.locator("[data-action=full]").click();
      await eventually("full", async () => (await c.getAttribute("data-full")) === "true");
      const boxFull = await c.boundingBox();
      assert(Math.abs(boxFull.width - 1440) < 2 && Math.abs(boxFull.height - 900) < 2 && boxFull.x === 0 && boxFull.y === 0, `fills the window: ${JSON.stringify(boxFull)}`);
      await page.keyboard.press("Escape");
      await eventually("back in place", async () => (await c.getAttribute("data-full")) === "false");
      assert(page.url().includes(`/ticket/`) || /#\/t\//.test(page.url()), `Escape left the ticket's page where it was: ${page.url()}`);
    } finally {
      await context.close();
    }
  });

  await test("code_ide_a_github_address_alone", async () => {
    const id = withTag("a blob's address", `<code-ide repo="https://github.com/${KILN}/blob/main/src/cones.rs#L5-L10"></code-ide>`);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      assert((await c.getAttribute("data-commit")) === MAIN, "main, from the address");
      assert((await c.locator("[data-dir=src]").getAttribute("aria-expanded")) === "true", "the tree open at src");
      assert(JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs*"]), `one tab: ${await tabsOf(c)}`);
      await eventually("lines 5 to 10 marked", async () => (await marked(c)) === 6);
      // A branch whose name holds a slash: the longest of the listing's names.
      const t2 = withTag("a tree's address on a slashed branch", `<code-ide repo="https://github.com/${KILN}/tree/feature/slash-name/src"></code-ide>`);
      const second = await openPage(`#/ticket/${t2}`);
      try {
        const c2 = ide(second.page);
        await ready(second.page, c2, second.seen);
        assert((await c2.getAttribute("data-commit")) === github.hashOf("kiln glazes"), `feature/slash-name's tip: ${await c2.getAttribute("data-commit")}`);
        assert((await c2.locator("[data-dir=src]").getAttribute("aria-expanded")) === "true" && (await c2.locator("[data-file='src/glaze.rs']").count()) === 1, "src/ open, glazes' file in it");
        assert((await c2.locator("[data-notice=pick]").count()) === 1, "no tab: a line asking for a file");
      } finally {
        await second.context.close();
      }
    } finally {
      await context.close();
    }
  });

  await test("code_ide_a_pinned_commit", async () => {
    const id = withTag("a pinned commit", `<code-ide repo="${KILN}" commit="${V120.slice(0, 8)}" dir="src, nope" open="src/cones.rs, src/log.rs"></code-ide>`);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      assert((await c.getAttribute("data-commit")) === V120 && (await c.getAttribute("data-pinned")) === "true", "the commit, in full, pinned");
      assert((await c.locator("[data-field=commit]").textContent()).includes("pinned"), "the head says pinned");
      assert((await c.locator("[data-action=refresh]").count()) === 0, "no refresh");
      assert((await c.locator("[data-field=crumb]").textContent()).includes(`nope is not in the tree at ${V120.slice(0, 8)}`) && (await c.locator("[data-dir=src]").getAttribute("aria-expanded")) === "true", "a directory the tree lacks said in the crumb, the rest open");
      await eventually("cones.rs drawn", async () => (await shownText(c)).includes("pub fn known"));
      await c.locator("[data-tab='src/log.rs']").click();
      assert((await c.locator("[data-notice=gone]").textContent()).includes("src/log.rs is not in the tree at"), "the tab the tree lacks says so");
    } finally {
      await context.close();
    }
  });

  await test("code_ide_the_ref_control_keeps_the_tabs", async () => {
    const id = withTag("the kiln, its refs", `<code-ide repo="${KILN}" open="src/cones.rs, README.md"></code-ide>`);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      assert((await c.getAttribute("data-commit")) === MAIN, "no ref: the default branch");
      const sel = c.locator("select[data-field=ref]");
      await eventually("the branches and tags listed", async () => {
        const opts = await sel.locator("option").evaluateAll((os) => os.map((o) => o.value));
        return ["b:main", "b:glazes", "b:feature/slash-name", "t:v1.2.0", "other"].every((v) => opts.includes(v));
      });
      await sel.selectOption("b:glazes");
      await eventually("glazes' tip", async () => (await c.getAttribute("data-commit")) === github.hashOf("kiln glazes") && (await state(c)) === "ready", 20_000);
      assert(JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs*", "README.md"]), `the tabs kept: ${await tabsOf(c)}`);
      await sel.selectOption("t:v1.2.0");
      await eventually("the tag's commit", async () => (await c.getAttribute("data-commit")) === V120 && (await state(c)) === "ready", 20_000);
      assert((await c.locator("[data-action=refresh]").count()) === 0, "a tag: no refresh");
      // A commit by its hash, and pin.
      await sel.selectOption("other");
      const hash = c.locator("[data-field=commit-input]");
      await hash.fill(MAIN.slice(0, 10));
      await hash.press("Enter");
      await eventually("pinned at the hash", async () => (await c.getAttribute("data-commit")) === MAIN && (await c.getAttribute("data-pinned")) === "true", 20_000);
      await sel.selectOption("b:main");
      await eventually("main again", async () => (await c.getAttribute("data-pinned")) === "false" && (await state(c)) === "ready", 20_000);
      await c.locator("[data-action=pin]").click();
      assert((await c.getAttribute("data-pinned")) === "true" && (await c.locator("[data-action=refresh]").count()) === 0, "pinned: no refresh");
    } finally {
      await context.close();
    }
  });

  await test("code_ide_a_repository_github_will_not_show", async () => {
    const id = withTag("a private repository", `<code-ide repo="hypermemetic-ai/secret" open="README.md"></code-ide>`);
    const bad = withTag("not a repository", `<code-ide repo="git.example.org/a/b"></code-ide>`);
    const { context, page } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await eventually("refused", async () => (await state(c)) === "refused", 30_000);
      const n = c.locator("[data-notice=refused]");
      assert((await n.getAttribute("data-reason")) === "not_public", `not_public: ${await n.getAttribute("data-reason")}`);
      const words = await n.textContent();
      assert(/not a public repository on GitHub/.test(words) && /only public repositories are read/i.test(words), `said in place: ${words}`);
      await page.goto(`${server.origin}/app/#/ticket/${bad}`);
      const c2 = ide(page);
      await eventually("bad_repo", async () => (await c2.locator("[data-notice=refused]").getAttribute("data-reason", { timeout: 500 })) === "bad_repo", 30_000);
      assert(/owner\/name/.test(await c2.locator("[data-notice=refused]").textContent()), "what to do");
    } finally {
      await context.close();
    }
  });

  // ── a file of 20,000 lines, and a binary one ────────────────────────────
  await test("code_ide_a_file_of_20000_lines", async () => {
    const id = withTag("a large file", `<code-ide repo="${KILN}" ref="main" dir="assets" open="src/big.rs, assets/logo.png"></code-ide>`);
    // A page of our own: the frame carrying the answer stamped as it arrives, the first line stamped when drawn.
    const { page, context: cx, seen } = await openTimed(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      await eventually("the big file drawn", async () => (await shownText(c)).includes("the big file"), 20_000);
      const t = await eventually("the answer, the first line and the next frame stamped", async () => {
        const v = await page.evaluate(() => window.__code);
        return v.answered != null && v.drawn != null && v.framed != null ? v : null;
      }, 5_000);
      const drawnMs = t.drawn - t.answered, framedMs = t.framed - t.answered;
      const counts = await page.evaluate(() => ({ lines: document.querySelectorAll(".cm-line").length, gutter: document.querySelectorAll(".cm-gutterElement").length, all: document.getElementsByTagName("*").length }));
      console.log(`     the 20,000-line file: drawn ${drawnMs.toFixed(1)} ms after its answer arrived (the next frame at ${framedMs.toFixed(1)} ms); ${counts.lines} .cm-line, ${counts.gutter} gutter elements, ${counts.all} elements in the document`);
      assert(drawnMs < 200 && framedMs < 200, `the first screen within 200 ms of the answer: ${drawnMs.toFixed(1)} / ${framedMs.toFixed(1)} ms`);
      assert(counts.lines < 1000 && counts.gutter < 1000, `fewer than 1,000 line elements: ${JSON.stringify(counts)}`);
      assert((await c.locator("[data-field=path]").textContent()).includes("20000 lines"), "the path line counts them");
      // Scrolled to the end: the last line drawn, still few.
      await c.locator("[data-surface=code-ide-view]").evaluate((el) => { el.codeView.scrollDOM.scrollTop = 1e9; });
      await eventually("the last line drawn", async () => (await shownText(c)).includes("line 20000 of the big file"), 5_000);
      assert((await page.evaluate(() => document.querySelectorAll(".cm-line").length)) < 1000, "still fewer than 1,000");
      // A binary file: its size, and GitHub.
      await c.locator("[data-tab='assets/logo.png']").click();
      const n = c.locator("[data-notice=binary]");
      await eventually("binary said", async () => (await n.count()) === 1, 5_000);
      assert(/binary file, 1\.2 KiB/.test(await n.textContent()), `its size: ${await n.textContent()}`);
      assert((await n.locator("a").getAttribute("href")) === `https://github.com/${KILN}/blob/${MAIN}/assets/logo.png`, "a link to GitHub");
    } finally {
      await cx.close();
    }
  });

  /** A page whose code.read answer of 20,000 lines is stamped when its frame arrives (before the engine hears
   *  it) and whose first line is stamped when it is in the document, and at the next frame. */
  async function openTimed(route) {
    const context = await ctx.browser.newContext({ viewport: { width: 1440, height: 900 } });
    await context.addCookies(sessionCookies(server.origin, token));
    await context.addInitScript(INIT);
    await context.addInitScript(() => {
      window.__code = { answered: null, drawn: null, framed: null };
      const C = window.__code;
      const W = window.WebSocket;
      window.WebSocket = class extends W {
        constructor(...a) {
          super(...a);
          this.addEventListener("message", (e) => { if (C.answered == null && typeof e.data === "string" && e.data.includes('"lines":20000')) C.answered = performance.now(); });
        }
      };
      new MutationObserver(() => {
        if (C.answered == null || C.drawn != null) return;
        for (const l of document.querySelectorAll(".cm-line")) if (l.textContent.startsWith("// the big file")) {
          C.drawn = performance.now();
          requestAnimationFrame(() => { C.framed = performance.now(); });
          break;
        }
      }).observe(document, { childList: true, subtree: true, characterData: true });
    });
    const page = await context.newPage();
    const seen = { console: [] };
    page.on("console", (m) => seen.console.push(`${m.type()}: ${m.text()}`));
    await page.goto(`${server.origin}/app/${route}`);
    return { page, context, seen };
  }

  // ── the address ─────────────────────────────────────────────────────────
  await test("code_ide_the_address_opened_cold_and_following_the_view", async () => {
    const view = encodeURIComponent("dir=ui/src;open=ui/routes.json,src/shell.rs#L183-L187");
    const { context, page, seen } = await openPage(`#/code/hypermemetic-ai/menger/v2/${view}`);
    try {
      const c = page.locator("[data-surface=page-code] [data-surface=code-ide]");
      await ready(page, c, seen);
      assert((await c.getAttribute("data-commit")) === github.hashOf("menger v2"), "v2's tip");
      assert((await c.locator("[data-dir='ui/src']").getAttribute("aria-expanded")) === "true" && (await c.locator("[data-dir=ui]").getAttribute("aria-expanded")) === "true", "the tree open at ui/src");
      assert(JSON.stringify(await tabsOf(c)) === JSON.stringify(["ui/routes.json*", "src/shell.rs"]), `both tabs: ${await tabsOf(c)}`);
      const box = await c.boundingBox();
      assert(box.width >= 1439 && box.height >= 899, `fills the page: ${JSON.stringify(box)}`);
      await c.locator("[data-tab='src/shell.rs']").click();
      await eventually("lines 183 to 187 of the second marked, in view", async () => (await marked(c)) === 5 && (await lineOnScreen(c, 183)), 5_000);
      // The address follows: README.md opened, three files named; no history entry for it.
      const length = await page.evaluate(() => history.length);
      await c.locator("[data-file='README.md']").click();
      const want = `#/code/hypermemetic-ai/menger/v2/dir=ui/src;open=ui%2Froutes.json,src%2Fshell.rs%23L183-L187,README.md`;
      await eventually("the address names three files", async () => decodeURIComponent(new URL(page.url()).hash) === decodeURIComponent(want), 3_000).catch(() => { throw new Error(`the address: ${page.url()}`); });
      assert((await page.evaluate(() => history.length)) === length, "replaced, not pushed");
      await page.reload();
      const c2 = page.locator("[data-surface=page-code] [data-surface=code-ide]");
      await ready(page, c2, seen, "ready after the reload");
      assert(JSON.stringify((await tabsOf(c2)).map((t) => t.replace("*", ""))) === JSON.stringify(["ui/routes.json", "src/shell.rs", "README.md"]), `the same three tabs: ${await tabsOf(c2)}`);
      // A route's ref may be a commit: pinned.
      await page.goto(`${server.origin}/app/#/code/hypermemetic-ai/menger/${github.hashOf("menger v2").slice(0, 10)}`);
      const c3 = page.locator("[data-surface=page-code] [data-surface=code-ide]");
      await eventually("pinned by the address", async () => (await c3.getAttribute("data-pinned", { timeout: 500 })) === "true" && (await state(c3)) === "ready", 20_000);
    } finally {
      await context.close();
    }
  });

  // ── CODE-6: follow — an agent the reader follows opens files in the reader's view ──
  // The agent is a second key, signed in as `builder`, an editor on `*` (ui.show and ui.sessions are an
  // editor's); the reader is the run's owner.
  const agentKey = path.join(server.work, "code-agent.key");
  const agentPrincipal = makeKey(agentKey);
  const granted = sy("members", "grant", "--principal", agentPrincipal, "--scope", "*", "--role", "editor");
  assert(granted.some((e) => e.type === "grant"), `builder granted: ${JSON.stringify(granted)}`);
  const agentSession = path.join(server.work, "code-agent.session");
  await writeSession(`ws://127.0.0.1:${server.ws}`, agentKey, agentSession, "builder");
  const agent = (...a) => synapse(server, agentSession, ...a);
  const sessionsOf = () => agent("ui", "sessions").filter((e) => e.type === "session");
  const show = (to, ...more) => agent("ui", "show", "--to", to, ...more);
  const kilnAt = (ref, view) => `/app/#/code/${KILN}/${encodeURIComponent(ref)}/${encodeURIComponent(view).replace(/%3D/g, "=").replace(/%2C/g, ",").replace(/%3B/g, ";")}`;
  const followBox = (c) => c.locator("input[data-action=follow]");
  const watchesWithSession = async (page) => (await sentCalls(page)).filter((x) => x.method === "ui.watch" && x.params?.session);
  /** Follow turned on by the reader; the agent sees one session, where the page is. */
  async function followOn(page, c, route) {
    await followBox(c).check();
    return eventually("builder sees the reader's session", async () => {
      const s = sessionsOf();
      return s.length === 1 && s[0].route && (!route || s[0].route === route) ? s[0] : null;
    }, 10_000).catch(async (e) => { throw new Error(`${e.message}: ${JSON.stringify(sessionsOf())}`); });
  }

  await test("code_ide_follow_is_off_until_the_reader_turns_it_on", async () => {
    const id = withTag("the kiln, followed", TAG);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      assert(!(await followBox(c).isChecked()) && (await c.locator("input[data-field=follow-who]").inputValue()) === "builder", "off, builder named");
      await sleep(600);
      assert(!(await watchesWithSession(page)).length, "no session opened");
      assert(!(await sentCalls(page)).some((x) => x.method === "ui.here" || x.method === "ui.follow"), "nothing said");
      assert(sessionsOf().length === 0, `builder sees no session: ${JSON.stringify(sessionsOf())}`);
      const nobody = show(kilnAt("main", "open=src/store.rs"));
      assert(nobody.some((e) => e.type === "refused" && e.reason === "nobody_follows"), `a show reaches nobody: ${JSON.stringify(nobody)}`);
      // Turned on: one session, its route the page's, kept in this browser.
      const s = await followOn(page, c);
      assert(s.route.startsWith("/app/#/") && s.route.includes(id), `its route the page's: ${s.route}`);
      assert(JSON.stringify(s.follows) === JSON.stringify(["builder"]), `it follows builder: ${JSON.stringify(s)}`);
      const w = await watchesWithSession(page);
      assert(w.length >= 1 && /^s-[0-9a-f]{12}$/.test(w[0].params.session) && JSON.stringify(w[0].params.follows) === JSON.stringify(["builder"]), `ui.watch with the page's session: ${JSON.stringify(w.map((x) => x.params))}`);
      const kept = await page.evaluate(() => localStorage.getItem("menger.code-ide.follow"));
      assert(JSON.parse(kept)?.on === true, `kept in the browser: ${kept}`);
      // Kept: the page opened again follows from the start, with a session of its own.
      await page.reload();
      const c2 = ide(page);
      await ready(page, c2, seen, "ready after the reload");
      assert(await followBox(c2).isChecked(), "still on after a reload");
      await eventually("one session again", async () => sessionsOf().length === 1, 10_000);
    } finally {
      await context.close();
    }
  });

  await test("code_ide_follow_a_file_opened_for_the_reader", async () => {
    const id = withTag("the kiln, shown a file", TAG);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      await eventually("cones.rs drawn", async () => (await shownText(c)).includes("pub fn known"));
      await followOn(page, c);
      const origin = await markPage(page);
      const length = await page.evaluate(() => history.length);
      const hash = await page.evaluate(() => location.hash);
      const a = show(kilnAt("main", "open=src/store.rs#L3-L5"), "--say", "where a firing is **appended**", "--point", "[data-tab='src/log.rs']");
      assert(a.some((e) => e.type === "shown" && e.sessions === 1), `shown to one session: ${JSON.stringify(a)}`);
      await eventually("store.rs shown beside the reader's tabs", async () => JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs", "src/log.rs", "src/store.rs*"]), 10_000)
        .catch(async (e) => { throw new Error(`${e.message}: ${await tabsOf(c)}\n${seen.console.slice(-8).join("\n")}`); });
      await eventually("lines 3 to 5 marked and in view", async () => (await shownText(c)).includes("pub fn append") && (await marked(c)) === 3 && (await lineOnScreen(c, 3)), 5_000);
      const note = c.locator("[data-surface=code-ide-note]");
      assert((await note.getAttribute("data-by")) === "builder", "the note says who");
      assert(/where a firing is appended/.test(await note.textContent()) && (await note.locator("strong").count()) === 1, `its words, markdown drawn: ${await note.innerHTML()}`);
      assert((await c.locator("[data-dir=src]").getAttribute("aria-expanded")) === "true", "its directory open");
      await eventually("the pointed tab ringed", async () => (await c.locator("[data-tab='src/log.rs'][data-code-ide-ring]").count()) === 1, 3_000);
      await sameDocument(page, origin, "a show applied in place");
      assert((await page.evaluate(() => location.hash)) === hash && (await page.evaluate(() => history.length)) === length, "no navigation, no history entry");
      // A second show replaces the note; dismissed, the ring goes too.
      show(kilnAt("main", "open=src/cones.rs#L5"), "--say", "and here the cone is checked");
      await eventually("cones.rs shown, its mark replaced", async () => (await tabsOf(c)).includes("src/cones.rs*") && (await marked(c)) === 1, 10_000);
      assert((await c.locator("[data-surface=code-ide-note]").count()) === 1 && /the cone is checked/.test(await note.textContent()), "the note replaced");
      await c.locator("[data-action=dismiss]").click();
      await eventually("the note gone", async () => (await note.count()) === 0 && (await c.locator("[data-code-ide-ring]").count()) === 0, 3_000);
      // Keyboard: the toggle and the name are reachable, and the dismiss leaves the focus on the toggle.
      assert(await followBox(c).evaluate((el) => el === document.activeElement), "focus back on follow");
      await sameDocument(page, origin, "after the second show");
    } finally {
      await context.close();
    }
  });

  await test("code_ide_follow_another_branch", async () => {
    const id = withTag("the kiln, shown a branch", TAG);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      assert((await c.getAttribute("data-commit")) === MAIN, "on main");
      await followOn(page, c);
      show(kilnAt("glazes", "dir=src;open=src/glaze.rs#L1-L2"), "--say", "the celadon");
      await eventually("glazes' tip", async () => (await c.getAttribute("data-commit")) === github.hashOf("kiln glazes") && (await state(c)) === "ready", 20_000);
      assert((await c.locator("select[data-field=ref]").inputValue()) === "b:glazes" && (await c.getAttribute("data-pinned")) === "false", "the head shows glazes, at its tip");
      await eventually("glaze.rs drawn at that commit, its lines marked", async () => (await shownText(c)).includes("celadon") && (await marked(c)) === 2, 10_000);
      assert(JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs", "src/log.rs", "src/glaze.rs*"]), `the reader's tabs kept: ${await tabsOf(c)}`);
    } finally {
      await context.close();
    }
  });

  await test("code_ide_follow_not_a_code_address", async () => {
    const id = withTag("the kiln, shown a ticket", TAG);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      await followOn(page, c);
      const origin = await markPage(page);
      const hash = await page.evaluate(() => location.hash);
      const before = await tabsOf(c);
      show("/app/#/ticket/KILN-2", "--say", "the firing to look at");
      const note = c.locator("[data-surface=code-ide-note]");
      await eventually("the note drawn", async () => (await note.count()) === 1, 10_000);
      assert(/the firing to look at/.test(await note.textContent()), "its words");
      assert((await note.locator("a[data-field=note-to]").getAttribute("href")) === "/app/#/ticket/KILN-2", "a link to the ticket");
      assert(JSON.stringify(await tabsOf(c)) === JSON.stringify(before) && (await c.getAttribute("data-commit")) === MAIN, "the view stays as it is");
      await sameDocument(page, origin, "a ticket shown");
      assert((await page.evaluate(() => location.hash)) === hash, "not navigated");
    } finally {
      await context.close();
    }
  });

  await test("code_ide_follow_turned_off", async () => {
    const id = withTag("the kiln, unfollowed", TAG);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      await followOn(page, c);
      await followBox(c).uncheck();
      assert((await sentCalls(page)).some((x) => x.method === "ui.follow" && JSON.stringify(x.params.follows) === "[]"), "ui.follow said at once: nobody");
      await eventually("builder sees no session", async () => sessionsOf().length === 0, 5_000);
      const before = await tabsOf(c);
      const a = show(kilnAt("main", "open=src/store.rs#L3-L5"), "--say", "not for you");
      assert(a.some((e) => e.type === "refused" && e.reason === "nobody_follows"), `nobody follows: ${JSON.stringify(a)}`);
      await sleep(800);
      assert(JSON.stringify(await tabsOf(c)) === JSON.stringify(before) && (await c.locator("[data-surface=code-ide-note]").count()) === 0, "the view did not move");
      assert(JSON.parse(await page.evaluate(() => localStorage.getItem("menger.code-ide.follow")))?.on === false, "kept off");
      // Another name: the session follows it, at once.
      await c.locator("input[data-field=follow-who]").fill("someone-else");
      await c.locator("input[data-field=follow-who]").press("Enter");
      await followBox(c).check();
      await sleep(600);
      assert(sessionsOf().length === 0, "following someone else: builder reaches nobody");
      await c.locator("input[data-field=follow-who]").fill("builder");
      await c.locator("input[data-field=follow-who]").press("Enter");
      assert((await sentCalls(page)).some((x) => x.method === "ui.follow" && JSON.stringify(x.params.follows) === '["builder"]'), "ui.follow said for the new name");
      await eventually("builder again", async () => sessionsOf().length === 1, 5_000);
      // The framework's knock (FRAME-48): the show that reached nobody was kept for this reader, and the next
      // session of theirs that opens following builder hears it once: the page opened again.
      await page.reload();
      const c2 = ide(page);
      await ready(page, c2, seen, "ready after the reload");
      await eventually("the show kept while off arrives once a session follows again", async () => /not for you/.test(await c2.locator("[data-surface=code-ide-note]").textContent({ timeout: 500 }).catch(() => "")), 10_000)
        .catch(async (e) => { throw new Error(`${e.message}: tabs ${await tabsOf(c2)}; sessions ${JSON.stringify(sessionsOf())}`); });
    } finally {
      await page.evaluate(() => localStorage.removeItem("menger.code-ide.follow")).catch(() => {});
      await context.close();
    }
  });

  await test("code_ide_follow_two_views_one_session_and_the_page", async () => {
    // Two views on one page: one session; the show of menger taken by the view showing menger.
    const id = withTag("two repositories", `<code-ide repo="${KILN}" ref="main" open="src/cones.rs"></code-ide>\n\n<code-ide repo="hypermemetic-ai/menger" ref="v2" open="README.md"></code-ide>`);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const all = page.locator("[data-surface=tag-component][data-name=code-ide] [data-surface=code-ide]");
      await eventually("two views", async () => (await all.count()) === 2, 20_000);
      const [k, m] = [all.nth(0), all.nth(1)];
      await ready(page, k, seen);
      await ready(page, m, seen, "menger ready");
      await followOn(page, k);
      assert(await followBox(m).isChecked(), "the choice is every view's");
      assert(new Set((await watchesWithSession(page)).map((x) => x.params.session)).size === 1, "one session for the page");
      show("/app/#/code/hypermemetic-ai/menger/v2/open=src%2Fshell.rs%23L183-L187", "--say", "the page policy");
      await eventually("menger's view takes it", async () => (await tabsOf(m)).includes("src/shell.rs*") && (await marked(m)) === 5, 10_000)
        .catch(async (e) => { throw new Error(`${e.message}: kiln ${await k.getAttribute("data-repo")} ${await tabsOf(k)}; menger ${await m.getAttribute("data-repo")} ${await tabsOf(m)} marked ${await marked(m)}; notes ${await page.locator("[data-surface=code-ide-note]").count()}\n${seen.console.slice(-8).join("\n")}`); });
      assert(JSON.stringify(await tabsOf(k)) === JSON.stringify(["src/cones.rs*"]) && (await k.locator("[data-surface=code-ide-note]").count()) === 0,
        `the kiln's view untouched: ${await tabsOf(k)}; its note: ${await k.locator("[data-surface=code-ide-note]").textContent({ timeout: 500 }).catch(() => "none")}`);
    } finally {
      await context.close();
    }
    // At #/code/…: where the page is follows the view; another repository taken in place.
    const view = encodeURIComponent("dir=src;open=src/cones.rs");
    const second = await openPage(`#/code/${KILN}/main/${view}`);
    try {
      const { page: p2, seen: s2 } = second;
      const c = p2.locator("[data-surface=page-code] [data-surface=code-ide]");
      await ready(p2, c, s2);
      const s = await followOn(p2, c);
      assert(s.route.startsWith(`/app/#/code/${KILN}/main/`), `its route the code address: ${s.route}`);
      await c.locator("[data-file='src/log.rs']").click();
      await eventually("ui.here follows the address", async () => sessionsOf()[0]?.route?.includes("log.rs"), 5_000);
      const origin = await markPage(p2);
      const length = await p2.evaluate(() => history.length);
      show("/app/#/code/hypermemetic-ai/menger/v2/dir=ui%2Fsrc;open=README.md", "--say", "now menger");
      await eventually("menger opened in place", async () => (await c.getAttribute("data-repo")) === "hypermemetic-ai/menger" && (await state(c)) === "ready" && JSON.stringify(await tabsOf(c)) === JSON.stringify(["README.md*"]), 20_000);
      assert((await c.locator("[data-dir='ui/src']").getAttribute("aria-expanded")) === "true", "its directory open");
      await eventually("the address follows", async () => new URL(p2.url()).hash.startsWith("#/code/hypermemetic-ai/menger/v2/"), 3_000);
      await sameDocument(p2, origin, "another repository, in place");
      assert((await p2.evaluate(() => history.length)) === length, "no history entry");
      await eventually("ui.here says menger", async () => sessionsOf()[0]?.route?.includes("hypermemetic-ai/menger"), 5_000);
    } finally {
      await second.context.close();
    }
  });

  if (process.env.CODE_IDE_FOLLOW_SHOTS) {
    await test("code_ide_follow_shots", async () => {
      const dir = process.env.CODE_IDE_FOLLOW_SHOTS;
      fs.mkdirSync(dir, { recursive: true });
      const id = withTag("the kiln, followed, shot", TAG);
      for (const scheme of ["light", "dark"]) for (const width of [390, 1440]) {
        const { context, page, seen } = await openPage(`#/ticket/${id}`);
        try {
          await page.emulateMedia({ colorScheme: scheme });
          await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
          const c = ide(page);
          await ready(page, c, seen);
          await followOn(page, c);
          show(kilnAt("main", "open=src/store.rs#L3-L5"), "--say", "This is where a firing is appended. One row a firing: `store::append`.");
          await eventually("shown", async () => (await marked(c)) === 3 && (await c.locator("[data-surface=code-ide-note]").count()) === 1, 10_000);
          await eventually("painted", async () => (await c.locator("[data-surface=code-ide-view]").getAttribute("data-painted")) === "yes", 10_000);
          await c.scrollIntoViewIfNeeded();
          await sleep(300);
          await page.screenshot({ path: path.join(dir, `follow-${width}-${scheme}.png`) });
          await c.screenshot({ path: path.join(dir, `follow-${width}-${scheme}-alone.png`) });
        } finally {
          await context.close();
        }
      }
    });
  }

  // ── a phone ─────────────────────────────────────────────────────────────
  await test("code_ide_at_390_the_tree_is_a_drawer", async () => {
    const id = withTag("the kiln on a phone", TAG);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      await page.setViewportSize({ width: 390, height: 844 });
      const c = ide(page);
      await ready(page, c, seen);
      await eventually("cones.rs drawn", async () => (await shownText(c)).includes("pub fn known"));
      assert(!(await c.locator("[data-surface=code-ide-tree]").isVisible()), "the tree is put away");
      const wide = await page.evaluate(() => ({ doc: document.documentElement.scrollWidth, view: innerWidth }));
      assert(wide.doc <= wide.view, `no sideways page scroll: ${JSON.stringify(wide)}`);
      await c.locator("[data-action=files]").click();
      await eventually("the drawer open", async () => (await c.locator("[data-surface=code-ide-tree]").isVisible()) && (await c.getAttribute("data-drawer")) === "open");
      const tb = await c.locator("[data-surface=code-ide-tree]").boundingBox(), cb = await c.boundingBox();
      assert(tb.x - cb.x < 3 && tb.width < cb.width, `over the view, from its left: ${JSON.stringify({ tb, cb })}`);
      await c.locator("[data-file='src/store.rs']").click();
      await eventually("opened, the drawer shut", async () => (await c.getAttribute("data-drawer")) === "closed" && JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs", "src/log.rs", "src/store.rs*"]));
      const again = await page.evaluate(() => ({ doc: document.documentElement.scrollWidth, view: innerWidth }));
      assert(again.doc <= again.view, `still no sideways scroll: ${JSON.stringify(again)}`);
    } finally {
      await context.close();
    }
  });

  // Screenshots of the tag's view, when asked: 390 and 1440, light and dark.
  if (process.env.CODE_IDE_SHOTS) {
    await test("code_ide_shots", async () => {
      const dir = process.env.CODE_IDE_SHOTS;
      fs.mkdirSync(dir, { recursive: true });
      const id = withTag("the kiln, shot", TAG);
      for (const scheme of ["light", "dark"]) for (const width of [390, 1440]) {
        const { context, page, seen } = await openPage(`#/ticket/${id}`);
        try {
          await page.emulateMedia({ colorScheme: scheme });
          await page.setViewportSize({ width, height: width === 390 ? 844 : 900 });
          const c = ide(page);
          await ready(page, c, seen);
          await eventually("painted", async () => (await c.locator("[data-surface=code-ide-view]").getAttribute("data-painted")) === "yes", 10_000);
          await c.scrollIntoViewIfNeeded();
          await page.screenshot({ path: path.join(dir, `code-ide-${width}-${scheme}.png`) });
          await c.screenshot({ path: path.join(dir, `code-ide-${width}-${scheme}-alone.png`) });
          if (width === 390) {
            await c.locator("[data-action=files]").click();
            await c.screenshot({ path: path.join(dir, `code-ide-${width}-${scheme}-drawer.png`) });
          }
        } finally {
          await context.close();
        }
      }
      const { context, page, seen } = await openPage(`#/code/${KILN}/main/${encodeURIComponent("dir=src;open=src/cones.rs#L5-L10,src/log.rs")}`);
      try {
        const c = page.locator("[data-surface=page-code] [data-surface=code-ide]");
        await ready(page, c, seen);
        await sleep(800);
        await page.screenshot({ path: path.join(dir, "page-code-1440-light.png") });
      } finally {
        await context.close();
      }
    });
  }

  // ── the branch moves (last: main stays moved) ───────────────────────────
  await test("code_ide_refresh_after_the_branch_moved", async () => {
    const id = withTag("the kiln, refreshed", TAG);
    const { context, page, seen } = await openPage(`#/ticket/${id}`);
    try {
      const c = ide(page);
      await ready(page, c, seen);
      assert((await c.getAttribute("data-commit")) === MAIN, "main's tip");
      github.move(KILN, "main", "kiln main moved");
      // The service keeps GitHub's listing a minute: refresh once it has gone stale.
      const listed = () => github.asked.filter((u) => u.startsWith(`/${KILN}.git/info/refs`)).length;
      const was = listed();
      await sleep(61_000);
      await c.locator("[data-action=refresh]").click();
      await eventually("the head shows the new commit", async () => (await c.getAttribute("data-commit")) === MOVED && (await state(c)) === "ready", 30_000);
      assert(listed() > was, "GitHub's listing asked again");
      assert((await c.locator("[data-field=commit]").textContent()).startsWith(MOVED.slice(0, 8)), "the head's commit");
      assert((await c.locator("[data-field=status]").textContent()).includes("moved"), "said: it moved");
      await eventually("cones.rs at the new commit", async () => (await shownText(c)).includes("pub fn known"));
      await c.locator("[data-tab='src/log.rs']").click();
      assert((await c.locator("[data-notice=gone]").textContent()).includes(`src/log.rs is not in the tree at ${MOVED.slice(0, 8)}`), "the gone file's tab says so");
      assert(JSON.stringify(await tabsOf(c)) === JSON.stringify(["src/cones.rs", "src/log.rs*"]), "the tabs stay");
    } finally {
      await context.close();
    }
  });
};
