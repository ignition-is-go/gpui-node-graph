# /// script
# requires-python = ">=3.11"
# dependencies = ["playwright==1.56.0", "pillow"]
# ///
"""Render the shared fixture in the real leptos-node-graph 0.2.2 crate and in
the GPUI gallery, replay the same input on both in headless Chromium, and
report geometry and pixel differences per interaction state.

Wires are compared as screen-space polylines: the reference's routed SVG path
(corner control points are the vertices) against GPUI's tagged route vertices,
by their endpoints and by the largest distance between points sampled at equal
arc-length steps along each.

    (cd parity/gallery && trunk build)
    (cd parity/reference && RUSTFLAGS=--cfg=parity_reference trunk build)
    uv run parity/capture.py [--theme scene|default] [state ...]

The reference build sets RUSTFLAGS so it does not inherit the repo's wasm
thread flags. Nothing opens on a display. Writes parity/out/<theme>-<state>.png
(reference | gpui | diff x4) and prints, per state, the largest socket-centre
and node-card offsets, the wire endpoint and path offsets, and the pixel
difference, plus the worst node card's mean |d| and the worst selection-ring
band's mean |d|. The overlap states put the upper card over the lower card's
output socket: the reference's elementsFromPoint there must name the expected
top element, and the socket must be drawn, or hidden, as in the reference (see
SOCKET_BOX). Exits non-zero when, in a geometry-matched state, a card exceeds
CARD_LIMIT or a ring band exceeds RING_LIMIT, or an overlap socket disagrees. PARITY_GALLERY_DIST
points it at another gallery build, such as a mutated one.
"""

import argparse
import functools
import glob
import http.server
import json
import os
import socketserver
import threading
from collections import Counter
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageStat
from playwright.sync_api import sync_playwright

HERE = Path(__file__).resolve().parent
OUT = HERE / "out"
W, H = 960, 400

# Port id by label, node ids in render order and connection ids, per fixture in
# parity/fixture.rs (labels are unique within one). `overlap` is two cards
# whose upper one covers the lower one's output socket.
FIXTURES = {
    "base": {
        "query": "",
        "ports": {
            "Value": "src:a_value",
            "Trigger": "src:b_trigger",
            "A": "calc:a",
            "B": "calc:b",
            "Out": "calc:out",
            "In": "sink:in",
        },
        "nodes": ["src", "calc", "sink"],
        "connections": ["c1", "c2"],
    },
    "overlap": {
        "query": "&overlap",
        "ports": {
            "In A": "ova:in",
            "Out A": "ova:out",
            "In B": "ovb:in",
            "Out B": "ovb:out",
        },
        "nodes": ["ova", "ovb"],
        "connections": [],
    },
}
SAMPLES = 64
# Half-width of the square around a socket centre, inside the 8px dot and clear
# of its label. In the overlap states the covered socket is gated on its ink: the
# pixels there that differ from the card fill. A socket drawn on the wrong side of
# a card has its ring in one rendering and not the other, which the mean |d| of
# the box cannot tell from antialiasing (a matching ring measures about 7).
SOCKET_BOX = 5
# Matching renderings measure 0.55 (ring antialiasing) to 1.0; one side missing, 0.
INK_RATIO = 0.4
CARD_MARGIN = 6
# Largest mean |d| a node card may show before its state counts as styled
# differently: matching states measure at most 4.74 (text antialiasing), a
# dragged card with a filled shadow under it 9.41.
CARD_LIMIT = 6.0
# The selection ring band, card edge -RING_INSET .. +RING_OUTSET px, where a
# 1px ring and its antialiasing land. The whole-card mean dilutes a 1px ring
# shift or a half-pixel width error below CARD_LIMIT; this band does not.
# Matching states measure at most 2.36 (corner and port-dot antialiasing).
# Builds with the ring shifted 1px measured 12.2 (default) and 21.7 (scene),
# with the 1.5px outline left unfloored 10.4 and 41.2, and a dragged card
# left translucent after release 6.62.
RING_INSET, RING_OUTSET = 2, 3
RING_LIMIT = 4.0
# Geometry is not expected to match here, so its cards are reported only.
CARD_UNGATED = {"zoomed"}


def header(g, node):
    x, y, w, _ = g["nodes"][node]
    return (x + w / 2, y + 12)


def dot(g, port):
    return g["ports"][port]


