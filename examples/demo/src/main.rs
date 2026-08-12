use gpui::prelude::*;
use gpui::{App, WindowOptions};
#[cfg(not(target_arch = "wasm32"))]
use gpui::{Bounds, WindowBounds, px, size};
use gpui_node_graph::{
    CatalogPort, GraphGroup, NodeCatalogItem, NodeGraph, NodeOverlay, OverlayAlign,
    OverlayPlacement, OverlaySide, WorldNodeBodyContext, WorldTextInputState,
    core::*,
    style::Color as GraphColor,
    world::{
        AccessibleControlRole, HitRole, HitShape, TextLines, WorldColor, WorldHitRegion,
        WorldPrimitive, WorldScene,
    },
};

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Float,
    Color,
    Any,
}
impl PortType for Kind {
    fn compatible(source: &Self, target: &Self) -> bool {
        matches!(target, Self::Any) || source == target
    }
}

static NEXT_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(100);
thread_local! {
    static MIX_AMOUNTS: std::cell::RefCell<std::collections::HashMap<String, f32>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}
thread_local! {
    static TEST_CONTROLS: std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<DemoControls>>>> =
        const { std::cell::RefCell::new(None) };
}

const BLEND_OPTIONS: [&str; 5] = ["Normal", "Multiply", "Screen", "Overlay", "Add"];

#[derive(Clone, Debug, Default, PartialEq)]
struct DemoControls {
    blends: std::collections::HashMap<String, usize>,
    factors: std::collections::HashMap<String, String>,
    input_counts: std::collections::HashMap<String, usize>,
    output_counts: std::collections::HashMap<String, usize>,
    /// (node id, control id). Keeping this outside graph/domain state makes the
    /// popup ephemeral, just like an HTML select's UA-owned popup.
    open_select: Option<(String, String)>,
    editing_number: Option<(String, String, String)>,
    number_selection_anchor: Option<usize>,
    number_cursor: usize,
}

impl DemoControls {
    fn blend_label(&self, node: &str) -> &'static str {
        BLEND_OPTIONS[self.blends.get(node).copied().unwrap_or(0) % BLEND_OPTIONS.len()]
    }

    #[allow(dead_code)]
    fn factor_text(&self, node: &str) -> &str {
        self.factors
            .iter()
            .find(|(control, _)| control.starts_with(&format!("{node}:")))
            .map(|(_, value)| value.as_str())
            .or_else(|| self.factors.get(node).map(String::as_str))
            .unwrap_or("0.0")
    }

    fn factor_text_for(&self, node: &str, control: &str) -> &str {
        self.factors
            .get(control)
            .or_else(|| self.factors.get(node))
            .map(String::as_str)
            .unwrap_or("0.0")
    }

    fn activate(&mut self, node: &str, control: &str) -> Option<(PortDirection, usize)> {
        if control.ends_with(":blend-select") {
            self.open_select = Some((node.into(), control.into()));
            self.editing_number = None;
        } else if let Some(index) = control
            .rsplit(":blend-option-")
            .next()
            .filter(|_| control.contains(":blend-option-"))
            .and_then(|value| value.parse::<usize>().ok())
        {
            self.blends
                .insert(node.into(), index.min(BLEND_OPTIONS.len() - 1));
            self.open_select = None;
        } else if control.ends_with(":factor-value") {
            self.open_select = None;
            let original = self
                .factors
                .entry(control.into())
                .or_insert_with(|| "0.0".into())
                .clone();
            self.number_cursor = original.chars().count();
            self.editing_number = Some((node.into(), control.into(), original));
            self.number_selection_anchor = None;
        } else if control.ends_with(":inputs-count") || control.ends_with(":outputs-count") {
            if control.ends_with(":inputs-count") {
                self.input_counts.entry(node.into()).or_insert(2);
            } else {
                self.output_counts.entry(node.into()).or_insert(1);
            }
            self.open_select = Some((node.into(), control.into()));
            self.editing_number = None;
        } else if let Some((direction, value)) = parse_count_option(control) {
            self.open_select = None;
            match direction {
                PortDirection::Input => {
                    self.input_counts.insert(node.into(), value);
                }
                PortDirection::Output => {
                    self.output_counts.insert(node.into(), value);
                }
            }
            return Some((direction, value));
        } else {
            self.open_select = None;
            self.editing_number = None;
        }
        None
    }

    fn focus(&mut self, node: &str, control: &str) {
        self.open_select = None;
        self.editing_number = None;
        self.number_selection_anchor = None;
        if control.ends_with(":factor-value") {
            let original = self
                .factors
                .entry(control.into())
                .or_insert_with(|| "0.0".into())
                .clone();
            self.number_cursor = original.chars().count();
            self.editing_number = Some((node.into(), control.into(), original));
        }
    }

    fn blur(&mut self, node: &str, control: &str) {
        if self
            .open_select
            .as_ref()
            .is_some_and(|(active_node, active_control)| {
                active_node == node && active_control == control
            })
        {
            self.open_select = None;
        }
        if self
            .editing_number
            .as_ref()
            .is_some_and(|(active_node, active_control, _)| {
                active_node == node && active_control == control
            })
        {
            self.editing_number = None;
            self.number_selection_anchor = None;
        }
    }

    fn pointer_activate(
        &mut self,
        node: &str,
        control: &str,
        world_position: Point,
        node_x: f32,
        click_count: usize,
    ) {
        if !control.ends_with(":factor-value") {
            return;
        }
        let char_count = self.factor_text_for(node, control).chars().count();
        if click_count >= 2 {
            self.number_selection_anchor = Some(0);
            self.number_cursor = char_count;
            return;
        }
        let text_start = node_x + 119.0 - char_count as f32 * 5.3;
        self.number_cursor = (((world_position.x - text_start) / 5.3).round() as isize)
            .clamp(0, char_count as isize) as usize;
        self.number_selection_anchor = None;
    }

    fn selected_factor_text(&self, node: &str, control: &str) -> Option<String> {
        let (active_node, active_control, _) = self.editing_number.as_ref()?;
        if active_node != node || active_control != control {
            return None;
        }
        let anchor = self.number_selection_anchor?;
        let value = self.factor_text_for(node, control);
        let (start_char, end_char) = (
            anchor.min(self.number_cursor),
            anchor.max(self.number_cursor),
        );
        if start_char == end_char {
            return None;
        }
        Some(
            value
                .chars()
                .skip(start_char)
                .take(end_char - start_char)
                .collect(),
        )
    }

    fn factor_input_state(&self, node: &str, control: &str) -> Option<WorldTextInputState> {
        let (active_node, active_control, _) = self.editing_number.as_ref()?;
        if active_node != node || active_control != control {
            return None;
        }
        let text = self.factor_text_for(node, control).to_owned();
        let to_utf16 = |chars: usize| text.chars().take(chars).map(char::len_utf16).sum::<usize>();
        let cursor = to_utf16(self.number_cursor);
        let (selection, reversed) = if let Some(anchor) = self.number_selection_anchor {
            let anchor = to_utf16(anchor);
            (anchor.min(cursor)..anchor.max(cursor), cursor < anchor)
        } else {
            (cursor..cursor, false)
        };
        Some(WorldTextInputState {
            text,
            selection,
            selection_reversed: reversed,
            marked: None,
        })
    }

    fn apply_platform_text(
        &mut self,
        _node: &str,
        control: &str,
        text: &str,
        selection: std::ops::Range<usize>,
        reversed: bool,
    ) {
        if !control.ends_with(":factor-value") {
            return;
        }
        self.factors.insert(control.into(), text.into());
        let to_char = |utf16: usize| {
            let mut units = 0;
            text.chars()
                .take_while(|ch| {
                    if units + ch.len_utf16() > utf16 {
                        false
                    } else {
                        units += ch.len_utf16();
                        true
                    }
                })
                .count()
        };
        let start = to_char(selection.start);
        let end = to_char(selection.end);
        self.number_cursor = if reversed { start } else { end };
        self.number_selection_anchor = (start != end).then_some(if reversed { end } else { start });
    }

    fn key_down(
        &mut self,
        node: &str,
        control: &str,
        key: &str,
        text: Option<&str>,
        command: bool,
        shift: bool,
    ) -> Option<(PortDirection, usize)> {
        if control.ends_with(":blend-select") {
            let value = self.blends.entry(node.into()).or_insert(0);
            match key {
                "up" => *value = value.saturating_sub(1),
                "down" => *value = (*value + 1).min(BLEND_OPTIONS.len() - 1),
                "home" => *value = 0,
                "end" => *value = BLEND_OPTIONS.len() - 1,
                "enter" => {
                    self.open_select = if self.open_select.is_some() {
                        None
                    } else {
                        Some((node.into(), control.into()))
                    };
                }
                "escape" | "tab" => self.open_select = None,
                _ => {
                    if let Some(prefix) = text.and_then(|s| s.chars().next())
                        && let Some(found) = BLEND_OPTIONS.iter().position(|option| {
                            option
                                .to_ascii_lowercase()
                                .starts_with(prefix.to_ascii_lowercase())
                        })
                    {
                        *value = found;
                    }
                }
            }
        } else if control.ends_with(":factor-value") {
            if !self
                .editing_number
                .as_ref()
                .is_some_and(|(active_node, active_control, _)| {
                    active_node == node && active_control == control
                })
            {
                return None;
            }
            let value = self
                .factors
                .entry(control.into())
                .or_insert_with(|| "0.0".into());
            let char_count = value.chars().count();
            let selection = self.number_selection_anchor.map(|anchor| {
                (
                    anchor.min(self.number_cursor),
                    anchor.max(self.number_cursor),
                )
            });
            let delete_selection =
                |value: &mut String, selection: Option<(usize, usize)>| -> Option<usize> {
                    let (start_char, end_char) = selection.filter(|(start, end)| start != end)?;
                    let start = value
                        .char_indices()
                        .nth(start_char)
                        .map_or(value.len(), |(index, _)| index);
                    let end = value
                        .char_indices()
                        .nth(end_char)
                        .map_or(value.len(), |(index, _)| index);
                    value.replace_range(start..end, "");
                    Some(start_char)
                };
            match key {
                "a" if command => {
                    self.number_selection_anchor = Some(0);
                    self.number_cursor = char_count;
                }
                "left" | "right" | "home" | "end" => {
                    let previous = self.number_cursor;
                    let next = match key {
                        "left" if command => 0,
                        "right" if command => char_count,
                        "left" => previous.saturating_sub(1),
                        "right" => (previous + 1).min(char_count),
                        "home" => 0,
                        "end" => char_count,
                        _ => previous,
                    };
                    if shift {
                        self.number_selection_anchor.get_or_insert(previous);
                    } else {
                        self.number_selection_anchor = None;
                    }
                    self.number_cursor = next;
                }
                "backspace" => {
                    if let Some(cursor) = delete_selection(value, selection) {
                        self.number_cursor = cursor;
                    } else if self.number_cursor > 0 {
                        let end = value
                            .char_indices()
                            .nth(self.number_cursor)
                            .map_or(value.len(), |(index, _)| index);
                        let start = value
                            .char_indices()
                            .nth(self.number_cursor - 1)
                            .map_or(0, |(index, _)| index);
                        value.replace_range(start..end, "");
                        self.number_cursor -= 1;
                    }
                    self.number_selection_anchor = None;
                }
                "delete" => {
                    if let Some(cursor) = delete_selection(value, selection) {
                        self.number_cursor = cursor;
                    } else if self.number_cursor < char_count {
                        let start = value
                            .char_indices()
                            .nth(self.number_cursor)
                            .map_or(value.len(), |(index, _)| index);
                        let end = value
                            .char_indices()
                            .nth(self.number_cursor + 1)
                            .map_or(value.len(), |(index, _)| index);
                        value.replace_range(start..end, "");
                    }
                    self.number_selection_anchor = None;
                }
                "x" if command => {
                    if let Some(cursor) = delete_selection(value, selection) {
                        self.number_cursor = cursor;
                    }
                    self.number_selection_anchor = None;
                }
                "c" if command => {}
                "escape" | "enter" => {}
                "tab" => {
                    self.editing_number = None;
                    self.number_selection_anchor = None;
                }
                _ => {
                    if let Some(text) = text
                        && !text.chars().any(char::is_control)
                        && !command
                    {
                        if let Some(cursor) = delete_selection(value, selection) {
                            self.number_cursor = cursor;
                        }
                        let byte = value
                            .char_indices()
                            .nth(self.number_cursor)
                            .map_or(value.len(), |(index, _)| index);
                        value.insert_str(byte, text);
                        self.number_cursor += text.chars().count();
                        self.number_selection_anchor = None;
                    } else if key == "v"
                        && command
                        && let Some(text) = text
                    {
                        if let Some(cursor) = delete_selection(value, selection) {
                            self.number_cursor = cursor;
                        }
                        let byte = value
                            .char_indices()
                            .nth(self.number_cursor)
                            .map_or(value.len(), |(index, _)| index);
                        value.insert_str(byte, text);
                        self.number_cursor += text.chars().count();
                        self.number_selection_anchor = None;
                    }
                }
            }
        } else if control.ends_with(":inputs-count") || control.ends_with(":outputs-count") {
            let direction = if control.ends_with(":inputs-count") {
                PortDirection::Input
            } else {
                PortDirection::Output
            };
            let counts = if direction == PortDirection::Input {
                &mut self.input_counts
            } else {
                &mut self.output_counts
            };
            let default = if direction == PortDirection::Input {
                2
            } else {
                1
            };
            let value = counts.entry(node.into()).or_insert(default);
            match key {
                "up" => {
                    *value = value.saturating_sub(1);
                    return Some((direction, *value));
                }
                "down" => {
                    *value = (*value + 1).min(8);
                    return Some((direction, *value));
                }
                "home" => {
                    *value = 0;
                    return Some((direction, 0));
                }
                "end" => {
                    *value = 8;
                    return Some((direction, 8));
                }
                "enter" => {
                    self.open_select = if self.open_select.is_some() {
                        None
                    } else {
                        Some((node.into(), control.into()))
                    };
                }
                "escape" | "tab" => self.open_select = None,
                _ => {
                    if let Ok(current) = text.unwrap_or("").parse::<usize>() {
                        *value = current.min(8);
                        return Some((direction, *value));
                    }
                }
            }
        }
        None
    }
}

