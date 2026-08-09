# Leptos → GPUI node graph parity audit

**Reference:** `leptos-node-graph@87658950fccfeeea123285c706820ffea4ab55d1`  
**Candidate:** `gpui-node-graph@fa2387e378c195506fc5984e24384319590e1b5f`  
**Audit rule:** parity is based on behavior actually wired in the reference source/demo, not on its older design document. A tested core helper that the GPUI view never renders or invokes is graded **partial**, not complete.

Supporting evidence:

- [`leptos-reference-inventory.md`](./leptos-reference-inventory.md)
- [`gpui-current-inventory.md`](./gpui-current-inventory.md)
- [`independent-parity-gaps.md`](./independent-parity-gaps.md)

## Verdict

`gpui-node-graph` is a good cross-platform persistence/model foundation, but it is **not yet an editor with product parity**. Its visible authoring surface is currently title rectangles, pre-existing elbow wires, one-node dragging, and middle-button panning (`crates/gpui-node-graph/src/lib.rs:119-244`). The Leptos reference's central loop—discover or create a typed node, see and interact with ports, author/remove typed connections, select/edit/delete graph items, and configure rich node content—is absent.

The independent audit found **3 blockers, 8 high-priority, 8 medium-priority, and 5 low-priority gaps**. The first milestone must restore the authoring loop; routing polish and advanced composition should not hide those blockers.

## Status legend

- **Complete:** usable through the shared GPUI view, not merely represented in a DTO.
- **Partial:** some reusable model/math exists, but the shared view or public integration contract is incomplete.
- **Missing:** no equivalent accessible behavior.
- **Do not port yet:** present only as an unintegrated/reference placeholder rather than shipped reference behavior.
- **GPUI stronger:** retain the candidate's stronger architecture while adding UX parity.

## Parity matrix

