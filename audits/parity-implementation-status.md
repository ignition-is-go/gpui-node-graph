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

## Validation improvements

The browser demo now retains `ApplicationHandle` from `Application::run_embedded` on
WASM. `Application::run` returned before the browser event loop and previously allowed
the application and canvas to disappear immediately after graphics initialization.
The demo explicitly requests a post-launch window refresh so the asynchronous browser
resize observer produces the first mounted frame. The CDP runtime check now requires a
correctly sized (not the transient 1×1 backing store) canvas to remain present for at
least three seconds, then drives and state-verifies menu opening, searched node creation, overlay dismissal,
and wheel zoom while also capturing the rendered canvas. Linux CI also requires the compiled native
GPUI demo to stay alive under Xvfb rather than merely compiling it.

## Still required before full parity

- further transaction ergonomics; explicit uncontrolled ownership now commits previews
  locally, while controlled ownership rolls them back and emits atomic typed mutation batches;
- further specialized-widget polish; `NodeBodyContext::isolated_control` blocks graph
  gestures around retained controls, consumer bodies now measure node size and port anchors,
  and the demo exercises editable numeric/color controls, dynamic inputs, and an adaptive
  flip/clamp/Escape-dismiss panel; right-edge resize, rollback, and reset also work;
- richer persistent ungroup/nested-group semantics; rendered groups now support inline
  label editing and Alt-drag membership updates with explicit core events;
- broader dynamic-port policy/UI polish; removal atomically keeps persisted graphs strict,
  renders transient red tombstones, and restores the original typed connection when a stable
  port ID reappears;
- deeper large-graph routing performance/visual tuning; deterministic subway routes now
  batch into stable per-source lanes, cache by graph/geometry/config fingerprint, paint rounded
  orthogonal corners, and switch at runtime to Bezier/simple modes;
- broader scripted native/browser interaction and visual tests; window-level capture now
  continues and completes gestures outside embedded bounds, and rich renderers receive an
  opt-in visibility hook.
