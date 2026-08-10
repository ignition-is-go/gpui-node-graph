#!/usr/bin/env node
import fs from "node:fs";
import os from "node:os";
import { execFileSync } from "node:child_process";
const [url, port = "9222", initialScreenshot] = process.argv.slice(2);
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
    return { left: r.x, top: r.y, x: r.x + r.width / 2, y: r.y + r.height / 2 }; })()`,
  returnByValue: true,
});
const { left, top, x, y } = canvasState.result.value;
const pause = (ms = 300) => new Promise((resolve) => setTimeout(resolve, ms));
async function click(atX = x, atY = y, clickCount = 1) {
  for (const type of ["mousePressed", "mouseReleased"])
    await command("Input.dispatchMouseEvent", {
      type, x: atX, y: atY, button: "left", clickCount,
    });
}
async function drag(fromX, fromY, toX, toY) {
  await command("Input.dispatchMouseEvent", {
    type: "mousePressed", x: fromX, y: fromY, button: "left", clickCount: 1,
  });
  await pause(75);
  await command("Input.dispatchMouseEvent", {
    type: "mouseMoved", x: toX, y: toY, button: "left", buttons: 1,
  });
  await pause(75);
  await command("Input.dispatchMouseEvent", {
    type: "mouseReleased", x: toX, y: toY, button: "left", clickCount: 1,
  });
}
async function middleDrag(fromX, fromY, toX, toY) {
  await command("Input.dispatchMouseEvent", {
    type: "mousePressed", x: fromX, y: fromY, button: "middle", clickCount: 1,
  });
  await pause(75);
  await command("Input.dispatchMouseEvent", {
    type: "mouseMoved", x: toX, y: toY, button: "middle", buttons: 4,
  });
  await pause(75);
  await command("Input.dispatchMouseEvent", {
    type: "mouseReleased", x: toX, y: toY, button: "middle", clickCount: 1,
  });
}
async function key(key, code = key, text) {
  const params = { key, code, ...(text ? { text } : {}) };
  await command("Input.dispatchKeyEvent", { type: "keyDown", ...params });
  await command("Input.dispatchKeyEvent", { type: "keyUp", ...params });
}
async function shot() {
  await pause();
  if (process.env.NODE_GRAPH_X11_CAPTURE === "1") {
    const geometry = await command("Runtime.evaluate", {
      expression: `JSON.stringify({
        screenX, screenY, outerWidth, outerHeight, innerWidth, innerHeight,
        rect: document.querySelector("canvas").getBoundingClientRect().toJSON(),
      })`,
      returnByValue: true,
    });
    const { screenX, screenY, outerWidth, outerHeight, innerWidth, innerHeight, rect } =
      JSON.parse(geometry.result.value);
    const x = Math.round(screenX + (outerWidth - innerWidth) / 2 + rect.x);
    const y = Math.round(screenY + outerHeight - innerHeight + rect.y);
    const width = Math.round(rect.width);
    const height = Math.round(rect.height);
    const root = `${os.tmpdir()}/node-graph-root-${process.pid}.png`;
    execFileSync("import", ["-display", process.env.DISPLAY, "-window", "root", root]);
    const png = execFileSync("convert", [
      root, "-crop", `${width}x${height}+${x}+${y}`, "+repage", "png:-",
    ], { maxBuffer: 16 * 1024 * 1024 });
    fs.rmSync(root, { force: true });
    return png.toString("base64");
  }
  return (await command("Page.captureScreenshot", {
    format: "png", captureBeyondViewport: false,
  })).data;
}
function screenshotPath(suffix) {
  if (!initialScreenshot) return undefined;
  const dot = initialScreenshot.lastIndexOf(".");
  return dot < 0
    ? `${initialScreenshot}-${suffix}`
    : `${initialScreenshot.slice(0, dot)}-${suffix}${initialScreenshot.slice(dot)}`;
}
async function saveStateScreenshot(suffix) {
  const output = screenshotPath(suffix);
  if (output) fs.writeFileSync(output, Buffer.from(await shot(), "base64"));
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
if (initialScreenshot) {
  fs.writeFileSync(initialScreenshot, Buffer.from(await shot(), "base64"));
}
if (baseline.sourceWidth < 100) {
  throw new Error(`node width collapsed before interaction trace: ${JSON.stringify(baseline)}`);
}
await click(left + 100, top + 65);
await saveStateScreenshot("selected");
await click();
await command("Input.dispatchMouseEvent", {
  type: "mousePressed", x: left + 509, y: top + 91, button: "left", clickCount: 1,
});
await command("Input.dispatchMouseEvent", {
  type: "mouseReleased", x: left + 509, y: top + 91, button: "left", clickCount: 1,
});
await waitFor("world-space control activation", (value) => value.controlActivated);
await saveStateScreenshot("overlay");
await drag(left + 545, top + 121, left + 700, top + 121);
await waitFor("Mix range dragging", (value) => value.mixAmount > 0.85);
await key("Escape", "Escape");
await waitFor("overlay dismissal", (value) => value.overlayDismissed);
await click(left + 509, top + 91);
await waitFor("overlay reopening", (value) => !value.overlayDismissed);
await pause();
await click(left + 800, top + 200);
await waitFor("outside-click overlay dismissal", (value) => value.overlayDismissed);
await click();
await key("Tab", "Tab");
await pause();
await waitFor("catalog opening", (value) => value.catalogOpen);
await saveStateScreenshot("menu");
for (const character of "Math") await key(character, `Key${character.toUpperCase()}`, character);
await key("Enter", "Enter");
const created = await waitFor(
  "searched node creation",
  (value) => !value.catalogOpen && value.nodes === baseline.nodes + 1,
);
await click(left + 430, top + 91);
await waitFor("blend selection", (value) => value.lastControl.endsWith(":blend-select"));
await click(left + 420, top + 160);
const authored = await waitFor(
  "factor input",
  (value) => value.lastControl.endsWith(":factor-value") && value.worldLayout !== created.worldLayout,
);
await click(left + 195, top + 95);
await click(left + 344, top + 120);
await waitFor("click-to-connect ports", (value) => value.connections === baseline.connections + 1);
await drag(left + 195, top + 95, left + 750, top + 250);
await pause();
await waitFor(
  "draft connection catalog",
  (value) => value.catalogOpen && value.catalogDraft && value.catalogEntries >= 2,
);
for (const character of "Mix") await key(character, `Key${character.toUpperCase()}`, character);
await waitFor("draft catalog filtering", (value) => value.catalogEntries === 2);
await saveStateScreenshot("draft-menu");
await key("ArrowDown", "ArrowDown");
await key("Enter", "Enter");
await waitFor(
  "draft catalog create-and-connect",
  (value) => !value.catalogOpen
    && value.nodes === baseline.nodes + 2
    && value.connections === baseline.connections + 2,
);
await key("Escape", "Escape");
await pause();
const authoredBeforeZoom = await graphState();
await command("Input.dispatchMouseEvent", {
  type: "mouseWheel", x: left + 400, y: top + 100, deltaX: 0, deltaY: -180,
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
if (stable.worldLayout !== authoredBeforeZoom.worldLayout) {
  throw new Error(`world display list changed across zoom: ${JSON.stringify({ authoredBeforeZoom, stable })}`);
}
const screenPoint = (worldX, worldY, state = stable) => ({
  x: left + worldX * state.zoom + state.panX,
  y: top + worldY * state.zoom + state.panY,
});
const dragStart = screenPoint(stable.mixX + 70, stable.mixY + 15);
await drag(dragStart.x, dragStart.y, dragStart.x + stable.zoom * 20, dragStart.y + stable.zoom * 20);
const dragged = await waitFor(
  "inverse node drag",
  (value) => value.mixX - stable.mixX > 15
    && value.mixX - stable.mixX < 22
    && Math.abs((value.mixX - stable.mixX) - (value.mixY - stable.mixY)) < 0.5,
);
const resizeStart = screenPoint(dragged.mixX + dragged.mixWidth, dragged.mixY + 60, dragged);
await drag(resizeStart.x, resizeStart.y, resizeStart.x + dragged.zoom * 20, resizeStart.y);
const resized = await waitFor(
  "inverse node resize",
  (value) => value.mixWidth - dragged.mixWidth > 15
    && value.mixWidth - dragged.mixWidth < 22,
);
const marqueeStart = screenPoint(250, 35, resized);
const marqueeEnd = screenPoint(610, 240, resized);
await drag(marqueeStart.x, marqueeStart.y, marqueeEnd.x, marqueeEnd.y);
await waitFor("inverse marquee selection", (value) => value.selectedNodes > 0);
const beforeMiddlePan = await graphState();
await middleDrag(left + 900, top + 400, left + 925, top + 420);
await waitFor(
  "middle-button panning",
  (value) => value.panX - beforeMiddlePan.panX > 20
    && value.panY - beforeMiddlePan.panY > 15,
);
const transitions = {
  menuOpened: true,
  nodeCreated: true,
  clickConnected: true,
  draftMenuConnected: true,
  blendChanged: true,
  factorChanged: true,
  rangeDragged: true,
  worldControlActivated: true,
  overlayDismissed: true,
  overlayOutsideDismissed: true,
  viewportChanged: zoomed.zoom !== created.zoom,
  inverseDrag: true,
  inverseResize: true,
  inverseMarquee: true,
  middlePan: true,
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