fn sync_factor_input(
    editor: &mut NodeGraph<Kind, String, String, String>,
    controls: &DemoControls,
    node: &str,
    control: &str,
    cx: &mut gpui::Context<NodeGraph<Kind, String, String, String>>,
) {
    if let Some(state) = controls.factor_input_state(node, control) {
        editor.set_world_text_input(node.to_owned(), control.to_owned(), state, cx);
    }
}

fn parse_count_option(control: &str) -> Option<(PortDirection, usize)> {
    let (direction, marker) = if control.contains(":inputs-option-") {
        (PortDirection::Input, ":inputs-option-")
    } else {
        (PortDirection::Output, ":outputs-option-")
    };
    control
        .rsplit_once(marker)?
        .1
        .parse()
        .ok()
        .map(|n: usize| (direction, n.min(8)))
}

fn next_id(prefix: &str) -> String {
    let value = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{prefix}_{value}")
}

fn port(id: &str, label: &str, direction: PortDirection, kind: Kind) -> CatalogPort<Kind> {
    CatalogPort {
        id: id.into(),
        label: label.into(),
        direction,
        kind,
    }
}

fn catalog() -> Vec<NodeCatalogItem<Kind>> {
    vec![
        NodeCatalogItem {
            id: "color_source".into(),
            label: "Color Source".into(),
            category: "Input".into(),
            category_color: Some(GraphColor::rgb(0x22d3ee)),
            description: "Produces a color and alpha".into(),
            keywords: Vec::new(),
            ports: vec![
                port("color", "Color", PortDirection::Output, Kind::Color),
                port("alpha", "Alpha", PortDirection::Output, Kind::Float),
            ],
        },
        NodeCatalogItem {
            id: "mix".into(),
            label: "Mix".into(),
            category: "Color".into(),
            category_color: Some(GraphColor::rgb(0xf59e0b)),
            description: "Blend two colors".into(),
            keywords: Vec::new(),
            ports: vec![
                port("a", "A", PortDirection::Input, Kind::Color),
                port("b", "B", PortDirection::Input, Kind::Color),
                port("factor", "Factor", PortDirection::Input, Kind::Float),
                port("result", "Result", PortDirection::Output, Kind::Color),
            ],
        },
        NodeCatalogItem {
            id: "math".into(),
            label: "Math".into(),
            category: "Math".into(),
            category_color: Some(GraphColor::rgb(0x8b5cf6)),
            description: "Arithmetic operation".into(),
            keywords: Vec::new(),
            ports: vec![
                port("a", "A", PortDirection::Input, Kind::Float),
                port("b", "B", PortDirection::Input, Kind::Float),
                port("result", "Result", PortDirection::Output, Kind::Float),
            ],
        },
        NodeCatalogItem {
            id: "output".into(),
            label: "Output".into(),
            category: "Output".into(),
            category_color: Some(GraphColor::rgb(0xef4444)),
            description: "Final output destination".into(),
            keywords: Vec::new(),
            ports: vec![
                port("color", "Color", PortDirection::Input, Kind::Color),
                port("value", "Value", PortDirection::Input, Kind::Any),
            ],
        },
        NodeCatalogItem {
            id: "custom".into(),
            label: "Custom".into(),
            category: "Utility".into(),
            category_color: Some(GraphColor::rgb(0x10b981)),
            description: "Configurable inputs/outputs".into(),
            keywords: Vec::new(),
            ports: Vec::new(),
        },
    ]
}

fn insert_connection(
    graph: &mut GraphState<String, String, String, Kind>,
    source: String,
    target: String,
) {
    let id = next_id("conn");
    graph
        .connections
        .insert(id.clone(), Connection { id, source, target });
}

