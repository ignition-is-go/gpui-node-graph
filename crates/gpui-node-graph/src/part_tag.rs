//! Names for the node graph's chrome, for a harness that measures where each
//! part lands on screen.
//!
//! The graph does not record anything itself. An application that wants the
//! parts named installs a [`PartTagger`] with [`set_part_tagger`]; the graph
//! then passes each part through it with a stable id. With no tagger installed
//! no probe element is built and no id string is formatted.
//!
//! Each id is `node-graph.` followed by the part and the `Debug` text of its
//! id, with surrounding quotes removed so a `String` id reads as itself:
//!
//! | id | part |
//! | --- | --- |
//! | `node-graph.node:<node>` | a node card, including its accent stripe |
//! | `node-graph.node-header:<node>` | the default chrome's header row |
//! | `node-graph.port:<port>` | a port socket, sized to the theme's dot |
//! | `node-graph.wire:<connection>` | the bounding box of a wire's route |
//! | `node-graph.wire-point:<connection>.<n>` | the route's `n`th vertex, as a zero-size box |
//!
//! Offscreen nodes are culled, so their parts are absent from a frame.

use gpui::{AnyElement, App, Global, IntoElement, SharedString};

/// Wraps a part's element with its id. It must not change the part's layout.
pub type PartTagger = fn(AnyElement, SharedString) -> AnyElement;

struct GlobalPartTagger(PartTagger);
impl Global for GlobalPartTagger {}

/// Installs the tagger every node graph passes its parts through.
pub fn set_part_tagger(cx: &mut App, tagger: PartTagger) {
    cx.set_global(GlobalPartTagger(tagger));
}

pub(crate) fn part_tagger(cx: &App) -> Option<PartTagger> {
    cx.try_global::<GlobalPartTagger>().map(|tagger| tagger.0)
}

/// `node-graph.<part>:<id>` for an id's `Debug` text.
pub(crate) fn part_id(part: &str, id: &impl std::fmt::Debug) -> SharedString {
    let id = format!("{id:?}");
    format!("node-graph.{part}:{}", id.trim_matches('"')).into()
}

pub(crate) fn tag(
    tagger: Option<PartTagger>,
    element: impl IntoElement,
    id: impl FnOnce() -> SharedString,
) -> AnyElement {
    let element = element.into_any_element();
    match tagger {
        Some(tagger) => tagger(element, id()),
        None => element,
    }
}
