// ACCESS-6: prints every raw frame of one spike.call on :4670 — node raw.mjs <session|-> <method> <json params>
const [cookie, method, params] = process.argv.slice(2);
const headers = cookie && cookie !== '-' ? { Cookie: `access_token=${cookie}` } : {};
const ws = new WebSocket('ws://127.0.0.1:4670', { headers });
ws.onopen = () => {
  const req = { jsonrpc: '2.0', id: 1, method: 'spike.call', params: { method, params: JSON.parse(params) } };
  console.log('>>', JSON.stringify(req));
  ws.send(JSON.stringify(req));
};
ws.onmessage = (m) => {
  console.log('<<', m.data);
  const j = JSON.parse(m.data);
  const t = j.params?.result?.type;
  if (j.error || t === 'done' || t === 'error') setTimeout(() => process.exit(0), 100);
};
ws.onerror = (e) => { console.log('!! ', e.message ?? e); process.exit(1); };
setTimeout(() => process.exit(2), 3000);
