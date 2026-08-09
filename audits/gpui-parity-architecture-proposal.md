# GPUI parity architecture proposal

## Decision

Keep `node-graph-core` framework-free, but turn `gpui-node-graph` into a **graph shell plus a consumer delegate**. The shell owns hit testing, transforms, selection, gestures, port layout, wire layers, menus, and transient previews; the delegate supplies the catalog, creates domain records, and renders arbitrary GPUI node bodies. Do not encode controls in the core DTO and do not add one enum per layer.

This design is compatible with the pinned Zed revision `08827f9208b4848d62f3faf86ffa15155966d63c`: `Render::render` returns `impl IntoElement` (`crates/gpui/src/element.rs`), heterogeneous children erase with `IntoElement::into_any_element() -> AnyElement`, retained controls are `Entity<V>` children, callbacks can hold `cx.entity().downgrade()` and call `WeakEntity::update`, and shell handlers use `cx.listener`. Keep wires/drafts in `canvas`; keep nodes, ports, menus, and controls as normal interactive elements. This avoids a bespoke scene/widget system and works in `gpui_web` as well as native GPUI.

## Public data model

Make type and payload explicit. Defaults can preserve the simple-string use case.

```rust
pub trait NodeTypeId: Clone + Eq + Hash + Debug + 'static {}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node<N, K = String, D = ()> {
    pub id: N,
    pub kind: K,
    pub title: String,             // instance override / accessible label
    pub data: D,                   // application data, never GPUI state
    pub position: Point,
    pub size: NodeSize,            // Auto or Fixed(Size)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Port<N, P, T> {
    pub id: P,
    pub node: N,
    pub label: String,
    pub direction: PortDirection,
    pub kind: T,
    pub slot: PortSlot,             // Header, BodyRow(u32), Footer, Custom(u32)
    pub active: bool,
    pub last_offset: Point,         // measured node-relative fallback/tombstone anchor
}

pub enum NodeSize { Auto, Fixed(Size) }

pub struct NodeBundle<N, P, K, D, T> {
    pub node: Node<N, K, D>,
    pub ports: Vec<Port<N, P, T>>,
}
```

`GraphSnapshot` becomes `GraphSnapshot<N,P,C,K,D,T>`. Connections may reference an **inactive** port, but never a nonexistent port. When a dynamic port disappears, mark it inactive instead of deleting it while referenced; render a dangling stub at `last_offset`. Reactivating the same stable `P` restores the wire. A compaction operation may delete unreferenced inactive tombstones. Thus validation remains strict about IDs/references while matching the reference's dynamic-port restoration behavior.

All maps become private. Expose read-only accessors and reducer operations; this removes today's bypass around validation and events.

## Catalog and rich rendering seam

Use a generic delegate, not `dyn Render` (which is not object-safe because it returns `impl IntoElement`). The delegate can itself contain heterogeneous `Entity<V>` values or match on `kind`, and returns `AnyElement` at the one intentional erasure boundary.

```rust
#[derive(Clone)]
pub struct NodeTypeDescriptor<K, T> {
    pub id: K,
    pub label: SharedString,
    pub category: SharedString,
    pub description: SharedString,
    pub keywords: Arc<[SharedString]>,
    pub inputs: Arc<[PortTemplate<T>]>,
    pub outputs: Arc<[PortTemplate<T>]>,
}

pub struct NodeRenderContext<Del: NodeGraphDelegate> {
    pub node: Node<Del::N, Del::K, Del::D>,
    pub ports: Arc<[Port<Del::N, Del::P, Del::T>]>,
    pub selected: bool,
    pub visible: bool,
    pub zoom: f32,
    pub commands: GraphCommandSink<Del>,
}

pub trait NodeGraphDelegate: Sized + 'static {
    type N: NodeId;
    type P: PortId;
    type C: ConnectionId;
    type K: NodeTypeId;
    type D: Clone + Debug + 'static;
    type T: PortType;

    fn node_types(&self) -> &[NodeTypeDescriptor<Self::K, Self::T>];

    // Called after menu selection in both ownership modes, so IDs and initial
    // application data are generated exactly once.
    fn instantiate(
        &mut self,
        kind: &Self::K,
        at: Point,
        auto_connect: Option<DraftEndpoint<Self::P>>,
        cx: &mut App,
    ) -> Result<NodeBundle<Self::N, Self::P, Self::K, Self::D, Self::T>, CreateNodeError>;

    // May return divs directly or retained Entity<MyNodeBody> children.
    fn make_connection(
        &mut self,
        source: &Self::P,
        target: &Self::P,
        cx: &mut App,
    ) -> Result<Connection<Self::P, Self::C>, ConnectError>;

    fn render_node(
        &mut self,
        context: NodeRenderContext<Self>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;
}

pub type GraphChangeOf<D> = GraphChange<
    <D as NodeGraphDelegate>::N, <D as NodeGraphDelegate>::P,
    <D as NodeGraphDelegate>::C, <D as NodeGraphDelegate>::K,
    <D as NodeGraphDelegate>::D, <D as NodeGraphDelegate>::T,
>;

pub struct GraphCommandSink<Del: NodeGraphDelegate> {
    graph: WeakEntity<NodeGraph<Del>>,
}
impl<Del: NodeGraphDelegate> GraphCommandSink<Del> {
    pub fn dispatch(
        &self,
        change: GraphChangeOf<Del>,
        cx: &mut App,
    ) -> Result<TransactionId, EntityNotFound>;
}
```

