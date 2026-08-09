# Leptos reference feature inventory

**Reference audited:** `/home/trevor/Code/leptos-node-graph`  
**Purpose:** implementation inventory for a parity audit (not a statement of intended GPUI behavior).  
**Status labels:** **Implemented** means there is a live source path wired into the editor/demo; **Partial / consumer-owned** means the library emits a request or exposes a primitive but does not complete the workflow; **Dead / unwired** means an API or field exists but has no live integration; **Documented only / stale** means the design document claims more or different behavior than the source.

## 1. Repository and build surface

- The workspace contains one library and one CSR browser demo (`Cargo.toml:1-3`). The library is Rust 2024, Leptos 0.8 CSR, `web-sys`, and `leptos-use`; the enabled Web APIs explicitly include HTML elements, DOM rects, mouse/keyboard/wheel/pointer events, SVG, computed CSS and Window (`crates/leptos-node-graph/Cargo.toml:1-22`). The demo is likewise CSR/WASM (`examples/demo/Cargo.toml:1-11`).
- There is **no README** and no external `tests/` directory in the reference checkout. User-facing prose consists of Rustdoc and two internal planning/design documents. The checked-in `examples/demo/dist` is a built browser artifact, not a second source implementation.
- Verification performed without writing build products into the repository (`CARGO_TARGET_DIR=/tmp/...`): workspace tests and all-target checks pass; 16 unit tests run. Test coverage is narrowly algorithmic: ten subway-router tests (`crates/leptos-node-graph/src/subway.rs:1626-1995`), five incremental route-cache tests (`crates/leptos-node-graph/src/connection.rs:807-881`), and one registry geometry/no-op test (`crates/leptos-node-graph/src/registry.rs:656-711`). There are **no DOM/browser interaction tests**, persistence tests, component tests, or demo end-to-end tests.

## 2. Domain model and ownership

### Implemented

- Consumer-defined type compatibility is the core extension seam. `PortType` requires typed compatibility, stable string IDs, reconstruction, and offers ID-level compatibility; IDs are trait aliases over clone/equality/hash/debug/send/sync/static with blanket implementations for `String`, `u64`, and `usize` (`crates/leptos-node-graph/src/types.rs:5-41`).
- Geometry primitives are `Position`, `Size`, and `Rect`, with point containment and rectangle intersection (`types.rs:43-94`). `ViewportTransform` provides bidirectional screen/canvas transforms (`types.rs:199-230`).
- `PortDirection` is Input/Output (`types.rs:96-100`). Connections are externally supplied records of `id`, `source`, and `target` (`registry.rs:33-37`). Nodes and ports are registered internal records; ports retain parent, type, direction, absolute position, stable per-direction slot index, and cached node-relative offset (`registry.rs:9-29`).
- Hybrid ownership is real: `NodeEditor` receives a reactive external connection map and an event callback, while nodes are consumer-rendered children (`editor.rs:107-133`); the external map is copied into the internal registry reactively (`editor.rs:210-215`). Node positions are consumer `RwSignal`s registered into the internal model and updated live during drag (`node.rs:207-227`, `registry.rs:304-388`).
- Transient editor state includes registered nodes/ports/connections, two selection sets, draft connection, viewport and debounced visibility viewport, container size, box selection, pan/menu/drag/resize state, and RAF drag batching (`registry.rs:85-136`).

### Partial / consumer-owned

- The event protocol covers batched node moves, resizes, connection request/removal, selection change, delete/copy/paste, undo/redo, group creation, and create-node-with-auto-connect metadata (`types.rs:102-158`). Graph mutation remains the consumer’s job; `emit` merely invokes the callback (`registry.rs:170-173`).
- Node resize and movement write live consumer signals, but durable storage is not supplied. Width persistence is explicitly possible only if the consumer passes its own width signal (`node.rs:68-75`).
- Removing a node/port from the rendered tree deregisters internal geometry, but external connection records are intentionally retained so a dynamic port can reappear (`registry.rs:195-218`, `registry.rs:259-287`). Thus deletion/cascade semantics are consumer-dependent.

### Dead / unwired

