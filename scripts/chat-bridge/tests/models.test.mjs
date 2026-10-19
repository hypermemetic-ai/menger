// CHAT-45: the archetypes file and the model names, held —
// `node --test scripts/chat-bridge/tests/models.test.mjs`.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import { ARCHETYPES, modelName, readDefaults, resolveModel } from "../models.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "chat-45-models-"));
process.on("exit", () => fs.rmSync(dir, { recursive: true, force: true }));
const file = (name, text) => { const f = path.join(dir, name); fs.writeFileSync(f, text); return f; };

test("a model is named by its short name, its id, its name, or a claude- id", () => {
  assert.equal(resolveModel("sonnet"), "claude-sonnet-5-5");
  assert.equal(resolveModel(" Opus "), "claude-opus-5-5");
  assert.equal(resolveModel("claude-haiku-5-5"), "claude-haiku-5-5");
  assert.equal(resolveModel("Fable 5.1"), "claude-fable-5-1");
  assert.equal(resolveModel("fable5.1"), "claude-fable-5-1");
  assert.equal(resolveModel("claude-opus-6-0"), "claude-opus-6-0");
  for (const w of ["", "gpt-9", "claude-", "claude-Opus; rm", "default"]) assert.equal(resolveModel(w), null, w);
  assert.equal(modelName("claude-sonnet-5-5"), "Sonnet 5.5");
  assert.equal(modelName("claude-opus-6-0"), "claude-opus-6-0");
});

test("no file: the built-in defaults", () => {
  const d = readDefaults(path.join(dir, "absent.env"));
  assert.deepEqual(d.models, { architect: "claude-opus-5-5", builder: "claude-opus-5-5", analyst: "claude-sonnet-5-5" });
  assert.deepEqual(d.problems, []);
  assert.equal(d.from.architect, "built-in");
});

test("the file's lines override; a bad line is named and keeps the built-in", () => {
  const f = file("a.env", [
    "# comment", "", "MENGER_MODEL_ARCHITECT=haiku", "export MENGER_MODEL_BUILDER=\"claude-fable-5-1\"  # quoted, exported",
    "MENGER_MODEL_ANALYST=gpt-9", "MENGER_MODEL_JANITOR=opus", "nonsense",
  ].join("\n"));
  const d = readDefaults(f);
  assert.deepEqual(d.models, { architect: "claude-haiku-5-5", builder: "claude-fable-5-1", analyst: "claude-sonnet-5-5" });
  assert.equal(d.from.architect, f);
  assert.equal(d.from.analyst, "built-in");
  assert.equal(d.problems.length, 3);
  assert.match(d.problems[0], /:5: no model 'gpt-9'/);
  assert.match(d.problems[1], /:6: no archetype 'janitor'/);
  assert.match(d.problems[2], /:7: not MENGER_MODEL_/);
});

test("the example file is the built-in defaults, and a shell may source it", () => {
  const ex = path.join(here, "..", "archetypes.env.example");
  const d = readDefaults(ex);
  assert.deepEqual(d.problems, []);
  for (const [a, v] of Object.entries(ARCHETYPES)) assert.equal(d.models[a], v.model, a);
  const out = execFileSync("sh", ["-c", `. "${ex}" && echo "$MENGER_MODEL_ANALYST"`], { encoding: "utf8" }).trim();
  assert.equal(out, "claude-sonnet-5-5");
});

test("a launcher asks: models.mjs ARCHETYPE prints its model", () => {
  const f = file("b.env", "MENGER_MODEL_BUILDER=sonnet\n");
  const run = (arch) => execFileSync(process.execPath, [path.join(here, "..", "models.mjs"), arch], { encoding: "utf8", env: { ...process.env, CHAT_BRIDGE_ARCHETYPES: f } }).trim();
  assert.equal(run("builder"), "claude-sonnet-5-5");
  assert.equal(run("analyst"), "claude-sonnet-5-5");
  assert.equal(run("architect"), "claude-opus-5-5");
  assert.throws(() => execFileSync(process.execPath, [path.join(here, "..", "models.mjs"), "janitor"], { stdio: "pipe" }), /status 2|Command failed/);
});
