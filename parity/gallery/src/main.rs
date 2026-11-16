//! GPUI half of the node-graph parity harness: the shared fixture in the
//! default node chrome, with every tagged part's bounds readable from
//! `globalThis.__nodeGraphParts()`.

use std::{cell::RefCell, collections::BTreeMap};

use gpui::{
    AnyElement, App, AppContext, Bounds, Element, ElementId, FontWeight, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, ParentElement, Pixels, SharedString, Styled, Window,
    WindowOptions, div, prelude::FluentBuilder, rgb,
};
use gpui_node_graph::{
    NodeBody, NodeBodyContext, NodeCatalogItem, NodeGraph, NodeGraphTheme, PortPresentation,
    core::{Connection, GraphState, Node, Point, Port, PortDirection, PortType, Size},
    graph_px as px,
    style::{AnchorLayout, Border, Color},
};

include!("../../fixture.rs");

#[derive(Clone, Debug, PartialEq)]
struct Kind;
impl PortType for Kind {
    fn compatible(_: &Self, _: &Self) -> bool {
        true
    }
}

thread_local! {
    static PARTS: RefCell<BTreeMap<String, [f32; 4]>> = const { RefCell::new(BTreeMap::new()) };
}

struct Probe {
    id: SharedString,
    child: Option<AnyElement>,
}

impl Element for Probe {
    type RequestLayoutState = AnyElement;
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let mut child = self.child.take().expect("probe laid out once");
        (child.request_layout(window, cx), child)
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.prepaint(window, cx);
        let rect = [
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height),
        ];
        PARTS.with(|parts| parts.borrow_mut().insert(self.id.to_string(), rect));
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        child: &mut AnyElement,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        child.paint(window, cx);
    }
}

impl IntoElement for Probe {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn probe(element: AnyElement, id: SharedString) -> AnyElement {
    Probe {
        id,
        child: Some(element),
    }
    .into_any_element()
}

/// The rship scenes theme (`apps/ui-leptos/src/activities/scenes/theme.rs`).
fn scene_graph_theme() -> NodeGraphTheme {
    let mut theme = NodeGraphTheme::default();
    let node = &mut theme.node;
    node.background = Color::rgb(0x1c1c1c);
    node.border = Border::none();
    node.border_radius = 2.0;
    node.shadow = Vec::new();
    node.shadow_selected = Vec::new();
    node.outline_selected = Border::solid(1.5, Color::rgb(0xf2f2f2));
    node.header_background = Color::rgb(0x1c1c1c);
    node.header_color = Color::rgb(0xebebeb);
    node.header_font_size = 12.0;
    node.header_border_bottom = Border::none();
    node.header_accent_height = 2.0;
    node.body_border_bottom = Border::none();
    node.min_width = 256.0;
    node.width = Some(256.0);
    node.padding_x = 6.0;
    node.header_padding_y = 4.0;
    node.body_padding_y = 3.0;
    node.ports_padding_y = 2.0;
    node.field_label_color = Color::rgb(0xbebebe);
    node.field_label_font_size = 11.0;
    node.resize_handle_width = 4.0;
    node.anchor_layout = AnchorLayout::Stacked;
    let anchor = &mut theme.anchor;
    anchor.dot_size = 8.0;
    anchor.dot_border_width = 1.0;
    anchor.dot_color = Color::rgb(0x8a8a8a);
    anchor.dot_connected_color = Color::rgb(0xa1a1a1);
    anchor.row_height = 20.0;
    anchor.row_padding_x = 6.0;
    let connection = &mut theme.connection;
    connection.stroke = Color::rgb(0xababab);
    connection.stroke_selected = Color::rgb(0xffffff);
    connection.stroke_draft = Color::rgb(0xd1d1d1);
    connection.stroke_width = 2.5;
    connection.stroke_width_selected = 3.0;
    theme
}

fn fixture(width: f32, query: &str) -> GraphState<String, String, String, Kind> {
    let mut graph = GraphState::default();
    graph.viewport.zoom = fixture_zoom(query);
    for node in fixture_nodes(query) {
        let position = Point::new(node.x, node.y);
        graph.nodes.insert(
            node.id.into(),
            Node {
                id: node.id.into(),
                node_type: node.id.into(),
                title: node.title.into(),
                position,
                size: Size { width, height: 0.0 },
            },
        );
        let ports = node
            .inputs
            .iter()
            .map(|port| (port, PortDirection::Input))
            .chain(
                node.outputs
                    .iter()
                    .map(|port| (port, PortDirection::Output)),
            );
        for ((id, label), direction) in ports {
            graph.ports.insert(
                (*id).into(),
                Port {
                    id: (*id).into(),
                    node: node.id.into(),
                    label: (*label).into(),
                    direction,
                    kind: Kind,
                    position,
                },
            );
        }
    }
    for (id, source, target) in fixture_connections(query) {
        graph.connections.insert(
            (*id).into(),
            Connection {
                id: (*id).into(),
                source: (*source).into(),
                target: (*target).into(),
            },
        );
    }
    graph
}

fn catalog(query: &str) -> Vec<NodeCatalogItem<Kind>> {
    fixture_nodes(query)
        .iter()
        .map(|node| NodeCatalogItem {
            id: node.id.into(),
            label: node.title.into(),
            category: String::new(),
            category_color: node.accent.map(Color::rgb),
            description: String::new(),
            keywords: Vec::new(),
            ports: Vec::new(),
        })
        .collect()
}

fn hsla(color: Color) -> gpui::Hsla {
    gpui::Hsla::from(rgb(color.rgb)).opacity(color.alpha)
}

/// The `body` fixture's card as a consumer draws it: leptos-node-graph's
/// accent, header, body slot and port rows, every length in graph pixels so
/// the whole card scales with zoom.
fn body_card(context: NodeBodyContext<Kind, String, String, String>) -> NodeBody {
    let node = &context.theme.node;
    let anchor = &context.theme.anchor;
    let id = &context.node.id;
    let header_border = node.header_border_bottom;
    let header = div()
        .px(px(node.padding_x))
        .py(px(node.header_padding_y))
        .bg(hsla(node.header_background))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(node.header_font_size))
        .line_height(px(node.header_font_size * 1.2))
        .when(header_border.width > 0.0, |element| {
            element
                .border_b(px(header_border.width))
                .border_color(hsla(header_border.color))
        })
        .child(context.node.title.to_uppercase());
    let value = div()
        .text_size(px(BODY_TEXT_SIZE))
        .line_height(px(BODY_LINE_HEIGHT))
        .text_color(rgb(BODY_TEXT_COLOR))
        .child(BODY_FIELD.1);
    let row = context.field(
        BODY_FIELD.0,
        probe(
            value.into_any_element(),
            format!("node-graph.body-text:{id}").into(),
        ),
    );
    let body = context.body_section(
        div()
            .px(px(node.padding_x))
            .child(probe(row, format!("node-graph.body-row:{id}").into())),
    );
    let port_row = |port: &Port<String, String, Kind>| {
        div()
            .flex()
            .items_center()
            .gap(px(anchor.row_gap))
            .px(px(anchor.row_padding_x))
            .h(px(anchor.row_height))
            .overflow_hidden()
            .when(port.direction == PortDirection::Output, |element| {
                element.flex_row_reverse()
            })
            .child(context.default_port_anchor(port.id.clone()))
            .child(
                div()
                    .whitespace_nowrap()
                    .text_size(px(anchor.label_font_size))
                    .text_color(hsla(anchor.label_color))
                    .child(port.label.clone()),
            )
    };
    let column = |direction| {
        div().flex().flex_col().children(
            context
                .ports
                .iter()
                .filter(|port| port.direction == direction)
                .map(port_row),
        )
    };
    let stacked = node.anchor_layout == AnchorLayout::Stacked;
    let ports = div()
        .flex()
        .py(px(node.ports_padding_y))
        .when(stacked, |element| element.flex_col())
        .when(!stacked, |element| element.justify_between())
        .child(column(PortDirection::Input))
        .child(column(PortDirection::Output).when(!stacked, |element| element.items_end()));
    let card = div()
        .flex()
        .flex_col()
        .when_some(context.category_color, |element, color| {
            element.child(div().h(px(node.header_accent_height)).bg(hsla(color)))
        })
        .child(header)
        .child(body)
        .child(ports);
    NodeBody::new(card).with_ports(PortPresentation::BodyAnchors)
}

