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
- left-button gesture cleanup through `on_mouse_up_out` (continuous movement outside an
  embedded editor still requires the planned captured/custom GPUI element).

## Validation improvements

The browser demo now retains `ApplicationHandle` from `Application::run_embedded` on
WASM. `Application::run` returned before the browser event loop and previously allowed
the application and canvas to disappear immediately after graphics initialization.
The demo explicitly requests a post-launch window refresh so the asynchronous browser
resize observer produces the first mounted frame. The CDP runtime check now requires a
correctly sized (not the transient 1×1 backing store) canvas to remain present for at
least three seconds, preventing both lifecycle and false-mount positives.

## Still required before full parity

- explicit controlled/uncontrolled mutation transactions rather than the current
  reference-like hybrid ownership;
- broader specialized widgets; `NodeBodyContext::isolated_control` now blocks graph
  keyboard, wheel, and pointer gestures around retained body controls, and `NodeBodyRenderer` supports
  shell-interactive measured anchors, transient node-relative geometry/tombstones, and
  unscaled overlays with adaptive flip/clamp/Escape-dismiss behavior; the demo proves an
  interactive panel, while right-edge resize,
  rollback and reset also work;
- group inline editing plus Alt-drag membership gestures (consumer-supplied group
  bounds/rendering and create-group handling are now present);
- dangling dynamic-port presentation without weakening strict persisted validation (the demo
  now adds/removes stable dynamic inputs, but refuses removal while referenced);
- deeper large-graph routing performance/visual tuning; deterministic subway routes now
  batch into stable per-source lanes, cache by graph/geometry/config fingerprint, paint rounded
  orthogonal corners, and switch at runtime to Bezier/simple modes;
- captured dragging outside embedded bounds and broader scripted native/browser
  interaction and visual tests (rich renderers now receive an opt-in visibility hook).
