//! GPUI half of the node-graph parity harness: the shared fixture in the
//! default node chrome, with every tagged part's bounds readable from
//! `globalThis.__nodeGraphParts()`.

use std::{cell::RefCell, collections::BTreeMap};

use gpui::{
    AnyElement, App, AppContext, Bounds, Element, ElementId, GlobalElementId, InspectorElementId, IntoElement,
    LayoutId, Pixels, SharedString, Window, WindowOptions,
};
use gpui_node_graph::{
    NodeCatalogItem, NodeGraph, NodeGraphTheme,
    core::{Connection, GraphState, Node, Point, Port, PortDirection, PortType, Size},
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

fn fixture(width: f32) -> GraphState<String, String, String, Kind> {
    let mut graph = GraphState::default();
    for node in NODES {
        let position = Point::new(node.x, node.y);
        graph.nodes.insert(
            node.id.into(),
            Node {
                id: node.id.into(),
                node_type: node.id.into(),
                title: node.title.into(),
                position,
                size: Size {
                    width,
                    height: 0.0,
                },
            },
        );
        let ports = node
            .inputs
            .iter()
            .map(|port| (port, PortDirection::Input))
            .chain(node.outputs.iter().map(|port| (port, PortDirection::Output)));
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
    for (id, source, target) in CONNECTIONS {
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

fn catalog() -> Vec<NodeCatalogItem<Kind>> {
    NODES
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
    let width = theme.node.width.unwrap_or(theme.node.min_width);
    gpui_node_graph::set_node_graph_theme(cx, theme);
    gpui_node_graph::set_part_tagger(cx, probe);
    cx.open_window(WindowOptions::default(), move |_, cx| {
        cx.new(|cx| NodeGraph::new(fixture(width), cx).with_catalog(catalog()))
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
