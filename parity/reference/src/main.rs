//! Leptos half of the node-graph parity harness: the shared fixture rendered by
//! the published leptos-node-graph 0.2.2 crate.

use std::{collections::HashMap, marker::PhantomData};

use leptos::prelude::*;
use leptos_node_graph::{
    AnchorLayout, AnchorStyle, ConnectionEntry, ConnectionStyle, EditorConfig, GraphEvent,
    InputAnchor, Node, NodeEditor, NodeStyle, OutputAnchor, PortType, Position,
};

include!("../../fixture.rs");

#[derive(Clone, Debug, PartialEq)]
struct Kind;
impl PortType for Kind {
    fn compatible(_: &Self, _: &Self) -> bool {
        true
    }
    fn type_id(&self) -> String {
        "value".into()
    }
    fn from_type_id(_: &str) -> Self {
        Kind
    }
}

type Marker = PhantomData<(String, String)>;

/// The rship scenes theme (`apps/ui-leptos/src/activities/scenes/theme.rs`).
fn provide_scene_theme() {
    provide_context(ConnectionStyle {
        stroke: "#ababab".into(),
        stroke_selected: "#ffffff".into(),
        stroke_draft: "#d1d1d1".into(),
        stroke_width: 2.5,
        stroke_width_selected: 3.0,
    });
    provide_context(NodeStyle {
        background: "#1c1c1c".into(),
        border: "none".into(),
        border_radius: "2px".into(),
        shadow: "none".into(),
        shadow_selected: "none".into(),
        outline_selected: "1.5px solid #f2f2f2".into(),
        header_background: "#1c1c1c".into(),
        header_color: "#ebebeb".into(),
        header_font_size: "12px".into(),
        header_border_bottom: "none".into(),
        header_accent_height: 2.0,
        body_border_bottom: "none".into(),
        min_width: "256px".into(),
        width: "256px".into(),
        padding_x: 6.0,
        header_padding_y: 4.0,
        body_padding_y: 3.0,
        ports_padding_y: 2.0,
        field_label_color: "#bebebe".into(),
        field_label_font_size: "11px".into(),
        resize_handle_width: 4.0,
        anchor_layout: AnchorLayout::Stacked,
        ..Default::default()
    });
    provide_context(AnchorStyle {
        dot_size: 8.0,
        dot_border_width: 1.0,
        dot_color: "#8a8a8a".into(),
        dot_connected_color: "#a1a1a1".into(),
        row_height: 20.0,
        row_padding: "0 6px".into(),
        ..Default::default()
    });
}

#[component]
fn App() -> impl IntoView {
    let search = window().location().search().unwrap_or_default();
    if scene_theme(&search) {
        provide_scene_theme();
    }
    let connections = RwSignal::new(
        CONNECTIONS
            .iter()
            .map(|(id, source, target)| {
                (
                    id.to_string(),
                    ConnectionEntry {
                        id: id.to_string(),
                        source: source.to_string(),
                        target: target.to_string(),
                    },
                )
            })
            .collect::<HashMap<_, _>>(),
    );
    let on_event = Callback::new(
        move |event: GraphEvent<String, String, String>| match event {
            GraphEvent::ConnectionRequested { source, target } => {
                let id = format!("{source}->{target}");
                connections.update(|map| {
                    map.insert(id.clone(), ConnectionEntry { id, source, target });
                });
            }
            GraphEvent::ConnectionRemoved { id } => {
                connections.update(|map| {
                    map.remove(&id);
                });
            }
            _ => {}
        },
    );
    // One `<For>` row per node, as rship's scenes surface renders them: each
    // row owns its reactive scope, so every `Node`'s `NodeContext` reaches only
    // its own anchors. Sibling components in one scope would all resolve the
    // last node's context and register every port under it.
    let node_view = |node: &'static FixtureNode| {
        let position = RwSignal::new(Position::new(node.x as f64, node.y as f64));
        let title = node.title;
        let inputs = node.inputs;
        let outputs = node.outputs;
        let header: Children = Box::new(move || view! { {title} }.into_any());
        view! {
            <Node<String, String, String, Kind>
                id=node.id.to_string()
                position=position
                header=header
                accent_color=node.accent.map(|rgb| format!("#{rgb:06x}"))
                inputs=move || inputs.iter().map(|(id, label)| view! {
                    <InputAnchor<String, String, String, Kind>
                        id=id.to_string() port_type=Kind label=label.to_string() _marker=Marker::default() />
                }).collect_view()
                outputs=move || outputs.iter().map(|(id, label)| view! {
                    <OutputAnchor<String, String, String, Kind>
                        id=id.to_string() port_type=Kind label=label.to_string() _marker=Marker::default() />
                }).collect_view()
            />
        }
    };
    view! {
        <div style="position:absolute; inset:0;">
            <NodeEditor<String, String, String, Kind>
                config=EditorConfig::default()
                connections=Signal::derive(move || connections.get())
                on_event=on_event
            >
                <For each=|| NODES.iter() key=|node| node.id children=node_view />
            </NodeEditor<String, String, String, Kind>>
        </div>
    }
}

fn main() {
    mount_to_body(App);
    if let Some(body) = document().body() {
        let _ = body.set_attribute("data-ready", "");
    }
}
