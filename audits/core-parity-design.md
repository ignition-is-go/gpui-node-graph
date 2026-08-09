# Core parity design: Leptos algorithms/state vs `node-graph-core`

## Scope and conclusion

This note compares only framework-independent behavior in the Leptos reference with
`crates/node-graph-core`; DOM measurement, Leptos signals, SVG/CSS, RAF scheduling,
mouse-event decoding, and GPUI painting are deliberately out of scope.

The core should **not** make `GraphSnapshot` permissive in order to imitate the
Leptos registry. The smallest sound design is:

1. keep nodes, logical ports, and connections in a strictly validated persisted
   snapshot;
2. add small, non-serialized interaction state and commands for selection and a
   connection draft;
3. add groups as validated document data (with transient hover/drag state outside
   the snapshot);
4. model a temporarily unavailable/render-unmounted port as a presentation
   projection of a valid logical port, not as a missing persisted reference; and
5. transplant the pure subway solver behind indexed DTOs, leaving route caches and
   incremental invalidation transient.

Most geometry, selection queries, compatibility, cascading deletion, and viewport
math already exist in core. The large subway solver is also directly reusable; it
is framework-free already.

## Current state comparison

| Concern | Leptos reference | `node-graph-core` now | Minimal gap |
|---|---|---|---|
| Draft connection | `DraftConnection` holds origin port/position/type/direction, cursor endpoint and optional snap target. Snapping searches the nearest compatible opposite-direction port on another node; configured screen distance is converted to canvas distance. Completion is consumer-owned through `ConnectionRequested`. | Has `compatible_target`, port positions, and `ConnectionRequested`, but no draft state or nearest-target query. | A transient `DraftConnection<P>`, begin/update/cancel/finish commands, and nearest-compatible-port query. Do not persist duplicated source geometry/type. |
| Selection | Separate node/connection sets; exclusive node/wire selection, shift toggle, clear/select-all, intersecting box selection, and reconciliation on deregistration. | Same two transient sets; `nodes_in_rect`, `remove_nodes`, and `reconcile` clean some state, but no coherent selection command API. | A handful of state-mutating selection methods and transient `BoxSelection`; emit one `SelectionChanged` only when the pair of sets changes. |
| Groups | Consumer supplies `GroupBox { id: String, node_ids, label, color, error }`. Bounds are derived from member node rectangles plus padding. Alt-drag removes a node from all groups at drag start and adds it to the first group containing the final node center. Rename/add/remove are requests. | No group data or algorithms. | A validated `Group<N>` map plus pure bounds/hit-test/membership commands. Hover and an in-flight alt drag remain UI state. |
| Subway routing | `subway.rs` is a pure indexed batch solver: sparse-grid A*, obstacle inflation, anchor stubs/escape lanes, bend/crossing/overlap costs, bounded fallbacks, simplification, and lane nudging. `connection.rs` adds a pure geometry batch/cache plan: cached/full/partial solve; partial includes incident wires and frozen routes close to both old and new moved-node rectangles. | `orthogonal_route` is only a midpoint elbow and does not avoid nodes or separate lanes. | Port the pure solver and (optionally, immediately after) its cache planner. Routes/caches/stats are derived transient data, never snapshot fields. |
| Dynamic dangling edges | The reactive registry may remove a rendered port while retaining its consumer connection. Rendering resolves each endpoint to `Option<Position>`: one present endpoint yields a short dashed 30-unit stub and `?`; both missing yields nothing; the wire restores if the port registers again. | Persisted validation correctly rejects any connection whose source/target is absent. `remove_nodes` also cascades actual domain deletion. | Add an endpoint-availability projection based on a set/predicate of currently presented ports. Never admit a dangling connection into `GraphSnapshot`. |

Important reference details:

- Leptos box selection uses rectangle **intersection**, as core already does. Despite
  preserving selection on shift-mousedown, the current mousemove path replaces the
  selected-node set; additive shift-box selection is therefore not reference
  behavior to copy accidentally.
