# gpui-node-graph

Cross-platform node graph editor on official Zed GPUI, pinned to `zed-industries/zed@08827f9208b4848d62f3faf86ffa15155966d63c`. This is the standalone replacement for the Leptos graph used during the Rship migration. One shared GPUI view runs on desktop and WebAssembly without Leptos or DOM rendering.

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
# R cycles subway/Bezier/simple routing; F fits; Escape rolls back/cancels/clears.

# Browser host (requires Trunk)
cd examples/demo && trunk serve
# verify the required cross-origin-isolation headers
curl -sSI http://127.0.0.1:8181/ | grep -Ei 'cross-origin-(opener|embedder)-policy'
```

## Architecture

- `node-graph-core`: canonical serializable model and deterministic logic. Persist `GraphSnapshot`; `GraphUiState` (selection and viewport) is session-only. `GraphState` still accepts the former top-level domain JSON shape, but deliberately skips transient fields when serialized. Call `validate`, `canonicalize_ids`, or `GraphState::from_snapshot` at trust boundaries. `NodeGraph::try_new` rejects invalid editor state; `NodeGraph::new` is the convenience constructor for already-trusted state and panics with the validation error rather than silently rendering a corrupt graph.
- `gpui-node-graph`: one generic `NodeGraph<T, N, P, C>` retained-mode view and input adapter. The adapter and core share one `GraphEvent` vocabulary (`EditorEvent` is a compatibility alias). Consumers may reconcile domain data, supply arbitrary `NodeBodyRenderer` GPUI element trees with shell-interactive measured port anchors and unscaled overlays, and handle connection/create/history hooks while the view owns transient gestures. Measured geometry stays transient and node-relative, preserving strict snapshots while following movement immediately.
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
- [ ] obstacle-aware subway router and incremental cache migration
- [ ] dynamic port/node catalog, searchable creation menu
- [ ] groups, resizing, overlays, culling and keyboard actions
- [ ] Rship persisted-graph fixtures and full interaction replay tests
- [ ] visual regression/performance suite
- [x] one-window browser architecture and native-only detached-window capability boundary

## Platform support

First-class Windows, macOS, Linux, and `wasm32-unknown-unknown` builds are checked in CI. The browser has exactly **one document-owned GPUI window**; Mullion composes every pane and workspace inside that root. `PlatformWindowService` is the only optional detached-window gateway: native builds may open an OS window, while wasm returns `DetachedWindowError::UnavailableInBrowser` without changing shared view code. Browser launch/HTML/COOP+COEP packaging is isolated in the demo host. Web GPUI uses shared memory, so production hosting **must** return `Cross-Origin-Opener-Policy: same-origin` and `Cross-Origin-Embedder-Policy: require-corp` on the document and assets. `Trunk.toml` supplies these locally and `_headers` is copied for Netlify/Cloudflare Pages; for other hosts configure the equivalent response headers (HTML `<meta>` tags are not sufficient). CI serves the production Trunk bundle with both isolation headers and executes it in Google Chrome on an isolated virtual display, asserting cross-origin isolation, successful Wasm startup, and creation of the shared GPUI canvas.

Licensed under Apache-2.0.
