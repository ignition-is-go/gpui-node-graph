# Final world-space and Leptos-demo parity validation

Validated implementation base: `09470228f04a2e002bdffb5e1bbda650e9f77e4f`.
Green cross-platform CI: https://github.com/ignition-is-go/gpui-node-graph/actions/runs/31357521105.
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
| initial | 0.4889 | 92.4369% | 0.6864% |
| selected | 0.4966 | 92.4686% | 0.6886% |
| overlay | 0.8242 | 90.0091% | 0.9919% |
| menu | 1.4827 | 87.1949% | 1.9741% |

The remaining differing pixels are principally GPUI-versus-DOM glyph/subpixel and shadow rasterization. Geometry, palette, fixture placement, route, controls, overlay anchor and menu placement are aligned. CI installs official Google Chrome because the generic hosted Chromium build can omit direct-present WebGPU surfaces from capture. A background-only/missing surface is an explicit failure (not a visual-pass escape hatch), dimensions must match exactly, and every frame is retained as an artifact. Functional state/fingerprint checks always run as well.

## Stateful interaction evidence

`.github/scripts/check_browser.mjs` sustains the real WASM app and verifies:

- selected frame and blank-canvas clearing;
- Mix overlay open, Escape dismiss, reopen, and outside-click dismiss;
- catalog open/search/keyboard creation and menu visual state;
- click-to-connect source/target ports;
- retained pane-space Blend and Custom option selection by mouse and keyboard;
- editable Factor text with pointer caret, Shift selection, platform text replacement, real composition/IME, native Enter/Escape behavior, and control Tab order;
- Mix range mouse dragging plus focused Home and Arrow keyboard behavior;
- port hover tooltips, default right-click connection removal/recreation, menu Escape dismissal, and live connected/draft visual state;
- measured semantic-control overlay anchoring and click-through dismissal;
- typed `NodeDrop` creation through `EditorHandle::drop_node`, including live client↔canvas conversion and catalog-backed cross-pane creation;
- pointer-anchored fractional zoom with an unchanged authored world layout;
- post-zoom inverse node dragging, right-edge resizing, and marquee selection;
- stable fixture dimensions across retained frames.

Overlay placement now measures retained panel bounds after layout, supports Top/Right/Bottom/Left with Start/Center/End alignment through `OverlayPlacement`, flips only when the opposite side fits, and clamps to the pane. The demo anchors by the trigger's stable world-control ID (not a duplicated screen gap); its trigger remains in immutable world space while its dropdown and overlay content remain unscaled in pane space. Outside dismissal runs before the underlying world interaction, so the dismissing click still activates its intended target.

Unit coverage additionally exercises all-side overlay placement plus semantic anchor extent/gap at multiple zooms, UTF-16/surrogate selection, copy/cut ranges, and marked-text replacement, live/broken anchor menus, client↔canvas conversion, every dot silhouette, runtime style resolution, and all reference zoom levels (`0.1`, `0.740818`, `1`, `1.349859`, `2`, `5`), fractional pan, inverse hit agreement, polygon/socket projection, multiple compatible catalog pins, group last-member removal, deterministic routing, strict snapshot validation/canonicalization, controlled atomic mutations, and transient dynamic-port tombstones.

## Validation commands

The acceptance run used `cargo fmt --check`, workspace tests, strict workspace/all-target Clippy, native checks, `wasm32-unknown-unknown` checks, release Trunk build, the stateful X11/Vulkan browser trace, four golden comparisons, and `git diff --check`. CI repeats Linux browser/WASM validation plus Linux/macOS/Windows native checks using the official GPUI pin.

Hosted Linux uses Dawn SwiftShader and an early CDP-injected readback mirror. The hook adds `COPY_SRC` to GPUI's actual WebGPU canvas configuration, copies each presented texture through a mapped GPU buffer, and displays those exact bytes in a screenshot-visible Canvas2D plane. This preserves strict pixel validation on software-only runners whose compositor otherwise omits direct-present WebGPU surfaces; a background-only or stale capture still fails the comparison. The same recipe was reproduced in a clean Ubuntu 24.04 container before enabling it in CI.
