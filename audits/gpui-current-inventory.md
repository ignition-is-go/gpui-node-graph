# GPUI node graph: current-feature inventory

## Scope and method

This is a source-level inventory of the current `gpui-node-graph` repository for later comparison with `leptos-node-graph`. It covers every tracked source/configuration/documentation file, the public model and view APIs, all implemented gestures, rendering, events, persistence, demo hosts, and tests. The locally generated `examples/demo/dist/*` bundle is ignored build output rather than an additional implementation (`.gitignore:3`); it was not treated as source. I also ran `cargo test --workspace`: all 15 unit tests passed (13 core, 1 view, 1 window-capability test); neither crate has doc tests and the demo has no tests.

The repository is a three-member Rust workspace: framework-free core, GPUI view, and one demo binary (`Cargo.toml:1-3`). It pins official Zed GPUI and `gpui_platform` to revision `08827f...` and declares `gpui_web` at the workspace level (`Cargo.toml:10-15`). The stated intent is one shared retained-mode GPUI view on desktop and Wasm, not DOM/Leptos rendering (`README.md:1-7`).

## Executive summary: what actually works

The current product is a **small, validated graph DTO plus a minimal visual demo/editor**:

* It models generic typed nodes, ports, and directed connections, with serializable geometry and a separately expressible persisted snapshot (`crates/node-graph-core/src/lib.rs:8-31`, `crates/node-graph-core/src/lib.rs:192-249`).
* It validates IDs/references/geometry/direction/type compatibility and supplies safe viewport math (`crates/node-graph-core/src/lib.rs:99-190`, `crates/node-graph-core/src/lib.rs:410-483`).
* The GPUI view paints titled rectangles and fixed three-segment orthogonal wires, selects one node on left press, drags a node (and its ports), and pans on the middle button (`crates/gpui-node-graph/src/lib.rs:119-244`).
* The demo displays exactly three hard-coded nodes, four invisible model ports, and two hard-coded wires (`examples/demo/src/main.rs:25-82`).
* Desktop and browser share the same application entry/view; browser initialization, isolation headers, Wasm/shared-memory flags, and a browser-startup smoke test are present (`examples/demo/src/main.rs:13-23`, `.cargo/config.toml:1-5`, `.github/workflows/ci.yml:27-78`).

It is **not yet a general interactive node editor**. There is no rendered port/socket UI, connection drafting/creation/removal gesture, wheel zoom wiring, box or wire selection, deselection, deletion/undo/redo commands, node creation/catalog/menu, grid/snap, groups, resize, keyboard actions, overlays, culling, persistence host, or production fixture/replay/visual/performance suite. The repository itself lists almost all of these as future work (`README.md:28-40`).

## 1. Framework-free domain model and public API

### IDs and typing

* `NodeId`, `PortId`, and `ConnectionId` are marker traits requiring clone/equality/hash/debug/static; primitives and `String` implement all three roles (`crates/node-graph-core/src/lib.rs:8-10`, `crates/node-graph-core/src/lib.rs:22-27`).
* `Id<T>` is a serde-transparent generic newtype and also implements all three ID roles. Compile-time role separation therefore requires distinct inner types, as its docs explicitly note (`crates/node-graph-core/src/lib.rs:11-21`).
* `PortType` is application-defined and has one directional `compatible(source, target)` predicate (`crates/node-graph-core/src/lib.rs:29-31`). Serialization is conditional through the containing derived types; `PortType` itself does not require serde.

### Geometry and viewport

