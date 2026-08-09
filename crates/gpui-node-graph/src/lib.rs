mod windows;

use gpui::{
    AnyElement, App, Bounds, Context, DispatchPhase, Element, ElementId, FocusHandle,
    GlobalElementId, InspectorElementId, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PathBuilder, Pixels, Render, ScrollWheelEvent, WeakEntity,
    Window, canvas, div, point, prelude::*, px, rgb,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::Arc,
};

pub use node_graph_core as core;
pub mod style;
pub use node_graph_core::*;
pub use style::GraphStyle;
pub use windows::*;

/// The GPUI adapter and framework-free core now expose one event vocabulary.
pub type EditorEvent<N = String, P = String, C = String> = core::GraphEvent<N, P, C>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PortPresentation {
    #[default]
    DefaultOverlay,
    BodyAnchors,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DanglingConnection<P, C> {
    pub id: C,
    pub source: P,
    pub target: P,
    pub missing_port: P,
    pub source_position: core::Point,
    pub target_position: core::Point,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderGeometry<N: Eq + std::hash::Hash, P: Eq + std::hash::Hash> {
    node_sizes: HashMap<N, core::Size>,
    port_offsets: HashMap<P, (N, core::Point)>,
}

impl<N: Eq + std::hash::Hash, P: Eq + std::hash::Hash> Default for RenderGeometry<N, P> {
    fn default() -> Self {
        Self {
            node_sizes: HashMap::new(),
            port_offsets: HashMap::new(),
        }
    }
}

impl<N: Eq + std::hash::Hash, P: Eq + std::hash::Hash> RenderGeometry<N, P> {
    pub fn node_size(&self, id: &N) -> Option<core::Size> {
        self.node_sizes.get(id).copied()
    }

    pub fn port_offset(&self, id: &P) -> Option<(&N, core::Point)> {
        self.port_offsets
            .get(id)
            .map(|(node, offset)| (node, *offset))
    }

    pub fn ports(&self) -> impl Iterator<Item = (&P, &N, core::Point)> {
        self.port_offsets
            .iter()
            .map(|(id, (node, offset))| (id, node, *offset))
    }
}

pub struct NodeOverlay {
    /// Node-relative world offset transformed with the viewport. Overlay content itself is
    /// not scaled, so retained controls keep normal GPUI hit testing while their anchor follows
    /// the node during pan and zoom.
    pub offset: core::Point,
    pub element: AnyElement,
    pub behavior: Option<OverlayBehavior>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayBehavior {
    pub id: String,
    pub estimated_size: core::Size,
    pub flip_horizontal: bool,
    pub clamp_to_canvas: bool,
    pub dismiss_on_escape: bool,
}

impl NodeOverlay {
    pub fn new(offset: core::Point, element: impl IntoElement) -> Self {
        Self {
            offset,
            element: element.into_any_element(),
            behavior: None,
        }
    }

    pub fn adaptive(mut self, id: impl Into<String>, estimated_size: core::Size) -> Self {
        self.behavior = Some(OverlayBehavior {
            id: id.into(),
            estimated_size,
            flip_horizontal: true,
            clamp_to_canvas: true,
            dismiss_on_escape: true,
        });
        self
    }

    pub fn with_behavior(mut self, behavior: OverlayBehavior) -> Self {
        self.behavior = Some(behavior);
        self
    }
}

pub struct NodeBody {
    pub element: AnyElement,
    pub overlays: Vec<NodeOverlay>,
    pub ports: PortPresentation,
}

impl NodeBody {
    pub fn new(element: impl IntoElement) -> Self {
        Self {
            element: element.into_any_element(),
            overlays: Vec::new(),
            ports: PortPresentation::DefaultOverlay,
        }
    }

    pub fn with_overlay(mut self, overlay: NodeOverlay) -> Self {
        self.overlays.push(overlay);
        self
    }

    pub fn with_ports(mut self, ports: PortPresentation) -> Self {
        self.ports = ports;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeVisualState {
    pub selected: bool,
    /// Whether the node intersects the current canvas plus the configured margin.
    /// Renderers may use this to skip expensive body content; the shell remains mounted.
    pub visible: bool,
    pub zoom: f32,
}

pub struct NodeBodyContext<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> {
    pub node: Node<N>,
    pub ports: Arc<[Port<N, P, T>]>,
    pub state: NodeVisualState,
    pub theme: Theme,
    graph: WeakEntity<NodeGraph<T, N, P, C>>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    viewport: Viewport,
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>
    NodeBodyContext<T, N, P, C>
{
    pub fn graph(&self) -> WeakEntity<NodeGraph<T, N, P, C>> {
        self.graph.clone()
    }

    pub fn port_anchor(&self, id: P, child: impl IntoElement) -> AnyElement {
        let graph_down = self.graph.clone();
        let graph_up = self.graph.clone();
        let down_id = id.clone();
        let up_id = id.clone();
        let interactive = div()
            .child(child)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                let _ = graph_down.update(cx, |editor, cx| editor.start_draft(&down_id, cx));
            })
            .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                cx.stop_propagation();
                let _ = graph_up.update(cx, |editor, cx| {
                    if editor
                        .draft
                        .as_ref()
                        .is_some_and(|draft| draft.origin != up_id)
                    {
                        editor.finish_draft(&up_id, cx);
                    }
                    editor.finish_left_gesture(cx);
                });
            });
        let graph = self.graph.clone();
        let canvas_bounds = self.canvas_bounds.clone();
        let viewport = self.viewport;
        let node_id = self.node.id.clone();
        let node_position = self.node.position;
        MeasuredElement::new(interactive, move |bounds, cx| {
            let canvas = canvas_bounds.get();
            let center = core::Point::new(
                f32::from(bounds.origin.x - canvas.origin.x + bounds.size.width * 0.5),
                f32::from(bounds.origin.y - canvas.origin.y + bounds.size.height * 0.5),
            );
            let world = viewport.screen_to_world(center);
            let offset = world - node_position;
            let port_id = id.clone();
            let owner = node_id.clone();
            let graph = graph.clone();
            cx.defer(move |cx| {
                let _ = graph.update(cx, |editor, cx| {
                    let changed = editor
                        .render_geometry
                        .port_offsets
                        .get(&port_id)
                        .is_none_or(|(current_owner, current)| {
                            current_owner != &owner || current.distance(offset) > 0.01
                        });
                    if changed {
                        editor
                            .render_geometry
                            .port_offsets
                            .insert(port_id, (owner, offset));
                        cx.notify();
                    }
                });
            });
        })
        .into_any_element()
    }

    /// Wrap a body control so graph shortcuts, zoom and drag gestures do not leak through it.
    pub fn isolated_control(&self, child: impl IntoElement) -> AnyElement {
        div()
            .child(child)
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
            })
            .on_mouse_down(MouseButton::Middle, |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
            })
            .on_mouse_down(MouseButton::Right, |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
            })
            .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
            .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
            .on_key_down(|_, _, cx| cx.stop_propagation())
            .into_any_element()
    }

    /// Isolate a control that consumes wheel input itself, such as a scrollable list or dial.
    pub fn isolated_scroll_control(&self, child: impl IntoElement) -> AnyElement {
        div()
            .child(self.isolated_control(child))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .into_any_element()
    }

    pub fn default_port_anchor(&self, id: P) -> AnyElement {
        let port = self.ports.iter().find(|port| port.id == id);
        let color = port.map_or(self.theme.port_connected, |port| {
            if port.direction == PortDirection::Input {
                self.theme.port_input
            } else {
                self.theme.port_output
            }
        });
        let diameter = self.viewport.scale_length(8.0);
        self.port_anchor(
            id,
            div()
                .w(px(diameter))
                .h(px(diameter))
                .rounded_full()
                .border_1()
                .border_color(rgb(self.theme.text))
                .bg(rgb(color)),
        )
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
    Bezier,
    Subway(core::subway::SubwayOptions),
}

