// IDENTITY-23: the smoke check's health half when the environment requires
// login — `tickets.health` over the WebSocket, as a caller with a token, and
// the proof that a caller without one reaches nothing. synapse does the same
// once its first probe carries a token (IDENTITY-16).
//
//   node deploy/smoke-health.mjs <ws port> [token | -]
// ACCESS-27 (L3): `-` reads the token from stdin, so it never rides in argv
// (`ps` shows argv to every user); the pipeline's smoke always passes it so.
// With a token: prints `ticket_count: N` and exits 0. AUTH-7: the token's
// principal needs `tickets.health` (viewer somewhere); a typed refusal is
// printed, and exits 1.
// Without a token: AUTH-7 admits the upgrade into the anonymous view — `_info`
// says `view: anonymous` and names only the auth door (auth.providers,
// auth.login, auth.signup), and `tickets.health` is refused unauthenticated;
// prints `anonymous view` and exits 0, or what was wrong and exits 1. A
// release from before AUTH-7 refuses the upgrade outright: `refused`, 0.
import { readFileSync } from 'node:fs';

const [port, given] = process.argv.slice(2);
if (!port) {
  console.error('usage: smoke-health.mjs <ws port> [token | -]');
  process.exit(2);
}
const token = given === '-' ? readFileSync(0, 'utf8').trim() : given;
if (given === '-' && !token) {
  console.error('health: no token on stdin');
  process.exit(1);
}
const DOOR = ['auth.providers', 'auth.login', 'auth.signup'];
const ws = new WebSocket(`ws://127.0.0.1:${port}`, token ? { headers: { authorization: `Bearer ${token}` } } : undefined);
const timer = setTimeout(() => {
  console.error('health: no answer in 10 s');
  process.exit(1);
}, 10000);
const end = (code, line, err = false) => {
  clearTimeout(timer);
  (err ? console.error : console.log)(line);
  process.exit(code);
};
ws.onerror = () => end(token ? 1 : 0, token ? 'health: the upgrade was refused' : 'refused');
ws.onopen = () => {
  if (!token) {
    ws.send(JSON.stringify({ jsonrpc: '2.0', id: 1, method: '_info', params: {} }));
    return;
  }
  ws.send(JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'menger.call', params: { method: 'tickets.health', params: {} } }));
};
let sawDoor = false;
ws.onmessage = (m) => {
  const item = JSON.parse(m.data).params?.result;
  if (!item) return;
  // The anonymous view's _info.
  if (!token && item.type === 'data' && item.content?.backend) {
    const c = item.content;
    const methods = c.auth?.methods || [];
    if (c.view !== 'anonymous' || JSON.stringify(methods) !== JSON.stringify(DOOR)) {
      end(1, `admitted, but not into the anonymous view: ${JSON.stringify(c)}`, true);
    }
    sawDoor = true;
    ws.send(JSON.stringify({ jsonrpc: '2.0', id: 2, method: 'menger.call', params: { method: 'tickets.health', params: {} } }));
    return;
  }
  if (item.type === 'data' && item.content?.type === 'health') {
    if (!token) end(1, 'admitted: an anonymous caller reached tickets.health', true);
    end(0, `ticket_count: ${item.content.ticket_count}`);
  }
  if (item.type === 'error') {
    if (!token && sawDoor && item.error_kind === 'unauthenticated') end(0, 'anonymous view');
    const what = item.grant ? ` (${item.error_kind}: needs ${item.grant} on ${item.scope})` : '';
    end(1, `health: ${item.message}${what}`, true);
  }
  if (item.type === 'data' && item.content?.type === 'error') end(1, `health: ${item.content.message}`, true);
};
