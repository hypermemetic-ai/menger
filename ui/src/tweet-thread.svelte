<!-- COMPS-15: a thread laid out like a post composer on the site it is
     posted to (svelte/1), at #/thread/:id (page-thread). It reads the
     ticket's body as MENGER-52's form — everything before the first
     `## Post N` heading kept as it is (the head); each `## Post N` a post,
     its lines its words, a line that is `[name](media:<64 hex>)` alone its
     video; any other heading ends the posts, and it and what follows are
     kept as they are (the tail) — and writes it back the same way, the posts
     numbered from 1, through `tickets.amend` when asked (the button, ctrl+s
     or cmd+s while the thread has the focus), never on a keystroke. Each post: its words in a box that
     grows, its count against 280 by code points (amber from 260, red past
     280: over is allowed and saved, the limit is the site's), its video
     playable, remove, up, down, and a + under it for the next.
     Marks: `[data-surface=tweet-thread]`, `[data-post=<n>]` (1-based),
     `[data-field=text|count|video|title|state]`, `[data-action=add|remove|up|down|save]`,
     `[data-state=loading|saved|dirty|saving|refused|missing]`, `[data-over]` on an over post.
     The owner: "isn't the ticket itself a component" — so it is a tag in any body too,
     `<tweet-thread id="KILN-2"></tweet-thread>`, drawn natively there (tag-components: a bundle
     component is a bound mark, its attributes its props); `id` is required, a tag without it says so.
     In its own ticket the tag sits in the head, which a save keeps as it is; the page then draws the
     body again and the thread comes back from `tickets.get`, saved. It reads once per `id` and writes
     only when asked: no effect writes what it reads. -->
<script module>
  export const uses = ["tickets.get", "tickets.amend"];

  const POST = /^##\s+Post\b/i;
  const HEADING = /^#{1,6}\s/;
  const VIDEO = /^\s*\[([^\]\n]*)\]\(media:([0-9a-f]{64})\)\s*$/;
  export const LIMIT = 280;
  export const NEAR = 260;

  /** A body as `{ head, posts: [{ text, video: { name, hash } | null }], tail }`. */
  export function parseThread(body) {
    const lines = String(body ?? "").split("\n");
    const first = lines.findIndex((l) => POST.test(l));
    // No post: the whole body is the head (`bare`), and the posts a save writes go after it, set apart.
    if (first < 0) return { head: String(body ?? ""), posts: [], tail: "", bare: true };
    const head = lines.slice(0, first).map((l) => l + "\n").join("");
    const posts = [];
    let cur = null;
    let i = first;
    for (; i < lines.length; i++) {
      const l = lines[i];
      if (POST.test(l)) {
        cur = { lines: [], video: null };
        posts.push(cur);
        continue;
      }
      if (HEADING.test(l)) break;
      const v = cur.video ? null : VIDEO.exec(l);
      if (v) cur.video = { name: v[1], hash: v[2] };
      else cur.lines.push(l);
    }
    const tail = lines.slice(i).join("\n");
    return { head, posts: posts.map((p) => ({ text: p.lines.join("\n").replace(/\n{3,}/g, "\n\n").trim(), video: p.video })), tail };
  }

  /** The thread back as a body: the head, each post numbered from 1, the tail. */
  export function serializeThread({ head, posts, tail, bare = false }) {
    let out = head ?? "";
    // A head is kept as it is; one that had no post after it (`bare`) is set apart from the first.
    if (posts.length && out && !out.endsWith(bare ? "\n\n" : "\n")) out += out.endsWith("\n") ? "\n" : bare ? "\n\n" : "\n";
    posts.forEach((p, n) => {
      out += `## Post ${n + 1}\n\n`;
      const text = String(p.text ?? "").replace(/\n{3,}/g, "\n\n").trim();
      if (text) out += text + "\n\n";
      if (p.video) out += `[${p.video.name}](media:${p.video.hash})\n\n`;
    });
    return out + (tail ?? "");
  }

  /** Characters as the site counts them: code points. */
  export const countOf = (text) => [...String(text ?? "")].length;
</script>