* Public DTOs are `Point`, `Size`, and `Rect`, all `f32`, `Clone + Copy + PartialEq + Serialize + Deserialize`; only `Point` and `Size` implement `Default` (`crates/node-graph-core/src/lib.rs:32-66`).
* `Point` supplies Euclidean distance plus addition/subtraction (`crates/node-graph-core/src/lib.rs:37-55`). `Rect::contains` includes its boundary, while `Rect::intersects` uses strict inequalities, so edge-only contact is not intersection (`crates/node-graph-core/src/lib.rs:67-80`).
* `Viewport` is screen-space pan plus positive zoom and defaults to `(0,0), 1` (`crates/node-graph-core/src/lib.rs:86-98`). It supplies world/screen transforms, safe nonnegative length scaling, screen-delta panning, cursor-anchored bounded zoom, validity checking, and sanitization (`crates/node-graph-core/src/lib.rs:99-190`). Calculations use `f64` and saturate at finite `f32` boundaries (`crates/node-graph-core/src/lib.rs:100-125`, `crates/node-graph-core/src/lib.rs:159-171`). Invalid zoom inputs are ignored; rendering/conversion sanitizes invalid stored viewport values rather than propagating NaN/zero (`crates/node-graph-core/src/lib.rs:144-189`).

### Graph records and state split

* `Node` contains only ID, title, position, and size (`crates/node-graph-core/src/lib.rs:192-198`). There is no payload, node type/catalog key, per-node style, controls, z-order, group, collapsed state, resizability, or arbitrary metadata.
* `Port` contains ID, owner node, label, input/output direction, application port kind, and an **absolute world-space position** (`crates/node-graph-core/src/lib.rs:199-209`). It has no capacity/cardinality, optionality, value, style, or relative layout declaration.
* `Connection` contains only ID and source/target port IDs (`crates/node-graph-core/src/lib.rs:210-215`). It has no label, payload, style, route, waypoints, or status.
* `GraphSnapshot` is the persisted domain: three hash maps of nodes, ports, connections (`crates/node-graph-core/src/lib.rs:242-258`). `GraphUiState` is transient selected node/connection sets plus viewport (`crates/node-graph-core/src/lib.rs:260-275`).
* `GraphState` flattens those six fields for convenience/backward-compatible JSON shape, but serde skips/defaults selection and viewport (`crates/node-graph-core/src/lib.rs:277-299`). `snapshot()` and `ui_state()` explicitly project the two concerns (`crates/node-graph-core/src/lib.rs:319-336`). There is no file/database/local-storage loader or saver in this repository: persistence stops at serde-compatible in-memory DTOs.

### Validation, construction, reconciliation

* `GraphState::from_snapshot` canonicalizes embedded IDs from map keys, validates, and starts with default transient state (`crates/node-graph-core/src/lib.rs:337-349`).
* `canonicalize_ids` overwrites embedded node/port/connection IDs with their keys (`crates/node-graph-core/src/lib.rs:388-409`). Consequently the view constructors canonicalize mismatches rather than report them (`crates/gpui-node-graph/src/lib.rs:61-76`). Direct `validate()` without canonicalization does report mismatches (`crates/node-graph-core/src/lib.rs:410-451`).
* Validation aggregates problems for ID mismatch, non-finite/negative node geometry, missing port owners, non-finite port positions, missing connection endpoints, invalid output-to-input/type-compatible connections, and invalid viewport (`crates/node-graph-core/src/lib.rs:410-483`). `compatible_target` additionally forbids same-node links and requires output -> input plus the application predicate (`crates/node-graph-core/src/lib.rs:584-595`).
* Validation does **not** check whether IDs in `selected_nodes` or `selected_connections` exist; only snapshot reconciliation prunes them (`crates/node-graph-core/src/lib.rs:350-386`). Thus `NodeGraph::try_new`/`set_graph` can accept stale transient selection even though their docs broadly say they validate state (`crates/gpui-node-graph/src/lib.rs:66-96`).
* `reconcile(snapshot)` validates a candidate before mutation, replaces only domain maps, preserves viewport, retains still-valid selection, and returns `GraphReconciled` plus `SelectionChanged` only when pruning occurred (`crates/node-graph-core/src/lib.rs:350-386`).

### Mutations and queries