fn instantiate_node(
    editor: &mut NodeGraph<Kind>,
    catalog: &[NodeCatalogItem<Kind>],
    item_id: &str,
    position: Point,
    connect_from: Option<String>,
    connect_to: Option<&str>,
    connect_direction: Option<PortDirection>,
) {
    let Some(item) = catalog.iter().find(|item| item.id == item_id) else {
        return;
    };
    let node_id = next_id(&item.id);
    let templates = if item.id == "custom" {
        vec![
            port("in_0", "In 0", PortDirection::Input, Kind::Any),
            port("in_1", "In 1", PortDirection::Input, Kind::Any),
            port("out_0", "Out 0", PortDirection::Output, Kind::Any),
        ]
    } else {
        item.ports.clone()
    };
    let input_count = templates
        .iter()
        .filter(|port| port.direction == PortDirection::Input)
        .count();
    let output_count = templates.len() - input_count;
    let rows = input_count.max(output_count);
    let body_height = match item.id.as_str() {
        "mix" => 25.0,
        "custom" => 54.0,
        _ => 0.0,
    };
    let node_size = Size {
        width: if item.id == "mix" { 202.0 } else { 160.0 },
        height: 39.0 + body_height + rows as f32 * 20.0,
    };
    editor.graph.nodes.insert(
        node_id.clone(),
        Node {
            id: node_id.clone(),
            node_type: item.id.clone(),
            title: item.label.clone(),
            position,
            size: node_size,
        },
    );
    let mut input_row = 0;
    let mut output_row = 0;
    let mut auto_port = None;
    for template in &templates {
        let row = if template.direction == PortDirection::Input {
            let row = input_row;
            input_row += 1;
            row
        } else {
            let row = output_row;
            output_row += 1;
            row
        };
        let port_id = format!("{}_{}", node_id, template.id);
        editor.graph.ports.insert(
            port_id.clone(),
            Port {
                id: port_id.clone(),
                node: node_id.clone(),
                label: template.label.clone(),
                direction: template.direction,
                kind: template.kind.clone(),
                position: Point::new(
                    position.x
                        + if template.direction == PortDirection::Input {
                            14.0
                        } else {
                            node_size.width - 14.0
                        },
                    position.y + 45.0 + body_height + row as f32 * 20.0,
                ),
            },
        );
        if connect_to == Some(template.id.as_str()) {
            auto_port = Some(port_id);
        }
    }
    if let (Some(existing), Some(created), Some(direction)) =
        (connect_from, auto_port, connect_direction)
    {
        let (source, target) = if direction == PortDirection::Output {
            (existing, created)
        } else {
            (created, existing)
        };
        insert_connection(&mut editor.graph, source, target);
    }
}

fn leptos_demo_graph() -> GraphState<String, String, String, Kind> {
    let mut graph: GraphState<String, String, String, Kind> = Default::default();
    for (id, node_type, title, position, size) in [
        (
            "color_source_0",
            "color_source",
            "Color Source",
            Point::new(50.0, 50.0),
            Size {
                width: 160.0,
                height: 79.0,
            },
        ),
        (
            "mix_1",
            "mix",
            "Mix",
            Point::new(330.0, 50.0),
            Size {
                width: 202.0,
                height: 124.0,
            },
        ),
    ] {
        graph.nodes.insert(
            id.into(),
            Node {
                id: id.into(),
                node_type: node_type.into(),
                title: title.into(),
                position,
                size,
            },
        );
    }
    for (id, node, label, direction, kind, position) in [
        (
            "color_source_0_color",
            "color_source_0",
            "Color",
            PortDirection::Output,
            Kind::Color,
            Point::new(196.0, 95.0),
        ),
        (
            "color_source_0_alpha",
            "color_source_0",
            "Alpha",
            PortDirection::Output,
            Kind::Float,
            Point::new(196.0, 115.0),
        ),
        (
            "mix_1_a",
            "mix_1",
            "A",
            PortDirection::Input,
            Kind::Color,
            Point::new(344.0, 120.0),
        ),
        (
            "mix_1_b",
            "mix_1",
            "B",
            PortDirection::Input,
            Kind::Color,
            Point::new(344.0, 140.0),
        ),
        (
            "mix_1_factor",
            "mix_1",
            "Factor",
            PortDirection::Input,
            Kind::Float,
            Point::new(344.0, 160.0),
        ),
        (
            "mix_1_result",
            "mix_1",
            "Result",
            PortDirection::Output,
            Kind::Color,
            Point::new(518.0, 120.0),
        ),
    ] {
        graph.ports.insert(
            id.into(),
            Port {
                id: id.into(),
                node: node.into(),
                label: label.into(),
                direction,
                kind,
                position,
            },
        );
    }
    graph.connections.insert(
        "conn_1".into(),
        Connection {
            id: "conn_1".into(),
            source: "color_source_0_color".into(),
            target: "mix_1_b".into(),
        },
    );

    // Keep the two reference nodes at their original coordinates for the guided demo,
    // then seed a deterministic off-canvas stress graph. Panning or Fit View reveals it.
    const SEEDED_NODES: usize = 148;
    for index in 0..SEEDED_NODES {
        let column = index % 15;
        let row = index / 15;
        let node_id = format!("seed_math_{index}");
        let position = Point::new(1_600.0 + column as f32 * 240.0, 50.0 + row as f32 * 150.0);
        let size = Size {
            width: 180.0,
            height: 110.0,
        };
        graph.nodes.insert(
            node_id.clone(),
            Node {
                id: node_id.clone(),
                node_type: "seed".into(),
                title: format!("Seed {index}"),
                position,
                size,
            },
        );
        for (suffix, label, direction, offset) in [
            ("a", "A", PortDirection::Input, Point::new(14.0, 55.0)),
            ("b", "B", PortDirection::Input, Point::new(14.0, 80.0)),
            (
                "result",
                "Result",
                PortDirection::Output,
                Point::new(size.width - 14.0, 55.0),
            ),
        ] {
            let port_id = format!("{node_id}_{suffix}");
            graph.ports.insert(
                port_id.clone(),
                Port {
                    id: port_id,
                    node: node_id.clone(),
                    label: label.into(),
                    direction,
                    kind: Kind::Float,
                    position: position + offset,
                },
            );
        }
    }

    // The original edge plus these 199 deterministic edges yields exactly 200.
    // Each input is occupied at most once while outputs are intentionally fanned out.
    for edge in 0..199usize {
        let target = edge % SEEDED_NODES;
        let target_suffix = if edge < SEEDED_NODES { "a" } else { "b" };
        let source = (target + SEEDED_NODES - 1) % SEEDED_NODES;
        let id = format!("seed_conn_{edge}");
        graph.connections.insert(
            id.clone(),
            Connection {
                id,
                source: format!("seed_math_{source}_result"),
                target: format!("seed_math_{target}_{target_suffix}"),
            },
        );
    }

    graph
}

fn world_text(
    scene: &mut WorldScene,
    x: f32,
    y: f32,
    text: impl Into<String>,
    size: f32,
    color: u32,
) {
    world_text_alpha(scene, x, y, text, size, color, 1.0);
}

fn world_text_alpha(
    scene: &mut WorldScene,
    x: f32,
    y: f32,
    text: impl Into<String>,
    size: f32,
    color: u32,
    alpha: f32,
) {
    scene.push(WorldPrimitive::Text {
        origin: Point::new(x, y),
        lines: TextLines::new([text.into()]),
        color: WorldColor::rgba(color, alpha),
        font_size: size,
        font_weight: if size >= 12.0 { 600 } else { 400 },
        line_height: size + 3.0,
    });
}

fn world_border_line(
    scene: &mut WorldScene,
    start: Point,
    end: Point,
    border: gpui_node_graph::style::Border,
) {
    use gpui_node_graph::style::LineStyle;
    if border.width <= 0.0 || border.style == LineStyle::None {
        return;
    }
    let color = WorldColor::rgba(border.color.rgb, border.color.alpha);
    if border.style == LineStyle::Dashed {
        let mut x = start.x;
        while x < end.x {
            let dash_end = (x + 4.0).min(end.x);
            scene.push(WorldPrimitive::Line {
                start: Point::new(x, start.y),
                end: Point::new(dash_end, end.y),
                color,
                width: border.width,
            });
            x += 7.0;
        }
    } else {
        scene.push(WorldPrimitive::Line {
            start,
            end,
            color,
            width: border.width,
        });
    }
}

fn world_socket(
    scene: &mut WorldScene,
    port: &Port<String, String, Kind>,
    state: gpui_node_graph::WorldPortVisualState,
    style: &gpui_node_graph::style::AnchorStyle,
    node_background: gpui_node_graph::style::Color,
) {
    use gpui_node_graph::style::DotShape;
    let highlighted = state.source || state.snap || state.compatible;
    let color = if highlighted {
        style.dot_compatible_color
    } else if state.connected {
        style.dot_connected_color
    } else {
        style.dot_color
    };
    let opacity = if state.incompatible {
        style.incompatible_opacity
    } else {
        1.0
    };
    let radius = style.dot_size * 0.5;
    let mut push_shape =
        |center: Point, radius: f32, fill: WorldColor| match style.default_dot_shape {
            DotShape::Circle => scene.push(WorldPrimitive::Circle {
                center,
                radius,
                fill,
            }),
            shape => scene.push(WorldPrimitive::Polygon {
                points: gpui_node_graph::dot_shape_points(center, radius, shape),
                fill,
            }),
        };
    if state.source || state.compatible {
        for shadow in style.dot_compatible_glow.iter().rev() {
            push_shape(
                Point::new(
                    port.position.x + shadow.offset_x,
                    port.position.y + shadow.offset_y,
                ),
                radius + shadow.spread + shadow.blur * 0.5,
                WorldColor::rgba(shadow.color.rgb, shadow.color.alpha * 0.22),
            );
        }
    }
    push_shape(
        port.position,
        radius,
        WorldColor::rgba(color.rgb, color.alpha * opacity),
    );
    if !state.connected && !highlighted {
        push_shape(
            port.position,
            (radius - style.dot_border_width).max(0.0),
            WorldColor::rgba(node_background.rgb, node_background.alpha * opacity),
        );
    }
}

fn effective_node_type(node: &Node<String>) -> &str {
    if !node.node_type.is_empty() {
        return &node.node_type;
    }
    match node.title.as_str() {
        "Color Source" => "color_source",
        "Mix" => "mix",
        "Math" => "math",
        "Output" => "output",
        "Custom" => "custom",
        _ => "",
    }
}

#[cfg(test)]
fn leptos_world_node(context: WorldNodeBodyContext<Kind, String, String>) -> WorldScene {
    leptos_world_node_with_values(context, "Normal", &std::collections::HashMap::new(), None)
}

