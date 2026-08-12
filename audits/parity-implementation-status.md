# Leptos parity implementation status

This document records implementation progress after the fixed-revision audit in
`leptos-parity-audit.md`. The audit remains an immutable description of commit
`1cfcb5d`'s predecessor; claims below apply to the working implementation that
followed it.

## Milestone 1 — authoring loop

Implemented in the shared native/WASM `NodeGraph`:

- visible labeled input/output ports with connected, compatible, source and snap states;
- click or drag drafts from either direction, screen-pixel snapping and directional type checks;
- input rerouting, connection request/removal events, wire hit testing/selection/deletion;
- shell-owned catalog metadata, keyboard search, Tab and blank-canvas double-click opening;
- draft-to-blank compatible catalog filtering and create-with-auto-connect metadata;
- a five-type demo catalog whose host instantiates nodes, ports and optional connections.

## Milestone 2 — editing and navigation

Implemented:

- focusable editor keyboard input; Escape, Delete/Backspace, select-all, copy/paste,
  undo/redo, group request and fit-view event paths;
- blank deselection, Shift node/wire selection, box selection and batched multi-node drag;
- optional grid snap, middle-button and Ctrl+left panning, pointer-centered wheel zoom,
  fit view, and finite viewport safeguards;
- window-level capture-phase continuation and completion for left gestures that leave an
  embedded editor, plus root `on_mouse_up_out` cleanup.

## Post-audit interaction hardening

The current implementation also closes the remaining draft/menu transition gaps found
by follow-up source comparison:

- draft catalogs render node identity as informational and make the exact compatible
  pins the only activation targets, including the single-pin case;
- catalog activation validates the selected pin and emits the reference
  `connect_from`/`connect_to`/draft-origin-direction contract;
- Tab transfers an active draft into the compatible catalog, while blank releases
  cancel and Ctrl+left pan releases preserve in-flight wiring;
- grabbing an occupied input now requests/removes the old edge immediately and hides
  a controlled snapshot edge while rerouting;
- catalog search and group rename use UTF-16 caret/selection editing with forward
  Delete, clipboard operations and IME routing; search changes reset menu highlight,
  and group rename commits when focus moves to graph interactions;
- compatible anchor label rows participate in draft completion and anchor context
  menus, and group label/membership hit geometry matches rendered padding;
- graph command shortcuts suppress platform defaults consistently;
- persisted nodes now carry a stable catalog/renderer `node_type` independent of
  mutable titles, with legacy snapshot fallback and demo dispatch migrated away from
  display labels;
- catalog categories carry consumer-defined RGBA accents instead of library-coded
  demo-name colors;
- per-port anchor presentation supports typed color, all reference socket shapes,
  and collection/multi ghost sockets, and is exposed to both retained and world
  body renderers;
- world controls now project named AccessKit button nodes at their zoomed/panned hit
  geometry, expose assistive Click actions through the same activation path as pointers,
  and report the active descendant from a labelled graph group;
- adaptive overlays independently control Escape dismissal, outside-click dismissal,
  and backdrop display; the editor overscan now matches the reference's 600 screen pixels;
- a deterministic default-layout primitive consumes measured section metrics and
  `NodeStyle`/`AnchorStyle` row, padding, inset, and column/stacked configuration; editor-owned
  default nodes now resolve those shell/port coordinates live, while measured custom bodies are
  authoritative transient geometry and emit resize events without mutating persisted snapshots;
- the core subway router now solves stable connection batches with whole-batch budgets, crossing
  and overlap costs, and obstacle-aware lane separation instead of post-shifting independent paths;
- semantic world controls carry button/text-input/combobox/slider/spinbutton roles plus values and
  numeric ranges, while catalog search/menu/options expose corresponding AccessKit semantics;
- group padding is style-driven and consumers can supply custom group header renderers; overlays
  accept fixed pane-rect or world-control anchors, independent dismissal/backdrop policies, and
  per-overlay dismissal callbacks; dangling ports render short direction-aware stubs;
- double-click width reset enters intrinsic/auto-width mode rather than forcing the configured
  default width, with manual resize returning the node to explicit-width ownership;
- Shift-click on blank canvas preserves selection, while the first Shift-marquee move now
  replaces selected nodes exactly like the reference rather than remaining additive.

## Validation improvements

The browser demo now retains `ApplicationHandle` from `Application::run_embedded` on
WASM. `Application::run` returned before the browser event loop and previously allowed
the application and canvas to disappear immediately after graphics initialization.
The demo explicitly requests a post-launch window refresh so the asynchronous browser
resize observer produces the first mounted frame. The CDP runtime check now requires a
correctly sized (not the transient 1×1 backing store) canvas to remain present for at
least three seconds, then drives and state-verifies menu opening, searched node creation, and wheel zoom while
also capturing the rendered canvas. Linux CI also requires the compiled native
GPUI demo to stay alive under Xvfb rather than merely compiling it.

## Final parity status

The final post-implementation source audit found no remaining concrete P0–P2 observable or
public-API parity gap. The typed node-definition registry now owns catalog/renderer dispatch and
lifecycle refresh for static and dynamic ports in both mutation modes; measured/default geometry,
batch subway routing, semantic controls, overlays, groups, width reset, style helpers, and dangling
stubs all have corresponding reference behavior. Styling now uses a required ambient GPUI global: `NodeGraphTheme` is the sole complete aggregate,
leaf records remain `*Style`, and one immutable `Arc<NodeGraphTheme>` snapshot (with preserved
pointer identity) is read once and shared across each root render. As an intentional extension beyond the
checked-in Leptos reference, selecting two or more nodes mounts an accessible pane-space alignment
toolbar with align/distribute operations and inferred-grid Tidy. Tidy clusters independently on each
axis, resolves overlaps with measured heights, and all operations use the same atomic controlled/
uncontrolled movement semantics as dragging. The Rship scene workflow automapping rules are also
integrated: selected columns pair by vertical rank with node-type fallback, otherwise the selected
endpoint side fans by stable anchor key; disconnect mapping is symmetric and ranked previews render
before the gesture.

Final validation covers 126 workspace tests (84 GPUI unit, 5 GPUI API/architecture, 9 demo, 28 core), Clippy with warnings denied,
WASM compilation, `git diff --check`, a sustained native Xvfb run, and the full scripted Chrome/WASM
interaction trace. All four reference-golden comparisons passed their configured thresholds:
initial MAE 0.4889, selected 0.4966, overlay 0.8242, and menu 1.4827.
