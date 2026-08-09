# Independent product/UX parity gap audit

**Reference:** `/home/trevor/Code/leptos-node-graph`  
**Candidate:** `/home/trevor/Code/gpui-node-graph`  
**Reference revision:** `87658950fccfeeea123285c706820ffea4ab55d1`  
**Candidate revision:** `fa2387e378c195506fc5984e24384319590e1b5f`  
**Scope:** committed library and demo behavior. I inspected both demo entry points and the interaction/rendering paths behind them. I did not treat an uncalled core helper as shipped UX. Both repositories were clean at the start of the audit. The GPUI README itself labels most of these areas unfinished (`README.md:19-29`).

## Executive verdict

The GPUI candidate is a sound cross-platform **foundation**, but it is not yet product/UX equivalent to the Leptos editor. Its shared view currently paints title-only rectangles and pre-existing elbow wires, supports single-node dragging and middle-button panning, and little else. The largest issue is not visual polish: the reference's central authoring loop—see ports, create typed wires, create/configure nodes, select/edit/delete—is absent.

I found **3 blockers, 8 high, 8 medium, and 5 low gaps**. Core-only capabilities such as `zoom_at`, `remove_nodes`, `compatible_target`, `nodes_in_rect`, and `bounds` do reduce future implementation cost, but they are not connected to GPUI input/rendering (`node-graph-core/src/lib.rs:147-171,532-634`; the entire rendered input surface is `gpui-node-graph/src/lib.rs:119-244`).

## Blockers

### B1. No visible or interactive ports; no connection authoring/removal

- **Reference behavior:** input/output anchors are first-class interactive components, typed compatibility is checked, both click-to-connect and drag-to-connect are supported, a dashed draft wire is painted, nearby compatible ports snap in screen-pixel radius, and completing/removing a connection emits events. Evidence: `leptos-node-graph/src/types.rs:9-31,111-123,199-213`; `interaction.rs:200-230,309-340`; `connection.rs:650-675`; `anchor.rs:918-983`.
- **Candidate behavior:** ports exist only as model coordinates used to obtain wire endpoints (`gpui-node-graph/src/lib.rs:124-134`). Rendering then iterates **nodes**, drawing only a rectangle and title (`:162-203`); there is no port render loop or port mouse handler. `compatible_target` exists only in core (`node-graph-core/src/lib.rs:584-595`) and is never called by the view. `EditorEvent` has no connection create/remove variants (`gpui-node-graph/src/lib.rs:10-24`).
- **Impact:** users cannot build or edit a graph, which is the primary product workflow.

### B2. No node catalog, searchable creation menu, or dynamic creation

- **Reference behavior:** Tab at the cursor or double-click on empty canvas opens a searchable menu (`editor.rs:113-120,251-316`); during a draft it filters to compatible node/port choices and can create-and-wire in one step (`editor.rs:318-389`; `menu.rs:160-177,220-260`). The demo exposes five catalog types with categories, descriptions and typed ports (`examples/demo/src/utils/catalog.rs:5-65`). It also accepts cross-pane HTML drop and converts client coordinates correctly under pan/zoom (`examples/demo/src/main.rs:229-278`; `editor.rs:77-104`).
- **Candidate behavior:** the demo hard-codes three nodes, four ports and two wires during startup (`examples/demo/src/main.rs:25-81`) and `NodeGraph` has no creation-menu state or handler (`gpui-node-graph/src/lib.rs:45-55,119-244`). There is no `CreateNode` editor event (`:10-24`).
- **Impact:** graph topology cannot grow at runtime through the UI.

### B3. Basic editing commands are absent

- **Reference behavior:** Delete/Backspace, select-all, fit, Escape, and Tab have live editor behavior; copy, paste, undo, redo, and group shortcuts emit consumer requests, with input/select/textarea/contenteditable protection (`interaction.rs:454-537`). The demo handles group creation but leaves copy/paste/undo/redo unhandled. Delete emits selected connection removals and node deletion; connection paths are clickable/selectable (`registry.rs:518-538`; `connection.rs:518-610`; demo catch-all `examples/demo/src/main.rs:202-204`).
- **Candidate behavior:** no key-down handler exists anywhere in the GPUI render chain (`gpui-node-graph/src/lib.rs:119-244`). Core has a correct `remove_nodes` helper (`node-graph-core/src/lib.rs:531-581`) and enum cases for delete/undo/redo (`:214-240`), but the view never invokes/emits them and its public `EditorEvent` omits them (`gpui-node-graph/src/lib.rs:10-24`).
- **Impact:** users lack working delete, select-all, fit, cancel, group, and menu commands, and consumers lack view-level hooks for the reference's partial clipboard/history protocol.