impl Default for RoutingMode {
    fn default() -> Self {
        Self::Subway(core::subway::SubwayOptions::default())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MutationMode {
    /// Gesture previews become the editor's committed graph state.
    #[default]
    Uncontrolled,
    /// Gesture previews roll back and emit one atomic `MutationRequested` transaction.
    Controlled,
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
    /// Screen-pixel margin used by [`NodeVisualState::visible`].
    pub visibility_margin: f32,
    /// World-space separation between deterministic routes that share a source pin.
    pub route_lane_spacing: f32,
    /// Screen-pixel radius used to round orthogonal route corners.
    pub route_corner_radius: f32,
    pub mutation_mode: MutationMode,
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
            visibility_margin: 160.0,
            route_lane_spacing: 8.0,
            route_corner_radius: 7.0,
            mutation_mode: MutationMode::Uncontrolled,
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
            node: 0x111111,
            node_selected: 0x111111,
            wire: 0x71717a,
            wire_selected: 0xdddddd,
            wire_draft: 0x22d3ee,
            text: 0xd4d4d8,
            port_input: 0x71717a,
            port_output: 0x71717a,
            port_connected: 0xa1a1aa,
            port_compatible: 0x22d3ee,
            selection_border: 0xffffff,
            selection_fill: 0xffffff,
        }
    }
}

#[derive(Clone)]
struct NodeDrag<N> {
    offsets: Vec<(N, core::Point)>,
    starts: Vec<(N, core::Point)>,
    moved: bool,
    alter_groups: bool,
}