fn query() -> String {
    web_query().unwrap_or_default()
}

#[cfg(target_family = "wasm")]
fn web_query() -> Option<String> {
    let location = js_sys::Reflect::get(&js_sys::global(), &"location".into()).ok()?;
    js_sys::Reflect::get(&location, &"search".into())
        .ok()?
        .as_string()
}

#[cfg(not(target_family = "wasm"))]
fn web_query() -> Option<String> {
    std::env::args().nth(1)
}

fn launch(cx: &mut App) {
    gpui_node_graph::init(cx);
    let scene = scene_theme(&query());
    let theme = if scene {
        scene_graph_theme()
    } else {
        NodeGraphTheme::default()
    };
    let mut theme = theme;
    // The reference page's background; the editor itself is transparent.
    theme.editor.background = Color::rgb(0x111111);
    let query = query();
    let width = theme.node.width.unwrap_or(theme.node.min_width);
    gpui_node_graph::set_node_graph_theme(cx, theme);
    gpui_node_graph::set_part_tagger(cx, probe);
    cx.open_window(WindowOptions::default(), move |_, cx| {
        cx.new(|cx| {
            let graph = NodeGraph::new(fixture(width, &query), cx).with_catalog(catalog(&query));
            if body_fixture(&query) {
                graph.with_node_body_renderer(|context, _: &mut Window, _: &mut App| {
                    body_card(context)
                })
            } else {
                graph
            }
        })
    })
    .expect("parity window opens");
}

#[cfg(target_family = "wasm")]
fn main() {
    use wasm_bindgen::{JsCast, closure::Closure};
    gpui_platform::web_init();
    let application = gpui_platform::application().run_embedded(launch);
    std::mem::forget(application);
    let parts = Closure::<dyn Fn() -> String>::new(|| {
        PARTS.with(|parts| {
            let body = parts
                .borrow()
                .iter()
                .map(|(id, [x, y, w, h])| format!(r#""{id}":[{x},{y},{w},{h}]"#))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{body}}}")
        })
    });
    js_sys::Reflect::set(
        &js_sys::global(),
        &"__nodeGraphParts".into(),
        parts.as_ref().unchecked_ref(),
    )
    .expect("globalThis accepts the parts bridge");
    parts.forget();
}

#[cfg(not(target_family = "wasm"))]
fn main() {
    gpui_platform::application().run(launch);
}
