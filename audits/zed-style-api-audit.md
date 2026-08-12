# Zed-style public UI API audit

This audit records the completed migration against the pinned Zed GPUI revision
`08827f9208b4848d62f3faf86ffa15155966d63c` and the A/B/C/D classification in
`/home/trevor/pulse-gpui/ZED_MIGRATION.md`.

## Result

| Public surface | Class | Ownership and identity |
|---|---:|---|
| `WorldSceneElement` | A | `#[derive(IntoElement)]` + `RenderOnce`; caller supplies a stable `ElementId`; projection is deferred until render. |
| `NodeGraph<T, N, P, C>` | C | Caller creates `cx.new(|cx| NodeGraph::new(graph, cx))`; it implements `Render`, `Focusable`, `InputHandler`, and typed `EventEmitter<GraphEvent<...>>`. The focus handle is created in `new`/`try_new`, never lazily by `render`. |
| `NodeDrop`, `NodeOverlay`, `NodeBody`, catalog/registry/group records, renderer traits | D | Typed data, callbacks, registry builders, or render adapters; none masquerades as a GPUI component or owns hidden semantic state. |
| `world::*`, `layout::*`, `style::*`, `windows::*` | D | Renderer-neutral display lists/geometry, complete ambient theme records, and explicit platform capability services. |
| `EditorHandle` | D | Deliberately weak consumer bridge; it cannot keep a closed caller-owned editor alive. |

## API decisions

- Removed the context-free `NodeGraph::new`, `NodeGraph::new_in` split. `new` and `try_new`
  both require `&mut Context<Self>`, making entity/focus ownership unambiguous.
- Removed the `EditorEvent` compatibility alias. The only event vocabulary is the typed
  `node_graph_core::GraphEvent` re-exported through the structured `core` namespace.
- Removed the unused `NodeMenuStyle` compatibility alias. `MenuStyle` is the one leaf style type;
  `NodeGraphTheme` remains the one complete aggregate.
- Replaced the opaque `world_scene_element(...) -> AnyElement` function with
  `WorldSceneElement::new(id, scene, viewport)`, a named deferred builder.
- Retained the private `GlobalNodeGraphTheme(Arc<NodeGraphTheme>)`. Each root render performs one
  ambient lookup and shares that exact immutable `Arc` snapshot. There are no public themed-div,
  provider, or partial-style wrapper components.
- Interactive catalog rows now derive element identity from stable catalog item IDs. Alignment
  controls use semantic action keys rather than list positions.

## Actions and accessibility

`actions` contains the public `node_graph::*` typed action family. `init(&mut App)` installs default
bindings in the `NodeGraph` key context; hosts may instead bind the same public actions themselves.
Delete/select-all/routing/fit/cancel/copy/paste/undo/redo/group/ungroup and every alignment command
converge on the same semantic action implementation. Raw key handling remains only at the real `InputHandler` boundary for IME/composition,
consumer-defined world controls, and Tab focus traversal; Tab-triggered catalog opening calls the same
typed action implementation.

The editor exposes a labelled group, an eager tab-stop focus handle, a toolbar role and named button
actions, menu/combobox semantics, active descendants, and projected roles/values/ranges for world
controls. Pointer and AccessKit click activation share callbacks.

## Performance and platform invariants

The migration does not alter culling, visibility margin, immutable world display lists, affine
projection, route caching, subway batching, measured geometry, overlay projection, or inverse hit
testing. Port/draft/reroute interactions and controlled/uncontrolled transaction boundaries remain
typed. Native detached windows and the one-window WASM architecture remain isolated in `windows`
and the demo host respectively.

## Public inventory checked

The audit covered every `pub struct`, `pub enum`, `pub trait`, `pub type`, and `pub fn` in both
workspace crates. Core graph values and algorithms are framework-free D APIs. GPUI-facing records
are typed DTOs/builders or explicit caller-owned state; there are no PascalCase function
constructors, `Stateful<S>` compatibility wrappers, public theme providers, implicit keyed entities,
or public opaque closure components.