| Area | Leptos reference behavior | GPUI status | Priority / evidence |
|---|---|---:|---|
| Shared desktop/browser implementation | Reference is browser-only; candidate intentionally shares one GPUI view | **GPUI stronger** | Preserve `examples/demo/src/main.rs:13-25` |
| Typed generic IDs and ports | Consumer ID traits and directional `PortType` | **Partial / mixed** | Candidate has stronger serializable ID wrappers, but its `PortType` omits reference type-ID round-tripping needed by catalog/menu compatibility (reference `types.rs:9-31`; candidate core `lib.rs:8-31`) |
| Geometry precision | Reference canvas DTOs are `f64`; candidate DTOs are `f32` with defensive sanitization | **Different / low risk** | Reference `types.rs:36-100`; candidate core `lib.rs:34-79,103-181` |
| Persisted vs transient state | Consumer owns reference data; registry owns transient data | **GPUI stronger** | Candidate snapshots/UI split `node-graph-core/src/lib.rs:242-301` |
| Trust-boundary validation | Reference relies on registration and consumer state | **GPUI stronger** | Candidate canonicalization/validation/reconcile `:303-483` |
| Consumer mutation/event contract | Reference recognizes gestures and asks the consumer to mutate; candidate directly mutates its public graph and emits only four editor event shapes | **Partial / semantic mismatch** | Reference `types.rs:102-158`, `registry.rs:170-173`; candidate `gpui-node-graph/src/lib.rs:10-24,51-54` |
| Selection-change notification | Declared but never emitted by the reference; candidate does emit it for its narrow selection paths | **GPUI stronger, incomplete UX** | Reference `types.rs:124-127`; candidate `gpui-node-graph/src/lib.rs:188-200` |
| Visible ports and labels | Rendered input/output sockets and row content | **Missing — blocker** | Reference `anchor.rs:873-990`; candidate renders no port loop, view `lib.rs:119-204` |
| Port layouts and content slots | Column/stacked layouts; labels or arbitrary child slots | **Missing** | Reference `theme.rs:3-12`, `node.rs:41-75,467-557` |
| Socket states and styling | Connected/source/compatible/incompatible/broken signals | **Missing** | Reference `anchor.rs:10-25,430-468` |
| Socket shapes/tooltips | Circle/diamond/square/hex/triangle/star, multi marker, portalled dot-anchored tooltip | **Missing** | Reference `theme.rs:19-48`, `anchor.rs:684-694,744-805` |
| Typed connection drafting | Start from input or output; click/drag flows; dashed live draft | **Missing — blocker** | Reference `anchor.rs:108-430`, `connection.rs:650-675`; candidate only has `compatible_target` core helper `:584-595` |
| Snap to compatible socket | Configurable screen-pixel snap and exact compatibility rules | **Missing** | Reference `types.rs:178-186`, `registry.rs:541-571` |
| Complete/request connection | Emits consumer-owned `ConnectionRequested` | **Missing — blocker** | Reference `types.rs:111-118`; candidate UI event lacks it `gpui-node-graph/src/lib.rs:10-24` |
| Connection removal | Selected-wire delete and anchor-menu removal | **Missing from view** | Reference `registry.rs:518-538`, `anchor.rs:513-550`; candidate cascade helper is core-only `:531-582` |
| Dangling dynamic connections | Missing endpoint renders a dashed `?` stub and may restore when port returns | **Missing / semantic mismatch** | Reference deliberately retains on deregistration `registry.rs:259-287`, renders stub `connection.rs:548-645`; candidate validation rejects dangling state |
| Connection selection | Click and Shift-toggle wire, selected styling | **Missing** | Reference `connection.rs:518-610`; candidate models selected IDs but paint discards connection IDs `gpui-node-graph/src/lib.rs:124-151` |
| Orthogonal routing | Reference default is obstacle-aware rounded subway routing | **Partial** | Candidate fixed midpoint H-V-H route `gpui-node-graph/src/lib.rs:139-151` |
| Bezier routing switch | Reactive orthogonal/Bezier mode | **Missing** | Reference `connection.rs:16-37,368-461` |
| Route caching/incremental solve | Stable routes, obstacle avoidance, lane separation, partial reroutes and budget fallbacks | **Missing** | Reference `connection.rs:40-269,368-461`; `subway.rs` |
| Searchable node creation | Tab-at-pointer/double-click menu owns search text; consumer filters the supplied item signal, while draft compatibility filtering is internal | **Missing — blocker** | Reference `editor.rs:251-316`, `menu.rs:164-236`; demo filtering `main.rs:87-111`; candidate demo is startup-hard-coded `examples/demo/src/main.rs:25-81` |
| Catalog/category metadata | Five demo types, categories, descriptions, typed ports | **Missing** | Reference demo `utils/catalog.rs:5-58` |
| Draft-to-new-node workflow | Menu filters compatible ports and creates+connects in one action | **Missing** | Reference `editor.rs:318-389`, `menu.rs:220-280,577-636` |
| Node type registry/builder | Declarative types, header/body/port slots, dynamic port closures | **Missing** | Reference `node_types.rs:1-391` |
| Dynamic nodes/ports | Reactive add/remove and port-count changes | **Partial/core-only — no dynamic composition UX** | Candidate can reconcile changed node/port maps, but has no retained-view registration, dynamic definition/measurement API, or UI creation path (candidate `gpui-node-graph/src/lib.rs:78-117,119-204`; reference demo `nodes.rs:117-176`) |
| Rich node content | Header, body controls, selects, numeric inputs, custom renderer | **Missing** | Reference `node.rs:41-75`; demo `nodes.rs:55-176`, `widgets.rs:4-58`; candidate renders only `n.title` |
| Node overlays/popovers | Pane-clipped, unscaled, anchored overlay with flip/clamp/dismiss | **Missing** | Reference `overlay.rs:51-385`; candidate has no overlay layer |
| External drop / coordinate handle | Public viewport plus client↔canvas conversion used by demo drop | **Partial/core math only** | Candidate has viewport transforms but no external live handle, container/client-offset conversion, or drop workflow; reference `editor.rs:15-104`, demo `main.rs:275-332`; candidate core `lib.rs:103-120` |
| Single-node selection | Click selects a node | **Partial** | Candidate supports singleton select but leaves connection selection stale and ignores modifiers `gpui-node-graph/src/lib.rs:186-200` |
| Multi-selection | Shift-toggle nodes and wires | **Missing** | Reference `node.rs:275-322`, `connection.rs:598-609` |
| Blank-canvas deselect | Click empty canvas clears selection | **Missing** | Reference `interaction.rs:60-90` |
| Box selection | Rubber-band overlay and rectangle query | **Partial/core-only** | Candidate has `nodes_in_rect` but no gesture/overlay `node-graph-core/src/lib.rs:596-607` |
| Multi-node dragging | Drag selection as a unit; optional grid snap | **Missing** | Reference `interaction.rs:137-175`; candidate drag token holds one ID `gpui-node-graph/src/lib.rs:53,223-231` |
| Drag event boundary | One batched `NodesMoved` on mouse-up | **Mismatch** | Candidate mutates/emits every mouse move; reference `interaction.rs:266-285` |
| Node resize | Right edge, min/max, Escape rollback, double-click reset, resize event | **Missing** | Reference `node.rs:330-404`, `interaction.rs:102-115,525-531` |
| Middle-button pan | Canvas pan | **Complete, narrow** | Candidate `gpui-node-graph/src/lib.rs:205-221,240-243` |
| Pointer release outside editor | Reference document listeners terminate fast/out-of-bounds gestures; candidate move/up handlers are attached to root | **Needs test / likely gap** | Reference `editor.rs:224-243`; candidate `gpui-node-graph/src/lib.rs:211-243` |
| Ctrl+left pan | Alternate pan gesture | **Missing** | Reference `interaction.rs:46-56` |
| Wheel zoom about pointer | Geometric zoom, min/max, pointer anchoring | **Partial/core-only** | Candidate `Viewport::zoom_at` exists `node-graph-core/src/lib.rs:147-171`, but view has no wheel handler |
| Wheel routing | Embedded controls/overlays consume their own scrolling | **Missing** | Reference `interaction.rs:343-451` |
| Fit graph | `F` frames bounds with padding and max zoom | **Partial/core-only** | Candidate has `bounds`, no command `node-graph-core/src/lib.rs:608-630` |
| Grid snap | Optional configured snapping during drag | **Missing** | Reference `types.rs:168-178`, `interaction.rs:151-170` |
| Delete / select all | Delete/Backspace and Ctrl/Cmd+A | **Partial/core-only / missing input** | Reference `interaction.rs:483-496`; candidate `remove_nodes` is never invoked by view |
| Focus and keyboard safety | Focusable editor; commands ignore text/select/contenteditable controls | **Missing** | Reference `editor.rs:203-208,451-460`, `interaction.rs:464-477` |
| Groups | Auto bounds, color/error state, inline rename, create, Alt-drag membership | **Missing** | Reference `group.rs:9-370` |
| Theme surface | Separate node/anchor/wire/menu/group/selection styling and per-node overrides | **Partial** | Candidate exposes five colors only and hard-codes other metrics `gpui-node-graph/src/lib.rs:25-42,139-185` |
| Off-screen visibility/culling hook | Debounced viewport computes and provides `NodeVisible`; consumers must opt in to gate expensive content, and the demo does not | **Missing hook** | Reference `editor.rs:151-193`, `node.rs:31-38,142-173,194-205` |
| Gesture/render performance | RAF-batched moves; cached per-wire routes and partial solve | **Missing** | Reference `interaction.rs:92-230`, `connection.rs:337-461` |
| Demo parity | Reference demonstrates group, typed ports, menu catalog, controls, dynamic ports and overlay | **Missing** | Candidate demo shows fixed homogeneous graph only `examples/demo/src/main.rs:25-81` |
| Browser/native parity harness | Candidate boots same view and verifies a canvas | **Partial** | Startup proof exists, but no scripted editor interaction or visual assertions |

