<!-- COMPS-3: a body holding raw HTML the page cannot draw (m-mockup's
     `holdsRaw`: an HTML element outside the sanitizer's allow-list, or an
     allowed one with an attribute it drops) — the whole body as one frame,
     an ordinary svelte/1 component on plexus-ui's frame API: it builds the
     frame's document and m-mockup frames it (`doc`, `data-kind=body`).
     ticket-view's `TAGS` table names it `m-page`, for a body it found raw,
     and hands it the body's pieces (`pieces`: each piece's text and the
     markdown it draws as; none: the source is one piece). The document: the
     markdown of each piece through the host's render, as drawn outside; a
     piece holding raw HTML as written; a page block a nested sandboxed frame
     (it inherits the frame's own policy, so its scripts run); the body's
     type on the app's tokens (put in by the engine's frame, COMPS-2:
     `theme`), and `<base target="_blank">`, so a link opens a tab.
     EDITOR-15, COMPS-2: its ```mermaid fences are drawn before it is handed
     over (`prepare`), so a diagram draws inside the frame as outside.
     EDITOR-14 (the owner: "also you know that v1 could just host raw html
     inside a frame right?"; v1's `is_framed`). -->
<script module>
  import { pageSpec, holdsRaw } from "./m-mockup.svelte";

  export const uses = [];

  const FENCE = /^ {0,3}(`{3,}|~{3,})/;
  const esc = (t) => String(t).replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  /** The frame's own sheet for a raw body: the body's type on the app's tokens (put in by the engine's frame,
   *  COMPS-2), and a diagram drawn before it was handed over (`drawDiagrams`) on Pele's variables. */
  const RAW_SHEET = `html,body{margin:0;background:transparent}body{color:var(--ink,#1d1b16);font:14px/1.6 var(--sans,system-ui,sans-serif);overflow-wrap:anywhere}
a{color:var(--accent,#2f5fb3)}h1,h2,h3,h4{margin:14px 0 4px;font-family:var(--font-display,inherit);line-height:1.3}h1{font-size:20px}h2{font-size:16px}h3,h4{font-size:14px}
p{margin:6px 0}ul,ol{margin:4px 0;padding-left:20px}img{max-width:100%;height:auto}
code{padding:0 3px;font-family:var(--font-text,ui-monospace,monospace);font-size:12.5px;background:var(--panel,#f2f1ec);border-radius:3px}
pre{overflow-x:auto;padding:10px 12px;font-family:var(--font-text,ui-monospace,monospace);font-size:12.5px;background:var(--panel,#f2f1ec);border-radius:6px}pre code{padding:0;background:none}
table{border-collapse:collapse;margin:6px 0}th,td{padding:4px 14px 4px 0;text-align:left;vertical-align:top;border-bottom:1px solid var(--line,#ddd)}
blockquote{margin:6px 0;padding-left:14px;color:var(--muted,#777);border-left:2px solid var(--line,#ddd)}hr{border:0;border-top:1px solid var(--line,#ddd)}
:is(m-summary,m-decision,m-input,m-cases,m-beat,m-status,m-diagram,m-results,m-walk,m-code,m-compare,m-callout,m-acceptance,m-cards,m-card,m-dag,m-details,m-step,m-question,m-answer){display:block;margin:6px 0}
m-step::before{content:attr(date) " · " attr(status) " · " attr(by) "  ";font-size:11px;color:var(--muted,#777)}
.m-diagram{margin:6px 0;overflow-x:auto;--pele-bg:transparent;--pele-fg:var(--ink);--pele-muted:var(--muted);--pele-line:var(--muted);--pele-surface:var(--panel);--pele-surface-alt:var(--hover);--pele-border:var(--line);--pele-accent:var(--accent);--pele-font:var(--sans);--pele-radius:var(--radius);--pele-series-1:var(--accent);--pele-series-2:var(--kind-scope);--pele-series-3:var(--kind-execution);--pele-series-4:var(--kind-spike);--pele-series-5:var(--kind-gap);--pele-series-6:var(--live);--pele-series-7:var(--warn);--pele-series-8:var(--kind-doc)}.m-diagram svg{display:block;max-width:100%;height:auto;margin:0 auto}
.m-page{margin:8px 0}.m-page figcaption{margin:0 0 6px;font-size:12px;font-weight:600}.m-page iframe{display:block;width:100%;border:1px solid var(--line,#ddd);border-radius:6px;background:var(--panel,#f2f1ec)}.m-note{font-size:12px;color:var(--muted,#777)}`;

  /** A page block inside a raw body: a nested frame (it inherits the frame's own policy, so its scripts run), or a note. */
  function nested(p) {
    if (p?.kind !== "page") return `<p class="m-note">${p?.kind === "ref" ? `The page is ${esc(p.attrs.ref)}'s body.` : "A live view draws outside a framed body."}</p>`;
    const label = p.attrs.label || p.attrs.title || "";
    const h = Math.min(2000, Number(p.attrs.height) || 600);
    return `<figure class="m-page">${label ? `<figcaption>${esc(label)}</figcaption>` : ""}<iframe sandbox="allow-scripts allow-forms" title="${esc(label || "a page")}" style="height:${h}px" srcdoc="${esc(p.html)}"></iframe></figure>`;
  }

  /** The frame's document of a raw body: `pieces` `[{ text, page, md }]`, each drawn as above by `render`
   *  (the host's markdown). */
  export function rawDoc(pieces, render) {
    const out = [];
    for (const p of pieces) {
      if (p.page) out.push(nested(pageSpec(p.text)));
      else if (!FENCE.test(p.text) && holdsRaw(p.text)) out.push(p.text);
      else out.push(render(p.md ?? p.text));
    }
    return `<!doctype html><html><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><base target="_blank">`
      + `<style>${RAW_SHEET}</style></head><body>\n${out.join("\n")}\n</body></html>`;
  }
