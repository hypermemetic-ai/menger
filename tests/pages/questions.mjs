// V2-20: questions and the queue — a question answered in place on the
// ticket page (decision, multi, text), asked, queued; the queue at #/queue in
// its order, live, by its keys; `g q` from the board. Run alone by
// `node tests/pages_browser.mjs --group questions`.
//
// Marks: `[data-surface=ticket-questions]` (`[data-asking]`), an input
// `[data-input=<id>]` (`[data-kind]`, `[data-state]`, `[data-option]`,
// `[data-field=question|answer|state]`), `[data-form=tickets.inputs.ask]`,
// `[data-form=tickets.queue]`, `[data-field=queued]`, `[data-refused]`;
// `[data-surface=queue-page]`, a row `[data-id]` (`[data-urgency]`,
// `[data-selected]`, `[data-snoozed]`), `[data-band]`, `[data-field=count]`.
const EPIC = "ASKS";
const BODY = `<m-decision id="cone"><m-question>Which cone?</m-question><m-option name="Six" value="6" recommended></m-option><m-option name="Five" value="5"></m-option></m-decision>

<m-input id="checks" kind="multi"><m-question>Which ran?</m-question><m-choice value="unit">Unit</m-choice><m-choice value="wire">Wire</m-choice></m-input>

<m-input id="notes" kind="text"><m-question>Anything else?</m-question></m-input>
`;