#[derive(Clone)]
struct GroupEditor {
    id: String,
    query: String,
}
struct DragCompletion<N> {
    nodes: Vec<(N, core::Point)>,
    group_changes: Vec<(String, Vec<N>)>,
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
struct NodeScaleElement {
    child: Option<AnyElement>,
    rem_size: Pixels,
}

impl NodeScaleElement {
    fn new(child: AnyElement, zoom: f32) -> Self {
        Self {
            child: Some(child),
            rem_size: px(16.0 * zoom),
        }
    }
}

impl Element for NodeScaleElement {
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
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut child = self.child.take().expect("scaled element laid out once");
        let layout_id = window.with_rem_size(Some(self.rem_size), |window| {
            child.request_layout(window, cx)
        });
        (layout_id, child)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_rem_size(Some(self.rem_size), |window| child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.with_rem_size(Some(self.rem_size), |window| child.paint(window, cx));
    }
}

impl IntoElement for NodeScaleElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

type BoundsCallback = Box<dyn FnMut(Bounds<Pixels>, &mut App)>;

struct MeasuredElement {
    child: Option<AnyElement>,
    on_bounds: BoundsCallback,
}

impl MeasuredElement {
    fn new(
        child: impl IntoElement,
        on_bounds: impl FnMut(Bounds<Pixels>, &mut App) + 'static,
    ) -> Self {
        Self {
            child: Some(child.into_any_element()),
            on_bounds: Box::new(on_bounds),
        }
    }
}

impl Element for MeasuredElement {
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
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut child = self.child.take().expect("measured element laid out once");
        let layout_id = child.request_layout(window, cx);
        (layout_id, child)
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.prepaint(window, cx);
        (self.on_bounds)(bounds, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        child: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        child.paint(window, cx);
    }
}

impl IntoElement for MeasuredElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
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
struct RouteCache<C: Eq + std::hash::Hash> {
    fingerprint: String,
    routes: HashMap<C, Vec<core::Point>>,
    generation: u64,
}

impl<C: Eq + std::hash::Hash> Default for RouteCache<C> {
    fn default() -> Self {
        Self {
            fingerprint: String::new(),
            routes: HashMap::new(),
            generation: 0,
        }
    }
}

pub struct NodeGraph<
    T: PortType,
    N: core::NodeId = String,
    P: core::PortId = String,
    C: core::ConnectionId = String,
> {
    pub graph: GraphState<N, P, C, T>,
    pub theme: Theme,
    /// Complete visual configuration. `theme` remains as a compatibility color facade while
    /// rendering migrates to the strongly typed Leptos-parity style vocabulary.
    pub style: GraphStyle,
    pub config: EditorConfig,
    drag: Option<NodeDrag<N>>,
    resize: Option<ResizeDrag<N, P>>,
    panning: Option<core::Point>,
    draft: Option<DraftConnection<P, C>>,
    catalog: Vec<NodeCatalogItem<T>>,
    catalog_menu: Option<CatalogMenu<P>>,
    node_body_renderer: Option<Box<dyn NodeBodyRenderer<T, N, P, C>>>,
    groups: Vec<GraphGroup<N>>,
    render_geometry: RenderGeometry<N, P>,
    dangling_connections: Vec<DanglingConnection<P, C>>,
    dismissed_overlays: HashSet<String>,
    active_dismissible_overlays: HashSet<String>,
    route_cache: RefCell<RouteCache<C>>,
    group_editor: Option<GroupEditor>,
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
            style: GraphStyle::default(),
            config: EditorConfig::default(),
            drag: None,
            resize: None,
            panning: None,
            draft: None,
            catalog: Vec::new(),
            catalog_menu: None,
            node_body_renderer: None,
            groups: Vec::new(),
            render_geometry: RenderGeometry::default(),
            dangling_connections: Vec::new(),
            dismissed_overlays: HashSet::new(),
            active_dismissible_overlays: HashSet::new(),
            route_cache: RefCell::new(RouteCache::default()),
            group_editor: None,
            box_selection: None,
            focus_handle: None,
            canvas_bounds: Rc::new(Cell::new(Bounds::default())),
        })
    }

    pub fn focus_handle(&self) -> Option<&FocusHandle> {
        self.focus_handle.as_ref()
    }

    pub fn with_style(mut self, style: GraphStyle) -> Self {
        self.style = style;
        self
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

    pub fn render_geometry(&self) -> &RenderGeometry<N, P> {
        &self.render_geometry
    }

    pub fn resolved_node_size(&self, id: &N) -> Option<core::Size> {
        // Shell dimensions are model-owned and change only through explicit resize/dynamic-port
        // mutations. Feeding child measurements made under a zoomed constraint back into layout
        // causes recursive shrinking and zoom-dependent wrapping. Measurements remain available
        // through `render_geometry()` but never resize the shell implicitly.
        self.graph.nodes.get(id).map(|node| node.size)
    }

    pub fn resolved_port_position(&self, id: &P) -> Option<core::Point> {
        if let Some((owner, offset)) = self.render_geometry.port_offsets.get(id)
            && let Some(node) = self.graph.nodes.get(owner)
        {
            return Some(node.position + *offset);
        }
        self.graph.ports.get(id).map(|port| port.position)
    }

    pub fn invalidate_render_geometry(&mut self, cx: &mut Context<Self>) {
        self.render_geometry = RenderGeometry::default();
        cx.notify();
    }

    pub fn dangling_connections(&self) -> &[DanglingConnection<P, C>] {
        &self.dangling_connections
    }

    /// Remove a dynamic port without weakening snapshot validation. Connections touching the
    /// port are removed from the strict graph and retained only as transient visual tombstones.
    pub fn remove_port_with_tombstones(&mut self, id: &P, cx: &mut Context<Self>) -> bool {
        let Some(removed_connections) = self.remove_port_to_tombstones(id) else {
            return false;
        };
        for id in removed_connections {
            cx.emit(core::GraphEvent::ConnectionRemoved { id });
        }
        cx.notify();
        true
    }

    fn remove_port_to_tombstones(&mut self, id: &P) -> Option<Vec<C>> {
        if !self.graph.ports.contains_key(id) {
            return None;
        }
        let mut affected: Vec<_> = self
            .graph
            .connections
            .values()
            .filter(|connection| connection.source == *id || connection.target == *id)
            .cloned()
            .collect();
        affected.sort_by_cached_key(|connection| format!("{:?}", connection.id));
        let mut removed_connections = Vec::with_capacity(affected.len());
        for connection in affected {
            let Some(source_position) = self.resolved_port_position(&connection.source) else {
                continue;
            };
            let Some(target_position) = self.resolved_port_position(&connection.target) else {
                continue;
            };
            self.graph.connections.remove(&connection.id);
            self.graph.selected_connections.remove(&connection.id);
            self.dangling_connections
                .retain(|tombstone| tombstone.id != connection.id);
            self.dangling_connections.push(DanglingConnection {
                id: connection.id.clone(),
                source: connection.source,
                target: connection.target,
                missing_port: id.clone(),
                source_position,
                target_position,
            });
            removed_connections.push(connection.id);
        }
        self.graph.ports.remove(id);
        self.render_geometry.port_offsets.remove(id);
        Some(removed_connections)
    }

    /// Restore strict connections whose previously missing dynamic port is available again.
    pub fn restore_tombstoned_connections(&mut self, port_id: &P, cx: &mut Context<Self>) -> usize {
        let restored = self.restore_tombstones_for_port(port_id);
        for id in &restored {
            cx.emit(core::GraphEvent::DanglingConnectionRestored { id: id.clone() });
        }
        if !restored.is_empty() {
            cx.notify();
        }
        restored.len()
    }

    fn restore_tombstones_for_port(&mut self, port_id: &P) -> Vec<C> {
        let mut restored = Vec::new();
        self.dangling_connections.retain(|connection| {
            if &connection.missing_port != port_id
                || self.graph.connections.contains_key(&connection.id)
            {
                return true;
            }
            let Some(source) = self.graph.ports.get(&connection.source) else {
                return true;
            };
            let Some(target) = self.graph.ports.get(&connection.target) else {
                return true;
            };
            if source.direction != PortDirection::Output
                || target.direction != PortDirection::Input
                || source.node == target.node
                || !T::compatible(&source.kind, &target.kind)
            {
                return true;
            }
            self.graph.connections.insert(
                connection.id.clone(),
                Connection {
                    id: connection.id.clone(),
                    source: connection.source.clone(),
                    target: connection.target.clone(),
                },
            );
            restored.push(connection.id.clone());
            false
        });
        restored
    }

    pub fn clear_dangling_connections(&mut self, cx: &mut Context<Self>) {
        if !self.dangling_connections.is_empty() {
            self.dangling_connections.clear();
            cx.notify();
        }
    }

    pub fn dismiss_overlay(&mut self, id: impl Into<String>, cx: &mut Context<Self>) {
        if self.dismissed_overlays.insert(id.into()) {
            cx.notify();
        }
    }

    pub fn is_overlay_dismissed(&self, id: &str) -> bool {
        self.dismissed_overlays.contains(id)
    }

    pub fn catalog_is_open(&self) -> bool {
        self.catalog_menu.is_some()
    }

    pub fn reopen_overlay(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.dismissed_overlays.remove(id) {
            cx.notify();
        }
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
        self.render_geometry
            .node_sizes
            .retain(|node, _| self.graph.nodes.contains_key(node));
        self.render_geometry.port_offsets.retain(|port, (node, _)| {
            self.graph.ports.contains_key(port) && self.graph.nodes.contains_key(node)
        });
        self.dangling_connections
            .retain(|connection| !self.graph.ports.contains_key(&connection.missing_port));
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
        self.render_geometry
            .node_sizes
            .retain(|node, _| self.graph.nodes.contains_key(node));
        self.render_geometry.port_offsets.retain(|port, (node, _)| {
            self.graph.ports.contains_key(port) && self.graph.nodes.contains_key(node)
        });
        self.dangling_connections
            .retain(|connection| !self.graph.ports.contains_key(&connection.missing_port));
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
                .world_to_screen(self.resolved_port_position(id)?);
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
        let current_screen = self.graph.viewport.world_to_screen(
            self.resolved_port_position(&origin)
                .unwrap_or(self.graph.ports[&origin].position),
        );
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
        if self.config.mutation_mode == MutationMode::Controlled {
            let mut mutations = Vec::new();
            if let Some(id) = draft.replaced_connection {
                mutations.push(core::GraphMutation::RemoveConnection { id });
            }
            mutations.push(core::GraphMutation::RequestConnection { source, target });
            cx.emit(core::GraphEvent::MutationRequested { mutations });
        } else {
            if let Some(id) = draft.replaced_connection {
                self.graph.connections.remove(&id);
                self.graph.selected_connections.remove(&id);
                cx.emit(core::GraphEvent::ConnectionRemoved { id });
            }
            cx.emit(core::GraphEvent::ConnectionRequested { source, target });
        }
        cx.notify();
        true
    }

    fn node_is_visible(&self, node: &Node<N>) -> bool {
        let canvas = self.canvas_bounds.get();
        let width = f32::from(canvas.size.width);
        let height = f32::from(canvas.size.height);
        if width <= 0.0 || height <= 0.0 {
            return true;
        }
        let viewport = self.graph.viewport.sanitized();
        let origin = viewport.world_to_screen(node.position);
        let node_width = viewport.scale_length(node.size.width);
        let node_height = viewport.scale_length(node.size.height);
        let margin = self.config.visibility_margin.max(0.0);
        origin.x + node_width >= -margin
            && origin.y + node_height >= -margin
            && origin.x <= width + margin
            && origin.y <= height + margin
    }

    fn connection_route(&self, connection: &Connection<P, C>) -> Option<Vec<core::Point>> {
        let source = self.graph.ports.get(&connection.source)?;
        let target = self.graph.ports.get(&connection.target)?;
        let start = self.resolved_port_position(&connection.source)?;
        let end = self.resolved_port_position(&connection.target)?;
        match self.config.routing {
            RoutingMode::SimpleOrthogonal => Some(core::orthogonal_route(start, end)),
            RoutingMode::Bezier => {
                let control_distance = ((end.x - start.x).abs() * 0.5).max(40.0);
                let control_a = core::Point::new(start.x + control_distance, start.y);
                let control_b = core::Point::new(end.x - control_distance, end.y);
                Some(
                    (0..=24)
                        .map(|step| {
                            let t = step as f32 / 24.0;
                            let inverse = 1.0 - t;
                            core::Point::new(
                                inverse.powi(3) * start.x
                                    + 3.0 * inverse.powi(2) * t * control_a.x
                                    + 3.0 * inverse * t.powi(2) * control_b.x
                                    + t.powi(3) * end.x,
                                inverse.powi(3) * start.y
                                    + 3.0 * inverse.powi(2) * t * control_a.y
                                    + 3.0 * inverse * t.powi(2) * control_b.y
                                    + t.powi(3) * end.y,
                            )
                        })
                        .collect(),
                )
            }
            RoutingMode::Subway(options) => {
                let mut nodes: Vec<_> = self.graph.nodes.values().collect();
                nodes.sort_by_cached_key(|node| format!("{:?}", node.id));
                let obstacles: Vec<_> = nodes
                    .iter()
                    .map(|node| core::Rect {
                        origin: node.position,
                        size: self.resolved_node_size(&node.id).unwrap_or(node.size),
                    })
                    .collect();
                let start_obstacle = nodes.iter().position(|node| node.id == source.node);
                let end_obstacle = nodes.iter().position(|node| node.id == target.node);
                Some(
                    core::subway::compute_subway_route(
                        &obstacles,
                        core::subway::SubwayConnection {
                            start,
                            end,
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

    fn connection_routes(&self) -> HashMap<C, Vec<core::Point>> {
        let fingerprint = format!(
            "{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
            self.config.routing,
            self.config.route_lane_spacing,
            self.graph.nodes,
            self.graph.ports,
            self.graph.connections,
            self.render_geometry.port_offsets,
        );
        if self.route_cache.borrow().fingerprint == fingerprint {
            return self.route_cache.borrow().routes.clone();
        }

        let mut connections: Vec<_> = self.graph.connections.values().collect();
        connections.sort_by_cached_key(|connection| format!("{:?}", connection.id));
        let mut source_lanes: HashMap<P, Vec<C>> = HashMap::new();
        for connection in &connections {
            source_lanes
                .entry(connection.source.clone())
                .or_default()
                .push(connection.id.clone());
        }
        for ids in source_lanes.values_mut() {
            ids.sort_by_cached_key(|id| format!("{id:?}"));
        }

        let mut routes = HashMap::new();
        for connection in connections {
            let Some(mut route) = self.connection_route(connection) else {
                continue;
            };
            let ids = &source_lanes[&connection.source];
            let index = ids
                .iter()
                .position(|id| id == &connection.id)
                .unwrap_or_default();
            let lane = (index as f32 - (ids.len().saturating_sub(1) as f32 * 0.5))
                * self.config.route_lane_spacing;
            apply_route_lane(&mut route, lane, self.config.routing);
            routes.insert(connection.id.clone(), route);
        }

        let mut cache = self.route_cache.borrow_mut();
        cache.fingerprint = fingerprint;
        cache.routes = routes.clone();
        cache.generation = cache.generation.saturating_add(1);
        routes
    }

    pub fn route_cache_generation(&self) -> u64 {
        self.route_cache.borrow().generation
    }

    fn connection_at(&self, cursor: core::Point, radius: f32) -> Option<C> {
        let viewport = self.graph.viewport.sanitized();
        let mut connections: Vec<_> = self.graph.connections.iter().collect();
        connections.sort_by_cached_key(|(id, _)| format!("{id:?}"));
        let routes = self.connection_routes();
        let mut nearest: Option<(C, f32)> = None;
        for (id, _) in connections {
            let Some(route) = routes.get(id) else {
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

    fn group_label_at(&self, screen: core::Point) -> Option<String> {
        let viewport = self.graph.viewport.sanitized();
        for group in &self.groups {
            let mut members = group.nodes.iter().filter_map(|id| self.graph.nodes.get(id));
            let Some(first) = members.next() else {
                continue;
            };
            let mut left = first.position.x;
            let mut top = first.position.y;
            for node in members {
                left = left.min(node.position.x);
                top = top.min(node.position.y);
            }
            let origin = viewport.world_to_screen(core::Point::new(left - 24.0, top - 24.0));
            let width = (group.label.chars().count() as f32 * 7.0 + 12.0).max(48.0);
            if screen.x >= origin.x
                && screen.x <= origin.x + width
                && screen.y >= origin.y
                && screen.y <= origin.y + 18.0
            {
                return Some(group.id.clone());
            }
        }
        None
    }

    fn render_bounds(&self) -> Option<core::Rect> {
        let mut nodes = self.graph.nodes.values();
        let first = nodes.next()?;
        let first_size = first.size;
        let (mut left, mut top, mut right, mut bottom) = (
            first.position.x,
            first.position.y,
            first.position.x + first_size.width,
            first.position.y + first_size.height,
        );
        for node in nodes {
            let size = node.size;
            left = left.min(node.position.x);
            top = top.min(node.position.y);
            right = right.max(node.position.x + size.width);
            bottom = bottom.max(node.position.y + size.height);
        }
        Some(core::Rect {
            origin: core::Point::new(left, top),
            size: core::Size {
                width: right - left,
                height: bottom - top,
            },
        })
    }

    fn nodes_in_render_rect(&self, rect: core::Rect) -> HashSet<N> {
        self.graph
            .nodes
            .values()
            .filter(|node| {
                core::Rect {
                    origin: node.position,
                    size: self.resolved_node_size(&node.id).unwrap_or(node.size),
                }
                .intersects(&rect)
            })
            .map(|node| node.id.clone())
            .collect()
    }

    fn fit_view(&mut self) -> bool {
        let Some(bounds) = self.render_bounds() else {
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
        let mut connection_ids: Vec<_> = self.graph.selected_connections.iter().cloned().collect();
        connection_ids.sort_by_cached_key(|id| format!("{id:?}"));
        let mut node_ids: Vec<_> = self.graph.selected_nodes.iter().cloned().collect();
        node_ids.sort_by_cached_key(|id| format!("{id:?}"));
        if self.config.mutation_mode == MutationMode::Controlled {
            let mut mutations: Vec<_> = connection_ids
                .into_iter()
                .map(|id| core::GraphMutation::RemoveConnection { id })
                .collect();
            if !node_ids.is_empty() {
                mutations.push(core::GraphMutation::DeleteNodes { ids: node_ids });
            }
            if !mutations.is_empty() {
                cx.emit(core::GraphEvent::MutationRequested { mutations });
            }
        } else {
            for id in connection_ids {
                if self.graph.connections.remove(&id).is_some() {
                    cx.emit(core::GraphEvent::ConnectionRemoved { id });
                }
            }
            for event in self.graph.remove_nodes(&node_ids) {
                cx.emit(event);
            }
        }
        self.graph.selected_nodes.clear();
        self.graph.selected_connections.clear();
        self.emit_selection(cx);
        cx.notify();
    }

    fn update_group_memberships(&mut self, dragged: &[N]) -> Vec<(String, Vec<N>)> {
        let dragged_set: HashSet<_> = dragged.iter().cloned().collect();
        let mut changes = Vec::new();
        for group in &mut self.groups {
            let mut members = group
                .nodes
                .iter()
                .filter(|id| !dragged_set.contains(*id))
                .filter_map(|id| self.graph.nodes.get(id));
            let Some(first) = members.next() else {
                continue;
            };
            let mut left = first.position.x;
            let mut top = first.position.y;
            let first_size = first.size;
            let mut right = first.position.x + first_size.width;
            let mut bottom = first.position.y + first_size.height;
            for node in members {
                let size = node.size;
                left = left.min(node.position.x);
                top = top.min(node.position.y);
                right = right.max(node.position.x + size.width);
                bottom = bottom.max(node.position.y + size.height);
            }
            let padding = 24.0;
            let mut changed = false;
            for id in dragged {
                let Some(node) = self.graph.nodes.get(id) else {
                    continue;
                };
                let center = core::Point::new(
                    node.position.x + node.size.width * 0.5,
                    node.position.y + node.size.height * 0.5,
                );
                let inside = center.x >= left - padding
                    && center.x <= right + padding
                    && center.y >= top - padding
                    && center.y <= bottom + padding;
                changed |= if inside {
                    group.nodes.insert(id.clone())
                } else {
                    group.nodes.remove(id)
                };
            }
            if changed {
                let mut node_ids: Vec<_> = group.nodes.iter().cloned().collect();
                node_ids.sort_by_cached_key(|id| format!("{id:?}"));
                changes.push((group.id.clone(), node_ids));
            }
        }
        changes
    }

    fn finalize_node_drag(&mut self, drag: &NodeDrag<N>) -> DragCompletion<N> {
        let dragged: Vec<_> = drag.offsets.iter().map(|(id, _)| id.clone()).collect();
        let nodes: Vec<_> = drag
            .offsets
            .iter()
            .filter_map(|(id, _)| Some((id.clone(), self.graph.nodes.get(id)?.position)))
            .collect();
        let previous_groups = self.groups.clone();
        let group_changes = if drag.alter_groups {
            self.update_group_memberships(&dragged)
        } else {
            Vec::new()
        };
        if self.config.mutation_mode == MutationMode::Controlled {
            let _ = self.graph.move_nodes(&drag.starts);
            self.groups = previous_groups;
        }
        DragCompletion {
            nodes,
            group_changes,
        }
    }

    fn handle_pointer_move(&mut self, local: core::Point, cx: &mut Context<Self>) {
        if let Some(previous) = self.panning.as_mut() {
            let old = *previous;
            *previous = local;
            if self.graph.viewport.pan_between(old, local) {
                cx.emit(core::GraphEvent::ViewportChanged {
                    viewport: self.graph.viewport,
                });
                cx.notify();
            }
        }

        if let Some(resize) = self.resize.clone() {
            let delta = (local.x - resize.start_screen_x) / self.graph.viewport.zoom;
            let width = (resize.start_size.width + delta)
                .clamp(self.config.min_node_width, self.config.max_node_width);
            if self.resize_node_width(&resize.id, width) {
                if let Some(active) = self.resize.as_mut() {
                    active.moved |= (width - active.start_size.width).abs() > f32::EPSILON;
                }
                cx.notify();
            }
        }

        if let Some(drag) = self.drag.clone() {
            let cursor = self.graph.viewport.screen_to_world(local);
            let updates: Vec<_> = drag
                .offsets
                .iter()
                .map(|(id, offset)| {
                    let mut position = cursor - *offset;
                    if let Some(grid) = self.config.grid_size.filter(|grid| *grid > 0.0) {
                        position.x = (position.x / grid).round() * grid;
                        position.y = (position.y / grid).round() * grid;
                    }
                    (id.clone(), position)
                })
                .collect();
            if self.graph.move_nodes(&updates).is_some() {
                if let Some(active) = self.drag.as_mut() {
                    active.moved = true;
                }
                cx.notify();
            }
        }

        let selection_state = self.box_selection.as_mut().map(|selection| {
            selection.current = self.graph.viewport.screen_to_world(local);
            (
                selection.rect(),
                selection.baseline_nodes.clone(),
                selection.baseline_connections.clone(),
            )
        });
        if let Some((rect, baseline_nodes, baseline_connections)) = selection_state {
            let mut nodes = self.nodes_in_render_rect(rect);
            nodes.extend(baseline_nodes);
            let before = self.graph.selected_nodes.clone();
            self.graph.selected_nodes = nodes;
            self.graph.selected_connections = baseline_connections;
            if before != self.graph.selected_nodes {
                self.emit_selection(cx);
            }
            cx.notify();
        }

        if let Some(origin) = self.draft.as_ref().map(|draft| draft.origin.clone()) {
            let snap_target = self.nearest_compatible_port(&origin, local);
            if let Some(draft) = self.draft.as_mut() {
                if draft.start_screen.distance(local) > 2.0 {
                    draft.moved = true;
                }
                draft.current_screen = local;
                draft.snap_target = snap_target;
            }
            cx.notify();
        }
    }

    fn finish_left_gesture(&mut self, cx: &mut Context<Self>) {
        self.panning = None;
        if let Some(resize) = self.resize.take().filter(|resize| resize.moved)
            && let Some(size) = self.graph.nodes.get(&resize.id).map(|node| node.size)
        {
            if self.config.mutation_mode == MutationMode::Controlled {
                if let Some(node) = self.graph.nodes.get_mut(&resize.id) {
                    node.size = resize.start_size;
                }
                for (id, position) in resize.start_ports {
                    if let Some(port) = self.graph.ports.get_mut(&id) {
                        port.position = position;
                    }
                }
                cx.emit(core::GraphEvent::MutationRequested {
                    mutations: vec![core::GraphMutation::ResizeNode {
                        id: resize.id,
                        size,
                    }],
                });
            } else {
                cx.emit(core::GraphEvent::NodeResized {
                    id: resize.id,
                    size,
                });
            }
        }
        if let Some(drag) = self.drag.take().filter(|drag| drag.moved) {
            let completion = self.finalize_node_drag(&drag);
            if self.config.mutation_mode == MutationMode::Controlled {
                let mut mutations = vec![core::GraphMutation::MoveNodes {
                    nodes: completion.nodes,
                }];
                mutations.extend(completion.group_changes.into_iter().map(
                    |(group_id, node_ids)| core::GraphMutation::SetGroupMembership {
                        group_id,
                        node_ids,
                    },
                ));
                cx.emit(core::GraphEvent::MutationRequested { mutations });
            } else {
                cx.emit(core::GraphEvent::NodesMoved {
                    nodes: completion.nodes,
                });
                for (group_id, node_ids) in completion.group_changes {
                    cx.emit(core::GraphEvent::GroupMembershipChanged { group_id, node_ids });
                }
            }
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

    fn selected_group_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self
            .groups
            .iter()
            .filter(|group| {
                group
                    .nodes
                    .iter()
                    .any(|id| self.graph.selected_nodes.contains(id))
            })
            .map(|group| group.id.clone())
            .collect();
        ids.sort();
        ids
    }

    fn ungroup_selection(&mut self, cx: &mut Context<Self>) {
        let group_ids = self.selected_group_ids();
        if group_ids.is_empty() {
            return;
        }
        if self.config.mutation_mode == MutationMode::Controlled {
            cx.emit(core::GraphEvent::MutationRequested {
                mutations: vec![core::GraphMutation::RemoveGroups { group_ids }],
            });
        } else {
            self.groups.retain(|group| !group_ids.contains(&group.id));
            cx.emit(core::GraphEvent::GroupsRemoved { group_ids });
        }
        cx.notify();
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
        if self.group_editor.is_some() {
            match key {
                "escape" => self.group_editor = None,
                "enter" => {
                    if let Some(editor) = self.group_editor.take()
                        && !editor.query.trim().is_empty()
                    {
                        let label = editor.query.trim().to_string();
                        if self.config.mutation_mode == MutationMode::Controlled {
                            cx.emit(core::GraphEvent::MutationRequested {
                                mutations: vec![core::GraphMutation::SetGroupLabel {
                                    group_id: editor.id,
                                    label,
                                }],
                            });
                        } else if let Some(group) =
                            self.groups.iter_mut().find(|group| group.id == editor.id)
                        {
                            group.label = label;
                            cx.emit(core::GraphEvent::GroupLabelChanged {
                                group_id: group.id.clone(),
                                label: group.label.clone(),
                            });
                        }
                    }
                }
                "backspace" => {
                    if let Some(editor) = self.group_editor.as_mut() {
                        editor.query.pop();
                    }
                }
                _ if !command => {
                    if let Some(character) = event.keystroke.key_char.as_ref()
                        && !character.chars().any(char::is_control)
                        && let Some(editor) = self.group_editor.as_mut()
                    {
                        editor.query.push_str(character);
                    }
                }
                _ => {}
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
            "r" if !command => {
                self.config.routing = match self.config.routing {
                    RoutingMode::Subway(_) => RoutingMode::Bezier,
                    RoutingMode::Bezier => RoutingMode::SimpleOrthogonal,
                    RoutingMode::SimpleOrthogonal => {
                        RoutingMode::Subway(core::subway::SubwayOptions::default())
                    }
                };
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
                if !self.active_dismissible_overlays.is_empty() {
                    self.dismissed_overlays
                        .extend(self.active_dismissible_overlays.drain());
                } else {
                    self.cancel_gestures();
                    self.graph.selected_nodes.clear();
                    self.graph.selected_connections.clear();
                    self.emit_selection(cx);
                }
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
            "g" if command && shift => {
                self.ungroup_selection(cx);
                cx.stop_propagation();
                window.prevent_default();
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

        let routes = self.connection_routes();
        let mut wires: Vec<_> = self
            .graph
            .connections
            .values()
            .filter_map(|connection| {
                Some((
                    routes.get(&connection.id)?.clone(),
                    self.graph.selected_connections.contains(&connection.id),
                    false,
                ))
            })
            .collect();
        wires.extend(self.dangling_connections.iter().map(|connection| {
            let source = self
                .resolved_port_position(&connection.source)
                .unwrap_or(connection.source_position);
            let target = self
                .resolved_port_position(&connection.target)
                .unwrap_or(connection.target_position);
            (core::orthogonal_route(source, target), false, true)
        }));
        let draft = self.draft.as_ref().and_then(|draft| {
            let source = self.resolved_port_position(&draft.origin)?;
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
        let dangling_color: gpui::Hsla = rgb(0xef4444).into();
        let canvas_bounds = self.canvas_bounds.clone();
        let captured_graph = cx.weak_entity();
        let corner_radius = if self.config.routing == RoutingMode::Bezier {
            0.0
        } else {
            self.config.route_corner_radius
        };
        let wire_layer = canvas(
            move |bounds, _, _| {
                canvas_bounds.set(bounds);
            },
            move |bounds, _, window, _| {
                let graph = captured_graph.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let graph = graph.clone();
                    let _ = graph.update(cx, |editor, cx| {
                        let local = editor.local_screen(event.position);
                        let canvas = editor.canvas_bounds.get();
                        let outside = local.x < 0.0
                            || local.y < 0.0
                            || local.x > f32::from(canvas.size.width)
                            || local.y > f32::from(canvas.size.height);
                        if outside {
                            editor.handle_pointer_move(local, cx);
                        }
                    });
                });
                let graph = captured_graph.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture || event.button != MouseButton::Left {
                        return;
                    }
                    let graph = graph.clone();
                    let _ = graph.update(cx, |editor, cx| {
                        let local = editor.local_screen(event.position);
                        let canvas = editor.canvas_bounds.get();
                        let outside = local.x < 0.0
                            || local.y < 0.0
                            || local.x > f32::from(canvas.size.width)
                            || local.y > f32::from(canvas.size.height);
                        if outside {
                            editor.finish_left_gesture(cx);
                        }
                    });
                });
                for (route, selected, dangling) in &wires {
                    paint_route(
                        window,
                        bounds,
                        route.iter().map(|point| viewport.world_to_screen(*point)),
                        if *dangling {
                            dangling_color
                        } else if *selected {
                            selected_wire_color
                        } else {
                            wire_color
                        },
                        if *selected || *dangling { 3.0 } else { 2.0 },
                        corner_radius,
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

        let mut group_labels = Vec::new();
        for group in &self.groups {
            let group_label = self
                .group_editor
                .as_ref()
                .filter(|editor| editor.id == group.id)
                .map_or_else(
                    || group.label.clone(),
                    |editor| format!("{}▏", editor.query),
                );
            let mut members = group.nodes.iter().filter_map(|id| self.graph.nodes.get(id));
            let Some(first) = members.next() else {
                continue;
            };
            let mut left = first.position.x;
            let mut top = first.position.y;
            let first_size = self.resolved_node_size(&first.id).unwrap_or(first.size);
            let mut right = first.position.x + first_size.width;
            let mut bottom = first.position.y + first_size.height;
            for node in members {
                let size = self.resolved_node_size(&node.id).unwrap_or(node.size);
                left = left.min(node.position.x);
                top = top.min(node.position.y);
                right = right.max(node.position.x + size.width);
                bottom = bottom.max(node.position.y + size.height);
            }
            let horizontal_padding = 16.0;
            let top_padding = 40.0;
            let bottom_padding = 16.0;
            let origin = viewport.world_to_screen(core::Point::new(
                left - horizontal_padding,
                top - top_padding,
            ));
            root = root.child(
                div()
                    .absolute()
                    .left(px(origin.x))
                    .top(px(origin.y))
                    .w(px(
                        viewport.scale_length(right - left + horizontal_padding * 2.0)
                    ))
                    .h(px(
                        viewport.scale_length(bottom - top + top_padding + bottom_padding)
                    ))
                    .rounded(px(viewport.scale_length(8.0)))
                    .border(px(viewport.scale_length(1.0)))
                    .border_dashed()
                    .border_color(rgb(group.color).opacity(0.5))
                    .bg(rgb(group.color).opacity(0.1)),
            );
            group_labels.push((origin, group.id.clone(), group_label, group.color));
        }
        root = root.child(wire_layer);
        for (origin, group_id, label, color) in group_labels {
            root = root.child(
                div()
                    .absolute()
                    .left(px(origin.x + viewport.scale_length(10.0)))
                    .top(px(origin.y + viewport.scale_length(6.0)))
                    .text_color(rgb(color))
                    .text_size(px(viewport.scale_length(10.0)))
                    .child(label.to_uppercase())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            if event.click_count >= 2
                                && let Some(group) =
                                    this.groups.iter().find(|group| group.id == group_id)
                            {
                                this.group_editor = Some(GroupEditor {
                                    id: group.id.clone(),
                                    query: group.label.clone(),
                                });
                                cx.notify();
                                cx.stop_propagation();
                                window.prevent_default();
                            }
                        }),
                    ),
            );
        }

        let mut node_overlays = Vec::new();
        self.active_dismissible_overlays.clear();
        let mut body_anchored_nodes = HashSet::new();
        let mut nodes: Vec<_> = self.graph.nodes.values().cloned().collect();
        nodes.sort_by_cached_key(|node| format!("{:?}", node.id));
        for mut node in nodes {
            let id = node.id.clone();
            let model_size = node.size;
            if let Some(size) = self.resolved_node_size(&id) {
                node.size = size;
            }
            let position = viewport.world_to_screen(node.position);
            let selected = self.graph.selected_nodes.contains(&id);
            let visible = self.node_is_visible(&node);
            let resize_id = id.clone();
            let has_custom_body = self.node_body_renderer.is_some();
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
                            visible,
                            zoom: viewport.zoom,
                        },
                        theme: self.theme.clone(),
                        graph: cx.weak_entity(),
                        canvas_bounds: self.canvas_bounds.clone(),
                        viewport,
                    },
                    window,
                    cx,
                )
            } else {
                NodeBody::new(div().child(node.title.clone()))
            };
            if body.ports == PortPresentation::BodyAnchors {
                body_anchored_nodes.insert(id.clone());
            }
            for overlay in body.overlays.drain(..) {
                let screen_offset = overlay_screen_offset(overlay.offset, viewport);
                let mut overlay_position = position + screen_offset;
                if let Some(behavior) = &overlay.behavior {
                    if self.dismissed_overlays.contains(&behavior.id) {
                        continue;
                    }
                    if behavior.dismiss_on_escape {
                        self.active_dismissible_overlays.insert(behavior.id.clone());
                    }
                    let canvas = self.canvas_bounds.get();
                    overlay_position = resolve_overlay_position(
                        position,
                        viewport.scale_length(node.size.width),
                        screen_offset,
                        behavior,
                        core::Size {
                            width: f32::from(canvas.size.width),
                            height: f32::from(canvas.size.height),
                        },
                    );
                }
                node_overlays.push((overlay_position, overlay.element));
            }
            let graph = cx.weak_entity();
            let measured_node = id.clone();
            let raw_body_element = body.element;
            let body_element = if has_custom_body {
                MeasuredElement::new(
                    NodeScaleElement::new(raw_body_element, viewport.zoom),
                    move |bounds, cx| {
                        let measured = core::Size {
                            width: model_size.width,
                            height: f32::from(bounds.size.height) / viewport.zoom + 16.0,
                        };
                        if !measured.width.is_finite()
                            || !measured.height.is_finite()
                            || measured.width <= 0.0
                            || measured.height <= 0.0
                        {
                            return;
                        }
                        let graph = graph.clone();
                        let node_id = measured_node.clone();
                        cx.defer(move |cx| {
                            let _ = graph.update(cx, |editor, cx| {
                                let changed =
                                    editor.render_geometry.node_sizes.get(&node_id).is_none_or(
                                        |current| {
                                            (current.width - measured.width).abs() > 0.1
                                                || (current.height - measured.height).abs() > 0.1
                                        },
                                    );
                                if changed {
                                    editor.render_geometry.node_sizes.insert(node_id, measured);
                                    cx.notify();
                                }
                            });
                        });
                    },
                )
                .into_any_element()
            } else {
                raw_body_element
            };
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
                    .h(px(viewport.scale_length(model_size.height)))
                    .rounded(px(viewport.scale_length(2.0)))
                    .overflow_hidden()
                    .when(selected, |element| {
                        element.border_1().border_color(rgb(0xff0000))
                    })
                    .bg(rgb(background))
                    .text_color(rgb(self.theme.text))
                    .text_size(px(viewport.scale_length(13.0)))
                    .child(body_element)
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
                                    alter_groups: event.modifiers.alt,
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
                                let previous_width =
                                    this.graph.nodes.get(&resize_id).map(|node| node.size.width);
                                if this.resize_node_width(&resize_id, width)
                                    && let Some(size) =
                                        this.graph.nodes.get(&resize_id).map(|node| node.size)
                                {
                                    if this.config.mutation_mode == MutationMode::Controlled {
                                        if let Some(previous_width) = previous_width {
                                            let _ =
                                                this.resize_node_width(&resize_id, previous_width);
                                        }
                                        cx.emit(core::GraphEvent::MutationRequested {
                                            mutations: vec![core::GraphMutation::ResizeNode {
                                                id: resize_id.clone(),
                                                size,
                                            }],
                                        });
                                    } else {
                                        cx.emit(core::GraphEvent::NodeResized {
                                            id: resize_id.clone(),
                                            size,
                                        });
                                    }
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
            if body_anchored_nodes.contains(&port.node) {
                continue;
            }
            let id = port.id.clone();
            let position = viewport.world_to_screen(
                self.resolved_port_position(&port.id)
                    .unwrap_or(port.position),
            );
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
                    .border_color(rgb(self.theme.selection_border).opacity(0.1))
                    .bg(rgb(self.theme.selection_fill).opacity(0.025)),
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
                    if let Some(group_id) = this.group_label_at(local)
                        && let Some(group) = this.groups.iter().find(|group| group.id == group_id)
                    {
                        this.group_editor = Some(GroupEditor {
                            id: group.id.clone(),
                            query: group.label.clone(),
                        });
                    } else {
                        this.open_catalog(local, None);
                    }
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
            this.handle_pointer_move(local, cx);
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

fn apply_route_lane(route: &mut Vec<core::Point>, lane: f32, mode: RoutingMode) {
    if lane.abs() <= f32::EPSILON || route.len() < 3 {
        return;
    }
    if mode == RoutingMode::Bezier {
        let last = route.len() - 1;
        for (index, point) in route.iter_mut().enumerate().skip(1).take(last - 1) {
            let t = index as f32 / last as f32;
            point.y += lane * (std::f32::consts::PI * t).sin();
        }
        return;
    }

    let original = route.clone();
    let start = original[0];
    let end = *original.last().expect("route has at least three points");
    let first = original[1];
    let last = original[original.len() - 2];
    let mut separated = Vec::with_capacity(original.len() + 4);
    separated.push(start);
    separated.push(core::Point::new(first.x, start.y));
    for point in original.iter().take(original.len() - 1).skip(1) {
        separated.push(core::Point::new(point.x, point.y + lane));
    }
    separated.push(core::Point::new(last.x, end.y));
    separated.push(end);
    separated.dedup();
    *route = separated;
}

fn overlay_screen_offset(offset: core::Point, viewport: Viewport) -> core::Point {
    core::Point::new(
        viewport.scale_length(offset.x),
        viewport.scale_length(offset.y),
    )
}

fn resolve_overlay_position(
    node_origin: core::Point,
    node_width: f32,
    offset: core::Point,
    behavior: &OverlayBehavior,
    canvas_size: core::Size,
) -> core::Point {
    let mut position = node_origin + offset;
    if behavior.flip_horizontal
        && canvas_size.width > 0.0
        && position.x + behavior.estimated_size.width > canvas_size.width
    {
        let gap = (offset.x - node_width).max(0.0);
        position.x = node_origin.x - behavior.estimated_size.width - gap;
    }
    if behavior.clamp_to_canvas && canvas_size.width > 0.0 && canvas_size.height > 0.0 {
        position.x = position.x.clamp(
            0.0,
            (canvas_size.width - behavior.estimated_size.width).max(0.0),
        );
        position.y = position.y.clamp(
            0.0,
            (canvas_size.height - behavior.estimated_size.height).max(0.0),
        );
    }
    position
}

fn paint_route(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    points: impl IntoIterator<Item = core::Point>,
    color: gpui::Hsla,
    width: f32,
    corner_radius: f32,
) {
    let points: Vec<_> = points.into_iter().collect();
    let Some(first) = points.first() else {
        return;
    };
    let mut path = PathBuilder::stroke(px(width));
    let absolute =
        |value: core::Point| point(bounds.origin.x + px(value.x), bounds.origin.y + px(value.y));
    path.move_to(absolute(*first));
    if corner_radius <= 0.0 || points.len() < 3 {
        for value in points.iter().skip(1) {
            path.line_to(absolute(*value));
        }
    } else {
        for index in 1..points.len() - 1 {
            let previous = points[index - 1];
            let corner = points[index];
            let next = points[index + 1];
            let incoming = corner - previous;
            let outgoing = next - corner;
            let incoming_length = (incoming.x * incoming.x + incoming.y * incoming.y).sqrt();
            let outgoing_length = (outgoing.x * outgoing.x + outgoing.y * outgoing.y).sqrt();
            if incoming_length <= f32::EPSILON || outgoing_length <= f32::EPSILON {
                continue;
            }
            let radius = corner_radius
                .min(incoming_length * 0.5)
                .min(outgoing_length * 0.5);
            let entry = core::Point::new(
                corner.x - incoming.x / incoming_length * radius,
                corner.y - incoming.y / incoming_length * radius,
            );
            let exit = core::Point::new(
                corner.x + outgoing.x / outgoing_length * radius,
                corner.y + outgoing.y / outgoing_length * radius,
            );
            path.line_to(absolute(entry));
            path.curve_to(absolute(exit), absolute(corner));
        }
        if let Some(last) = points.last() {
            path.line_to(absolute(*last));
        }
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
    fn controlled_drag_reports_final_positions_and_rolls_preview_back() {
        let mut editor = NodeGraph::new(interactive_graph());
        editor.config.mutation_mode = MutationMode::Controlled;
        editor
            .graph
            .move_node(&String::from("a"), core::Point::new(80.0, 0.0));
        let completion = editor.finalize_node_drag(&NodeDrag {
            offsets: vec![(String::from("a"), core::Point::new(0.0, 0.0))],
            starts: vec![(String::from("a"), core::Point::new(0.0, 0.0))],
            moved: true,
            alter_groups: false,
        });
        assert_eq!(
            completion.nodes,
            vec![(String::from("a"), core::Point::new(80.0, 0.0))]
        );
        assert_eq!(editor.graph.nodes["a"].position, core::Point::new(0.0, 0.0));
        assert_eq!(
            editor.graph.ports["out"].position,
            core::Point::new(50.0, 25.0)
        );
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
            alter_groups: false,
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
    fn selected_groups_are_deterministic_for_ungroup_transactions() {
        let mut editor = NodeGraph::new(interactive_graph()).with_groups(vec![GraphGroup {
            id: "group".into(),
            label: "Group".into(),
            color: 0,
            nodes: [String::from("a"), String::from("b")].into_iter().collect(),
        }]);
        assert!(editor.selected_group_ids().is_empty());
        editor.graph.selected_nodes.insert("a".into());
        assert_eq!(editor.selected_group_ids(), vec![String::from("group")]);
    }

    #[test]
    fn group_label_hit_area_matches_rendered_group_origin() {
        let editor = NodeGraph::new(interactive_graph()).with_groups(vec![GraphGroup {
            id: "group".into(),
            label: "Group".into(),
            color: 0,
            nodes: [String::from("b")].into_iter().collect(),
        }]);
        assert_eq!(
            editor.group_label_at(core::Point::new(80.0, -10.0)),
            Some(String::from("group"))
        );
        assert_eq!(editor.group_label_at(core::Point::new(20.0, 20.0)), None);
    }

    #[test]
    fn alt_drag_membership_adds_inside_and_removes_outside() {
        let mut editor = NodeGraph::new(interactive_graph()).with_groups(vec![GraphGroup {
            id: "group".into(),
            label: "Group".into(),
            color: 0,
            nodes: [String::from("b")].into_iter().collect(),
        }]);
        editor.graph.nodes.get_mut("a").unwrap().position = core::Point::new(80.0, 0.0);
        let changes = editor.update_group_memberships(&[String::from("a")]);
        assert_eq!(changes.len(), 1);
        assert!(editor.groups[0].nodes.contains("a"));
        editor.graph.nodes.get_mut("a").unwrap().position = core::Point::new(0.0, 0.0);
        editor.update_group_memberships(&[String::from("a")]);
        assert!(!editor.groups[0].nodes.contains("a"));
    }

    #[test]
    fn route_batch_separates_shared_pins_and_reuses_stable_cache() {
        let mut graph = interactive_graph();
        graph.ports.insert(
            "in2".into(),
            Port {
                id: "in2".into(),
                node: "b".into(),
                label: "In 2".into(),
                direction: PortDirection::Input,
                kind: Kind,
                position: core::Point::new(100.0, 40.0),
            },
        );
        graph.connections.insert(
            "wire2".into(),
            Connection {
                id: "wire2".into(),
                source: "out".into(),
                target: "in2".into(),
            },
        );
        let mut editor = NodeGraph::new(graph);
        editor.config.routing = RoutingMode::SimpleOrthogonal;
        let routes = editor.connection_routes();
        assert_ne!(routes["wire"], routes["wire2"]);
        assert_eq!(routes["wire"].first(), Some(&core::Point::new(50.0, 25.0)));
        assert_eq!(editor.route_cache_generation(), 1);
        assert_eq!(routes, editor.connection_routes());
        assert_eq!(editor.route_cache_generation(), 1);
        editor.graph.nodes.get_mut("b").unwrap().position.x += 10.0;
        editor.connection_routes();
        assert_eq!(editor.route_cache_generation(), 2);
    }

    #[test]
    fn bezier_route_is_stable_and_keeps_endpoints() {
        let mut editor = NodeGraph::new(interactive_graph());
        editor.config.routing = RoutingMode::Bezier;
        let route = editor
            .connection_route(&editor.graph.connections["wire"])
            .unwrap();
        assert_eq!(route.len(), 25);
        assert_eq!(route.first(), Some(&core::Point::new(50.0, 25.0)));
        assert_eq!(route.last(), Some(&core::Point::new(100.0, 25.0)));
        assert_eq!(
            route,
            editor
                .connection_route(&editor.graph.connections["wire"])
                .unwrap()
        );
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
    fn overlay_anchor_offset_scales_with_viewport_without_scaling_content() {
        let viewport = Viewport {
            pan: core::Point::new(25.0, 30.0),
            zoom: 2.0,
        };
        assert_eq!(
            overlay_screen_offset(core::Point::new(192.0, 20.0), viewport),
            core::Point::new(384.0, 40.0)
        );
    }

    #[test]
    fn adaptive_overlay_flips_and_clamps_to_canvas() {
        let behavior = OverlayBehavior {
            id: "menu".into(),
            estimated_size: core::Size {
                width: 100.0,
                height: 50.0,
            },
            flip_horizontal: true,
            clamp_to_canvas: true,
            dismiss_on_escape: true,
        };
        assert_eq!(
            resolve_overlay_position(
                core::Point::new(250.0, 280.0),
                50.0,
                core::Point::new(60.0, 0.0),
                &behavior,
                core::Size {
                    width: 300.0,
                    height: 300.0,
                },
            ),
            core::Point::new(140.0, 250.0)
        );
    }

    #[test]
    fn dynamic_port_removal_uses_transient_tombstones_and_keeps_graph_strict() {
        let mut editor = NodeGraph::new(interactive_graph());
        let removed = editor.remove_port_to_tombstones(&"out".into()).unwrap();
        assert_eq!(removed, vec![String::from("wire")]);
        assert!(!editor.graph.ports.contains_key("out"));
        assert!(editor.graph.connections.is_empty());
        editor.graph.validate().unwrap();
        assert_eq!(editor.dangling_connections.len(), 1);
        assert_eq!(editor.dangling_connections[0].missing_port, "out");
        assert_eq!(
            editor.dangling_connections[0].source_position,
            core::Point::new(50.0, 25.0)
        );
        editor.graph.ports.insert(
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
        assert_eq!(
            editor.restore_tombstones_for_port(&"out".into()),
            vec![String::from("wire")]
        );
        assert!(editor.dangling_connections.is_empty());
        assert!(editor.graph.connections.contains_key("wire"));
        editor.graph.validate().unwrap();
    }

    #[test]
    fn measured_body_size_never_mutates_model_owned_shell_geometry() {
        let mut editor = NodeGraph::new(interactive_graph());
        editor.render_geometry.node_sizes.insert(
            "a".into(),
            core::Size {
                width: 1.0,
                height: 500.0,
            },
        );
        assert_eq!(
            editor.render_geometry().node_size(&"a".into()),
            Some(core::Size {
                width: 1.0,
                height: 500.0,
            })
        );
        assert_eq!(
            editor.resolved_node_size(&"a".into()),
            Some(core::Size {
                width: 50.0,
                height: 50.0,
            })
        );
        assert_eq!(editor.render_bounds().unwrap().size.height, 50.0);
    }

    #[test]
    fn measured_port_offsets_follow_node_movement() {
        let mut editor = NodeGraph::new(interactive_graph());
        editor
            .render_geometry
            .port_offsets
            .insert("out".into(), ("a".into(), core::Point::new(45.0, 18.0)));
        assert_eq!(
            editor.resolved_port_position(&"out".into()),
            Some(core::Point::new(45.0, 18.0))
        );
        editor
            .graph
            .move_node(&"a".into(), core::Point::new(20.0, 10.0));
        assert_eq!(
            editor.resolved_port_position(&"out".into()),
            Some(core::Point::new(65.0, 28.0))
        );
    }

    #[test]
    fn visibility_hook_uses_canvas_and_screen_margin() {
        let mut editor = NodeGraph::new(interactive_graph());
        editor.config.visibility_margin = 0.0;
        editor.canvas_bounds.set(Bounds {
            origin: point(px(20.0), px(30.0)),
            size: gpui::size(px(100.0), px(100.0)),
        });
        assert!(editor.node_is_visible(&editor.graph.nodes["a"]));
        editor.graph.nodes.get_mut("a").unwrap().position = core::Point::new(500.0, 500.0);
        assert!(!editor.node_is_visible(&editor.graph.nodes["a"]));
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