fn leptos_world_node_with_values(
    context: WorldNodeBodyContext<Kind, String, String>,
    blend: &str,
    factors: &std::collections::HashMap<String, String>,
    factor_edit: Option<(String, usize, Option<usize>)>,
) -> WorldScene {
    let mut scene = WorldScene::new();
    let node = &context.node;
    let node_style = &context.theme.node;
    let anchor_style = &context.theme.anchor;
    let node_type = effective_node_type(node);
    let (category, accent) = match node_type {
        "color_source" => ("INPUT", 0x22d3ee),
        "mix" => ("COLOR", 0xf59e0b),
        "math" => ("MATH", 0x8b5cf6),
        "output" => ("OUTPUT", 0xef4444),
        _ => ("UTILITY", 0x10b981),
    };
    scene.push(WorldPrimitive::Quad {
        bounds: Rect {
            origin: node.position,
            size: Size {
                width: node.size.width,
                height: node_style.header_accent_height,
            },
        },
        fill: WorldColor::rgb(accent),
        corner_radius: 0.0,
    });
    world_text(
        &mut scene,
        node.position.x + node_style.padding_x,
        node.position.y + node_style.header_accent_height + node_style.header_padding_y + 2.0,
        node.title.to_uppercase(),
        node_style.header_font_size,
        node_style.header_color.rgb,
    );
    let category_width = category.len() as f32 * 6.0;
    world_text(
        &mut scene,
        node.position.x + node.size.width - node_style.padding_x - category_width,
        node.position.y + node_style.header_accent_height + node_style.header_padding_y + 2.0,
        category,
        10.0,
        accent,
    );

    world_border_line(
        &mut scene,
        Point::new(node.position.x, node.position.y + 28.0),
        Point::new(node.position.x + node.size.width, node.position.y + 28.0),
        node_style.header_border_bottom,
    );

    if node_type == "custom" {
        for (row, (label, count, control)) in [
            (
                "Inputs",
                context
                    .ports
                    .iter()
                    .filter(|port| port.direction == PortDirection::Input)
                    .count(),
                "inputs-count",
            ),
            (
                "Outputs",
                context
                    .ports
                    .iter()
                    .filter(|port| port.direction == PortDirection::Output)
                    .count(),
                "outputs-count",
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let y = node.position.y
                + 31.0
                + node_style.body_padding_y
                + row as f32 * (22.0 + node_style.field_gap);
            world_text(
                &mut scene,
                node.position.x + node_style.padding_x,
                y + 4.0,
                label.to_uppercase(),
                node_style.field_label_font_size,
                node_style.field_label_color.rgb,
            );
            let select = Rect {
                origin: Point::new(
                    node.position.x
                        + node_style.padding_x
                        + node_style.field_label_min_width
                        + node_style.field_gap,
                    y,
                ),
                size: Size {
                    width: node.size.width - 66.0,
                    height: 22.0,
                },
            };
            scene.push(WorldPrimitive::BorderedQuad {
                bounds: select,
                fill: WorldColor::rgb(0x27272a),
                border: WorldColor::rgb(0x3f3f46),
                border_width: 1.0,
                corner_radius: 4.0,
            });
            world_text(
                &mut scene,
                select.origin.x + 7.0,
                select.origin.y + 4.0,
                count.to_string(),
                11.0,
                0xd4d4d8,
            );
            scene.push_hit_region(
                WorldHitRegion::new(
                    format!("{}:{control}", node.id),
                    HitRole::Control,
                    HitShape::Rect(select),
                )
                .with_accessible_label(if control == "input-count" {
                    "Input count"
                } else {
                    "Output count"
                })
                .with_accessible_role(AccessibleControlRole::SpinButton)
                .with_accessible_numeric_range(count as f64, 1.0, 8.0, 1.0),
            );
        }
    }

    if node_type == "mix" {
        world_text(
            &mut scene,
            node.position.x + node_style.padding_x,
            node.position.y + 32.0 + node_style.body_padding_y,
            "Blend",
            node_style.field_label_font_size,
            node_style.field_label_color.rgb,
        );
        let blend_select = Rect {
            origin: Point::new(
                node.position.x
                    + node_style.padding_x
                    + node_style.field_label_min_width
                    + node_style.field_gap,
                node.position.y + 28.5 + node_style.body_padding_y,
            ),
            size: Size {
                width: 107.0,
                height: 22.0,
            },
        };
        scene.push(WorldPrimitive::BorderedQuad {
            bounds: blend_select,
            fill: WorldColor::rgb(0x27272a),
            border: WorldColor::rgb(0x3f3f46),
            border_width: 1.0,
            corner_radius: 4.0,
        });
        world_text(
            &mut scene,
            blend_select.origin.x + 10.0,
            node.position.y + 32.0 + node_style.body_padding_y,
            blend,
            11.0,
            0xd4d4d8,
        );
        scene.push_hit_region(
            WorldHitRegion::new(
                format!("{}:blend-select", node.id),
                HitRole::Control,
                HitShape::Rect(blend_select),
            )
            .with_accessible_label("Blend mode")
            .with_accessible_role(AccessibleControlRole::ComboBox)
            .with_accessible_value(blend),
        );
        scene.push(WorldPrimitive::BorderedQuad {
            bounds: Rect {
                origin: Point::new(node.position.x + 167.0, node.position.y + 29.0),
                size: Size {
                    width: 25.0,
                    height: 25.0,
                },
            },
            fill: WorldColor::rgb(0x27272a),
            border: WorldColor::rgb(0x3f3f46),
            border_width: 1.0,
            corner_radius: 4.0,
        });
        scene.push(WorldPrimitive::Line {
            start: Point::new(node.position.x + 175.0, node.position.y + 46.0),
            end: Point::new(node.position.x + 184.0, node.position.y + 37.0),
            color: WorldColor::rgb(0xd4d4d8),
            width: 1.5,
        });
        scene.push(WorldPrimitive::Line {
            start: Point::new(node.position.x + 174.0, node.position.y + 47.0),
            end: Point::new(node.position.x + 177.0, node.position.y + 46.0),
            color: WorldColor::rgb(0xd4d4d8),
            width: 1.5,
        });
        scene.push_hit_region(
            WorldHitRegion::new(
                format!("{}:mix-amount", node.id),
                HitRole::Control,
                HitShape::Rect(Rect {
                    origin: Point::new(node.position.x + 167.0, node.position.y + 29.0),
                    size: Size {
                        width: 25.0,
                        height: 25.0,
                    },
                }),
            )
            .with_accessible_label("Mix amount"),
        );
    }

    if matches!(node_type, "mix" | "custom")
        && let Some(first_port_y) = context
            .ports
            .iter()
            .map(|port| port.position.y)
            .min_by(f32::total_cmp)
    {
        let border_y = first_port_y - anchor_style.row_height * 0.5 - node_style.ports_padding_y;
        world_border_line(
            &mut scene,
            Point::new(node.position.x, border_y),
            Point::new(node.position.x + node.size.width, border_y),
            node_style.body_border_bottom,
        );
    }

    for port in context.ports.iter() {
        let port_state = context.port_state(&port.id);
        world_socket(
            &mut scene,
            port,
            port_state,
            anchor_style,
            node_style.background,
        );
        let (x, y) = match (node_type, port.label.as_str(), port.direction) {
            ("color_source", "Color", _) => (node.position.x + 107.0, port.position.y - 6.0),
            ("color_source", "Alpha", _) => (node.position.x + 107.0, port.position.y - 6.0),
            ("mix", "Result", _) => (node.position.x + 148.0, port.position.y - 6.0),
            (_, _, PortDirection::Input) => (port.position.x + 9.0, port.position.y - 6.0),
            (_, _, PortDirection::Output) => (port.position.x - 50.0, port.position.y - 6.0),
        };
        world_text_alpha(
            &mut scene,
            x,
            y,
            port.label.clone(),
            anchor_style.label_font_size,
            if port_state.compatible {
                anchor_style.label_compatible_color.rgb
            } else {
                anchor_style.label_color.rgb
            },
            if port_state.incompatible {
                anchor_style.incompatible_opacity
            } else {
                1.0
            },
        );
        if port.direction == PortDirection::Input && port.kind == Kind::Float {
            let factor_control_id = format!("{}:{}:factor-value", node.id, port.id);
            let factor = factors
                .get(&factor_control_id)
                .map(String::as_str)
                .unwrap_or("0.0");
            let active_factor_edit = factor_edit
                .as_ref()
                .filter(|(control, _, _)| control == &factor_control_id)
                .map(|(_, cursor, anchor)| (*cursor, *anchor));
            scene.push(WorldPrimitive::BorderedQuad {
                bounds: Rect {
                    origin: Point::new(node.position.x + 60.0, port.position.y - 9.0),
                    size: Size {
                        width: 66.0,
                        height: 18.0,
                    },
                },
                fill: WorldColor::rgb(0x27272a),
                border: WorldColor::rgb(0x3f3f46),
                border_width: 1.0,
                corner_radius: 4.0,
            });
            let factor_character_width = 16.0 / 3.0;
            let factor_text_x =
                node.position.x + 119.0 - factor.chars().count() as f32 * factor_character_width;
            if let Some((cursor, anchor)) = active_factor_edit {
                if let Some(anchor) = anchor
                    && anchor != cursor
                {
                    let selection_start = anchor.min(cursor) as f32;
                    let selection_width = anchor.abs_diff(cursor) as f32;
                    scene.push(WorldPrimitive::Quad {
                        bounds: Rect {
                            origin: Point::new(
                                factor_text_x + selection_start * factor_character_width,
                                port.position.y - 7.0,
                            ),
                            size: Size {
                                width: selection_width * factor_character_width,
                                height: 13.0,
                            },
                        },
                        fill: WorldColor::rgba(0x2563eb, 0.55),
                        corner_radius: 0.0,
                    });
                }
                scene.push(WorldPrimitive::Quad {
                    bounds: Rect {
                        origin: Point::new(
                            factor_text_x + cursor as f32 * factor_character_width,
                            port.position.y - 7.0,
                        ),
                        size: Size {
                            width: 1.0,
                            height: 13.0,
                        },
                    },
                    fill: WorldColor::rgb(0xd4d4d8),
                    corner_radius: 0.0,
                });
            }
            world_text(
                &mut scene,
                factor_text_x,
                port.position.y - 6.0,
                factor.to_string(),
                11.0,
                0xd4d4d8,
            );
            scene.push_hit_region(
                WorldHitRegion::new(
                    factor_control_id,
                    HitRole::Control,
                    HitShape::Rect(Rect {
                        origin: Point::new(node.position.x + 60.0, port.position.y - 9.0),
                        size: Size {
                            width: 66.0,
                            height: 18.0,
                        },
                    }),
                )
                .with_accessible_label("Factor value")
                .with_accessible_role(AccessibleControlRole::TextInput)
                .with_accessible_value(factor),
            );
        }
    }
    scene
}

fn set_custom_port_count(
    editor: &mut NodeGraph<Kind>,
    node_id: &str,
    direction: PortDirection,
    next: usize,
    cx: &mut gpui::Context<NodeGraph<Kind>>,
) {
    let Some(node) = editor.graph.nodes.get(node_id).cloned() else {
        return;
    };
    let prefix = if direction == PortDirection::Input {
        "in"
    } else {
        "out"
    };
    let current = editor
        .graph
        .ports
        .values()
        .filter(|port| port.node == node_id && port.direction == direction)
        .count();
    let next = next.min(8);
    if next < current {
        let ids: Vec<_> = editor
            .graph
            .ports
            .values()
            .filter(|port| port.node == node_id && port.direction == direction)
            .filter_map(|port| {
                let index = port
                    .label
                    .split_whitespace()
                    .last()?
                    .parse::<usize>()
                    .ok()?;
                (index >= next).then(|| port.id.clone())
            })
            .collect();
        for id in ids {
            editor.remove_port_with_tombstones(&id, cx);
        }
    } else {
        for index in current..next {
            let id = format!("{node_id}_{prefix}_{index}");
            editor.graph.ports.insert(
                id.clone(),
                Port {
                    id,
                    node: node_id.to_string(),
                    label: format!(
                        "{} {index}",
                        if direction == PortDirection::Input {
                            "In"
                        } else {
                            "Out"
                        }
                    ),
                    direction,
                    kind: Kind::Any,
                    position: Point::new(0.0, 0.0),
                },
            );
        }
    }
    let inputs = editor
        .graph
        .ports
        .values()
        .filter(|port| port.node == node_id && port.direction == PortDirection::Input)
        .count();
    let outputs = editor
        .graph
        .ports
        .values()
        .filter(|port| port.node == node_id && port.direction == PortDirection::Output)
        .count();
    let height = 39.0 + 54.0 + inputs.max(outputs) as f32 * 20.0;
    if let Some(node) = editor.graph.nodes.get_mut(node_id) {
        node.size.height = height;
    }
    for port in editor
        .graph
        .ports
        .values_mut()
        .filter(|port| port.node == node_id)
    {
        let index = port
            .label
            .split_whitespace()
            .last()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        port.position = Point::new(
            node.position.x
                + if port.direction == PortDirection::Input {
                    14.0
                } else {
                    node.size.width - 14.0
                },
            node.position.y + 99.0 + index as f32 * 20.0,
        );
    }
    cx.notify();
}

fn launch(cx: &mut App) {
    gpui_node_graph::init(cx);
    gpui_node_graph::set_node_graph_theme(cx, gpui_node_graph::NodeGraphTheme::leptos_demo());
    #[cfg(not(target_arch = "wasm32"))]
    let window_bounds = Some(WindowBounds::Windowed(Bounds::centered(
        None,
        size(px(1000.), px(700.)),
        cx,
    )));
    #[cfg(target_arch = "wasm32")]
    let window_bounds = None;
    cx.open_window(
        WindowOptions {
            window_bounds,
            ..Default::default()
        },
        |_, cx| {
            let catalog = catalog();
            let editor_catalog = catalog.clone();
            let open_overlays = std::rc::Rc::new(std::cell::RefCell::new(
                std::collections::HashSet::<String>::new(),
            ));
            let renderer_overlays = open_overlays.clone();
            let renderer_amount_overlays = open_overlays.clone();
            let event_overlays = open_overlays.clone();
            let controls = std::rc::Rc::new(std::cell::RefCell::new(DemoControls::default()));
            TEST_CONTROLS.with(|slot| *slot.borrow_mut() = Some(controls.clone()));
            let renderer_controls = controls.clone();
            let overlay_controls = controls.clone();
            let event_controls = controls.clone();
            let graph = cx.new(move |cx| {
                let graph_weak = cx.entity().downgrade();
                let graph = leptos_demo_graph();
                NodeGraph::new(graph, cx)
                    .with_world_node_body_renderer(
                        move |context: WorldNodeBodyContext<Kind, String, String>| {
                            let (blend, factors, factor_edit) = {
                                let controls = renderer_controls.borrow();
                                let editing = controls.editing_number.as_ref().and_then(
                                    |(node, control, _)| {
                                        (node == &context.node.id).then_some((
                                            control.clone(),
                                            controls.number_cursor,
                                            controls.number_selection_anchor,
                                        ))
                                    },
                                );
                                (
                                    controls.blend_label(&context.node.id).to_string(),
                                    controls.factors.clone(),
                                    editing,
                                )
                            };
                            let node_id = context.node.id.clone();
                            let mut scene = leptos_world_node_with_values(
                                context,
                                &blend,
                                &factors,
                                factor_edit,
                            );
                            if renderer_amount_overlays.borrow().contains(&node_id)
                                && let Some(index) = scene
                                    .hit_regions
                                    .iter()
                                    .position(|hit| hit.id.ends_with(":mix-amount"))
                            {
                                scene.hit_regions.insert(
                                    index + 1,
                                    WorldHitRegion::new(
                                        format!("{node_id}:mix-range"),
                                        HitRole::Control,
                                        HitShape::Rect(Rect {
                                            origin: Point::new(-1_000_000.0, -1_000_000.0),
                                            size: Size {
                                                width: 0.0,
                                                height: 0.0,
                                            },
                                        }),
                                    )
                                    .with_accessible_label("Mix range")
                                    .with_accessible_role(AccessibleControlRole::Slider)
                                    .with_accessible_numeric_range(0.75, 0.0, 1.0, 0.05),
                                );
                            }
                            scene
                        },
                    )
                    .with_catalog(editor_catalog)
                    .with_node_overlay_renderer(
                        move |context: WorldNodeBodyContext<Kind, String, String>,
                              _: &mut gpui::Window,
                              _: &mut App| {
                            let mut overlays = Vec::new();
                            let open_select = overlay_controls
                                .borrow()
                                .open_select
                                .clone()
                                .filter(|(node, _)| node == &context.node.id);
                            if let Some((node_id, control_id)) = open_select {
                                let (offset, width, option_prefix, options, selected) =
                                    if control_id.ends_with(":blend-select") {
                                        (
                                            Point::new(54.0, 52.5),
                                            107.0,
                                            "blend-option",
                                            BLEND_OPTIONS
                                                .iter()
                                                .map(|option| (*option).to_string())
                                                .collect::<Vec<_>>(),
                                            overlay_controls
                                                .borrow()
                                                .blends
                                                .get(&node_id)
                                                .copied()
                                                .unwrap_or(0),
                                        )
                                    } else {
                                        let inputs = control_id.ends_with(":inputs-count");
                                        let selected = if inputs {
                                            overlay_controls
                                                .borrow()
                                                .input_counts
                                                .get(&node_id)
                                                .copied()
                                                .unwrap_or(2)
                                        } else {
                                            overlay_controls
                                                .borrow()
                                                .output_counts
                                                .get(&node_id)
                                                .copied()
                                                .unwrap_or(1)
                                        };
                                        (
                                            Point::new(56.0, if inputs { 55.0 } else { 83.0 }),
                                            context.node.size.width - 66.0,
                                            if inputs {
                                                "inputs-option"
                                            } else {
                                                "outputs-option"
                                            },
                                            (0..=8).map(|value| value.to_string()).collect(),
                                            selected,
                                        )
                                    };
                                let mut panel = gpui::div()
                                    .w(gpui::px(width))
                                    .flex()
                                    .flex_col()
                                    .rounded(gpui::px(4.0))
                                    .border_1()
                                    .border_color(gpui::rgb(0x3f3f46))
                                    .bg(gpui::rgb(0x27272a))
                                    .shadow(vec![gpui::BoxShadow {
                                        color: gpui::rgba(0x00000080).into(),
                                        offset: gpui::point(gpui::px(0.0), gpui::px(6.0)),
                                        blur_radius: gpui::px(14.0),
                                        spread_radius: gpui::px(0.0),
                                        inset: false,
                                    }]);
                                for (index, label) in options.into_iter().enumerate() {
                                    let option_id = format!("{node_id}:{option_prefix}-{index}");
                                    let option_node = node_id.clone();
                                    let option_controls = overlay_controls.clone();
                                    let option_graph = graph_weak.clone();
                                    panel = panel.child(
                                        gpui::div()
                                            .id(("node-select-option", index))
                                            .h(gpui::px(22.0))
                                            .px(gpui::px(7.0))
                                            .flex()
                                            .items_center()
                                            .text_size(gpui::px(11.0))
                                            .text_color(gpui::rgb(0xd4d4d8))
                                            .when(index == selected, |row| {
                                                row.bg(gpui::rgb(0x3f3f46))
                                            })
                                            .hover(|row| row.bg(gpui::rgb(0x52525b)))
                                            .child(label)
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                move |_, window, cx| {
                                                    let mutation = option_controls
                                                        .borrow_mut()
                                                        .activate(&option_node, &option_id);
                                                    let _ =
                                                        option_graph.update(cx, |editor, cx| {
                                                            if let Some((direction, count)) =
                                                                mutation
                                                            {
                                                                set_custom_port_count(
                                                                    editor,
                                                                    &option_node,
                                                                    direction,
                                                                    count,
                                                                    cx,
                                                                );
                                                            } else {
                                                                cx.notify();
                                                            }
                                                        });
                                                    cx.stop_propagation();
                                                    window.prevent_default();
                                                },
                                            ),
                                    );
                                }
                                overlays.push(NodeOverlay::new(offset, panel));
                            }

                            if effective_node_type(&context.node) == "mix"
                                && renderer_overlays.borrow().contains(&context.node.id)
                            {
                                let node_id = context.node.id.clone();
                                let amount = MIX_AMOUNTS.with(|amounts| {
                                    amounts.borrow().get(&node_id).copied().unwrap_or(0.5)
                                });
                                let slider_steps = (0..=100)
                                    .map(|step| {
                                        let down_node_id = node_id.clone();
                                        let move_node_id = node_id.clone();
                                        let focus_node_id = node_id.clone();
                                        let focus_graph = graph_weak.clone();
                                        let value = step as f32 / 100.0;
                                        gpui::div()
                                            .absolute()
                                            .left(gpui::px(2.0 + step as f32 * 1.72))
                                            .top_0()
                                            .w(gpui::px(2.0))
                                            .h(gpui::px(14.0))
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                move |_, window, cx| {
                                                    MIX_AMOUNTS.with(|amounts| {
                                                        amounts
                                                            .borrow_mut()
                                                            .insert(down_node_id.clone(), value);
                                                    });
                                                    let _ = focus_graph.update(cx, |editor, cx| {
                                                        editor.focus_world_control(
                                                            focus_node_id.clone(),
                                                            format!("{}:mix-range", focus_node_id),
                                                            cx,
                                                        );
                                                    });
                                                    cx.refresh_windows();
                                                    cx.stop_propagation();
                                                    window.prevent_default();
                                                },
                                            )
                                            .on_mouse_move(move |event, window, cx| {
                                                if event.pressed_button
                                                    == Some(gpui::MouseButton::Left)
                                                {
                                                    MIX_AMOUNTS.with(|amounts| {
                                                        amounts
                                                            .borrow_mut()
                                                            .insert(move_node_id.clone(), value);
                                                    });
                                                    cx.refresh_windows();
                                                    cx.stop_propagation();
                                                    window.prevent_default();
                                                }
                                            })
                                    })
                                    .collect::<Vec<_>>();
                                overlays.push(
                                    NodeOverlay::new(
                                        Point::new(167.0, 29.0),
                                        gpui::div()
                                            .w(gpui::px(200.0))
                                            .flex()
                                            .flex_col()
                                            .gap_2()
                                            .rounded(gpui::px(6.0))
                                            .border_1()
                                            .border_color(gpui::rgb(0x3f3f46))
                                            .bg(gpui::rgb(0x18181b))
                                            .p(gpui::px(10.0))
                                            .text_size(gpui::px(11.0))
                                            .child(
                                                gpui::div()
                                                    .text_size(gpui::px(10.0))
                                                    .text_color(gpui::rgb(0xa1a1aa))
                                                    .child("MIX AMOUNT"),
                                            )
                                            .child(
                                                gpui::div()
                                                    .relative()
                                                    .h(gpui::px(14.0))
                                                    .child(
                                                        gpui::div()
                                                            .absolute()
                                                            .left(gpui::px(2.0))
                                                            .right(gpui::px(2.0))
                                                            .top(gpui::px(5.0))
                                                            .h(gpui::px(4.0))
                                                            .rounded_full()
                                                            .bg(gpui::rgb(0x71717a)),
                                                    )
                                                    .child(
                                                        gpui::div()
                                                            .absolute()
                                                            .left(gpui::px(2.0 + amount * 164.0))
                                                            .top(gpui::px(0.0))
                                                            .w(gpui::px(14.0))
                                                            .h(gpui::px(14.0))
                                                            .rounded_full()
                                                            .bg(gpui::rgb(0x93c5fd)),
                                                    )
                                                    .children(slider_steps),
                                            )
                                            .child(format!("{amount:.2}"))
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                |_, window, cx| {
                                                    cx.stop_propagation();
                                                    window.prevent_default();
                                                },
                                            ),
                                    )
                                    .with_screen_offset(Point::new(8.0, 0.0))
                                    .adaptive(
                                        "mix-amount",
                                        Size {
                                            width: 200.0,
                                            height: 86.0,
                                        },
                                    ),
                                );
                            }
                            overlays
                        },
                    )
                    .with_overlay_placement(
                        "mix-amount",
                        OverlayPlacement {
                            side: OverlaySide::Right,
                            align: OverlayAlign::Start,
                            anchor_size: Size {
                                width: 25.0,
                                height: 25.0,
                            },
                            gap: 8.0,
                            flip: true,
                            clamp_to_canvas: true,
                        },
                    )
                    .with_overlay_anchor_control("mix-amount", "mix_1:mix-amount")
                    .with_groups(vec![GraphGroup {
                        id: "group_0".into(),
                        label: Some("Group 1".into()),
                        color: Some(gpui_node_graph::style::Color::rgb(0x8b5cf6)),
                        error: false,
                        nodes: ["color_source_0".to_string(), "mix_1".to_string()]
                            .into_iter()
                            .collect(),
                    }])
            });
            graph.update(cx, |_, cx| {
                cx.subscribe(&graph, move |editor, _, event, cx| {
                    match event {
                        GraphEvent::NodeControlActivated {
                            node_id,
                            control_id,
                        } if !control_id.ends_with(":mix-amount") => {
                            if let Some((direction, count)) =
                                event_controls.borrow_mut().activate(node_id, control_id)
                            {
                                set_custom_port_count(editor, node_id, direction, count, cx);
                            }
                            sync_factor_input(
                                editor,
                                &event_controls.borrow(),
                                node_id,
                                control_id,
                                cx,
                            );
                        }
                        GraphEvent::NodeControlKeyDown {
                            node_id,
                            control_id,
                            key,
                            ..
                        } if control_id.ends_with(":mix-range") => {
                            let mut amount = MIX_AMOUNTS.with(|amounts| {
                                amounts.borrow().get(node_id).copied().unwrap_or(0.5)
                            });
                            amount = match key.as_str() {
                                "left" | "down" => amount - 0.01,
                                "right" | "up" => amount + 0.01,
                                "pagedown" => amount - 0.1,
                                "pageup" => amount + 0.1,
                                "home" => 0.0,
                                "end" => 1.0,
                                _ => amount,
                            }
                            .clamp(0.0, 1.0);
                            MIX_AMOUNTS.with(|amounts| {
                                amounts.borrow_mut().insert(node_id.clone(), amount);
                            });
                            if key == "escape" {
                                event_overlays.borrow_mut().remove(node_id);
                                editor.dismiss_overlay("mix-amount", cx);
                                editor.focus_world_control(
                                    node_id.clone(),
                                    format!("{node_id}:mix-amount"),
                                    cx,
                                );
                            }
                        }
                        GraphEvent::NodeControlKeyDown {
                            node_id,
                            control_id,
                            key,
                            ..
                        } if control_id.ends_with(":mix-amount")
                            && matches!(key.as_str(), "escape" | "enter") =>
                        {
                            let mut overlays = event_overlays.borrow_mut();
                            if key == "escape" {
                                overlays.remove(node_id);
                                editor.dismiss_overlay("mix-amount", cx);
                            } else if editor.is_overlay_dismissed("mix-amount") {
                                overlays.insert(node_id.clone());
                                editor.reopen_overlay("mix-amount", cx);
                            } else if !overlays.remove(node_id) {
                                overlays.insert(node_id.clone());
                            }
                        }
                        GraphEvent::NodeControlKeyDown {
                            node_id,
                            control_id,
                            key,
                            text,
                            command,
                            shift,
                            ..
                        } => {
                            if *command
                                && matches!(key.as_str(), "c" | "x")
                                && let Some(selected) = event_controls
                                    .borrow()
                                    .selected_factor_text(node_id, control_id)
                            {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(selected));
                            }
                            // Printable text and paste are inserted by GPUI's InputHandler.
                            // Structural keys remain in the existing browser-compatible path.
                            let platform_inserts = control_id.ends_with(":factor-value")
                                && ((!*command && text.is_some()) || (*command && key == "v"));
                            if !platform_inserts
                                && let Some((direction, count)) =
                                    event_controls.borrow_mut().key_down(
                                        node_id,
                                        control_id,
                                        key,
                                        text.as_deref(),
                                        *command,
                                        *shift,
                                    )
                            {
                                set_custom_port_count(editor, node_id, direction, count, cx);
                            }
                            sync_factor_input(
                                editor,
                                &event_controls.borrow(),
                                node_id,
                                control_id,
                                cx,
                            );
                        }
                        GraphEvent::NodeControlActivated {
                            node_id,
                            control_id,
                        } if control_id.ends_with(":mix-amount") => {
                            let mut overlays = event_overlays.borrow_mut();
                            if editor.is_overlay_dismissed("mix-amount") {
                                overlays.insert(node_id.clone());
                                editor.reopen_overlay("mix-amount", cx);
                            } else if !overlays.remove(node_id) {
                                overlays.insert(node_id.clone());
                            }
                        }
                        GraphEvent::NodeOverlayDismissed { id } if id == "mix-amount" => {
                            event_overlays.borrow_mut().clear();
                        }
                        GraphEvent::NodeControlPointerActivated {
                            node_id,
                            control_id,
                            world_position,
                            click_count,
                        } => {
                            if let Some(node) = editor.graph.nodes.get(node_id) {
                                event_controls.borrow_mut().pointer_activate(
                                    node_id,
                                    control_id,
                                    *world_position,
                                    node.position.x,
                                    *click_count,
                                );
                            }
                            sync_factor_input(
                                editor,
                                &event_controls.borrow(),
                                node_id,
                                control_id,
                                cx,
                            );
                        }
                        GraphEvent::NodeControlFocused {
                            node_id,
                            control_id,
                        } => {
                            event_controls.borrow_mut().focus(node_id, control_id);
                            sync_factor_input(
                                editor,
                                &event_controls.borrow(),
                                node_id,
                                control_id,
                                cx,
                            );
                        }
                        GraphEvent::NodeControlTextChanged {
                            node_id,
                            control_id,
                            text,
                            selection,
                            selection_reversed,
                            ..
                        } => {
                            event_controls.borrow_mut().apply_platform_text(
                                node_id,
                                control_id,
                                text,
                                selection.clone(),
                                *selection_reversed,
                            );
                        }
                        GraphEvent::NodeControlBlurred {
                            node_id,
                            control_id,
                        } => {
                            event_controls.borrow_mut().blur(node_id, control_id);
                        }
                        GraphEvent::ConnectionRequested { source, target } => {
                            insert_connection(&mut editor.graph, source.clone(), target.clone());
                        }
                        GraphEvent::CreateNode {
                            item_id,
                            position,
                            connect_from,
                            connect_to,
                            connect_direction,
                        } => instantiate_node(
                            editor,
                            &catalog,
                            item_id,
                            *position,
                            connect_from.clone(),
                            connect_to.as_deref(),
                            *connect_direction,
                        ),
                        GraphEvent::GroupCreated { node_ids } if node_ids.len() > 1 => {
                            let sequence = editor.groups().len() + 1;
                            editor.upsert_group(
                                GraphGroup {
                                    id: format!("group-{sequence}"),
                                    label: Some(format!("Group {sequence}")),
                                    color: Some(gpui_node_graph::style::Color::rgb(0xa78bfa)),
                                    error: false,
                                    nodes: node_ids.iter().cloned().collect(),
                                },
                                cx,
                            );
                        }
                        _ => return,
                    }
                    cx.notify();
                })
                .detach();
            });
            #[cfg(target_family = "wasm")]
            TEST_GRAPH.with(|slot| *slot.borrow_mut() = Some(graph.clone()));
            graph
        },
    )
    .unwrap();
    cx.activate(true);
    cx.refresh_windows();
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    gpui_platform::application().run(launch);
}

