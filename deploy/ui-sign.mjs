#!/usr/bin/env node
// V2-7: `ui.push`'s params for a bundle, signed — one JSON line on stdout,
// for deploy/menger-call.mjs to read on its stdin (a bundle of up to 16 MiB
// never rides an argv):
//
//   node deploy/ui-sign.mjs BUNDLE.tar MANIFEST.json COMMIT | node deploy/menger-call.mjs --url … --agent pipeline ui.push
//
// { bundle: the tar in standard base64, manifest: the bundle's manifest.json
// as text, signature: Ed25519 over the bundle's sha256 digest in base64,
// commit: COMMIT } — UI-3's push: the signature by a key the service names
// with --ui-signer. The key is the pipeline's: $MENGER_DEPLOY_KEY_FILE (the
// seed's file, 0600; or $MENGER_DEPLOY_KEY), never argv — as menger-call
// reads it. Node ≥ 18, its standard library only.
import crypto from "node:crypto";
import fs from "node:fs";
import { deployKeyText, seedOf } from "./menger-call.mjs";

const die = (msg) => { process.stderr.write(`ui-sign: ${msg}\n`); process.exit(1); };
const [bundleFile, manifestFile, commit] = process.argv.slice(2);
if (!bundleFile || !manifestFile) die("usage: ui-sign.mjs BUNDLE.tar MANIFEST.json [COMMIT]");
let text;
try { text = deployKeyText(); } catch (e) { die(e.message); }
if (!text) die("no key: $MENGER_DEPLOY_KEY_FILE names the pipeline's key file");
const seed = seedOf(text);
const priv = crypto.createPrivateKey({ key: Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), seed]), format: "der", type: "pkcs8" });
const bundle = fs.readFileSync(bundleFile);
const manifest = fs.readFileSync(manifestFile, "utf8").trim();
const digest = crypto.createHash("sha256").update(bundle).digest();
const signature = crypto.sign(null, digest, priv).toString("base64");
const params = { bundle: bundle.toString("base64"), manifest, signature };
if (commit) params.commit = commit;
process.stdout.write(JSON.stringify(params) + "\n");
