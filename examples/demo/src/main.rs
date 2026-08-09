use gpui::prelude::*;
use gpui::{App, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_node_graph::{
    CatalogPort, GraphGroup, NodeBody, NodeBodyContext, NodeCatalogItem, NodeGraph, NodeOverlay,
    PortPresentation, core::*,
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
            description: "Dynamic port configuration".into(),
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

fn leptos_demo_graph() -> GraphState<String, String, String, Kind> {
    let mut graph: GraphState<String, String, String, Kind> = Default::default();
    for (id, title, position, size) in [
        (
            "color_source_0",
            "Color Source",
            Point::new(50.0, 50.0),
            Size {
                width: 160.0,
                height: 79.0,
            },
        ),
        (
            "mix_1",
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
    graph
}

fn launch(cx: &mut App) {
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
            let graph = cx.new(|cx| {
                let graph = leptos_demo_graph();
                let control_values = std::rc::Rc::new(std::cell::RefCell::new(
                    std::collections::HashMap::<String, f32>::new(),
                ));
                let renderer_values = control_values.clone();
                let open_overlays = std::rc::Rc::new(std::cell::RefCell::new(
                    std::collections::HashSet::<String>::new(),
                ));
                let renderer_overlays = open_overlays.clone();
                NodeGraph::new_in(graph, cx)
                    .with_style(gpui_node_graph::style::leptos_demo())
                    .with_catalog(editor_catalog)
                    .with_node_body_renderer(
                        move |context: NodeBodyContext<Kind, String, String, String>,
                         _: &mut gpui::Window,
                         _: &mut App| {
                            let mut input_rows = Vec::new();
                            let mut output_rows = Vec::new();
                            let mut ordered_ports = context.ports.iter().collect::<Vec<_>>();
                            ordered_ports.sort_by_key(|port| match port.label.as_str() {
                                "Color" | "A" | "Result" => 0,
                                "Alpha" | "B" => 1,
                                "Factor" => 2,
                                _ => 3,
                            });
                            for port in ordered_ports {
                                let anchor = context.default_port_anchor(port.id.clone());
                                let row = if port.direction == PortDirection::Input {
                                    let mut row = gpui::div()
                                        .h(gpui::px(20.0 * context.state.zoom))
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .text_size(gpui::px(11.0 * context.state.zoom))
                                        .child(anchor)
                                        .child(port.label.clone());
                                    if port.kind == Kind::Float {
                                        row = row.child(
                                            gpui::div()
                                                .ml_auto()
                                                .w(gpui::px(52.0 * context.state.zoom))
                                                .rounded(gpui::px(4.0 * context.state.zoom))
                                                .border(gpui::px(context.state.zoom))
                                                .border_color(gpui::rgb(0x3f3f46))
                                                .bg(gpui::rgb(0x27272a))
                                                .text_size(gpui::px(11.0 * context.state.zoom))
                                                .text_center()
                                                .child("0.0"),
                                        );
                                    }
                                    row
                                } else {
                                    gpui::div()
                                        .h(gpui::px(20.0 * context.state.zoom))
                                        .flex()
                                        .items_center()
                                        .justify_end()
                                        .gap_1()
                                        .text_size(gpui::px(11.0 * context.state.zoom))
                                        .child(port.label.clone())
                                        .child(anchor)
                                };
                                if port.direction == PortDirection::Input {
                                    input_rows.push(row);
                                } else {
                                    output_rows.push(row);
                                }
                            }
                            let is_custom = context.node.title == "Custom";
                            let overlay_eligible =
                                context.node.title == "Multiply" || context.node.title == "Mix";
                            let show_overlay = overlay_eligible
                                && renderer_overlays.borrow().contains(&context.node.id);
                            let node_title = context.node.title.clone();
                            let node_zoom = context.state.zoom;
                            let numeric_control = matches!(
                                node_title.as_str(),
                                "Math"
                            );
                            let color_control = false;
                            let overlay_x = context.node.size.width + 12.0;
                            let node_id = context.node.id.clone();
                            let add_graph = context.graph();
                            let remove_graph = context.graph();
                            let (category, accent) = match node_title.as_str() {
                                "Color Source" => ("INPUT", 0x22d3ee),
                                "Mix" => ("COLOR", 0xf59e0b),
                                "Math" => ("MATH", 0x8b5cf6),
                                "Output" => ("OUTPUT", 0xef4444),
                                _ => ("UTILITY", 0x10b981),
                            };
                            let header = gpui::div()
                                .h(gpui::px(29.0 * node_zoom))
                                .flex()
                                .items_center()
                                .justify_between()
                                .px_2()
                                .text_size(gpui::px(12.0 * node_zoom))
                                .text_color(gpui::rgb(0xa1a1aa))
                                .child(node_title.to_uppercase())
                                .child(
                                    gpui::div()
                                        .text_size(gpui::px(10.0 * node_zoom))
                                        .text_color(gpui::rgb(accent))
                                        .child(category),
                                );
                            let ports_element = gpui::div()
                                .w_full()
                                .flex()
                                .justify_between()
                                .px_2()
                                .py_1()
                                .child(gpui::div().flex().flex_col().children(input_rows))
                                .child(gpui::div().flex().flex_col().children(output_rows));
                            let mut body = gpui::div()
                                .w_full()
                                .h_full()
                                .flex()
                                .flex_col()
                                .child(
                                    gpui::div()
                                        .w_full()
                                        .h(gpui::px(2.0 * node_zoom))
                                        .bg(gpui::rgb(accent)),
                                )
                                .child(header);
                            if numeric_control {
                                let value = *renderer_values
                                    .borrow()
                                    .get(&node_id)
                                    .unwrap_or(&0.5);
                                let values = renderer_values.clone();
                                let graph = context.graph();
                                let value_node = node_id.clone();
                                let control = gpui::div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .rounded_sm()
                                    .bg(gpui::rgb(0x27272a))
                                    .px_2()
                                    .child(format!("Value {value:.2}"))
                                    .child(
                                        gpui::div()
                                            .rounded_sm()
                                            .bg(gpui::rgb(0x3f3f46))
                                            .px_2()
                                            .child("+")
                                            .on_mouse_down(
                                                gpui::MouseButton::Left,
                                                move |_, window, cx| {
                                                    cx.stop_propagation();
                                                    window.prevent_default();
                                                    let next = (values
                                                        .borrow()
                                                        .get(&value_node)
                                                        .copied()
                                                        .unwrap_or(0.5)
                                                        + 0.1)
                                                        .min(1.0);
                                                    values
                                                        .borrow_mut()
                                                        .insert(value_node.clone(), next);
                                                    let _ = graph.update(cx, |_, cx| cx.notify());
                                                },
                                            ),
                                    );
                                body = body.child(context.isolated_control(control));
                            }
                            if overlay_eligible {
                                let overlays = renderer_overlays.clone();
                                let graph = context.graph();
                                let overlay_node = node_id.clone();
                                let trigger = gpui::div()
                                    .h(gpui::px(25.0 * node_zoom))
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .px_2()
                                    .text_size(gpui::px(11.0 * node_zoom))
                                    .child("Blend")
                                    .child(
                                        gpui::div()
                                            .flex()
                                            .items_center()
                                            .gap_1()
                                            .child(
                                                gpui::div()
                                                    .w(gpui::px(106.0 * node_zoom))
                                                    .rounded(gpui::px(4.0 * node_zoom))
                                                    .border(gpui::px(node_zoom))
                                                    .border_color(gpui::rgb(0x3f3f46))
                                                    .bg(gpui::rgb(0x27272a))
                                                    .px_2()
                                                    .child("Normal"),
                                            )
                                            .child(
                                                gpui::div()
                                                    .rounded(gpui::px(4.0 * node_zoom))
                                                    .border(gpui::px(node_zoom))
                                                    .border_color(gpui::rgb(0x3f3f46))
                                                    .bg(gpui::rgb(0x27272a))
                                                    .px_2()
                                                    .child("✎")
                                                    .on_mouse_down(
                                                        gpui::MouseButton::Left,
                                                        move |_, window, cx| {
                                                            cx.stop_propagation();
                                                            window.prevent_default();
                                                            let mut overlays = overlays.borrow_mut();
                                                            if !overlays.remove(&overlay_node) {
                                                                overlays.insert(overlay_node.clone());
                                                            }
                                                            drop(overlays);
                                                            let _ = graph.update(cx, |_, cx| cx.notify());
                                                        },
                                                    ),
                                            ),
                                    );
                                body = body.child(context.isolated_control(trigger));
                            }
                            if color_control {
                                let values = renderer_values.clone();
                                let graph = context.graph();
                                let value_node = node_id.clone();
                                let active = renderer_values
                                    .borrow()
                                    .get(&node_id)
                                    .copied()
                                    .unwrap_or_default()
                                    > 0.5;
                                let swatch = gpui::div()
                                    .h(gpui::px(18.0 * node_zoom))
                                    .rounded_sm()
                                    .bg(gpui::rgb(if active { 0xf472b6 } else { 0x38bdf8 }))
                                    .on_mouse_down(
                                        gpui::MouseButton::Left,
                                        move |_, window, cx| {
                                            cx.stop_propagation();
                                            window.prevent_default();
                                            let next = if values
                                                .borrow()
                                                .get(&value_node)
                                                .copied()
                                                .unwrap_or_default()
                                                > 0.5
                                            {
                                                0.0
                                            } else {
                                                1.0
                                            };
                                            values.borrow_mut().insert(value_node.clone(), next);
                                            let _ = graph.update(cx, |_, cx| cx.notify());
                                        },
                                    );
                                body = body.child(context.isolated_control(swatch));
                            }
                            if is_custom {
                                let add_node = node_id.clone();
                                let remove_node = node_id;
                                body = body.child(
                                    gpui::div()
                                        .flex()
                                        .gap_1()
                                        .child(
                                            gpui::div()
                                                .rounded_sm()
                                                .bg(gpui::rgb(0x334155))
                                                .px_2()
                                                .child("+ input")
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    move |_, window, cx| {
                                                        cx.stop_propagation();
                                                        window.prevent_default();
                                                        let _ = add_graph.update(cx, |editor, cx| {
                                                            let Some(node) = editor
                                                                .graph
                                                                .nodes
                                                                .get(&add_node)
                                                                .cloned()
                                                            else {
                                                                return;
                                                            };
                                                            let mut sequence = 1;
                                                            let port_id = loop {
                                                                let id = format!(
                                                                    "{}.dynamic-{sequence}",
                                                                    add_node
                                                                );
                                                                if !editor
                                                                    .graph
                                                                    .ports
                                                                    .contains_key(&id)
                                                                {
                                                                    break id;
                                                                }
                                                                sequence += 1;
                                                            };
                                                            let row = editor
                                                                .graph
                                                                .ports
                                                                .values()
                                                                .filter(|port| {
                                                                    port.node == add_node
                                                                        && port.direction
                                                                            == PortDirection::Input
                                                                })
                                                                .count();
                                                            editor.graph.ports.insert(
                                                                port_id.clone(),
                                                                Port {
                                                                    id: port_id.clone(),
                                                                    node: add_node.clone(),
                                                                    label: format!(
                                                                        "Dynamic {}",
                                                                        row + 1
                                                                    ),
                                                                    direction: PortDirection::Input,
                                                                    kind: Kind::Float,
                                                                    position: Point::new(
                                                                        node.position.x,
                                                                        node.position.y
                                                                            + 52.0
                                                                            + row as f32 * 22.0,
                                                                    ),
                                                                },
                                                            );
                                                            editor.restore_tombstoned_connections(&port_id, cx);
                                                            if let Some(node) = editor
                                                                .graph
                                                                .nodes
                                                                .get_mut(&add_node)
                                                            {
                                                                node.size.height += 22.0;
                                                            }
                                                            cx.notify();
                                                        });
                                                    },
                                                ),
                                        )
                                        .child(
                                            gpui::div()
                                                .rounded_sm()
                                                .bg(gpui::rgb(0x3f2a2a))
                                                .px_2()
                                                .child("− input")
                                                .on_mouse_down(
                                                    gpui::MouseButton::Left,
                                                    move |_, window, cx| {
                                                        cx.stop_propagation();
                                                        window.prevent_default();
                                                        let _ = remove_graph.update(
                                                            cx,
                                                            |editor, cx| {
                                                                let mut candidates: Vec<_> = editor
                                                                    .graph
                                                                    .ports
                                                                    .iter()
                                                                    .filter(|(id, port)| {
                                                                        port.node == remove_node
                                                                            && id.contains(
                                                                                ".dynamic-",
                                                                            )
                                                                    })
                                                                    .map(|(id, _)| id.clone())
                                                                    .collect();
                                                                candidates.sort();
                                                                if let Some(id) = candidates.pop() {
                                                                    editor.remove_port_with_tombstones(&id, cx);
                                                                    if let Some(node) = editor
                                                                        .graph
                                                                        .nodes
                                                                        .get_mut(&remove_node)
                                                                    {
                                                                        node.size.height =
                                                                            (node.size.height - 22.0)
                                                                                .max(110.0);
                                                                    }
                                                                    cx.notify();
                                                                }
                                                            },
                                                        );
                                                    },
                                                ),
                                        ),
                                );
                            }
                            body = body.child(ports_element);
                            let body = NodeBody::new(body).with_ports(PortPresentation::BodyAnchors);
                            if show_overlay {
                                body.with_overlay(NodeOverlay::new(
                                    Point::new(overlay_x, 20.0),
                                    context.isolated_control(
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
                                                .h(gpui::px(4.0))
                                                .rounded_full()
                                                .bg(gpui::rgb(0x71717a)),
                                        )
                                        .child("0.50")
                                        .on_mouse_down(
                                            gpui::MouseButton::Left,
                                            |_, window, cx| {
                                                cx.stop_propagation();
                                                window.prevent_default();
                                            },
                                        ),
                                    ),
                                )
                                .adaptive("blend-controls", Size { width: 200.0, height: 86.0 }))
                            } else {
                                body
                            }
                        },
                    )
                    .with_groups(vec![GraphGroup {
                        id: "group_0".into(),
                        label: "Group 1".into(),
                        color: 0x8b5cf6,
                        nodes: ["color_source_0".to_string(), "mix_1".to_string()]
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
                format!(
                    r#"{{"nodes":{},"catalogOpen":{},"overlayDismissed":{},"zoom":{},"sourceWidth":{},"sourceHeight":{}}}"#,
                    graph.graph.nodes.len(),
                    graph.catalog_is_open(),
                    graph.is_overlay_dismissed("blend-controls"),
                    graph.graph.viewport.zoom,
                    graph
                        .resolved_node_size(&String::from("color_source_0"))
                        .map_or(0.0, |size| size.width),
                    graph
                        .resolved_node_size(&String::from("color_source_0"))
                        .map_or(0.0, |size| size.height),
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
    fn initial_graph_matches_leptos_seed_two_nodes_one_connection() {
        let graph = leptos_demo_graph();
        assert_eq!(graph.nodes.len(), 2);
        assert_eq!(graph.ports.len(), 6);
        assert_eq!(graph.connections.len(), 1);
        let color = &graph.nodes["color_source_0"];
        assert_eq!(color.position, Point::new(50.0, 50.0));
        assert_eq!(
            color.size,
            Size {
                width: 160.0,
                height: 79.0
            }
        );
        let mix = &graph.nodes["mix_1"];
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
}