# A state is a fixture and a list of steps; each step is a function of the
# reference's idle geometry so both renderings receive the same viewport
# coordinates.
BASE = [
    ("idle", []),
    ("hover-node", [lambda g: ("move", *header(g, "calc"))]),
    ("selected", [lambda g: ("click", *header(g, "calc"))]),
    ("hover-port", [lambda g: ("move", *dot(g, "src:a_value"))]),
    ("draft", [
        lambda g: ("down", *dot(g, "src:b_trigger")),
        lambda g: ("move", 520, 330),
    ]),
    ("dragging", [
        lambda g: ("down", *header(g, "calc")),
        lambda g: ("move", header(g, "calc")[0] + 40, header(g, "calc")[1] + 60),
    ]),
    ("dragged", [
        lambda g: ("down", *header(g, "calc")),
        lambda g: ("move", header(g, "calc")[0] + 40, header(g, "calc")[1] + 60),
        lambda g: ("up",),
    ]),
    ("zoomed", [lambda g: ("move", 400, 200), lambda g: ("wheel", 0, -100)]),
]
# The lower card's output socket lies under the upper card. In 0.2.2 the dot is
# `position: relative; z-index: 1` in a card that is no stacking context, so at
# rest it paints above the upper card and takes the press. A dragged card has
# opacity < 1, which makes it a stacking context holding its own dot: dragging
# the lower card puts its socket under the upper one, dragging the upper card
# leaves it on top. `top` is what the reference's elementsFromPoint must answer
# at that socket first, so a changed crate cannot silently move the premise.
OVERLAP = [
    ("overlap", [], "dot"),
    ("overlap-press", [
        lambda g: ("down", *dot(g, "ova:out")),
        lambda g: ("move", 700, 330),
    ], "dot"),
    ("overlap-drag-lower", [
        lambda g: ("down", *header(g, "ova")),
        lambda g: ("move", header(g, "ova")[0] + 3, header(g, "ova")[1] + 3),
    ], "card"),
    ("overlap-drag-upper", [
        lambda g: ("down", *header(g, "ovb")),
        lambda g: ("move", header(g, "ovb")[0] + 3, header(g, "ovb")[1] + 3),
    ], "dot"),
]
STATES = {name: ("base", steps, None) for name, steps in BASE}
STATES.update({name: ("overlap", steps, top) for name, steps, top in OVERLAP})
HIT_PORT = "ova:out"


