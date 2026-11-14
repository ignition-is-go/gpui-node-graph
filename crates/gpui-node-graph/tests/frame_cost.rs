//! Frame cost on a large synthetic graph. Ignored by default; run with
//! `cargo test --release -p gpui-node-graph --test frame_cost -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use gpui::{Modifiers, VisualTestContext, point, px};
use gpui::{ParentElement, Styled, div};
use gpui_node_graph::{NodeBody, NodeBodyContext, NodeGraph, NodeGraphTheme, core::*, graph_px};

#[derive(Clone, Debug, PartialEq)]
struct Kind;
impl PortType for Kind {
    fn compatible(_: &Self, _: &Self) -> bool {
        true
    }
}

const COLUMNS: usize = 25;
const ROWS: usize = 20;
const PORTS_PER_SIDE: usize = 2;

/// Cards are 160x100. The default pitch leaves gaps; the overlapping pitch
/// stacks every card over its neighbours' sockets, so the card grid and the
/// visible-piece clipping have work to do.
const SPARSE_PITCH: (f32, f32) = (220.0, 140.0);
const OVERLAPPING_PITCH: (f32, f32) = (110.0, 70.0);

fn large_graph(zoom: f32, pitch: (f32, f32)) -> GraphState<String, String, String, Kind> {
    let mut graph = GraphState::default();
    for index in 0..COLUMNS * ROWS {
        let id = format!("n{index}");
        let position = Point::new(
            (index % COLUMNS) as f32 * pitch.0,
            (index / COLUMNS) as f32 * pitch.1,
        );
        graph.nodes.insert(
            id.clone(),
            Node {
                id: id.clone(),
                node_type: "op".into(),
                title: format!("Node {index}"),
                position,
                size: Size {
                    width: 160.0,
                    height: 100.0,
                },
            },
        );
        for (direction, side) in [(PortDirection::Input, "in"), (PortDirection::Output, "out")] {
            for slot in 0..PORTS_PER_SIDE {
                let port = format!("{id}:{side}{slot}");
                graph.ports.insert(
                    port.clone(),
                    Port {
                        id: port,
                        node: id.clone(),
                        label: format!("{side} {slot}"),
                        direction,
                        kind: Kind,
                        position,
                    },
                );
            }
        }
        if index % COLUMNS != 0 {
            let source = format!("n{}:out0", index - 1);
            let connection = format!("c{index}");
            graph.connections.insert(
                connection.clone(),
                Connection {
                    id: connection,
                    source,
                    target: format!("{id}:in0"),
                },
            );
        }
    }
    graph.viewport = Viewport {
        pan: Point::new(0.0, 0.0),
        zoom,
    };
    graph
}

fn draw(cx: &mut VisualTestContext) -> Duration {
    let start = Instant::now();
    cx.update(|window, cx| {
        let arena = window.draw(cx);
        arena.clear(cx)
    });
    start.elapsed()
}

fn report(label: &str, mut samples: Vec<Duration>) {
    samples.sort();
    let median = samples[samples.len() / 2];
    let p90 = samples[samples.len() * 9 / 10];
    println!(
        "{label}: median {:.2} ms, p90 {:.2} ms over {} frames",
        median.as_secs_f64() * 1e3,
        p90.as_secs_f64() * 1e3,
        samples.len()
    );
}

/// Whether cards use the default body or a consumer-authored one (header,
/// field row and body section), which takes the measured, unlayered path.
#[derive(Clone, Copy)]
enum Body {
    Default,
    Custom,
}

impl Body {
    fn suffix(self) -> &'static str {
        match self {
            Body::Default => "",
            Body::Custom => " custom body",
        }
    }
}

fn measure(cx: &mut gpui::TestAppContext, zoom: f32, pitch: (f32, f32), body: Body) {
    cx.update(|cx| {
        gpui_node_graph::init(cx);
        gpui_node_graph::set_node_graph_theme(cx, NodeGraphTheme::dark());
    });
    let graph = large_graph(zoom, pitch);
    let (editor, cx) = cx.add_window_view(move |_, cx| {
        let editor = NodeGraph::new(graph, cx);
        match body {
            Body::Default => editor,
            Body::Custom => editor.with_node_body_renderer(
                |context: NodeBodyContext<Kind, String, String, String>,
                 _: &mut gpui::Window,
                 _: &mut gpui::App| {
                    NodeBody::new(
                        div()
                            .h(graph_px(100.0))
                            .child(context.field("Gain", div().child("0.50")))
                            .child(context.body_section(
                                div().child(context.field("Mode", div().child("Linear"))),
                            )),
                    )
                },
            ),
        }
    });
    cx.run_until_parked();
    for _ in 0..5 {
        draw(cx);
    }

    let idle = (0..60)
        .map(|_| {
            editor.update(cx, |_, cx| cx.notify());
            draw(cx)
        })
        .collect();
    report(&format!("zoom {zoom}{}: full redraw", body.suffix()), idle);

    let hover = (0..60)
        .map(|frame| {
            let x = 20.0 + (frame % 30) as f32 * 9.0;
            cx.simulate_mouse_move(point(px(x), px(15.0)), None, Modifiers::default());
            draw(cx)
        })
        .collect();
    report(
        &format!("zoom {zoom}{}: hover move + redraw", body.suffix()),
        hover,
    );
}

#[gpui::test]
#[ignore = "benchmark"]
fn frame_cost_all_500_nodes_visible(cx: &mut gpui::TestAppContext) {
    measure(cx, 0.2, SPARSE_PITCH, Body::Default);
}

#[gpui::test]
#[ignore = "benchmark"]
fn frame_cost_500_nodes_mostly_offscreen(cx: &mut gpui::TestAppContext) {
    measure(cx, 1.0, SPARSE_PITCH, Body::Default);
}

#[gpui::test]
#[ignore = "benchmark"]
fn frame_cost_500_overlapping_nodes_visible(cx: &mut gpui::TestAppContext) {
    measure(cx, 0.3, OVERLAPPING_PITCH, Body::Default);
}

#[gpui::test]
#[ignore = "benchmark"]
fn frame_cost_500_custom_body_nodes_visible(cx: &mut gpui::TestAppContext) {
    measure(cx, 0.3, SPARSE_PITCH, Body::Custom);
}