`NodeGraph::render` builds a positioned/scaled shell, calls `delegate.render_node(...)`, and places shell-owned input/output rows and sockets around that body. Consumer controls call `cx.stop_propagation()` for mouse handlers they consume; the shell must not infer this from element types. Retained text inputs/selects therefore keep focus and state and do not start a node drag.

The catalog menu is shell-owned (`MenuState { query, screen_anchor, draft }`), opened by Tab or blank-canvas double click. It filters descriptor metadata and, during a draft, `PortType::compatible`. Selection calls `instantiate`, then dispatches one `GraphChange::InsertBundle`; auto-connect is part of that same transaction.

### Dynamic ports and geometry

A body control dispatches `GraphChange::SetNodePorts { node, ports }` (or the higher-level `UpdateNodeData`, whose application calls a delegate port derivation hook). The reducer performs an atomic diff: add/reactivate current stable IDs, inactivate removed referenced IDs, delete removed unreferenced IDs, and reject duplicate/wrong-owner IDs.

Sockets are shell elements keyed by `(node_id, port_id)`. Add a small `Measured` custom GPUI `Element` wrapper using the pinned `Element::{request_layout, prepaint, paint}` lifecycle. In `prepaint`, record the node bounds and socket center in a frame-local geometry table; publish changed world-space `size`/`last_offset` after the frame via the graph's `WeakEntity` and `cx.notify()`. Paint wires from the previous complete table, so layout never mutates while rendering. `NodeSize::Fixed` constrains the shell; `Auto` accepts measured size. This provides live rich-content geometry without forcing consumers to manually calculate absolute port positions.

## One mutation and event vocabulary

Delete `EditorEvent` and use a single public vocabulary end-to-end. Core reducers consume `GraphChange`; GPUI emits only `GraphEvent`, including commands that intentionally remain host-owned.

```rust
pub type TransactionId = u64;

#[derive(Clone, Debug)]
pub enum GraphChange<N, P, C, K, D, T> {
    InsertBundle { bundle: NodeBundle<N, P, K, D, T>, connect: Option<(P, P)> },
    MoveNodes { nodes: Vec<(N, Point)> },
    ResizeNode { id: N, size: NodeSize },
    UpdateNodeData { id: N, data: D },
    SetNodePorts { node: N, ports: Vec<Port<N, P, T>> },
    InsertConnection { connection: Connection<P, C> },
    RemoveConnections { ids: Vec<C> },
    DeleteNodes { ids: Vec<N> },
    SetSelection { nodes: HashSet<N>, connections: HashSet<C> },
    SetViewport { viewport: Viewport },
}

#[derive(Clone, Debug)]
pub enum GraphCommand<N> { Copy, Paste, Undo, Redo, CreateGroup { nodes: Vec<N> } }

#[derive(Clone, Debug)]
pub enum GraphEvent<N, P, C, K, D, T> {
    Change { id: TransactionId, stage: ChangeStage, origin: ChangeOrigin,
             change: GraphChange<N, P, C, K, D, T> },
    Command { origin: ChangeOrigin, command: GraphCommand<N> },
    Reconciled { revision: u64 },
    Rejected { id: TransactionId, error: GraphMutationError },
}
pub enum ChangeStage { Requested, Applied }
pub enum ChangeOrigin { Pointer, Keyboard, Menu, ConsumerApi }

impl<Del: NodeGraphDelegate> EventEmitter<GraphEvent<...>> for NodeGraph<Del> {}
```

There is no parallel `NodeMoved` editor event and no special callback API. A drag maintains transient preview positions, then submits one ordered `MoveNodes` on mouse-up. Connection draft completion validates compatibility, calls `delegate.make_connection` once, and submits `InsertConnection`; therefore both ownership modes observe the same consumer-defined `C`. Selection and viewport use the same change vocabulary, although they remain transient and are always applied locally.

