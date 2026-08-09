use gpui::prelude::*;
use gpui::{App, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_node_graph::{
    CatalogPort, GraphGroup, NodeBody, NodeBodyContext, NodeCatalogItem, NodeGraph, NodeOverlay,
    PortPresentation, core::*,
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
                let control_values = std::rc::Rc::new(std::cell::RefCell::new(
                    std::collections::HashMap::<String, f32>::new(),
                ));
                let renderer_values = control_values.clone();
                NodeGraph::new_in(graph, cx)
                    .with_catalog(editor_catalog)
                    .with_node_body_renderer(
                        move |context: NodeBodyContext<Kind, String, String, String>,
                         _: &mut gpui::Window,
                         _: &mut App| {
                            let port_count = context.ports.len();
                            let mut input_rows = Vec::new();
                            let mut output_rows = Vec::new();
                            for port in context.ports.iter() {
                                let anchor = context.default_port_anchor(port.id.clone());
                                let row = if port.direction == PortDirection::Input {
                                    gpui::div()
                                        .flex()
                                        .items_center()
                                        .gap_1()
                                        .child(anchor)
                                        .child(port.label.clone())
                                } else {
                                    gpui::div()
                                        .flex()
                                        .items_center()
                                        .justify_end()
                                        .gap_1()
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
                            let show_overlay =
                                context.node.title == "Multiply" || context.node.title == "Mix";
                            let node_title = context.node.title.clone();
                            let numeric_control = matches!(
                                node_title.as_str(),
                                "Number" | "Math" | "Multiply" | "Mix"
                            );
                            let color_control = node_title == "Color Source";
                            let overlay_x = context.node.size.width + 12.0;
                            let node_id = context.node.id.clone();
                            let add_graph = context.graph();
                            let remove_graph = context.graph();
                            let mut body = gpui::div()
                                .w_full()
                                .flex()
                                .flex_col()
                                .gap_2()
                                .child(node_title)
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
                                )
                                .child(
                                    gpui::div()
                                        .flex()
                                        .justify_between()
                                        .child(gpui::div().flex().flex_col().children(input_rows))
                                        .child(gpui::div().flex().flex_col().children(output_rows)),
                                );
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
                                    .h(gpui::px(18.0))
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
                                                                    kind: Kind::Number,
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
                            let body = NodeBody::new(body).with_ports(PortPresentation::BodyAnchors);
                            if show_overlay {
                                body.with_overlay(NodeOverlay::new(
                                    Point::new(overlay_x, 20.0),
                                    context.isolated_control(
                                        gpui::div()
                                            .w(gpui::px(112.0))
                                        .rounded_md()
                                        .border_1()
                                        .border_color(gpui::rgb(0x52525b))
                                        .bg(gpui::rgb(0x202023))
                                        .p_2()
                                        .text_size(gpui::px(11.0))
                                        .child("Blend controls")
                                        .child("Mode · Multiply")
                                        .on_mouse_down(
                                            gpui::MouseButton::Left,
                                            |_, window, cx| {
                                                cx.stop_propagation();
                                                window.prevent_default();
                                            },
                                        ),
                                    ),
                                )
                                .adaptive("blend-controls", Size { width: 112.0, height: 54.0 }))
                            } else {
                                body
                            }
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
                    r#"{{"nodes":{},"catalogOpen":{},"overlayDismissed":{},"zoom":{}}}"#,
                    graph.graph.nodes.len(),
                    graph.catalog_is_open(),
                    graph.is_overlay_dismissed("blend-controls"),
                    graph.graph.viewport.zoom,
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
