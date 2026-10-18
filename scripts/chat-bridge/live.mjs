// CHAT-18: the chat bridge's live line — what `claude -p --output-format
// stream-json --verbose` says it does, as short lines for `chats.activity`:
// one per tool call, the assistant's interim text, a heartbeat when it is
// quiet; throttled, and never a secret. Pure but for the timers; bridge.mjs
// drives it, tests/live.test.mjs holds it (`node --test`).

/** The longest line sent; the server keeps 500 characters. */
export const LINE_MAX = 300;
/** At least this long between two sends; what comes meanwhile is batched. */
export const THROTTLE_MS = 300;
/** A heartbeat after this long without a send. */
export const HEARTBEAT_MS = 15_000;

/** `text` with anything that looks like a secret masked: `NAME=value` where
 *  NAME says key/token/secret/password, a bearer, `mgs_…` session tokens,
 *  provider-shaped keys, and any run of 32+ hex or base64 characters. */
export function mask(text) {
  return String(text ?? "")
    .replace(/\b([A-Za-z0-9_]*(?:KEY|TOKEN|SECRET|PASSWORD|PASSWD|PASS|AUTH|COOKIE|SEED|CREDENTIALS?)[A-Za-z0-9_]*)("?\s*([=:])\s*)("[^"]*"|'[^']*'|\S+)/gi,
      // `=` always; `:` for a NAME_LIKE_THIS (prose's "the key: …" stays)
      (all, name, sep, op, _v) => (op === "=" || /_|^[A-Z0-9]+$/.test(name) ? `${name}${sep}•••` : all))
    .replace(/\b(Bearer|Basic)\s+\S+/gi, "$1 •••")
    .replace(/\bmgs_[A-Za-z0-9_\-]+/g, "mgs_•••")
    .replace(/\b(sk|pk|rk|ghp|gho|ghs|github_pat|xox[abprs]|glpat|AKIA)[-_][A-Za-z0-9_\-]{8,}/g, "$1_•••")
    .replace(/[A-Za-z0-9+/_\-]{32,}={0,2}/g, (run) => (secretish(run) ? "•••" : run));
}

/** Whether a 32+ run reads as key material: all hex, or base64-like
 *  (digits and both cases) — a path's segments each judged alone, so a path
 *  or a long identifier stays. */
function secretish(run) {
  const bare = run.replace(/=+$/, "");
  if (/^[0-9a-f]{32,}$/i.test(bare)) return true;
  const mixed = (t) => /[0-9]/.test(t) && /[a-z]/.test(t) && /[A-Z]/.test(t);
  if (bare.includes("/")) return bare.split("/").some((seg) => seg.length >= 24 && mixed(seg));
  return mixed(bare);
}

/** One line, at most `max` characters: whitespace folded, cut with `…`. */
export function clip(text, max = LINE_MAX) {
  const t = String(text ?? "").replace(/\s+/g, " ").trim();
  return t.length <= max ? t : `${t.slice(0, max - 1)}…`;
}

/** A path as the agent's working directory sees it — `cwd` one directory
 *  or several (its name and its real path, /home being /var/home here). */
const rel = (p, cwd) => {
  const s = String(p ?? "");
  const at = [cwd].flat().filter(Boolean).find((c) => s.startsWith(`${c}/`));
  return at ? s.slice(at.length + 1) : s;
};

/** One tool call, as a line: `Bash: <what>`, `Read <path>`, `Agent: <what>`, … */
export function describeTool(name, input = {}, cwd = "") {
  const i = input ?? {};
  switch (name) {
    case "Bash": return `Bash: ${i.description || clip(i.command, 120)}`;
    case "Read": case "Edit": case "Write": case "MultiEdit": return `${name} ${rel(i.file_path, cwd)}`;
    case "NotebookEdit": return `NotebookEdit ${rel(i.notebook_path ?? i.file_path, cwd)}`;
    case "Grep": return `Grep ${clip(i.pattern, 120)}${i.path ? ` in ${rel(i.path, cwd)}` : ""}`;
    case "Glob": return `Glob ${clip(i.pattern, 120)}`;
    case "Agent": case "Task": return `Agent: ${i.description || clip(i.prompt, 120)}`;
    case "WebFetch": return `WebFetch ${i.url ?? ""}`;
    case "WebSearch": return `WebSearch ${clip(i.query, 120)}`;
    case "TodoWrite": return `TodoWrite (${(i.todos ?? []).length} items)`;
    case "Skill": return `Skill ${i.skill ?? ""}`;
    default: {
      const shown = Object.entries(i).filter(([, v]) => typeof v === "string" || typeof v === "number").map(([k, v]) => `${k}=${v}`).join(" ");
      return `${name}${shown ? ` ${clip(shown, 120)}` : ""}`;
    }
  }
}

/** The lines one stream-json event says, masked and clipped. */
export function linesOf(event, cwd = "") {
  if (event?.type !== "assistant") return [];
  const out = [];
  for (const block of event.message?.content ?? []) {
    if (block?.type === "tool_use") out.push(describeTool(block.name, block.input, cwd));
    else if (block?.type === "text" && block.text?.trim()) out.push(clip(block.text, 200));
  }
  return out.map((l) => clip(mask(l)));
}

/** The session a stream names (any event carries it), the final result. */
export function readStream(text) {
  let session = null, result = null, events = 0;
  for (const raw of String(text).split("\n")) {
    const line = raw.trim();
    if (!line) continue;
    let j; try { j = JSON.parse(line); } catch { continue; }
    events++;
    if (j.session_id) session = j.session_id;
    if (j.type === "result") result = j;
  }
  return { session, result, events };
}

/** A JSONL splitter: `push(chunk)` calls `onEvent` for each whole line. */
export function jsonLines(onEvent) {
  let rest = "";
  return {
    push(chunk) {
      rest += chunk;
      let n;
      while ((n = rest.indexOf("\n")) >= 0) {
        const line = rest.slice(0, n).trim();
        rest = rest.slice(n + 1);
        if (!line) continue;
        try { onEvent(JSON.parse(line)); } catch {}
      }
    },
  };
}

/** The lines waiting at a flush, as one: the newest kept whole when they
 *  do not fit. */
export function batch(lines, max = LINE_MAX) {
  if (lines.length <= 1) return lines[0] ?? null;
  const joined = lines.join(" · ");
  if (joined.length <= max) return joined;
  return clip(`(+${lines.length - 1}) ${lines[lines.length - 1]}`, max);
}

/** A turn's live line: `line(text)` queues, at most one send each
 *  THROTTLE_MS (what queued meanwhile batched), a heartbeat
 *  `working · Ns` after HEARTBEAT_MS of quiet; `stop()` flushes and ends.
 *  `send(state, line)` returns a promise; its failure is the caller's. */
export function liveLine(send, { now = () => Date.now(), throttleMs = THROTTLE_MS, heartbeatMs = HEARTBEAT_MS, setTimer = setTimeout, clearTimer = clearTimeout } = {}) {
  const began = now();
  let last = 0, queued = [], timer = null, beat = null, stopped = false, sent = 0;
  const fire = () => {
    timer = null;
    if (!queued.length || stopped) return;
    const l = batch(queued);
    queued = [];
    last = now(); sent++;
    send("working", l);
  };
  const tick = () => {
    if (stopped) return;
    if (now() - last >= heartbeatMs && !queued.length) {
      last = now();
      send("working", `working · ${Math.round((now() - began) / 1000)}s`);
    }
    beat = setTimer(tick, Math.min(heartbeatMs, 1000));
  };
  last = now();
  beat = setTimer(tick, Math.min(heartbeatMs, 1000));
  return {
    line(text) {
      if (stopped || !text) return;
      queued.push(text);
      if (timer) return;
      const wait = Math.max(0, last + throttleMs - now());
      timer = setTimer(fire, wait);
    },
    /** Flush what waits; no heartbeat after. */
    stop() {
      if (timer) { clearTimer(timer); fire(); }
      stopped = true;
      clearTimer(beat);
      return sent;
    },
  };
}
