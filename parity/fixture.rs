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

pub fn fixture_nodes(query: &str) -> &'static [FixtureNode] {
    if overlap_fixture(query) { OVERLAP_NODES } else { NODES }
}

pub fn fixture_connections(query: &str) -> &'static [(&'static str, &'static str, &'static str)] {
    if overlap_fixture(query) { &[] } else { CONNECTIONS }
}