- A Leptos group silently ignores missing member nodes while calculating bounds and
  uses 160x80 fallback dimensions for unmeasured nodes. Core snapshots have real,
  validated non-negative sizes, so persisted groups should reject missing members
  and should use the stored size. A presentation layer may choose fallback measured
  sizes without weakening domain validation.
- Leptos dangling behavior comes from component registration lifetime. It is not
  evidence that saved connections are allowed to reference nonexistent logical
  ports.

## Proposed smallest public core extensions

Names below are illustrative; the ownership boundaries are the important part.

### 1. Transient draft state and commands

```rust
#[derive(Clone, Debug, PartialEq)]
pub struct DraftConnection<P> {
    pub origin: P,
    pub current_end: Point,
    pub snap_target: Option<P>,
}

pub enum FinishDraft<P> {
    Requested { source: P, target: P },
    Cancelled,
}
```

Change `GraphUiState` to `GraphUiState<N, P, C>` and add
`draft_connection: Option<DraftConnection<P>>` and
`box_selection: Option<BoxSelection>`. Add the same `#[serde(skip, default)]`
fields to `GraphState` only if `GraphState` remains the editor aggregate. An even
cleaner later refactor is `GraphState { document, ui }`, but it is not required for
parity and should not block this small change.

Suggested methods:

```rust
begin_draft(&mut self, origin: &P) -> bool
nearest_compatible_port(&self, origin: &P, cursor: Point, radius: f32)
    -> Option<(P, Point)>
update_draft(&mut self, cursor: Point, radius_world: f32) -> bool
cancel_draft(&mut self) -> bool
finish_draft(&mut self, target: Option<&P>) -> Option<GraphEvent<N, P, C>>
```

Rules:

- reject non-finite cursor/radius, missing origin, same-node, same-direction, and
  incompatible candidates;
- normalize completion to output -> input even when the gesture began at an input;
- deterministic equal-distance tie breaking must not depend on `HashMap` order.
  The smallest API can accept an ordered candidate iterator; alternatively add a
  caller-supplied stable key. Do not add `Ord` to every ID trait just for snapping;
- `finish_draft` only emits a request. It must not invent a `C` or insert a
  connection, matching the consumer-owned Leptos workflow;
- the view converts a screen-pixel snap radius with the sanitized viewport before
  calling core. Core should only accept world units.

Do not store origin position, direction, or `T` in the draft: they are authoritative
on the logical port and duplicating them creates stale state. If the origin becomes
unavailable in the presentation projection it may still remain a logical port; if
it is actually removed from the document, reconciliation cancels the draft.

### 2. Selection transaction API

Add `BoxSelection { start: Point, current: Point }` with a normalized `rect()` and:

```rust
select_node(id, SelectionMode) -> Option<GraphEvent<...>>
select_connection(id, SelectionMode) -> Option<GraphEvent<...>>
clear_selection() -> Option<GraphEvent<...>>
select_all_nodes() -> Option<GraphEvent<...>>
set_box_selection(rect, SelectionMode) -> Option<GraphEvent<...>>
```

`SelectionMode` only needs `Replace` and `Toggle` initially. Commands should update
both sets atomically from the caller's perspective and emit no event for a no-op.
`reconcile`, removal of a port/connection, and node deletion must continue pruning
selection. Core already provides the hard parts: `nodes_in_rect` and cascading node
deletion can be reused unchanged.

Avoid a generic "selected item" enum: the two existing sets match the reference,
serialize efficiently as UI state if an application chooses to save a workspace,
and require less churn.

### 3. Validated groups in the persisted document

Use the reference's string group ID to avoid adding a fourth ID generic everywhere:

```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Group<N> {
    pub id: String,
    pub node_ids: Vec<N>,
    pub label: Option<String>,
    pub color: Option<String>,
    pub error: bool,
}

// on GraphSnapshot and GraphState
#[serde(default)]
pub groups: HashMap<String, Group<N>>;
```

Validation should reject a map-key/embedded-ID mismatch, duplicate members inside a
group, and references to missing nodes. Membership in multiple groups is valid in the reference (an alt-drag removes the
node from every current group). Do not invent a cross-group uniqueness invariant.
The transfer command should remove from every group and then add to the selected
target; hit testing must use a documented stable group order when boxes overlap.

