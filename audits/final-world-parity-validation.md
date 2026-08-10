# Final world-space and Leptos-demo parity validation

Validated implementation base: `5242a86` (this report is a documentation-only follow-up).
Reference: `leptos-node-graph@87658950fccfeeea123285c706820ffea4ab55d1`.

## Rendering contract

- `WorldNodeBodyRenderer` receives `WorldNodeVisualState`, which intentionally has no zoom or pan. It authors fixed world primitives and explicit `TextLines`.
- The paint adapter projects the completed scene with `screen = world * zoom + pan`; hit regions use the inverse transform.
- The transformed plane contains groups, wires, nodes, sockets, labels and controls. Marquee, catalog, overlay/backdrop, and overlay content remain in pane pixels.
- Overlay node offsets project with zoom while their screen gap and content size do not.
- The reference zoom range, exponential wheel step, pointer anchoring, fit padding and fit ceiling are covered in core/world tests and the browser trace.

## Fixture and visual evidence

The shared native/WASM demo starts with the audited Leptos fixture: `color_source_0` at `50,50` (`160x79`), `mix_1` at `330,50` (`202x124`), `conn_1` from Color to B, and `group_0` (`Group 1`, `#8b5cf6`). Catalog IDs, categories, descriptions, ports, compatibility, dynamic Custom counts, Blend values and Mix amount/factor controls use the reference vocabulary.

Audited 1200px goldens cover the initial, selected-node, Mix-overlay, and catalog states. On local Mesa/X11 the measurements are:

| frame | MAE | exact pixels | pixels delta > 20 |
|---|---:|---:|---:|
| initial | 0.5266 | 91.4585% | 0.7294% |
| selected | 0.7198 | 89.8572% | 0.8321% |
| overlay | 0.7942 | 89.1893% | 0.9768% |
| menu | 1.6906 | 85.0250% | 2.3020% |

The remaining differing pixels are principally GPUI-versus-DOM glyph/subpixel and shadow rasterization. Geometry, palette, fixture placement, route, controls, overlay anchor and menu placement are aligned. CI installs official Google Chrome because the generic hosted Chromium build can omit direct-present WebGPU surfaces from capture. A background-only/missing surface is an explicit failure (not a visual-pass escape hatch), dimensions must match exactly, and every frame is retained as an artifact. Functional state/fingerprint checks always run as well.

## Stateful interaction evidence

`.github/scripts/check_browser.mjs` sustains the real WASM app and verifies:

- selected frame and blank-canvas clearing;
- Mix overlay open, Escape dismiss, reopen, and outside-click dismiss;
- catalog open/search/keyboard creation and menu visual state;
- click-to-connect source/target ports;
- Blend and Factor mutations plus Mix overlay control activation;
- pointer-anchored fractional zoom with an unchanged authored world layout;
- post-zoom inverse node dragging, right-edge resizing, and marquee selection;
- stable fixture dimensions across retained frames.

Unit coverage additionally exercises all reference zoom levels (`0.1`, `0.740818`, `1`, `1.349859`, `2`, `5`), fractional pan, inverse hit agreement, polygon/socket projection, multiple compatible catalog pins, group last-member removal, deterministic routing, strict snapshot validation/canonicalization, controlled atomic mutations, and transient dynamic-port tombstones.

## Validation commands

The acceptance run used `cargo fmt --check`, workspace tests, strict workspace/all-target Clippy, native checks, `wasm32-unknown-unknown` checks, release Trunk build, the stateful X11/Vulkan browser trace, four golden comparisons, and `git diff --check`. CI repeats Linux browser/WASM validation plus Linux/macOS/Windows native checks using the official GPUI pin.
