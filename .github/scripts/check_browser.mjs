#!/usr/bin/env node
const [url, port = "9222"] = process.argv.slice(2);
if (!url) throw new Error("usage: check_browser.mjs URL [DEBUG_PORT]");
const deadline = Date.now() + 60_000;
let page;
while (Date.now() < deadline) {
  try {
    const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    page = pages.find((entry) => entry.type === "page" && entry.url.startsWith(url));
    if (page) break;
  } catch {}
  await new Promise((resolve) => setTimeout(resolve, 250));
}
if (!page) throw new Error("Chrome did not open the requested page through DevTools");
const socket = new WebSocket(page.webSocketDebuggerUrl);
await new Promise((resolve, reject) => {
  socket.addEventListener("open", resolve, { once: true });
  socket.addEventListener("error", reject, { once: true });
});
let nextId = 0;
const pending = new Map();
socket.addEventListener("message", (event) => {
  const message = JSON.parse(event.data);
  if (!message.id) return;
  const waiter = pending.get(message.id);
  if (!waiter) return;
  pending.delete(message.id);
  if (message.error) waiter.reject(new Error(JSON.stringify(message.error)));
  else waiter.resolve(message.result);
});
function command(method, params = {}) {
  const id = ++nextId;
  socket.send(JSON.stringify({ id, method, params }));
  return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
}
await command("Page.enable");
await command("Runtime.enable");
let state;
let readySince;
while (Date.now() < deadline) {
  const evaluated = await command("Runtime.evaluate", {
    expression: `({
      started: document.documentElement.dataset.trunkApplicationStarted === "true",
      isolated: globalThis.crossOriginIsolated === true,
      canvas: (() => {
        const canvas = document.querySelector("canvas");
        if (!canvas) return false;
        const rect = canvas.getBoundingClientRect();
        const expectedWidth = Math.floor(rect.width * devicePixelRatio);
        const expectedHeight = Math.floor(rect.height * devicePixelRatio);
        return rect.width > 0 && rect.height > 0
          && canvas.width >= Math.max(2, expectedWidth * 0.9)
          && canvas.height >= Math.max(2, expectedHeight * 0.9);
      })(),
      href: location.href
    })`,
    returnByValue: true,
  });
  state = evaluated.result?.value;
  if (state?.started && state?.isolated && state?.canvas) {
    readySince ??= Date.now();
    // A dropped embedded Application briefly creates and then removes its canvas.
    // Require sustained readiness so the smoke test proves a live GPUI runtime.
    if (Date.now() - readySince >= 3_000) {
      console.log(JSON.stringify({ ...state, sustainedMs: Date.now() - readySince }));
      socket.close();
      process.exit(0);
    }
  } else {
    readySince = undefined;
  }
  await new Promise((resolve) => setTimeout(resolve, 500));
}
socket.close();
throw new Error(`GPUI browser readiness timed out: ${JSON.stringify(state)}`);