## High priority

### H1. Selection is single-node only and differs on every modifier/blank-wire case

- Reference: click selects, Shift toggles, selected nodes drag together (`node.rs:275-322`); empty-canvas click clears/starts box selection and Shift preserves existing selection (`interaction.rs:64-90,179-197`); wires are selectable and Shift-toggleable (`connection.rs:598-609`).
- Candidate: node mouse-down unconditionally clears all selected nodes and inserts one, ignoring modifiers (`gpui-node-graph/src/lib.rs:186-200`). The root has no left-button blank-canvas handler, no box overlay, and wires are paint-only canvas paths. `selected_connections` is reported but cannot be changed by candidate UI. Core `nodes_in_rect` is latent only (`node-graph-core/src/lib.rs:596-607`).

### H2. Multi-node dragging and gesture/event semantics differ

- Reference moves all selected nodes, optionally snaps each to grid, batches visual updates via RAF, and emits one atomic `NodesMoved` only at mouse-up (`interaction.rs:137-175,266-285`; `types.rs:105-110`).
- Candidate drag state stores one ID (`gpui-node-graph/src/lib.rs:53`) and `move_node` returns only that one node (`node-graph-core/src/lib.rs:487-529`). It mutates and emits `EditorEvent::NodeMoved` on **every mouse move** (`gpui-node-graph/src/lib.rs:223-231`), while mouse-up merely clears drag (`:234-238`). This can flood persistence/history and loses the reference's gesture boundary.

### H3. Zoom, fit-view, Ctrl+drag pan, and zoom limits are not exposed

- Reference: wheel zooms around cursor with configurable min/max (`interaction.rs:393-451`; `types.rs:144-178`), F frames all nodes with padding/zoom ceiling (`interaction.rs:492-496`), and pan accepts middle-drag or Ctrl+left (`interaction.rs:46-56`). It deliberately yields wheel input to node/overlay scrollable content (`interaction.rs:343-390`).
- Candidate: view registers only middle mouse for pan (`gpui-node-graph/src/lib.rs:205-220,240-242`) and no wheel/key handler. Core implements robust cursor-anchored `zoom_at` and `bounds` (`node-graph-core/src/lib.rs:147-171,608-629`) but the UI never calls them. No candidate config exposes reference min/max, fit padding, or fit max zoom.

### H4. Groups are wholly absent

- Reference demo starts with a group around its two initial nodes (`examples/demo/src/main.rs:61-65`; `examples/demo/src/utils/seed.rs:78-110`). Groups auto-bound their members, have label/color/error styling, inline rename, and Alt-drag remove/add with hover feedback (`group.rs:9-74,84-184,186-275,278-370`). Ctrl+G emits group creation (`interaction.rs:518-524`).
- Candidate model/view has no group type, layer, event, input gesture, or group demo data (`node-graph-core/src/lib.rs:188-285`; `gpui-node-graph/src/lib.rs:10-55`).

### H5. Rich node content and dynamic ports are absent

- Reference nodes have header/body/input/output slots, reactive accent/header colors, dynamic port views, configurable column/stacked layouts, and consumer controls (`node.rs:41-75,467-557`). The demo's Float inputs contain editable number fields; Mix has a blend select and mix editor; Custom can change input/output counts 0–8 reactively (`examples/demo/src/nodes.rs:55-176`; `widgets.rs:4-58`).
- Candidate `Node` model contains only `id/title/position/size` (`node-graph-core/src/lib.rs:188-197`), and the view renders only `n.title` (`gpui-node-graph/src/lib.rs:171-203`). Port positions must be supplied manually and cannot react to content/layout.

### H6. Node resizing and live measured geometry are absent

- Reference measures DOM node/header/body geometry, emits `NodeResized`, supports right-edge resize with min/max, Escape rollback, and double-click reset-to-auto (`node.rs:89-140,229-243,330-404,517-524`; `interaction.rs:102-115,260-264,525-531`).
- Candidate node size is static model data and there is no resize state/handle/event (`node-graph-core/src/lib.rs:188-197`; `gpui-node-graph/src/lib.rs:45-55,171-203`).

### H7. Connection routing is substantially less capable

- Reference defaults to deterministic obstacle-aware subway routing with rounded corners, lane separation, caching and partial re-solves; it can reactively switch to Bezier (`connection.rs:16-37,115-269,368-461,502-512`; `subway.rs:158-437`). It also distinguishes normal/selected/draft and dangling connections (`connection.rs:518-645`).
- Candidate always paints a fixed midpoint H-V-H polyline (`gpui-node-graph/src/lib.rs:139-151`; same simple core helper at `node-graph-core/src/lib.rs:632-634`). It does not avoid nodes, round corners, separate overlapping routes, select/highlight wires, switch style, draft, or show dangling endpoints.

