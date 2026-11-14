// Shared by the Leptos reference and the GPUI gallery through `include!`, so
// both render the same graph from one definition.

pub struct FixtureNode {
    pub id: &'static str,
    pub title: &'static str,
    pub x: f32,
    pub y: f32,
    pub accent: Option<u32>,
    pub inputs: &'static [(&'static str, &'static str)],
    pub outputs: &'static [(&'static str, &'static str)],
}

pub const NODES: &[FixtureNode] = &[
    FixtureNode {
        id: "src",
        title: "Source",
        x: 40.0,
        y: 40.0,
        accent: Some(0x22c55e),
        inputs: &[],
        outputs: &[("src:a_value", "Value"), ("src:b_trigger", "Trigger")],
    },
    FixtureNode {
        id: "calc",
        title: "Calc",
        x: 330.0,
        y: 70.0,
        accent: Some(0x3b82f6),
        inputs: &[("calc:a", "A"), ("calc:b", "B")],
        outputs: &[("calc:out", "Out")],
    },
    FixtureNode {
        id: "sink",
        title: "Sink",
        x: 640.0,
        y: 40.0,
        accent: None,
        inputs: &[("sink:in", "In")],
        outputs: &[],
    },
];

/// Two overlapping cards: the lower card's output socket sits under the upper
/// card. Ids sort in stacking order, as the Leptos `<For>` renders them.
pub const OVERLAP_NODES: &[FixtureNode] = &[
    FixtureNode {
        id: "ova",
        title: "A",
        x: 40.0,
        y: 40.0,
        accent: Some(0x22c55e),
        inputs: &[("ova:in", "In A")],
        outputs: &[("ova:out", "Out A")],
    },
    FixtureNode {
        id: "ovb",
        title: "B",
        x: 150.0,
        y: 70.0,
        accent: Some(0x3b82f6),
        inputs: &[("ovb:in", "In B")],
        outputs: &[("ovb:out", "Out B")],
    },
];

/// One card with consumer-authored body content between its header and ports:
/// a labelled field row whose value is `BODY_FIELD.1`.
pub const BODY_NODES: &[FixtureNode] = &[FixtureNode {
    id: "gain",
    title: "Gain",
    x: 60.0,
    y: 50.0,
    accent: Some(0xf59e0b),
    inputs: &[("gain:in", "Signal")],
    outputs: &[("gain:out", "Result")],
}];
pub const BODY_FIELD: (&str, &str) = ("Level", "0.50");
/// The body value's text size and line height, set on both sides so the row
/// height does not depend on either toolkit's default font metrics.
pub const BODY_TEXT_SIZE: f32 = 11.0;
pub const BODY_LINE_HEIGHT: f32 = 16.0;
pub const BODY_TEXT_COLOR: u32 = 0xe4e4e7;

pub const CONNECTIONS: &[(&str, &str, &str)] = &[
    ("c1", "src:a_value", "calc:a"),
    ("c2", "calc:out", "sink:in"),
];

/// `scene` mirrors the rship scenes theme; anything else is the crate default.
pub fn scene_theme(query: &str) -> bool {
    query.contains("theme=scene")
}

/// `overlap` swaps the three-node graph for the two overlapping cards.
pub fn overlap_fixture(query: &str) -> bool {
    query.contains("overlap")
}

/// `body` swaps the graph for the one card with a custom body.
pub fn body_fixture(query: &str) -> bool {
    query.contains("body")
}

/// `zoom=<f>` opens the editor at that zoom with no pan.
pub fn fixture_zoom(query: &str) -> f32 {
    query
        .split(['?', '&'])
        .find_map(|pair| pair.strip_prefix("zoom="))
        .and_then(|value| value.parse().ok())
        .unwrap_or(1.0)
}

pub fn fixture_nodes(query: &str) -> &'static [FixtureNode] {
    if overlap_fixture(query) {
        OVERLAP_NODES
    } else if body_fixture(query) {
        BODY_NODES
    } else {
        NODES
    }
}

pub fn fixture_connections(query: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    if overlap_fixture(query) || body_fixture(query) { &[] } else { CONNECTIONS }
}
