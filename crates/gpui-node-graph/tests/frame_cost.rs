//! Frame cost on a large synthetic graph. Ignored by default; run with
//! `cargo test --release -p gpui-node-graph --test frame_cost -- --ignored --nocapture`.

use std::time::{Duration, Instant};

use gpui::{Modifiers, MouseButton, ScrollDelta, ScrollWheelEvent, VisualTestContext, point, px};
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
/// The interaction benchmarks use a 20x10 grid: about the size of a busy
/// hand-built graph, all of it on screen.
const INTERACTION_GRID: (usize, usize) = (20, 10);
const PORTS_PER_SIDE: usize = 2;

/// Cards are 160x100. The default pitch leaves gaps; the overlapping pitch
/// stacks every card over its neighbours' sockets, so the card grid and the
/// visible-piece clipping have work to do.
const SPARSE_PITCH: (f32, f32) = (220.0, 140.0);
const OVERLAPPING_PITCH: (f32, f32) = (110.0, 70.0);

fn large_graph(zoom: f32, pitch: (f32, f32)) -> GraphState<String, String, String, Kind> {
    grid_graph(zoom, pitch, (COLUMNS, ROWS))
}

fn grid_graph(
    zoom: f32,
    pitch: (f32, f32),
    (columns, rows): (usize, usize),
) -> GraphState<String, String, String, Kind> {
    let mut graph = GraphState::default();
    for index in 0..columns * rows {
        let id = format!("n{index}");
        let position = Point::new(
            (index % columns) as f32 * pitch.0,
            (index / columns) as f32 * pitch.1,
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
        if index % columns != 0 {
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

type Editor = NodeGraph<Kind, String, String, String>;

fn editor(
    graph: GraphState<String, String, String, Kind>,
    body: Body,
    cx: &mut gpui::Context<Editor>,
) -> Editor {
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
}

fn measure(cx: &mut gpui::TestAppContext, zoom: f32, pitch: (f32, f32), body: Body) {
    cx.update(|cx| {
        gpui_node_graph::init(cx);
        gpui_node_graph::set_node_graph_theme(cx, NodeGraphTheme::dark());
    });
    let graph = large_graph(zoom, pitch);
    let (editor, cx) = cx.add_window_view(move |_, cx| editor(graph, body, cx));
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

/// Pan (middle drag over blank canvas), wheel zoom and a card drag, each one
/// input event plus a full frame, on 200 visible cards with wires.
fn measure_interactions(cx: &mut gpui::TestAppContext, body: Body) {
    cx.update(|cx| {
        gpui_node_graph::init(cx);
        gpui_node_graph::set_node_graph_theme(cx, NodeGraphTheme::dark());
    });
    let zoom = 0.4;
    let graph = grid_graph(zoom, SPARSE_PITCH, INTERACTION_GRID);
    let (_, cx) = cx.add_window_view(move |_, cx| editor(graph, body, cx));
    cx.run_until_parked();
    for _ in 0..5 {
        draw(cx);
    }
    let none = Modifiers::default();
    let suffix = body.suffix();

    let header = point(px(80.0 * zoom), px(12.0 * zoom));
    cx.simulate_mouse_down(header, MouseButton::Left, none);
    let drag = (0..120)
        .map(|frame| {
            let step = (frame % 40) as f32;
            cx.simulate_mouse_move(
                header + point(px(step * 4.0), px(step * 2.0)),
                MouseButton::Left,
                none,
            );
            draw(cx)
        })
        .collect();
    cx.simulate_mouse_up(header, MouseButton::Left, none);
    report(&format!("200 nodes{suffix}: card drag + redraw"), drag);

    let blank = point(px(190.0 * zoom), px(120.0 * zoom));
    cx.simulate_mouse_down(blank, MouseButton::Middle, none);
    let pan = (0..120)
        .map(|frame| {
            let step = (frame % 40) as f32 - 20.0;
            cx.simulate_mouse_move(
                blank + point(px(step * 3.0), px(step * 2.0)),
                MouseButton::Middle,
                none,
            );
            draw(cx)
        })
        .collect();
    cx.simulate_mouse_up(blank, MouseButton::Middle, none);
    report(&format!("200 nodes{suffix}: pan + redraw"), pan);

    let zoom_frames = (0..120)
        .map(|frame| {
            let direction = if (frame / 10) % 2 == 0 { 40.0 } else { -40.0 };
            cx.simulate_event(ScrollWheelEvent {
                position: point(px(400.0), px(300.0)),
                delta: ScrollDelta::Pixels(point(px(0.0), px(direction))),
                ..Default::default()
            });
            draw(cx)
        })
        .collect();
    report(
        &format!("200 nodes{suffix}: wheel zoom + redraw"),
        zoom_frames,
    );
}

#[gpui::test]
#[ignore = "benchmark"]
fn frame_cost_200_nodes_pan_zoom_drag(cx: &mut gpui::TestAppContext) {
    measure_interactions(cx, Body::Default);
}

#[gpui::test]
#[ignore = "benchmark"]
fn frame_cost_200_custom_body_nodes_pan_zoom_drag(cx: &mut gpui::TestAppContext) {
    measure_interactions(cx, Body::Custom);
}
