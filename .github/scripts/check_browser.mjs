#!/usr/bin/env node
import fs from "node:fs";
import os from "node:os";
import { execFileSync } from "node:child_process";
const [url, port = "9222", initialScreenshot] = process.argv.slice(2);
if (!url) throw new Error("usage: check_browser.mjs URL [DEBUG_PORT]");
const deadline = Date.now() + 60_000;
const softwareReadback = process.env.NODE_GRAPH_SOFTWARE_READBACK === "1";
let page;
while (Date.now() < deadline) {
  try {
    const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
    page = pages.find((entry) => entry.type === "page" && entry.url.startsWith(url));
    if (!page && softwareReadback) page = pages.find((entry) => entry.type === "page");
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
if (softwareReadback) {
  const source = `(() => {
    const contexts = new Set();
    const originalConfigure = GPUCanvasContext.prototype.configure;
    const originalGetCurrentTexture = GPUCanvasContext.prototype.getCurrentTexture;
    const originalSubmit = GPUQueue.prototype.submit;
    GPUCanvasContext.prototype.configure = function(config) {
      this.__gpuiDevice = config.device;
      this.__gpuiFormat = config.format;
      contexts.add(this);
      return originalConfigure.call(this, {
        ...config,
        usage: (config.usage || GPUTextureUsage.RENDER_ATTACHMENT) | GPUTextureUsage.COPY_SRC,
      });
    };
    GPUCanvasContext.prototype.getCurrentTexture = function() {
      const texture = originalGetCurrentTexture.call(this);
      this.__gpuiTexture = texture;
      return texture;
    };
    let sequence = 0;
    let painted = 0;
    let copying = false;
    GPUQueue.prototype.submit = function(commands) {
      const result = originalSubmit.call(this, commands);
      if (copying) return result;
      const context = [...contexts].find((candidate) => candidate.__gpuiDevice?.queue === this);
      const texture = context?.__gpuiTexture;
      const sourceCanvas = context?.canvas;
      if (!texture || !sourceCanvas?.width || !sourceCanvas?.height) return result;
      const current = ++sequence;
      globalThis.__gpuiSoftwareSequence = current;
      const width = sourceCanvas.width;
      const height = sourceCanvas.height;
      const bytesPerRow = Math.ceil(width * 4 / 256) * 256;
      try {
        const device = context.__gpuiDevice;
        const buffer = device.createBuffer({
          size: bytesPerRow * height,
          usage: GPUBufferUsage.COPY_DST | GPUBufferUsage.MAP_READ,
        });
        const encoder = device.createCommandEncoder();
        encoder.copyTextureToBuffer(
          { texture },
          { buffer, bytesPerRow, rowsPerImage: height },
          { width, height, depthOrArrayLayers: 1 },
        );
        copying = true;
        originalSubmit.call(this, [encoder.finish()]);
        copying = false;
        buffer.mapAsync(GPUMapMode.READ).then(() => {
          if (current < painted) { buffer.destroy(); return; }
          const raw = new Uint8Array(buffer.getMappedRange());
          const pixels = new Uint8ClampedArray(width * height * 4);
          const bgra = String(context.__gpuiFormat).startsWith("bgra");
          for (let y = 0; y < height; y++) {
            const row = raw.subarray(y * bytesPerRow, y * bytesPerRow + width * 4);
            pixels.set(row, y * width * 4);
          }
          if (bgra) {
            for (let offset = 0; offset < pixels.length; offset += 4) {
              const red = pixels[offset];
              pixels[offset] = pixels[offset + 2];
              pixels[offset + 2] = red;
            }
          }
          let mirror = document.getElementById("gpui-software-readback");
          if (!mirror) {
            mirror = document.createElement("canvas");
            mirror.id = "gpui-software-readback";
            Object.assign(mirror.style, {
              position: "fixed", pointerEvents: "none", zIndex: "2147483647",
            });
            document.documentElement.appendChild(mirror);
          }
          const rect = sourceCanvas.getBoundingClientRect();
          mirror.width = width;
          mirror.height = height;
          Object.assign(mirror.style, {
            left: rect.left + "px", top: rect.top + "px",
            width: rect.width + "px", height: rect.height + "px",
          });
          mirror.getContext("2d").putImageData(new ImageData(pixels, width, height), 0, 0);
          painted = current;
          globalThis.__gpuiSoftwareFrame = painted;
          buffer.destroy();
        }).catch(() => { copying = false; buffer.destroy(); });
      } catch (_) { copying = false; }
      return result;
    };
  })();`;
  await command("Page.addScriptToEvaluateOnNewDocument", { source });
  await command("Emulation.setDeviceMetricsOverride", {
    width: 1200,
    height: 661,
    deviceScaleFactor: 1,
    mobile: false,
  });
  await command("Page.navigate", { url });
}
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
async function rightClick(atX, atY) {
  for (const type of ["mousePressed", "mouseReleased"])
    await command("Input.dispatchMouseEvent", {
      type, x: atX, y: atY, button: "right",
      buttons: type === "mousePressed" ? 2 : 0, clickCount: 1,
    });
}
async function move(toX, toY) {
  await command("Input.dispatchMouseEvent", {
    type: "mouseMoved",
    x: toX,
    y: toY,
    button: "none",
  });
  await pause();
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
async function key(key, code = key, text, modifiers = 0) {
  const params = { key, code, modifiers, ...(text ? { text } : {}) };
  await command("Input.dispatchKeyEvent", { type: "keyDown", ...params });
  await command("Input.dispatchKeyEvent", { type: "keyUp", ...params });
}
let lastSoftwareFrame = 0;
async function shot() {
  await pause();
  if (softwareReadback) {
    const until = Date.now() + 8_000;
    let matched = false;
    while (Date.now() < until) {
      const status = await command("Runtime.evaluate", {
        expression: `[globalThis.__gpuiSoftwareFrame || 0, globalThis.__gpuiSoftwareSequence || 0]`,
        returnByValue: true,
      });
      const [frame, sequence] = status.result?.value || [0, 0];
      if (frame > lastSoftwareFrame && frame === sequence) {
        lastSoftwareFrame = frame;
        matched = true;
        break;
      }
      await pause(50);
    }
    if (!matched) throw new Error("software WebGPU readback did not reach the latest submitted frame");
  }
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
await move(left + 195, top + 95);
await waitFor("anchor tooltip", (value) => value.anchorTooltip);
await move(x, y);
await waitFor("anchor tooltip dismissal", (value) => !value.anchorTooltip);
await rightClick(left + 195, top + 95);
await waitFor("anchor menu opening", (value) => value.anchorMenu);
await pause();
await click(left + 260, top + 114);
await waitFor("anchor remove-connections action", (value) => !value.anchorMenu && value.connections === baseline.connections - 1 && !value.sourceConnected);
await pause();
await click(left + 195, top + 95);
await pause();
await click(left + 344, top + 140);
await waitFor("anchor connection recreation", (value) => value.connections === baseline.connections && value.sourceConnected);
await rightClick(left + 195, top + 95);
await waitFor("anchor menu reopening", (value) => value.anchorMenu);
await key("Escape", "Escape");
await waitFor("anchor menu Escape dismissal", (value) => !value.anchorMenu);
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
await key("Tab", "Tab");
await waitFor(
  "range keyboard focus",
  (value) => value.lastControl.endsWith(":mix-range"),
);
await key("Home", "Home");
await waitFor("range keyboard Home", (value) => value.mixAmount === 0);
await key("ArrowRight", "ArrowRight");
await waitFor("range keyboard step", (value) => Math.abs(value.mixAmount - 0.01) < 0.001);
await drag(left + 545, top + 121, left + 700, top + 121);
await waitFor("Mix range dragging", (value) => value.mixAmount > 0.85);
await key("Escape", "Escape");
await waitFor("overlay dismissal", (value) => value.overlayDismissed);
await click(left + 509, top + 91);
await waitFor("overlay reopening", (value) => !value.overlayDismissed);
await pause();
await click(left + 430, top + 91);
await waitFor(
  "click-through overlay dismissal",
  (value) => value.overlayDismissed
    && value.selectOpen
    && value.lastControl.endsWith(":blend-select"),
);
await key("Escape", "Escape");
await waitFor("click-through target cleanup", (value) => !value.selectOpen);
await click();
await key("Tab", "Tab");
await pause();
await waitFor("catalog opening", (value) => value.catalogOpen);
await key("Tab", "Tab");
await waitFor("catalog Tab cancellation", (value) => !value.catalogOpen);
await key("Tab", "Tab");
await pause();
await waitFor("catalog reopening", (value) => value.catalogOpen);
await saveStateScreenshot("menu");
for (const character of "Math") await key(character, `Key${character.toUpperCase()}`, character);
await key("Enter", "Enter");
const created = await waitFor(
  "searched node creation",
  (value) => !value.catalogOpen && value.nodes === baseline.nodes + 1,
);
await click(left + 430, top + 91);
await waitFor(
  "blend dropdown opening",
  (value) => value.lastControl.endsWith(":blend-select") && value.selectOpen,
);
await pause();
await click(left + 420, top + 160);
await waitFor(
  "blend option selection",
  (value) => value.blend === "Screen" && !value.selectOpen,
);
await click(left + 420, top + 160);
await waitFor("factor focus", (value) => value.lastControl.endsWith(":factor-value"));
await key("a", "KeyA", undefined, 2);
for (const [character, code] of [["0", "Digit0"], [".", "Period"], ["7", "Digit7"], ["5", "Digit5"]]) {
  await key(character, code, character);
}
await key("ArrowLeft", "ArrowLeft");
await key("ArrowLeft", "ArrowLeft", undefined, 8);
await key("9", "Digit9", "9");
await key("Escape", "Escape");
await waitFor("native Factor Escape preservation", (value) => value.factorText === "0.95");
await key("a", "KeyA", undefined, 2);
for (const [character, code] of [["0", "Digit0"], [".", "Period"], ["7", "Digit7"], ["5", "Digit5"]]) {
  await key(character, code, character);
}
await key("Enter", "Enter");
await command("Runtime.evaluate", {
  expression: `(() => { const input = document.querySelector("input");
    input.dispatchEvent(new CompositionEvent("compositionstart", { bubbles: true, data: "" }));
    input.dispatchEvent(new CompositionEvent("compositionupdate", { bubbles: true, data: "漢" }));
    input.dispatchEvent(new CompositionEvent("compositionend", { bubbles: true, data: "漢" }));
  })()`,
});
await waitFor("Factor IME composition", (value) => value.factorText === "0.75漢");
await key("a", "KeyA", undefined, 2);
for (const [character, code] of [["0", "Digit0"], [".", "Period"], ["7", "Digit7"], ["5", "Digit5"]]) {
  await key(character, code, character);
}
await key("Tab", "Tab", undefined, 8);
await waitFor(
  "reverse control focus traversal",
  (value) => value.lastControl.endsWith(":mix-amount"),
);
await key("Tab", "Tab");
await waitFor(
  "forward control focus traversal",
  (value) => value.lastControl.endsWith(":factor-value"),
);
const authored = await waitFor(
  "factor text editing",
  (value) => value.factorText === "0.75" && value.worldLayout !== created.worldLayout,
);
await click(left + 195, top + 95);
await click(left + 344, top + 120);
await waitFor("click-to-connect ports", (value) => value.connections === baseline.connections + 1);
await click(left + 195, top + 95);
await key("Tab", "Tab");
await pause();
await waitFor(
  "draft connection catalog",
  (value) => value.catalogOpen && value.catalogDraft && value.catalogEntries >= 2,
);
for (const character of "Mix") await key(character, `Key${character.toUpperCase()}`, character);
await waitFor(
  "draft catalog filtering",
  (value) => value.catalogEntries === 2 && value.catalogSelected === 0,
);
await saveStateScreenshot("draft-menu");
await move(left + 800, top + 450);
await waitFor("draft pin mouse selection", (value) => value.catalogSelected === 1);
await move(left + 800, top + 425);
await waitFor("draft pin mouse reselection", (value) => value.catalogSelected === 0);
await key("ArrowDown", "ArrowDown");
await waitFor("draft pin keyboard selection", (value) => value.catalogSelected === 1);
await key("Enter", "Enter");
await waitFor(
  "draft catalog create-and-connect",
  (value) => !value.catalogOpen
    && value.nodes === baseline.nodes + 2
    && value.connections === baseline.connections + 2,
);
await key("Escape", "Escape");
await pause();
await click(left + 900, top + 100);
await key("Tab", "Tab");
await pause();
await waitFor("custom catalog opening", (value) => value.catalogOpen);
for (const character of "Custom") await key(character, `Key${character.toUpperCase()}`, character);
await key("Enter", "Enter");
await waitFor(
  "custom node creation",
  (value) => !value.catalogOpen && value.nodes === baseline.nodes + 3 && value.customInputs === 2,
);
const beforeCustomSelect = await graphState();
await click(left + 870, top + 469);
await waitFor(
  "custom count dropdown opening",
  (value) => value.selectOpen && value.worldLayout === beforeCustomSelect.worldLayout,
);
await pause();
await click(left + 870, top + 580);
await waitFor(
  "custom count option selection",
  (value) => !value.selectOpen && value.customInputs === 4,
);
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
const dropped = await command("Runtime.evaluate", {
  expression: `globalThis.__nodeGraphTestDrop?.("output", ${left + 1000}, ${top + 550})`,
  returnByValue: true,
});
if (!dropped.result.value) throw new Error("cross-pane drop handle rejected a laid-out catalog item");
await waitFor("cross-pane drop node creation", (value) => value.nodes === baseline.nodes + 4);

const transitions = {
  menuOpened: true,
  nodeCreated: true,
  clickConnected: true,
  draftMenuConnected: true,
  blendChanged: true,
  factorChanged: true,
  factorIme: true,
  controlFocusTraversal: true,
  customSelectChanged: true,
  rangeDragged: true,
  rangeKeyboard: true,
  worldControlActivated: true,
  anchorTooltip: true,
  anchorMenu: true,
  overlayDismissed: true,
  overlayOutsideDismissed: true,
  viewportChanged: zoomed.zoom !== created.zoom,
  inverseDrag: true,
  inverseResize: true,
  inverseMarquee: true,
  middlePan: true,
  crossPaneDrop: true,
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
