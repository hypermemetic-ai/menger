// `<spec-outline>` in a ticket's body (spec-outline.svelte). Written empty it draws the body's outline, read as
// an OpenSpec change: the counts, the sections as links, each group of requirements a line that opens, a
// requirement with no scenario marked, the open questions; a click scrolls the body to the heading; an amend
// redraws it. Holding the spec between its open and close lines it is the spec: its sections as tabs, a
// requirement opening to its statement, its code and its scenarios' steps, nothing of it drawn outside; a
// click in it stays in it, and `edit source` shows its markdown. Run alone by
// `node tests/pages_browser.mjs --group spec`.
//
// Marks: `[data-surface=spec-outline]` (`[data-holds]`), `[data-stat=requirements|scenarios|tasks|questions]`,
// `[data-section]` (`[aria-selected]`), `[data-panel]`, `[data-group=<name>]` (`[aria-expanded]`),
// `[data-req=<name>]` (`[aria-expanded]`, `[data-bare]`), `[data-req-body]`, `[data-scenario]`, `[data-step]`,
// `[data-question]`, `[data-edit]`.

const EPIC = "SPEC";

export default async (ctx) => {
  const { test, assert: ok, eventually, openPage, sy, fs, path, session } = ctx;
  const same = (got, want, what = "") => ok(JSON.stringify(got) === JSON.stringify(want), `${what || "got"} ${JSON.stringify(got)}, wanted ${JSON.stringify(want)}`);
  const assert = { equal: same, deepEqual: same, match: (got, re) => ok(re.test(got), `${JSON.stringify(got)} does not match ${re}`) };
  const bodyFile = (name, text) => { const f = path.join(path.dirname(session), `spec-${name}.md`); fs.writeFileSync(f, text); return `@${f}`; };
  const createdId = (out, what) => {
    const c = out.find((e) => e.type === "created" || e.ticket?.id);
    if (!c) throw new Error(`create ${what}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const filler = Array.from({ length: 40 }, (_, i) => `Line ${i + 1} of the reasons.`).join("\n\n");
  const BODY = (tasks) => `<spec-outline></spec-outline>

# The kiln

## Why

${filler}

## Requirements

### firing

#### Requirement: A firing can be paused

The kiln SHALL hold its temperature when a firing is paused.

##### Scenario: paused at the hold
- **WHEN** a firing is paused at 1040
- **THEN** it holds 1040

##### Scenario: resumed
- **WHEN** it is resumed
- **THEN** the schedule carries on

#### Requirement: A firing is logged

Every firing SHALL be logged.

### glazes

#### Requirement: A glaze names its cone

A glaze SHALL name the cone it matures at.

##### Scenario: cone six
- **WHEN** a glaze is added with cone 6
- **THEN** it is listed under cone 6

## Design

\`\`\`md
#### Requirement: Inside a fence, never counted
- [ ] nor this
\`\`\`

### Open Questions

1. **Gas or electric.** Which kiln first?
2. Who loads the shelves?

## Tasks

${tasks}
`;
  const root = createdId(sy("tickets", "create", "--epic", EPIC, "--kind", "scope", "--title", "the kiln's spec", "--body", bodyFile("a", BODY("- [x] 1. Wire the kiln\n- [ ] 2. Fire it\n- [ ] 3. Unload"))), "root");
  const S = "[data-surface=ticket-body] [data-surface=spec-outline]";
  const open = async () => {
    const opened = await openPage(`#/ticket/${root}`);
    await eventually("the outline is drawn", async () => (await opened.page.locator(S).count()) === 1, 15_000);
    return opened;
  };
  const stat = (page, name) => page.locator(`${S} [data-stat=${name}] b`).innerText();

  await test("spec_outline_counts_what_the_body_holds", async () => {
    const { context, page } = await open();
    try {
      assert.equal(await stat(page, "requirements"), "3");
      assert.equal(await stat(page, "scenarios"), "3");
      assert.equal(await stat(page, "tasks"), "1/3");
      assert.equal(await stat(page, "questions"), "2");
      assert.deepEqual(await page.locator(`${S} [data-section]`).allInnerTexts(), ["Why", "Requirements", "Design", "Tasks"]);
      assert.deepEqual(await page.locator(`${S} [data-group]`).evaluateAll((els) => els.map((e) => e.dataset.group)), ["firing", "glazes"]);
      assert.deepEqual(await page.locator(`${S} [data-question]`).allInnerTexts(), ["Gas or electric", "Who loads the shelves?"]);
    } finally { await context.close(); }
  });

  await test("spec_outline_a_group_opens_and_a_requirement_scrolls_to_its_heading", async () => {
    const { context, page } = await open();
    try {
      const firing = page.locator(`${S} [data-group=firing]`);
      assert.equal(await firing.getAttribute("aria-expanded"), "false");
      assert.match(await firing.innerText(), /1 with no scenario/);
      assert.equal(await page.locator(`${S} [data-req]`).count(), 0);
      await firing.click();
      assert.equal(await firing.getAttribute("aria-expanded"), "true");
      assert.deepEqual(await page.locator(`${S} [data-req]`).evaluateAll((els) => els.map((e) => [e.dataset.req, e.dataset.bare != null])),
        [["A firing can be paused", false], ["A firing is logged", true]]);
      const heading = page.locator("[data-surface=ticket-body] :is(h1, h2, h3, h4, h5, h6)", { hasText: "Requirement: A firing is logged" });
      const inView = () => heading.evaluate((h) => { const r = h.getBoundingClientRect(); return r.top >= 0 && r.bottom <= innerHeight; });
      assert.equal(await inView(), false, "the heading starts below the fold");
      await page.locator(`${S} [data-req="A firing is logged"]`).click();
      await eventually("the heading is scrolled into view", inView, 5_000);
    } finally { await context.close(); }
  });

  await test("spec_outline_follows_an_amend", async () => {
    const { context, page } = await open();
    try {
      sy("tickets", "amend", "--id", root, "--body", bodyFile("b", BODY("- [x] 1. Wire the kiln\n- [x] 2. Fire it\n- [ ] 3. Unload\n- [ ] 4. Sort")));
      await eventually("the tasks count moves", async () => (await stat(page, "tasks")) === "2/4", 10_000);
    } finally { await context.close(); }
  });

  // ── holding the spec ─────────────────────────────────────────────────────
  const HELD = `An overview, outside the spec.

<spec-outline>
# The kiln

## Why

Because pots crack when a firing cannot wait.

## Requirements

### firing

#### Requirement: A firing can be paused

The kiln SHALL hold its temperature when a firing is paused.

\`\`\`rust
fn pause(&self) -> Box<dyn Kiln> { todo!() }
\`\`\`

##### Scenario: paused at the hold
- **WHEN** a firing is paused at \`1040\`
- **THEN** it holds 1040

#### Requirement: A firing is logged

Every firing SHALL be logged.

## Design

### Open Questions

<m-decision data-field="decision" id="fuel">

<m-question data-field="question">Gas or electric?</m-question>

<m-option data-field="option" name="Gas" value="gas" recommended></m-option>

<m-option data-field="option" name="Electric" value="electric"></m-option>

</m-decision>

## Tasks

- [x] 1. Wire the kiln
- [ ] 2. Fire it
</spec-outline>
`;
  const held = createdId(sy("tickets", "create", "--epic", EPIC, "--kind", "build", "--parent", root, "--title", "the kiln's spec, held", "--body", bodyFile("h", HELD)), "held");
  const H = "[data-surface=spec-outline][data-holds]";
  const openHeld = async () => {
    const opened = await openPage(`#/ticket/${held}`);
    await eventually("the spec is drawn", async () => (await opened.page.locator(H).count()) === 1, 15_000);
    return opened;
  };

  await test("spec_outline_holds_the_spec_and_draws_it", async () => {
    const { context, page } = await openHeld();
    try {
      assert.equal(await page.locator(`${H} .spec-title`).innerText(), "The kiln");
      assert.equal(await page.locator(`${H} [data-stat=requirements] b`).innerText(), "2");
      assert.equal(await page.locator(`${H} [data-stat=tasks] b`).innerText(), "1/2");
      assert.deepEqual(await page.locator(`${H} [data-section]`).evaluateAll((els) => els.map((e) => [e.dataset.section, e.getAttribute("aria-selected")])),
        [["Why", "true"], ["Requirements", "false"], ["Design", "false"], ["Tasks", "false"]]);
      assert.match(await page.locator(`${H} [data-panel=Why]`).innerText(), /pots crack/);
      // Nothing of the spec is drawn outside it: the one heading on the page beyond the component is none of its.
      assert.equal(await page.evaluate((sel) => [...document.querySelectorAll("h1, h2, h3, h4, h5, h6")].filter((h) => !h.closest(sel) && /Requirement:|The kiln$|^Why$/.test(h.textContent.trim())).length, H), 0);
      await page.locator(`${H} [data-section=Requirements]`).click();
      assert.deepEqual(await page.locator(`${H} [data-group]`).evaluateAll((els) => els.map((e) => e.dataset.group)), ["firing"]);
      const req = page.locator(`${H} [data-req="A firing can be paused"]`);
      assert.equal(await req.getAttribute("aria-expanded"), "false");
      assert.equal(await page.locator(`${H} [data-req="A firing is logged"]`).getAttribute("data-bare"), "");
      await req.click();
      const bodyOf = page.locator(`${H} [data-req-body]`);
      await eventually("the requirement's code is a card", async () => (await bodyOf.locator(".code").count()) === 1, 10_000);
      const said = await bodyOf.innerText();
      assert.match(said, /SHALL hold its temperature/);
      assert.match(said, /Box<dyn Kiln>/);
      assert.deepEqual(await bodyOf.locator("[data-scenario] [data-step]").evaluateAll((els) => els.map((e) => [e.dataset.step, e.querySelector("dd").textContent.trim()])),
        [["WHEN", "a firing is paused at 1040"], ["THEN", "it holds 1040"]]);
      // A question is an input of the body: listed while open, it goes to its section, and is answered there.
      await eventually("the question is listed", async () => (await page.locator(`${H} [data-question]`).allInnerTexts()).join() === "Gas or electric?", 10_000);
      assert.equal(await page.locator(`${H} [data-stat=questions] b`).innerText(), "1");
      await page.locator(`${H} [data-question]`).click();
      await eventually("the design tab is chosen", async () => (await page.locator(`${H} [data-section=Design]`).getAttribute("aria-selected")) === "true", 5_000);
      const q = page.locator(`${H} [data-input=fuel]`);
      assert.equal(await q.getAttribute("data-state"), "open");
      assert.deepEqual(await q.locator("[data-option]").evaluateAll((els) => els.map((e) => [e.dataset.option, e.dataset.recommended])), [["gas", "true"], ["electric", "false"]]);
      await q.locator("[data-option=electric]").click();
      await eventually("the answer is kept and said", async () => (await q.getAttribute("data-state")) === "answered" && /electric/.test(await q.locator("[data-field=answer]").innerText()), 10_000);
      const kept = sy("tickets", "inputs", "list", "--ticket", held).flatMap((e) => e.items ?? [e.input ?? e]).find((i) => i?.id === "fuel");
      assert.equal(kept?.answer?.value?.option, "electric", "the service's answer");
      await eventually("no question is left open", async () => (await page.locator(`${H} [data-question]`).count()) === 0, 10_000);
      await q.locator("[data-action='tickets.inputs.clear']").click();
      await eventually("cleared, it is open again", async () => (await q.getAttribute("data-state")) === "open", 10_000);
    } finally { await context.close(); }
  });

  await test("spec_outline_a_click_stays_in_it_and_edit_source_shows_its_markdown", async () => {
    const { context, page } = await openHeld();
    try {
      await page.locator(`${H} [data-panel=Why]`).click();
      await page.waitForTimeout(300);
      assert.equal(await page.locator(H).count(), 1, "a click on its words leaves it drawn");
      const edit = page.locator(`${H} [data-edit]`);
      if (await edit.count()) {
        await edit.click();
        let seen = "";
        try {
          await eventually("its markdown is shown", async () => { seen = `${await page.locator(H).count()} drawn; ${(await page.locator("[data-field=body-source]").innerText()).slice(0, 400)}`; return /^0 drawn/.test(seen) && /Requirement: A firing is logged/.test(await page.locator("[data-field=body-source]").innerText()); }, 8_000);
        } catch (e) { throw new Error(`${e.message}: ${JSON.stringify(seen)}`); }
      }
    } finally { await context.close(); }
  });
};

