mod windows;
use gpui::{
    Context, MouseButton, MouseDownEvent, MouseMoveEvent, PathBuilder, Render, Window, canvas, div,
    point, prelude::*, px, rgb,
};
pub use node_graph_core as core;
pub use node_graph_core::*;
pub use windows::*;

#[derive(Clone, Debug)]
pub enum EditorEvent<N: Eq + std::hash::Hash = String, C: Eq + std::hash::Hash = String> {
    NodeMoved {
        id: N,
        position: core::Point,
    },
    SelectionChanged {
        nodes: std::collections::HashSet<N>,
        connections: std::collections::HashSet<C>,
    },
    ViewportChanged {
        viewport: Viewport,
    },
    GraphReconciled,
}
#[derive(Clone, Debug)]
pub struct Theme {
    pub background: u32,
    pub node: u32,
    pub node_selected: u32,
    pub wire: u32,
    pub text: u32,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            background: 0x18181b,
            node: 0x27272a,
            node_selected: 0x3f3f46,
            wire: 0x71717a,
            text: 0xe4e4e7,
        }
    }
}
/// Shared native/WebAssembly retained-mode GPUI node editor. Domain state stays framework-free in `node_graph_core`.
pub struct NodeGraph<
    T: PortType,
    N: core::NodeId = String,
    P: core::PortId = String,
    C: core::ConnectionId = String,
> {
    pub graph: GraphState<N, P, C, T>,
    pub theme: Theme,
    drag: Option<(N, core::Point)>,
    panning: Option<core::Point>,
}
impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>
    gpui::EventEmitter<EditorEvent<N, C>> for NodeGraph<T, N, P, C>
{
}
impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> NodeGraph<T, N, P, C> {
    pub fn new(mut graph: GraphState<N, P, C, T>) -> Self {
        graph.canonicalize_ids();
        graph.viewport = graph.viewport.sanitized();
        Self {
            graph,
            theme: Theme::default(),
            drag: None,
            panning: None,
        }
    }
    /// Replace domain and transient state after canonicalization and validation.
    pub fn set_graph(
        &mut self,
        mut graph: GraphState<N, P, C, T>,
        cx: &mut Context<Self>,
    ) -> Result<(), GraphValidationError> {
        graph.canonicalize_ids();
        graph.viewport = graph.viewport.sanitized();
        graph.validate()?;
        self.graph = graph;
        cx.emit(EditorEvent::GraphReconciled);
        cx.emit(EditorEvent::SelectionChanged {
            nodes: self.graph.selected_nodes.clone(),
            connections: self.graph.selected_connections.clone(),
        });
        cx.emit(EditorEvent::ViewportChanged {
            viewport: self.graph.viewport,
        });
        cx.notify();
        Ok(())
    }
    /// Reconcile persisted domain data while preserving valid selection and viewport.
    pub fn reconcile(
        &mut self,
        snapshot: GraphSnapshot<N, P, C, T>,
        cx: &mut Context<Self>,
    ) -> Result<(), GraphValidationError> {
        self.graph.reconcile(snapshot)?;
        cx.emit(EditorEvent::GraphReconciled);
        cx.notify();
        Ok(())
    }
}
impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> Render
    for NodeGraph<T, N, P, C>
{
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vp = self.graph.viewport.sanitized();
        let wires: Vec<_> = self
            .graph
            .connections
            .values()
            .filter_map(|c| {
                Some((
                    self.graph.ports.get(&c.source)?.position,
                    self.graph.ports.get(&c.target)?.position,
                ))
            })
            .collect();
        let color = rgb(self.theme.wire);
        let wire_layer = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                for (a, b) in &wires {
                    let a = vp.world_to_screen(*a);
                    let b = vp.world_to_screen(*b);
                    let mid = (a.x + b.x) / 2.;
                    let mut p = PathBuilder::stroke(px(2.));
                    p.move_to(point(px(a.x), px(a.y)));
                    p.line_to(point(px(mid), px(a.y)));
                    p.line_to(point(px(mid), px(b.y)));
                    p.line_to(point(px(b.x), px(b.y)));
                    if let Ok(path) = p.build() {
                        window.paint_path(path, color);
                    }
                }
            },
        )
        .absolute()
        .size_full();
        let mut root = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(self.theme.background))
            .child(wire_layer);
        for n in self.graph.nodes.values() {
            let id = n.id.clone();
            let pos = vp.world_to_screen(n.position);
            let selected = self.graph.selected_nodes.contains(&id);
            let bg = if selected {
                self.theme.node_selected
            } else {
                self.theme.node
            };
            root = root.child(
                div()
                    .absolute()
                    .left(px(pos.x))
                    .top(px(pos.y))
                    .w(px(n.size.width * vp.zoom))
                    .h(px(n.size.height * vp.zoom))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x52525b))
                    .bg(rgb(bg))
                    .text_color(rgb(self.theme.text))
                    .text_size(px(14. * vp.zoom))
                    .p(px(8. * vp.zoom))
                    .child(n.title.clone())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            let screen = core::Point::new(e.position.x.into(), e.position.y.into());
                            let world = this.graph.viewport.screen_to_world(screen);
                            if let Some(node) = this.graph.nodes.get(&id) {
                                this.drag = Some((id.clone(), world - node.position));
                                this.graph.selected_nodes.clear();
                                this.graph.selected_nodes.insert(id.clone());
                                cx.emit(EditorEvent::SelectionChanged {
                                    nodes: this.graph.selected_nodes.clone(),
                                    connections: this.graph.selected_connections.clone(),
                                });
                                cx.notify();
                            }
                        }),
                    ),
            )
        }
        root.on_mouse_down(
            MouseButton::Middle,
            cx.listener(|this, e: &MouseDownEvent, _, _| {
                this.panning = Some(core::Point::new(e.position.x.into(), e.position.y.into()))
            }),
        )
        .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
            let s = core::Point::new(e.position.x.into(), e.position.y.into());
            if let Some(last) = this.panning.as_mut() {
                this.graph.viewport.pan = this.graph.viewport.pan + (s - *last);
                *last = s;
                cx.emit(EditorEvent::ViewportChanged {
                    viewport: this.graph.viewport,
                });
                cx.notify();
            }
            if let Some((id, off)) = this.drag.clone() {
                let p = this.graph.viewport.screen_to_world(s) - off;
                if this.graph.move_node(&id, p).is_some() {
                    cx.notify();
                }
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                if let Some((id, _)) = this.drag.take()
                    && let Some(n) = this.graph.nodes.get(&id)
                {
                    cx.emit(EditorEvent::NodeMoved {
                        id,
                        position: n.position,
                    });
                }
            }),
        )
        .on_mouse_up(
            MouseButton::Middle,
            cx.listener(|this, _, _, _| this.panning = None),
        )
    }
}
