mod windows;

use gpui::{
    AnyElement, App, Bounds, Context, FocusHandle, KeyDownEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Render, ScrollWheelEvent, WeakEntity,
    Window, canvas, div, point, prelude::*, px, rgb,
};
use std::{cell::Cell, collections::HashSet, rc::Rc, sync::Arc};

pub use node_graph_core as core;
pub use node_graph_core::*;
pub use windows::*;

/// The GPUI adapter and framework-free core now expose one event vocabulary.
pub type EditorEvent<N = String, P = String, C = String> = core::GraphEvent<N, P, C>;

pub struct NodeOverlay {
    /// Screen-pixel offset from the rendered node origin. Overlay content is not
    /// viewport-scaled, so retained controls keep normal GPUI hit testing.
    pub offset: core::Point,
    pub element: AnyElement,
}

impl NodeOverlay {
    pub fn new(offset: core::Point, element: impl IntoElement) -> Self {
        Self {
            offset,
            element: element.into_any_element(),
        }
    }
}

pub struct NodeBody {
    pub element: AnyElement,
    pub overlays: Vec<NodeOverlay>,
}

impl NodeBody {
    pub fn new(element: impl IntoElement) -> Self {
        Self {
            element: element.into_any_element(),
            overlays: Vec::new(),
        }
    }