- `GraphEvent::SelectionChanged` is declared (`types.rs:124-127`) but no source path emits it. Node and connection selection mutate internal signals directly (`registry.rs:476-507`, `connection.rs:598-609`).
- `EditorConfig.layout_mode` and `LayoutMode::{Classic,Structured}` exist (`types.rs:160-195`) but are never read by rendering or interaction code. `LayoutGraph`/`LayoutEngine::compute` are standalone declarations with no editor API or invocation (`layout.rs:1-14`).
- `UndoHistory` is a usable bounded two-stack container (`history.rs:3-69`) but is not owned or called by `NodeEditor`/registry. Keyboard undo/redo only emits events.
- `EditorRegistry::is_compatible_target` is an older input-only helper (`registry.rs:442-474`); live anchor completion and snapping use their own bidirectional logic (`anchor.rs:108-165`, `registry.rs:547-571`).

## 3. Public extension API

### Implemented

- The crate publicly exposes modules plus anchors/menu builders, connection style/routing mode, editor handle/root component, group types/component/events, history primitive, layout declarations, node/menu/type registry APIs, overlays, registry records, selection style, themes, and all core types (`crates/leptos-node-graph/src/lib.rs:1-39`).
- `NodeTypeBuilder` supports body renderers, reactive dynamic inputs/outputs, per-port slot overrides, and a fully custom renderer (`node_types.rs:69-151`). Auto-rendering combines static and dynamic ports, resolves per-port slots before global type/direction slots, constructs headers/body/anchors, and derives accent color from category (`node_types.rs:154-279`).
- `NodeTypeRegistry` preserves registration order for menu items, supports a global header renderer, global `(type ID, direction)` port slots, lookup, and render-by-type (`node_types.rs:322-390`).
- Consumers may bypass the builder using `NodeTypeDef::custom` (`node_types.rs:289-319`) or render `Node`, `InputAnchor`, and `OutputAnchor` directly. `Node` offers header/body/input/output slots, reactive accent, header color override, and controlled/uncontrolled width (`node.rs:40-76`). `NodeField` is a themed label/content row (`node.rs:562-579`). Anchors accept label or arbitrary children, dot color, six dot shapes, and a collection/ghost marker (`anchor.rs:918-985`, `theme.rs:14-50`).
- `AnchorContext` exposes direction, compatibility/incompatibility/source/connected/broken signals, dot ref, and type label for custom child rendering (`anchor.rs:10-25`, `anchor.rs:601-612`). A consumer-provided `AnchorMenuBuilder` replaces built-in right-click items wholesale and supports custom callbacks (`anchor.rs:67-105`, `anchor.rs:473-510`).
- `EditorHandle` is the supported outside-the-context bridge: live writable viewport, container node ref, client→canvas and canvas→client conversion, including container offset (`editor.rs:15-105`). This enables wrapper drop zones, toolbars, minimaps, and external viewport controls.
- Theme values are Leptos contexts. The public style structs cover node cards/sections/resize/cursors/layout (`theme.rs:53-160`), creation menu (`theme.rs:164-202`), groups (`theme.rs:204-230`), anchors/tooltips (`theme.rs:232-301`), connections (`connection.rs:299-317`), and rubber band (`selection.rs:8-22`). Defaults are provided.

### API caveats

- Node/port IDs in the generic core can be strings or integers, but `NodeTypeRegistry` is string-specific: generated node IDs and port IDs use `String`, and ports are named `"{node_id}_{local_port_id}"` (`node_types.rs:15-51`, `node_types.rs:289-390`).
- `NodeEditor`’s creation-menu search is deliberately two-way but **does not filter itself**: it exposes `menu_search` and renders the supplied `menu_items` (`editor.rs:113-120`, `menu.rs:164-178`). The demo implements label/category/description filtering outside the library (`examples/demo/src/main.rs:87-111`).
- Styling is configurable through structs, slots, and inline values, not CSS-class-state hooks. The design’s “headless/unstyled” characterization is inaccurate (see §11).

## 4. Canvas, pan, zoom, framing, and browser behavior

### Implemented

