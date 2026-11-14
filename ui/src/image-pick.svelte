<!-- EDITOR-30: a choice between images, picked like a radio (svelte/1, menger's bundle). The owner, 2026-11-14,
     of KAYOKEN-14's six canvas studies: "I want to be able to click on any of those and select them as if they
     were radio buttons. put them in a component." Written in a body in block form (its open and close lines each
     alone, tag-source), around a decision whose options carry their images:
       <image-pick>
       <m-decision id="canvas-study">
       <m-question>Which study goes on to film?</m-question>
       <m-option name="A" image="media:<64 hex>">**Sunk: a hairline.** The lightest reading.</m-option>
       <m-option name="B" image="media:<64 hex>">…</m-option>
       </m-decision>
       </image-pick>
     The server reads that decision as any `<m-decision id>` in a body (INPUT-3): one input of the ticket, its
     options the `m-option`s (value: `value`, else `name`). tag-components hands this native tag its `source` and
     its `ticket`; it draws each option as its image, its name and its words (inline markdown, the host's
     sanitizer), as one radio group — `role=radiogroup`, each image `role=radio` with `aria-checked`, one tab
     stop (the picked one, else the first). A click, Space or Enter picks; the arrows, Home and End move the pick
     and the focus with it. **The pick is the decision's answer**: `tickets.inputs.answer { ticket, input, value:
     { type: choice, option } }` — a click at once, keys once they rest (REST ms), one answer for a run of them.
     What is drawn picked is the input's answer as it stands (`tickets.inputs.list`, live on `tickets.watch`'s
     `input_changed`), or the pick being sent until the answer says it; a refusal says the service's words and
     the answer is drawn again. So an agent reads the pick from `tickets inputs list --ticket`, or waits for it
     (`await_reply`), and the ticket's questions show the same input. Nothing of one set of images is here: any
     options and images are the same tag. An image is `media:<hash>` (this origin's /media, behind the
     session) or a `data:image/…` URI; the page's policy loads nothing from elsewhere.
     Drawn only, saving nothing, with no ticket (a preview, a document) or no decision id: it says so.
     Marks: `[data-surface=image-pick]` (`data-ticket`, `data-input`, `data-picked`, `data-state=saved|saving|
     refused|unsaved|loading|empty`), `[role=radio][data-option=<value>]` (`aria-checked`), `[data-field=question|
     state|caption|full]`. -->
<script module>
  import { attrsOf } from "./tag-source.svelte";
  export const uses = ["tickets.inputs.list", "tickets.inputs.answer", "tickets.watch"];

  export const TAG = "image-pick";
  /** Keys that move the pick send one answer once they rest this long (ms). */
  export const REST = 500;
  const OPEN_ATTRS = `(?:\\s(?:[^>"']|"[^"]*"|'[^']*')*)?`;
  const MEDIA = /^media:([0-9a-fA-F]{64})$/;
  const DATA = /^data:image\/(?:png|jpeg|gif|webp|avif);base64,[A-Za-z0-9+/=\s]+$/;

  /** An option's `image` as a URL this page may load: `/media/<hash>` for `media:<hash>`, a `data:image/…` URI
   *  as it is; anything else (another origin, which the page's policy refuses) none. */
  export function imageUrl(v) {
    const s = String(v ?? "").trim();
    const m = MEDIA.exec(s);
    if (m) return `/media/${m[1].toLowerCase()}`;
    return DATA.test(s) ? s.replace(/\s+/g, "") : null;
  }

  /** The tag's source read as the server reads its decision: `{ input, question, options: [{ value, label,
   *  image, text }] }` — `input` the first `m-decision`'s id (null: none, nothing is saved), its options the
   *  `m-option`s in order (value: `value`, else `name`; label: `name`, else the value; `image` the attribute
   *  as written; `text` the option's words, markdown). An option with neither value nor name is not one. */
  export function readPick(source) {
    const lines = String(source ?? "").split("\n");
    const inner = lines.length > 2 ? lines.slice(1, -1).join("\n") : String(source ?? "");
    const dec = new RegExp(`<m-decision(${OPEN_ATTRS})>`).exec(inner);
    const id = dec ? (attrsOf(dec[1]).find(([k]) => k === "id")?.[1] ?? "").trim() : "";
    const q = /<m-question\b[^>]*>([\s\S]*?)<\/m-question>/.exec(inner);
    const options = [];
    const seen = new Set();
    for (const m of inner.matchAll(new RegExp(`<m-option(${OPEN_ATTRS})>([\\s\\S]*?)</m-option>`, "g"))) {
      const a = new Map(attrsOf(m[1]));
      const name = (a.get("name") ?? "").trim(), value = (a.get("value") ?? "").trim() || name;
      if (!value || seen.has(value)) continue;
      seen.add(value);
      options.push({ value, label: name || value, image: a.get("image") ?? "", text: m[2].trim() });
    }
    return { input: id || null, question: q ? q[1].replace(/<[^>]*>/g, "").trim() : "", options };
  }

  /** The index `key` moves `at` to among `n` (wrapping), or null when it moves nothing. */
  export function moved(key, at, n) {
    if (!n) return null;
    if (key === "ArrowRight" || key === "ArrowDown") return (at + 1) % n;
    if (key === "ArrowLeft" || key === "ArrowUp") return (at - 1 + n) % n;
    if (key === "Home") return 0;
    if (key === "End") return n - 1;
    return null;
  }
</script>

<script>
  import { tick } from "svelte";
  import { bus, live, markdown } from "plexus-ui/host";

  let { source = "", ticket = "", doc = "", n = "", editor = "" } = $props();
  const b = bus();

  const pick = $derived(readPick(source));
  const saves = $derived(!!ticket && !doc && !!pick.input);
  const inputs = live({ snapshot: "tickets.inputs.list", params: () => ({ ticket }), changes: "tickets.watch", key: "id",
                        on: { input_changed: "upsert:input" }, when: () => saves });
  const row = $derived(saves ? inputs.rows.find((i) => i.id === pick.input) ?? null : null);
  const answered = $derived(row?.answer?.value?.type === "choice" ? row.answer.value.option : null);

  let pending = $state(null); // the pick drawn while it is sent (or, saving nothing, the pick itself)
  let sending = $state(false);
  let refused = $state("");
  let root = $state(null);
  let timer = null;
  let resting = $state(false); // keys moved the pick; its answer waits for them to rest
  const picked = $derived(pending ?? answered);
  const at = $derived(Math.max(0, pick.options.findIndex((o) => o.value === picked)));

  // The answer caught up with what was sent: drawn from the answer again, so another's pick later shows too.
  $effect(() => { if (saves && !sending && pending != null && answered === pending) pending = null; });

  async function send(value) {
    clearTimeout(timer);
    timer = null;
    resting = false;
    if (!saves || value === answered) { if (saves) pending = null; return; }
    sending = true;
    refused = "";
    try {
      const a = await b.call("tickets.inputs.answer", { ticket, input: pick.input, value: { type: "choice", option: value } });
      const err = a?.ok ? a.events?.find((e) => e?.type === "error" || e?.type === "refused") : null;
      if (!a?.ok || err) { refused = String(a?.message ?? err?.message ?? "refused").replace(/^Invalid params: /, ""); pending = null; }
    } catch (e) {
      refused = String(e?.message ?? e);
      pending = null;
    } finally {
      sending = false;
    }
  }

  function choose(value, later = false) {
    pending = value;
    refused = "";
    if (!saves) return;
    if (later) { clearTimeout(timer); resting = true; timer = setTimeout(() => send(value), REST); }
    else send(value);
  }

  async function keyed(ev) {
    const k = pick.options.length;
    if (ev.key === " " || ev.key === "Enter") {
      const v = ev.target?.closest?.("[role=radio]")?.dataset?.option;
      if (v != null) { ev.preventDefault(); choose(v); }
      return;
    }
    const to = moved(ev.key, at, k);
    if (to == null) return;
    ev.preventDefault();
    choose(pick.options[to].value, true);
    await tick();
    root?.querySelector(`[role=radio][data-option="${CSS.escape(pick.options[to].value)}"]`)?.focus();
  }
  // Leaving before the keys rest sends the pick at once.
  const left = (ev) => { if (timer && !root?.contains(ev.relatedTarget)) send(pending); };
  $effect(() => () => clearTimeout(timer));

  const phase = $derived(!pick.options.length ? "empty" : !saves ? "unsaved" : sending || resting || (pending != null && pending !== answered) ? "saving"
    : refused ? "refused" : inputs.status === "pending" && !row ? "loading" : "saved");
  const said = $derived.by(() => {
    if (phase === "empty") return "no options: give the decision m-option lines, each with a name and an image";
    if (phase === "unsaved") return !pick.input ? "not saved: give its m-decision an id" : "not saved here: the pick is a ticket's answer";
    if (phase === "refused") return refused;
    if (phase === "saving") return `picking ${pick.options.find((o) => o.value === picked)?.label ?? picked}…`;
    if (phase === "loading") return "reading the answer…";
    if (row && row.state !== "open" && row.state !== "answered") return `${row.state}: the question takes no answer`;
    if (answered == null) return "nothing picked yet";
    const o = pick.options.find((x) => x.value === answered);
    return `picked ${o?.label ?? answered}${row?.answer?.name ? ` · ${row.answer.name}` : ""}`;
  });
</script>

<section class="image-pick" data-surface="image-pick" data-ticket={ticket || undefined} data-input={pick.input ?? undefined}
         data-picked={picked ?? undefined} data-state={phase} data-n={n === "" ? undefined : n} data-editor={editor ? "" : undefined}>
  {#if pick.question}<p class="ip-q" data-field="question" id={`ip-q-${pick.input ?? n}`}>{pick.question}</p>{/if}
  <div class="ip-grid" role="radiogroup" aria-label={pick.question || undefined} aria-labelledby={pick.question ? `ip-q-${pick.input ?? n}` : undefined}
       bind:this={root} onkeydown={keyed} onfocusout={left}>
    {#each pick.options as o, i (o.value)}
      {@const url = imageUrl(o.image)}
      <div class="ip-card" data-checked={o.value === picked ? "" : undefined}>
        <!-- A button: in the editor a press on a control is its own, never the caret's (editor-body's Drawn). -->
        <button type="button" class="ip-opt" role="radio" data-option={o.value} aria-checked={o.value === picked ? "true" : "false"}
                aria-label={o.label} tabindex={i === at ? 0 : -1} onclick={() => choose(o.value)}>
          <span class="ip-frame">
            {#if url}<img src={url} alt={o.label} loading="lazy" draggable="false"/>{:else}<span class="ip-none">no image</span>{/if}
            <span class="ip-mark" aria-hidden="true"></span>
          </span>
          <span class="ip-cap" data-field="caption"><b class="ip-name">{o.label}</b>{#if o.text}<span class="ip-text">{@html markdown(o.text)}</span>{/if}</span>
        </button>
        {#if url && !url.startsWith("data:")}<button type="button" class="ip-full" data-field="full" tabindex="-1" onclick={() => window.open(url, "_blank", "noopener")}>full size</button>{/if}
      </div>
    {/each}
  </div>
  <p class="ip-state" data-field="state" role={phase === "refused" ? "alert" : "status"}>{said}</p>
</section>

<style>
  .image-pick { margin: 6px 0; color: var(--ink); font-family: var(--sans); }
  .ip-q { margin: 0 0 8px; font-weight: var(--weight-strong); }
  .ip-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(min(100%, 300px), 1fr)); gap: 14px; }
  .ip-card { position: relative; min-width: 0; }
  .ip-opt { display: flex; flex-direction: column; gap: 6px; width: 100%; padding: 6px; font: inherit; color: inherit; text-align: left; border: var(--border) solid var(--line); border-radius: var(--radius-lg);
            background: var(--card); cursor: pointer; outline: none; transition: border-color var(--motion-fast) var(--ease), background-color var(--motion-fast) var(--ease); }
  .ip-opt:hover, .ip-opt:global([data-sheet~=hover]) { border-color: var(--muted); }
  .ip-opt[aria-checked=true], .ip-opt:global([data-sheet~=checked]) { border-color: var(--accent); background: var(--sel); box-shadow: 0 0 0 1px var(--accent); }
  .ip-opt:focus-visible, .ip-opt:global([data-sheet~=focus]) { box-shadow: 0 0 0 2px var(--bg), 0 0 0 4px var(--focus); }
  .ip-frame { position: relative; display: block; overflow: hidden; aspect-ratio: 16 / 10; border-radius: var(--radius); background: var(--panel); }
  .ip-frame img { display: block; width: 100%; height: 100%; object-fit: contain; }
  .ip-none { display: grid; place-items: center; height: 100%; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  /* The radio's dot, over the image's corner: a ring, filled with a check when picked. */
  .ip-mark { position: absolute; top: 8px; left: 8px; width: 20px; height: 20px; box-sizing: border-box; border-radius: 50%;
             border: 2px solid var(--muted); background: var(--card); }
  .ip-opt[aria-checked=true] .ip-mark, .ip-opt:global([data-sheet~=checked]) .ip-mark { border-color: var(--accent); background: var(--accent); }
  .ip-opt[aria-checked=true] .ip-mark::after, .ip-opt:global([data-sheet~=checked]) .ip-mark::after {
    content: ""; position: absolute; left: 5px; top: 1px; width: 5px; height: 10px; border: solid var(--card); border-width: 0 2px 2px 0; transform: rotate(45deg); }
  .ip-cap { display: block; padding: 0 2px 2px; font-size: 13px; line-height: 1.45; overflow-wrap: anywhere; }
  .ip-name { margin-right: 6px; font-family: var(--font-display); font-weight: var(--weight-strong); }
  .ip-opt[aria-checked=true] .ip-name { color: var(--accent); }
  .ip-text :global(p) { display: inline; margin: 0; }
  .ip-text :global(strong) { font-weight: var(--weight-medium); }
  .ip-full { position: absolute; top: 12px; right: 12px; padding: 0 6px; font-family: var(--font-text); font-size: 11px; line-height: 18px;
             color: var(--muted); background: var(--card); cursor: pointer; border: var(--border) solid var(--line); border-radius: var(--radius);
             opacity: 0; transition: opacity var(--motion-fast) var(--ease); }
  .ip-card:hover .ip-full, .ip-full:focus-visible { opacity: 1; }
  .ip-full:hover { color: var(--ink); }
  .ip-state { margin: 8px 0 0; font-family: var(--font-text); font-size: 11px; color: var(--muted); }
  .image-pick[data-state=refused] .ip-state { color: var(--warn); }
  .image-pick[data-state=saved][data-picked] .ip-state { color: var(--ink); }
  @media (hover: none) { .ip-full { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .ip-opt, .ip-full { transition: none; } }
</style>
