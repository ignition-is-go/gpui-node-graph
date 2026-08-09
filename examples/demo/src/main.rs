use gpui::prelude::*;
use gpui::{App, Bounds, WindowBounds, WindowOptions, px, size};
use gpui_node_graph::{NodeGraph, core::*};
#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Number,
}
impl PortType for Kind {
    fn compatible(a: &Self, b: &Self) -> bool {
        a == b
    }
}
fn main() {
    #[cfg(target_family = "wasm")]
    gpui_platform::web_init();
    gpui_platform::application().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                ..Default::default()
            },
            |_, cx| {
                cx.new(|_| {
                    let mut g: GraphState<String, String, String, Kind> = Default::default();
                    for (id, title, x) in [
                        ("source", "Number", 80.),
                        ("math", "Multiply", 380.),
                        ("output", "Output", 700.),
                    ] {
                        g.nodes.insert(
                            id.into(),
                            Node {
                                id: id.into(),
                                title: title.into(),
                                position: Point::new(x, 180.),
                                size: Size {
                                    width: 180.,
                                    height: 110.,
                                },
                            },
                        );
                    }
                    for (id, node, x) in [
                        ("source.out", "source", 260.),
                        ("math.a", "math", 380.),
                        ("math.out", "math", 560.),
                        ("output.in", "output", 700.),
                    ] {
                        g.ports.insert(
                            id.into(),
                            Port {
                                id: id.into(),
                                node: node.into(),
                                label: id.into(),
                                direction: if id.ends_with("out") {
                                    PortDirection::Output
                                } else {
                                    PortDirection::Input
                                },
                                kind: Kind::Number,
                                position: Point::new(x, 235.),
                            },
                        );
                    }
                    g.connections.insert(
                        "c1".into(),
                        Connection {
                            id: "c1".into(),
                            source: "source.out".into(),
                            target: "math.a".into(),
                        },
                    );
                    g.connections.insert(
                        "c2".into(),
                        Connection {
                            id: "c2".into(),
                            source: "math.out".into(),
                            target: "output.in".into(),
                        },
                    );
                    NodeGraph::new(g)
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
