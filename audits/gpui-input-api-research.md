# GPUI input/API research for `gpui-node-graph`

## Scope and source of truth

This audit is against the workspace-pinned Zed commit **`08827f9208b4848d62f3faf86ffa15155966d63c`** (`Cargo.toml`) and its Cargo checkout at:

`~/.cargo/git/checkouts/zed-a70e2ad075855582/08827f9`

Line references below are to that checkout. This matters: several of these APIs, notably explicit pointer capture, are revision-specific. The current graph package depends only on `gpui`, not Zed's `ui`, `picker`, or `fuzzy` crates (`crates/gpui-node-graph/Cargo.toml`). Recommendations distinguish core-GPUI APIs from Zed UI conveniences.

## Recommended input architecture

1. Give the graph view one retained `FocusHandle`; track it on the root, focus it on graph mouse-down, and attach a graph-specific `key_context` and action listeners to that same root.
2. Keep canvas paint and hit testing separate. The canvas is a low-level paint callback, not a retained scene/hit-test tree. Continue using positioned `div`s for nodes/ports that need ordinary interaction; analytically hit-test wires/background in graph-local coordinates.
3. For a drag that must continue outside the node/root, use GPUI typed drag (`on_drag` + `on_drag_move`) or a custom element plus `Window::capture_pointer`. The current root-level `on_mouse_move`/`on_mouse_up` alone is hover-gated and is therefore not sufficient outside the root.
4. Put pan/zoom in the viewport/domain layer. Normalize wheel line/pixel deltas and zoom about `event.position` converted to graph-local coordinates.
5. Keep menus renderer-agnostic at the library boundary (callbacks/actions/events). An application that already uses Zed `ui`/`picker` can adapt those; do not force those large internal crates on the core widget merely to obtain menus.

## Focus, actions, and keyboard bindings

### Exact APIs

* Allocate once in the entity constructor with `cx.focus_handle()` and retain the result. `App::focus_handle()` is defined at `crates/gpui/src/app.rs:2607-2612`.
* Implement `gpui::Focusable` as `fn focus_handle(&self, cx: &App) -> FocusHandle` (`crates/gpui/src/window.rs:673-688`). Return a clone of the retained handle.
* Associate it with the rendered dispatch tree via `.track_focus(&self.focus_handle)` and scope bindings with `.key_context("NodeGraph")`.
* Move focus with `focus_handle.focus(window, cx)` (`window.rs:574-577`). Queries include `is_focused`, `contains_focused`, and `contains` (`window.rs:579-599`). `.tab_stop(true).tab_index(n)` opts into tab navigation (`window.rs:546-563`).
* Declare unit actions with `gpui::actions!(node_graph, [DeleteSelection, SelectAll, ...])`; the macro and namespace behavior are documented/implemented at `crates/gpui/src/action.rs:11-40`. A data-bearing action can derive `gpui::Action`; use `#[action(no_json)]` if the type intentionally does not implement the JSON/schema traits (`action.rs:55-79`).
* Bind application defaults using `cx.bind_keys([KeyBinding::new("delete", DeleteSelection, Some("NodeGraph")), ...])`. `KeyBinding::new(keystrokes, action, context)` and chord parsing are at `crates/gpui/src/keymap/binding.rs:31-74`; `App::bind_keys` is at `app.rs:2164`.
* Handle actions on the focused root with `.on_action(cx.listener(Self::delete_selection))`; `.capture_action(...)` is the capture-phase alternative. The precise fluent signatures are at `elements/div.rs:1030-1053`, and the dispatch-phase behavior is at `elements/div.rs:405-440`. Call `cx.stop_propagation()` only when the graph consumes an action; capture handlers otherwise explicitly propagate (`div.rs:415-421`).

### Canonical pinned example

`crates/gpui/examples/testing.rs:16-38,74-87,180-185` is almost exactly the desired pattern: `actions!`, a stored `FocusHandle` created with `cx.focus_handle()`, `Focusable`, and a root with `.key_context("Counter").on_action(...).track_focus(...)`, plus context-qualified `KeyBinding::new` calls.

Suggested graph skeleton (API names verified above):

```rust
gpui::actions!(node_graph, [DeleteSelection, SelectAll, ZoomIn, ZoomOut]);

pub struct NodeGraph<...> {
    focus_handle: gpui::FocusHandle,
    // ...
}

// in construction from Context<Self>
focus_handle: cx.focus_handle().tab_stop(true),

// in Render
let focus = self.focus_handle.clone();
div()
    .key_context("NodeGraph")
    .track_focus(&self.focus_handle)
    .on_mouse_down(MouseButton::Left, move |_, window, cx| focus.focus(window, cx))
    .on_action(cx.listener(Self::delete_selection))
    .on_action(cx.listener(Self::select_all))
```