- The editor root is focusable, fills and clips its parent, and owns mousedown/wheel/keydown/double-click. Canvas pan/zoom is one CSS `translate(...px) scale(...)` with origin 0,0 (`editor.rs:393-425`, `editor.rs:451-477`). It auto-focuses on mount and restores focus after the menu closes (`editor.rs:203-208`, `editor.rs:351-360`).
- Pan starts with middle button or Ctrl+left; a per-editor flag prevents document-global listeners from moving sibling graphs. Deltas use `clientX/Y`, explicitly avoiding Chrome HiDPI physical-pixel `movementX/Y` behavior (`interaction.rs:46-56`, `interaction.rs:117-135`; registry rationale at `registry.rs:110-122`). Mousemove and mouseup listeners are document-level so gestures survive leaving the element (`editor.rs:224-243`).
- Wheel zoom is cursor-centered, geometric (`exp(±0.3)` per wheel direction), and clamped to configured min/max (`interaction.rs:393-451`). It does not hijack scrollable descendants, creation menus, overlays/backdrops, or consumer-tagged `[data-graph-no-zoom]`; it walks ancestors and computed `overflow-y` (`interaction.rs:343-390`).
- `F` frames all measured nodes with configurable screen padding and a separate magnification ceiling; fit is a no-op before measurement or on an empty graph (`interaction.rs:492-496`, `registry.rs:574-635`).
- Container measurement combines initial DOM offsets and `ResizeObserver`, avoiding a delayed/hidden-tab zero-size window (`editor.rs:151-175`).
- Viewport culling is content-level, not node unmounting. Each node stays registered but receives `NodeVisible`; visibility uses a 600-screen-pixel overscan and a viewport debounced 140 ms so expensive consumer subscriptions do not churn while panning (`node.rs:31-38`, `node.rs:142-173`, `editor.rs:177-193`).

### Limitations

- The connection SVG has a fixed `10000px × 10000px` drawing surface at canvas origin, though overflow is visible (`connection.rs:679-684`); negative/far coordinates rely on overflow behavior and are not an actually infinite SVG viewport.
- Interactions are mouse-centric. There is pointer usage for menus/dismissal, but node dragging, connection building, panning, and zoom plumbing use `MouseEvent`/`WheelEvent`; no touch, pen, pointer capture, ARIA keyboard graph navigation, or mobile gestures are implemented (`interaction.rs:1-2`, `editor.rs:217-273`).
- Zoom is wheel direction based rather than smooth delta magnitude by explicit design (`interaction.rs:432-440`).

## 5. Node rendering, editing, and performance

### Implemented

- Nodes measure border-box width/height using immediate DOM offsets plus ResizeObserver and update registry geometry; each size change emits `NodeResized` (`node.rs:89-130`, `node.rs:229-243`). Header/body/ports are separate measured/rendered sections, with ports in Columns or Stacked mode (`node.rs:467-515`, `theme.rs:1-12`).
- Click selects; Shift+click toggles. Clicking an already selected node preserves the multi-selection, then dragging moves the full selected set. Form controls and anchor dots do not initiate node drag (`node.rs:245-325`).
- Drag mousemoves are RAF-batched; all selected nodes update atomically, optional grid snapping is applied, cached port offsets move wires in the same batch, and one `NodesMoved { nodes }` event is emitted at mouseup (`interaction.rs:137-176`, `interaction.rs:266-286`, `registry.rs:325-388`).
- Nodes have a theme-controlled right-edge resize handle by default. Width is clamped, Escape restores the starting width, mouseup ends the gesture, and double-click resets to auto width (`node.rs:330-420`, `interaction.rs:102-115`, `interaction.rs:260-264`, `interaction.rs:525-531`). Drag/resize cursor remains active across the whole editor if the pointer outruns the element (`editor.rs:409-425`).
- Rendering provides selection outline/shadow, drag opacity, accent stripe, header/body styles, overflow clipping, and cursor states (`node.rs:422-465`, `node.rs:528-558`).

### Partial / consumer-owned

- The demo handles connection creation/removal, node deletion, node creation/auto-wire, group creation and group edits (`examples/demo/src/main.rs:130-205`, `examples/demo/src/main.rs:247-273`). It logs all other graph events as unhandled, so node-resize persistence, move command persistence, selection notifications, copy/paste, and undo/redo are not demonstrated (`examples/demo/src/main.rs:202-204`). Live position changes still appear because the library writes node signals.
- Resize emits continuously through measurement rather than one gesture-boundary event (`interaction.rs:260-261`, `node.rs:229-243`), unlike movement’s one final batch event.

## 6. Selection, deletion, clipboard, history, and shortcuts

### Implemented

