# gpui-node-graph

Cross-platform node graph editor on official Zed GPUI, pinned to `zed-industries/zed@08827f9208b4848d62f3faf86ffa15155966d63c`. This is the standalone GPUI port of the audited Leptos graph. One shared GPUI view runs on desktop and WebAssembly without Leptos or DOM rendering.

## Ecosystem theme API convention

GPUI libraries use `set_<crate>_theme(cx, theme)` for installation and
`Active<Crate>Theme::<crate>_theme(&self)` for ambient access. The backing
`Global<Crate>Theme(Arc<CrateTheme>)` remains private. Setters do not refresh or
fall back; applications batch all crate-theme installs and refresh once.

## Status

The production foundation includes a serde-compatible framework-free domain snapshot, separately managed transient editor state, generic typed IDs, validated/canonical graph references, directional port compatibility, safe viewport transforms, geometry and selection queries, deterministic orthogonal routes, and a shared GPUI view. The shared editor now renders typed ports and supports draft/snap/complete/reroute gestures, wire and box selection, modifier multi-selection and batched multi-node drag, pointer-centered wheel zoom, grid snap, fit view, keyboard editing hooks, and middle/Ctrl-drag panning. The same view code runs on Windows, macOS, Linux, and WebAssembly.

```sh
cargo run -p gpui-node-graph-demo
# Force Wayland when both Wayland and X11 session variables are present
# env -u DISPLAY cargo run -p gpui-node-graph-demo
cargo test --workspace

# Demo controls: drag nodes; Shift-click or drag the canvas to multi-select;
# drag/click between ports to connect; drag a wire to blank space to create-and-connect;
# double-click blank space or press Tab to search/create; click wires and press Delete;
# drag a node's right edge to resize (double-click resets); wheel zoom;
# middle-drag or Ctrl-drag to pan; Ctrl/Cmd+G groups a multi-selection;
# Multi-select nodes to show alignment/distribution plus inferred-grid Tidy above the selection;
# connecting across a multi-selection pairs inferred columns/types by vertical rank, or fans the
# selected endpoint side when no pair mapping applies; mapped disconnects are symmetric;
# Blend/Custom selects expose mouse and keyboard option choice; Factor uses native/IME text input;
# right-click ports for connection actions; hover ports for typed tooltips;
# R cycles subway/Bezier/simple routing; F fits; Escape rolls back/cancels/clears.

# Browser host (requires Trunk)
cd examples/demo && trunk serve
# verify the required cross-origin-isolation headers
curl -sSI http://127.0.0.1:8181/ | grep -Ei 'cross-origin-(opener|embedder)-policy'
```

## Theming

`NodeGraphTheme` is the single complete look configuration. It contains the component-level
`EditorStyle`, `NodeStyle`, `AnchorStyle`, `ConnectionStyle`, `SelectionBoxStyle`, `GroupStyle`,
`MenuStyle`, and `OverlayStyle` records.

Install actions and the required ambient theme before opening or rendering a node graph:

```rust
use gpui_node_graph::{init, set_node_graph_theme, NodeGraph, NodeGraphTheme};
use std::sync::Arc;

init(cx);
let mut theme = NodeGraphTheme::dark();
theme.node.border_radius = 6.0;
let theme = Arc::new(theme);
set_node_graph_theme(cx, Arc::clone(&theme));
```

`set_node_graph_theme` installs or replaces the application global without refreshing windows, so
applications can replace several ambient themes and call `App::refresh_windows` once. Rendering
without an installed theme panics rather than silently choosing a fallback. The root reads the
immutable `Arc<NodeGraphTheme>` once per render and passes that snapshot to layout and leaf
renderers.

The value-returning `NodeGraphTheme::light()`, `dark()`, `system(window_appearance)`, and
`leptos_demo()` constructors remain available for customization.

Automatic multi-connect uses registry port keys when available and otherwise matches port labels.
Consumers with encoded or localized port IDs can install an exact logical-key extractor with
`NodeGraph::with_port_match_key`. Set `EditorConfig::auto_multi_connect = false` to retain strictly
singular connection requests. `multi_connection_specs`, `multi_connection_ids_for`, and
`multi_connection_preview` expose the same policy for host integrations.

## Architecture