Small pure API:

```rust
group_bounds(&self, id: &str, padding: f32) -> Option<Rect>
group_at_node_center(&self, node: &N, padding: f32) -> Option<String>
rename_group(...)
remove_node_from_groups(...)
move_node_to_group(node, Option<&str>)
```

Require finite non-negative padding and use checked/wide geometry so bounds remain
finite. `remove_nodes` should delete member IDs and either retain empty groups or
remove them according to one documented policy (retaining is closer to consumer-
owned Leptos definitions). Group hover, inline-label edit text, and alt-drag phase
are interaction state and must not be serialized.

If changing the snapshot schema is temporarily unacceptable, the same `Group<N>`
and `validate_groups(&GraphState, ...)` can ship as a separate `GroupSnapshot<N>`.
That is source-smaller but leaves persistence atomicity to every consumer; adding a
`#[serde(default)] groups` field is the preferable minimal end state.

### 4. Presentation availability for dangling rendering

Add no relaxed validation mode. Instead expose a derived DTO/helper:

```rust
pub struct ConnectionEndpoints {
    pub source: Option<Point>,
    pub target: Option<Point>,
}

pub enum ConnectionPresentation {
    Complete { source: Point, target: Point },
    Dangling { present: Point, missing: PortDirection },
    Hidden,
}

connection_presentation(
    &self,
    id: &C,
    is_present: impl Fn(&P) -> bool,
) -> Option<ConnectionPresentation>
```

The authoritative graph still contains both ports. `is_present` represents a
conditional/dynamic port's current registration or render availability. The GPUI
adapter can maintain a `HashSet<P>` if it ever virtualizes/dynamically mounts ports;
when it renders the full logical graph it simply passes `true`.

A tiny pure `dangling_stub(presentation, length)` helper may return the same
orientation-neutral geometry as Leptos (30 world units and a label anchor), but
stroke, dash pattern, `?`, and pointer behavior belong in the renderer.

If an application changes its **logical schema** and truly deletes a port, it has
only two valid choices: delete/rewrite dependent connections transactionally, or
hold them in a separate non-persisted `SuspendedConnection` application buffer.
`from_snapshot` and `reconcile` must continue rejecting the invalid candidate
atomically. Never serialize an unresolved endpoint in `Connection<P, C>` and never
add `validate_lenient`.

### 5. Subway routing module and cache

Port `leptos-node-graph/src/subway.rs` as a leaf module with the same indexed shape:

```rust
pub struct SubwayRect { pub x: f64, pub y: f64, pub w: f64, pub h: f64 }
pub struct SubwayConnection {
    pub start: RoutePoint,
    pub end: RoutePoint,
    pub start_rect: Option<usize>,
    pub end_rect: Option<usize>,
}
pub struct SubwayOptions { /* existing tuning */ }
pub struct SubwayRoutingStats { /* existing counters */ }

compute_subway_routes(...)
compute_subway_routes_with_stats(...)
route_intersects_rect(...)
```

Keep routing arithmetic in `f64` so the roughly 68 KB reference algorithm can be
moved almost verbatim and its tolerance/cost behavior is not silently changed.
Provide checked conversions between validated core `Point`/`Rect` (`f32`) and a
small `RoutePoint` (`f64`) at the boundary; reject or saturate non-finite/out-of-range
results before returning render points. Changing all persisted geometry to `f64`
would be much larger and is not necessary.

The indexed API is valuable: it has no ID bounds, no serde impact, and is
deterministic for a deterministic input order. Do **not** build batches directly by
iterating core `HashMap`s. The Leptos adapter sorts `Debug` strings, but distinct IDs
can share the same debug text, so that is not a sufficient core determinism
contract. Accept ordered node/connection slices from the adapter (or a stable-key
callback).

Then port the framework-free cache planner from `connection.rs` as an optional
`SubwayRouter<N, C>`/`SubwayCache<N, C>`:

- identical geometry -> cached;
- structural or endpoint-ownership change -> full solve;
- geometry-only node change -> partial solve of incident routes plus cached routes
  within 48 units of both the old and new obstacle rectangles;