- Empty-canvas left drag starts a selection rectangle; a non-Shift start clears selection, and intersection (not full containment) selects nodes live (`interaction.rs:59-90`, `interaction.rs:179-197`, `registry.rs:637-652`). The box is rendered in screen coordinates and scales with zoom (`selection.rs:24-65`).
- Connection paths are stroke-hit-testable and selectable; Shift toggles a connection while ordinary click makes it exclusive (`connection.rs:518-542`, `connection.rs:589-610`).
- Delete/Backspace emits removals for selected connections first, then a batched `NodesDeleted`; Ctrl/Meta+A selects all nodes (`interaction.rs:483-491`, `registry.rs:509-538`). Escape cancels resize, draft, and selection (`interaction.rs:525-534`).
- Ctrl/Meta+C emits selected IDs, Ctrl/Meta+V emits a fixed `(20,20)` offset, Ctrl/Meta+Z and Ctrl/Meta+Shift+Z emit Undo/Redo, and Ctrl/Meta+G emits selected IDs for group creation (`interaction.rs:497-524`). Key handlers ignore input, textarea, select, and contenteditable targets (`interaction.rs:464-477`).

### Partial / dead

- Copy/paste has no clipboard serialization, clipboard API, internal copied-node buffer, paste-at-mouse logic, or default mutation. It only emits requests; the demo does not handle them (`types.rs:131-138`, `interaction.rs:497-509`, `examples/demo/src/main.rs:202-204`).
- Undo/redo likewise only emits events; the standalone `UndoHistory` is not connected (`interaction.rs:510-517`, `history.rs:8-69`).
- Shortcut bindings are hard-coded match arms, despite the design claim that they are configurable; `EditorConfig` has no bindings field (`interaction.rs:479-537`, `types.rs:167-195`).
- Selection does not emit `SelectionChanged`; therefore consumers cannot observe it through the advertised event surface without obtaining/internalizing registry context.

## 7. Connection creation, validation, rerouting, and broken wires

### Implemented

- A primary-button press on a dot starts a draft from either input or output. A connected input first emits removal of its existing incoming connection and reroutes from the original output (`anchor.rs:259-350`). A draft completes only on opposite direction, different node, and `PortType::compatible`, with emitted endpoints normalized output→input (`anchor.rs:108-165`, `anchor.rs:353-400`). Click-to-start/click-to-complete and drag-to-connect share the same state.
- During a draft, valid ports glow/color, the source is highlighted, incompatible rows fade and disable pointer events, and the draft is a dashed path (`anchor.rs:372-468`, `anchor.rs:629-742`, `connection.rs:650-677`). Port snapping chooses the nearest compatible port within a screen-consistent radius and completing a snapped release is guaranteed to use the same validity rules (`interaction.rs:200-230`, `interaction.rs:309-324`, `registry.rs:547-571`).
- Empty-canvas mouseup cancels, anchor mouseup is allowed to complete/retain click-flow, and an open creation menu owns the draft until create/cancel (`interaction.rs:233-341`). Escape cancels it (`interaction.rs:525-533`).
- Ports register dynamically, obtain deterministic row slots, and compute positions analytically from node position/width, measured ports-section offset, padding, row height, and stacked-input count (`anchor.rs:189-257`, `registry.rs:220-287`).
- Anchors show connected/broken state. Broken means exactly one endpoint remains registered (`anchor.rs:441-468`). Default right-click actions remove all or only broken connections by emitting one removal per record; consumer menu actions can replace them (`anchor.rs:480-581`). Tooltips are portalled to body, anchored to the dot, and reposition on viewport changes (`anchor.rs:744-806`).

### Partial / consumer-owned

- The library validates type, direction, and same-node constraints but does **not** generally enforce cardinality or duplicate edges. The demo independently rejects a second incoming connection (`examples/demo/src/main.rs:135-145`).
- ConnectionRequested is a request; visibility occurs only after the consumer inserts the connection into the supplied signal (`editor.rs:210-215`). Removal behaves the same way.
- Dynamic missing-port connections are kept and rendered as dashed 30px stubs with `?` where exactly one endpoint exists (`connection.rs:548-644`, `connection.rs:687-709`). Where both endpoints are missing, nothing is drawn.

## 8. Connection rendering and routing

### Implemented

