#!/usr/bin/env node
// CHAT-45: which model work goes through — the archetypes' defaults, kept in
// one file that the chat bridge and every launcher read, and the models a
// chat may be switched to.
//
// The file (CHAT_BRIDGE_ARCHETYPES, else ~/.config/menger/archetypes.env) is
// env-style, so a shell launcher may also source it:
//
//   MENGER_MODEL_ARCHITECT=claude-opus-5-5    # answers in chat, through the bridge
//   MENGER_MODEL_BUILDER=claude-opus-5-5      # a detached run that builds a ticket
//   MENGER_MODEL_ANALYST=claude-sonnet-5-5    # reads and reports
//
// A delegate (CHAT-46) takes the default of the archetype it is given. A
// line missing, or the file missing, is the built-in default below. Each
// value is a model's id or its short name (`opus`, `sonnet`, …).
//
// A launcher asks for an archetype's model:
//
//   claude -p --model "$(node scripts/chat-bridge/models.mjs builder)" …
//
// Node ≥ 22, its standard library only.
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

/** The models a chat may be switched to: a short name, the id, the name said. */
export const MODELS = [
  { alias: "opus", id: "claude-opus-5-5", name: "Opus 5.5" },
  { alias: "sonnet", id: "claude-sonnet-5-5", name: "Sonnet 5.5" },
  { alias: "haiku", id: "claude-haiku-5-5", name: "Haiku 5.5" },
  { alias: "fable", id: "claude-fable-5-1", name: "Fable 5.1" },
];

/** The archetypes, what each is, and its built-in default. */
export const ARCHETYPES = {
  architect: { what: "answers in chat, through the bridge", model: "claude-opus-5-5" },
  builder: { what: "a detached run that builds a ticket", model: "claude-opus-5-5" },
  analyst: { what: "reads and reports", model: "claude-sonnet-5-5" },
};

export const defaultFile = (env = process.env) =>
  env.CHAT_BRIDGE_ARCHETYPES || path.join(os.homedir(), ".config/menger/archetypes.env");

/** A model named by its id, short name or spoken name; null when none is. A
 *  `claude-…` id not in the list is taken as written (a newer model). */
export function resolveModel(word) {
  const w = String(word ?? "").trim().toLowerCase();
  if (!w) return null;
  const hit = MODELS.find((m) => m.alias === w || m.id === w || m.name.toLowerCase() === w || m.name.toLowerCase().replace(/\s+/g, "") === w.replace(/\s+/g, ""));
  if (hit) return hit.id;
  return /^claude-[a-z0-9][a-z0-9.-]*$/.test(w) ? w : null;
}

/** A model's name as said in a chat: `Sonnet 5.5`, or the id itself. */
export const modelName = (id) => MODELS.find((m) => m.id === id)?.name ?? id;

/** The archetypes' defaults: the built-ins, overridden by the file's lines.
 *  Read on every call (an edit needs no restart); `problems` names a line it
 *  could not read, which keeps the built-in. */
export function readDefaults(file = defaultFile()) {
  const models = Object.fromEntries(Object.entries(ARCHETYPES).map(([a, v]) => [a, v.model]));
  const from = Object.fromEntries(Object.keys(ARCHETYPES).map((a) => [a, "built-in"]));
  const problems = [];
  let text = null;
  try { text = fs.readFileSync(file, "utf8"); } catch (e) { if (e.code !== "ENOENT") problems.push(`${file}: ${e.message}`); }
  for (const [i, raw] of (text ?? "").split("\n").entries()) {
    const line = raw.replace(/#.*/, "").trim();
    if (!line) continue;
    const m = /^(?:export\s+)?MENGER_MODEL_([A-Z_]+)\s*=\s*["']?([^"'\s]*)["']?$/.exec(line);
    if (!m) { problems.push(`${file}:${i + 1}: not MENGER_MODEL_<ARCHETYPE>=<model>`); continue; }
    const arch = m[1].toLowerCase();
    if (!Object.hasOwn(ARCHETYPES, arch)) { problems.push(`${file}:${i + 1}: no archetype '${arch}'`); continue; }
    const id = resolveModel(m[2]);
    if (!id) { problems.push(`${file}:${i + 1}: no model '${m[2]}'`); continue; }
    models[arch] = id; from[arch] = file;
  }
  return { models, from, problems, file };
}

// a launcher's question: `node models.mjs builder` prints the builder's model
if (process.argv[1] && fs.realpathSync(process.argv[1]) === fs.realpathSync(fileURLToPath(import.meta.url))) {
  const arch = (process.argv[2] ?? "").toLowerCase();
  const d = readDefaults();
  for (const p of d.problems) console.error(`models: ${p}`);
  if (!Object.hasOwn(ARCHETYPES, arch)) {
    console.error(`usage: models.mjs ARCHETYPE — one of ${Object.keys(ARCHETYPES).join(", ")}`);
    process.exit(2);
  }
  console.log(d.models[arch]);
}