* `move_node` rejects non-finite movement and atomically translates the node and every owned world-space port by the same delta; overflow of any port aborts the entire mutation. It returns one `NodesMoved` event (`crates/node-graph-core/src/lib.rs:484-530`).
* `remove_nodes` cascades through owned ports and incident connections, prunes selection, and returns connection-removal, node-deletion, and (if changed) selection events (`crates/node-graph-core/src/lib.rs:531-582`). Because hash maps/sets drive collection, event ordering and deleted-ID order are not specified/deterministic.
* `nodes_in_rect` returns nodes whose rectangles overlap the query; `bounds` computes the union bounds of all nodes or `None` for empty state (`crates/node-graph-core/src/lib.rs:596-630`). Neither query is wired into the GPUI UI.
* `orthogonal_route(a,b)` returns `[a, midpoint-at-a-y, midpoint-at-b-y, b]` with an x midpoint (`crates/node-graph-core/src/lib.rs:632-635`). It is deterministic but not obstacle-aware, direction-aware, cached, or guarded against midpoint overflow. The renderer duplicates this simple midpoint scheme instead of calling this API (`crates/gpui-node-graph/src/lib.rs:139-149`).
* There are no public operations to add a node/port/connection, remove a connection alone, change selection, fit the viewport, or perform undo/redo; consumers can directly mutate the public maps and fields instead (`crates/node-graph-core/src/lib.rs:277-287`).

## 2. Events: declared, emitted, and unwired

### Core event vocabulary

`GraphEvent` declares nodes moved, connection requested/removed, selection changed, nodes deleted, undo, viewport changed, graph reconciled, and redo (`crates/node-graph-core/src/lib.rs:216-241`). Actual core methods emit only:

* `NodesMoved` from `move_node` (`crates/node-graph-core/src/lib.rs:527-529`);
* `ConnectionRemoved`, `NodesDeleted`, and sometimes `SelectionChanged` from cascading node removal (`crates/node-graph-core/src/lib.rs:549-580`);
* `GraphReconciled` and sometimes `SelectionChanged` from snapshot reconciliation (`crates/node-graph-core/src/lib.rs:374-385`).

There is no implementation that produces `ConnectionRequested`, `Undo`, `Redo`, or core `ViewportChanged`. Those are vocabulary/placeholders with **no UI wiring**.

### GPUI event surface

`EditorEvent` exposes only `NodeMoved`, `SelectionChanged`, `ViewportChanged`, and unit-like `GraphReconciled` (`crates/gpui-node-graph/src/lib.rs:10-24`), and `NodeGraph` implements GPUI `EventEmitter` (`crates/gpui-node-graph/src/lib.rs:56-59`). Therefore core deletion/connection/undo/redo events have no matching view event.

* `set_graph` emits reconciliation, selection, and viewport unconditionally after successful replacement, then notifies (`crates/gpui-node-graph/src/lib.rs:78-96`).
* view `reconcile` emits reconciliation, conditionally selection when core reports pruning, and notifies; it does not emit viewport because reconciliation preserves it (`crates/gpui-node-graph/src/lib.rs:98-117`).
* node press emits selection even if the same singleton was already selected (`crates/gpui-node-graph/src/lib.rs:186-200`).
* middle-drag emits viewport change on each accepted move (`crates/gpui-node-graph/src/lib.rs:211-221`); node drag emits node movement on each successful move (`crates/gpui-node-graph/src/lib.rs:223-231`).

The demo never subscribes to any event and never persists or otherwise reacts to edits: it simply constructs and returns the entity (`examples/demo/src/main.rs:23-84`). Thus the public event API exists but has **no host/demo wiring**.

## 3. Rendering inventory

The single generic `NodeGraph<T,N,P,C>` owns a public `GraphState` and `Theme`, plus private active node-drag and pan state (`crates/gpui-node-graph/src/lib.rs:44-55`). `Theme` has exactly five `u32` RGB colors: background, normal/selected node, wire, and text, with dark defaults (`crates/gpui-node-graph/src/lib.rs:25-42`). Border color, rounding, stroke width, font size, and padding are hard-coded rather than themed (`crates/gpui-node-graph/src/lib.rs:143-149`, `crates/gpui-node-graph/src/lib.rs:176-185`).

