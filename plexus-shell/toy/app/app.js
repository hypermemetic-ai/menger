// The toy's app: open the hub's WebSocket the shell stamped into the page
// and ask `echo.say` through the hub's `toy.call` subscription.
const port = document.querySelector('meta[name="plexus-ws-port"]').content;
const ws = new WebSocket(`ws://${location.hostname}:${port}`);
ws.onopen = () =>
  ws.send(JSON.stringify({ jsonrpc: "2.0", id: 1, method: "toy.call", params: { method: "echo.say", params: { text: "hi" } } }));
ws.onmessage = (m) => document.body.append(m.data);