    pub fn with_overlay(mut self, overlay: NodeOverlay) -> Self {
        self.overlays.push(overlay);
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeVisualState {
    pub selected: bool,
    pub zoom: f32,
}

pub struct NodeBodyContext<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> {
    pub node: Node<N>,
    pub ports: Arc<[Port<N, P, T>]>,
    pub state: NodeVisualState,
    pub theme: Theme,
    graph: WeakEntity<NodeGraph<T, N, P, C>>,
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>
    NodeBodyContext<T, N, P, C>
{
    pub fn graph(&self) -> WeakEntity<NodeGraph<T, N, P, C>> {
        self.graph.clone()
    }
}

pub trait NodeBodyRenderer<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>:
    'static
{
    fn render_node(
        &mut self,
        context: NodeBodyContext<T, N, P, C>,
        window: &mut Window,
        cx: &mut App,
    ) -> NodeBody;
}

impl<T, N, P, C, F> NodeBodyRenderer<T, N, P, C> for F
where
    T: PortType,
    N: core::NodeId,
    P: core::PortId,
    C: core::ConnectionId,
    F: FnMut(NodeBodyContext<T, N, P, C>, &mut Window, &mut App) -> NodeBody + 'static,
{
    fn render_node(
        &mut self,
        context: NodeBodyContext<T, N, P, C>,
        window: &mut Window,
        cx: &mut App,
    ) -> NodeBody {
        self(context, window, cx)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoutingMode {
    SimpleOrthogonal,
    Subway(core::subway::SubwayOptions),
}

impl Default for RoutingMode {
    fn default() -> Self {
        Self::Subway(core::subway::SubwayOptions::default())
    }
}

#[derive(Clone, Debug)]
pub struct EditorConfig {
    pub min_zoom: f32,
    pub max_zoom: f32,
    pub zoom_step: f32,
    pub grid_size: Option<f32>,
    /// Port snap radius in screen pixels.
    pub snap_distance: f32,
    pub fit_padding: f32,
    pub fit_max_zoom: f32,
    pub routing: RoutingMode,
    pub min_node_width: f32,
    pub max_node_width: f32,
    pub default_node_width: f32,
}
impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            min_zoom: 0.1,
            max_zoom: 5.0,
            zoom_step: 0.3,
            grid_size: None,
            snap_distance: 22.0,
            fit_padding: 48.0,
            fit_max_zoom: 1.0,
            routing: RoutingMode::default(),
            min_node_width: 96.0,
            max_node_width: 800.0,
            default_node_width: 180.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CatalogPort<T> {
    /// Stable, consumer-defined port key used by `GraphEvent::CreateNode`.
    pub id: String,
    pub label: String,
    pub direction: PortDirection,
    pub kind: T,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeCatalogItem<T> {
    pub id: String,
    pub label: String,
    pub category: String,
    pub description: String,
    pub keywords: Vec<String>,
    pub ports: Vec<CatalogPort<T>>,
}

#[derive(Clone)]
struct CatalogMenu<P> {
    anchor_world: core::Point,
    query: String,
    selected: usize,
    connect_from: Option<P>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphGroup<N: Eq + std::hash::Hash> {
    pub id: String,
    pub label: String,
    pub color: u32,
    pub nodes: HashSet<N>,
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub background: u32,
    pub node: u32,
    pub node_selected: u32,
    pub wire: u32,
    pub wire_selected: u32,
    pub wire_draft: u32,
    pub text: u32,
    pub port_input: u32,
    pub port_output: u32,
    pub port_connected: u32,
    pub port_compatible: u32,
    pub selection_border: u32,
    pub selection_fill: u32,
}
impl Default for Theme {
    fn default() -> Self {
        Self {
            background: 0x18181b,
            node: 0x27272a,
            node_selected: 0x3f3f46,
            wire: 0x71717a,
            wire_selected: 0xef4444,
            wire_draft: 0x22d3ee,
            text: 0xe4e4e7,
            port_input: 0x60a5fa,
            port_output: 0xf59e0b,
            port_connected: 0xa1a1aa,
            port_compatible: 0x22d3ee,
            selection_border: 0x60a5fa,
            selection_fill: 0x1e3a5f,
        }
    }
}

#[derive(Clone)]
struct NodeDrag<N> {
    offsets: Vec<(N, core::Point)>,
    starts: Vec<(N, core::Point)>,
    moved: bool,
}
#[derive(Clone)]
struct ResizeDrag<N, P> {
    id: N,
    start_screen_x: f32,
    start_size: core::Size,
    start_ports: Vec<(P, core::Point)>,
    moved: bool,
}

#[derive(Clone)]
struct DraftConnection<P, C> {
    origin: P,
    start_screen: core::Point,
    current_screen: core::Point,
    snap_target: Option<P>,
    replaced_connection: Option<C>,
    moved: bool,
}
#[derive(Clone)]
struct BoxSelection<N, C> {
    start: core::Point,
    current: core::Point,
    baseline_nodes: HashSet<N>,
    baseline_connections: HashSet<C>,
}
impl<N, C> BoxSelection<N, C> {
    fn rect(&self) -> core::Rect {
        let x = self.start.x.min(self.current.x);
        let y = self.start.y.min(self.current.y);
        core::Rect {
            origin: core::Point::new(x, y),
            size: core::Size {
                width: (self.current.x - self.start.x).abs(),
                height: (self.current.y - self.start.y).abs(),
            },
        }
    }
}

/// Shared native/WebAssembly retained-mode GPUI node editor. Domain state stays
/// framework-free in `node_graph_core`.
pub struct NodeGraph<
    T: PortType,
    N: core::NodeId = String,
    P: core::PortId = String,
    C: core::ConnectionId = String,
> {
    pub graph: GraphState<N, P, C, T>,
    pub theme: Theme,
    pub config: EditorConfig,
    drag: Option<NodeDrag<N>>,
    resize: Option<ResizeDrag<N, P>>,
    panning: Option<core::Point>,
    draft: Option<DraftConnection<P, C>>,
    catalog: Vec<NodeCatalogItem<T>>,
    catalog_menu: Option<CatalogMenu<P>>,
    node_body_renderer: Option<Box<dyn NodeBodyRenderer<T, N, P, C>>>,
    groups: Vec<GraphGroup<N>>,
    box_selection: Option<BoxSelection<N, C>>,
    focus_handle: Option<FocusHandle>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>
    gpui::EventEmitter<core::GraphEvent<N, P, C>> for NodeGraph<T, N, P, C>
{
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> NodeGraph<T, N, P, C> {
    /// Construct an editor, panicking with the validation error for invalid state.
    /// Prefer [`Self::try_new`] when invalid application data is recoverable.
    pub fn new(graph: GraphState<N, P, C, T>) -> Self {
        Self::try_new(graph).unwrap_or_else(|error| panic!("{error}"))
    }

    /// Construct an editor with a focus handle available before its first render.
    pub fn new_in(graph: GraphState<N, P, C, T>, cx: &mut Context<Self>) -> Self {
        let mut editor = Self::new(graph);
        editor.focus_handle = Some(cx.focus_handle());
        editor
    }

    /// Construct an editor only after canonicalizing map-owned IDs and validating
    /// all graph references, geometry, port compatibility, and viewport state.
    pub fn try_new(mut graph: GraphState<N, P, C, T>) -> Result<Self, GraphValidationError> {
        graph.canonicalize_ids();
        graph.validate()?;
        Ok(Self {
            graph,
            theme: Theme::default(),
            config: EditorConfig::default(),
            drag: None,
            resize: None,
            panning: None,
            draft: None,
            catalog: Vec::new(),
            catalog_menu: None,
            node_body_renderer: None,
            groups: Vec::new(),
            box_selection: None,
            focus_handle: None,
            canvas_bounds: Rc::new(Cell::new(Bounds::default())),
        })
    }

    pub fn focus_handle(&self) -> Option<&FocusHandle> {
        self.focus_handle.as_ref()
    }

    pub fn with_catalog(mut self, catalog: Vec<NodeCatalogItem<T>>) -> Self {
        self.catalog = catalog;
        self
    }

    pub fn set_catalog(&mut self, catalog: Vec<NodeCatalogItem<T>>, cx: &mut Context<Self>) {
        self.catalog = catalog;
        self.catalog_menu = None;
        cx.notify();
    }

    pub fn with_node_body_renderer<R>(mut self, renderer: R) -> Self
    where
        R: NodeBodyRenderer<T, N, P, C>,
    {
        self.node_body_renderer = Some(Box::new(renderer));
        self
    }

    pub fn set_node_body_renderer<R>(&mut self, renderer: R, cx: &mut Context<Self>)
    where
        R: NodeBodyRenderer<T, N, P, C>,
    {
        self.node_body_renderer = Some(Box::new(renderer));
        cx.notify();
    }

    pub fn clear_node_body_renderer(&mut self, cx: &mut Context<Self>) {
        self.node_body_renderer = None;
        cx.notify();
    }

    pub fn with_groups(mut self, groups: Vec<GraphGroup<N>>) -> Self {
        self.groups = groups;
        self
    }

    pub fn set_groups(&mut self, groups: Vec<GraphGroup<N>>, cx: &mut Context<Self>) {
        self.groups = groups;
        cx.notify();
    }

    pub fn groups(&self) -> &[GraphGroup<N>] {
        &self.groups
    }

    pub fn upsert_group(&mut self, group: GraphGroup<N>, cx: &mut Context<Self>) {
        if let Some(existing) = self
            .groups
            .iter_mut()
            .find(|existing| existing.id == group.id)
        {
            *existing = group;
        } else {
            self.groups.push(group);
        }
        cx.notify();
    }

    fn compatible_catalog_port(&self, item_index: usize, origin: &P) -> Option<&CatalogPort<T>> {
        let origin = self.graph.ports.get(origin)?;
        self.catalog
            .get(item_index)?
            .ports
            .iter()
            .find(|candidate| match (origin.direction, candidate.direction) {
                (PortDirection::Output, PortDirection::Input) => {
                    T::compatible(&origin.kind, &candidate.kind)
                }
                (PortDirection::Input, PortDirection::Output) => {
                    T::compatible(&candidate.kind, &origin.kind)
                }
                _ => false,
            })
    }

    fn filtered_catalog_indices(&self) -> Vec<usize> {
        let Some(menu) = self.catalog_menu.as_ref() else {
            return Vec::new();
        };
        let query = menu.query.to_lowercase();
        self.catalog
            .iter()
            .enumerate()
            .filter(|(index, item)| {
                menu.connect_from
                    .as_ref()
                    .is_none_or(|origin| self.compatible_catalog_port(*index, origin).is_some())
                    && (query.is_empty()
                        || item.label.to_lowercase().contains(&query)
                        || item.category.to_lowercase().contains(&query)
                        || item.description.to_lowercase().contains(&query)
                        || item
                            .keywords
                            .iter()
                            .any(|keyword| keyword.to_lowercase().contains(&query)))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn open_catalog(&mut self, at_screen: core::Point, connect_from: Option<P>) {
        if self.catalog.is_empty() {
            self.catalog_menu = None;
            self.draft = None;
            return;
        }
        self.draft = None;
        self.catalog_menu = Some(CatalogMenu {
            anchor_world: self.graph.viewport.screen_to_world(at_screen),
            query: String::new(),
            selected: 0,
            connect_from,
        });
    }

    fn choose_catalog(&mut self, item_index: usize, cx: &mut Context<Self>) {
        let Some(menu) = self.catalog_menu.take() else {
            return;
        };
        let Some(item) = self.catalog.get(item_index) else {
            return;
        };
        let compatible = menu
            .connect_from
            .as_ref()
            .and_then(|origin| self.compatible_catalog_port(item_index, origin))
            .map(|port| (port.id.clone(), port.direction));
        cx.emit(core::GraphEvent::CreateNode {
            item_id: item.id.clone(),
            position: menu.anchor_world,
            connect_from: menu.connect_from,
            connect_to: compatible.as_ref().map(|(id, _)| id.clone()),
            connect_direction: compatible.map(|(_, direction)| direction),
        });
        cx.notify();
    }

    /// Replace domain and transient state after canonicalization and validation.
    pub fn set_graph(
        &mut self,
        mut graph: GraphState<N, P, C, T>,
        cx: &mut Context<Self>,
    ) -> Result<(), GraphValidationError> {
        graph.canonicalize_ids();
        graph.validate()?;
        self.graph = graph;
        self.cancel_gestures();
        cx.emit(core::GraphEvent::GraphReconciled);
        self.emit_selection(cx);
        cx.emit(core::GraphEvent::ViewportChanged {
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
        let events = self.graph.reconcile(snapshot)?;
        if self
            .draft
            .as_ref()
            .is_some_and(|draft| !self.graph.ports.contains_key(&draft.origin))
        {
            self.draft = None;
        }
        self.drag = None;
        self.box_selection = None;
        for event in events {
            cx.emit(event);
        }
        cx.notify();
        Ok(())
    }

    fn resize_node_width(&mut self, id: &N, width: f32) -> bool {
        if !width.is_finite() || width <= 0.0 {
            return false;
        }
        let Some(node) = self.graph.nodes.get(id) else {
            return false;
        };
        let delta = width as f64 - node.size.width as f64;
        let translated: Option<Vec<_>> = self
            .graph
            .ports
            .iter()
            .filter(|(_, port)| port.node == *id && port.direction == PortDirection::Output)
            .map(|(port_id, port)| {
                let x = port.position.x as f64 + delta;
                (port.position.x.is_finite() && x.abs() <= f32::MAX as f64)
                    .then(|| (port_id.clone(), core::Point::new(x as f32, port.position.y)))
            })
            .collect();
        let Some(translated) = translated else {
            return false;
        };
        self.graph
            .nodes
            .get_mut(id)
            .expect("node remained present during resize")
            .size
            .width = width;
        for (port_id, position) in translated {
            self.graph
                .ports
                .get_mut(&port_id)
                .expect("port remained present during resize")
                .position = position;
        }
        true
    }

    fn cancel_gestures(&mut self) {
        if let Some(drag) = self.drag.take().filter(|drag| drag.moved) {
            let _ = self.graph.move_nodes(&drag.starts);
        }
        if let Some(resize) = self.resize.take().filter(|resize| resize.moved) {
            if let Some(node) = self.graph.nodes.get_mut(&resize.id) {
                node.size = resize.start_size;
            }
            for (id, position) in resize.start_ports {
                if let Some(port) = self.graph.ports.get_mut(&id) {
                    port.position = position;
                }
            }
        }
        self.panning = None;
        self.draft = None;
        self.box_selection = None;
    }

    fn emit_selection(&self, cx: &mut Context<Self>) {
        cx.emit(core::GraphEvent::SelectionChanged {
            nodes: self.graph.selected_nodes.clone(),
            connections: self.graph.selected_connections.clone(),
        });
    }

    fn local_screen(&self, position: gpui::Point<Pixels>) -> core::Point {
        let bounds = self.canvas_bounds.get();
        core::Point::new(
            f32::from(position.x - bounds.origin.x),
            f32::from(position.y - bounds.origin.y),
        )
    }

    fn focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(handle) = &self.focus_handle {
            handle.focus(window, cx);
        }
    }

    fn normalized_connection(&self, first: &P, second: &P) -> Option<(P, P)> {
        let a = self.graph.ports.get(first)?;
        let b = self.graph.ports.get(second)?;
        if a.node == b.node || a.direction == b.direction {
            return None;
        }
        let (source, target) = if a.direction == PortDirection::Output {
            (a, b)
        } else {
            (b, a)
        };
        (T::compatible(&source.kind, &target.kind)).then(|| (source.id.clone(), target.id.clone()))
    }

    fn nearest_compatible_port(&self, origin: &P, cursor: core::Point) -> Option<P> {
        let mut ports: Vec<_> = self.graph.ports.keys().collect();
        ports.sort_by_cached_key(|id| format!("{id:?}"));
        let mut nearest: Option<(P, f32)> = None;
        for id in ports {
            if id == origin || self.normalized_connection(origin, id).is_none() {
                continue;
            }
            let screen = self
                .graph
                .viewport
                .world_to_screen(self.graph.ports[id].position);
            let distance = screen.distance(cursor);
            if distance <= self.config.snap_distance
                && nearest.as_ref().is_none_or(|(_, best)| distance < *best)
            {
                nearest = Some((id.clone(), distance));
            }
        }
        nearest.map(|(id, _)| id)
    }

    fn start_draft(&mut self, id: &P, cx: &mut Context<Self>) {
        let Some(port) = self.graph.ports.get(id) else {
            return;
        };
        let mut origin = id.clone();
        let mut replaced_connection = None;
        // Dragging an occupied input previews a reroute from its existing source.
        // Defer removal until a compatible replacement is actually completed so
        // Escape or a click-only draft leaves the original edge intact.
        if port.direction == PortDirection::Input
            && let Some((connection_id, source)) = self
                .graph
                .connections
                .iter()
                .find(|(_, connection)| &connection.target == id)
                .map(|(connection_id, connection)| {
                    (connection_id.clone(), connection.source.clone())
                })
        {
            replaced_connection = Some(connection_id);
            origin = source;
        }
        let current_screen = self
            .graph
            .viewport
            .world_to_screen(self.graph.ports[&origin].position);
        self.draft = Some(DraftConnection {
            origin,
            start_screen: current_screen,
            current_screen,
            snap_target: None,
            replaced_connection,
            moved: false,
        });
        cx.notify();
    }

    fn finish_draft(&mut self, target: &P, cx: &mut Context<Self>) -> bool {
        let Some(draft) = self.draft.take() else {
            return false;
        };
        let Some((source, target)) = self.normalized_connection(&draft.origin, target) else {
            self.draft = Some(draft);
            return false;
        };
        if let Some(id) = draft.replaced_connection {
            self.graph.connections.remove(&id);
            self.graph.selected_connections.remove(&id);
            cx.emit(core::GraphEvent::ConnectionRemoved { id });
        }
        cx.emit(core::GraphEvent::ConnectionRequested { source, target });
        cx.notify();
        true
    }

    fn connection_route(&self, connection: &Connection<P, C>) -> Option<Vec<core::Point>> {
        let source = self.graph.ports.get(&connection.source)?;
        let target = self.graph.ports.get(&connection.target)?;
        match self.config.routing {
            RoutingMode::SimpleOrthogonal => {
                Some(core::orthogonal_route(source.position, target.position))
            }
            RoutingMode::Subway(options) => {
                let mut nodes: Vec<_> = self.graph.nodes.values().collect();
                nodes.sort_by_cached_key(|node| format!("{:?}", node.id));
                let obstacles: Vec<_> = nodes
                    .iter()
                    .map(|node| core::Rect {
                        origin: node.position,
                        size: node.size,
                    })
                    .collect();
                let start_obstacle = nodes.iter().position(|node| node.id == source.node);
                let end_obstacle = nodes.iter().position(|node| node.id == target.node);
                Some(
                    core::subway::compute_subway_route(
                        &obstacles,
                        core::subway::SubwayConnection {
                            start: source.position,
                            end: target.position,
                            start_obstacle,
                            end_obstacle,
                        },
                        options,
                    )
                    .points,
                )
            }
        }
    }

    fn connection_at(&self, cursor: core::Point, radius: f32) -> Option<C> {
        let viewport = self.graph.viewport.sanitized();
        let mut connections: Vec<_> = self.graph.connections.iter().collect();
        connections.sort_by_cached_key(|(id, _)| format!("{id:?}"));
        let mut nearest: Option<(C, f32)> = None;
        for (id, connection) in connections {
            let Some(route) = self.connection_route(connection) else {
                continue;
            };
            let distance = route
                .windows(2)
                .map(|segment| {
                    distance_to_segment(
                        cursor,
                        viewport.world_to_screen(segment[0]),
                        viewport.world_to_screen(segment[1]),
                    )
                })
                .fold(f32::INFINITY, f32::min);
            if distance <= radius && nearest.as_ref().is_none_or(|(_, best)| distance < *best) {
                nearest = Some((id.clone(), distance));
            }
        }
        nearest.map(|(id, _)| id)
    }

    fn fit_view(&mut self) -> bool {
        let Some(bounds) = self.graph.bounds() else {
            return false;
        };
        let canvas = self.canvas_bounds.get();
        let width = f32::from(canvas.size.width);
        let height = f32::from(canvas.size.height);
        if width <= 0.0 || height <= 0.0 {
            return false;
        }
        let graph_width = bounds.size.width.max(1.0);
        let graph_height = bounds.size.height.max(1.0);
        let available_width = (width - self.config.fit_padding * 2.0).max(1.0);
        let available_height = (height - self.config.fit_padding * 2.0).max(1.0);
        let zoom = (available_width / graph_width)
            .min(available_height / graph_height)
            .clamp(
                self.config.min_zoom,
                self.config.fit_max_zoom.min(self.config.max_zoom),
            );
        let center = core::Point::new(
            bounds.origin.x + graph_width * 0.5,
            bounds.origin.y + graph_height * 0.5,
        );
        let viewport = Viewport {
            zoom,
            pan: core::Point::new(
                width * 0.5 - center.x * zoom,
                height * 0.5 - center.y * zoom,
            ),
        };
        if viewport.is_valid() && viewport != self.graph.viewport {
            self.graph.viewport = viewport;
            true
        } else {
            false
        }
    }

    fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let connection_ids: Vec<_> = self.graph.selected_connections.iter().cloned().collect();
        for id in connection_ids {
            if self.graph.connections.remove(&id).is_some() {
                cx.emit(core::GraphEvent::ConnectionRemoved { id });
            }
        }
        let node_ids: Vec<_> = self.graph.selected_nodes.iter().cloned().collect();
        for event in self.graph.remove_nodes(&node_ids) {
            cx.emit(event);
        }
        self.graph.selected_connections.clear();
        cx.notify();
    }

    fn finish_left_gesture(&mut self, cx: &mut Context<Self>) {
        self.panning = None;
        if let Some(resize) = self.resize.take().filter(|resize| resize.moved)
            && let Some(node) = self.graph.nodes.get(&resize.id)
        {
            cx.emit(core::GraphEvent::NodeResized {
                id: resize.id,
                size: node.size,
            });
        }
        if let Some(drag) = self.drag.take().filter(|drag| drag.moved) {
            let nodes = drag
                .offsets
                .into_iter()
                .filter_map(|(id, _)| Some((id.clone(), self.graph.nodes.get(&id)?.position)))
                .collect();
            cx.emit(core::GraphEvent::NodesMoved { nodes });
        }
        self.box_selection = None;
        let draft_result = self.draft.as_ref().map(|draft| {
            (
                draft.snap_target.clone(),
                draft.moved,
                draft.current_screen,
                draft.origin.clone(),
            )
        });
        match draft_result {
            Some((Some(target), _, _, _)) => {
                self.finish_draft(&target, cx);
            }
            Some((None, true, at, origin)) => {
                self.open_catalog(at, Some(origin));
                cx.notify();
            }
            _ => {}
        }
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let command = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
        let shift = event.keystroke.modifiers.shift;
        if self.catalog_menu.is_some() {
            let mut before = self.filtered_catalog_indices();
            before.truncate(8);
            match key {
                "escape" => self.catalog_menu = None,
                "enter" => {
                    if let Some(menu) = self.catalog_menu.as_ref()
                        && let Some(index) = before.get(menu.selected).copied()
                    {
                        self.choose_catalog(index, cx);
                    }
                }
                "up" => {
                    if let Some(menu) = self.catalog_menu.as_mut() {
                        menu.selected = menu.selected.saturating_sub(1);
                    }
                }
                "down" => {
                    if let Some(menu) = self.catalog_menu.as_mut() {
                        menu.selected = (menu.selected + 1).min(before.len().saturating_sub(1));
                    }
                }
                "backspace" => {
                    if let Some(menu) = self.catalog_menu.as_mut() {
                        menu.query.pop();
                    }
                }
                _ if !command => {
                    if let Some(character) = event.keystroke.key_char.as_ref()
                        && !character.chars().any(char::is_control)
                        && let Some(menu) = self.catalog_menu.as_mut()
                    {
                        menu.query.push_str(character);
                    }
                }
                _ => {}
            }
            let result_len = self.filtered_catalog_indices().len().min(8);
            if let Some(menu) = self.catalog_menu.as_mut() {
                menu.selected = menu.selected.min(result_len.saturating_sub(1));
            }
            cx.notify();
            cx.stop_propagation();
            window.prevent_default();
            return;
        }
        match key {
            "tab" => {
                let bounds = self.canvas_bounds.get();
                self.open_catalog(
                    core::Point::new(
                        f32::from(bounds.size.width) * 0.5,
                        f32::from(bounds.size.height) * 0.5,
                    ),
                    None,
                );
                cx.notify();
                cx.stop_propagation();
                window.prevent_default();
            }
            "delete" | "backspace" => {
                self.delete_selected(cx);
                cx.stop_propagation();
                window.prevent_default();
            }
            "a" if command => {
                self.graph.selected_nodes = self.graph.nodes.keys().cloned().collect();
                self.graph.selected_connections.clear();
                self.emit_selection(cx);
                cx.notify();
                cx.stop_propagation();
                window.prevent_default();
            }
            "f" if !command => {
                if self.fit_view() {
                    cx.emit(core::GraphEvent::ViewportChanged {
                        viewport: self.graph.viewport,
                    });
                    cx.notify();
                }
                cx.stop_propagation();
                window.prevent_default();
            }
            "escape" => {
                self.cancel_gestures();
                self.graph.selected_nodes.clear();
                self.graph.selected_connections.clear();
                self.emit_selection(cx);
                cx.notify();
                cx.stop_propagation();
                window.prevent_default();
            }
            "c" if command => {
                cx.emit(core::GraphEvent::NodesCopied {
                    ids: self.graph.selected_nodes.iter().cloned().collect(),
                });
                cx.stop_propagation();
            }
            "v" if command => {
                cx.emit(core::GraphEvent::NodesPasted {
                    offset: core::Point::new(20.0, 20.0),
                });
                cx.stop_propagation();
            }
            "z" if command && shift => {
                cx.emit(core::GraphEvent::Redo);
                cx.stop_propagation();
            }
            "z" if command => {
                cx.emit(core::GraphEvent::Undo);
                cx.stop_propagation();
            }
            "g" if command => {
                cx.emit(core::GraphEvent::GroupCreated {
                    node_ids: self.graph.selected_nodes.iter().cloned().collect(),
                });
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> Render
    for NodeGraph<T, N, P, C>
{
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_handle.is_none() {
            self.focus_handle = Some(cx.focus_handle());
        }
        let focus_handle = self
            .focus_handle
            .as_ref()
            .expect("focus handle was initialized")
            .clone();
        let viewport = self.graph.viewport.sanitized();

        let wires: Vec<_> = self
            .graph
            .connections
            .values()
            .filter_map(|connection| {
                Some((
                    self.connection_route(connection)?,
                    self.graph.selected_connections.contains(&connection.id),
                ))
            })
            .collect();
        let draft = self.draft.as_ref().and_then(|draft| {
            let source = self.graph.ports.get(&draft.origin)?.position;
            let end = draft
                .snap_target
                .as_ref()
                .and_then(|id| self.graph.ports.get(id))
                .map(|port| viewport.world_to_screen(port.position))
                .unwrap_or(draft.current_screen);
            Some((source, end))
        });
        let wire_color: gpui::Hsla = rgb(self.theme.wire).into();
        let selected_wire_color: gpui::Hsla = rgb(self.theme.wire_selected).into();
        let draft_color: gpui::Hsla = rgb(self.theme.wire_draft).into();
        let canvas_bounds = self.canvas_bounds.clone();
        let wire_layer = canvas(
            move |bounds, _, _| {
                canvas_bounds.set(bounds);
            },
            move |bounds, _, window, _| {
                for (route, selected) in &wires {
                    paint_route(
                        window,
                        bounds,
                        route.iter().map(|point| viewport.world_to_screen(*point)),
                        if *selected {
                            selected_wire_color
                        } else {
                            wire_color
                        },
                        if *selected { 3.0 } else { 2.0 },
                    );
                }
                if let Some((source, end)) = draft {
                    paint_elbow(
                        window,
                        bounds,
                        viewport.world_to_screen(source),
                        end,
                        draft_color,
                        2.0,
                    );
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
            .track_focus(&focus_handle)
            .key_context("NodeGraph")
            .on_key_down(cx.listener(Self::handle_key_down));

        for group in &self.groups {
            let mut members = group.nodes.iter().filter_map(|id| self.graph.nodes.get(id));
            let Some(first) = members.next() else {
                continue;
            };
            let mut left = first.position.x;
            let mut top = first.position.y;
            let mut right = first.position.x + first.size.width;
            let mut bottom = first.position.y + first.size.height;
            for node in members {
                left = left.min(node.position.x);
                top = top.min(node.position.y);
                right = right.max(node.position.x + node.size.width);
                bottom = bottom.max(node.position.y + node.size.height);
            }
            let padding = 24.0;
            let origin = viewport.world_to_screen(core::Point::new(left - padding, top - padding));
            root = root.child(
                div()
                    .absolute()
                    .left(px(origin.x))
                    .top(px(origin.y))
                    .w(px(viewport.scale_length(right - left + padding * 2.0)))
                    .h(px(viewport.scale_length(bottom - top + padding * 2.0)))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(group.color).opacity(0.75))
                    .bg(rgb(group.color).opacity(0.08))
                    .text_color(rgb(group.color))
                    .text_size(px(11.0))
                    .p_1()
                    .child(group.label.clone()),
            );
        }
        root = root.child(wire_layer);

        let mut node_overlays = Vec::new();
        let mut nodes: Vec<_> = self.graph.nodes.values().cloned().collect();
        nodes.sort_by_cached_key(|node| format!("{:?}", node.id));
        for node in nodes {
            let id = node.id.clone();
            let position = viewport.world_to_screen(node.position);
            let selected = self.graph.selected_nodes.contains(&id);
            let resize_id = id.clone();
            let mut body = if let Some(renderer) = self.node_body_renderer.as_mut() {
                let mut ports: Vec<_> = self
                    .graph
                    .ports
                    .values()
                    .filter(|port| port.node == id)
                    .cloned()
                    .collect();
                ports.sort_by_cached_key(|port| format!("{:?}", port.id));
                renderer.render_node(
                    NodeBodyContext {
                        node: node.clone(),
                        ports: ports.into(),
                        state: NodeVisualState {
                            selected,
                            zoom: viewport.zoom,
                        },
                        theme: self.theme.clone(),
                        graph: cx.weak_entity(),
                    },
                    window,
                    cx,
                )
            } else {
                NodeBody::new(div().child(node.title.clone()))
            };
            node_overlays.extend(body.overlays.drain(..).map(|overlay| {
                (
                    core::Point::new(position.x + overlay.offset.x, position.y + overlay.offset.y),
                    overlay.element,
                )
            }));
            let background = if selected {
                self.theme.node_selected
            } else {
                self.theme.node
            };
            root = root.child(
                div()
                    .absolute()
                    .left(px(position.x))
                    .top(px(position.y))
                    .w(px(viewport.scale_length(node.size.width)))
                    .h(px(viewport.scale_length(node.size.height)))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(0x52525b))
                    .bg(rgb(background))
                    .text_color(rgb(self.theme.text))
                    .text_size(px(viewport.scale_length(14.0).clamp(8.0, 24.0)))
                    .p(px(viewport.scale_length(8.0)))
                    .child(body.element)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.focus(window, cx);
                            let cursor = this
                                .graph
                                .viewport
                                .screen_to_world(this.local_screen(event.position));
                            let before = (
                                this.graph.selected_nodes.clone(),
                                this.graph.selected_connections.clone(),
                            );
                            if event.modifiers.shift {
                                if !this.graph.selected_nodes.remove(&id) {
                                    this.graph.selected_nodes.insert(id.clone());
                                }
                            } else if !this.graph.selected_nodes.contains(&id) {
                                this.graph.selected_nodes.clear();
                                this.graph.selected_connections.clear();
                                this.graph.selected_nodes.insert(id.clone());
                            } else {
                                this.graph.selected_connections.clear();
                            }
                            if before
                                != (
                                    this.graph.selected_nodes.clone(),
                                    this.graph.selected_connections.clone(),
                                )
                            {
                                this.emit_selection(cx);
                            }
                            if this.graph.selected_nodes.contains(&id) {
                                let selected: Vec<_> = this
                                    .graph
                                    .selected_nodes
                                    .iter()
                                    .filter_map(|selected_id| {
                                        let node = this.graph.nodes.get(selected_id)?;
                                        Some((
                                            selected_id.clone(),
                                            cursor - node.position,
                                            node.position,
                                        ))
                                    })
                                    .collect();
                                this.drag = Some(NodeDrag {
                                    offsets: selected
                                        .iter()
                                        .map(|(id, offset, _)| (id.clone(), *offset))
                                        .collect(),
                                    starts: selected
                                        .into_iter()
                                        .map(|(id, _, position)| (id, position))
                                        .collect(),
                                    moved: false,
                                });
                            }
                            cx.notify();
                        }),
                    ),
            );
            root = root.child(
                div()
                    .absolute()
                    .left(px(position.x + viewport.scale_length(node.size.width) - 4.0))
                    .top(px(position.y))
                    .w(px(8.0))
                    .h(px(viewport.scale_length(node.size.height)))
                    .cursor_col_resize()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.focus(window, cx);
                            if event.click_count >= 2 {
                                let width = this
                                    .config
                                    .default_node_width
                                    .clamp(this.config.min_node_width, this.config.max_node_width);
                                if this.resize_node_width(&resize_id, width)
                                    && let Some(node) = this.graph.nodes.get(&resize_id)
                                {
                                    cx.emit(core::GraphEvent::NodeResized {
                                        id: resize_id.clone(),
                                        size: node.size,
                                    });
                                    cx.notify();
                                }
                                return;
                            }
                            let Some(node) = this.graph.nodes.get(&resize_id) else {
                                return;
                            };
                            let start_size = node.size;
                            let start_ports = this
                                .graph
                                .ports
                                .iter()
                                .filter(|(_, port)| port.node == resize_id)
                                .map(|(id, port)| (id.clone(), port.position))
                                .collect();
                            this.drag = None;
                            this.resize = Some(ResizeDrag {
                                id: resize_id.clone(),
                                start_screen_x: this.local_screen(event.position).x,
                                start_size,
                                start_ports,
                                moved: false,
                            });
                        }),
                    ),
            );
        }

        for port in self.graph.ports.values() {
            let id = port.id.clone();
            let position = viewport.world_to_screen(port.position);
            let connected = self
                .graph
                .connections
                .values()
                .any(|connection| connection.source == port.id || connection.target == port.id);
            let is_source = self
                .draft
                .as_ref()
                .is_some_and(|draft| draft.origin == port.id);
            let is_snap = self
                .draft
                .as_ref()
                .and_then(|draft| draft.snap_target.as_ref())
                == Some(&port.id);
            let compatible = self.draft.as_ref().is_some_and(|draft| {
                self.normalized_connection(&draft.origin, &port.id)
                    .is_some()
            });
            let color = if is_source || is_snap || compatible {
                self.theme.port_compatible
            } else if connected {
                self.theme.port_connected
            } else if port.direction == PortDirection::Input {
                self.theme.port_input
            } else {
                self.theme.port_output
            };
            let label_x = if port.direction == PortDirection::Input {
                position.x + 9.0
            } else {
                position.x - 89.0
            };
            root = root
                .child(
                    div()
                        .absolute()
                        .left(px(label_x))
                        .top(px(position.y - 8.0))
                        .w(px(80.0))
                        .text_size(px(11.0))
                        .text_color(rgb(self.theme.text))
                        .when(port.direction == PortDirection::Output, |element| {
                            element.text_right()
                        })
                        .child(port.label.clone()),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(position.x - 7.0))
                        .top(px(position.y - 7.0))
                        .w(px(14.0))
                        .h(px(14.0))
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(self.theme.text))
                        .bg(rgb(color))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener({
                                let id = id.clone();
                                move |this, _: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    window.prevent_default();
                                    this.focus(window, cx);
                                    if this.draft.as_ref().is_some_and(|draft| draft.origin != id)
                                        && this.finish_draft(&id, cx)
                                    {
                                        return;
                                    }
                                    if this.draft.is_none() {
                                        this.start_draft(&id, cx);
                                    }
                                }
                            }),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                cx.stop_propagation();
                                if this.draft.as_ref().is_some_and(|draft| draft.origin != id) {
                                    this.finish_draft(&id, cx);
                                }
                                // The port stops bubbling, so it must also terminate any
                                // node, box, pan, or draft gesture owned by the root.
                                this.finish_left_gesture(cx);
                            }),
                        ),
                );
        }

        if let Some(selection) = &self.box_selection {
            let rect = selection.rect();
            let top_left = viewport.world_to_screen(rect.origin);
            root = root.child(
                div()
                    .absolute()
                    .left(px(top_left.x))
                    .top(px(top_left.y))
                    .w(px(viewport.scale_length(rect.size.width)))
                    .h(px(viewport.scale_length(rect.size.height)))
                    .border_1()
                    .border_color(rgb(self.theme.selection_border))
                    .bg(rgb(self.theme.selection_fill)),
            );
        }

        for (position, overlay) in node_overlays {
            root = root.child(
                div()
                    .absolute()
                    .left(px(position.x))
                    .top(px(position.y))
                    .child(overlay),
            );
        }

        if let Some(menu) = self.catalog_menu.as_ref() {
            let anchor = viewport.world_to_screen(menu.anchor_world);
            let selected = menu.selected;
            let query = menu.query.clone();
            let filtered = self.filtered_catalog_indices();
            let mut menu_element = div()
                .absolute()
                .left(px(anchor.x))
                .top(px(anchor.y))
                .w(px(280.0))
                .max_h(px(360.0))
                .overflow_hidden()
                .rounded_md()
                .border_1()
                .border_color(rgb(0x52525b))
                .bg(rgb(0x202023))
                .text_color(rgb(self.theme.text))
                .p_2()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .pb_2()
                        .text_size(px(12.0))
                        .text_color(rgb(0xa1a1aa))
                        .child(if query.is_empty() {
                            "Create node · type to search".to_string()
                        } else {
                            format!("Search: {query}")
                        }),
                );
            if filtered.is_empty() {
                menu_element = menu_element
                    .child(div().p_2().text_size(px(12.0)).child("No compatible nodes"));
            }
            for (row, item_index) in filtered.into_iter().take(8).enumerate() {
                let item = &self.catalog[item_index];
                let item_id = item_index;
                let subtitle = if item.description.is_empty() {
                    item.category.clone()
                } else {
                    format!("{} · {}", item.category, item.description)
                };
                menu_element = menu_element.child(
                    div()
                        .rounded_sm()
                        .px_2()
                        .py_1()
                        .when(row == selected, |element| element.bg(rgb(0x3f3f46)))
                        .child(div().text_size(px(13.0)).child(item.label.clone()))
                        .child(
                            div()
                                .text_size(px(10.0))
                                .text_color(rgb(0xa1a1aa))
                                .child(subtitle),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.choose_catalog(item_id, cx);
                            }),
                        ),
                );
            }
            root = root.child(menu_element);
        }

        root.on_mouse_down(
            MouseButton::Middle,
            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                this.focus(window, cx);
                this.panning = Some(this.local_screen(event.position));
                cx.stop_propagation();
                window.prevent_default();
            }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                this.focus(window, cx);
                let local = this.local_screen(event.position);
                if event.click_count >= 2 {
                    this.open_catalog(local, None);
                    cx.notify();
                    cx.stop_propagation();
                    window.prevent_default();
                    return;
                }
                this.catalog_menu = None;
                if event.modifiers.control {
                    this.panning = Some(local);
                    this.box_selection = None;
                    cx.stop_propagation();
                    window.prevent_default();
                    return;
                }
                let before = (
                    this.graph.selected_nodes.clone(),
                    this.graph.selected_connections.clone(),
                );
                if let Some(connection) = this.connection_at(local, 7.0) {
                    if event.modifiers.shift {
                        if !this.graph.selected_connections.remove(&connection) {
                            this.graph.selected_connections.insert(connection);
                        }
                    } else {
                        this.graph.selected_nodes.clear();
                        this.graph.selected_connections.clear();
                        this.graph.selected_connections.insert(connection);
                    }
                    this.box_selection = None;
                } else {
                    let start = this.graph.viewport.screen_to_world(local);
                    let (baseline_nodes, baseline_connections) = if event.modifiers.shift {
                        (
                            this.graph.selected_nodes.clone(),
                            this.graph.selected_connections.clone(),
                        )
                    } else {
                        this.graph.selected_nodes.clear();
                        this.graph.selected_connections.clear();
                        (HashSet::new(), HashSet::new())
                    };
                    this.box_selection = Some(BoxSelection {
                        start,
                        current: start,
                        baseline_nodes,
                        baseline_connections,
                    });
                }
                if before
                    != (
                        this.graph.selected_nodes.clone(),
                        this.graph.selected_connections.clone(),
                    )
                {
                    this.emit_selection(cx);
                }
                cx.notify();
                window.prevent_default();
            }),
        )
        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
            let local = this.local_screen(event.position);
            if let Some(previous) = this.panning.as_mut() {
                let old = *previous;
                *previous = local;
                if this.graph.viewport.pan_between(old, local) {
                    cx.emit(core::GraphEvent::ViewportChanged {
                        viewport: this.graph.viewport,
                    });
                    cx.notify();
                }
            }

            if let Some(resize) = this.resize.clone() {
                let delta = (local.x - resize.start_screen_x) / this.graph.viewport.zoom;
                let width = (resize.start_size.width + delta)
                    .clamp(this.config.min_node_width, this.config.max_node_width);
                if this.resize_node_width(&resize.id, width) {
                    if let Some(active) = this.resize.as_mut() {
                        active.moved |= (width - active.start_size.width).abs() > f32::EPSILON;
                    }
                    cx.notify();
                }
            }

            if let Some(drag) = this.drag.clone() {
                let cursor = this.graph.viewport.screen_to_world(local);
                let updates: Vec<_> = drag
                    .offsets
                    .iter()
                    .map(|(id, offset)| {
                        let mut position = cursor - *offset;
                        if let Some(grid) = this.config.grid_size.filter(|grid| *grid > 0.0) {
                            position.x = (position.x / grid).round() * grid;
                            position.y = (position.y / grid).round() * grid;
                        }
                        (id.clone(), position)
                    })
                    .collect();
                if this.graph.move_nodes(&updates).is_some() {
                    if let Some(active) = this.drag.as_mut() {
                        active.moved = true;
                    }
                    cx.notify();
                }
            }

            if let Some(selection) = this.box_selection.as_mut() {
                selection.current = this.graph.viewport.screen_to_world(local);
                let mut nodes = this.graph.nodes_in_rect(selection.rect());
                nodes.extend(selection.baseline_nodes.iter().cloned());
                let before = this.graph.selected_nodes.clone();
                this.graph.selected_nodes = nodes;
                this.graph.selected_connections = selection.baseline_connections.clone();
                if before != this.graph.selected_nodes {
                    this.emit_selection(cx);
                }
                cx.notify();
            }

            if let Some(origin) = this.draft.as_ref().map(|draft| draft.origin.clone()) {
                let snap_target = this.nearest_compatible_port(&origin, local);
                if let Some(draft) = this.draft.as_mut() {
                    if draft.start_screen.distance(local) > 2.0 {
                        draft.moved = true;
                    }
                    draft.current_screen = local;
                    draft.snap_target = snap_target;
                }
                cx.notify();
            }
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| this.finish_left_gesture(cx)),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(|this, _: &MouseUpEvent, _, cx| this.finish_left_gesture(cx)),
        )
        .on_mouse_up(
            MouseButton::Middle,
            cx.listener(|this, _: &MouseUpEvent, _, _| this.panning = None),
        )
        .on_mouse_up_out(
            MouseButton::Middle,
            cx.listener(|this, _: &MouseUpEvent, _, _| this.panning = None),
        )
        .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
            let delta = event.delta.pixel_delta(window.line_height());
            let delta_y = f32::from(delta.y);
            if delta_y.abs() <= f32::EPSILON {
                return;
            }
            let factor = if delta_y < 0.0 {
                this.config.zoom_step.exp()
            } else {
                (-this.config.zoom_step).exp()
            };
            let local = this.local_screen(event.position);
            let previous = this.graph.viewport;
            this.graph
                .viewport
                .zoom_at(local, factor, this.config.min_zoom, this.config.max_zoom);
            if this.graph.viewport != previous {
                cx.emit(core::GraphEvent::ViewportChanged {
                    viewport: this.graph.viewport,
                });
                cx.notify();
            }
            cx.stop_propagation();
            window.prevent_default();
        }))
    }
}