- Connections are SVG paths behind groups/nodes; connection paths receive per-state stroke/width styles, selected styling, pointer hit-testing, and stable debug-ID ordering (`editor.rs:462-466`, `connection.rs:464-648`). `ConnectionStyle` customizes normal/selected/draft strokes and widths (`connection.rs:299-317`).
- `RoutingMode` is a consumer-provided reactive context signal. Default is Orthogonal; Bezier uses cubic control points (`connection.rs:16-38`, `connection.rs:329-333`, `utils.rs:3-23`).
- Orthogonal mode uses a batch “subway” router around node rectangles, rounded polyline corners, deterministic ordering, shared-lane/crossing/overlap penalties, limits, and fallbacks; its public inputs/options/stats and entry points are defined at `subway.rs:48-166`. The renderer caches identical geometry and performs partial solves for incident/nearby routes when only nodes move (`connection.rs:40-113`, `connection.rs:368-461`). It logs routing statistics when their signature changes (`connection.rs:711-737`).
- Router tests cover anchor contact/orthogonality, obstacle avoidance, separated parallel corridors, shared trunks, missing rectangles, determinism, simplification, near-intersection, nesting preference cycles, and oversized-grid fallback (`subway.rs:1626-1995`). Cache tests cover changed-node mapping, previous/current proximity, cache hits, and full structural re-solves (`connection.rs:807-881`).

### Limitations

- Draft connections in Orthogonal mode use the simple single-connection elbow helper rather than the subway batch route (`connection.rs:650-677`, `utils.rs:62-90`).
- Missing endpoint records deliberately bypass normal hit testing and use noninteractive stubs (`connection.rs:548-618`).

## 9. Creation menu and graph-creation workflows

### Implemented

- The menu domain includes category/color, node label/description, and typed input/output port metadata convertible to type-erased compatibility IDs (`menu.rs:7-119`).
- Tab opens at the last document mouse position converted to canvas coordinates. Empty-canvas double-click also opens, but double-click is suppressed on nodes, anchors, or during a draft (`editor.rs:195-201`, `editor.rs:251-316`). Menu screen position is separately retained for fixed rendering.
- Opening clears search and focuses the input. Outside pointerdown, Escape, or Tab cancels. After closure editor focus is restored (`menu.rs:185-218`, `menu.rs:280-317`, `editor.rs:351-360`). The fixed panel clamps to viewport edges and provides scrolling/empty state (`menu.rs:337-414`).
- Keyboard navigation uses ArrowUp/Down/Enter, maintains a selected row, keeps it in view, and mouse hover synchronizes selection (`menu.rs:238-335`, `menu.rs:488-560`, `menu.rs:599-627`). Categories and descriptions render (`menu.rs:432-475`, `menu.rs:552-557`).
- During a draft the menu filters to node types with compatible opposite-direction ports. One compatible port auto-selects; multiple ports become independently navigable/clickable subrows. The create event carries initiating port, chosen new local port, and origin direction so the consumer can create and wire atomically (`menu.rs:220-278`, `menu.rs:572-597`, `editor.rs:318-390`).
- The demo registers five node types (Color Source, Mix, Math, Output, Custom) with categories, descriptions and typed ports (`examples/demo/src/utils/catalog.rs:5-57`). It demonstrates searchable catalog filtering and node creation/auto-wire (`examples/demo/src/main.rs:87-111`, `examples/demo/src/main.rs:154-186`).
- External HTML5 drop is demonstrated on a wrapper: data-transfer text selects a node type and `EditorHandle.client_to_canvas` places it correctly under pan/zoom (`examples/demo/src/main.rs:124-129`, `examples/demo/src/main.rs:275-332`). This is **demo/extension behavior**, not built into `NodeEditor`.

## 10. Groups

### Implemented

- `GroupBox` is consumer data: string group ID, node IDs, optional label/color, and error flag; bounds are computed from live node rectangles (`group.rs:9-40`, `group.rs:376-400`). The editor optionally renders groups behind nodes and optionally forwards group callbacks (`editor.rs:121-126`, `editor.rs:430-447`).
- Visual group bounds include configurable padding and label height. Normal groups use dashed color-mixed border/background, error groups use error theme, and Alt-drag hover uses stronger solid styling (`group.rs:186-275`). A custom group-header callback can replace the built-in label (`group.rs:59-74`, `group.rs:243-266`).
- Ctrl/Meta+G requests a group from the current node selection (`interaction.rs:518-524`). The demo creates groups only for more than one node and assigns label/color (`examples/demo/src/main.rs:188-200`).
- Double-clicking the label edits inline, focuses/selects text, and commits rename on blur, Enter, **or Escape** (`group.rs:278-373`).
- Alt-drag immediately emits removal from every current group, highlights the group under the dragged node center, then emits one add on drop (`group.rs:84-184`). The demo mutates its group vector in response (`examples/demo/src/main.rs:247-273`).

