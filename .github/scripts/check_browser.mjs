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
      bridge: typeof globalThis.__nodeGraphTestState === "function",
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
  if (state?.started && state?.isolated && state?.canvas && state?.bridge) {
    readySince ??= Date.now();
    // A dropped embedded Application briefly creates and then removes its canvas.
    // Require sustained readiness so the smoke test proves a live GPUI runtime.
    if (Date.now() - readySince >= 3_000) break;
  } else {
    readySince = undefined;
  }
  await new Promise((resolve) => setTimeout(resolve, 500));
}
if (!(state?.started && state?.isolated && state?.canvas && state?.bridge)) {
  socket.close();
  throw new Error(`GPUI browser readiness timed out: ${JSON.stringify(state)}`);
}

const canvasState = await command("Runtime.evaluate", {
  expression: `(() => { const r = document.querySelector("canvas").getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`,
  returnByValue: true,
});
const { x, y } = canvasState.result.value;
const pause = (ms = 300) => new Promise((resolve) => setTimeout(resolve, ms));
async function click(clickCount = 1) {
  for (const type of ["mousePressed", "mouseReleased"])
    await command("Input.dispatchMouseEvent", { type, x, y, button: "left", clickCount });
}
async function key(key, code = key, text) {
  const params = { key, code, ...(text ? { text } : {}) };
  await command("Input.dispatchKeyEvent", { type: "keyDown", ...params });
  await command("Input.dispatchKeyEvent", { type: "keyUp", ...params });
}
async function shot() {
  await pause();
  return (await command("Page.captureScreenshot", {
    format: "png", captureBeyondViewport: false,
  })).data;
}

async function graphState() {
  const result = await command("Runtime.evaluate", {
    expression: `globalThis.__nodeGraphTestState?.()`,
    returnByValue: true,
  });
  return JSON.parse(result.result.value);
}
async function waitFor(label, predicate) {
  const until = Date.now() + 8_000;
  let value;
  while (Date.now() < until) {
    value = await graphState();
    if (predicate(value)) return value;
    await pause(100);
  }
  throw new Error(`${label} timed out: ${JSON.stringify(value)}`);
}

const baseline = await graphState();
if (baseline.sourceWidth < 100) {
  throw new Error(`node width collapsed before interaction trace: ${JSON.stringify(baseline)}`);
}
await click();
await key("Tab", "Tab");
await waitFor("catalog opening", (value) => value.catalogOpen);
for (const character of "Math") await key(character, `Key${character.toUpperCase()}`, character);
await key("Enter", "Enter");
const created = await waitFor(
  "searched node creation",
  (value) => !value.catalogOpen && value.nodes === baseline.nodes + 1,
);
await key("Escape", "Escape");
await waitFor("overlay dismissal", (value) => value.overlayDismissed);
await command("Input.dispatchMouseEvent", {
  type: "mouseWheel", x, y, deltaX: 0, deltaY: -180,
});
const zoomed = await waitFor("pointer zoom", (value) => value.zoom !== created.zoom);
await pause(1_000);
const stable = await graphState();
if (Math.abs(stable.sourceWidth - baseline.sourceWidth) > 0.1) {
  throw new Error(`node width changed across retained frames: ${JSON.stringify({ baseline, stable })}`);
}
if (Math.abs(stable.sourceHeight - baseline.sourceHeight) > 0.5) {
  throw new Error(`node layout changed across zoom: ${JSON.stringify({ baseline, stable })}`);
}
const transitions = {
  menuOpened: true,
  nodeCreated: true,
  overlayDismissed: true,
  viewportChanged: zoomed.zoom !== created.zoom,
};
await shot();

const finalState = await command("Runtime.evaluate", {
  expression: `(() => { const c = document.querySelector("canvas"); const r = c?.getBoundingClientRect();
    return !!c && c.width > 1 && c.height > 1 && r.width > 0 && r.height > 0; })()`,
  returnByValue: true,
});
if (!finalState.result.value) throw new Error("GPUI canvas was lost during interaction trace");
console.log(JSON.stringify({ ...state, sustainedMs: Date.now() - readySince, transitions }));
socket.close();