export default async (ctx) => {
  const { test, assert, eventually, openPage, sy, fs, path, session, ticketAt } = ctx;
  const body = path.join(path.dirname(session), "questions-body.md");
  fs.writeFileSync(body, BODY);
  const created = (out, what) => {
    const c = out.find((e) => e.type === "created");
    if (!c) throw new Error(`create ${what}: ${JSON.stringify(out)}`);
    return c.ticket.id;
  };
  const root = created(sy("tickets", "create", "--epic", EPIC, "--kind", "scope", "--title", "the questions"), "root");
  const make = (title, ...flags) => created(sy("tickets", "create", "--epic", EPIC, "--parent", root, "--title", title, ...flags), title);

  await test("a_question_is_answered_in_place_each_kind", async () => {
    const id = make("every kind", "--body", `@${body}`);
    const { context, page } = await openPage(`#/ticket/${id}`, { signedIn: true });
    try {
      const q = page.locator("[data-surface=ticket-questions]");
      await q.locator("[data-input=cone]").waitFor();
      assert((await q.getAttribute("data-asking")) === "3", "three open");
      assert((await q.locator("[data-input=cone] [data-field=question]").innerText()) === "Which cone?", "the question");
      await q.locator("[data-input=cone] [data-option='6']").click();
      await eventually("the decision answered", async () => (await q.locator("[data-input=cone]").getAttribute("data-state")) === "answered");
      assert((await q.locator("[data-input=cone] [data-option='6']").getAttribute("aria-pressed")) === "true", "its option pressed");

      await q.locator("[data-input=checks] [data-option=wire]").check();
      await q.locator("[data-input=checks] [data-action='tickets.inputs.answer']").click();
      await eventually("the multi answered", async () => (await q.locator("[data-input=checks]").getAttribute("data-state")) === "answered");
      assert((await q.locator("[data-input=checks] [data-field=answer]").innerText()).includes("wire"), "what was ticked");

      // A blank reply: the service's words, in place.
      await q.locator("[data-input=notes] [data-action='tickets.inputs.answer']").click();
      await eventually("refused in place", async () => (await q.locator("[data-refused]").count()) === 1);
      assert((await q.locator("[data-refused]").innerText()).includes("not blank"), "the service's words");
      await q.locator("[data-input=notes] textarea").fill("ship it");
      await q.locator("[data-input=notes] textarea").press("Control+Enter");
      await eventually("the text answered", async () => (await q.locator("[data-input=notes]").getAttribute("data-state")) === "answered");
      await eventually("asking none", async () => (await q.getAttribute("data-asking")) === "0");

      // Another's answer, cleared, comes in by the watch.
      sy("tickets", "inputs", "clear", "--ticket", id, "--input", "notes");
      await eventually("cleared by another", async () => (await q.locator("[data-input=notes]").getAttribute("data-state")) === "open");
      // Closed, then reopened.
      await q.locator("[data-input=cone] [data-action='tickets.inputs.close']").click();
      await eventually("closed", async () => (await q.locator("[data-input=cone]").getAttribute("data-state")) === "closed");
      assert((await q.locator("[data-input=cone] [data-option]").count()) === 0, "a closed question takes no answer");
      await q.locator("[data-input=cone] [data-action='tickets.inputs.reopen']").click();
      await eventually("reopened", async () => (await q.locator("[data-input=cone]").getAttribute("data-state")) === "answered");
    } finally {
      await context.close();
    }
  });

  await test("a_question_is_asked_and_the_ticket_queued_by_its_keys", async () => {
    const id = make("nothing asked yet");
    const { context, page } = await openPage(`#/ticket/${id}`, { signedIn: true });
    try {
      const q = page.locator("[data-surface=ticket-questions]");
      await q.locator("[data-empty]").waitFor();
      await q.locator("[data-form='tickets.inputs.ask'] input").fill("Hold at cone 6?");
      await q.locator("[data-action='tickets.inputs.ask']").click();
      await q.locator("[data-input=ask-1]").waitFor();
      assert((await q.locator("[data-input=ask-1]").getAttribute("data-kind")) === "text", "a question is a text input");
      await page.keyboard.press("q");
      await eventually("q: the queue's form", () => page.evaluate(() => document.activeElement?.name === "reason"));
      await page.keyboard.type("the kiln waits");
      await q.locator("[data-form='tickets.queue'] select").selectOption("now");
      await q.locator("[data-input-name=ask-1]").check();
      await q.locator("[data-action='tickets.queue']").click();
      await q.locator("[data-field=queued]").waitFor();
      assert((await q.locator("[data-field=queued]").getAttribute("data-urgency")) === "now", "at its urgency");
      const row = sy("tickets", "queued", "--id", id).find((r) => r.type === "queue_row");
      assert(row?.entry?.input === "ask-1", `the entry names the question: ${JSON.stringify(row)}`);
      await q.locator("[data-action='tickets.unqueue']").click();
      await q.locator("[data-form='tickets.queue']").waitFor();
    } finally {
      await context.close();
    }
  });

  await test("the_queue_is_in_its_order_live_and_by_its_keys", async () => {
    const [low, top, mid] = [make("a whenever"), make("a blocker", "--body", `@${body}`), make("a now")];
    const q = (id, urgency, ...more) => sy("tickets", "queue", "--id", id, "--urgency", urgency, "--reason", `why ${id}`, ...more);
    q(low, "whenever");
    q(top, "blocking", "--input", "cone");
    const { context, page } = await openPage("#/queue", { signedIn: true });
    try {
      const s = page.locator("[data-surface=queue-page]");
      await s.locator(`[data-id="${top}"]`).waitFor();
      const ids = () => s.locator("[data-id]").evaluateAll((els) => els.map((e) => e.dataset.id));
      assert((await ids()).indexOf(top) < (await ids()).indexOf(low), "most urgent first");
      assert((await s.locator(`[data-id="${top}"] [data-field=reason]`).innerText()) === `why ${top}`, "why");
      assert((await s.locator(`[data-id="${top}"] [data-field=asks]`).innerText()).startsWith("1"), "what it asks");
      // Another queues: the row lands in its band, unread.
      q(mid, "now");
      await s.locator(`[data-id="${mid}"]`).waitFor();
      await eventually("in its order", async () => {
        const o = await ids();
        return o.indexOf(top) < o.indexOf(mid) && o.indexOf(mid) < o.indexOf(low);
      });
      // Raised, it moves up the order.
      q(low, "now");
      await eventually("raised", async () => (await s.locator(`[data-id="${low}"]`).getAttribute("data-urgency")) === "now");
      await eventually("low is above mid: the older", async () => {
        const o = await ids();
        return o.indexOf(low) < o.indexOf(mid);
      });
      // Out by another, it goes.
      sy("tickets", "unqueue", "--id", mid);
      await eventually("unqueued by another", async () => (await s.locator(`[data-id="${mid}"]`).count()) === 0);
    } finally {
      await context.close();
    }
  });

  await test("the_queues_keys_move_snooze_finish_and_open", async () => {
    const [a, b] = [make("key one"), make("key two")];
    for (const id of [a, b]) sy("tickets", "queue", "--id", id, "--urgency", "whenever", "--reason", "keys");
    const { context, page } = await openPage("#/queue", { signedIn: true });
    try {
      const s = page.locator("[data-surface=queue-page]");
      await s.locator(`[data-id="${b}"]`).waitFor();
      const selected = () => s.locator("[data-selected=true]").getAttribute("data-id");
      // To the last row, whatever the other cases left.
      const n = await s.locator("[data-id]").count();
      for (let i = 0; i < n; i++) await page.keyboard.press("j");
      assert((await selected()) === b, "j: down to the last");
      await page.keyboard.press("k");
      assert((await selected()) === a, "k: up one");
      await page.keyboard.press("s");
      await eventually("s: snoozed", async () => (await s.locator(`[data-id="${a}"]`).getAttribute("data-snoozed")) === "true");
      await page.keyboard.press("j");
      await page.keyboard.press("d");
      await eventually("d: out of the queue", async () => (await s.locator(`[data-id="${b}"]`).count()) === 0);
      await page.keyboard.press("k");
      await eventually("on the snoozed row", async () => (await selected()) === a);
      await page.keyboard.press("Enter");
      await eventually("enter: its page", async () => ticketAt(await page.evaluate(() => location.hash), a));
    } finally {
      await context.close();
    }
  });

  await test("the_queues_head_moves_and_a_deleted_ticket_goes", async () => {
    const [a, gone] = [make("the count's"), make("deleted while queued")];
    for (const id of [a, gone]) sy("tickets", "queue", "--id", id, "--urgency", "today", "--reason", "count");
    const { context, page } = await openPage("#/queue", { signedIn: true });
    try {
      const s = page.locator("[data-surface=queue-page]");
      await s.locator(`[data-id="${gone}"]`).waitFor();
      const count = async () => Number(await s.locator("[data-field=count]").innerText());
      const first = await count();
      assert(first === (await s.locator("[data-id]").count()), `the head is the rows: ${first}`);
      const fresh = make("arrives while it is open");
      sy("tickets", "queue", "--id", fresh, "--urgency", "today", "--reason", "new");
      await eventually("the count rises", async () => (await count()) === first + 1);
      const d = sy("tickets", "delete", "--ids", gone, "--message", "gone");
      assert(d.some((e) => e.type === "deleted"), `deleted: ${JSON.stringify(d)}`);
      await eventually("the row goes", async () => (await s.locator(`[data-id="${gone}"]`).count()) === 0);
      await eventually("the count falls", async () => (await count()) === first);
      // The ages read the server's clock as it moves: a write heard moves it.
      const at = (await s.locator(`[data-id="${a}"]`).getAttribute("data-now")) ?? "";
      await new Promise((r) => setTimeout(r, 1100));
      sy("tickets", "queue", "--id", a, "--urgency", "today", "--reason", "again");
      await eventually("the clock moved", async () => (await s.locator(`[data-id="${a}"]`).getAttribute("data-now")) > at);
    } finally {
      await context.close();
    }
  });

  await test("changes_are_requested_from_the_queue", async () => {
    const id = make("handed back");
    sy("tickets", "queue", "--id", id, "--urgency", "whenever", "--reason", "look at it");
    const { context, page } = await openPage("#/queue", { signedIn: true });
    try {
      const s = page.locator("[data-surface=queue-page]");
      await s.locator(`[data-id="${id}"]`).waitFor();
      await s.locator(`[data-id="${id}"] [data-action='tickets.request_changes']`).click();
      const form = s.locator("[data-form='tickets.request_changes']");
      await form.locator("textarea").fill("thinner, please");
      await form.locator("button").click();
      await eventually("handed back: out of the queue", async () => (await s.locator(`[data-id="${id}"]`).count()) === 0);
      const lines = sy("tickets", "history", "--id", id).filter((e) => e.type === "queue_record");
      const last = lines.at(-1)?.record;
      assert(last?.action === "changes_requested" && last?.reason === "thinner, please", `kept on the log: ${JSON.stringify(last)}`);
      // By its key, and a blank one is the service's refusal, in place.
      const again = make("handed back by key");
      sy("tickets", "queue", "--id", again, "--urgency", "now", "--reason", "by key");
      await s.locator(`[data-id="${again}"]`).waitFor();
      const n = await s.locator("[data-id]").count();
      for (let i = 0; i < n; i++) await page.keyboard.press("k");
      await page.keyboard.press("c");
      await form.locator("textarea").waitFor();
      await page.keyboard.press("Control+Enter");
      await eventually("a blank body refused", async () => (await s.locator("[data-refused]").count()) === 1);
    } finally {
      await context.close();
    }
  });

  await test("g_q_from_the_board_opens_the_queue", async () => {
    const { context, page } = await openPage("#/board", { signedIn: true });
    try {
      await page.locator("[data-chord='g q']").waitFor();
      await page.keyboard.press("g");
      await page.keyboard.press("q");
      await page.locator("[data-surface=queue-page]").waitFor();
    } finally {
      await context.close();
    }
  });
};