### H8. Consumer mutation/event contract is incomplete

- Reference declares atomic node moves, resize, connection request/removal, selection, delete/copy/paste, undo/redo, group/create-node and lets the consumer own graph data (`types.rs:103-142`; `examples/demo/src/main.rs:96-207`). Most mutation paths emit those requests, but `SelectionChanged` is declared and never emitted, while the demo leaves movement persistence, resize persistence, copy/paste and undo/redo unhandled.
- Candidate's UI mutates its public `graph` internally and publicly emits only one-node move, selection, viewport and reconciliation (`gpui-node-graph/src/lib.rs:10-24,51-54,188-231`). Core declares a broader `GraphEvent` (`node-graph-core/src/lib.rs:210-240`) but the GPUI `EventEmitter` does not surface most of it. This is an integration blocker for consumers expecting reference behavior even after adding custom chrome.

## Medium priority

### M1. Node overlays/popovers are absent

Reference `NodeOverlay` portals unscaled content to a pane-clipped layer, anchors to node/element/selector, supports side/alignment/offset, collision flipping/clamping, backdrop dismissal, Escape, and repositioning under viewport changes (`overlay.rs:51-125,127-385`; editor mount layer `editor.rs:402-407,468-476`). The Mix demo exercises a live slider overlay with inherited reactive context (`examples/demo/src/nodes.rs:13-52,73-108`). Candidate has no overlay layer or overlay API.

### M2. Per-anchor menus/actions and connection affordances are absent

Reference anchors support custom menu items/actions and state (`anchor.rs:29-110`), connected/compatible visual states, direction-aware draft interactions, labels/slots, and dot theming (anchor implementation `anchor.rs:168-983`). Candidate does not paint anchors at all; therefore it also lacks anchor menus, hover compatibility, connection replacement/removal affordances, and input default-value slots.

### M3. Styling/extensibility parity is far behind

Reference exposes separate `NodeStyle`, `AnchorStyle`, `ConnectionStyle`, `SelectionBoxStyle`, `NodeMenuStyle`, `GroupStyle`, dot shapes and anchor layouts (`theme.rs:3-299`; `connection.rs:297-315`). Node content remains consumer-defined. Candidate `Theme` exposes only five RGB colors (`gpui-node-graph/src/lib.rs:25-42`); border, radius, padding, font/size, wire width/route, selected styling, port styling and slots are hard-coded (`:139-185`).

### M4. Off-screen visibility hook and interaction performance behavior are absent

Reference measures the container, debounces the visibility viewport 140 ms, and provides each always-mounted node a `NodeVisible` signal with overscan; consumers must explicitly use that hook to gate expensive content, and the demo does not (`editor.rs:151-193`; `node.rs:31-38,142-173,194-205`). It also RAF-batches drags and incrementally reroutes affected wires. Candidate rebuilds children by iterating every node every render and every wire every canvas paint (`gpui-node-graph/src/lib.rs:124-162`) with no equivalent visibility hook, route cache, or performance suite; the README acknowledges culling and visual/performance tests remain undone (`README.md:25-29`).

### M5. Grid snapping and layout-mode surface are missing

Reference `EditorConfig` exposes optional grid size and layout mode (`types.rs:134-168`), and drag applies snap-to-grid (`interaction.rs:151-170`). Candidate has neither editor config nor snap behavior. Note: although `LayoutMode::Structured` and a layout trait exist in the reference API, `layout.rs` is only a trait definition and the demo does not visibly switch modes; this audit therefore calls **grid snapping** a demonstrated behavioral gap, while structured auto-layout remains parity debt rather than a proven current-demo regression.

### M6. External editor handle and coordinate conversion/drop integration are absent

Reference `EditorHandle` exposes reactive viewport, container, client→canvas and canvas→client conversion for toolbars/minimaps/drop targets (`editor.rs:15-104,127-149`). Demo uses it for correct cross-pane drag/drop at any pan/zoom (`examples/demo/src/main.rs:229-278`). Candidate exposes its graph publicly but no supported external viewport/coordinate handle and no demo drop workflow.

### M7. Dynamic registration and dangling/restored-port behavior are not available through the view