- `node-graph-core`: canonical serializable model and deterministic logic. Persist `GraphSnapshot`; `GraphUiState` (selection and viewport) is session-only. `GraphState` still accepts the former top-level domain JSON shape, but deliberately skips transient fields when serialized. Call `validate`, `canonicalize_ids`, or `GraphState::from_snapshot` at trust boundaries. Create explicit caller-owned editor state with `cx.new(|cx| NodeGraph::new(graph, cx))`; `NodeGraph::try_new(graph, cx)` rejects invalid state.
- `gpui-node-graph`: one generic `NodeGraph<T, N, P, C>` view and input adapter. `WorldNodeBodyRenderer` is the canonical affine world-space path: it authors an immutable world-unit display list without access to zoom/pan, GPUI projects the completed primitives at paint time, and pointer input is inverse-transformed before hit testing, dragging, resizing, marquee selection, or port gestures. It receives live connected/source/snap/compatibility state, and pane-space menus and measured overlays remain unscaled while semantic control anchors follow the projection. World text controls can register GPUI's real `InputHandler` for UTF-16 selection, composition/IME, marked text, and clipboard insertion. The older `NodeBodyRenderer` is a compatibility path for retained GPUI controls, but pinned GPUI has no arbitrary retained-subtree affine transform; consumers requiring zoom-stable parity should use the world display list. The adapter emits the core's single typed `GraphEvent` vocabulary directly. Measured geometry stays transient and node-relative, preserving strict snapshots. Dynamic-port removal uses dashed transient wire tombstones and can restore the original strict connection when the same stable port ID returns; controlled removal/restoration emits atomic mutations instead of taking model ownership.

`EditorConfig::mutation_mode` makes ownership explicit. `Uncontrolled` commits move/resize/delete/group previews locally and emits compatibility events. `Controlled` rolls persistent previews back and emits one atomic `GraphEvent::MutationRequested` batch of `GraphMutation` operations for the host to reconcile. Selection and viewport remain transient editor state in either mode; connection creation always remains a request because generic consumer IDs cannot be synthesized safely by the view.
- `examples/demo`: shared GPUI application with thin target-specific desktop/browser packaging. `gpui_platform::application()` selects the backend.

All saved-model migrations belong in core and must have fixture round-trip tests. Leptos is a behavior/serialization reference during migration only, not a co-maintained target.

## Parity roadmap

The evidence-backed Leptos parity audit and phased acceptance plan are in
[`audits/leptos-parity-audit.md`](audits/leptos-parity-audit.md).

- [x] portable typed model, viewport transforms, selection geometry
- [x] native nodes and connection painting
- [x] node drag event and middle-button pan
- [x] wheel zoom around cursor, grid snap, box/connection selection, fit view
- [x] typed draft connections, snapping and remove/create events
- [x] obstacle-aware subway router and deterministic batch route cache
- [x] dynamic ports and searchable, keyboard-navigable node catalog
- [x] groups, resizing, overlays, culling and keyboard actions
- [x] persisted-graph fixtures and full browser interaction replay tests
- [x] audited multi-state visual regression suite and stateful browser interaction trace
- [x] one-window browser architecture and native-only detached-window capability boundary

## Platform support

First-class Windows, macOS, Linux, and `wasm32-unknown-unknown` builds are checked in CI. The browser has exactly **one document-owned GPUI window**; Mullion composes every pane and workspace inside that root. `PlatformWindowService` is the only optional detached-window gateway: native builds may open an OS window, while wasm returns `DetachedWindowError::UnavailableInBrowser` without changing shared view code. Browser launch/HTML/COOP+COEP packaging is isolated in the demo host. Web GPUI uses shared memory, so production hosting **must** return `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp` on the document and assets. `Trunk.toml` supplies these locally and `_headers` is copied for Netlify/Cloudflare Pages; for other hosts configure the equivalent response headers (HTML `<meta>` tags are not sufficient). CI serves the production Trunk bundle with both isolation headers and executes it in Google Chrome on an isolated virtual display, asserting cross-origin isolation, successful Wasm startup, and creation of the shared GPUI canvas.

Licensed under Apache-2.0.

## Zed-style API

The completed public UI ownership/action audit is in [`audits/zed-style-api-audit.md`](audits/zed-style-api-audit.md). `WorldSceneElement` is the named `RenderOnce` display-list builder; the graph itself is explicit caller-owned entity state implementing `Render`, `Focusable`, and typed `EventEmitter`.