</script>

<script>
  import { mount, unmount } from "svelte";
  import { markdown } from "plexus-ui/host";
  import Diagram from "plexus-ui:mermaid-diagram";
  import MMockup from "./m-mockup.svelte";

  /** `source`: the body. `pieces`: its pieces as ticket-view split it, else the source as one. */
  let { source = "", pieces = null } = $props();
  const doc = $derived(rawDoc(pieces ?? [{ text: source, page: false, md: source }], markdown));

  /** EDITOR-15, COMPS-2: a raw body's ```mermaid fences drawn before the frame is handed it — each by the diagram's
   *  own component, in `within` (m-mockup's stage, unseen), its SVG put in place of the fence's `<pre>` in the frame's
   *  document — so a diagram draws inside the frame as outside (the frame loads no module of the app's). The SVG is
   *  Pele's own drawing (it escapes the diagram's text, filters links and styles) and goes only into the sandboxed
   *  frame, never into the page. A fence Pele refuses stays its source. */
  const FENCE_HTML = /<pre><code class="language-mermaid">([\s\S]*?)<\/code><\/pre>/g;
  const unescaped = (t) => t.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&amp;/g, "&");
  async function drawn1(source, within) {
    const at = document.createElement("div");
    at.setAttribute("aria-hidden", "true");
    at.style.cssText = "position:absolute;left:0;top:0;width:100%;opacity:0;pointer-events:none;z-index:-1";
    within.appendChild(at);
    const app = mount(Diagram, { target: at, props: { source } });
    try {
      for (const t0 = Date.now(); Date.now() - t0 < 8000; await new Promise((r) => setTimeout(r, 50))) {
        const fig = at.querySelector("[data-surface=mermaid-diagram]");
        if (fig?.dataset.state === "refused") return null;
        const svg = fig?.dataset.state === "drawn" ? fig.querySelector("svg") : null;
        if (svg) return new XMLSerializer().serializeToString(svg);
      }
      return null;
    } finally {
      unmount(app);
      at.remove();
    }
  }
  async function drawDiagrams(html, within) {
    const fences = [...html.matchAll(FENCE_HTML)];
    if (!fences.length) return html;
    const svgs = await Promise.all(fences.map((m) => drawn1(unescaped(m[1]), within ?? document.body)));
    let k = 0;
    return html.replace(FENCE_HTML, (whole) => {
      const svg = svgs[k++];
      return svg ? `<div class="m-diagram">${svg}</div>` : whole;
    });
  }
</script>

<MMockup {source} {doc} prepare={drawDiagrams}/>