## Important non-targets in the reference

The old design spec overstates several areas. These should not be used to inflate the immediate parity scope:

1. **Undo/redo is not an integrated reference history system.** `UndoHistory` is a standalone exported utility with no editor use (`history.rs:1-70`). Keyboard shortcuts only emit `Undo`/`Redo` (`interaction.rs:510-517`), and the demo's catch-all logs them rather than applying history (`examples/demo/src/main.rs:204-207`).
2. **Copy/paste is event-only in the demo.** Ctrl/Cmd+C/V emits `NodesCopied`/`NodesPasted` (`interaction.rs:497-509`), but the demo does not implement those events.
3. **Structured layout/auto-layout is a placeholder.** `LayoutEngine` is only a trait (`layout.rs:1-14`); `LayoutMode::Structured` is stored in config but no editor path consumes it.
4. **Dynamic port removal intentionally retains dangling consumer connections** so they can reappear, contrary to the older spec's cascade-removal claim (`registry.rs:259-287`).

Port the event vocabulary if API compatibility matters, but do not call these workflows complete until GPUI supplies a real product behavior and tests it.

## What must not regress

The GPUI rewrite already improves several architectural areas:

- serde-ready, framework-free `GraphSnapshot` and explicit `GraphUiState` separation;
- validation, ID canonicalization, safe reconciliation, and transient-selection pruning;
- directional asymmetric connection validation;
- atomic node plus world-port movement with finite/overflow rejection;
- finite/saturating viewport and rendering arithmetic;
- one shared retained-mode view across Windows, macOS, Linux, and browser/WASM.

Parity work should extend these contracts rather than replace them with DOM-shaped state or direct application-data mutation.

## Decisions required before implementation

