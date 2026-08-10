use gpui::prelude::*;
use gpui::{App, WindowOptions};
#[cfg(not(target_arch = "wasm32"))]
use gpui::{Bounds, WindowBounds, px, size};
use gpui_node_graph::{
    CatalogPort, GraphGroup, NodeCatalogItem, NodeGraph, NodeOverlay, WorldNodeBodyContext,
    core::*,
    world::{HitRole, HitShape, TextLines, WorldColor, WorldHitRegion, WorldPrimitive, WorldScene},
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

fn world_text(
    scene: &mut WorldScene,
    x: f32,
    y: f32,
    text: impl Into<String>,
    size: f32,
    color: u32,
) {
    scene.push(WorldPrimitive::Text {
        origin: Point::new(x, y),
        lines: TextLines::new([text.into()]),
        color: WorldColor::rgb(color),
        font_size: size,
        font_weight: if size >= 12.0 { 600 } else { 400 },
        line_height: size + 3.0,
    });
}

fn world_socket(
    scene: &mut WorldScene,
    port: &Port<String, String, Kind>,
    connected: bool,
    style: &gpui_node_graph::style::AnchorStyle,
    node_background: gpui_node_graph::style::Color,
) {
    scene.push(WorldPrimitive::Circle {
        center: port.position,
        radius: style.dot_size * 0.5,
        fill: WorldColor::rgba(
            if connected {
                style.dot_connected_color.rgb
            } else {
                style.dot_color.rgb
            },
            if connected {
                style.dot_connected_color.alpha
            } else {
                style.dot_color.alpha
            },
        ),
    });
    if !connected {
        scene.push(WorldPrimitive::Circle {
            center: port.position,
            radius: (style.dot_size * 0.5 - style.dot_border_width).max(0.0),
            fill: WorldColor::rgba(node_background.rgb, node_background.alpha),
        });
    }
}

fn leptos_world_node(context: WorldNodeBodyContext<Kind, String, String>) -> WorldScene {
    let mut scene = WorldScene::new();
    let node = &context.node;
    let node_style = &context.style.node;
    let anchor_style = &context.style.anchor;
    let (category, accent) = match node.title.as_str() {
        "Color Source" => ("INPUT", 0x22d3ee),
        "Mix" => ("COLOR", 0xf59e0b),
        "Math" => ("MATH", 0x8b5cf6),
        "Output" => ("OUTPUT", 0xef4444),
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

    if node.title == "Custom" {
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
            let y = node.position.y + 33.0 + row as f32 * 28.0;
            world_text(
                &mut scene,
                node.position.x + 10.0,
                y + 4.0,
                label.to_uppercase(),
                10.0,
                0x71717a,
            );
            let select = Rect {
                origin: Point::new(node.position.x + 56.0, y),
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
            scene.push_hit_region(WorldHitRegion::new(
                format!("{}:{control}", node.id),
                HitRole::Control,
                HitShape::Rect(select),
            ));
        }
    }

    if node.title == "Mix" {
        world_text(
            &mut scene,
            node.position.x + 10.0,
            node.position.y + 34.0,
            "Blend",
            11.0,
            0xa1a1aa,
        );
        scene.push(WorldPrimitive::BorderedQuad {
            bounds: Rect {
                origin: Point::new(node.position.x + 54.0, node.position.y + 30.5),
                size: Size {
                    width: 107.0,
                    height: 22.0,
                },
            },
            fill: WorldColor::rgb(0x27272a),
            border: WorldColor::rgb(0x3f3f46),
            border_width: 1.0,
            corner_radius: 4.0,
        });
        world_text(
            &mut scene,
            node.position.x + 64.0,
            node.position.y + 34.0,
            "Normal",
            11.0,
            0xd4d4d8,
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
        scene.push_hit_region(WorldHitRegion::new(
            format!("{}:mix-amount", node.id),
            HitRole::Control,
            HitShape::Rect(Rect {
                origin: Point::new(node.position.x + 167.0, node.position.y + 29.0),
                size: Size {
                    width: 25.0,
                    height: 25.0,
                },
            }),
        ));
    }

    let connected = ["color_source_0_color", "mix_1_b"];
    for port in context.ports.iter() {
        world_socket(
            &mut scene,
            port,
            connected.contains(&port.id.as_str()),
            anchor_style,
            node_style.background,
        );
        let (x, y) = match (node.title.as_str(), port.label.as_str(), port.direction) {
            ("Color Source", "Color", _) => (node.position.x + 107.0, port.position.y - 6.0),
            ("Color Source", "Alpha", _) => (node.position.x + 107.0, port.position.y - 6.0),
            ("Mix", "Result", _) => (node.position.x + 148.0, port.position.y - 6.0),
            (_, _, PortDirection::Input) => (port.position.x + 9.0, port.position.y - 6.0),
            (_, _, PortDirection::Output) => (port.position.x - 50.0, port.position.y - 6.0),
        };
        world_text(
            &mut scene,
            x,
            y,
            port.label.clone(),
            anchor_style.label_font_size,
            anchor_style.label_color.rgb,
        );
        if port.direction == PortDirection::Input && port.kind == Kind::Float {
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
            world_text(
                &mut scene,
                node.position.x + 103.0,
                port.position.y - 6.0,
                "0.0",
                11.0,
                0xd4d4d8,
            );
        }
    }
    scene
}

fn cycle_custom_port_count(
    editor: &mut NodeGraph<Kind>,
    node_id: &str,
    direction: PortDirection,
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
    let next = (current + 1) % 9;
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
            let event_overlays = open_overlays.clone();
            let graph = cx.new(move |cx| {
                let graph = leptos_demo_graph();
                NodeGraph::new_in(graph, cx)
                    .with_style(gpui_node_graph::style::leptos_demo())
                    .with_world_node_body_renderer(leptos_world_node)
                    .with_catalog(editor_catalog)
                    .with_node_overlay_renderer(
                        move |context: WorldNodeBodyContext<Kind, String, String>,
                              _: &mut gpui::Window,
                              _: &mut App| {
                            if context.node.title != "Mix"
                                || !renderer_overlays.borrow().contains(&context.node.id)
                            {
                                return Vec::new();
                            }
                            vec![
                                NodeOverlay::new(
                                    Point::new(context.node.size.width - 10.0, 29.0),
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
                                                        .left(gpui::px(82.0))
                                                        .top(gpui::px(0.0))
                                                        .w(gpui::px(14.0))
                                                        .h(gpui::px(14.0))
                                                        .rounded_full()
                                                        .bg(gpui::rgb(0x93c5fd)),
                                                ),
                                        )
                                        .child("0.50")
                                        .on_mouse_down(gpui::MouseButton::Left, |_, window, cx| {
                                            cx.stop_propagation();
                                            window.prevent_default();
                                        }),
                                )
                                .with_screen_offset(Point::new(8.0, 0.0))
                                .adaptive(
                                    "mix-amount",
                                    Size {
                                        width: 200.0,
                                        height: 86.0,
                                    },
                                ),
                            ]
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
                        GraphEvent::NodeControlActivated {
                            node_id,
                            control_id,
                        } if control_id.ends_with(":inputs-count") => {
                            cycle_custom_port_count(editor, node_id, PortDirection::Input, cx);
                        }
                        GraphEvent::NodeControlActivated {
                            node_id,
                            control_id,
                        } if control_id.ends_with(":outputs-count") => {
                            cycle_custom_port_count(editor, node_id, PortDirection::Output, cx);
                        }
                        GraphEvent::NodeControlActivated {
                            node_id,
                            control_id,
                        } if control_id.ends_with(":mix-amount") => {
                            let mut overlays = event_overlays.borrow_mut();
                            if !overlays.remove(node_id) {
                                overlays.insert(node_id.clone());
                            }
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
                    r#"{{"nodes":{},"catalogOpen":{},"overlayDismissed":{},"zoom":{},"sourceWidth":{},"sourceHeight":{},"controlActivated":{},"worldLayout":"{}"}}"#,
                    graph.graph.nodes.len(),
                    graph.catalog_is_open(),
                    graph.is_overlay_dismissed("mix-amount"),
                    graph.graph.viewport.zoom,
                    graph
                        .resolved_node_size(&String::from("color_source_0"))
                        .map_or(0.0, |size| size.width),
                    graph
                        .resolved_node_size(&String::from("color_source_0"))
                        .map_or(0.0, |size| size.height),
                    graph.last_world_control().is_some(),
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
        let scenes = zooms.map(|zoom| {
            leptos_world_node(WorldNodeBodyContext {
                node: node.clone(),
                ports: ports.clone(),
                state: gpui_node_graph::NodeVisualState {
                    selected: false,
                    visible: true,
                    zoom,
                },
                style: gpui_node_graph::style::leptos_demo(),
            })
        });
        for scene in &scenes[1..] {
            assert_eq!(scene, &scenes[0], "zoom changed the authored display list");
        }
        let control = scenes[0]
            .hit_regions
            .iter()
            .find(|hit| hit.id.ends_with(":mix-amount"))
            .unwrap();
        for zoom in zooms {
            let transform = gpui_node_graph::world::Transform::new(Point::new(17.25, -9.5), zoom);
            let center_world = Point::new(509.5, 91.5);
            let screen = transform.point(center_world);
            assert_eq!(
                scenes[0]
                    .hit_test_screen(screen, transform)
                    .map(|hit| &hit.id),
                Some(&control.id),
                "inverse hit testing failed at zoom {zoom}",
            );
        }
    }
}