### Caveats

- Groups are visual/data overlays, not parent transforms: dragging a group does not move its contents, there is no collapse/expand, nesting, group selection, resize, or delete workflow.
- Escape in rename commits rather than cancels (`group.rs:349-353`).
- Alt removal occurs at drag start; cancellation/undo is entirely consumer responsibility.

## 11. Styling and overlay system

### Implemented

- The implementation ships substantial inline default styling, including dark node/menu/group/anchor/connection/selection palettes (`theme.rs:125-160`, `theme.rs:183-230`, `theme.rs:277-301`, `connection.rs:307-317`, `selection.rs:15-22`). Consumers override by providing style structs through context, per-node colors/widths, per-anchor shape/color, slots, or fully custom node renderers.
- Stable structural hooks actually emitted include `.node-editor`, `.node-editor__canvas`, `.node-editor__overlays`, plus `data-node`, section/resize markers, anchor/dot/tooltip/menu markers, connection normal/dangling/draft markers, menu/list/selected markers, and overlay/backdrop markers (`editor.rs:451-476`, `node.rs:517-558`, `anchor.rs:873-914`, `connection.rs:589-674`, `menu.rs:384-404`). State styling is mostly computed inline, not class modifiers.
- `NodeOverlay` portals consumer content into an editor-owned pane layer outside the scaled canvas but inside clipping. It supports Node/selector/NodeRef/pane-rect anchors, four sides, three alignments, offset, opposite-side flip and clamp, optional per-frame anchor tracking, transparent backdrop, outside/Escape dismissal, and extra style (`overlay.rs:46-155`, `overlay.rs:173-303`, `overlay.rs:305-330`). It warns and renders nothing outside a `NodeEditor` (`overlay.rs:157-162`).
- The demo proves overlay context ownership with a Mix slider panel anchored to a trigger inside the node; portalled children retain the node body’s `BlendMix` context (`examples/demo/src/nodes.rs:14-59`, `examples/demo/src/nodes.rs:83-120`).

### Dead / misleading styling fields

- `AnchorStyle.first_port_y` is retained/defaulted but live geometry instead uses measured `ports_y_offset`; it is not read in anchor positioning (`theme.rs:256-262`, `theme.rs:290-293`, versus `anchor.rs:219-257`).
- `NodeMenuStyle.placeholder_color` exists/defaults but the input only receives the common inline input style; no placeholder selector/style consumes it (`theme.rs:166-200`, `menu.rs:372-402`).

## 12. Persistence and serialization

**Not implemented.** There is no serde dependency, file format, save/load API, local/session storage use, IndexedDB, URL state, clipboard serialization, migration/versioning, or autosave in either library or demo (`crates/leptos-node-graph/Cargo.toml:6-22`, `examples/demo/Cargo.toml:6-11`). Demo nodes, connections, groups, control values, widths, and history are memory-only signals initialized by `generate_demo_graph` (`examples/demo/src/main.rs:71-85`; `examples/demo/src/utils/seed.rs:10-138`).

What exists is a consumer-event architecture suitable for external persistence: final multi-node drag boundaries (`interaction.rs:266-285`), live controlled position/width signals (`node.rs:41-75`), and mutation requests in `GraphEvent` (`types.rs:102-158`). The library does not implement the persistence layer.

## 13. Demo-specific behavior

- Demo port compatibility permits exact matches or any target of `Any`, with string round-tripping (`examples/demo/src/main.rs:17-40`).
- Initial data is only two nodes and one attempted deterministic connection (`examples/demo/src/main.rs:75-82`); the seed generator supports arbitrary counts and groups rows, but the call’s `num_nodes=2` produces one initial group containing both nodes (`examples/demo/src/utils/seed.rs:10-43`, `examples/demo/src/utils/seed.rs:110-138`).
- Global Float input slots render number inputs; Mix renders a blend select and overlay; Custom renders 0–8 reactive input/output anchors; other catalog entries auto-render (`examples/demo/src/nodes.rs:65-175`).
- Demo theme overrides node/card/selection/connection/anchor values and uses a full-window dark wrapper (`examples/demo/src/main.rs:209-242`, `examples/demo/src/main.rs:275-281`).
- The demo’s `DynNode.category` is stored but marked dead and never read by rendering; the registry definition supplies render category (`examples/demo/src/main.rs:56-63`, `examples/demo/src/main.rs:340-342`).