Reference nodes/ports register and deregister with component lifetime and keep measured positions synchronized (`node.rs:207-227`; `registry.rs:222-287`). It deliberately retains consumer connection records when a dynamic port disappears so a one-ended dashed stub can render and the wire can restore if the port returns (`registry.rs:259-287`; `connection.rs:548-645`). Candidate core can reconcile snapshots and cascade when explicitly calling `remove_nodes` (`node-graph-core/src/lib.rs:352-386,531-581`), but the retained view has no dynamic registration API or UI mutation path, and validated candidate snapshots reject dangling endpoints. Candidate ports are absolute world-space DTOs maintained manually.

### M8. Demo coverage/content is not comparable

Reference demo is a full-viewport graph (`examples/demo/src/main.rs:215-228`) with two generated typed nodes, a connection, an initial group, five discoverable types, category colors and interactive controls. Candidate opens a centered 1000×700 window and hard-codes three homogeneous Number nodes (`examples/demo/src/main.rs:17-42`), four ports whose labels are raw IDs (`:44-64`), and two wires (`:66-81`). It demonstrates neither the candidate core's validation/reconciliation nor most editor workflows, so it cannot serve as parity acceptance evidence.

## Low priority

### L1. Geometry precision differs

Reference public geometry uses `f64` (`types.rs:36-100`); candidate uses `f32` (`node-graph-core/src/lib.rs:34-79`). Candidate carefully sanitizes/saturates values (`:103-181`), so this is not currently a UX failure, but very large canvases or repeated transforms can drift sooner than the reference.

### L2. Pan input has a possible stuck-gesture edge

Reference listens for document-level move/up so a fast gesture that leaves the editor still terminates (`editor.rs:224-243`). Candidate attaches move/up to the root element (`gpui-node-graph/src/lib.rs:211-243`). If GPUI does not implicitly capture these events, releasing outside the root can leave `drag`/`panning` set; this needs an interaction test.

### L3. Candidate selection does not clear selected connections

On node click candidate clears only `selected_nodes` and emits whatever `selected_connections` already contains (`gpui-node-graph/src/lib.rs:193-198`). Reference single-select helpers produce coherent node/connection selection and clickable wires (`connection.rs:598-609`; `registry.rs` selection methods). This becomes user-visible once external reconciliation preselects a wire.

### L4. Theme/selected visual semantics differ

Reference demo uses a thin red outline for selected nodes and category accent/header styling (`examples/demo/src/main.rs:185-205`; catalog categories `utils/catalog.rs:8-62`). Candidate signals selection by changing the whole fill from `0x27272a` to `0x3f3f46` (`gpui-node-graph/src/lib.rs:33-41,165-181`) and has no category color. This is polish after functional parity.

### L5. Reference accessibility/DOM semantics do not carry over automatically

Reference menu uses a real focused search input and keyboard navigation, node editor is focusable, forms are native controls, and group rename is an input (`editor.rs:451-460`; `menu.rs:181-195`; `group.rs:323-356`). Candidate currently renders non-focusable title rectangles and no controls. GPUI can provide accessibility, but the candidate source contains no explicit focus/action/accessibility implementation for graph items.

## What is already at or ahead of parity (do not regress)

- Candidate has framework-free serde snapshots and deliberately separates transient selection/viewport (`node-graph-core/src/lib.rs:242-301`), plus canonicalization/validation/reconciliation (`:303-483`). This is stronger persisted-domain hygiene than the demo-facing reference.
- Node dragging atomically translates its world-space ports and rejects non-finite/overflow updates (`node-graph-core/src/lib.rs:484-530`).
- Viewport math is defensive against invalid/non-finite input (`node-graph-core/src/lib.rs:103-181`).
- One shared GPUI render implementation is used by native and wasm (`examples/demo/src/main.rs:13-23`; `README.md:1-3,31-37`).

## Recommended acceptance order

1. **Authoring loop:** paint/hit-test typed ports; draft/snap/complete/remove connections; searchable create menu.
2. **Editing loop:** modifier/multi/box/wire selection; blank deselect; delete; gesture-boundary move events; undo/redo/copy/paste.
3. **Navigation:** wheel zoom about cursor, Ctrl+left pan, fit view, limits, grid snap.
4. **Node composition:** body/port slots, dynamic ports, controls, resize, overlay layer.
5. **Groups and routing:** group UX, obstacle-aware cached routing, dangling/selected wires.
6. **Parity harness:** run the same scripted interaction trace on desktop and browser and add screenshots for initial, menu, draft, multi-select, group, overlay, zoom/fit, and dynamic-port states.

A parity claim should require that the GPUI **view** exercises core helpers; existence of a tested helper alone is not equivalent to accessible product behavior.
