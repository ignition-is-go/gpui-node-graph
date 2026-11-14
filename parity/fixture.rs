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

pub const CONNECTIONS: &[(&str, &str, &str)] = &[
    ("c1", "src:a_value", "calc:a"),
    ("c2", "calc:out", "sink:in"),
];

/// `scene` mirrors the rship scenes theme; anything else is the crate default.
pub fn scene_theme(query: &str) -> bool {
    query.contains("theme=scene")
}