fn distance_to_segment(point: core::Point, start: core::Point, end: core::Point) -> f32 {
    let segment = end - start;
    let length_squared = segment.x * segment.x + segment.y * segment.y;
    if length_squared <= f32::EPSILON {
        return point.distance(start);
    }
    let offset = point - start;
    let t = ((offset.x * segment.x + offset.y * segment.y) / length_squared).clamp(0.0, 1.0);
    point.distance(core::Point::new(
        start.x + segment.x * t,
        start.y + segment.y * t,
    ))
}

fn paint_route(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    points: impl IntoIterator<Item = core::Point>,
    color: gpui::Hsla,
    width: f32,
) {
    let mut points = points.into_iter();
    let Some(first) = points.next() else {
        return;
    };
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(point(
        bounds.origin.x + px(first.x),
        bounds.origin.y + px(first.y),
    ));
    for point_value in points {
        path.line_to(point(
            bounds.origin.x + px(point_value.x),
            bounds.origin.y + px(point_value.y),
        ));
    }
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

fn paint_elbow(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    start: core::Point,
    end: core::Point,
    color: gpui::Hsla,
    width: f32,
) {
    let mid = start.x * 0.5 + end.x * 0.5;
    let mut path = PathBuilder::stroke(px(width));
    path.move_to(point(
        bounds.origin.x + px(start.x),
        bounds.origin.y + px(start.y),
    ));
    path.line_to(point(
        bounds.origin.x + px(mid),
        bounds.origin.y + px(start.y),
    ));
    path.line_to(point(
        bounds.origin.x + px(mid),
        bounds.origin.y + px(end.y),
    ));
    path.line_to(point(
        bounds.origin.x + px(end.x),
        bounds.origin.y + px(end.y),
    ));
    if let Ok(path) = path.build() {
        window.paint_path(path, color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct Kind;
    impl PortType for Kind {
        fn compatible(_: &Self, _: &Self) -> bool {
            true
        }
    }

    #[test]
    fn try_new_rejects_invalid_state_and_new_does_not_hide_it() {
        let mut graph: GraphState<String, String, String, Kind> = GraphState::default();
        graph.viewport.zoom = 0.0;
        assert!(NodeGraph::try_new(graph.clone()).is_err());
        assert!(std::panic::catch_unwind(|| NodeGraph::new(graph)).is_err());
    }

    fn interactive_graph() -> GraphState<String, String, String, Kind> {
        let mut graph = GraphState::default();
        for (id, x) in [("a", 0.0), ("b", 100.0)] {
            graph.nodes.insert(
                id.into(),
                Node {
                    id: id.into(),
                    title: id.into(),
                    position: core::Point::new(x, 0.0),
                    size: core::Size {
                        width: 50.0,
                        height: 50.0,
                    },
                },
            );
        }
        graph.ports.insert(
            "out".into(),
            Port {
                id: "out".into(),
                node: "a".into(),
                label: "Out".into(),
                direction: PortDirection::Output,
                kind: Kind,
                position: core::Point::new(50.0, 25.0),
            },
        );
        graph.ports.insert(
            "in".into(),
            Port {
                id: "in".into(),
                node: "b".into(),
                label: "In".into(),
                direction: PortDirection::Input,
                kind: Kind,
                position: core::Point::new(100.0, 25.0),
            },
        );
        graph.connections.insert(
            "wire".into(),
            Connection {
                id: "wire".into(),
                source: "out".into(),
                target: "in".into(),
            },
        );
        graph
    }

    #[test]
    fn draft_normalizes_direction_and_snaps_in_screen_pixels() {
        let editor = NodeGraph::new(interactive_graph());
        assert_eq!(
            editor.normalized_connection(&"in".into(), &"out".into()),
            Some(("out".into(), "in".into()))
        );
        assert_eq!(
            editor.nearest_compatible_port(&"out".into(), core::Point::new(102.0, 25.0)),
            Some("in".into())
        );
        assert_eq!(
            editor.nearest_compatible_port(&"out".into(), core::Point::new(200.0, 25.0)),
            None
        );
    }

    #[test]
    fn node_width_resize_moves_output_ports_atomically() {
        let mut editor = NodeGraph::new(interactive_graph());
        assert!(editor.resize_node_width(&"a".into(), 80.0));
        assert_eq!(editor.graph.nodes["a"].size.width, 80.0);
        assert_eq!(
            editor.graph.ports["out"].position,
            core::Point::new(80.0, 25.0)
        );
        assert_eq!(
            editor.graph.ports["in"].position,
            core::Point::new(100.0, 25.0)
        );
        let before_node = editor.graph.nodes["a"].clone();
        let before_ports = editor.graph.ports.clone();
        assert!(!editor.resize_node_width(&"a".into(), f32::INFINITY));
        assert_eq!(editor.graph.nodes["a"], before_node);
        assert_eq!(editor.graph.ports, before_ports);
    }

    #[test]
    fn cancelling_a_drag_restores_nodes_and_ports() {
        let mut editor = NodeGraph::new(interactive_graph());
        let starts = vec![("a".to_string(), editor.graph.nodes["a"].position)];
        editor
            .graph
            .move_nodes(&[("a".into(), core::Point::new(40.0, 20.0))])
            .unwrap();
        editor.drag = Some(NodeDrag {
            offsets: Vec::new(),
            starts,
            moved: true,
        });
        editor.cancel_gestures();
        assert_eq!(editor.graph.nodes["a"].position, core::Point::new(0.0, 0.0));
        assert_eq!(
            editor.graph.ports["out"].position,
            core::Point::new(50.0, 25.0)
        );
    }

    #[test]
    fn catalog_filters_search_and_draft_compatibility() {
        let mut editor = NodeGraph::new(interactive_graph()).with_catalog(vec![
            NodeCatalogItem {
                id: "sink".into(),
                label: "Number Sink".into(),
                category: "Output".into(),
                description: "Consumes values".into(),
                keywords: vec!["final".into()],
                ports: vec![CatalogPort {
                    id: "value".into(),
                    label: "Value".into(),
                    direction: PortDirection::Input,
                    kind: Kind,
                }],
            },
            NodeCatalogItem {
                id: "source".into(),
                label: "Source".into(),
                category: "Input".into(),
                description: String::new(),
                keywords: Vec::new(),
                ports: vec![CatalogPort {
                    id: "value".into(),
                    label: "Value".into(),
                    direction: PortDirection::Output,
                    kind: Kind,
                }],
            },
        ]);
        editor.catalog_menu = Some(CatalogMenu {
            anchor_world: core::Point::default(),
            query: "final".into(),
            selected: 0,
            connect_from: Some("out".into()),
        });
        assert_eq!(editor.filtered_catalog_indices(), vec![0]);
        editor.catalog_menu.as_mut().unwrap().query = "source".into();
        assert!(editor.filtered_catalog_indices().is_empty());
    }

    #[test]
    fn subway_connection_route_avoids_node_obstacles() {
        let mut graph = interactive_graph();
        graph.nodes.get_mut("b").unwrap().position = core::Point::new(300.0, 0.0);
        graph.ports.get_mut("in").unwrap().position = core::Point::new(300.0, 25.0);
        graph.nodes.insert(
            "blocker".into(),
            Node {
                id: "blocker".into(),
                title: "blocker".into(),
                position: core::Point::new(150.0, 0.0),
                size: core::Size {
                    width: 50.0,
                    height: 50.0,
                },
            },
        );
        let editor = NodeGraph::new(graph);
        let route = editor
            .connection_route(&editor.graph.connections["wire"])
            .unwrap();
        assert!(
            route
                .iter()
                .any(|point| point.y <= -16.0 || point.y >= 66.0),
            "{route:?}"
        );
        assert_eq!(
            editor.connection_at(core::Point::new(125.0, -16.0), 4.0),
            Some("wire".into())
        );
    }

    #[test]
    fn connection_hit_testing_and_fit_use_render_coordinates() {
        let mut editor = NodeGraph::new(interactive_graph());
        assert_eq!(
            editor.connection_at(core::Point::new(75.0, 27.0), 3.0),
            Some("wire".into())
        );
        editor.canvas_bounds.set(Bounds {
            origin: point(px(20.0), px(30.0)),
            size: gpui::size(px(500.0), px(300.0)),
        });
        assert!(editor.fit_view());
        assert!(editor.graph.viewport.is_valid());
        assert!(editor.graph.viewport.zoom <= editor.config.fit_max_zoom);
    }

    #[test]
    fn box_selection_rect_normalizes_drag_direction() {
        let selection = BoxSelection::<String, String> {
            start: core::Point::new(20.0, 30.0),
            current: core::Point::new(5.0, 10.0),
            baseline_nodes: HashSet::new(),
            baseline_connections: HashSet::new(),
        };
        assert_eq!(
            selection.rect(),
            core::Rect {
                origin: core::Point::new(5.0, 10.0),
                size: core::Size {
                    width: 15.0,
                    height: 20.0,
                },
            }
        );
    }

    #[test]
    fn segment_hit_distance_handles_degenerate_and_projected_points() {
        let start = core::Point::new(0.0, 0.0);
        let end = core::Point::new(10.0, 0.0);
        assert_eq!(
            distance_to_segment(core::Point::new(4.0, 3.0), start, end),
            3.0
        );
        assert_eq!(
            distance_to_segment(core::Point::new(4.0, 3.0), start, start),
            5.0
        );
    }
}