Use actions rather than raw `.on_key_down` for commands so host keymaps can override defaults and the bindings follow focus/context dispatch. Raw key events remain available (`elements/div.rs:1069-1090`) for genuine text-like or stateful input.

## Mouse capture and dragging beyond an element

### Why ordinary handlers are insufficient

The fluent `.on_mouse_move` and `.on_mouse_up` callbacks ultimately test `hitbox.is_hovered(window)` in bubble phase (`crates/gpui/src/elements/div.rs:202-219,295-308`). Thus the current graph's root handlers (`crates/gpui-node-graph/src/lib.rs:205-242`) cease receiving ordinary moves/up once the pointer leaves the root/window hit region. `MouseMoveEvent` exposes `position`, `pressed_button`, `modifiers`, and `dragging()` (`interactive.rs:483-509`), but checking the pressed button does not change event routing.

### Option A — typed GPUI drag (preferred for nodes, ports, resize handles)

A stateful element with an `.id(...)` can call:

```rust
.on_drag(DraggedNode { id, grab_offset }, |_, _, _, cx| cx.new(|_| gpui::Empty))
```

and a containing element can register:

```rust
.on_drag_move::<DraggedNode>(cx.listener(Self::drag_node))
.on_drop::<DraggedNode>(cx.listener(Self::finish_drag))
```

`on_drag_move` is explicitly documented to receive **all move events inside or outside the element** after a drag starts (`elements/div.rs:327-357`). `on_drag` is both the drag initiation API and the drag-start counterpart; its constructor returns an `Entity<W: Render>` and receives the click offset (`div.rs:589-615,1551-1569`). A no-preview idiom using `cx.new(|_| gpui::Empty)` appears in `crates/editor/src/split_editor_view.rs:120-140`; the containing resize region handles typed moves/drop at lines 182-211. This is the best pinned example for graph node dragging.