## 14. Documented-only and stale claims

The internal design spec is an intention artifact, not reliable current API documentation.

- It claims “headless/unstyled — zero default visual styles” and lists CSS-state classes (`docs/superpowers/specs/2026-04-07-leptos-node-graph-design.md:5-11`, `:208-224`). **Stale:** the code provides detailed defaults and inline styling; most listed `.node`, `.anchor--*`, `.connection--*`, and `.selection-box` classes are not emitted.
- It says transient registry includes history and the library owns undo/redo (`design.md:31-39`, `:182-185`). **Documented only:** history is standalone/unwired; keyboard emits requests.
- It says port/node deregistration automatically removes referenced connections and emits removal (`design.md:40-44`, `:76-79`). **Contradicted:** current code intentionally retains external connections when ports disappear (`registry.rs:259-262`) and deregistration does not emit removal.
- It describes `NodeEditor graph=...`, optional Node size, and generic component shapes (`design.md:48-75`, `:95-125`). **Stale signature:** current editor accepts `connections`, children, menu/groups/handle; current Node accepts header/body/anchors/colors/width.
- Its event example uses singular `NodeMoved` and tuple geometry and mentions `ConnectionCreated` (`design.md:127-147`, `:157-167`). **Stale:** implementation emits batched `NodesMoved` with `Position` and `ConnectionRequested` (`types.rs:102-158`).
- It claims all shortcuts configurable, paste at mouse position, command-based internal undo, and full demo support (`design.md:176-185`, `:255-263`). **Not implemented:** keys are hard-coded, paste offset is fixed, history is external/unwired, and the demo logs these events unhandled.
- It claims functional Classic and Structured modes, grid/slot management, layout invocation, and switching (`design.md:187-207`, `:263`). **Documented only:** only enum/config/trait declarations exist.
- It describes Bezier as the internal connection renderer (`design.md:90-93`, `:208-214`). **Outdated default:** current default is Orthogonal/subway, with Bezier optional (`connection.rs:16-38`).
- It claims “library owns all interaction logic” including copy/paste/undo and grouping (`design.md:3-11`). More precisely, the library recognizes gestures and emits events; graph-data semantics and persistence remain consumer-owned.

## 15. Parity checklist / concise disposition

| Area | Reference disposition |
|---|---|
| Generic IDs + typed port compatibility | **Implemented** |
| Consumer-owned nodes/connections + event protocol | **Implemented**, mutation consumer-owned |
| HTML nodes, SVG wires, pan/zoom transform | **Implemented** |
| Mouse pan, cursor wheel zoom, fit view | **Implemented** |
| Multi-node drag, grid snap, box/select-all/connection selection | **Implemented** |
| Width resize/reset | **Implemented**, persistence consumer-owned |
| Bidirectional connect, reroute input, click/drag flow, snap radius | **Implemented** |
| Type/direction/same-node validation | **Implemented**; cardinality/duplicates consumer-owned |
| Orthogonal obstacle routing + Bezier option + caching | **Implemented** |
| Broken/dynamic ports and connection stubs | **Implemented** |
| Searchable creation menu + compatible pin submenus | **Implemented**, text filtering supplied by consumer |
| External drop coordinate API | **Implemented API + demo**, not editor-native |
| Groups, rename, Ctrl+G request, Alt-drag membership | **Implemented**, data consumer-owned |
| Node type builder, dynamic ports, slots/custom renderer | **Implemented** |
| Theme/style contexts and per-anchor shapes | **Implemented**, not truly headless |
| Pane-level anchored overlays | **Implemented** |
| Off-screen expensive-content gating | **Implemented** |
| Copy/paste | **Event-only / partial** |
| Undo/redo | **Event-only; standalone history unwired** |
| SelectionChanged event | **Dead/unemitted** |
| Layout engine / Structured mode | **Declarations only / dead** |
| Persistence/serialization | **Absent** |
| Touch/accessibility interaction parity | **Absent** |
| Browser/E2E tests | **Absent** |