Each render:

1. sanitizes a copy of the viewport (`crates/gpui-node-graph/src/lib.rs:122-124`);
2. resolves each connection to its two model port positions, silently skipping a connection whose endpoint cannot be found (`crates/gpui-node-graph/src/lib.rs:124-134`);
3. paints all wires in one full-size absolute canvas behind nodes, as a fixed horizontal/vertical/horizontal 2px path in one color (`crates/gpui-node-graph/src/lib.rs:135-161`);
4. iterates the node hash map and creates absolute titled rectangles, scaling position, dimensions, text, and padding with zoom (`crates/gpui-node-graph/src/lib.rs:162-204`). Selected nodes only change fill color (`crates/gpui-node-graph/src/lib.rs:165-181`).

Material limitations visible in source:

* **Ports are never rendered**, despite labels/directions/types/positions being modeled. There are no sockets or port hit targets anywhere in the render tree (`crates/gpui-node-graph/src/lib.rs:119-204`). Wire endpoints therefore appear on node edges only because demo coordinates were manually chosen (`examples/demo/src/main.rs:44-64`).
* Connection IDs and selected connection state are not carried into the wire layer; wires cannot differ when selected and cannot be hit-tested (`crates/gpui-node-graph/src/lib.rs:124-151`).
* Nodes render only `title`; there is no custom node-content render callback/component or body/control rendering (`crates/gpui-node-graph/src/lib.rs:171-185`).
* There is no background grid, minimap, draft wire, marquee, handles, resize affordance, selection outline, context/search menu, tooltip, status/empty state, or overlay.
* There is no culling: every connection and node is processed every render (`crates/gpui-node-graph/src/lib.rs:124-162`). There is also no route cache.
* Node iteration uses `HashMap::values`, so overlapping node paint order is not an explicit stable z-order (`crates/gpui-node-graph/src/lib.rs:162-204`).
* Invalid dangling wires are silently dropped at render even though normal constructors reject them; direct mutation is possible because `graph` is public (`crates/gpui-node-graph/src/lib.rs:51-52`, `crates/gpui-node-graph/src/lib.rs:128-133`).

## 4. Interaction inventory

### Implemented gestures

* **Select/start drag:** left mouse-down on a node converts cursor screen -> world, stores `(node ID, cursor-to-origin offset)`, clears only the node selection set, inserts that node, emits `SelectionChanged`, and notifies (`crates/gpui-node-graph/src/lib.rs:186-201`). Existing connection selection is deliberately/incidentally left untouched (`crates/gpui-node-graph/src/lib.rs:193-198`).
* **Drag:** any subsequent root mouse move with an active drag converts to world coordinates, calls atomic `move_node`, emits `NodeMoved`, and redraws (`crates/gpui-node-graph/src/lib.rs:223-231`). Since ports translate in core, connected wires follow (`crates/node-graph-core/src/lib.rs:484-529`).
* **End drag:** root left mouse-up clears drag state (`crates/gpui-node-graph/src/lib.rs:234-239`).
* **Pan:** middle mouse-down stores screen position; each root mouse move calls `pan_between`, emits and redraws; middle mouse-up clears state (`crates/gpui-node-graph/src/lib.rs:205-221`, `crates/gpui-node-graph/src/lib.rs:240-243`).

### Not implemented / demo-only interaction