#[cfg(target_family = "wasm")]
thread_local! {
    static APPLICATION: std::cell::RefCell<Option<gpui::ApplicationHandle>> = const {
        std::cell::RefCell::new(None)
    };
    static TEST_GRAPH: std::cell::RefCell<Option<gpui::Entity<NodeGraph<Kind>>>> = const {
        std::cell::RefCell::new(None)
    };
}

#[cfg(target_family = "wasm")]
fn browser_test_state() -> String {
    APPLICATION.with(|application| {
        let application = application.borrow();
        let application = application
            .as_ref()
            .expect("embedded application is retained");
        TEST_GRAPH.with(|graph| {
            let graph = graph.borrow();
            let graph = graph.as_ref().expect("node graph entity is retained");
            application.update(|cx| {
                let graph = graph.read(cx);
                let (select_open, blend, factor_text) = TEST_CONTROLS.with(|slot| {
                    let controls = slot.borrow();
                    let controls = controls.as_ref().expect("demo controls are retained").borrow();
                    (
                        controls.open_select.is_some(),
                        controls.blend_label("mix_1").to_string(),
                        controls.factor_text("mix_1").to_string(),
                    )
                });
                let custom_inputs = graph
                    .graph
                    .nodes
                    .values()
                    .find(|node| effective_node_type(node) == "custom")
                    .map_or(0, |node| {
                        graph
                            .graph
                            .ports
                            .values()
                            .filter(|port| {
                                port.node == node.id && port.direction == PortDirection::Input
                            })
                            .count()
                    });
                format!(
                    r#"{{"nodes":{},"connections":{},"catalogOpen":{},"catalogDraft":{},"activeDraft":{},"catalogEntries":{},"catalogSelected":{},"selectOpen":{},"blend":"{}","factorText":"{}","customInputs":{},"overlayDismissed":{},"zoom":{},"sourceWidth":{},"sourceHeight":{},"controlActivated":{},"lastControl":"{}","activeOverlays":{},"anchorTooltip":{},"anchorMenu":{},"textInput":{},"textInputActive":{},"sourceConnected":{},"mixAmount":{},"selectedNodes":{},"mixX":{},"mixY":{},"mixWidth":{},"panX":{},"panY":{},"worldLayout":"{}"}}"#,
                    graph.graph.nodes.len(),
                    graph.graph.connections.len(),
                    graph.catalog_is_open(),
                    graph.catalog_connects_draft(),
                    graph.has_active_draft(),
                    graph.catalog_entry_count(),
                    graph.catalog_selected_entry().unwrap_or(0),
                    select_open,
                    blend,
                    factor_text,
                    custom_inputs,
                    graph.is_overlay_dismissed("mix-amount"),
                    graph.graph.viewport.zoom,
                    graph
                        .resolved_node_size(&String::from("color_source_0"))
                        .map_or(0.0, |size| size.width),
                    graph
                        .resolved_node_size(&String::from("color_source_0"))
                        .map_or(0.0, |size| size.height),
                    graph.last_world_control().is_some(),
                    graph.last_world_control().map_or("", |(_, control)| control),
                    graph.active_overlay_count(),
                    graph.hovered_port().is_some(),
                    graph.anchor_menu_is_open(),
                    graph.world_text_input().is_some(),
                    graph.world_text_input_is_active(),
                    graph
                        .port_visual_state(&"color_source_0_color".to_string())
                        .is_some_and(|state| state.connected),
                    MIX_AMOUNTS.with(|amounts| {
                        amounts.borrow().get("mix_1").copied().unwrap_or(0.5)
                    }),
                    graph.graph.selected_nodes.len(),
                    graph.graph.nodes["mix_1"].position.x,
                    graph.graph.nodes["mix_1"].position.y,
                    graph.graph.nodes["mix_1"].size.width,
                    graph.graph.viewport.pan.x,
                    graph.graph.viewport.pan.y,
                    graph.world_layout_fingerprint(),
                )
            })
        })
    })
}

