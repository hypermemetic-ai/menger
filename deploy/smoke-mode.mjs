// ACCESS-27: the login mode a running menger is in, read from the outside
// as anyone meets it — the smoke compares it with the mode the environment
// expects (MENGER_EXPECT_LOGIN) and fails on a mismatch (ACCESS-4 H3: an
// install that dropped test's login settings left test open, and the smoke,
// taking the no-login path, passed).
//
//   node deploy/smoke-mode.mjs <ws port> <shell port>
// Prints one line, `login: off`, `login: on` or `login: required`, and exits
// 0; exits 1 (saying why) when the server does not answer.
//   off       the hub has no gate: an anonymous `_info` is the whole hub
//   on        AUTH-7's gate: an anonymous `_info` is the anonymous view
//             (auth.providers, auth.login, auth.signup), but the app is served
//             without a session
//   required  the gate, and the app is behind its login page (the shell
//             redirects `/` to `/login?next=…`); a release from before AUTH-7
//             that refuses an anonymous upgrade outright, its app behind the
//             login page, is `required` too
// Nothing here needs a credential, and nothing is written.
const [wsPort, shellPort] = process.argv.slice(2);
if (!wsPort || !shellPort) {
  console.error('usage: smoke-mode.mjs <ws port> <shell port>');
  process.exit(2);
}

const timer = setTimeout(() => {
  console.error('mode: no answer in 10 s');
  process.exit(1);
}, 10000);

/** `gated` — the anonymous `_info` is a view; `refused` — no upgrade; `open` — the whole hub. */
function info() {
  return new Promise((resolve, reject) => {
    // No Origin, as synapse and smoke-health.mjs: a script, not a page.
    const ws = new WebSocket(`ws://127.0.0.1:${wsPort}`);
    ws.onerror = () => resolve('refused');
    ws.onopen = () => ws.send(JSON.stringify({ jsonrpc: '2.0', id: 1, method: '_info', params: {} }));
    ws.onmessage = (m) => {
      const item = JSON.parse(m.data).params?.result;
      if (item?.type === 'data' && item.content?.backend) {
        ws.close();
        resolve(item.content.view === 'anonymous' ? 'gated' : 'open');
      } else if (item?.type === 'error') {
        ws.close();
        reject(new Error(`_info: ${item.message}`));
      }
    };
  });
}

/** Whether the shell sends a caller without a session to its login page. */
async function behindLogin() {
  const r = await fetch(`http://127.0.0.1:${shellPort}/`, { redirect: 'manual' });
  return r.status >= 300 && r.status < 400 && (r.headers.get('location') || '').includes('/login?next=');
}

try {
  const [hub, login] = await Promise.all([info(), behindLogin()]);
  // A refused upgrade with the app served openly is a socket not up yet (or
  // broken), not a mode: said, and the smoke asks again.
  if (hub === 'refused' && !login) throw new Error('the WebSocket refused the upgrade, and the app is served without a session');
  const mode = hub === 'refused' ? 'required' : hub === 'open' ? 'off' : login ? 'required' : 'on';
  clearTimeout(timer);
  console.log(`login: ${mode}`);
  process.exit(0);
} catch (e) {
  clearTimeout(timer);
  console.error(`mode: ${e.message}`);
  process.exit(1);
}