1. **Controlled vs editor-owned graph state.** The reference emits mutation requests into consumer-owned signals; the candidate mutates `NodeGraph.graph` directly. Choose and document a controlled/uncontrolled contract before expanding events. A delegate that applies/approves mutations while the editor owns transient gesture state can support both.
2. **One event vocabulary.** `node_graph_core::GraphEvent` and GPUI `EditorEvent` already diverge. Replace the split with one coherent, gesture-boundary-aware event contract rather than adding parallel variants feature by feature.
3. **Node composition model.** Current persisted `Node` has only `title`. Rich GPUI nodes need an application renderer/delegate or typed payload/type key without embedding `Entity`/UI state in `GraphSnapshot`.
4. **Port geometry ownership.** Current ports store absolute world coordinates. Dynamic content and resizing require measured/derived anchor positions. Define whether a renderer reports port bounds, a layout delegate computes offsets, or the editor owns rows; do not make consumers manually rewrite every port coordinate.
5. **Dangling connection semantics.** Strict candidate validation rejects the reference's temporary missing-port records. Decide whether unresolved edges are a transient reconciliation layer, a separately typed persisted state, or an allowed validated status. Do not silently weaken all trust-boundary validation.
6. **Cardinality and replacement policy.** Reference core checks direction/type/same-node, while its demo alone enforces one incoming edge and anchor drag replaces an occupied input. Make capacity/replacement a configurable library contract.
7. **Scope of nominal features.** Keep structured layout, built-in history, and real clipboard semantics outside the parity critical path because the reference does not actually integrate them.

## Recommended delivery sequence

### Milestone 1 — minimum viable authoring loop (blockers)

1. Introduce a GPUI editor configuration/delegate contract and a node-type catalog.
2. Render typed input/output sockets and labels from real measured node geometry.
3. Add socket hit testing, hover compatibility, draft state, pointer capture, snap, completion and cancellation.
4. Surface connection requested/removed events through one coherent editor event API.
5. Add searchable create menu at pointer, including create-and-connect during a draft.
6. Upgrade the demo to prove: create node → connect → select wire → delete wire/node.

**Exit:** a user can build and modify a typed graph without application code mutating maps behind the editor.

### Milestone 2 — baseline editing/navigation

1. Stable focus handle and GPUI actions for delete, select all, Escape, fit, copy/paste, undo/redo hooks and menu open.
2. Blank deselect, Shift-toggle, connection selection, box selection and multi-node drag.
3. Emit one batched movement event per completed gesture; add cancel behavior and pointer capture.
4. Wire wheel zoom about cursor, Ctrl+left pan, min/max zoom, fit padding and optional grid snapping.
5. Add deterministic selection and gesture tests.

**Exit:** reference selection, drag, navigation and command traces pass on desktop and browser.

### Milestone 3 — composition and dynamic graph behavior

1. Node header/body/input/output render slots and consumer-defined GPUI content.
2. Declarative node registry/builder with categories, descriptions, typed ports and dynamic port definitions.
3. Node measurement and resize handles with min/max/reset/cancel semantics.
4. Anchor visual states, shapes, tooltips, default/custom menus, broken connection management.
5. External coordinate handle and overlay/popover layer.

**Exit:** port the five-type reference demo, including Float editors, Mix controls/overlay, and Custom 0–8 dynamic ports.

### Milestone 4 — groups, routing and scale

1. Group DTO/view/events, inline rename and Alt-drag membership.
2. Orthogonal/Bezier modes and dangling-wire presentation.
3. Port the deterministic subway router as framework-free core logic, then add route cache/partial invalidation.
4. A consumer-opt-in off-screen `NodeVisible` equivalent and stable z-order.
5. Add performance budgets for large graphs and routing fallbacks.

**Exit:** group, overlay, routing, dynamic-port and large-graph scenarios match reference screenshots and interaction traces.

## Required parity harness

A parity claim should include the same scripted scenario on native and browser hosts:

1. open node menu with Tab and double-click;
2. search/create each catalog type;
3. draft compatible and incompatible connections from both directions;
4. create a node from an unfinished draft;
5. select node/wire, Shift-toggle, box-select and drag a selection;
6. delete/cancel/select-all, fit, pan and zoom around pointer;
7. resize/reset a node and edit embedded controls without triggering graph shortcuts;
8. add/remove dynamic ports and confirm dangling/restored connection behavior;
9. create/rename/reassign a group;
10. open/dismiss a node overlay;
11. assert emitted events, persisted snapshot, rendered state and screenshot landmarks.

The harness must verify behavior, not just that GPUI creates a canvas.