## Controlled and uncontrolled ownership

```rust
pub enum GraphOwnership { Uncontrolled, Controlled }

impl<Del: NodeGraphDelegate> NodeGraph<Del> {
    pub fn try_new(snapshot: GraphSnapshotOf<Del>, delegate: Del,
                   ownership: GraphOwnership) -> Result<Self, GraphValidationError>;

    pub fn submit(&mut self, change: GraphChangeOf<Del>, origin: ChangeOrigin,
                  cx: &mut Context<Self>) -> Result<TransactionId, GraphMutationError>;

    // Controlled host acknowledgement/rejection boundary.
    pub fn reconcile(&mut self, revision: u64, snapshot: GraphSnapshotOf<Del>,
                     acknowledged: &[TransactionId], cx: &mut Context<Self>)
        -> Result<(), GraphValidationError>;

    // Explicit application is useful for a controlled host in the same process.
    pub fn apply(&mut self, id: TransactionId, change: GraphChangeOf<Del>,
                 cx: &mut Context<Self>) -> Result<(), GraphMutationError>;

    pub fn snapshot(&self) -> GraphSnapshotOf<Del>;
    pub fn ui_state(&self) -> &GraphUiState<Del::N, Del::C>;
}
```

* **Uncontrolled:** validate/reduce atomically, emit `Change { Applied }`, notify.
* **Controlled:** validate against the current snapshot, retain an optimistic gesture preview, emit `Change { Requested }`, and do not mutate persisted domain maps. The host sends a new monotonically numbered snapshot plus acknowledged transaction IDs. Reconciliation clears acknowledged previews and preserves valid selection/viewport.
* Invalid changes emit/return `Rejected`; no partial mutation occurs. Programmatic host reconciliation never masquerades as a user change.

This makes ownership a construction policy rather than two editor implementations. The same gesture code always calls `submit`.

## Render/interaction layering

Render in this order: background/grid; groups; normal wires; selected/draft wire canvas; node shells and rich bodies; sockets/hit targets; selection rectangle; unscaled pane-clipped overlay/menu layer. Use screen-pixel socket hit radii and wire widths independent of zoom. Keep a geometry registry and spatial index in view state rather than persisting screen coordinates. Add one focused root key context for Delete, select-all, fit, Escape, Tab, copy/paste, undo/redo; focused consumer controls stop propagation. Pointer state is one enum (`Idle | Pan | BoxSelect | Move | Resize | Connect`) so gestures cannot overlap.

## Migration plan

1. **Unify vocabulary first.** Move the useful cases from current core `GraphEvent` into `GraphChange`; replace `EditorEvent` and adapt current move/pan/select code to `submit`. Add reducer tests for atomicity and deterministic ordering.
2. **Encapsulate state and add ownership.** Privatize maps, implement uncontrolled mode, then controlled request/reconcile/ack tests. Keep deprecated `graph()` and `set_graph()` adapters for one release.
3. **Evolve DTOs.** Add `K`, `D`, `NodeSize`, `PortSlot`, `active`, and `last_offset`, with serde defaults and a `LegacySnapshot -> GraphSnapshot` conversion (`kind = "default"`, `data = ()`, existing absolute port positions converted to offsets).
4. **Add `NodeGraphDelegate` and shell rendering.** Ship `DefaultDelegate` that reproduces today's title rectangle, so existing demos migrate before rich controls. Add a fixture whose delegate embeds retained GPUI text input/select entities.
5. **Add measured geometry and visible sockets.** Introduce `Measured`, geometry registry, declarative port rows, compatibility styling, draft canvas, click/drag completion, snap, selected-wire removal, and inactive-port stubs. Test port diff/tombstone restoration in core and hit testing in GPUI.
6. **Add catalog menu.** Implement search, Tab/double-click placement, compatibility filtering, `instantiate`, and atomic create-and-connect. Replace the hard-coded demo with five catalog types including a 0–8 dynamic-port node.
7. **Close basic authoring UX.** Add multi/box/wire selection, batched multi-node drag, wheel zoom, fit, delete, focus-safe shortcuts, resize, and overlay layer. Only then tackle obstacle routing/groups and visual/performance polish.
8. **Remove compatibility APIs** after the demo and downstream consumer use `NodeGraph<Delegate>`, controlled/uncontrolled tests cover identical gestures, and no code matches `EditorEvent`.

Acceptance for the parity milestone is a desktop and browser fixture that can search/create a type, render and edit a rich body control without dragging, change dynamic port count while retaining/restoring a dangling connection, create/remove typed wires, and replay the exact same `GraphEvent` trace in controlled and uncontrolled modes (differing only in `ChangeStage`).