* No wheel event is registered, although `Viewport::zoom_at` is implemented and tested. Therefore users cannot zoom (`crates/node-graph-core/src/lib.rs:144-172`; complete registered-handler list is `crates/gpui-node-graph/src/lib.rs:186-243`).
* No left-background handler: clicking empty space does not clear selection. No modifier handling or multi-select; every node press replaces node selection with a singleton (`crates/gpui-node-graph/src/lib.rs:186-200`).
* No box selection despite `nodes_in_rect`; no connection selection despite the transient selected set (`crates/node-graph-core/src/lib.rs:262-265`, `crates/node-graph-core/src/lib.rs:596-607`).
* No port gesture or connection hit target, so connection request/create/remove cannot happen from UI.
* No keyboard/focus handlers: delete, escape/cancel, select all, undo/redo, copy/paste, navigation, fit view, and shortcuts do not exist.
* No grid or snapping. No group drag, node resize, reroute handles, or drag-to-create node.
* No explicit pointer capture or mouse-leave/cancel handling is present; drag/pan termination relies on the root receiving mouse-up (`crates/gpui-node-graph/src/lib.rs:205-243`).
* `set_graph` replaces the graph but does not clear an in-progress `drag`/`panning` token (`crates/gpui-node-graph/src/lib.rs:78-96`), an edge case hosts would need to avoid.

## 5. Persistence and application integration

The persistence contract is well-defined at the DTO layer but no persistence workflow exists:

* Save `GraphSnapshot` or serialized `GraphState`; either excludes viewport/selection (`crates/node-graph-core/src/lib.rs:242-249`, `crates/node-graph-core/src/lib.rs:277-287`, `crates/node-graph-core/src/lib.rs:319-329`).
* Restore through `from_snapshot` to canonicalize/validate (`crates/node-graph-core/src/lib.rs:337-349`).
* Reconcile an updated application-owned snapshot while keeping viable UI state (`crates/node-graph-core/src/lib.rs:350-386`; view adapter at `crates/gpui-node-graph/src/lib.rs:98-117`).

There are no version/schema tags, migrations, checked-in persisted JSON fixtures, import/export UI, autosave, filesystem APIs, browser storage APIs, or history stack. The README mandates migration fixture tests but the roadmap admits Rship fixtures are still absent (`README.md:26`, `README.md:38`). The only “fixture” test serializes a value produced in memory and immediately round-trips it; it is not a checked-in legacy/production fixture (`crates/node-graph-core/src/lib.rs:763-787`).

## 6. Demo and host behavior

### Shared demo graph

`Kind` has only `Number` and equality compatibility (`examples/demo/src/main.rs:4-11`). On startup the app creates one centered 1000x700 window (`examples/demo/src/main.rs:16-23`), then hard-codes Source/Multiply/Output rectangles of identical size on one row (`examples/demo/src/main.rs:25-43`), four ports at manually chosen absolute coordinates (`examples/demo/src/main.rs:44-65`), and two connections (`examples/demo/src/main.rs:66-81`). It constructs `NodeGraph::new`, unwraps window creation, and activates the app (`examples/demo/src/main.rs:82-88`).

This is a presentation/smoke demo, not an application shell: no toolbar, instructions, inspector, mutation controls, event subscription, persistence, dynamic data, error recovery, or use of `try_new`, `set_graph`, `reconcile`, removal, queries, window service, or custom theme.

### Native desktop

`gpui_platform::application()` chooses the backend and the package enables both Linux Wayland and X11 features (`examples/demo/src/main.rs:16`, `Cargo.toml:11-13`). CI compiles/checks/tests on Ubuntu, macOS, and Windows and explicitly verifies both Linux backend features (`.github/workflows/ci.yml:4-25`). The README suggests unsetting `DISPLAY` to force Wayland when both sessions are present (`README.md:9-17`).

`PlatformWindowService` advertises detached-window capability on non-Wasm and wraps `App::open_window`, mapping platform errors to a string (`crates/gpui-node-graph/src/windows.rs:3-18`, `crates/gpui-node-graph/src/windows.rs:24-45`). This is a public capability boundary only: the demo never instantiates or invokes it. There is no multiwindow UI/workflow.

### Browser/Wasm

