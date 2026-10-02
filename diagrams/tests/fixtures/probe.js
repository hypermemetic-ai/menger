const fs = require('fs'), vm = require('vm'), { JSDOM } = require('jsdom');
const src = fs.readFileSync(process.argv[2], 'utf8');
const dom = new JSDOM('<!doctype html><html><body></body></html>', { runScripts: 'outside-only', pretendToBeVisual: true });
const t0 = performance.now();
new vm.Script(src, { filename: "mermaid.min.js" }).runInContext(dom.getInternalVMContext());
const mermaid = dom.window.mermaid;
const loadMs = performance.now() - t0;
(async () => {
  mermaid.initialize({ startOnLoad: false, securityLevel: 'strict' });
  const cases = JSON.parse(fs.readFileSync(process.argv[3], 'utf8'));
  let ok = 0, bad = 0; const times = [];
  for (const c of cases) {
    const t = performance.now();
    try { await mermaid.parse(c.src); ok++; if (c.expect === 'error') console.log('MISSED (browser says error):', c.id); }
    catch (e) { bad++; console.log((c.expect === 'error' ? 'error, as in the browser:' : 'FALSE ERROR:').padEnd(28), c.id, '|', String(e.message || e).split('\n').slice(0, 2).join(' | ').slice(0, 150)); }
    times.push(performance.now() - t);
  }
  times.sort((a, b) => a - b);
  console.log(`load ${loadMs.toFixed(0)} ms; ${cases.length} diagrams: ${ok} ok, ${bad} error; parse median ${times[times.length >> 1].toFixed(1)} ms, max ${times[times.length - 1].toFixed(1)} ms; RSS ${(process.memoryUsage().rss / 2 ** 20).toFixed(0)} MiB`);
})();