#[cfg(target_family = "wasm")]
fn install_browser_test_bridge() {
    use wasm_bindgen::{JsCast, JsValue, closure::Closure};
    let snapshot = Closure::<dyn Fn() -> String>::new(browser_test_state);
    js_sys::Reflect::set(
        &js_sys::global(),
        &JsValue::from_str("__nodeGraphTestState"),
        snapshot.as_ref().unchecked_ref(),
    )
    .expect("globalThis accepts the browser test bridge");
    snapshot.forget();

    let drop_node =
        Closure::<dyn Fn(JsValue, f64, f64) -> bool>::new(|item_id: JsValue, x: f64, y: f64| {
            let Some(item_id) = item_id.as_string() else {
                return false;
            };
            TEST_GRAPH.with(|graph| {
                let Some(graph) = graph.borrow().clone() else {
                    return false;
                };
                APPLICATION.with(|application| {
                    let application = application.borrow();
                    let Some(application) = application.as_ref() else {
                        return false;
                    };
                    application.update(|cx| {
                        gpui_node_graph::EditorHandle::new(&graph).drop_node(
                            &gpui_node_graph::NodeDrop::new(item_id),
                            Point::new(x as f32, y as f32),
                            cx,
                        )
                    })
                })
            })
        });
    js_sys::Reflect::set(
        &js_sys::global(),
        &JsValue::from_str("__nodeGraphTestDrop"),
        drop_node.as_ref().unchecked_ref(),
    )
    .expect("globalThis accepts the drop bridge");
    drop_node.forget();
}

