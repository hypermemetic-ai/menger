<!-- COMPS-16: the tweet builder as a component whose own content is the thread (svelte/1, menger's bundle).
     The owner, 2026-11-10: "it's a component whose body you literally fill in … it should literally then allow
     me to use it and save its state somewhere, I think directly in the ticket". Written in a body as
       <tweet-writer>
       <tweet-post video="media:<64 hex>" name="clip.mp4">words

       a second paragraph</tweet-post>
       <tweet-post>more</tweet-post>
       </tweet-writer>
     — its open line and its close line each alone (block form, tag-source), each `tweet-post` child a post:
     its words the child's text, its video the child's `video` (`media:<hash>`) and `name`. It is drawn by its
     tag (tag-components hands a native tag its `source`, its `children` as structure, the `ticket` or `doc`
     it sits in, `editor` when it is in the editor, and `n`, its place among the body's `tweet-writer`s) as
     the composer: each post's words in a box that grows, its count against 280 by code points (amber from
     260, red past 280: over is allowed and saved, the limit is the site's), its video playable from this
     origin's /media, up, down, remove, and a + under it for the next. It reads no ticket to draw.
     **Its state is its own markup.** Save (the button, ctrl/⌘+s while the focus is inside it; never a
     keystroke) reads the body as it is now (`tickets.get`), finds the `n`th `tweet-writer` there, and —
     when that is still the source it was drawn from — replaces that tag alone and writes the body back
     (`tickets.amend`), every other byte kept; when it is gone or someone changed it, it refuses and says
     so, overwriting nothing. Words are written escaped (`&` and `<`), so a word never closes the tag.
     Drawn only: in the editor (the block under the caret is the tag's source, and the editor saves the
     body), in a document (the docs service's: no save here), or with no ticket known.
     Marks: `[data-surface=tweet-writer]` (`data-ticket`, `data-n`, `data-where=ticket|editor|doc|none|loose`),
     `[data-post=<n>]` (1-based; `[data-over]` past 280), `[data-field=text|count|video|over|state|note]`,
     `[data-action=add|remove|up|down|save]`, `[data-state=saved|dirty|saving|refused|changed|readonly]`. -->
<script module>
  import { tagSpans, childrenOf, escText, escAttr, openName } from "./tag-source.svelte";
  export const uses = ["tickets.get", "tickets.amend"];

  export const LIMIT = 280;
  export const NEAR = 260;
  export const TAG = "tweet-writer";
  export const POST = "tweet-post";
  const MEDIA = /^media:([0-9a-fA-F]{64})$/;

  /** Characters as the site counts them: code points. */
  export const countOf = (text) => [...String(text ?? "")].length;

  /** A post's video, from its attributes: `{ hash, name }` or null. */
  export function videoOf(attrs) {
    const get = (k) => attrs.find(([a]) => a === k)?.[1];
    const m = MEDIA.exec(String(get("video") ?? "").trim());
    return m ? { hash: m[1].toLowerCase(), name: get("name") ?? "" } : null;
  }

  /** The writer's children as posts: `[{ text, attrs }]` (only `tweet-post`s; `attrs` as written). */
  export function postsOf(children) {
    return (children ?? []).filter((c) => c?.tag === POST).map((c) => ({ text: String(c.text ?? ""), attrs: Array.isArray(c.attrs) ? c.attrs : [] }));
  }

  /** The tag back as its source: the open line as it was (`<tweet-writer>` when none), a line per post. */
  export function serializeWriter(posts, open = `<${TAG}>`) {
    const line = (p) => `<${POST}${p.attrs.map(([k, v]) => ` ${k}="${escAttr(v)}"`).join("")}>${escText(p.text)}</${POST}>`;
    return [openName(open) === TAG ? open : `<${TAG}>`, ...posts.map(line), `</${TAG}>`].join("\n");
  }

  /** `body` with its `n`th writer replaced by `next` — only while that writer is still `was`; else
   *  `{ refused }` saying why. Every other byte of `body` is kept. */
  export function replaceWriter(body, n, was, next) {
    const s = tagSpans(body).filter((t) => t.name === TAG)[n];
    if (!s) return { refused: "gone", message: `the body no longer holds this writer (the ${n + 1}${["st", "nd", "rd"][n] ?? "th"} <${TAG}>): nothing saved — reload to see it as it is` };
    if (s.text !== was) return { refused: "changed", message: "this writer was changed since it was drawn (someone else's edit): nothing saved — reload to see theirs" };
    return { body: body.slice(0, s.from) + next + body.slice(s.to) };
  }
</script>

<script>
  import { tick, untrack } from "svelte";
  import { bus } from "plexus-ui/host";

  let { source = "", children = null, ticket = "", doc = "", n = "", editor = "" } = $props();
  const b = bus();

  const where = $derived(doc ? "doc" : editor ? "editor" : !ticket ? "none" : n !== "" && n != null && source ? "ticket" : "loose");
  const notes = {
    editor: "drawn only in the editor: the tag's source is where the caret is, and the editor saves the body — save the thread from the ticket's page",
    doc: "drawn only: a writer in a document cannot save here",
    none: "drawn only: no ticket known here, so it cannot save",
    loose: "drawn only: its place in the body is not known — write its open and close tags each on a line of their own to save it",
  };

  let posts = $state([]);
  let open = $state(`<${TAG}>`);
  let drawnFrom = $state(""); // the tag's source as drawn, or as last saved: what a save expects to find
  let baseline = $state(""); // the posts as loaded, serialized
  let saving = $state(false);
  let refused = $state(null);
  let root = $state(null);
  let seq = 0;
  const post = (text = "", attrs = []) => ({ key: ++seq, text, attrs, video: videoOf(attrs) });

  function load(src, kids) {
    let list = null;
    try { list = typeof kids === "string" && kids ? JSON.parse(kids) : Array.isArray(kids) ? kids : null; } catch { list = null; }
    const ps = postsOf(list ?? childrenOf(src));
    open = String(src ?? "").split("\n")[0] || `<${TAG}>`;
    posts = ps.length ? ps.map((p) => post(p.text, p.attrs)) : [post()];
    drawnFrom = String(src ?? "");
    baseline = serializeWriter(ps, open);
    refused = null;
  }
  // Drawn again from each source the tag is handed (the effect reads the props alone; what load writes is untracked).
  $effect(() => {
    const src = source, kids = children;
    untrack(() => load(src, kids));
  });

  const text = $derived(serializeWriter(posts, open));
  const phase = $derived(where !== "ticket" ? "readonly" : saving ? "saving" : refused ? refused.kind : text !== baseline ? "dirty" : "saved");
  const said = { saved: "saved", dirty: "unsaved changes", saving: "saving…" };
  const locked = $derived(where !== "ticket"); // drawn only: nothing typed here could be kept

  async function save() {
    if (where !== "ticket" || saving) return;
    const next = text;
    if (next === baseline) return;
    saving = true;
    refused = null;
    try {
      const r = await b.call("tickets.get", { id: ticket });
      const t = r?.ok ? r.events?.find((e) => e?.type === "ticket")?.ticket : null;
      if (!t) { refused = { kind: "refused", message: r?.message ?? `no ticket ${ticket}` }; return; }
      const out = replaceWriter(t.body ?? "", Number(n), drawnFrom, next);
      if (out.refused) { refused = { kind: "changed", message: out.message }; return; }
      const w = await b.call("tickets.amend", { id: t.id, body: out.body });
      if (w?.ok && !w.events?.some((e) => e?.type === "error")) { drawnFrom = next; baseline = next; }
      else refused = { kind: "refused", message: w?.message ?? w?.events?.find((e) => e?.message)?.message ?? "refused" };
    } catch (e) {
      refused = { kind: "refused", message: e?.message ?? String(e) };
    } finally {
      saving = false;
    }
  }

  // ctrl/⌘+s is the engine's chord (app-top holds it page-wide); this writer's save when the focus is inside it.
  const saveKey = () => { if (root && root.contains(document.activeElement)) save(); };
  b.keys({ "ctrl+s": { to: saveKey, label: "save the thread" }, "meta+s": { to: saveKey, label: "save the thread" } });

  async function focusPost(k) {
    await tick();
    root?.querySelector(`[data-post="${k + 1}"] textarea`)?.focus();
  }
  function add(at) {
    posts.splice(at + 1, 0, post());
    focusPost(at + 1);
  }
  function remove(at) {
    if (posts.length > 1) posts.splice(at, 1);
  }
  function move(at, by) {
    const to = at + by;
    if (to < 0 || to >= posts.length) return;
    const [p] = posts.splice(at, 1);
    posts.splice(to, 0, p);
    focusPost(to);
  }

  /** A box that grows with its words: its height follows what it holds. */
  function grow(node, _text) {
    const fit = () => {
      node.style.height = "auto";
      node.style.height = node.scrollHeight + "px";
    };
    fit();
    // Drawn into a tag's mark before it is laid out, and moved by up and down: fitted again once it has its
    // width, and whenever the width changes (the height it sets changes no width, so this never loops).
    let width = 0;
    const seen = new ResizeObserver(() => { if (node.clientWidth !== width) { width = node.clientWidth; fit(); } });
    seen.observe(node);
    return { update: fit, destroy: () => seen.disconnect() };
  }

  // The ring: its circumference, and how much of it a count fills.
  const R = 9;
  const C = 2 * Math.PI * R;
  const level = (k) => (k > LIMIT ? "over" : k >= NEAR ? "near" : "ok");
</script>

<div class="tweet-writer" data-surface="tweet-writer" data-ticket={ticket || undefined} data-n={n} data-where={where} role="group" aria-label="the thread" bind:this={root}>
  <header class="tw-head">
    <span class="tw-label">thread · {posts.length} {posts.length === 1 ? "post" : "posts"}</span>
    <span class="tw-state" data-field="state" data-state={phase}>{phase === "readonly" ? "" : refused ? refused.message : said[phase]}</span>
    {#if where === "ticket"}<button type="button" class="btn primary" data-action="save" disabled={saving || phase === "saved"} onclick={save}>save <kbd>ctrl</kbd><kbd>s</kbd></button>{/if}
  </header>
  {#if where !== "ticket"}<p class="tw-note" data-field="note">{notes[where]}</p>{/if}

  <ol class="tw-posts">
    {#each posts as p, i (p.key)}
      {@const k = countOf(p.text)}
      <li class="tw-post" data-post={i + 1} data-over={k > LIMIT ? "" : undefined}>
        <div class="tw-rail" aria-hidden="true">
          <span class="tw-avatar"></span>
          <span class="tw-line"></span>
        </div>
        <div class="tw-main">
          <div class="tw-row">
            <textarea class="tw-text" data-field="text" rows="2" readonly={locked} placeholder={i === 0 ? "What is happening?" : "Add another post"} aria-label={`post ${i + 1}`} bind:value={p.text} use:grow={p.text}></textarea>
            <div class="tw-side">
              <span class="tw-count" data-field="count" data-level={level(k)} data-count={k} title={`${k} / ${LIMIT}`}>
                <svg class="ring" viewBox="0 0 24 24" aria-hidden="true">
                  <circle class="ring-bg" cx="12" cy="12" r={R}/>
                  <circle class="ring-fg" cx="12" cy="12" r={R} stroke-dasharray={C} stroke-dashoffset={C * (1 - Math.min(k, LIMIT) / LIMIT)}/>
                </svg>
                <span class="tw-n">{k} / {LIMIT}</span>
              </span>
              {#if k > LIMIT}<span class="tw-over" data-field="over">over by {k - LIMIT}</span>{/if}
            </div>
          </div>
          {#if p.video}
            <figure class="tw-video">
              <!-- svelte-ignore a11y_media_has_caption -->
              <video data-field="video" data-hash={p.video.hash} controls preload="metadata" src={`${location.origin}/media/${p.video.hash}`}></video>
              <figcaption>{p.video.name}</figcaption>
            </figure>
          {/if}
          {#if !locked}
            <div class="tw-tools">
              <button type="button" class="tool" data-action="up" aria-label={`move post ${i + 1} up`} disabled={i === 0} onclick={() => move(i, -1)}>↑</button>
              <button type="button" class="tool" data-action="down" aria-label={`move post ${i + 1} down`} disabled={i === posts.length - 1} onclick={() => move(i, 1)}>↓</button>
              <button type="button" class="tool" data-action="remove" aria-label={`remove post ${i + 1}`} disabled={posts.length === 1} onclick={() => remove(i)}>remove</button>
              <span class="tw-num">{i + 1} / {posts.length}</span>
            </div>
            <button type="button" class="add" data-action="add" aria-label={`add a post after post ${i + 1}`} onclick={() => add(i)}>+</button>
          {/if}
        </div>
      </li>
    {/each}
  </ol>
</div>

<style>
  .tweet-writer { display: block; max-width: 640px; margin: 4px 0; padding: 4px 16px 12px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .tw-head { display: flex; gap: 12px; align-items: center; padding: 8px 0; border-bottom: var(--border) solid var(--line); }
  .tw-label { flex: 1; min-width: 0; font-size: 12px; color: var(--muted); white-space: nowrap; }
  .tw-state { font-size: 12px; color: var(--muted); }
  .tw-state[data-state="dirty"] { color: var(--sev-medium); }
  .tw-state[data-state="refused"], .tw-state[data-state="changed"] { color: var(--warn); }
  .tw-state[data-state="saved"] { color: var(--live); }
  .tw-note { margin: 8px 0 0; font-size: 12px; color: var(--muted); }
  .tw-posts { margin: 0; padding: 12px 0 0; list-style: none; }
  .tw-post { display: grid; grid-template-columns: 40px minmax(0, 1fr); gap: 12px; }
  .tw-rail { display: flex; flex-direction: column; align-items: center; }
  .tw-avatar { display: block; flex: none; width: 40px; height: 40px; background: var(--panel); border: var(--border) solid var(--line); border-radius: 50%; }
  .tw-line { display: block; flex: 1; width: 2px; margin: 4px 0; background: var(--tree-line); border-radius: var(--radius-xs); }
  .tw-post:last-child .tw-line { visibility: hidden; }
  .tw-main { min-width: 0; padding-bottom: 12px; }
  .tw-row { display: flex; gap: 10px; align-items: flex-start; }
  .tw-text { display: block; flex: 1; min-width: 0; min-height: 48px; padding: 8px 0; overflow: hidden; font-family: var(--font-text); font-size: 16px; line-height: 1.45; color: var(--ink); background: none; border: 0; border-bottom: var(--border) solid var(--line); resize: none; field-sizing: content; }
  .tw-text::placeholder { color: var(--muted); }
  .tw-text:focus, .tw-text:global([data-sheet~=focus]) { outline: none; border-bottom-color: var(--focus); }
  .tw-side { display: flex; flex: none; flex-direction: column; gap: 2px; align-items: flex-end; width: 92px; padding-top: 8px; }
  .tw-count { display: inline-flex; gap: 6px; align-items: center; font-size: 11px; color: var(--muted); white-space: nowrap; }
  .ring { width: 22px; height: 22px; transform: rotate(-90deg); }
  .ring circle { fill: none; stroke-width: 2.5; }
  .ring-bg { stroke: var(--line); }
  .ring-fg { stroke: var(--accent); stroke-linecap: round; transition: stroke-dashoffset var(--motion-fast) var(--ease); }
  .tw-count[data-level="near"] { color: var(--sev-medium); }
  .tw-count[data-level="near"] .ring-fg { stroke: var(--sev-medium); }
  .tw-count[data-level="over"] { color: var(--warn); }
  .tw-count[data-level="over"] .ring-fg { stroke: var(--warn); }
  .tw-over { font-size: 11px; color: var(--warn); }
  .tw-video { margin: 10px 0 0; }
  .tw-video video { display: block; width: 100%; max-height: 420px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .tw-video figcaption { margin-top: 4px; font-size: 11px; color: var(--muted); }
  .tw-tools { display: flex; gap: 4px; align-items: center; margin-top: 8px; }
  .tw-num { margin-left: auto; font-size: 11px; color: var(--muted); }
  .tool { padding: 0 8px; font-size: 12px; line-height: 20px; color: var(--muted); background: none; border: var(--border) solid transparent; border-radius: var(--radius); cursor: pointer; }
  .tool:hover, .tool:global([data-sheet~=hover]) { color: var(--ink); background: var(--hover); }
  .tool:focus-visible, .tool:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .tool:disabled, .tool:global([data-sheet~=disabled]) { color: var(--disabled); background: none; cursor: not-allowed; }
  .add { display: inline-flex; align-items: center; justify-content: center; width: 28px; height: 28px; margin-top: 8px; padding: 0; font-size: 18px; line-height: 1; color: var(--accent); background: var(--card); border: var(--border) solid var(--line); border-radius: 50%; cursor: pointer; }
  .add:hover, .add:global([data-sheet~=hover]) { background: var(--hover); border-color: var(--accent); }
  .add:focus-visible, .add:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn { display: inline-flex; gap: 5px; align-items: center; padding: 1px 10px; font-size: 12px; line-height: 20px; white-space: nowrap; color: var(--ink); background: var(--card); border: var(--border) solid var(--line); border-radius: var(--radius); cursor: pointer; }
  .btn.primary { color: var(--bg); background: var(--accent); border-color: var(--accent); }
  .btn.primary:hover, .btn.primary:global([data-sheet~=hover]) { opacity: .88; }
  .btn:focus-visible, .btn:global([data-sheet~=focus]) { outline: 2px solid var(--focus); outline-offset: 1px; }
  .btn:disabled, .btn.primary:disabled, .btn:global([data-sheet~=disabled]) { color: var(--disabled); background: var(--card); border-color: var(--line); cursor: not-allowed; }
  .btn kbd { padding: 0 3px; font-family: var(--font-text); font-size: 10px; line-height: 14px; color: inherit; border: var(--border) solid currentColor; border-radius: var(--radius-sm); opacity: .7; }
  @media (prefers-reduced-motion: reduce) {
    .ring-fg { transition: none; }
  }
</style>
