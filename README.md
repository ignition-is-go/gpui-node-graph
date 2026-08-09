# gpui-node-graph

Native, cross-platform node graph editor for **GPUI 0.2.2**. This is the standalone replacement for the browser/Leptos graph used during the Rship desktop migration; it does not depend on Leptos, WASM, a DOM, or a browser runtime.

## Status

The initial production foundation includes a serde-compatible framework-free model, typed port compatibility, geometry/viewport and selection queries, deterministic orthogonal routes, a native GPUI view, painted connections, positioned nodes, selection, node dragging with events, and middle-button panning. The demo runs on Windows, macOS and Linux.

```sh
cargo run -p gpui-node-graph-demo
cargo test --workspace
```

## Architecture

- `node-graph-core`: canonical serializable model and deterministic logic. Keep persisted Rship graph DTOs here; UI state must not leak into serialization.
- `gpui-node-graph`: native retained-mode view and input adapter. Consumers own/reconcile domain data and can subscribe to `EditorEvent`.
- `examples/demo`: minimal native smoke application.

All saved-model migrations belong in core and must have fixture round-trip tests. Leptos is a behavior/serialization reference during migration only, not a co-maintained target.

## Parity roadmap

- [x] portable typed model, viewport transforms, selection geometry
- [x] native nodes and connection painting
- [x] node drag event and middle-button pan
- [ ] wheel zoom around cursor, grid snap, box/connection selection, fit view
- [ ] typed draft connections, snapping and remove/create events
- [ ] obstacle-aware subway router and incremental cache migration
- [ ] dynamic port/node catalog, searchable creation menu
- [ ] groups, resizing, overlays, culling and keyboard actions
- [ ] Rship persisted-graph fixtures and full interaction replay tests
- [ ] visual regression/performance suite

## Platform support

First-class Windows, macOS and Linux are checked in CI. Platform-specific behavior must be isolated behind GPUI APIs; browser-only behavior is intentionally out of scope.

Licensed under Apache-2.0.