The exact same `main` calls `gpui_platform::web_init()` only on Wasm, then uses the same application/window/entity construction (`examples/demo/src/main.rs:13-25`). The browser HTML is a blank full-viewport page styled around an injected canvas; it records Trunk startup and isolation in document data attributes (`examples/demo/index.html:1-6`). There is no DOM application UI or JS-to-Rust application API.

The browser is designed for one document-owned GPUI window; Wasm detached-window requests return `UnavailableInBrowser` (`crates/gpui-node-graph/src/windows.rs:21-44`). “Exactly one” is an architectural convention/documented boundary rather than a test of attempted second shared `open_window`; the service test only checks the compile-target capability boolean (`crates/gpui-node-graph/src/windows.rs:47-56`).

Because GPUI Web uses shared memory, the repo:

* enables atomics/bulk memory/mutable globals, shared/imported memory, TLS exports, and builds `std`/`panic_abort` for wasm (`.cargo/config.toml:1-5`);
* gives Trunk local COEP/COOP response headers (`examples/demo/Trunk.toml:1-5`) and copies an equivalent static-host `_headers` file (`examples/demo/_headers:1-3`, `examples/demo/index.html:1`);
* checks the workspace for `wasm32`, makes a release Trunk build, serves it with isolation/no-cache headers, launches Chrome under Xvfb, and runs a DevTools smoke assertion (`.github/workflows/ci.yml:27-78`, `.github/scripts/serve_dist.py:1-19`).

The JS smoke check waits for a page and asserts only: Trunk application-start event, `crossOriginIsolated === true`, and existence of any canvas (`.github/scripts/check_browser.mjs:1-58`). It does **not** inspect nodes/wires, exercise input, compare pixels, test persistence/events, or surface Rust runtime state beyond canvas startup.

## 7. Test inventory and coverage limits

### Core: 13 unit tests

The tests cover cursor-anchored zoom (`crates/node-graph-core/src/lib.rs:647-654`), trivial orthogonal routing (`crates/node-graph-core/src/lib.rs:655-660`), rectangle selection (`crates/node-graph-core/src/lib.rs:661-686`), directional/asymmetric compatibility (`crates/node-graph-core/src/lib.rs:748-754`), port translation during movement (`crates/node-graph-core/src/lib.rs:755-761`), transient-free serde round trip (`crates/node-graph-core/src/lib.rs:763-787`), ID validation/canonicalization and missing owners (`crates/node-graph-core/src/lib.rs:788-809`), cascading node removal (`crates/node-graph-core/src/lib.rs:810-824`), invalid zoom inputs (`crates/node-graph-core/src/lib.rs:825-836`), atomic extreme movement (`crates/node-graph-core/src/lib.rs:837-858`), finite extreme viewport math (`crates/node-graph-core/src/lib.rs:859-883`), selection pruning events (`crates/node-graph-core/src/lib.rs:884-909`), and transparent typed-ID serialization (`crates/node-graph-core/src/lib.rs:910-916`).

Not covered include bounds, point/rect boundary semantics, same-node compatibility, multiple validation errors in one assertion, missing connection endpoints, negative/nonfinite geometry, direct connection removal/creation (not implemented), deterministic ordering, real persisted fixtures/migrations, and property/fuzz tests.

### GPUI view/window: 2 unit tests

One test checks `try_new` errors and `new` panics on zero zoom (`crates/gpui-node-graph/src/lib.rs:247-265`). One checks the platform-derived detached-window capability boolean (`crates/gpui-node-graph/src/windows.rs:47-56`). There are **no GPUI render, event subscription, pointer/keyboard interaction, reconciliation-adapter, theme, native-window-opening, or detached-window error-path tests**.

### CI quality gates

CI runs formatting, all-target checking, native workspace tests, warnings-as-errors Clippy, platform matrix builds, Wasm checking/build, header checks, and browser startup smoke (`.github/workflows/ci.yml:4-78`). The roadmap explicitly acknowledges missing full interaction replay and visual regression/performance suites (`README.md:38-39`).