def serve(directory: Path) -> int:
    class Isolated(http.server.SimpleHTTPRequestHandler):
        def end_headers(self):
            self.send_header("Cross-Origin-Opener-Policy", "same-origin")
            self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
            super().end_headers()

        def log_message(self, *args):
            pass

    handler = functools.partial(Isolated, directory=str(directory))
    server = socketserver.ThreadingTCPServer(("127.0.0.1", 0), handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server.server_address[1]


def replay(page, steps, geometry):
    for step in steps:
        kind, *args = step(geometry)
        if kind == "move":
            page.mouse.move(*args, steps=8)
        elif kind == "click":
            page.mouse.click(*args)
        elif kind == "down":
            page.mouse.move(*args)
            page.mouse.down()
        elif kind == "up":
            page.mouse.up()
        elif kind == "wheel":
            page.mouse.wheel(*args)
        page.wait_for_timeout(150)


REFERENCE_GEOMETRY = r"""() => {
  const center = (r) => [r.x + r.width / 2, r.y + r.height / 2];
  const nodes = {}, ports = {}, headers = {};
  const ids = %s;
  document.querySelectorAll('[data-node]').forEach((el, i) => {
    const r = el.getBoundingClientRect();
    nodes[ids[i]] = [r.x, r.y, r.width, r.height];
    const h = el.querySelector('[data-node-header]').getBoundingClientRect();
    headers[ids[i]] = [h.x, h.y, h.width, h.height];
  });
  const labels = %s;
  document.querySelectorAll('[data-anchor]').forEach((row) => {
    const port = labels[row.textContent.trim()];
    const d = row.querySelector('[data-anchor-dot]');
    if (port && d) ports[port] = center(d.getBoundingClientRect());
  });
  // A routed wire is `M` then `L` runs joined by `Q` corners whose control
  // point is the polyline vertex; keep M, each control point, and the end.
  // `<For>` renders connections in id order; a dangling one has no attribute.
  const wires = {};
  const conns = %s;
  document.querySelectorAll('path[data-connection]').forEach((path, i) => {
    const m = path.getScreenCTM();
    const nums = (path.getAttribute('d').match(/[MLQ][^MLQ]*/g) || []).map((seg) =>
      [seg[0], seg.slice(1).trim().split(/[\s,]+/).map(Number)]);
    const pts = [];
    nums.forEach(([cmd, n], j) => {
      if (cmd === 'M' || cmd === 'Q') pts.push([n[0], n[1]]);
      if (j === nums.length - 1) pts.push(n.slice(-2));
    });
    wires[conns[i]] = pts.map(([x, y]) => {
      const p = new DOMPoint(x, y).matrixTransform(m);
      return [p.x, p.y];
    });
  });
  // What the browser gives the press at every socket centre, topmost first:
  // the dot's own elements, then the owning card or whatever covers the dot.
  const hits = {};
  Object.entries(ports).forEach(([port, [x, y]]) => {
    hits[port] = document.elementsFromPoint(x, y).map((el) => {
      if (el.hasAttribute('data-anchor-dot') || el.closest('[data-anchor-dot]')) return 'dot';
      const card = el.closest('[data-node]');
      return card ? 'card:' + ids[[...document.querySelectorAll('[data-node]')].indexOf(card)] : 'other';
    }).filter((kind, i, all) => i === 0 || kind !== all[i - 1]);
  });
  return {nodes, ports, headers, wires, hits};
}"""

def reference_geometry(fixture):
    return REFERENCE_GEOMETRY % (json.dumps(fixture["nodes"]), json.dumps(fixture["ports"]),
                                 json.dumps(fixture["connections"]))


def gpui_geometry(page):
    parts = json.loads(page.evaluate("() => globalThis.__nodeGraphParts()"))
    nodes, ports, headers, points = {}, {}, {}, {}
    for key, (x, y, w, h) in parts.items():
        kind, _, ident = key.removeprefix("node-graph.").partition(":")
        if kind == "node":
            nodes[ident] = [x, y, w, h]
        elif kind == "node-header":
            headers[ident] = [x, y, w, h]
        elif kind == "port":
            ports[ident] = [x + w / 2, y + h / 2]
        elif kind == "wire-point":
            wire, _, index = ident.rpartition(".")
            points.setdefault(wire, {})[int(index)] = [x, y]
    wires = {wire: [p for _, p in sorted(vs.items())] for wire, vs in points.items()}
    return {"nodes": nodes, "ports": ports, "headers": headers, "wires": wires}


def capture(browser, url, steps, geometry, gpui, name, fixture):
    page = browser.new_page(viewport={"width": W, "height": H}, device_scale_factor=1)
    page.goto(url)
    if gpui:
        page.wait_for_selector("canvas")
        page.wait_for_function("() => typeof globalThis.__nodeGraphParts === 'function'")
        page.wait_for_timeout(1500)
    else:
        page.wait_for_selector("body[data-ready]")
        page.wait_for_timeout(300)
    page.mouse.move(W - 1, H - 1)
    replay(page, steps, geometry)
    page.wait_for_timeout(400 if gpui else 100)
    shot = OUT / f"tmp-{name}.png"
    page.screenshot(path=str(shot))
    measured = gpui_geometry(page) if gpui else page.evaluate(reference_geometry(FIXTURES[fixture]))
    page.close()
    return Image.open(shot).convert("RGB"), measured


def pixel_diff(ref, gpui):
    diff = ImageChops.difference(ref, gpui)
    r, g, b = diff.split()
    peak = ImageChops.lighter(ImageChops.lighter(r, g), b)
    hist = diff.histogram()
    mean = sum(i * hist[c * 256 + i] for c in range(3) for i in range(256)) / (W * H * 3)
    bad = peak.point(lambda v: 255 if v > 32 else 0).histogram()[255] / (W * H)
    return mean, bad, diff


def card_diffs(ref_img, gpui_img, geometry):
    """Mean |difference| per node card, over the reference card rect grown by
    CARD_MARGIN so the selection ring and glow count. A fill or ring that one
    side has and the other lacks moves this; text antialiasing barely does."""
    out = {}
    for node, (x, y, w, h) in geometry["nodes"].items():
        box = (max(0, int(x - CARD_MARGIN)), max(0, int(y - CARD_MARGIN)),
               min(W, int(x + w + CARD_MARGIN + 1)), min(H, int(y + h + CARD_MARGIN + 1)))
        diff = ImageChops.difference(ref_img.crop(box), gpui_img.crop(box))
        hist = diff.histogram()
        area = (box[2] - box[0]) * (box[3] - box[1]) * 3
        out[node] = sum(i * hist[c * 256 + i] for c in range(3) for i in range(256)) / area
    return out


def ring_diffs(ref_img, gpui_img, geometry):
    """Mean |difference| per node over the selection ring band only."""
    out = {}
    for node, (x, y, w, h) in geometry["nodes"].items():
        mask = Image.new("L", (W, H), 0)
        draw = ImageDraw.Draw(mask)
        draw.rectangle((round(x) - RING_OUTSET, round(y) - RING_OUTSET,
                        round(x + w) + RING_OUTSET - 1, round(y + h) + RING_OUTSET - 1), fill=255)
        draw.rectangle((round(x) + RING_INSET, round(y) + RING_INSET,
                        round(x + w) - RING_INSET - 1, round(y + h) - RING_INSET - 1), fill=0)
        diff = ImageChops.difference(ref_img, gpui_img)
        out[node] = sum(ImageStat.Stat(diff, mask).mean) / 3
    return out


def socket_ink(img, port_xy):
    """Pixels in the box around a socket centre that differ from the box's
    commonest colour, which is the card fill under the dot."""
    x, y = port_xy
    box = img.crop((int(x - SOCKET_BOX), int(y - SOCKET_BOX), int(x + SOCKET_BOX), int(y + SOCKET_BOX)))
    pixels = list(box.get_flattened_data())
    fill = Counter(pixels).most_common(1)[0][0]
    return sum(1 for px in pixels if max(abs(c - f) for c, f in zip(px, fill)) > 32)


def max_offset(ref, gpui, key, width):
    worst, where = 0.0, "-"
    for ident, a in ref[key].items():
        b = gpui[key].get(ident)
        if b is None:
            return float("inf"), f"{ident} missing"
        delta = max(abs(p - q) for p, q in zip(a[:width], b[:width]))
        if delta > worst:
            worst, where = delta, ident
    return worst, where


def resample(points, n=SAMPLES):
    """`n` points at equal arc-length steps along a polyline."""
    lengths = [((b[0] - a[0]) ** 2 + (b[1] - a[1]) ** 2) ** 0.5 for a, b in zip(points, points[1:])]
    total = sum(lengths)
    if total == 0:
        return [points[0]] * n
    out, seg, walked = [], 0, 0.0
    for k in range(n):
        target = total * k / (n - 1)
        while seg < len(lengths) - 1 and walked + lengths[seg] < target:
            walked += lengths[seg]
            seg += 1
        t = 0 if lengths[seg] == 0 else min(1.0, (target - walked) / lengths[seg])
        a, b = points[seg], points[seg + 1]
        out.append([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t])
    return out


def wire_offsets(ref, gpui):
    """Largest endpoint and along-path distance between matching wires."""
    worst_end, end_at, worst_path, path_at = 0.0, "-", 0.0, "-"
    for wire, a in ref["wires"].items():
        b = gpui["wires"].get(wire)
        if not b or len(a) < 2:
            return (float("inf"), f"{wire} missing"), (float("inf"), f"{wire} missing")
        dist = lambda p, q: ((p[0] - q[0]) ** 2 + (p[1] - q[1]) ** 2) ** 0.5
        end = max(dist(a[0], b[0]), dist(a[-1], b[-1]))
        path = max(dist(p, q) for p, q in zip(resample(a), resample(b)))
        if end > worst_end:
            worst_end, end_at = end, wire
        if path > worst_path:
            worst_path, path_at = path, wire
    if set(gpui["wires"]) - set(ref["wires"]):
        extra = sorted(set(gpui["wires"]) - set(ref["wires"]))[0]
        return (float("inf"), f"{extra} extra"), (float("inf"), f"{extra} extra")
    return (worst_end, end_at), (worst_path, path_at)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--theme", default="scene", choices=["scene", "default"])
    parser.add_argument("states", nargs="*")
    args = parser.parse_args()
    OUT.mkdir(exist_ok=True)
    ref_port = serve(HERE / "reference" / "dist")
    gpui_port = serve(Path(os.environ.get("PARITY_GALLERY_DIST") or HERE / "gallery" / "dist"))
    rows, hits = [], []
    with sync_playwright() as p:
        shells = sorted(glob.glob(os.path.expanduser(
            "~/.cache/ms-playwright/chromium_headless_shell-*/chrome-*/chrome-headless-shell")))
        browser = p.chromium.launch(
            executable_path=os.environ.get("PARITY_CHROMIUM") or (shells[-1] if shells else None),
            args=["--use-angle=swiftshader", "--enable-unsafe-swiftshader",
                  "--enable-unsafe-webgpu", "--enable-features=Vulkan"],
        )
        idles = {}
        for name, (fixture, steps, top) in STATES.items():
            if args.states and name not in args.states:
                continue
            query = f"?theme={args.theme}{FIXTURES[fixture]['query']}"
            ref_url = f"http://127.0.0.1:{ref_port}/index.html{query}"
            if fixture not in idles:
                _, idles[fixture] = capture(browser, ref_url, [], None, False, "ref", fixture)
            idle = idles[fixture]
            ref_img, ref_geo = capture(browser, ref_url, steps, idle, False, "ref", fixture)
            gpui_img, gpui_geo = capture(browser, f"http://127.0.0.1:{gpui_port}/index.html{query}",
                                         steps, idle, True, "gpui", fixture)
            mean, bad, diff = pixel_diff(ref_img, gpui_img)
            sheet = Image.new("RGB", (W, H * 3))
            sheet.paste(ref_img, (0, 0))
            sheet.paste(gpui_img, (0, H))
            sheet.paste(diff.point(lambda v: min(255, v * 4)), (0, H * 2))
            sheet.save(OUT / f"{args.theme}-{name}.png")
            (OUT / f"{args.theme}-{name}.json").write_text(
                json.dumps({"reference": ref_geo, "gpui": gpui_geo}, indent=1))
            cards = card_diffs(ref_img, gpui_img, ref_geo)
            worst_card = max(cards.items(), key=lambda kv: kv[1])
            worst_ring = max(ring_diffs(ref_img, gpui_img, ref_geo).items(), key=lambda kv: kv[1])
            ink = (0, 0)
            if top:
                ink = (socket_ink(ref_img, ref_geo["ports"][HIT_PORT]),
                       socket_ink(gpui_img, ref_geo["ports"][HIT_PORT]))
            if top:
                hits.append((name, top, ref_geo["hits"][HIT_PORT]))
            rows.append((name, max_offset(ref_geo, gpui_geo, "ports", 2),
                         max_offset(ref_geo, gpui_geo, "nodes", 4),
                         *wire_offsets(ref_geo, gpui_geo), worst_card, worst_ring, ink,
                         mean, bad))
        browser.close()
    print(f"theme {args.theme}")
    print(f"{'state':20} {'socket max px':>18} {'card max px':>18} {'wire end px':>14}"
          f" {'wire path px':>14} {'card |d|':>13} {'ring |d|':>13} {'dot ink r/g':>13} {'mean|d|':>8} {'px>32':>7}")
    failed = []
    for name, *offsets, (card_at, card), (ring_at, ring), (ink_ref, ink_gpui), mean, bad in rows:
        cells = " ".join(f"{v:9.2f} {at:>{8 if i < 2 else 4}}" for i, (v, at) in enumerate(offsets))
        gated = name not in CARD_UNGATED
        if gated and card > CARD_LIMIT:
            failed.append(f"{name} card {card_at} {card:.2f} > {CARD_LIMIT}")
        if gated and ring > RING_LIMIT:
            failed.append(f"{name} ring {ring_at} {ring:.2f} > {RING_LIMIT}")
        if STATES[name][2] and min(ink_ref, ink_gpui) < INK_RATIO * max(ink_ref, ink_gpui):
            failed.append(f"{name} socket {HIT_PORT} ink {ink_ref} vs {ink_gpui}")
        mark = "" if gated else "~"
        print(f"{name:20} {cells} {card:7.2f}{mark:1} {card_at:>5} {ring:7.2f}{mark:1} {ring_at:>5}"
              f" {ink_ref:5d}/{ink_gpui:<5d} {mean:8.2f} {bad:7.2%}")
    for name, top, kinds in hits:
        print(f"reference elementsFromPoint at {HIT_PORT} in {name}: {' > '.join(kinds)}")
        if kinds[0].split(':')[0] != top:
            failed.append(f"{name} reference top is {kinds[0]}, expected {top}")
    if failed:
        raise SystemExit("card styling differs: " + ", ".join(failed))


if __name__ == "__main__":
    main()