- route cache, previous batch, and stats are transient and excluded from every
  snapshot.

The solver can land before the incremental cache without semantic loss; full batch
solves already produce correct parity. Incremental planning is a performance parity
port and is almost directly reusable once the batch DTO is public/crate-private.

## What can be ported directly

### Direct or mechanical ports

- Entire pure `subway.rs`: A*, sparse grid/heap, obstacle inflation, stubs and escape
  lanes, fallback elbows, occupancy, simplification, overlap clustering/lane
  assignment, diagnostics, and its ten algorithmic tests. Mechanical changes are
  imports and the point type.
- Pure cache planning from `connection.rs`: `SubwayBatch`, subset/diff,
  `incremental_route_ids`, `plan_subway_solve`, and their tests. Batch collection
  should be rewritten to consume stable ordered input rather than Leptos registry
  maps.
- `BoxSelect::to_rect`; core's `Rect`, `nodes_in_rect`, and intersection semantics
  already implement the result.
- Nearest snap search and compatibility ordering; use existing
  `compatible_target` and `Point::distance`, but normalize input-origin drafts.
- Group bounds and center-in-group hit testing; replace the Leptos `NodeEntry` map
  with validated core nodes and omit DOM/unmeasured fallbacks.
- `dangling_geometry`; make it a pure renderer geometry helper over derived endpoint
  availability.

### Already present; wire it rather than porting it

- directional/asymmetric port compatibility and different-node rule;
- node/port/connection geometry DTOs;
- rectangle intersection selection and graph bounds;
- selection storage and selection pruning during reconciliation/deletion;
- translating owned port positions during atomic node movement;
- cascading actual node deletion;
- viewport transforms, cursor-preserving zoom, panning, and finite-value defenses;
- `ConnectionRequested`, `ConnectionRemoved`, `SelectionChanged`, and node movement/
  deletion events.

### Must stay framework-specific

DOM/GPUI hit testing, screen event/modifier decoding, screen-to-world snap-radius
conversion, live port measurement/registration, rendering paths and rounded corners,
CSS/style/labels, menu ownership of a draft, RAF drag batching, and reactive route
notification are adapter/view responsibilities. The core should expose decisions and
geometry, not simulate either Leptos signals or GPUI entities.

## Validation and reconciliation requirements

All new persisted mutations should follow the existing candidate-first pattern:
canonicalize embedded IDs, validate the entire candidate, and swap only on success.
Extend validation with groups, but do not condition it on presentation availability.
Also prune/cancel transient state after a successful reconcile:

- retain only selected existing nodes/connections;
- cancel a draft whose logical origin or snap target no longer exists or is no
  longer compatible;
- clear a box gesture (coordinates may belong to the old document);
- invalidate all route cache geometry on a structural reconcile;
- prune group hover/drag presentation state in the view.

Tests required before production implementation:

1. snapshots with dangling connections or missing group members are rejected and
   reconciliation is atomic;
2. presentation-unmounted endpoints produce complete/dangling/hidden states without
   changing `validate()` or `snapshot()`;
3. draft snapping is directional, asymmetric, cross-node, radius-bounded, finite,
   deterministic on ties, and completion normalizes output -> input;
4. all selection commands emit once on change and never on no-op; deletion/reconcile
   prune selections and drafts;
5. group key IDs/membership are validated, bounds are finite, and a group transfer
   is atomic;
6. transplant the reference subway tests plus conversion/extreme-value tests and
   stable-order/cache invalidation tests.

## Recommended implementation order

1. Selection commands and `BoxSelection` (tiny, using existing primitives).
2. Draft state, snapping, and finish request (restores the core authoring decision
   loop without persistence changes).
3. Endpoint-availability projection (enables dynamic dangling parity without
   compromising snapshots).
4. Pure subway solver, initially full-batch; then cache planner.
5. Validated groups and group commands (only item that intentionally extends the
   persisted schema).

This is the smallest extension set that gives both UI adapters the same decisions
while preserving `node-graph-core`'s strongest advantage over the Leptos registry:
a saved graph is either fully valid or is not admitted at all.
