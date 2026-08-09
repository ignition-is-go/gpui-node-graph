use gpui::prelude::*;
use gpui::{App, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_node_graph::{
    CatalogPort, GraphGroup, NodeBody, NodeBodyContext, NodeCatalogItem, NodeGraph, core::*,
};

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Number,
    Color,
}
impl PortType for Kind {
    fn compatible(source: &Self, target: &Self) -> bool {
        source == target
    }
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
            id: "color-source".into(),
            label: "Color Source".into(),
            category: "Input".into(),
            description: "Produces a color".into(),
            keywords: vec!["rgb".into(), "value".into()],
            ports: vec![port("color", "Color", PortDirection::Output, Kind::Color)],
        },
        NodeCatalogItem {
            id: "mix".into(),
            label: "Mix".into(),
            category: "Color".into(),
            description: "Blends two colors".into(),
            keywords: vec!["blend".into(), "lerp".into()],
            ports: vec![
                port("a", "A", PortDirection::Input, Kind::Color),
                port("b", "B", PortDirection::Input, Kind::Color),
                port("result", "Result", PortDirection::Output, Kind::Color),
            ],
        },
        NodeCatalogItem {
            id: "math".into(),
            label: "Math".into(),
            category: "Number".into(),
            description: "Combines numeric values".into(),
            keywords: vec!["add".into(), "multiply".into()],
            ports: vec![
                port("a", "A", PortDirection::Input, Kind::Number),
                port("b", "B", PortDirection::Input, Kind::Number),
                port("result", "Result", PortDirection::Output, Kind::Number),
            ],
        },
        NodeCatalogItem {
            id: "output".into(),
            label: "Output".into(),
            category: "Sink".into(),
            description: "Accepts a graph result".into(),
            keywords: vec!["final".into(), "sink".into()],
            ports: vec![
                port("number", "Number", PortDirection::Input, Kind::Number),
                port("color", "Color", PortDirection::Input, Kind::Color),
            ],
        },
        NodeCatalogItem {
            id: "custom".into(),
            label: "Custom".into(),
            category: "Utility".into(),
            description: "Extensible typed node".into(),
            keywords: vec!["dynamic".into(), "ports".into()],
            ports: vec![
                port("in", "In", PortDirection::Input, Kind::Number),
                port("out", "Out", PortDirection::Output, Kind::Number),
            ],
        },
    ]
}

fn insert_connection(
    graph: &mut GraphState<String, String, String, Kind>,
    source: String,
    target: String,
) {
    let mut sequence = graph.connections.len() + 1;
    let id = loop {
        let id = format!("connection-{sequence}");
        if !graph.connections.contains_key(&id) {
            break id;
        }
        sequence += 1;
    };
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
    let mut sequence = editor.graph.nodes.len() + 1;
    let node_id = loop {
        let id = format!("{}-{sequence}", item.id);
        if !editor.graph.nodes.contains_key(&id) {
            break id;
        }
        sequence += 1;
    };
    let node_size = Size {
        width: 180.0,
        height: 72.0 + item.ports.len() as f32 * 22.0,
    };
    editor.graph.nodes.insert(
        node_id.clone(),
        Node {
            id: node_id.clone(),
            title: item.label.clone(),
            position,
            size: node_size,
        },
    );
    let mut input_row = 0;
    let mut output_row = 0;
    let mut auto_port = None;
    for template in &item.ports {
        let row = if template.direction == PortDirection::Input {
            let row = input_row;
            input_row += 1;
            row
        } else {
            let row = output_row;
            output_row += 1;
            row
        };
        let port_id = format!("{}.{}", node_id, template.id);
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
                            0.0
                        } else {
                            node_size.width
                        },
                    position.y + 52.0 + row as f32 * 22.0,
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
        let (source, target) = if direction == PortDirection::Input {
            (existing, created)
        } else {
            (created, existing)
        };
        insert_connection(&mut editor.graph, source, target);
    }
}

fn launch(cx: &mut App) {
    let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..Default::default()
        },
        |_, cx| {
            let catalog = catalog();
            let editor_catalog = catalog.clone();
            let graph = cx.new(|cx| {
                let mut graph: GraphState<String, String, String, Kind> = Default::default();
                for (id, title, x) in [
                    ("source", "Number", 80.0),
                    ("math", "Multiply", 380.0),
                    ("output", "Output", 700.0),
                ] {
                    graph.nodes.insert(
                        id.into(),
                        Node {
                            id: id.into(),
                            title: title.into(),
                            position: Point::new(x, 180.0),
                            size: Size {
                                width: 180.0,
                                height: 110.0,
                            },
                        },
                    );
                }
                for (id, node, label, direction, x) in [
                    (
                        "source.out",
                        "source",
                        "Value",
                        PortDirection::Output,
                        260.0,
                    ),
                    ("math.a", "math", "A", PortDirection::Input, 380.0),
                    ("math.out", "math", "Result", PortDirection::Output, 560.0),
                    ("output.in", "output", "Value", PortDirection::Input, 700.0),
                ] {
                    graph.ports.insert(
                        id.into(),
                        Port {
                            id: id.into(),
                            node: node.into(),
                            label: label.into(),
                            direction,
                            kind: Kind::Number,
                            position: Point::new(x, 235.0),
                        },
                    );
                }
                insert_connection(&mut graph, "source.out".into(), "math.a".into());
                insert_connection(&mut graph, "math.out".into(), "output.in".into());
                NodeGraph::new_in(graph, cx)
                    .with_catalog(editor_catalog)
                    .with_node_body_renderer(
                        |context: NodeBodyContext<Kind, String, String, String>,
                         _: &mut gpui::Window,
                         _: &mut App| {
                            let port_count = context.ports.len();
                            NodeBody::new(
                                gpui::div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(context.node.title)
                                    .child(
                                        gpui::div()
                                            .rounded_sm()
                                            .bg(gpui::rgb(0x18181b))
                                            .px_2()
                                            .py_1()
                                            .text_size(gpui::px(10.0))
                                            .child(format!("{port_count} typed ports"))
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                |_, window, cx| {
                                                    cx.stop_propagation();
                                                    window.prevent_default();
                                                },
                                            ),
                                    ),
                            )
                        },
                    )
                    .with_groups(vec![GraphGroup {
                        id: "initial".into(),
                        label: "Processing".into(),
                        color: 0x60a5fa,
                        nodes: ["source".to_string(), "math".to_string()]
                            .into_iter()
                            .collect(),
                    }])
            });
            graph.update(cx, |_, cx| {
                cx.subscribe(&graph, move |editor, _, event, cx| {
                    match event {
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
                                    label: format!("Group {sequence}"),
                                    color: 0xa78bfa,
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
}

#[cfg(target_family = "wasm")]
fn main() {
    gpui_platform::web_init();
    let application = gpui_platform::application().run_embedded(launch);
    APPLICATION.with(|slot| *slot.borrow_mut() = Some(application));
}
