// ACCESS-7 spike: the smoke's credential under login always, with no IdP and
// no stored session — a fresh plexus-key proof per check, signed from a key
// file (0600, or $CREDENTIALS_DIRECTORY/<name>.key), never argv or the env.
//
//   node spike/ACCESS-7/smoke-key-proof.mjs <ws port> <key file> [agent]
//
// Prints the bearer value on stdout for deploy/smoke-health.mjs to present:
//   node deploy/smoke-health.mjs 4610 "$(node spike/ACCESS-7/smoke-key-proof.mjs 4610 key)"
// (The value is good once, for a minute: argv exposure is a used nonce.)
// A spike, not production code: ACCESS-24 / ACCESS-27 build the real one.
import fs from "node:fs";
import { seedOf, keyProof } from "../../deploy/menger-call.mjs";

const [port, file, agent = "smoke"] = process.argv.slice(2);
if (!port || !file) {
  console.error("usage: smoke-key-proof.mjs <ws port> <key file> [agent]");
  process.exit(2);
}
const mode = fs.statSync(file).mode & 0o077;
if (mode) {
  console.error(`${file}: readable by group or others (mode ${(fs.statSync(file).mode & 0o777).toString(8)}); 0600 or 0400 only`);
  process.exit(2);
}
const seed = seedOf(fs.readFileSync(file, "utf8"));
process.stdout.write(keyProof(seed, `ws://127.0.0.1:${port}`, agent) + "\n");