## 8. Explicit incomplete/demo-only behavior and APIs with no UI wiring

| Item | What exists | What is visibly missing / unwired | Evidence |
|---|---|---|---|
| Zoom | robust `Viewport::zoom_at` | no wheel/input handler | `crates/node-graph-core/src/lib.rs:144-172`; `crates/gpui-node-graph/src/lib.rs:186-243` |
| Box selection | `nodes_in_rect` query | no marquee or invocation | `crates/node-graph-core/src/lib.rs:596-607` |
| Fit view | `bounds` query | no fit calculation/command/UI | `crates/node-graph-core/src/lib.rs:608-630` |
| Connection compatibility | directional typed predicate | no rendered sockets, drag, snap, or request/create | `crates/node-graph-core/src/lib.rs:584-595`; `crates/gpui-node-graph/src/lib.rs:119-204` |
| Connection events | core request/removal variants | request never emitted; view has neither variant | `crates/node-graph-core/src/lib.rs:216-241`; `crates/gpui-node-graph/src/lib.rs:10-24` |
| Connection selection | transient set modeled | no rendering/hit test/mutation UI | `crates/node-graph-core/src/lib.rs:260-275`; `crates/gpui-node-graph/src/lib.rs:124-151` |
| Delete | cascading `remove_nodes` | no keyboard/action/view adapter; demo never calls it | `crates/node-graph-core/src/lib.rs:531-582`; `examples/demo/src/main.rs:13-88` |
| Undo/redo | event enum variants only | no history, commands, emission, or UI | `crates/node-graph-core/src/lib.rs:232-241` |
| Routing | simple four-point helper | renderer duplicates it; no obstacles/cache/subway routing | `crates/node-graph-core/src/lib.rs:632-635`; `crates/gpui-node-graph/src/lib.rs:139-149` |
| Persistence | serde snapshot/state projections | no host loader/saver/storage/migrations/real fixtures | `crates/node-graph-core/src/lib.rs:242-349`; `README.md:26`, `README.md:38` |
| Reconciliation/events | public view APIs and emitter | demo never subscribes/calls them | `crates/gpui-node-graph/src/lib.rs:56-117`; `examples/demo/src/main.rs:23-84` |
| Detached windows | native-only service/capability API | demo has no detached-window workflow | `crates/gpui-node-graph/src/windows.rs:21-45`; `examples/demo/src/main.rs:13-88` |
| Theme | five public colors | demo has no theming UI; much styling hard-coded | `crates/gpui-node-graph/src/lib.rs:25-42`, `crates/gpui-node-graph/src/lib.rs:143-185` |
| Dynamic graph | public generic maps/types | demo hard-codes a fixed graph; no catalog/menu/add APIs | `examples/demo/src/main.rs:25-82`; roadmap `README.md:36` |
| Advanced editor | roadmap names grid, groups, resize, overlays, culling, keyboard | none appears in view implementation | `README.md:33-39`; `crates/gpui-node-graph/src/lib.rs:119-244` |

## Bottom line for comparison

Treat the reusable implemented baseline as: **generic serde model + trust-boundary validation/canonicalization + transient selection/viewport split + safe transforms + basic geometry queries + atomic node/port translation and cascading delete + minimal GPUI node/wire painting + singleton node drag + middle-pan + four editor notifications + shared native/Wasm launch and browser isolation scaffolding**.

Do not infer parity from the broader enum/type vocabulary. Connection requests, undo/redo, connection selection, viewport zoom, selection geometry, bounds, node removal, reconciliation, detached windows, and persistence DTOs are wholly or partly **library-only APIs** with no demo UI wiring. The visible program remains a fixed three-node demonstration with two wires and only node drag/middle-pan interaction (`examples/demo/src/main.rs:25-82`, `crates/gpui-node-graph/src/lib.rs:186-243`).