Typed drag has drag-and-drop semantics (including GPUI's initiation behavior). Use it when that is acceptable. A distinct payload type per operation (`DraggedNode<N>`, `DraggedConnection<P>`, `Panning`) prevents handlers from colliding.

### Option B — true pointer capture for immediate manipulation

This revision has:

```rust
window.capture_pointer(hitbox.id);
window.release_pointer();
window.captured_hitbox();
```

`Window::capture_pointer(HitboxId)` routes mouse move/up listeners for that hitbox regardless of actual hit testing, specifically for outside-bounds dragging; capture auto-releases on mouse-up (`crates/gpui/src/window.rs:2828-2850`). `HitboxId::is_hovered` returns true for the captured hitbox (`window.rs:740-767`).

A normal `div` listener is not handed its hitbox ID, and there are no in-tree callers of `capture_pointer` at this commit. Therefore the sound way to use this low-level API is a small custom `Element`/wrapper that:

1. calls `window.insert_hitbox(bounds, HitboxBehavior::Normal)` during `prepaint`;
2. retains the returned `Hitbox` in `PrepaintState`;
3. in `paint`, registers `window.on_mouse_event` and captures `prepaint.hitbox.id` on the desired down event;
4. registers move/up listeners against the same ID, releasing explicitly on cancellation (mouse-up is automatic).

The contracts for `insert_hitbox` are at `window.rs:4693-4711`. Zed's `RightClickMenu` demonstrates the complete custom-element mechanics: child layout/`AnyElement` storage (`crates/ui/src/components/right_click_menu.rs:108-181`), hitbox insertion/prepainting (`:186-210`), and registering a hitbox-ID-aware `window.on_mouse_event` during paint (`:213-297`). Do not try to call `insert_hitbox` from `Render`; it is prepaint-only.

For initial implementation, typed drag is substantially less custom code. Pointer capture is appropriate if graph panning/node movement must begin on the first movement with no DnD initiation threshold.

Also note `.on_mouse_down_out(...)` / `.on_mouse_up_out(...)` exist for dismissal/cancellation, not continuous capture (`elements/div.rs:254-293`).

## Scroll wheel, pan, and zoom

Attach `.on_scroll_wheel(cx.listener(Self::handle_wheel))` to the root. It is bubble-phase and dispatches when the hitbox `should_handle_scroll`, rather than ordinary keyboard-modality hover (`elements/div.rs:360-373`; `window.rs:780-785`). `ScrollWheelEvent` has window position, `ScrollDelta`, modifiers, and touch phase (`interactive.rs:511-533`). Delta is either exact `Pixels(Point<Pixels>)` or inexact `Lines(Point<f32>)`; normalize using:

```rust
let delta = event.delta.pixel_delta(window.line_height());
```

The enum and conversion are at `interactive.rs:543-550,595-610`. Real examples use exactly this normalization in `crates/git_ui/src/git_graph.rs:3551-3574` and `crates/debugger_ui/src/session/running/memory_view.rs:232-240`.

Recommended policy:

* unmodified wheel/trackpad delta pans (`x` and `y`, retaining precise fractional pixels);
* Ctrl/Command + vertical delta zooms about the pointer, or expose policy to the host because platform conventions differ;
* `.on_pinch(...)` is separately available, with `PinchEvent { position, delta, modifiers, phase }` (`interactive.rs:558-585` and `elements/div.rs:1010-1017`);
* call `cx.stop_propagation()` only when the graph actually consumes the event, allowing an outer scroller otherwise.

## Canvas coordinates and hit testing

`canvas(prepaint, paint)` only supplies `Bounds<Pixels>` and arbitrary prepaint state to two `FnOnce` callbacks (`crates/gpui/src/elements/canvas.rs:8-27,37-88`). It does **not** insert a hitbox or provide per-shape hit testing. The pinned painting example puts the canvas inside an interactive `div`, with handlers on the `div` (`crates/gpui/examples/painting.rs:352-439`).

Event positions are window coordinates. Canvas/node coordinates must first be made root-local:

```rust
let local = core::Point::new(
    f32::from(event.position.x - graph_bounds.origin.x),
    f32::from(event.position.y - graph_bounds.origin.y),
);
let world = viewport.screen_to_world(local);
```

The current graph directly sends the window position to `screen_to_world` (`gpui-node-graph/src/lib.rs:186-192,211-225`) and paints canvas paths from `(0,0)` without adding the canvas bounds origin (`:136-153`). That works only when the graph is at the window origin. Retain/observe the root bounds (custom element prepaint, canvas prepaint state, or a bounds-observer helper) and use one canonical window→local→world transform for all picking and painting.

Recommended picking split:

* **Nodes and ports:** positioned interactive child `div`s. GPUI supplies z-order/occlusion and hitboxes.
* **Wires:** analytical world-space distance-to-segment/Bezier/stroked-polyline test with tolerance divided by viewport scale. Iterate reverse paint/z order and stop on the first hit.
* **Background:** root hitbox.
* **Custom rectangular region:** `window.insert_hitbox`; a `Hitbox` is a rectangular bounds plus behavior (`window.rs:793-805`), not arbitrary path geometry.

Compute picking from the same immutable geometry snapshot used by the canvas paint closure to prevent visible/pick geometry drift. Canvas prepaint may return prepared paths/spatial-index data as its generic `T` (`canvas.rs:10-13,62-87`).

## Context menus

### Core GPUI-only library boundary

Core `gpui` has the event, focus, anchor/deferred-element, and custom-element primitives, but Zed's ready-made `ContextMenu` and `right_click_menu` live in the separate `ui` crate. Keep `gpui-node-graph` independent by exposing one of:

* an `EditorEvent::ContextMenuRequested { target, world_position, window_position }`;
* an optional `'static` menu builder callback returning `AnyElement`/host-managed entity;
* graph actions (Delete, Duplicate, Disconnect, etc.) that a host menu can dispatch.

On right mouse-down, focus/select the target first, stop propagation, and request the menu. This keeps the widget usable outside Zed and allows a host to use any menu system.

### If the host already depends on Zed `ui`

Use:

```rust
right_click_menu("node-context-menu")
    .menu(move |window, cx| ContextMenu::build(window, cx, |menu, _, _| {
        menu.context(graph_focus.clone())
            .action("Delete", Box::new(DeleteSelection))
            .separator()
            .entry("Duplicate", None, move |window, cx| { /* ... */ })
    }))
    .trigger(|is_open, window, cx| graph_element)
```

Exact evidence:

* `right_click_menu(id)`, `.menu`/`.maybe_menu`, `.trigger`, `.anchor`, `.attach`: `crates/ui/src/components/right_click_menu.rs:18-83`.
* It detects right-button bubble events, stops propagation/prevents default, anchors at `window.mouse_position()`, focuses the menu, restores previous focus on dismiss, and defers drawing above content: `right_click_menu.rs:145-158,245-297`.
* `ContextMenu::build(window, cx, builder)`: `crates/ui/src/components/context_menu.rs:271-355`.
* `.context(focus)` makes action entries refocus/dispatch into the graph; `.header`, `.separator`, `.entry`: `context_menu.rs:510-570`; `.action` and checked/disabled variants: `:745-810`; custom rows/entries and submenus are listed/implemented around `:687-737,864-907`.
* A compact production composition is `crates/workspace/src/multi_workspace.rs:66-90`.

`RightClickMenu` requires `M: ManagedView`; that is `Focusable + EventEmitter<DismissEvent> + Render` (`gpui/src/window.rs:686-688`).

## Searchable menus and popovers

Again, these are Zed UI-layer facilities, not core `gpui`.

* `ui::PopoverMenu<M: ManagedView>`: `.new(id)`, `.menu(|window,cx| Option<Entity<M>>)`, `.trigger(...)`, `.trigger_with_tooltip(...)`, `.with_handle(...)`, `.anchor(...)`, `.attach(...)`, `.offset(...)`, `.on_open(...)` (`crates/ui/src/components/popover_menu.rs:128-243`).
* `picker::Picker<D: PickerDelegate>` supplies query input, selection/actions, virtual list, async filtering, and custom row rendering. `PickerDelegate`'s required core is `ListItem: IntoElement`, name/count/selection, placeholder, `update_matches -> Task<()>`, `confirm`, and `render_match` (`crates/picker/src/picker.rs:127-247,369-375`). Optional `render_editor`, header/footer, preview, and checkbox hooks provide deeper customization (`:350-400`).
* A picker in a popover is wrapped by `PickerPopoverMenu::new(picker, trigger, tooltip, anchor, cx)` and renders through `PopoverMenu` (`crates/picker/src/popover_menu.rs:11-64,75-101`).
* The best small searchable delegate example is `crates/git_ui/src/picker_prompt.rs:88-140,143-239`: it asynchronously fuzzy-filters candidates in `update_matches`, confirms by candidate ID, and renders highlighted custom `ListItem` rows. Construction uses `Picker::uniform_list(delegate, window, cx)` at `:51-65`.

For a graph command/node search palette in an application already using these crates, define a delegate whose candidate carries a stable node/action ID; never identify a result by its filtered index. For a standalone library, provide candidate data and selection callbacks and let the host choose `picker` or another UI.

## Custom child and delegate rendering

GPUI's idiomatic reusable component is a plain owned type implementing `RenderOnce`, usually with `#[derive(IntoElement)]`; `RenderOnce::render(self, window, cx) -> impl IntoElement` is defined at `gpui/src/element.rs:174-184`. Heterogeneous/host-supplied children are erased to `AnyElement` via `IntoElement::into_any_element` (`element.rs:144-157`). `ParentElement::child/children/extend` is defined at `element.rs:186-208`.

Pinned patterns:

* A component owning a collection of arbitrary children and implementing `ParentElement`: `crates/ui/src/components/facepile.rs:30-80` (`SmallVec<[AnyElement; 2]>`, `extend`, then `.children(...)` in `RenderOnce`).
* Named arbitrary slots accepting `impl IntoElement` and storing `AnyElement`: `crates/ui/src/components/callout.rs:27-52,81-90`.
* A render delegate returning a concrete `IntoElement`: `PickerDelegate::ListItem` and `render_match` above.
* A dynamic row-render closure returning `AnyElement`: `crates/ui/src/components/data_table.rs:143-156,447-452`.

For `gpui-node-graph`, a renderer must run each frame because elements are consumed during rendering (`Element` callbacks/tree are dropped each frame, `gpui/src/element.rs:10-17`). Suitable public designs are:

```rust
pub trait NodeRenderer<N, P, T>: 'static {
    fn render_node(
        &mut self,
        node: &Node<N, P, T>,
        state: NodeRenderState,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;
}
```

or a stored `Rc<dyn Fn(&Node<...>, NodeRenderState, &mut Window, &mut App) -> AnyElement>`. Do not accept and store a single `AnyElement` per node: it is consumed. A slot/delegate should receive stable IDs, selection/hover state, scale, and action/event callbacks, while the graph owns absolute placement, hit target, focus, drag, and transforms. Wrap the returned child inside the graph's positioned interactive node container so a custom renderer cannot accidentally remove graph interaction.

For a one-off custom element that itself owns children, follow the lifecycle illustrated by `RightClickMenu`: request child layout, include child layout IDs in the parent's `window.request_layout`, call child `prepaint`, then child `paint` (`right_click_menu.rs:131-210,213-234`). Prefer `RenderOnce` composition unless manual layout/hitbox/paint control is actually needed; GPUI says exactly that at `element.rs:19-32`.

## Concrete gaps to address in the current graph (no production changes made)

1. Add retained focus/action/key-context support; current `NodeGraph` has no `FocusHandle`.
2. Replace hover-only drag continuation with typed drag or a pointer-capturing custom wrapper.
3. Track graph bounds and correct window-vs-local coordinates for both node movement and canvas painting.
4. Add `.on_scroll_wheel`/`.on_pinch` with normalized deltas and pointer-anchored zoom.
5. Define explicit node/port/wire hit testing and z-order rules; canvas paths do not create hit targets.
6. Expose menu request/actions and a custom node renderer/slot without taking a hard dependency on Zed `ui`/`picker`.
7. Add interaction tests for: focus-context key dispatch, mouse release outside bounds, graph embedded at nonzero origin, precise vs line wheel deltas, zoom-about-pointer invariance, overlapping node/wire priority, and context-menu focus restoration.
