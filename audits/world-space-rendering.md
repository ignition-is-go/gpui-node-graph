# World-space rendering and zoom contract

Reference: `leptos-node-graph` at `87658950fccfeeea123285c706820ffea4ab55d1`.
GPUI pin: Zed `08827f9208b4848d62f3faf86ffa15155966d63c`.

## Reference contract

The Leptos canvas owns one CSS transform:

```text
translate(pan_x, pan_y) scale(zoom), transform-origin: 0 0
screen = world * zoom + pan
world = (screen - pan) / zoom
```

Nodes, groups, ports, text and wires are authored in world pixels. Selection and
portalled overlays are pane-space siblings and remain unscaled. Wheel zoom uses only
the delta sign, `exp(±0.3)`, and keeps the cursor world point invariant.

## GPUI constraint

The pinned public GPUI API has no generic affine transform for an arbitrary retained
`AnyElement` subtree. `TransformationMatrix` is accepted by SVG sprite painting, not
by quads, paths, arbitrary elements, layout or hitboxes. `with_rem_size` reruns layout
and therefore cannot implement CSS-transform semantics.

## Canonical implementation

`world.rs` is an immutable world display list:

- `WorldPrimitive::{Quad,Line,Text,Circle}` stores only world geometry.
- `TextLines` stores resolved lines, not a wrapping constraint.
- `Transform` projects the list once per frame.
- hit regions stay in world coordinates and pointer input is inverse-transformed.
- `world_scene_element` rasterizes the projected list without flex/text reflow.
- `WorldNodeBodyRenderer` is the compositor-style node body path.
- `NodeOverlayRenderer` remains a pane-space retained path for unscaled overlays.

The legacy arbitrary retained `NodeBodyRenderer` remains available for compatibility,
but it is not the pixel-parity rendering path. The demo uses the world renderer.

## Required invariants

Pure and demo tests cover zooms `0.1`, `0.740818`, `1`, `1.349859`, `2`, and `5`:

1. Authored primitives and text lines do not change with zoom.
2. Every projected coordinate/length changes only by the affine viewport transform.
3. Screen hits inverse-project to the same topmost world region.
4. Model-owned node dimensions never consume measured child dimensions.
5. Pane-space overlay dimensions remain 1×; their world anchor scales and their CSS-like gap does not.
6. Wires, borders, sockets and text raster sizes derive from the same zoom.

## Paint and interaction planes

1. clipped editor root (`#18181b` in the parity demo),
2. group background,
3. wires,
4. model-owned node shells,
5. world display list,
6. inverse-transformed world interaction plane,
7. pane-space selection,
8. pane-space overlays/menu.

The interaction plane owns node/control/port hit testing so visual zoom never changes
the authored hit geometry. Middle/Ctrl-left panning and blank-canvas selection continue
to bubble to the editor root. Overlays and the node menu stop wheel propagation.
