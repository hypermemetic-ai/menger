// IDENTITY-23: the smoke check's health half when the environment requires
// login — `tickets.health` over the WebSocket, as a caller with a token, and
// the proof that a caller without one is refused at the upgrade. synapse
// does the same once its first probe carries a token (IDENTITY-16).
//
//   node deploy/smoke-health.mjs <ws port> [token]
// Prints `ticket_count: N` and exits 0; without a token, prints `refused`
// and exits 0 when the upgrade is refused, 1 when it is admitted.
const [port, token] = process.argv.slice(2);
if (!port) {
  console.error('usage: smoke-health.mjs <ws port> [token]');
  process.exit(2);
}
const ws = new WebSocket(`ws://127.0.0.1:${port}`, token ? { headers: { authorization: `Bearer ${token}` } } : undefined);
const timer = setTimeout(() => {
  console.error('health: no answer in 10 s');
  process.exit(1);
}, 10000);
ws.onerror = () => {
  clearTimeout(timer);
  console.log(token ? 'health: the upgrade was refused' : 'refused');
  process.exit(token ? 1 : 0);
};
ws.onopen = () => {
  if (!token) {
    clearTimeout(timer);
    console.log('admitted: anonymous callers are not refused');
    process.exit(1);
  }
  ws.send(JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'menger.call', params: { method: 'tickets.health', params: {} } }));
};
ws.onmessage = (m) => {
  const item = JSON.parse(m.data).params?.result;
  if (item?.type === 'data' && item.content?.type === 'health') {
    clearTimeout(timer);
    console.log(`ticket_count: ${item.content.ticket_count}`);
    process.exit(0);
  }
  if (item?.type === 'data' && item.content?.type === 'error') {
    clearTimeout(timer);
    console.error(`health: ${item.content.message}`);
    process.exit(1);
  }
};
