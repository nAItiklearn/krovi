// QA only: evaluate an expression inside the installed Krovi webview via WebView2's
// remote-debugging port.  usage: node qa/cdp.mjs <port> "<js expression>"
const [port, expr] = process.argv.slice(2);
const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
const page = targets.find((t) => t.type === 'page');
const ws = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((r) => (ws.onopen = r));
ws.send(JSON.stringify({ id: 1, method: 'Runtime.evaluate', params: { expression: expr, awaitPromise: true, returnByValue: true } }));
ws.onmessage = (m) => {
  const d = JSON.parse(m.data);
  if (d.id !== 1) return;
  const r = d.result?.result;
  console.log(d.result?.exceptionDetails ? 'EXCEPTION ' + JSON.stringify(d.result.exceptionDetails) : JSON.stringify(r?.value ?? r, null, 1));
  ws.close();
};