#[cfg(target_family = "wasm")]
fn main() {
    gpui_platform::web_init();
    let application = gpui_platform::application().run_embedded(launch);
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(application));
    install_browser_test_bridge();
}

#[cfg(test)]
mod parity_tests {
    use super::*;

    #[test]
    fn demo_select_controls_choose_options_instead_of_click_cycling() {
        let mut controls = DemoControls::default();
        assert_eq!(controls.blend_label("mix"), "Normal");
        controls.activate("mix", "mix:blend-select");
        assert_eq!(controls.blend_label("mix"), "Normal");
        controls.activate("mix", "mix:blend-option-3");
        assert_eq!(controls.blend_label("mix"), "Overlay");
        controls.activate("custom", "custom:inputs-count");
        assert_eq!(
            controls.key_down("custom", "custom:inputs-count", "down", None, false, false),
            Some((PortDirection::Input, 3))
        );
    }

    #[test]
    fn port_number_editors_keep_value_and_cursor_state_scoped_to_the_exact_control() {
        let mut controls = DemoControls::default();
        let first = "custom:custom_in_0:factor-value";
        let second = "custom:custom_in_1:factor-value";

        controls.activate("custom", first);
        controls.key_down("custom", first, "a", None, true, false);
        controls.key_down("custom", first, "1", Some("1"), false, false);
        assert_eq!(controls.factor_text_for("custom", first), "1");
        assert_eq!(controls.factor_text_for("custom", second), "0.0");

        controls.activate("custom", second);
        assert!(controls.factor_input_state("custom", first).is_none());
        assert!(controls.factor_input_state("custom", second).is_some());
        controls.key_down("custom", second, "a", None, true, false);
        controls.key_down("custom", second, "2", Some("2"), false, false);
        assert_eq!(controls.factor_text_for("custom", first), "1");
        assert_eq!(controls.factor_text_for("custom", second), "2");
    }