<script>
  import { tick, untrack } from "svelte";
  import { bus } from "plexus-ui/host";

  let { id } = $props();
  const b = bus();

  let ticket = $state(null);
  let missing = $state(null);
  let head = $state("");
  let tail = $state("");
  let bare = $state(false);
  let posts = $state([]);
  let saved = $state(""); // the body as last read or written, in the thread's form
  let saving = $state(false);
  let refused = $state(null);
  let root = $state(null);
  let seq = 0;
  const post = (text = "", video = null) => ({ key: ++seq, text, video });

  const body = $derived(serializeThread({ head, posts, tail, bare }));
  const phase = $derived(!ticket ? (missing ? "missing" : "loading") : saving ? "saving" : refused ? "refused" : body !== saved ? "dirty" : "saved");
  const said = { loading: "loading…", missing: "", saved: "saved", dirty: "unsaved changes", saving: "saving…" };

  async function load(at) {
    ticket = null;
    if (!at) {
      missing = "no ticket named: give the tag its ticket, <tweet-thread id=\"KILN-2\"></tweet-thread>";
      return;
    }
    missing = null;
    refused = null;
    const r = await b.call("tickets.get", { id: at });
    if (at !== id) return;
    const t = r?.ok ? r.events?.find((e) => e?.type === "ticket")?.ticket : null;
    if (!t) {
      missing = r?.message ?? `no ticket ${at}`;
      return;
    }
    const th = parseThread(t.body ?? "");
    head = th.head;
    tail = th.tail;
    bare = !!th.bare;
    posts = th.posts.length ? th.posts.map((p) => post(p.text, p.video)) : [post()];
    // What is drawn is what is saved until it is edited (a body with no post shows an empty one, unsaved by nothing).
    saved = serializeThread({ head, posts, tail, bare });
    ticket = t;
  }
  // Read once per id: the effect reads `id` alone, and what `load` writes is untracked.
  $effect(() => {
    const at = id;
    untrack(() => load(at));
  });

  async function save() {
    if (!ticket || saving) return;
    const asked = body;
    saving = true;
    refused = null;
    try {
      const r = await b.call("tickets.amend", { id: ticket.id, body: asked });
      if (r?.ok) saved = asked;
      else refused = r?.message ?? "refused";
    } catch (e) {
      refused = e?.message ?? String(e);
    } finally {
      saving = false;
    }
  }

  async function focusPost(n) {
    await tick();
    root?.querySelector(`[data-post="${n + 1}"] textarea`)?.focus();
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

  function onkey(e) {
    if ((e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      e.stopPropagation(); // the thread's save, not the page's (app-top's ctrl+s)
      save();
    }
  }

  /** A box that grows with its words: its height follows what it holds. */
  function grow(node, _text) {
    const fit = () => {
      node.style.height = "auto";
      node.style.height = node.scrollHeight + "px";
    };
    fit();
    return { update: fit };
  }

  // The ring: its circumference, and how much of it a count fills.
  const R = 9;
  const C = 2 * Math.PI * R;
  const level = (n) => (n > LIMIT ? "over" : n >= NEAR ? "near" : "ok");
</script>

<div class="tweet-thread" data-surface="tweet-thread" data-ticket={id} role="group" aria-label="the thread" bind:this={root} onkeydown={onkey}>
  <header class="tt-head">
    <div class="tt-id">
      <span class="tt-tid">{ticket?.id ?? id}</span>
      {#if ticket}<span class="tt-title" data-field="title">{ticket.title}</span>{/if}
    </div>
    <span class="tt-state" data-field="state" data-state={phase}>{phase === "refused" ? refused : phase === "missing" ? missing : said[phase]}</span>
    <button type="button" class="btn primary" data-action="save" disabled={!ticket || saving} onclick={save}>save <kbd>ctrl</kbd><kbd>s</kbd></button>
  </header>

  {#if ticket}
    <ol class="tt-posts">
      {#each posts as p, i (p.key)}
        {@const n = countOf(p.text)}
        <li class="tt-post" data-post={i + 1} data-over={n > LIMIT ? "" : undefined}>
          <div class="tt-rail" aria-hidden="true">
            <span class="tt-avatar"></span>
            <span class="tt-line"></span>
          </div>
          <div class="tt-main">
            <div class="tt-row">
              <textarea class="tt-text" data-field="text" rows="2" placeholder={i === 0 ? "What is happening?" : "Add another post"} aria-label={`post ${i + 1}`} bind:value={p.text} use:grow={p.text}></textarea>
              <div class="tt-side">
                <span class="tt-count" data-field="count" data-level={level(n)} data-count={n} title={`${n} / ${LIMIT}`}>
                  <svg class="ring" viewBox="0 0 24 24" aria-hidden="true">
                    <circle class="ring-bg" cx="12" cy="12" r={R}/>
                    <circle class="ring-fg" cx="12" cy="12" r={R} stroke-dasharray={C} stroke-dashoffset={C * (1 - Math.min(n, LIMIT) / LIMIT)}/>
                  </svg>
                  <span class="tt-n">{n} / {LIMIT}</span>
                </span>
                {#if n > LIMIT}<span class="tt-over" data-field="over">over by {n - LIMIT}</span>{/if}
              </div>
            </div>
            {#if p.video}
              <figure class="tt-video">
                <!-- svelte-ignore a11y_media_has_caption -->
                <video data-field="video" data-hash={p.video.hash} controls preload="metadata" src={`${location.origin}/media/${p.video.hash}`}></video>
                <figcaption>{p.video.name}</figcaption>
              </figure>
            {/if}
            <div class="tt-tools">
              <button type="button" class="tool" data-action="up" aria-label={`move post ${i + 1} up`} disabled={i === 0} onclick={() => move(i, -1)}>↑</button>
              <button type="button" class="tool" data-action="down" aria-label={`move post ${i + 1} down`} disabled={i === posts.length - 1} onclick={() => move(i, 1)}>↓</button>
              <button type="button" class="tool" data-action="remove" aria-label={`remove post ${i + 1}`} disabled={posts.length === 1} onclick={() => remove(i)}>remove</button>
              <span class="tt-num">{i + 1} / {posts.length}</span>
            </div>
            <button type="button" class="add" data-action="add" aria-label={`add a post after post ${i + 1}`} onclick={() => add(i)}>+</button>
          </div>
        </li>
      {/each}
    </ol>
  {/if}
</div>

<style>
  .tweet-thread { display: block; max-width: 640px; margin: 0 auto; padding: 12px 16px 32px; color: var(--ink); font-family: var(--font-text); font-size: var(--text-size); }
  .tt-head { position: sticky; top: 0; z-index: 1; display: flex; gap: 12px; align-items: center; padding: 10px 0; background: var(--bg); border-bottom: var(--border) solid var(--line); }
  .tt-id { display: flex; flex: 1; gap: 8px; align-items: baseline; min-width: 0; }
  .tt-tid { color: var(--accent); font-weight: var(--weight-strong); white-space: nowrap; }
  .tt-title { overflow: hidden; font-family: var(--font-display); font-weight: var(--weight-strong); white-space: nowrap; text-overflow: ellipsis; }
  .tt-state { font-size: 12px; color: var(--muted); white-space: nowrap; }
  .tt-state[data-state="dirty"] { color: var(--sev-medium); }
  .tt-state[data-state="refused"], .tt-state[data-state="missing"] { color: var(--warn); white-space: normal; }
  .tt-state[data-state="saved"] { color: var(--live); }
  .tt-posts { margin: 0; padding: 16px 0 0; list-style: none; }
  .tt-post { display: grid; grid-template-columns: 40px minmax(0, 1fr); gap: 12px; }
  .tt-rail { display: flex; flex-direction: column; align-items: center; }
  .tt-avatar { display: block; flex: none; width: 40px; height: 40px; background: var(--panel); border: var(--border) solid var(--line); border-radius: 50%; }
  .tt-line { display: block; flex: 1; width: 2px; margin: 4px 0; background: var(--tree-line); border-radius: var(--radius-xs); }
  .tt-post:last-child .tt-line { visibility: hidden; }
  .tt-main { min-width: 0; padding-bottom: 12px; }
  .tt-row { display: flex; gap: 10px; align-items: flex-start; }
  .tt-text { display: block; flex: 1; min-width: 0; min-height: 48px; padding: 8px 0; overflow: hidden; font-family: var(--font-text); font-size: 16px; line-height: 1.45; color: var(--ink); background: none; border: 0; border-bottom: var(--border) solid var(--line); resize: none; field-sizing: content; }
  .tt-text::placeholder { color: var(--muted); }
  .tt-text:focus, .tt-text:global([data-sheet~=focus]) { outline: none; border-bottom-color: var(--focus); }
  .tt-side { display: flex; flex: none; flex-direction: column; gap: 2px; align-items: flex-end; width: 92px; padding-top: 8px; }
  .tt-count { display: inline-flex; gap: 6px; align-items: center; font-size: 11px; color: var(--muted); white-space: nowrap; }
  .ring { width: 22px; height: 22px; transform: rotate(-90deg); }
  .ring circle { fill: none; stroke-width: 2.5; }
  .ring-bg { stroke: var(--line); }
  .ring-fg { stroke: var(--accent); stroke-linecap: round; transition: stroke-dashoffset var(--motion-fast) var(--ease); }
  .tt-count[data-level="near"] { color: var(--sev-medium); }
  .tt-count[data-level="near"] .ring-fg { stroke: var(--sev-medium); }
  .tt-count[data-level="over"] { color: var(--warn); }
  .tt-count[data-level="over"] .ring-fg { stroke: var(--warn); }
  .tt-over { font-size: 11px; color: var(--warn); }
  .tt-video { margin: 10px 0 0; }
  .tt-video video { display: block; width: 100%; max-height: 420px; background: var(--panel); border: var(--border) solid var(--line); border-radius: var(--radius-lg); }
  .tt-video figcaption { margin-top: 4px; font-size: 11px; color: var(--muted); }
  .tt-tools { display: flex; gap: 4px; align-items: center; margin-top: 8px; }
  .tt-num { margin-left: auto; font-size: 11px; color: var(--muted); }
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