    #[test]
    fn factor_editor_supports_select_all_and_native_enter_escape_preservation() {
        let mut controls = DemoControls::default();
        controls.activate("mix", "mix:factor-value");
        controls.key_down("mix", "mix:factor-value", "a", None, true, false);
        controls.key_down("mix", "mix:factor-value", "7", Some("7"), false, false);
        controls.key_down("mix", "mix:factor-value", ".", Some("."), false, false);
        controls.key_down("mix", "mix:factor-value", "5", Some("5"), false, false);
        assert_eq!(controls.factor_text("mix"), "7.5");
        controls.key_down("mix", "mix:factor-value", "enter", None, false, false);
        controls.activate("mix", "mix:factor-value");
        controls.key_down("mix", "mix:factor-value", "backspace", None, false, false);
        assert_eq!(controls.factor_text("mix"), "7.");
        controls.key_down("mix", "mix:factor-value", "escape", None, false, false);
        assert_eq!(controls.factor_text("mix"), "7.");
    }

    #[test]
    fn factor_editor_supports_caret_delete_and_unrestricted_text_input() {
        let mut controls = DemoControls::default();
        controls.activate("mix", "mix:factor-value");
        controls.key_down("mix", "mix:factor-value", "a", None, true, false);
        controls.key_down("mix", "mix:factor-value", "x", Some("x"), false, false);
        controls.key_down("mix", "mix:factor-value", "y", Some("y"), false, false);
        controls.key_down("mix", "mix:factor-value", "left", None, false, false);
        controls.key_down("mix", "mix:factor-value", "z", Some("z"), false, false);
        assert_eq!(controls.factor_text("mix"), "xzy");
        controls.key_down("mix", "mix:factor-value", "delete", None, false, false);
        assert_eq!(controls.factor_text("mix"), "xz");
    }

    #[test]
    fn factor_editor_supports_pointer_caret_shift_selection_and_clipboard_ranges() {
        let mut controls = DemoControls::default();
        controls
            .factors
            .insert("mix:factor-value".into(), "abcd".into());
        controls.activate("mix", "mix:factor-value");
        let node_x = 330.0;
        let text_start = node_x + 119.0 - 4.0 * 5.3;
        controls.pointer_activate(
            "mix",
            "mix:factor-value",
            Point::new(text_start + 2.0 * 5.3, 0.0),
            node_x,
            1,
        );
        controls.key_down("mix", "mix:factor-value", "right", None, false, true);
        assert_eq!(
            controls.selected_factor_text("mix", "mix:factor-value"),
            Some("c".into())
        );
        controls.key_down("mix", "mix:factor-value", "x", Some("x"), false, false);
        assert_eq!(controls.factor_text("mix"), "abxd");
        controls.pointer_activate("mix", "mix:factor-value", Point::default(), node_x, 2);
        assert_eq!(
            controls.selected_factor_text("mix", "mix:factor-value"),
            Some("abxd".into())
        );
    }

    #[test]
    fn catalog_matches_leptos_demo_schema() {
        let items = catalog();
        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["color_source", "mix", "math", "output", "custom"]
        );
        assert_eq!(
            items
                .iter()
                .map(|item| (
                    item.category.as_str(),
                    item.category_color
                        .expect("demo categories have accents")
                        .rgb,
                ))
                .collect::<Vec<_>>(),
            [
                ("Input", 0x22d3ee),
                ("Color", 0xf59e0b),
                ("Math", 0x8b5cf6),
                ("Output", 0xef4444),
                ("Utility", 0x10b981),
            ]
        );
        assert!(items.iter().all(|item| {
            item.category_color
                .is_some_and(|color| (color.alpha - 1.0).abs() < f32::EPSILON)
        }));
        assert_eq!(
            items
                .iter()
                .map(|item| {
                    item.ports
                        .iter()
                        .map(|port| (port.id.as_str(), port.direction, port.kind.clone()))
                        .collect::<Vec<_>>()
                })
                .collect::<Vec<_>>(),
            vec![
                vec![
                    ("color", PortDirection::Output, Kind::Color),
                    ("alpha", PortDirection::Output, Kind::Float),
                ],
                vec![
                    ("a", PortDirection::Input, Kind::Color),
                    ("b", PortDirection::Input, Kind::Color),
                    ("factor", PortDirection::Input, Kind::Float),
                    ("result", PortDirection::Output, Kind::Color),
                ],
                vec![
                    ("a", PortDirection::Input, Kind::Float),
                    ("b", PortDirection::Input, Kind::Float),
                    ("result", PortDirection::Output, Kind::Float),
                ],
                vec![
                    ("color", PortDirection::Input, Kind::Color),
                    ("value", PortDirection::Input, Kind::Any),
                ],
                vec![],
            ]
        );
        assert_eq!(
            items[0]
                .ports
                .iter()
                .map(|port| port.label.as_str())
                .collect::<Vec<_>>(),
            ["Color", "Alpha"]
        );
        assert_eq!(
            items[1]
                .ports
                .iter()
                .map(|port| port.label.as_str())
                .collect::<Vec<_>>(),
            ["A", "B", "Factor", "Result"]
        );
        assert!(items[4].ports.is_empty());
        assert!(Kind::compatible(&Kind::Color, &Kind::Any));
        assert!(!Kind::compatible(&Kind::Color, &Kind::Float));
    }

    #[test]
    fn stable_node_type_is_independent_from_display_title_with_legacy_fallback() {
        let mut graph = leptos_demo_graph();
        let mut node = graph.nodes.remove("mix_1").unwrap();
        node.title = "Renamed Blend".into();
        assert_eq!(effective_node_type(&node), "mix");

        node.node_type.clear();
        node.title = "Mix".into();
        assert_eq!(effective_node_type(&node), "mix");
        node.title = "Localized Mix".into();
        assert_eq!(effective_node_type(&node), "");

        node.node_type = "consumer.custom.type".into();
        node.title = "Mix".into();
        assert_eq!(effective_node_type(&node), "consumer.custom.type");
    }

    #[test]
    fn initial_graph_keeps_reference_fixture_and_adds_stress_seed() {
        let graph = leptos_demo_graph();
        assert_eq!(graph.nodes.len(), 150);
        assert_eq!(graph.ports.len(), 450);
        assert_eq!(graph.connections.len(), 200);
        graph.validate().unwrap();
        let color = &graph.nodes["color_source_0"];
        assert_eq!(color.node_type, "color_source");
        assert_eq!(color.position, Point::new(50.0, 50.0));
        assert_eq!(
            color.size,
            Size {
                width: 160.0,
                height: 79.0
            }
        );
        let mix = &graph.nodes["mix_1"];
        assert_eq!(mix.node_type, "mix");
        assert_eq!(mix.position, Point::new(330.0, 50.0));
        assert_eq!(
            mix.size,
            Size {
                width: 202.0,
                height: 124.0
            }
        );
        assert_eq!(graph.connections["conn_1"].source, "color_source_0_color");
        assert_eq!(graph.connections["conn_1"].target, "mix_1_b");
    }
    #[test]
    fn world_node_layout_is_immutable_across_all_reference_zoom_levels() {
        let graph = leptos_demo_graph();
        let node = graph.nodes["mix_1"].clone();
        let ports: std::sync::Arc<[_]> = graph
            .ports
            .values()
            .filter(|port| port.node == node.id)
            .cloned()
            .collect::<Vec<_>>()
            .into();
        let zooms = [0.1, 0.740818, 1.0, 1.349859, 2.0, 5.0];
        let scene = leptos_world_node(WorldNodeBodyContext {
            node: node.clone(),
            ports: ports.clone(),
            port_states: std::sync::Arc::new(std::collections::HashMap::new()),
            port_presentations: std::sync::Arc::new(std::collections::HashMap::new()),
            state: gpui_node_graph::WorldNodeVisualState {
                selected: false,
                visible: true,
            },
            theme: std::sync::Arc::new(gpui_node_graph::NodeGraphTheme::leptos_demo()),
        });
        let control = scene
            .hit_regions
            .iter()
            .find(|hit| hit.id.ends_with(":mix-amount"))
            .unwrap();
        for zoom in zooms {
            let transform = gpui_node_graph::world::Transform::new(Point::new(17.25, -9.5), zoom);
            let center_world = Point::new(509.5, 91.5);
            let screen = transform.point(center_world);
            assert_eq!(
                scene.hit_test_screen(screen, transform).map(|hit| &hit.id),
                Some(&control.id),
                "inverse hit testing failed at zoom {zoom}",
            );
        }
    }
}
