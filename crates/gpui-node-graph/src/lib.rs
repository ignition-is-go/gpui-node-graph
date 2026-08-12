mod windows;

use gpui::{
    AnyElement, App, BorderStyle, Bounds, BoxShadow, Context, DispatchPhase, Element, ElementId,
    ElementInputHandler, EntityInputHandler, FocusHandle, FontWeight, Global, GlobalElementId,
    InspectorElementId, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, PathBuilder, Pixels, Render, ScrollHandle, ScrollWheelEvent, ShapedLine,
    SharedString, TextAlign, TextRun, UTF16Selection, WeakEntity, Window, canvas, div, point,
    prelude::*, px, quad, rgb,
};
use std::{
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    ops::Range,
    rc::Rc,
    sync::Arc,
};

pub use node_graph_core as core;
pub mod layout;
pub mod style;
pub mod world;
pub use node_graph_core::*;
pub use style::NodeGraphTheme;

struct GlobalNodeGraphTheme(Arc<NodeGraphTheme>);
impl Global for GlobalNodeGraphTheme {}

/// Install or replace the complete application-global node graph theme.
///
/// This does not refresh windows. Downstream applications can replace several
/// ambient themes and then call `App::refresh_windows` exactly once.
pub fn set_node_graph_theme(cx: &mut App, theme: impl Into<Arc<NodeGraphTheme>>) {
    cx.set_global(GlobalNodeGraphTheme(theme.into()));
}

/// Access to the required application-wide node graph theme.
pub trait ActiveNodeGraphTheme {
    fn node_graph_theme(&self) -> &Arc<NodeGraphTheme>;
}

impl ActiveNodeGraphTheme for App {
    fn node_graph_theme(&self) -> &Arc<NodeGraphTheme> {
        &self.global::<GlobalNodeGraphTheme>().0
    }
}
pub use windows::*;

/// The GPUI adapter and framework-free core now expose one event vocabulary.
pub type EditorEvent<N = String, P = String, C = String, T = ()> = core::GraphEvent<N, P, C, T>;

/// Typed payload for a node dragged from consumer-owned chrome into an editor.
///
/// Attach this value to the palette element with GPUI's `on_drag`, then catch it on
/// any common ancestor with `on_drop::<NodeDrop>`. GPUI's pinned drop callback does
/// not include the pointer position; read `window.mouse_position()` there and pass it
/// to [`EditorHandle::drop_node`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NodeDrop {
    pub item_id: String,
}

impl NodeDrop {
    pub fn new(item_id: impl Into<String>) -> Self {
        Self {
            item_id: item_id.into(),
        }
    }
}

/// One action in an anchor's right-click menu.
#[derive(Clone)]
pub enum AnchorMenuAction {
    RemoveConnections,
    RemoveBrokenConnections,
    Custom(Rc<dyn Fn()>),
}

impl std::fmt::Debug for AnchorMenuAction {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RemoveConnections => formatter.write_str("RemoveConnections"),
            Self::RemoveBrokenConnections => formatter.write_str("RemoveBrokenConnections"),
            Self::Custom(_) => formatter.write_str("Custom(..)"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct AnchorMenuItem {
    pub label: String,
    pub action: AnchorMenuAction,
    pub enabled: bool,
}

impl AnchorMenuItem {
    pub fn action(label: impl Into<String>, on_select: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            action: AnchorMenuAction::Custom(Rc::new(on_select)),
            enabled: true,
        }
    }
}

type AnchorMenuFn<P> = Rc<dyn Fn(&P, PortDirection) -> Vec<AnchorMenuItem>>;

#[derive(Clone)]
pub struct AnchorMenuBuilder<P: core::PortId>(AnchorMenuFn<P>);

impl<P: core::PortId> AnchorMenuBuilder<P> {
    pub fn new(build: impl Fn(&P, PortDirection) -> Vec<AnchorMenuItem> + 'static) -> Self {
        Self(Rc::new(build))
    }

    pub fn build(&self, port: &P, direction: PortDirection) -> Vec<AnchorMenuItem> {
        (self.0)(port, direction)
    }
}

#[derive(Clone, Debug)]
struct ActiveAnchorMenu<P> {
    port: P,
    position: core::Point,
    items: Vec<AnchorMenuItem>,
}

/// Consumer-side bridge to a retained [`NodeGraph`] entity.
///
/// GPUI entities are context-bound rather than reactive signals, so reads take an
/// [`App`] and writes take `&mut App`. The handle is weak: putting one in a toolbar or
/// sibling pane does not keep a closed editor alive.
#[derive(Clone, Debug)]
pub struct EditorHandle<
    T: PortType,
    N: core::NodeId = String,
    P: core::PortId = String,
    C: core::ConnectionId = String,
> {
    editor: WeakEntity<NodeGraph<T, N, P, C>>,
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>
    EditorHandle<T, N, P, C>
{
    pub fn new(editor: &gpui::Entity<NodeGraph<T, N, P, C>>) -> Self {
        Self {
            editor: editor.downgrade(),
        }
    }

    /// Read the live viewport, or `None` after the editor entity is released.
    pub fn viewport(&self, cx: &App) -> Option<core::Viewport> {
        self.editor
            .read_with(cx, |editor, _| editor.graph.viewport)
            .ok()
    }

    /// Convert window/client pixels into canvas/world coordinates.
    ///
    /// Returns `None` before first layout or after the editor is released.
    pub fn client_to_canvas(&self, client: core::Point, cx: &App) -> Option<core::Point> {
        self.editor
            .read_with(cx, |editor, _| editor.client_to_canvas(client))
            .ok()
            .flatten()
    }

    /// Convert canvas/world coordinates into window/client pixels.
    pub fn canvas_to_client(&self, canvas: core::Point, cx: &App) -> Option<core::Point> {
        self.editor
            .read_with(cx, |editor, _| editor.canvas_to_client(canvas))
            .ok()
            .flatten()
    }

    /// Apply a consumer-owned viewport change and emit the normal viewport event.
    /// Returns false for a released editor or a no-op change.
    pub fn set_viewport(&self, viewport: core::Viewport, cx: &mut App) -> bool {
        self.editor
            .update(cx, |editor, cx| editor.set_external_viewport(viewport, cx))
            .unwrap_or(false)
    }

    /// Turn a typed cross-pane drop into the same `CreateNode` request used by the
    /// built-in catalog. Returns false before layout or for an unknown catalog item.
    pub fn drop_node(&self, payload: &NodeDrop, client: core::Point, cx: &mut App) -> bool {
        self.editor
            .update(cx, |editor, cx| {
                editor.request_create_node_at_client(&payload.item_id, client, cx)
            })
            .unwrap_or(false)
    }
}

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
    /// Additional unscaled pane-space offset, matching CSS overlay gaps.
    pub screen_offset: core::Point,
    pub element: AnyElement,
    pub behavior: Option<OverlayBehavior>,
    pub on_dismiss: Option<Rc<dyn Fn()>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayBehavior {
    pub id: String,
    pub estimated_size: core::Size,
    pub flip_horizontal: bool,
    pub clamp_to_canvas: bool,
    pub dismiss_on_escape: bool,
    pub dismiss_on_outside_click: bool,
    pub show_backdrop: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlaySide {
    Top,
    #[default]
    Right,
    Bottom,
    Left,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OverlayAlign {
    #[default]
    Start,
    Center,
    End,
}

/// Selector-style overlay placement configured independently from the public
/// overlay DTO, preserving compatibility with existing `NodeOverlay` literals.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OverlayPlacement {
    pub side: OverlaySide,
    pub align: OverlayAlign,
    /// World-space size of the retained trigger/anchor rectangle.
    pub anchor_size: core::Size,
    /// Unscaled pane-space gap between the projected anchor and panel.
    pub gap: f32,
    pub flip: bool,
    pub clamp_to_canvas: bool,
}

impl Default for OverlayPlacement {
    fn default() -> Self {
        Self {
            side: OverlaySide::Right,
            align: OverlayAlign::Start,
            anchor_size: core::Size {
                width: 0.0,
                height: 0.0,
            },
            gap: 8.0,
            flip: true,
            clamp_to_canvas: true,
        }
    }
}

impl NodeOverlay {
    pub fn new(offset: core::Point, element: impl IntoElement) -> Self {
        Self {
            offset,
            screen_offset: core::Point::new(0.0, 0.0),
            element: element.into_any_element(),
            behavior: None,
            on_dismiss: None,
        }
    }

    pub fn with_screen_offset(mut self, offset: core::Point) -> Self {
        self.screen_offset = offset;
        self
    }

    pub fn adaptive(mut self, id: impl Into<String>, estimated_size: core::Size) -> Self {
        self.behavior = Some(OverlayBehavior {
            id: id.into(),
            estimated_size,
            flip_horizontal: true,
            clamp_to_canvas: true,
            dismiss_on_escape: true,
            dismiss_on_outside_click: true,
            show_backdrop: true,
        });
        self
    }

    pub fn with_behavior(mut self, behavior: OverlayBehavior) -> Self {
        self.behavior = Some(behavior);
        self
    }

    pub fn on_dismiss(mut self, callback: impl Fn() + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(callback));
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
/// Viewport-independent state exposed to immutable world display-list renderers.
/// Deliberately omits zoom and pan so world layout cannot branch on projection state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorldNodeVisualState {
    pub selected: bool,
    pub visible: bool,
}

pub struct NodeBodyContext<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> {
    pub node: Node<N>,
    pub ports: Arc<[Port<N, P, T>]>,
    pub port_states: Arc<HashMap<P, WorldPortVisualState>>,
    pub port_presentations: Arc<HashMap<P, AnchorPresentation>>,
    pub state: NodeVisualState,
    pub theme: Arc<NodeGraphTheme>,
    graph: WeakEntity<NodeGraph<T, N, P, C>>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    viewport: Viewport,
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>
    NodeBodyContext<T, N, P, C>
{
    pub fn port_state(&self, id: &P) -> WorldPortVisualState {
        self.port_states.get(id).copied().unwrap_or_default()
    }

    pub fn port_presentation(&self, id: &P) -> AnchorPresentation {
        self.port_presentations.get(id).copied().unwrap_or_default()
    }

    /// Build the reference-style label/control row from `NodeStyle::field_*`.
    pub fn field(&self, label: impl Into<SharedString>, control: impl IntoElement) -> AnyElement {
        let label_color = self.theme.node.field_label_color;
        div()
            .flex()
            .items_center()
            .gap(px(self.theme.node.field_gap))
            .child(
                div()
                    .min_w(px(self.theme.node.field_label_min_width))
                    .text_size(px(self.theme.node.field_label_font_size))
                    .text_color(rgb(label_color.rgb).opacity(label_color.alpha))
                    .child(label.into()),
            )
            .child(control)
            .into_any_element()
    }

    /// Wrap retained fields in the themed body section.
    pub fn body_section(&self, child: impl IntoElement) -> AnyElement {
        let border = self.theme.node.body_border_bottom;
        div()
            .py(px(self.theme.node.body_padding_y))
            .when(
                border.width > 0.0 && border.style != style::LineStyle::None,
                |element| {
                    element
                        .border_b(px(border.width))
                        .border_color(rgb(border.color.rgb).opacity(border.color.alpha))
                        .when(border.style == style::LineStyle::Dashed, |element| {
                            element.border_dashed()
                        })
                },
            )
            .child(child)
            .into_any_element()
    }

    pub fn graph(&self) -> WeakEntity<NodeGraph<T, N, P, C>> {
        self.graph.clone()
    }

    pub fn port_anchor(&self, id: P, child: impl IntoElement) -> AnyElement {
        let graph_down = self.graph.clone();
        let graph_up = self.graph.clone();
        let theme_up = Arc::clone(&self.theme);
        let down_id = id.clone();
        let up_id = id.clone();
        let interactive = div()
            .child(child)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                let _ = graph_down.update(cx, |editor, cx| editor.engage_port(&down_id, cx));
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
                    editor.finish_left_gesture(true, &theme_up, cx);
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
        let color = port.map_or(self.theme.anchor.dot_connected_color, |_| {
            self.theme.anchor.dot_color
        });
        let diameter = self.viewport.scale_length(self.theme.anchor.dot_size);
        self.port_anchor(
            id,
            div()
                .w(px(diameter))
                .h(px(diameter))
                .rounded_full()
                .border(px(self
                    .viewport
                    .scale_length(self.theme.anchor.dot_border_width)))
                .border_color(rgb(color.rgb).opacity(color.alpha))
                .bg(rgb(color.rgb).opacity(if port.is_some() { 0.0 } else { color.alpha })),
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

pub trait NodeOverlayRenderer<T: PortType, N: core::NodeId, P: core::PortId>: 'static {
    fn render_node_overlays(
        &mut self,
        context: WorldNodeBodyContext<T, N, P>,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<NodeOverlay>;
}

impl<T, N, P, F> NodeOverlayRenderer<T, N, P> for F
where
    T: PortType,
    N: core::NodeId,
    P: core::PortId,
    F: FnMut(WorldNodeBodyContext<T, N, P>, &mut Window, &mut App) -> Vec<NodeOverlay> + 'static,
{
    fn render_node_overlays(
        &mut self,
        context: WorldNodeBodyContext<T, N, P>,
        window: &mut Window,
        cx: &mut App,
    ) -> Vec<NodeOverlay> {
        self(context, window, cx)
    }
}

/// Optional presentation overrides for one logical port.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AnchorPresentation {
    /// Per-port socket silhouette. `None` uses the graph-wide default.
    pub dot_shape: Option<style::DotShape>,
    /// Solid idle socket color. Connection/draft state colors still take precedence.
    pub dot_color: Option<style::Color>,
    /// Draw a smaller ghost socket toward the node interior for collection ports.
    pub multi: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorldPortVisualState {
    pub connected: bool,
    pub source: bool,
    pub snap: bool,
    pub compatible: bool,
    pub incompatible: bool,
}

#[derive(Clone)]
pub struct WorldNodeBodyContext<T: PortType, N: core::NodeId, P: core::PortId> {
    pub node: Node<N>,
    pub ports: Arc<[Port<N, P, T>]>,
    pub port_states: Arc<HashMap<P, WorldPortVisualState>>,
    pub port_presentations: Arc<HashMap<P, AnchorPresentation>>,
    pub state: WorldNodeVisualState,
    pub theme: Arc<NodeGraphTheme>,
}

impl<T: PortType, N: core::NodeId, P: core::PortId> WorldNodeBodyContext<T, N, P> {
    pub fn port_state(&self, id: &P) -> WorldPortVisualState {
        self.port_states.get(id).copied().unwrap_or_default()
    }

    pub fn port_presentation(&self, id: &P) -> AnchorPresentation {
        self.port_presentations.get(id).copied().unwrap_or_default()
    }
}

/// Produces an immutable world-space display list. Layout happens in world units once; viewport
/// changes only project the list for painting and inverse hit testing.
pub trait WorldNodeBodyRenderer<T: PortType, N: core::NodeId, P: core::PortId>: 'static {
    fn render_world_node(&mut self, context: WorldNodeBodyContext<T, N, P>) -> world::WorldScene;
}

impl<T, N, P, F> WorldNodeBodyRenderer<T, N, P> for F
where
    T: PortType,
    N: core::NodeId,
    P: core::PortId,
    F: FnMut(WorldNodeBodyContext<T, N, P>) -> world::WorldScene + 'static,
{
    fn render_world_node(&mut self, context: WorldNodeBodyContext<T, N, P>) -> world::WorldScene {
        self(context)
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
            visibility_margin: 600.0,
            route_lane_spacing: 8.0,
            route_corner_radius: 6.0,
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
    /// Optional category accent. `None` uses [`style::MenuStyle::category_color`].
    pub category_color: Option<style::Color>,
    pub description: String,
    pub keywords: Vec<String>,
    pub ports: Vec<CatalogPort<T>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DynamicPort<T> {
    pub key: String,
    pub label: String,
    pub kind: T,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RegistryError {
    DuplicateNodeType(String),
    DuplicatePortKey { node_type: String, key: String },
    EmptyPortKey { node_type: String },
    PortIdCollision,
}

type DynamicPortProducer<T, N> = Rc<dyn Fn(&core::Node<N>) -> Vec<DynamicPort<T>>>;
type RetainedNodeRenderer<T, N, P, C> = RefCell<Box<dyn NodeBodyRenderer<T, N, P, C>>>;
type RegisteredWorldRenderer<T, N, P> = RefCell<Box<dyn WorldNodeBodyRenderer<T, N, P>>>;
type PortIdFactory<N, P> = Rc<dyn Fn(&N, &str) -> P>;
/// Exact counterpart of the Leptos port slot: the renderer receives only the port label.
type PortSlot = Rc<dyn Fn(String) -> AnyElement>;

pub struct NodeTypeDefinition<T, N, P, C>
where
    N: core::NodeId,
    P: core::PortId,
    C: core::ConnectionId,
{
    pub item: NodeCatalogItem<T>,
    dynamic_inputs: Option<DynamicPortProducer<T, N>>,
    dynamic_outputs: Option<DynamicPortProducer<T, N>>,
    retained_renderer: Option<RetainedNodeRenderer<T, N, P, C>>,
    world_renderer: Option<RegisteredWorldRenderer<T, N, P>>,
    port_slots: HashMap<String, PortSlot>,
}

pub struct NodeTypeBuilder<T, N, P, C>
where
    N: core::NodeId,
    P: core::PortId,
    C: core::ConnectionId,
{
    definition: NodeTypeDefinition<T, N, P, C>,
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>
    NodeTypeBuilder<T, N, P, C>
{
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            definition: NodeTypeDefinition {
                item: NodeCatalogItem {
                    id: id.into(),
                    label: label.into(),
                    category: String::new(),
                    category_color: None,
                    description: String::new(),
                    keywords: Vec::new(),
                    ports: Vec::new(),
                },
                dynamic_inputs: None,
                dynamic_outputs: None,
                retained_renderer: None,
                world_renderer: None,
                port_slots: HashMap::new(),
            },
        }
    }

    pub fn category(mut self, category: impl Into<String>, color: Option<style::Color>) -> Self {
        self.definition.item.category = category.into();
        self.definition.item.category_color = color;
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.definition.item.description = description.into();
        self
    }

    pub fn keywords(mut self, keywords: impl IntoIterator<Item = String>) -> Self {
        self.definition.item.keywords = keywords.into_iter().collect();
        self
    }

    pub fn input(mut self, key: impl Into<String>, label: impl Into<String>, kind: T) -> Self {
        self.definition.item.ports.push(CatalogPort {
            id: key.into(),
            label: label.into(),
            direction: PortDirection::Input,
            kind,
        });
        self
    }

    pub fn output(mut self, key: impl Into<String>, label: impl Into<String>, kind: T) -> Self {
        self.definition.item.ports.push(CatalogPort {
            id: key.into(),
            label: label.into(),
            direction: PortDirection::Output,
            kind,
        });
        self
    }

    pub fn dynamic_inputs(
        mut self,
        producer: impl Fn(&core::Node<N>) -> Vec<DynamicPort<T>> + 'static,
    ) -> Self {
        self.definition.dynamic_inputs = Some(Rc::new(producer));
        self
    }

    pub fn dynamic_outputs(
        mut self,
        producer: impl Fn(&core::Node<N>) -> Vec<DynamicPort<T>> + 'static,
    ) -> Self {
        self.definition.dynamic_outputs = Some(Rc::new(producer));
        self
    }

    /// Override one generated port's label slot. The callback arity intentionally matches the
    /// Leptos API exactly: it receives only the port label.
    pub fn port_slot(
        mut self,
        port_id: impl Into<String>,
        renderer: impl Fn(String) -> AnyElement + 'static,
    ) -> Self {
        self.definition
            .port_slots
            .insert(port_id.into(), Rc::new(renderer));
        self
    }

    pub fn renderer(mut self, renderer: impl NodeBodyRenderer<T, N, P, C>) -> Self {
        self.definition.retained_renderer = Some(RefCell::new(Box::new(renderer)));
        self
    }

    pub fn world_renderer(mut self, renderer: impl WorldNodeBodyRenderer<T, N, P>) -> Self {
        self.definition.world_renderer = Some(RefCell::new(Box::new(renderer)));
        self
    }

    pub fn build(self) -> Result<NodeTypeDefinition<T, N, P, C>, RegistryError> {
        let mut keys = HashSet::new();
        for port in &self.definition.item.ports {
            if port.id.is_empty() {
                return Err(RegistryError::EmptyPortKey {
                    node_type: self.definition.item.id.clone(),
                });
            }
            if !keys.insert(port.id.clone()) {
                return Err(RegistryError::DuplicatePortKey {
                    node_type: self.definition.item.id.clone(),
                    key: port.id.clone(),
                });
            }
        }
        Ok(self.definition)
    }
}

pub struct NodeTypeRegistry<T, N, P, C>
where
    N: core::NodeId,
    P: core::PortId,
    C: core::ConnectionId,
{
    id_for: PortIdFactory<N, P>,
    order: Vec<String>,
    definitions: HashMap<String, NodeTypeDefinition<T, N, P, C>>,
    port_type_slots: Vec<(T, PortDirection, PortSlot)>,
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>
    NodeTypeRegistry<T, N, P, C>
{
    pub fn new(id_for: impl Fn(&N, &str) -> P + 'static) -> Self {
        Self {
            id_for: Rc::new(id_for),
            order: Vec::new(),
            definitions: HashMap::new(),
            port_type_slots: Vec::new(),
        }
    }

    /// Register a slot for every port with the matching type and direction. As in the Leptos
    /// implementation, the slot renderer itself receives only the port label.
    pub fn register_port_type_slot(
        &mut self,
        port_type: T,
        direction: PortDirection,
        renderer: impl Fn(String) -> AnyElement + 'static,
    ) {
        self.port_type_slots
            .push((port_type, direction, Rc::new(renderer)));
    }

    pub fn register(
        &mut self,
        definition: NodeTypeDefinition<T, N, P, C>,
    ) -> Result<(), RegistryError> {
        let id = definition.item.id.clone();
        if self.definitions.contains_key(&id) {
            return Err(RegistryError::DuplicateNodeType(id));
        }
        self.order.push(id.clone());
        self.definitions.insert(id, definition);
        Ok(())
    }

    pub fn get(&self, node_type: &str) -> Option<&NodeTypeDefinition<T, N, P, C>> {
        self.definitions.get(node_type)
    }

    pub fn catalog(&self) -> Vec<NodeCatalogItem<T>> {
        self.order
            .iter()
            .filter_map(|id| self.definitions.get(id))
            .map(|definition| definition.item.clone())
            .collect()
    }
}

fn catalog_category_color<T>(item: &NodeCatalogItem<T>, fallback: style::Color) -> style::Color {
    item.category_color.unwrap_or(fallback)
}

#[derive(Clone)]
struct CatalogMenu<P> {
    anchor_world: core::Point,
    query: WorldTextInputState,
    selected: usize,
    connect_from: Option<P>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GraphGroup<N: Eq + std::hash::Hash> {
    pub id: String,
    /// Omit the built-in label while retaining the group box/custom header.
    pub label: Option<String>,
    /// Optional per-group RGBA override; `None` uses [`style::GroupStyle::default_color`].
    pub color: Option<style::Color>,
    pub error: bool,
    pub nodes: HashSet<N>,
}

#[derive(Clone, Debug)]
pub struct GroupHeaderContext<N: Eq + std::hash::Hash> {
    pub group: GraphGroup<N>,
    /// Padded group bounds in world coordinates.
    pub bounds: core::Rect,
    pub hovered: bool,
    pub error: bool,
    pub theme: Arc<NodeGraphTheme>,
}

pub trait GroupHeaderRenderer<N: Eq + std::hash::Hash>: 'static {
    fn render_group_header(
        &mut self,
        context: GroupHeaderContext<N>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;
}

impl<N, F> GroupHeaderRenderer<N> for F
where
    N: Eq + std::hash::Hash + 'static,
    F: FnMut(GroupHeaderContext<N>, &mut Window, &mut App) -> AnyElement + 'static,
{
    fn render_group_header(
        &mut self,
        context: GroupHeaderContext<N>,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        self(context, window, cx)
    }
}

#[derive(Clone)]
struct NodeDrag<N> {
    primary: N,
    offsets: Vec<(N, core::Point)>,
    starts: Vec<(N, core::Point)>,
    moved: bool,
    alter_groups: bool,
}

#[derive(Clone)]
struct GroupEditor {
    id: String,
    query: WorldTextInputState,
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
    current_screen: core::Point,
    snap_target: Option<P>,
    moved: bool,
    /// A controlled-mode edge already requested for removal but still present in
    /// the last host snapshot. It is hidden while the reroute draft is active.
    detached_connection: Option<C>,
}
#[derive(Clone)]
struct BoxSelection<N, C> {
    start: core::Point,
    current: core::Point,
    baseline_nodes: HashSet<N>,
    baseline_connections: HashSet<C>,
}
struct WorldTextPaint {
    origin: core::Point,
    line_height: f32,
    line: ShapedLine,
}

struct WorldPaintState {
    scene: world::ScreenScene,
    text: Vec<WorldTextPaint>,
}

/// Return the reference SVG silhouette for a non-circular anchor dot in world units.
pub fn dot_shape_points(
    center: core::Point,
    radius: f32,
    shape: style::DotShape,
) -> Arc<[core::Point]> {
    use style::DotShape;

    // These are the reference SVG paths normalized around its 24x24 view box.
    // Normalizing each silhouette to `radius` preserves the authored orientation:
    // a right-pointing trigger triangle and a four-pointed (not five-pointed) star.
    let normalized: &[(f32, f32)] = match shape {
        DotShape::Diamond => &[(0.0, -1.0), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)],
        DotShape::Square => &[(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)],
        DotShape::Hexagon => &[
            (0.0, -1.0),
            (18.0 / 19.0, -8.5 / 19.0),
            (18.0 / 19.0, 8.5 / 19.0),
            (0.0, 1.0),
            (-18.0 / 19.0, 8.5 / 19.0),
            (-18.0 / 19.0, -8.5 / 19.0),
        ],
        DotShape::Triangle => &[(-7.5 / 9.0, -1.0), (1.0, 0.0), (-7.5 / 9.0, 1.0)],
        DotShape::Star => &[
            (0.0, -1.0),
            (0.26, -0.26),
            (1.0, 0.0),
            (0.26, 0.26),
            (0.0, 1.0),
            (-0.26, 0.26),
            (-1.0, 0.0),
            (-0.26, -0.26),
        ],
        DotShape::Circle => unreachable!("circles do not need polygon points"),
    };
    normalized
        .iter()
        .map(|(x, y)| core::Point::new(center.x + x * radius, center.y + y * radius))
        .collect::<Vec<_>>()
        .into()
}

fn push_dot_shape(
    scene: &mut world::WorldScene,
    center: core::Point,
    radius: f32,
    shape: style::DotShape,
    fill: style::Color,
    opacity: f32,
) {
    let fill = world::WorldColor::rgba(fill.rgb, fill.alpha * opacity);
    match shape {
        style::DotShape::Circle => scene.push(world::WorldPrimitive::Circle {
            center,
            radius,
            fill,
        }),
        shape => scene.push(world::WorldPrimitive::Polygon {
            points: dot_shape_points(center, radius, shape),
            fill,
        }),
    }
}

/// Paint a world-space display list through one deterministic viewport projection.
///
/// Text line breaks and primitive geometry are authored once in [`world::WorldScene`]. Zoom only
/// projects that immutable display list; it never asks GPUI flex layout to reflow the content.
pub fn world_scene_element(scene: world::WorldScene, viewport: Viewport) -> AnyElement {
    let projected = scene.project(viewport);
    canvas(
        move |_bounds, window, _cx| {
            let mut text = Vec::new();
            let text_style = window.text_style();
            for primitive in &projected.primitives {
                if let world::ScreenPrimitive::Text {
                    origin,
                    lines,
                    color,
                    font_size,
                    font_weight,
                    line_height,
                } = primitive
                {
                    for (index, value) in lines.as_slice().iter().enumerate() {
                        let value: SharedString = value.clone().into();
                        let mut font = text_style.font();
                        font.weight = gpui::FontWeight(*font_weight as f32);
                        let run = TextRun {
                            len: value.len(),
                            font,
                            color: rgb(color.rgb).opacity(color.alpha).into(),
                            ..Default::default()
                        };
                        text.push(WorldTextPaint {
                            origin: core::Point::new(
                                origin.x,
                                origin.y + *line_height * index as f32,
                            ),
                            line_height: *line_height,
                            line: window.text_system().shape_line(
                                value,
                                px(*font_size),
                                &[run],
                                None,
                            ),
                        });
                    }
                }
            }
            WorldPaintState {
                scene: projected,
                text,
            }
        },
        move |bounds, state, window, cx| {
            let offset = core::Point::new(f32::from(bounds.origin.x), f32::from(bounds.origin.y));
            for primitive in &state.scene.primitives {
                match primitive {
                    world::ScreenPrimitive::Quad {
                        bounds: rect,
                        fill,
                        corner_radius,
                    } => {
                        let paint_bounds = Bounds::new(
                            point(px(rect.origin.x + offset.x), px(rect.origin.y + offset.y)),
                            gpui::size(px(rect.size.width), px(rect.size.height)),
                        );
                        window.paint_quad(quad(
                            paint_bounds,
                            px(*corner_radius),
                            rgb(fill.rgb).opacity(fill.alpha),
                            px(0.0),
                            gpui::transparent_black(),
                            BorderStyle::default(),
                        ));
                    }
                    world::ScreenPrimitive::BorderedQuad {
                        bounds: rect,
                        fill,
                        border,
                        border_width,
                        corner_radius,
                    } => {
                        let paint_bounds = Bounds::new(
                            point(px(rect.origin.x + offset.x), px(rect.origin.y + offset.y)),
                            gpui::size(px(rect.size.width), px(rect.size.height)),
                        );
                        window.paint_quad(quad(
                            paint_bounds,
                            px(*corner_radius),
                            rgb(fill.rgb).opacity(fill.alpha),
                            px(*border_width),
                            rgb(border.rgb).opacity(border.alpha),
                            BorderStyle::default(),
                        ));
                    }
                    world::ScreenPrimitive::Line {
                        start,
                        end,
                        color,
                        width,
                    } => {
                        let mut builder = PathBuilder::stroke(px(*width));
                        builder.move_to(point(px(start.x + offset.x), px(start.y + offset.y)));
                        builder.line_to(point(px(end.x + offset.x), px(end.y + offset.y)));
                        if let Ok(path) = builder.build() {
                            window.paint_path(path, rgb(color.rgb).opacity(color.alpha));
                        }
                    }
                    world::ScreenPrimitive::Circle {
                        center,
                        radius,
                        fill,
                    } => {
                        let diameter = radius * 2.0;
                        let paint_bounds = Bounds::new(
                            point(
                                px(center.x - radius + offset.x),
                                px(center.y - radius + offset.y),
                            ),
                            gpui::size(px(diameter), px(diameter)),
                        );
                        window.paint_quad(quad(
                            paint_bounds,
                            px(*radius),
                            rgb(fill.rgb).opacity(fill.alpha),
                            px(0.0),
                            gpui::transparent_black(),
                            BorderStyle::default(),
                        ));
                    }
                    world::ScreenPrimitive::Polygon { points, fill } => {
                        if let Some(first) = points.first() {
                            let mut builder = PathBuilder::fill();
                            builder.move_to(point(px(first.x + offset.x), px(first.y + offset.y)));
                            for point_value in &points[1..] {
                                builder.line_to(point(
                                    px(point_value.x + offset.x),
                                    px(point_value.y + offset.y),
                                ));
                            }
                            builder.close();
                            if let Ok(path) = builder.build() {
                                window.paint_path(path, rgb(fill.rgb).opacity(fill.alpha));
                            }
                        }
                    }
                    world::ScreenPrimitive::Text { .. } => {}
                }
            }
            for text in &state.text {
                let _ = text.line.paint(
                    point(px(text.origin.x + offset.x), px(text.origin.y + offset.y)),
                    px(text.line_height),
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
        },
    )
    .absolute()
    .size_full()
    .into_any_element()
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

/// Platform text state for the currently focused world-space text control.
/// All offsets are UTF-16 code-unit offsets, matching [`gpui::InputHandler`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldTextInputState {
    pub text: String,
    pub selection: Range<usize>,
    pub selection_reversed: bool,
    pub marked: Option<Range<usize>>,
}

impl WorldTextInputState {
    pub fn new(text: impl Into<String>, selection: Range<usize>) -> Self {
        Self {
            text: text.into(),
            selection,
            selection_reversed: false,
            marked: None,
        }
    }

    fn at_end(text: impl Into<String>) -> Self {
        let text = text.into();
        let end = utf16_len(&text);
        Self::new(text, end..end)
    }
}

pub struct NodeGraph<
    T: PortType,
    N: core::NodeId = String,
    P: core::PortId = String,
    C: core::ConnectionId = String,
> {
    pub graph: GraphState<N, P, C, T>,
    pub config: EditorConfig,
    drag: Option<NodeDrag<N>>,
    resize: Option<ResizeDrag<N, P>>,
    panning: Option<core::Point>,
    last_pointer_screen: Option<core::Point>,
    hovered_port: Option<P>,
    port_presentations: HashMap<P, AnchorPresentation>,
    draft: Option<DraftConnection<P, C>>,
    catalog: Vec<NodeCatalogItem<T>>,
    node_type_registry: Option<NodeTypeRegistry<T, N, P, C>>,
    defined_port_order: HashMap<N, Vec<P>>,
    pending_port_changes: HashMap<N, core::PortChange<N, P, T>>,
    node_type_registry_error: Option<RegistryError>,
    catalog_menu: Option<CatalogMenu<P>>,
    anchor_menu_builder: Option<AnchorMenuBuilder<P>>,
    anchor_menu: Option<ActiveAnchorMenu<P>>,
    catalog_scroll_handle: ScrollHandle,
    node_body_renderer: Option<Box<dyn NodeBodyRenderer<T, N, P, C>>>,
    world_node_body_renderer: Option<Box<dyn WorldNodeBodyRenderer<T, N, P>>>,
    node_overlay_renderer: Option<Box<dyn NodeOverlayRenderer<T, N, P>>>,
    group_header_renderer: Option<Box<dyn GroupHeaderRenderer<N>>>,
    world_scene: world::WorldScene,
    world_control_owners: HashMap<String, N>,
    world_control_order: Vec<(N, String)>,
    last_world_control: Option<(N, String)>,
    world_text_input: Option<(N, String, WorldTextInputState)>,
    groups: Vec<GraphGroup<N>>,
    auto_width_nodes: HashSet<N>,
    render_geometry: RenderGeometry<N, P>,
    dangling_connections: Vec<DanglingConnection<P, C>>,
    dismissed_overlays: HashSet<String>,
    active_dismissible_overlays: HashSet<String>,
    active_escape_overlays: HashSet<String>,
    active_outside_overlays: HashSet<String>,
    active_backdrop_overlays: HashSet<String>,
    active_overlay_dismiss_callbacks: HashMap<String, Rc<dyn Fn()>>,
    active_overlay_bounds: Vec<core::Rect>,
    measured_overlay_sizes: HashMap<String, core::Size>,
    overlay_placements: HashMap<String, OverlayPlacement>,
    overlay_anchor_controls: HashMap<String, String>,
    overlay_pane_anchors: HashMap<String, core::Rect>,
    route_cache: RefCell<RouteCache<C>>,
    group_editor: Option<GroupEditor>,
    group_errors: HashSet<String>,
    box_selection: Option<BoxSelection<N, C>>,
    focus_handle: Option<FocusHandle>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId>
    gpui::EventEmitter<core::GraphEvent<N, P, C, T>> for NodeGraph<T, N, P, C>
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
            config: EditorConfig::default(),
            drag: None,
            resize: None,
            panning: None,
            last_pointer_screen: None,
            hovered_port: None,
            port_presentations: HashMap::new(),
            draft: None,
            catalog: Vec::new(),
            node_type_registry: None,
            defined_port_order: HashMap::new(),
            pending_port_changes: HashMap::new(),
            node_type_registry_error: None,
            catalog_menu: None,
            anchor_menu_builder: None,
            anchor_menu: None,
            catalog_scroll_handle: ScrollHandle::new(),
            node_body_renderer: None,
            world_node_body_renderer: None,
            node_overlay_renderer: None,
            group_header_renderer: None,
            world_scene: world::WorldScene::new(),
            world_control_owners: HashMap::new(),
            world_control_order: Vec::new(),
            last_world_control: None,
            world_text_input: None,
            groups: Vec::new(),
            auto_width_nodes: HashSet::new(),
            render_geometry: RenderGeometry::default(),
            dangling_connections: Vec::new(),
            dismissed_overlays: HashSet::new(),
            active_dismissible_overlays: HashSet::new(),
            active_escape_overlays: HashSet::new(),
            active_outside_overlays: HashSet::new(),
            active_backdrop_overlays: HashSet::new(),
            active_overlay_dismiss_callbacks: HashMap::new(),
            active_overlay_bounds: Vec::new(),
            measured_overlay_sizes: HashMap::new(),
            overlay_placements: HashMap::new(),
            overlay_anchor_controls: HashMap::new(),
            overlay_pane_anchors: HashMap::new(),
            route_cache: RefCell::new(RouteCache::default()),
            group_editor: None,
            group_errors: HashSet::new(),
            box_selection: None,
            focus_handle: None,
            canvas_bounds: Rc::new(Cell::new(Bounds::default())),
        })
    }

    pub fn focus_handle(&self) -> Option<&FocusHandle> {
        self.focus_handle.as_ref()
    }

    pub fn with_overlay_placement(
        mut self,
        id: impl Into<String>,
        placement: OverlayPlacement,
    ) -> Self {
        self.overlay_placements.insert(id.into(), placement);
        self
    }

    /// Anchor a pane-space overlay to a world control hit region by stable control ID.
    pub fn with_overlay_anchor_control(
        mut self,
        overlay_id: impl Into<String>,
        control_id: impl Into<String>,
    ) -> Self {
        self.overlay_anchor_controls
            .insert(overlay_id.into(), control_id.into());
        self
    }

    pub fn set_overlay_anchor_control(
        &mut self,
        overlay_id: impl Into<String>,
        control_id: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        self.overlay_anchor_controls
            .insert(overlay_id.into(), control_id.into());
        cx.notify();
    }

    /// Anchor an overlay to an arbitrary fixed pane-space rectangle.
    pub fn with_overlay_pane_anchor(
        mut self,
        overlay_id: impl Into<String>,
        bounds: core::Rect,
    ) -> Self {
        self.overlay_pane_anchors.insert(overlay_id.into(), bounds);
        self
    }

    pub fn set_overlay_pane_anchor(
        &mut self,
        overlay_id: impl Into<String>,
        bounds: core::Rect,
        cx: &mut Context<Self>,
    ) {
        self.overlay_pane_anchors.insert(overlay_id.into(), bounds);
        cx.notify();
    }

    pub fn set_overlay_placement(
        &mut self,
        id: impl Into<String>,
        placement: OverlayPlacement,
        cx: &mut Context<Self>,
    ) {
        self.overlay_placements.insert(id.into(), placement);
        cx.notify();
    }

    /// Replace the built-in anchor menu wholesale. Returning no items suppresses it.
    pub fn with_anchor_menu_builder(mut self, builder: AnchorMenuBuilder<P>) -> Self {
        self.anchor_menu_builder = Some(builder);
        self
    }

    pub fn set_anchor_menu_builder(
        &mut self,
        builder: Option<AnchorMenuBuilder<P>>,
        cx: &mut Context<Self>,
    ) {
        self.anchor_menu_builder = builder;
        self.anchor_menu = None;
        cx.notify();
    }

    pub fn anchor_menu_is_open(&self) -> bool {
        self.anchor_menu.is_some()
    }

    pub fn anchor_menu_items(&self, port_id: &P) -> Vec<AnchorMenuItem> {
        let Some(port) = self.graph.ports.get(port_id) else {
            return Vec::new();
        };
        if let Some(builder) = &self.anchor_menu_builder {
            return builder.build(port_id, port.direction);
        }
        let has_connections =
            self.graph
                .connections
                .values()
                .any(|connection| connection.source == *port_id || connection.target == *port_id)
                || self.dangling_connections.iter().any(|connection| {
                    connection.source == *port_id || connection.target == *port_id
                });
        let has_broken = self
            .dangling_connections
            .iter()
            .any(|connection| connection.source == *port_id || connection.target == *port_id);
        vec![
            AnchorMenuItem {
                label: "Remove connections".into(),
                action: AnchorMenuAction::RemoveConnections,
                enabled: has_connections,
            },
            AnchorMenuItem {
                label: "Remove broken connections".into(),
                action: AnchorMenuAction::RemoveBrokenConnections,
                enabled: has_broken,
            },
        ]
    }

    pub fn with_catalog(mut self, catalog: Vec<NodeCatalogItem<T>>) -> Self {
        self.catalog = catalog;
        self
    }

    pub fn with_node_type_registry(mut self, registry: NodeTypeRegistry<T, N, P, C>) -> Self {
        self.catalog = registry.catalog();
        self.node_type_registry = Some(registry);
        self
    }

    pub fn with_node_type_registry_in(
        mut self,
        registry: NodeTypeRegistry<T, N, P, C>,
        cx: &mut Context<Self>,
    ) -> Result<Self, RegistryError> {
        self.catalog = registry.catalog();
        self.node_type_registry = Some(registry);
        self.refresh_node_types(cx)?;
        Ok(self)
    }

    pub fn node_type_registry_error(&self) -> Option<&RegistryError> {
        self.node_type_registry_error.as_ref()
    }

    pub fn set_node_type_registry(
        &mut self,
        registry: Option<NodeTypeRegistry<T, N, P, C>>,
        cx: &mut Context<Self>,
    ) -> Result<(), RegistryError> {
        self.catalog = registry
            .as_ref()
            .map_or_else(Vec::new, NodeTypeRegistry::catalog);
        self.node_type_registry = registry;
        self.pending_port_changes.clear();
        if self.node_type_registry.is_some() {
            self.refresh_node_types(cx)?;
        } else {
            cx.notify();
        }
        Ok(())
    }

    pub fn refresh_node_types(&mut self, cx: &mut Context<Self>) -> Result<bool, RegistryError> {
        let Some(registry) = self.node_type_registry.as_ref() else {
            return Ok(false);
        };
        let (plans, desired_orders) = {
            let mut generated: HashMap<P, N> = HashMap::new();
            let mut plans = Vec::new();
            let mut desired_orders = HashMap::new();
            for node in self.graph.nodes.values() {
                let Some(definition) = registry.get(&node.node_type) else {
                    continue;
                };
                let mut specs = definition.item.ports.clone();
                if let Some(producer) = &definition.dynamic_inputs {
                    specs.extend(producer(node).into_iter().map(|port| CatalogPort {
                        id: port.key,
                        label: port.label,
                        direction: PortDirection::Input,
                        kind: port.kind,
                    }));
                }
                if let Some(producer) = &definition.dynamic_outputs {
                    specs.extend(producer(node).into_iter().map(|port| CatalogPort {
                        id: port.key,
                        label: port.label,
                        direction: PortDirection::Output,
                        kind: port.kind,
                    }));
                }
                let mut keys = HashSet::new();
                let mut desired = Vec::new();
                for spec in specs {
                    if spec.id.is_empty() {
                        return Err(RegistryError::EmptyPortKey {
                            node_type: node.node_type.clone(),
                        });
                    }
                    if !keys.insert(spec.id.clone()) {
                        return Err(RegistryError::DuplicatePortKey {
                            node_type: node.node_type.clone(),
                            key: spec.id,
                        });
                    }
                    let id = (registry.id_for)(&node.id, &spec.id);
                    if generated.insert(id.clone(), node.id.clone()).is_some()
                        || self
                            .graph
                            .ports
                            .get(&id)
                            .is_some_and(|port| port.node != node.id)
                    {
                        return Err(RegistryError::PortIdCollision);
                    }
                    let position = self
                        .graph
                        .ports
                        .get(&id)
                        .map_or(node.position, |port| port.position);
                    desired.push(Port {
                        id,
                        node: node.id.clone(),
                        label: spec.label,
                        direction: spec.direction,
                        kind: spec.kind,
                        position,
                    });
                }
                let desired_ids: Vec<_> = desired.iter().map(|port| port.id.clone()).collect();
                let managed = self
                    .defined_port_order
                    .get(&node.id)
                    .cloned()
                    .unwrap_or_else(|| {
                        desired_ids
                            .iter()
                            .filter(|id| {
                                self.graph
                                    .ports
                                    .get(*id)
                                    .is_some_and(|port| port.node == node.id)
                            })
                            .cloned()
                            .collect()
                    });
                let desired_set: HashSet<_> = desired_ids.iter().cloned().collect();
                let remove: Vec<_> = managed
                    .into_iter()
                    .filter(|id| !desired_set.contains(id) && self.graph.ports.contains_key(id))
                    .collect();
                let upsert: Vec<_> = desired
                    .into_iter()
                    .filter(|desired| {
                        self.graph.ports.get(&desired.id).is_none_or(|current| {
                            current.node != desired.node
                                || current.label != desired.label
                                || current.direction != desired.direction
                                || current.kind != desired.kind
                        })
                    })
                    .collect();
                desired_orders.insert(node.id.clone(), desired_ids);
                if !remove.is_empty() || !upsert.is_empty() {
                    plans.push(core::PortChange {
                        node_id: node.id.clone(),
                        remove,
                        upsert,
                    });
                }
            }
            (plans, desired_orders)
        };

        if self.config.mutation_mode == MutationMode::Controlled {
            let fresh: Vec<_> = plans
                .into_iter()
                .filter(|change| self.pending_port_changes.get(&change.node_id) != Some(change))
                .collect();
            for change in &fresh {
                for id in &change.remove {
                    self.capture_tombstones_for_port(id);
                }
                self.pending_port_changes
                    .insert(change.node_id.clone(), change.clone());
            }
            if fresh.is_empty() {
                for (node, order) in desired_orders {
                    if order.iter().all(|id| self.graph.ports.contains_key(id)) {
                        self.defined_port_order.insert(node, order);
                    }
                }
                return Ok(false);
            }
            cx.emit(core::GraphEvent::MutationRequested {
                mutations: fresh
                    .into_iter()
                    .map(|change| core::GraphMutation::ReconcileNodePorts { change })
                    .collect(),
            });
            cx.notify();
            return Ok(true);
        }

        let mut changed = false;
        for change in plans {
            for id in &change.remove {
                if let Some(removed) = self.remove_port_to_tombstones(id) {
                    changed = true;
                    for id in removed {
                        cx.emit(core::GraphEvent::ConnectionRemoved { id });
                    }
                }
            }
            for port in &change.upsert {
                let semantic_change = self.graph.ports.get(&port.id).is_some_and(|current| {
                    current.direction != port.direction || current.kind != port.kind
                });
                if semantic_change && let Some(removed) = self.remove_port_to_tombstones(&port.id) {
                    for id in removed {
                        cx.emit(core::GraphEvent::ConnectionRemoved { id });
                    }
                }
                self.graph.ports.insert(port.id.clone(), port.clone());
                self.restore_tombstoned_connections(&port.id, cx);
                changed = true;
            }
            self.defined_port_order.insert(
                change.node_id.clone(),
                desired_orders
                    .get(&change.node_id)
                    .cloned()
                    .unwrap_or_default(),
            );
            cx.emit(core::GraphEvent::NodePortsReconciled { change });
        }
        for (node, order) in desired_orders {
            self.defined_port_order.insert(node, order);
        }
        if changed {
            self.graph
                .validate()
                .expect("registry reconciliation must remain strict");
            cx.notify();
        }
        Ok(changed)
    }

    pub fn set_catalog(&mut self, catalog: Vec<NodeCatalogItem<T>>, cx: &mut Context<Self>) {
        self.catalog = catalog;
        self.catalog_menu = None;
        cx.notify();
    }

    pub fn with_anchor_presentations(
        mut self,
        presentations: impl IntoIterator<Item = (P, AnchorPresentation)>,
    ) -> Self {
        self.port_presentations = presentations
            .into_iter()
            .filter(|(id, presentation)| {
                self.graph.ports.contains_key(id) && *presentation != AnchorPresentation::default()
            })
            .collect();
        self
    }

    pub fn set_anchor_presentations(
        &mut self,
        presentations: impl IntoIterator<Item = (P, AnchorPresentation)>,
        cx: &mut Context<Self>,
    ) {
        self.port_presentations = presentations
            .into_iter()
            .filter(|(id, presentation)| {
                self.graph.ports.contains_key(id) && *presentation != AnchorPresentation::default()
            })
            .collect();
        cx.notify();
    }

    pub fn with_node_overlay_renderer(
        mut self,
        renderer: impl NodeOverlayRenderer<T, N, P>,
    ) -> Self {
        self.node_overlay_renderer = Some(Box::new(renderer));
        self
    }

    pub fn with_group_header_renderer(mut self, renderer: impl GroupHeaderRenderer<N>) -> Self {
        self.group_header_renderer = Some(Box::new(renderer));
        self
    }

    pub fn with_world_node_body_renderer(
        mut self,
        renderer: impl WorldNodeBodyRenderer<T, N, P>,
    ) -> Self {
        self.world_node_body_renderer = Some(Box::new(renderer));
        self
    }

    /// Replace the immutable world-space renderer used for node bodies.
    pub fn set_world_node_body_renderer(
        &mut self,
        renderer: impl WorldNodeBodyRenderer<T, N, P>,
        cx: &mut Context<Self>,
    ) {
        self.world_node_body_renderer = Some(Box::new(renderer));
        cx.notify();
    }

    /// Return to the retained renderer (when configured) or the built-in body.
    pub fn clear_world_node_body_renderer(&mut self, cx: &mut Context<Self>) {
        self.world_node_body_renderer = None;
        self.world_scene = world::WorldScene::new();
        self.world_control_owners.clear();
        self.world_control_order.clear();
        self.blur_world_control(cx);
        cx.notify();
    }

    /// The most recently authored immutable scene, before viewport projection.
    pub fn world_scene(&self) -> &world::WorldScene {
        &self.world_scene
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
        self.group_errors
            .retain(|id| self.groups.iter().any(|group| &group.id == id));
        cx.notify();
    }

    pub fn groups(&self) -> &[GraphGroup<N>] {
        &self.groups
    }

    /// Mark a group as erroneous without changing the existing `GraphGroup` data API.
    pub fn set_group_error(&mut self, id: &str, error: bool, cx: &mut Context<Self>) {
        let changed = if error {
            self.group_errors.insert(id.to_owned())
        } else {
            self.group_errors.remove(id)
        };
        if changed {
            cx.notify();
        }
    }

    pub fn group_has_error(&self, id: &str) -> bool {
        self.group_errors.contains(id)
    }

    pub fn world_layout_fingerprint(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        format!("{:?}", self.world_scene.primitives).hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    pub fn focus_world_control(
        &mut self,
        node_id: N,
        control_id: impl Into<String>,
        cx: &mut Context<Self>,
    ) {
        let control_id = control_id.into();
        if self.last_world_control.as_ref() == Some(&(node_id.clone(), control_id.clone())) {
            return;
        }
        self.blur_world_control(cx);
        self.last_world_control = Some((node_id.clone(), control_id.clone()));
        cx.emit(core::GraphEvent::NodeControlFocused {
            node_id,
            control_id,
        });
        cx.notify();
    }

    fn activate_world_control(&mut self, node_id: N, control_id: String, cx: &mut Context<Self>) {
        if self.last_world_control.as_ref() != Some(&(node_id.clone(), control_id.clone())) {
            self.blur_world_control(cx);
        }
        self.last_world_control = Some((node_id.clone(), control_id.clone()));
        cx.emit(core::GraphEvent::NodeControlActivated {
            node_id,
            control_id,
        });
        cx.notify();
    }

    fn connection_is_detached(&self, id: &C) -> bool {
        self.draft
            .as_ref()
            .and_then(|draft| draft.detached_connection.as_ref())
            == Some(id)
    }

    pub fn port_visual_state(&self, id: &P) -> Option<WorldPortVisualState> {
        self.graph.ports.get(id)?;
        let connected = self.graph.connections.values().any(|connection| {
            !self.connection_is_detached(&connection.id)
                && (connection.source == *id || connection.target == *id)
        });
        let source = self.draft.as_ref().is_some_and(|draft| draft.origin == *id);
        let snap = self
            .draft
            .as_ref()
            .and_then(|draft| draft.snap_target.as_ref())
            == Some(id);
        let compatible = self
            .draft
            .as_ref()
            .is_some_and(|draft| self.normalized_connection(&draft.origin, id).is_some());
        Some(WorldPortVisualState {
            connected,
            source,
            snap,
            compatible,
            incompatible: self.draft.is_some() && !(source || snap || compatible),
        })
    }

    pub fn hovered_port(&self) -> Option<&P> {
        self.hovered_port.as_ref()
    }

    pub fn port_presentation(&self, id: &P) -> AnchorPresentation {
        self.port_presentations.get(id).copied().unwrap_or_default()
    }

    pub fn set_port_presentation(
        &mut self,
        id: P,
        presentation: AnchorPresentation,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.graph.ports.contains_key(&id) {
            return false;
        }
        if presentation == AnchorPresentation::default() {
            self.port_presentations.remove(&id);
        } else {
            self.port_presentations.insert(id, presentation);
        }
        cx.notify();
        true
    }

    /// Synchronize the platform text document for a world-space text control.
    /// Call this on activation and when its externally-owned value changes.
    pub fn set_world_text_input(
        &mut self,
        node_id: N,
        control_id: impl Into<String>,
        mut state: WorldTextInputState,
        cx: &mut Context<Self>,
    ) {
        let control_id = control_id.into();
        clamp_world_text_state(&mut state);
        self.world_text_input = Some((node_id, control_id, state));
        cx.notify();
    }

    pub fn clear_world_text_input(&mut self, cx: &mut Context<Self>) {
        if self.world_text_input.take().is_some() {
            cx.notify();
        }
    }

    pub fn world_text_input_is_active(&self) -> bool {
        self.active_text_world_rect().is_some()
    }

    pub fn world_text_input(&self) -> Option<(&N, &str, &WorldTextInputState)> {
        self.world_text_input
            .as_ref()
            .map(|(node, control, state)| (node, control.as_str(), state))
    }

    pub fn last_world_control(&self) -> Option<(&N, &str)> {
        self.last_world_control
            .as_ref()
            .map(|(node, control)| (node, control.as_str()))
    }
    fn blur_world_control(&mut self, cx: &mut Context<Self>) {
        if let Some((node_id, control_id)) = self.last_world_control.take() {
            cx.emit(core::GraphEvent::NodeControlBlurred {
                node_id,
                control_id,
            });
        }
        self.world_text_input = None;
    }

    /// Convert window/client coordinates into immutable world/canvas coordinates.
    /// Returns `None` until the editor has completed its first layout.
    pub fn client_to_canvas(&self, client: core::Point) -> Option<core::Point> {
        let bounds = self.canvas_bounds.get();
        if bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
            return None;
        }
        let local = core::Point::new(
            client.x - f32::from(bounds.origin.x),
            client.y - f32::from(bounds.origin.y),
        );
        Some(self.graph.viewport.screen_to_world(local))
    }

    /// Convert immutable world/canvas coordinates back into window/client coordinates.
    /// Returns `None` until the editor has completed its first layout.
    pub fn canvas_to_client(&self, canvas: core::Point) -> Option<core::Point> {
        let bounds = self.canvas_bounds.get();
        if bounds.size.width <= px(0.0) || bounds.size.height <= px(0.0) {
            return None;
        }
        let local = self.graph.viewport.world_to_screen(canvas);
        Some(core::Point::new(
            local.x + f32::from(bounds.origin.x),
            local.y + f32::from(bounds.origin.y),
        ))
    }

    /// Create a weak consumer-side handle suitable for toolbars and sibling panes.
    pub fn editor_handle(entity: &gpui::Entity<Self>) -> EditorHandle<T, N, P, C> {
        EditorHandle::new(entity)
    }

    /// Apply an outside-the-editor viewport write using the same event surface as gestures.
    pub fn set_external_viewport(
        &mut self,
        mut viewport: core::Viewport,
        cx: &mut Context<Self>,
    ) -> bool {
        viewport = viewport.sanitized();
        viewport.zoom = viewport
            .zoom
            .clamp(self.config.min_zoom, self.config.max_zoom);
        if viewport == self.graph.viewport {
            return false;
        }
        self.graph.viewport = viewport;
        cx.emit(core::GraphEvent::ViewportChanged { viewport });
        cx.notify();
        true
    }

    /// Emit a catalog-backed create request at a window/client position.
    ///
    /// This deliberately does not mutate graph state: it matches `choose_catalog`,
    /// leaving node ownership and ID allocation with the consumer.
    pub fn request_create_node_at_client(
        &mut self,
        item_id: &str,
        client: core::Point,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.catalog.iter().any(|item| item.id == item_id) {
            return false;
        }
        let Some(position) = self.client_to_canvas(client) else {
            return false;
        };
        cx.emit(core::GraphEvent::CreateNode {
            item_id: item_id.to_owned(),
            position,
            connect_from: None,
            connect_to: None,
            connect_direction: None,
        });
        cx.notify();
        true
    }

    pub fn render_geometry(&self) -> &RenderGeometry<N, P> {
        &self.render_geometry
    }

    fn automatic_default_layout(
        &self,
        node: &core::Node<N>,
        theme: &NodeGraphTheme,
    ) -> Option<(layout::DefaultNodeLayout, Vec<P>)> {
        let registered = self
            .node_type_registry
            .as_ref()
            .and_then(|registry| registry.get(&node.node_type))
            .is_some_and(|definition| {
                definition.world_renderer.is_some() || definition.retained_renderer.is_some()
            });
        if registered
            || self.node_body_renderer.is_some()
            || self.world_node_body_renderer.is_some()
        {
            return None;
        }
        let mut ports: Vec<_> = self
            .graph
            .ports
            .values()
            .filter(|port| port.node == node.id)
            .collect();
        ports.sort_by_cached_key(|port| format!("{:?}", port.id));
        let ids = ports.iter().map(|port| port.id.clone()).collect();
        let directions: Vec<_> = ports.iter().map(|port| port.direction).collect();
        let accent_height = self
            .catalog
            .iter()
            .find(|item| item.id == node.node_type)
            .and_then(|item| item.category_color)
            .map_or(0.0, |_| theme.node.header_accent_height);
        let header_height = accent_height
            + theme.node.header_font_size * 1.2
            + theme.node.header_padding_y * 2.0
            + visible_border_width(theme.node.header_border_bottom);
        let width = if self.auto_width_nodes.contains(&node.id) {
            theme.node.width.unwrap_or(theme.node.min_width)
        } else {
            node.size.width
        };
        let metrics = layout::NodeSectionMetrics::contiguous(width, header_height, 0.0);
        Some((
            layout::layout_default_node(theme, metrics, &directions),
            ids,
        ))
    }

    fn refresh_default_render_geometry(&mut self, theme: &NodeGraphTheme) {
        let layouts: Vec<_> = self
            .graph
            .nodes
            .values()
            .filter_map(|node| {
                let (layout, ports) = self.automatic_default_layout(node, theme)?;
                let size = core::Size {
                    width: if self.auto_width_nodes.contains(&node.id) {
                        layout.size.width
                    } else {
                        layout.size.width.max(node.size.width)
                    },
                    height: layout.size.height.max(node.size.height),
                };
                Some((node.id.clone(), size, ports, layout.port_offsets))
            })
            .collect();
        for (node_id, size, ports, offsets) in layouts {
            self.render_geometry
                .node_sizes
                .insert(node_id.clone(), size);
            for (port, offset) in ports.into_iter().zip(offsets) {
                self.render_geometry
                    .port_offsets
                    .insert(port, (node_id.clone(), offset));
            }
        }
    }

    pub fn resolved_node_size(&self, id: &N) -> Option<core::Size> {
        let node = self.graph.nodes.get(id)?;
        Some(
            self.render_geometry
                .node_sizes
                .get(id)
                .copied()
                .unwrap_or(node.size),
        )
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
        if !self.graph.ports.contains_key(id) {
            return false;
        }
        if self.config.mutation_mode == MutationMode::Controlled {
            self.capture_tombstones_for_port(id);
            let mut connection_ids: Vec<_> = self
                .graph
                .connections
                .values()
                .filter(|connection| connection.source == *id || connection.target == *id)
                .map(|connection| connection.id.clone())
                .collect();
            connection_ids.sort_by_cached_key(|connection_id| format!("{connection_id:?}"));
            let mut mutations = connection_ids
                .into_iter()
                .map(|id| core::GraphMutation::RemoveConnection { id })
                .collect::<Vec<_>>();
            mutations.push(core::GraphMutation::RemovePort { id: id.clone() });
            cx.emit(core::GraphEvent::MutationRequested { mutations });
            return true;
        }
        let Some(removed_connections) = self.remove_port_to_tombstones(id) else {
            return false;
        };
        for id in removed_connections {
            cx.emit(core::GraphEvent::ConnectionRemoved { id });
        }
        cx.notify();
        true
    }

    fn capture_tombstones_for_port(&mut self, id: &P) {
        let mut affected: Vec<_> = self
            .graph
            .connections
            .values()
            .filter(|connection| connection.source == *id || connection.target == *id)
            .cloned()
            .collect();
        affected.sort_by_cached_key(|connection| format!("{:?}", connection.id));
        for connection in affected {
            let source_position = self
                .resolved_port_position(&connection.source)
                .unwrap_or_default();
            let target_position = self
                .resolved_port_position(&connection.target)
                .unwrap_or_default();
            self.dangling_connections
                .retain(|tombstone| tombstone.id != connection.id);
            self.dangling_connections.push(DanglingConnection {
                id: connection.id,
                source: connection.source,
                target: connection.target,
                missing_port: id.clone(),
                source_position,
                target_position,
            });
        }
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
        self.port_presentations.remove(id);
        Some(removed_connections)
    }

    /// Restore strict connections whose previously missing dynamic port is available again.
    pub fn restore_tombstoned_connections(&mut self, port_id: &P, cx: &mut Context<Self>) -> usize {
        if self.config.mutation_mode == MutationMode::Controlled {
            let connections = self.restorable_tombstone_connections(port_id);
            if !connections.is_empty() {
                cx.emit(core::GraphEvent::MutationRequested {
                    mutations: connections
                        .iter()
                        .cloned()
                        .map(|connection| core::GraphMutation::RestoreConnection { connection })
                        .collect(),
                });
            }
            return connections.len();
        }
        let restored = self.restore_tombstones_for_port(port_id);
        for id in &restored {
            cx.emit(core::GraphEvent::DanglingConnectionRestored { id: id.clone() });
        }
        if !restored.is_empty() {
            cx.notify();
        }
        restored.len()
    }

    fn restorable_tombstone_connections(&self, port_id: &P) -> Vec<Connection<P, C>> {
        self.dangling_connections
            .iter()
            .filter(|connection| &connection.missing_port == port_id)
            .filter_map(|connection| {
                let source = self.graph.ports.get(&connection.source)?;
                let target = self.graph.ports.get(&connection.target)?;
                (source.direction == PortDirection::Output
                    && target.direction == PortDirection::Input
                    && source.node != target.node
                    && T::compatible(&source.kind, &target.kind))
                .then(|| Connection {
                    id: connection.id.clone(),
                    source: connection.source.clone(),
                    target: connection.target.clone(),
                })
            })
            .collect()
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
    pub fn active_overlay_count(&self) -> usize {
        self.active_dismissible_overlays.len()
    }

    pub fn catalog_is_open(&self) -> bool {
        self.catalog_menu.is_some()
    }
    pub fn has_active_draft(&self) -> bool {
        self.draft.is_some()
    }

    pub fn catalog_connects_draft(&self) -> bool {
        self.catalog_menu
            .as_ref()
            .is_some_and(|menu| menu.connect_from.is_some())
    }

    pub fn catalog_entry_count(&self) -> usize {
        self.filtered_catalog_entries().len()
    }
    pub fn catalog_selected_entry(&self) -> Option<usize> {
        self.catalog_menu.as_ref().map(|menu| menu.selected)
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

    fn compatible_catalog_port_indices(&self, item_index: usize, origin: &P) -> Vec<usize> {
        let Some(origin) = self.graph.ports.get(origin) else {
            return Vec::new();
        };
        let Some(item) = self.catalog.get(item_index) else {
            return Vec::new();
        };
        item.ports
            .iter()
            .enumerate()
            .filter_map(|(port_index, candidate)| {
                let compatible = match (origin.direction, candidate.direction) {
                    (PortDirection::Output, PortDirection::Input) => {
                        T::compatible(&origin.kind, &candidate.kind)
                    }
                    (PortDirection::Input, PortDirection::Output) => {
                        T::compatible(&candidate.kind, &origin.kind)
                    }
                    _ => false,
                };
                compatible.then_some(port_index)
            })
            .collect()
    }

    fn compatible_catalog_port(&self, item_index: usize, origin: &P) -> Option<&CatalogPort<T>> {
        let port_index = self
            .compatible_catalog_port_indices(item_index, origin)
            .into_iter()
            .next()?;
        self.catalog.get(item_index)?.ports.get(port_index)
    }

    fn filtered_catalog_indices(&self) -> Vec<usize> {
        let Some(menu) = self.catalog_menu.as_ref() else {
            return Vec::new();
        };
        let query = menu.query.text.to_lowercase();
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

    fn scroll_selected_catalog_item_into_view(&self) {
        let Some(menu) = self.catalog_menu.as_ref() else {
            return;
        };
        let filtered = self.filtered_catalog_indices();
        let mut previous_category: Option<&str> = None;
        let mut child_index = 0;
        let mut entry_index = 0;
        for item_index in filtered {
            let category = self.catalog[item_index].category.as_str();
            if previous_category != Some(category) {
                child_index += 1;
                previous_category = Some(category);
            }
            let compatible = menu
                .connect_from
                .as_ref()
                .map(|origin| self.compatible_catalog_port_indices(item_index, origin))
                .unwrap_or_default();
            let consumed = compatible.len().max(1);
            if menu.selected >= entry_index && menu.selected < entry_index + consumed {
                let selected_child = if menu.connect_from.is_some() {
                    // During a draft the node row is only a label; selection always
                    // belongs to one of the compatible pin rows beneath it.
                    child_index + 1 + (menu.selected - entry_index)
                } else {
                    child_index
                };
                self.catalog_scroll_handle.scroll_to_item(selected_child);
                return;
            }
            entry_index += consumed;
            child_index += if menu.connect_from.is_some() {
                1 + compatible.len()
            } else {
                1
            };
        }
    }

    fn filtered_catalog_entries(&self) -> Vec<(usize, Option<usize>)> {
        let Some(menu) = self.catalog_menu.as_ref() else {
            return Vec::new();
        };
        self.filtered_catalog_indices()
            .into_iter()
            .flat_map(|item_index| {
                if let Some(origin) = menu.connect_from.as_ref() {
                    self.compatible_catalog_port_indices(item_index, origin)
                        .into_iter()
                        .map(|port_index| (item_index, Some(port_index)))
                        .collect()
                } else {
                    vec![(item_index, None)]
                }
            })
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
            query: WorldTextInputState::at_end(String::new()),
            selected: 0,
            connect_from,
        });
    }

    fn catalog_creation_event(
        &self,
        item_index: usize,
        port_index: Option<usize>,
    ) -> Option<core::GraphEvent<N, P, C, T>> {
        let menu = self.catalog_menu.as_ref()?;
        let item = self.catalog.get(item_index)?;
        let (connect_to, connect_direction) = if let Some(origin_id) = menu.connect_from.as_ref() {
            let origin = self.graph.ports.get(origin_id)?;
            let port_index = port_index?;
            if !self
                .compatible_catalog_port_indices(item_index, origin_id)
                .contains(&port_index)
            {
                return None;
            }
            let port = item.ports.get(port_index)?;
            (Some(port.id.clone()), Some(origin.direction))
        } else {
            if port_index.is_some() {
                return None;
            }
            (None, None)
        };
        Some(core::GraphEvent::CreateNode {
            item_id: item.id.clone(),
            position: menu.anchor_world,
            connect_from: menu.connect_from.clone(),
            connect_to,
            connect_direction,
        })
    }

    fn take_catalog_creation_event(
        &mut self,
        item_index: usize,
        port_index: Option<usize>,
    ) -> Option<core::GraphEvent<N, P, C, T>> {
        let event = self.catalog_creation_event(item_index, port_index)?;
        self.catalog_menu = None;
        Some(event)
    }

    fn choose_catalog(
        &mut self,
        item_index: usize,
        port_index: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        let Some(event) = self.take_catalog_creation_event(item_index, port_index) else {
            return;
        };
        cx.emit(event);
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
        self.pending_port_changes.clear();
        self.auto_width_nodes
            .retain(|node| self.graph.nodes.contains_key(node));
        self.render_geometry
            .node_sizes
            .retain(|node, _| self.graph.nodes.contains_key(node));
        self.render_geometry.port_offsets.retain(|port, (node, _)| {
            self.graph.ports.contains_key(port) && self.graph.nodes.contains_key(node)
        });
        self.defined_port_order.retain(|node, ports| {
            if !self.graph.nodes.contains_key(node) {
                return false;
            }
            ports.retain(|port| self.graph.ports.contains_key(port));
            true
        });
        self.port_presentations
            .retain(|port, _| self.graph.ports.contains_key(port));
        self.prune_resolved_tombstones();
        self.cancel_gestures();
        cx.emit(core::GraphEvent::GraphReconciled);
        self.emit_selection(cx);
        cx.emit(core::GraphEvent::ViewportChanged {
            viewport: self.graph.viewport,
        });
        self.node_type_registry_error = self.refresh_node_types(cx).err();
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
        self.pending_port_changes.clear();
        if self
            .draft
            .as_ref()
            .is_some_and(|draft| !self.graph.ports.contains_key(&draft.origin))
        {
            self.draft = None;
        }
        self.drag = None;
        self.box_selection = None;
        self.auto_width_nodes
            .retain(|node| self.graph.nodes.contains_key(node));
        self.render_geometry
            .node_sizes
            .retain(|node, _| self.graph.nodes.contains_key(node));
        self.render_geometry.port_offsets.retain(|port, (node, _)| {
            self.graph.ports.contains_key(port) && self.graph.nodes.contains_key(node)
        });
        self.defined_port_order.retain(|node, ports| {
            if !self.graph.nodes.contains_key(node) {
                return false;
            }
            ports.retain(|port| self.graph.ports.contains_key(port));
            true
        });
        self.port_presentations
            .retain(|port, _| self.graph.ports.contains_key(port));
        self.prune_resolved_tombstones();
        for event in events {
            cx.emit(event);
        }
        self.node_type_registry_error = self.refresh_node_types(cx).err();
        cx.notify();
        Ok(())
    }

    fn prune_resolved_tombstones(&mut self) {
        self.dangling_connections
            .retain(|connection| !self.graph.connections.contains_key(&connection.id));
    }

    fn node_resize_bounds(&self, theme: &NodeGraphTheme) -> (f32, f32) {
        let minimum = self
            .config
            .min_node_width
            .max(theme.node.min_width)
            .max(theme.node.resize_min_width);
        let maximum = theme
            .node
            .resize_max_width
            .unwrap_or(self.config.max_node_width)
            .min(self.config.max_node_width)
            .max(minimum);
        (minimum, maximum)
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

    fn handle_scroll_wheel(
        &mut self,
        event: &ScrollWheelEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(window.line_height());
        let delta_y = f32::from(delta.y);
        if delta_y.abs() <= f32::EPSILON {
            return;
        }
        let factor = if delta_y < 0.0 {
            self.config.zoom_step.exp()
        } else {
            (-self.config.zoom_step).exp()
        };
        let local = self.local_screen(event.position);
        let previous = self.graph.viewport;
        self.graph
            .viewport
            .zoom_at(local, factor, self.config.min_zoom, self.config.max_zoom);
        if self.graph.viewport != previous {
            cx.emit(core::GraphEvent::ViewportChanged {
                viewport: self.graph.viewport,
            });
            cx.notify();
        }
        cx.stop_propagation();
        window.prevent_default();
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

    fn node_at_screen(&self, screen: core::Point) -> Option<N> {
        let world = self.graph.viewport.screen_to_world(screen);
        let mut nodes: Vec<_> = self.graph.nodes.values().collect();
        nodes.sort_by_cached_key(|node| format!("{:?}", node.id));
        nodes.into_iter().rev().find_map(|node| {
            core::Rect {
                origin: node.position,
                size: self.resolved_node_size(&node.id).unwrap_or(node.size),
            }
            .contains(world)
            .then(|| node.id.clone())
        })
    }

    fn reset_node_width(&mut self, id: &N, theme: &NodeGraphTheme, cx: &mut Context<Self>) {
        if !theme.node.resizable || !self.graph.nodes.contains_key(id) {
            return;
        }
        self.auto_width_nodes.insert(id.clone());
        self.render_geometry.node_sizes.remove(id);
        if let Some(size) = self.resolved_node_size(id) {
            cx.emit(core::GraphEvent::NodeResized {
                id: id.clone(),
                size,
            });
        }
        cx.notify();
    }

    fn begin_node_resize(&mut self, id: &N, screen_x: f32, theme: &NodeGraphTheme) {
        if !theme.node.resizable {
            return;
        }
        self.auto_width_nodes.remove(id);
        let Some(node) = self.graph.nodes.get(id) else {
            return;
        };
        let start_size = node.size;
        let start_ports = self
            .graph
            .ports
            .iter()
            .filter(|(_, port)| port.node == *id)
            .map(|(id, port)| (id.clone(), port.position))
            .collect();
        self.drag = None;
        self.resize = Some(ResizeDrag {
            id: id.clone(),
            start_screen_x: screen_x,
            start_size,
            start_ports,
            moved: false,
        });
    }

    fn begin_node_drag(
        &mut self,
        id: &N,
        local: core::Point,
        shift: bool,
        alt: bool,
        cx: &mut Context<Self>,
    ) {
        self.commit_group_editor(cx);
        let cursor = self.graph.viewport.screen_to_world(local);
        let before = (
            self.graph.selected_nodes.clone(),
            self.graph.selected_connections.clone(),
        );
        if shift {
            if !self.graph.selected_nodes.remove(id) {
                self.graph.selected_nodes.insert(id.clone());
            }
        } else if !self.graph.selected_nodes.contains(id) {
            self.graph.selected_nodes.clear();
            self.graph.selected_connections.clear();
            self.graph.selected_nodes.insert(id.clone());
        } else {
            self.graph.selected_connections.clear();
        }
        if before
            != (
                self.graph.selected_nodes.clone(),
                self.graph.selected_connections.clone(),
            )
        {
            self.emit_selection(cx);
        }
        if self.graph.selected_nodes.contains(id) {
            let selected: Vec<_> = self
                .graph
                .selected_nodes
                .iter()
                .filter_map(|selected_id| {
                    let node = self.graph.nodes.get(selected_id)?;
                    Some((selected_id.clone(), cursor - node.position, node.position))
                })
                .collect();
            self.drag = Some(NodeDrag {
                primary: id.clone(),
                offsets: selected
                    .iter()
                    .map(|(id, offset, _)| (id.clone(), *offset))
                    .collect(),
                starts: selected
                    .into_iter()
                    .map(|(id, _, position)| (id, position))
                    .collect(),
                moved: false,
                alter_groups: alt,
            });
        }
        if alt && self.drag.is_some() {
            self.detach_node_from_groups_for_alt_drag(id, cx);
        }
        cx.notify();
    }

    fn port_at_screen(&self, screen: core::Point, radius: f32) -> Option<P> {
        self.graph
            .ports
            .keys()
            .filter_map(|id| {
                let position = self.resolved_port_position(id)?;
                let screen_position = self.graph.viewport.world_to_screen(position);
                let distance = screen_position.distance(screen);
                (distance <= radius).then(|| (id.clone(), distance))
            })
            .min_by(|(_, left), (_, right)| left.total_cmp(right))
            .map(|(id, _)| id)
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

    /// Preserve an in-flight click draft when another port is pressed; a compatible
    /// target becomes the pending snap instead of replacing the source draft.
    fn engage_port(&mut self, id: &P, cx: &mut Context<Self>) {
        self.commit_group_editor(cx);
        if let Some(origin) = self.draft.as_ref().map(|draft| draft.origin.clone()) {
            let compatible = origin != *id && self.normalized_connection(&origin, id).is_some();
            let screen = self
                .resolved_port_position(id)
                .map(|position| self.graph.viewport.world_to_screen(position));
            if let Some(draft) = self.draft.as_mut() {
                draft.snap_target = compatible.then(|| id.clone());
                if let Some(screen) = screen {
                    draft.current_screen = screen;
                }
            }
            cx.notify();
            return;
        }
        self.start_draft(id, cx);
    }

    fn start_draft(&mut self, id: &P, cx: &mut Context<Self>) {
        let Some(direction) = self.graph.ports.get(id).map(|port| port.direction) else {
            return;
        };
        let mut origin = id.clone();
        let mut detached_connection = None;
        // Match the reference reroute gesture: grabbing an occupied input removes
        // its edge immediately and continues the draft from the old output.
        if direction == PortDirection::Input
            && let Some((connection_id, source)) = self
                .graph
                .connections
                .iter()
                .find(|(_, connection)| &connection.target == id)
                .map(|(connection_id, connection)| {
                    (connection_id.clone(), connection.source.clone())
                })
        {
            origin = source;
            if self.config.mutation_mode == MutationMode::Controlled {
                detached_connection = Some(connection_id.clone());
                cx.emit(core::GraphEvent::MutationRequested {
                    mutations: vec![core::GraphMutation::RemoveConnection { id: connection_id }],
                });
            } else {
                self.graph.connections.remove(&connection_id);
                self.graph.selected_connections.remove(&connection_id);
                cx.emit(core::GraphEvent::ConnectionRemoved { id: connection_id });
            }
        }
        let current_screen = self.graph.viewport.world_to_screen(
            self.resolved_port_position(&origin)
                .unwrap_or(self.graph.ports[&origin].position),
        );
        self.draft = Some(DraftConnection {
            origin,
            current_screen,
            snap_target: None,
            moved: false,
            detached_connection,
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
            cx.emit(core::GraphEvent::MutationRequested {
                mutations: vec![core::GraphMutation::RequestConnection { source, target }],
            });
        } else {
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
                        &options,
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

        // Subway routing is deliberately solved as one batch. The core router
        // can then price crossings and occupied corridors globally and assign
        // stable parallel lanes, rather than shifting independently solved
        // polylines after the fact.
        if let RoutingMode::Subway(options) = self.config.routing {
            let mut nodes: Vec<_> = self.graph.nodes.values().collect();
            nodes.sort_by_cached_key(|node| format!("{:?}", node.id));
            let obstacles: Vec<_> = nodes
                .iter()
                .map(|node| core::Rect {
                    origin: node.position,
                    size: self.resolved_node_size(&node.id).unwrap_or(node.size),
                })
                .collect();
            let mut ids = Vec::new();
            let mut batch = Vec::new();
            for connection in &connections {
                let Some(source) = self.graph.ports.get(&connection.source) else {
                    continue;
                };
                let Some(target) = self.graph.ports.get(&connection.target) else {
                    continue;
                };
                let Some(start) = self.resolved_port_position(&connection.source) else {
                    continue;
                };
                let Some(end) = self.resolved_port_position(&connection.target) else {
                    continue;
                };
                ids.push(connection.id.clone());
                batch.push(core::subway::SubwayConnection {
                    start,
                    end,
                    start_obstacle: nodes.iter().position(|node| node.id == source.node),
                    end_obstacle: nodes.iter().position(|node| node.id == target.node),
                });
            }
            let routed = core::subway::compute_subway_routes(&obstacles, &batch, &options);
            let routes: HashMap<C, Vec<core::Point>> = ids.into_iter().zip(routed).collect();
            let mut cache = self.route_cache.borrow_mut();
            cache.fingerprint = fingerprint;
            cache.routes = routes.clone();
            cache.generation = cache.generation.saturating_add(1);
            return routes;
        }

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

    fn group_label_at(&self, screen: core::Point, theme: &NodeGraphTheme) -> Option<String> {
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
            let group_origin = viewport.world_to_screen(core::Point::new(
                left - theme.group.padding_x,
                top - theme.group.padding_top,
            ));
            let origin = core::Point::new(
                group_origin.x + viewport.scale_length(theme.group.label_left),
                group_origin.y + viewport.scale_length(theme.group.label_top),
            );
            let Some(label) = self
                .group_editor
                .as_ref()
                .filter(|editor| editor.id == group.id)
                .map(|editor| editor.query.text.as_str())
                .or(group.label.as_deref())
            else {
                continue;
            };
            let font_size = viewport.scale_length(theme.group.label_font_size);
            let width = (label.chars().count() as f32 * font_size * 0.65 + 12.0).max(48.0);
            let height = (font_size + 6.0).max(18.0);
            if screen.x >= origin.x
                && screen.x <= origin.x + width
                && screen.y >= origin.y
                && screen.y <= origin.y + height
            {
                return Some(group.id.clone());
            }
        }
        None
    }

    fn render_bounds(&self) -> Option<core::Rect> {
        let mut nodes = self.graph.nodes.values();
        let first = nodes.next()?;
        let first_size = self.resolved_node_size(&first.id).unwrap_or(first.size);
        let (mut left, mut top, mut right, mut bottom) = (
            first.position.x,
            first.position.y,
            first.position.x + first_size.width,
            first.position.y + first_size.height,
        );
        for node in nodes {
            let size = self.resolved_node_size(&node.id).unwrap_or(node.size);
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

    fn detach_node_from_groups_for_alt_drag(&mut self, node_id: &N, cx: &mut Context<Self>) {
        let mut changes = Vec::new();
        for group in &mut self.groups {
            if group.nodes.contains(node_id) {
                let mut node_ids: Vec<_> = group
                    .nodes
                    .iter()
                    .filter(|id| *id != node_id)
                    .cloned()
                    .collect();
                node_ids.sort_by_cached_key(|id| format!("{id:?}"));
                changes.push((group.id.clone(), node_ids));
                if self.config.mutation_mode == MutationMode::Uncontrolled {
                    group.nodes.remove(node_id);
                }
            }
        }
        if self.config.mutation_mode == MutationMode::Controlled {
            if !changes.is_empty() {
                cx.emit(core::GraphEvent::MutationRequested {
                    mutations: changes
                        .into_iter()
                        .map(
                            |(group_id, node_ids)| core::GraphMutation::SetGroupMembership {
                                group_id,
                                node_ids,
                            },
                        )
                        .collect(),
                });
            }
        } else {
            for (group_id, node_ids) in changes {
                cx.emit(core::GraphEvent::GroupMembershipChanged { group_id, node_ids });
            }
        }
    }

    fn update_group_memberships(
        &mut self,
        dragged: &[N],
        theme: &NodeGraphTheme,
    ) -> Vec<(String, Vec<N>)> {
        let dragged_set: HashSet<_> = dragged.iter().cloned().collect();
        let mut changes = Vec::new();
        let mut assigned_group = false;
        for group in &mut self.groups {
            let mut members = group
                .nodes
                .iter()
                .filter(|id| !dragged_set.contains(*id))
                .filter_map(|id| self.graph.nodes.get(id));
            let Some(first) = members.next() else {
                let before = group.nodes.len();
                group.nodes.retain(|id| !dragged_set.contains(id));
                if group.nodes.len() != before {
                    let mut node_ids: Vec<_> = group.nodes.iter().cloned().collect();
                    node_ids.sort_by_cached_key(|id| format!("{id:?}"));
                    changes.push((group.id.clone(), node_ids));
                }
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
            let horizontal_padding = theme.group.padding_x;
            let top_padding = theme.group.padding_top;
            let bottom_padding = theme.group.padding_bottom;
            let mut changed = false;
            for id in dragged {
                let Some(node) = self.graph.nodes.get(id) else {
                    continue;
                };
                let center = core::Point::new(
                    node.position.x + node.size.width * 0.5,
                    node.position.y + node.size.height * 0.5,
                );
                let inside = !assigned_group
                    && center.x >= left - horizontal_padding
                    && center.x <= right + horizontal_padding
                    && center.y >= top - top_padding
                    && center.y <= bottom + bottom_padding;
                changed |= if inside {
                    assigned_group = true;
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

    fn finalize_node_drag(
        &mut self,
        drag: &NodeDrag<N>,
        theme: &NodeGraphTheme,
    ) -> DragCompletion<N> {
        let nodes: Vec<_> = drag
            .offsets
            .iter()
            .filter_map(|(id, _)| Some((id.clone(), self.graph.nodes.get(id)?.position)))
            .collect();
        let previous_groups = self.groups.clone();
        let group_changes = if drag.alter_groups {
            self.update_group_memberships(std::slice::from_ref(&drag.primary), theme)
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

    fn open_anchor_menu(&mut self, port_id: P, position: core::Point, cx: &mut Context<Self>) {
        self.commit_group_editor(cx);
        let items = self.anchor_menu_items(&port_id);
        self.anchor_menu = (!items.is_empty()).then_some(ActiveAnchorMenu {
            port: port_id,
            position,
            items,
        });
        self.catalog_menu = None;
        cx.notify();
    }

    fn execute_anchor_menu_item(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(menu) = self.anchor_menu.take() else {
            return;
        };
        let Some(item) = menu.items.get(index).filter(|item| item.enabled).cloned() else {
            cx.notify();
            return;
        };
        let broken_only = match item.action {
            AnchorMenuAction::Custom(callback) => {
                callback();
                cx.notify();
                return;
            }
            AnchorMenuAction::RemoveConnections => false,
            AnchorMenuAction::RemoveBrokenConnections => true,
        };
        let mut strict_ids: Vec<C> = if broken_only {
            Vec::new()
        } else {
            self.graph
                .connections
                .values()
                .filter(|connection| {
                    connection.source == menu.port || connection.target == menu.port
                })
                .map(|connection| connection.id.clone())
                .collect()
        };
        let mut dangling_ids: Vec<C> = self
            .dangling_connections
            .iter()
            .filter(|connection| connection.source == menu.port || connection.target == menu.port)
            .map(|connection| connection.id.clone())
            .collect();
        strict_ids.sort_by_cached_key(|id| format!("{id:?}"));
        strict_ids.dedup();
        dangling_ids.sort_by_cached_key(|id| format!("{id:?}"));
        dangling_ids.dedup();
        if self.config.mutation_mode == MutationMode::Controlled {
            if !strict_ids.is_empty() {
                cx.emit(core::GraphEvent::MutationRequested {
                    mutations: strict_ids
                        .into_iter()
                        .map(|id| core::GraphMutation::RemoveConnection { id })
                        .collect(),
                });
            }
            for id in dangling_ids {
                self.dangling_connections
                    .retain(|connection| connection.id != id);
                self.graph.selected_connections.remove(&id);
                cx.emit(core::GraphEvent::ConnectionRemoved { id });
            }
        } else {
            strict_ids.extend(dangling_ids);
            strict_ids.sort_by_cached_key(|id| format!("{id:?}"));
            strict_ids.dedup();
            for id in strict_ids {
                let removed = self.graph.connections.remove(&id).is_some();
                let previous_len = self.dangling_connections.len();
                self.dangling_connections
                    .retain(|connection| connection.id != id);
                if removed || self.dangling_connections.len() != previous_len {
                    self.graph.selected_connections.remove(&id);
                    cx.emit(core::GraphEvent::ConnectionRemoved { id });
                }
            }
        }
        cx.notify();
    }

    fn dismiss_overlay_ids(
        &mut self,
        ids: impl IntoIterator<Item = String>,
        cx: &mut Context<Self>,
    ) {
        let mut changed = false;
        for id in ids {
            changed |= self.dismissed_overlays.insert(id.clone());
            self.active_dismissible_overlays.remove(&id);
            self.active_escape_overlays.remove(&id);
            self.active_outside_overlays.remove(&id);
            self.active_backdrop_overlays.remove(&id);
            if let Some(callback) = self.active_overlay_dismiss_callbacks.remove(&id) {
                callback();
            }
            cx.emit(core::GraphEvent::NodeOverlayDismissed { id });
        }
        if changed {
            cx.notify();
        }
    }

    fn dismiss_escape_overlays(&mut self, cx: &mut Context<Self>) {
        let ids = std::mem::take(&mut self.active_escape_overlays);
        self.dismiss_overlay_ids(ids, cx);
    }

    fn dismiss_outside_overlays(&mut self, cx: &mut Context<Self>) {
        let ids = std::mem::take(&mut self.active_outside_overlays);
        self.dismiss_overlay_ids(ids, cx);
    }

    fn dismiss_overlays_before_world_pointer(
        &mut self,
        local: core::Point,
        cx: &mut Context<Self>,
    ) {
        if self.active_outside_overlays.is_empty()
            || self.active_overlay_bounds.iter().any(|bounds| {
                local.x >= bounds.origin.x
                    && local.y >= bounds.origin.y
                    && local.x <= bounds.origin.x + bounds.size.width
                    && local.y <= bounds.origin.y + bounds.size.height
            })
        {
            return;
        }
        self.dismiss_outside_overlays(cx);
    }

    fn begin_canvas_selection(
        &mut self,
        local: core::Point,
        shift: bool,
        theme: &NodeGraphTheme,
        cx: &mut Context<Self>,
    ) {
        self.commit_group_editor(cx);
        self.catalog_menu = None;
        self.draft = None;
        if !self.active_outside_overlays.is_empty() {
            self.dismiss_outside_overlays(cx);
        }
        let before = (
            self.graph.selected_nodes.clone(),
            self.graph.selected_connections.clone(),
        );
        if let Some(connection) = self.connection_at(
            local,
            self.graph
                .viewport
                .scale_length(theme.connection.stroke_width * 0.5),
        ) {
            if shift {
                if !self.graph.selected_connections.remove(&connection) {
                    self.graph.selected_connections.insert(connection);
                }
            } else {
                self.graph.selected_nodes.clear();
                self.graph.selected_connections.clear();
                self.graph.selected_connections.insert(connection);
            }
            self.box_selection = None;
        } else {
            let start = self.graph.viewport.screen_to_world(local);
            let (baseline_nodes, baseline_connections) = if shift {
                // Match the reference: Shift preserves the existing selection on a blank
                // click, but once the marquee moves its contents replace selected nodes.
                (HashSet::new(), self.graph.selected_connections.clone())
            } else {
                self.graph.selected_nodes.clear();
                self.graph.selected_connections.clear();
                (HashSet::new(), HashSet::new())
            };
            self.box_selection = Some(BoxSelection {
                start,
                current: start,
                baseline_nodes,
                baseline_connections,
            });
        }
        if before
            != (
                self.graph.selected_nodes.clone(),
                self.graph.selected_connections.clone(),
            )
        {
            self.emit_selection(cx);
        }
        cx.notify();
    }

    fn handle_pointer_move(
        &mut self,
        local: core::Point,
        theme: &NodeGraphTheme,
        cx: &mut Context<Self>,
    ) {
        self.last_pointer_screen = Some(local);
        let hovered_port = self.port_at_screen(
            local,
            self.graph
                .viewport
                .scale_length(theme.anchor.dot_size * 0.5),
        );
        if self.hovered_port != hovered_port {
            self.hovered_port = hovered_port;
        }
        // Hover changes group and anchor semantics even when no drag is active.
        cx.notify();
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
            let width = (resize.start_size.width + delta).clamp(
                self.node_resize_bounds(theme).0,
                self.node_resize_bounds(theme).1,
            );
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
                draft.moved |= draft.current_screen.distance(local) > 0.5;
                draft.current_screen = local;
                draft.snap_target = snap_target;
            }
            cx.notify();
        }
    }

    fn finish_left_gesture(
        &mut self,
        preserve_click_draft: bool,
        theme: &NodeGraphTheme,
        cx: &mut Context<Self>,
    ) {
        let was_panning = self.panning.take().is_some();
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
        if let Some(drag) = self.drag.take()
            && (drag.moved || drag.alter_groups)
        {
            let completion = self.finalize_node_drag(&drag, theme);
            if self.config.mutation_mode == MutationMode::Controlled {
                let mut mutations = Vec::new();
                if drag.moved {
                    mutations.push(core::GraphMutation::MoveNodes {
                        nodes: completion.nodes,
                    });
                }
                mutations.extend(completion.group_changes.into_iter().map(
                    |(group_id, node_ids)| core::GraphMutation::SetGroupMembership {
                        group_id,
                        node_ids,
                    },
                ));
                if !mutations.is_empty() {
                    cx.emit(core::GraphEvent::MutationRequested { mutations });
                }
            } else {
                if drag.moved {
                    cx.emit(core::GraphEvent::NodesMoved {
                        nodes: completion.nodes,
                    });
                }
                for (group_id, node_ids) in completion.group_changes {
                    cx.emit(core::GraphEvent::GroupMembershipChanged { group_id, node_ids });
                }
            }
        }
        self.box_selection = None;
        if was_panning {
            return;
        }
        if let Some(target) = self
            .draft
            .as_ref()
            .and_then(|draft| draft.snap_target.clone())
        {
            self.finish_draft(&target, cx);
        } else if !preserve_click_draft
            && self.draft.as_ref().is_some_and(|draft| draft.moved)
            && self.draft.take().is_some()
        {
            cx.notify();
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

    fn commit_group_editor(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.group_editor.take() else {
            return;
        };
        let label = editor.query.text.trim();
        if label.is_empty() {
            return;
        }
        let label = label.to_string();
        if self.config.mutation_mode == MutationMode::Controlled {
            cx.emit(core::GraphEvent::MutationRequested {
                mutations: vec![core::GraphMutation::SetGroupLabel {
                    group_id: editor.id,
                    label,
                }],
            });
        } else if let Some(group) = self.groups.iter_mut().find(|group| group.id == editor.id) {
            group.label = Some(label.clone());
            cx.emit(core::GraphEvent::GroupLabelChanged {
                group_id: group.id.clone(),
                label,
            });
        }
        cx.notify();
    }

    fn inline_text_state(&self) -> Option<&WorldTextInputState> {
        self.catalog_menu
            .as_ref()
            .map(|menu| &menu.query)
            .or_else(|| self.group_editor.as_ref().map(|editor| &editor.query))
    }

    fn inline_text_state_mut(&mut self) -> Option<&mut WorldTextInputState> {
        if let Some(menu) = self.catalog_menu.as_mut() {
            Some(&mut menu.query)
        } else {
            self.group_editor.as_mut().map(|editor| &mut editor.query)
        }
    }

    fn edit_inline_text_key(
        &mut self,
        key: &str,
        character: Option<&str>,
        command: bool,
        shift: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if command && key == "c" {
            if let Some(text) = self.inline_text_state().and_then(selected_text) {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            }
            return true;
        }
        let paste = (command && key == "v")
            .then(|| cx.read_from_clipboard().and_then(|item| item.text()))
            .flatten();
        let cut = if command && key == "x" {
            self.inline_text_state().and_then(selected_text)
        } else {
            None
        };
        if let Some(text) = cut.as_ref() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.clone()));
        }
        let Some(state) = self.inline_text_state_mut() else {
            return false;
        };
        let before = state.clone();
        match key {
            "left" => move_text_cursor(state, false, shift),
            "right" => move_text_cursor(state, true, shift),
            "home" => move_text_selection(state, 0, shift),
            "end" => move_text_selection(state, utf16_len(&state.text), shift),
            "backspace" => {
                let range = if state.selection.is_empty() {
                    let end = state.selection.end;
                    let start = text_boundaries(&state.text)
                        .into_iter()
                        .rev()
                        .find(|offset| *offset < end)
                        .unwrap_or(0);
                    start..end
                } else {
                    state.selection.clone()
                };
                replace_text_state(state, Some(range), "", None);
            }
            "delete" => {
                let range = if state.selection.is_empty() {
                    let start = state.selection.end;
                    let end = text_boundaries(&state.text)
                        .into_iter()
                        .find(|offset| *offset > start)
                        .unwrap_or(start);
                    start..end
                } else {
                    state.selection.clone()
                };
                replace_text_state(state, Some(range), "", None);
            }
            "a" if command => {
                state.selection = 0..utf16_len(&state.text);
                state.selection_reversed = false;
            }
            "v" if command => {
                if let Some(text) = paste {
                    replace_text_state(state, None, &text, None);
                }
            }
            "x" if command => replace_text_state(state, None, "", None),
            _ if !command => {
                let Some(character) = character
                    .filter(|text| !text.is_empty() && !text.chars().any(char::is_control))
                else {
                    return false;
                };
                replace_text_state(state, None, character, None);
            }
            _ => return false,
        }
        let changed = state.text != before.text;
        if changed && let Some(menu) = self.catalog_menu.as_mut() {
            menu.selected = 0;
        }
        cx.notify();
        true
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
        if let Some((node_id, control_id)) = self.last_world_control.clone() {
            cx.emit(core::GraphEvent::NodeControlKeyDown {
                node_id: node_id.clone(),
                control_id: control_id.clone(),
                key: key.to_string(),
                text: if command && key == "v" && self.world_text_input.is_none() {
                    cx.read_from_clipboard().and_then(|item| item.text())
                } else {
                    event.keystroke.key_char.clone()
                },
                shift,
                command,
            });
            if key == "tab" {
                self.blur_world_control(cx);
                if !self.world_control_order.is_empty() {
                    let current =
                        self.world_control_order
                            .iter()
                            .position(|(entry_node, entry_control)| {
                                entry_node == &node_id && entry_control == &control_id
                            });
                    let next = match (current, shift) {
                        (Some(index), true) => {
                            (index + self.world_control_order.len() - 1)
                                % self.world_control_order.len()
                        }
                        (Some(index), false) => (index + 1) % self.world_control_order.len(),
                        (None, true) => self.world_control_order.len() - 1,
                        (None, false) => 0,
                    };
                    let focused = self.world_control_order[next].clone();
                    self.last_world_control = Some(focused.clone());
                    cx.emit(core::GraphEvent::NodeControlFocused {
                        node_id: focused.0,
                        control_id: focused.1,
                    });
                }
            }
            cx.notify();
            let platform_text_key = self.world_text_input.is_some()
                && ((!command && event.keystroke.key_char.is_some())
                    || (command && key == "v")
                    || matches!(key, "process" | "unidentified" | "dead"));
            if !platform_text_key {
                cx.stop_propagation();
                window.prevent_default();
            }
            return;
        }
        if self.catalog_menu.is_some() {
            let before = self.filtered_catalog_entries();
            let text_handled = match key {
                "escape" | "tab" => {
                    self.catalog_menu = None;
                    false
                }
                "enter" => {
                    if let Some(menu) = self.catalog_menu.as_ref()
                        && let Some((item_index, port_index)) = before.get(menu.selected).copied()
                    {
                        self.choose_catalog(item_index, port_index, cx);
                    }
                    false
                }
                "up" => {
                    if let Some(menu) = self.catalog_menu.as_mut() {
                        menu.selected = menu.selected.saturating_sub(1);
                    }
                    false
                }
                "down" => {
                    if let Some(menu) = self.catalog_menu.as_mut() {
                        menu.selected = (menu.selected + 1).min(before.len().saturating_sub(1));
                    }
                    false
                }
                _ => self.edit_inline_text_key(
                    key,
                    event.keystroke.key_char.as_deref(),
                    command,
                    shift,
                    cx,
                ),
            };
            let result_len = self.filtered_catalog_entries().len();
            if let Some(menu) = self.catalog_menu.as_mut() {
                menu.selected = menu.selected.min(result_len.saturating_sub(1));
            }
            self.scroll_selected_catalog_item_into_view();
            cx.notify();
            let platform_text_key =
                !text_handled && matches!(key, "process" | "unidentified" | "dead");
            if !platform_text_key {
                cx.stop_propagation();
                window.prevent_default();
            }
            return;
        }

        if self.group_editor.is_some() {
            let text_handled = match key {
                "escape" | "enter" => {
                    self.commit_group_editor(cx);
                    false
                }
                _ => self.edit_inline_text_key(
                    key,
                    event.keystroke.key_char.as_deref(),
                    command,
                    shift,
                    cx,
                ),
            };
            cx.notify();
            let platform_text_key =
                !text_handled && matches!(key, "process" | "unidentified" | "dead");
            if !platform_text_key {
                cx.stop_propagation();
                window.prevent_default();
            }
            return;
        }
        match key {
            "tab" if !self.catalog.is_empty() => {
                let bounds = self.canvas_bounds.get();
                let fallback = core::Point::new(
                    f32::from(bounds.size.width) * 0.5,
                    f32::from(bounds.size.height) * 0.5,
                );
                let connect_from = self.draft.as_ref().map(|draft| draft.origin.clone());
                self.open_catalog(self.last_pointer_screen.unwrap_or(fallback), connect_from);
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
                if self.anchor_menu.take().is_some() {
                    cx.notify();
                } else if !self.active_escape_overlays.is_empty() {
                    self.dismiss_escape_overlays(cx);
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
                window.prevent_default();
            }
            "v" if command => {
                cx.emit(core::GraphEvent::NodesPasted {
                    offset: core::Point::new(20.0, 20.0),
                });
                cx.stop_propagation();
                window.prevent_default();
            }
            "z" if command && shift => {
                cx.emit(core::GraphEvent::Redo);
                cx.stop_propagation();
                window.prevent_default();
            }
            "z" if command => {
                cx.emit(core::GraphEvent::Undo);
                cx.stop_propagation();
                window.prevent_default();
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
                window.prevent_default();
            }
            _ => {}
        }
    }
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

fn utf16_to_byte(text: &str, offset: usize) -> (usize, usize) {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        let next = units + ch.len_utf16();
        if offset < next {
            return (byte, units);
        }
        units = next;
    }
    (text.len(), units)
}

fn clamp_world_text_state(state: &mut WorldTextInputState) {
    let len = utf16_len(&state.text);
    state.selection.start = state.selection.start.min(len);
    state.selection.end = state.selection.end.min(len);
    if state.selection.start > state.selection.end {
        std::mem::swap(&mut state.selection.start, &mut state.selection.end);
        state.selection_reversed = !state.selection_reversed;
    }
    if let Some(marked) = &mut state.marked {
        marked.start = marked.start.min(len);
        marked.end = marked.end.min(len);
        if marked.start > marked.end {
            std::mem::swap(&mut marked.start, &mut marked.end);
        }
    }
}

fn replace_text_state(
    state: &mut WorldTextInputState,
    replacement: Option<Range<usize>>,
    text: &str,
    marked_selection: Option<Range<usize>>,
) {
    let requested = replacement
        .or_else(|| state.marked.clone())
        .unwrap_or_else(|| state.selection.clone());
    let (start_byte, start) = utf16_to_byte(&state.text, requested.start);
    let (end_byte, _) = utf16_to_byte(&state.text, requested.end.max(requested.start));
    state.text.replace_range(start_byte..end_byte, text);
    let inserted = utf16_len(text);
    state.marked = marked_selection.as_ref().map(|_| start..start + inserted);
    state.selection = if let Some(relative) = marked_selection {
        (start + relative.start.min(inserted))..(start + relative.end.min(inserted))
    } else {
        (start + inserted)..(start + inserted)
    };
    state.selection_reversed = false;
    clamp_world_text_state(state);
}

fn text_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries = vec![0];
    let mut offset = 0;
    for character in text.chars() {
        offset += character.len_utf16();
        boundaries.push(offset);
    }
    boundaries
}

fn move_text_selection(state: &mut WorldTextInputState, target: usize, extend: bool) {
    let target = target.min(utf16_len(&state.text));
    if !extend {
        state.selection = target..target;
        state.selection_reversed = false;
        return;
    }
    let anchor = if state.selection_reversed {
        state.selection.end
    } else {
        state.selection.start
    };
    state.selection = anchor.min(target)..anchor.max(target);
    state.selection_reversed = target < anchor;
}

fn move_text_cursor(state: &mut WorldTextInputState, forward: bool, extend: bool) {
    let focus = if state.selection_reversed {
        state.selection.start
    } else {
        state.selection.end
    };
    if !extend && !state.selection.is_empty() {
        move_text_selection(
            state,
            if forward {
                state.selection.end
            } else {
                state.selection.start
            },
            false,
        );
        return;
    }
    let boundaries = text_boundaries(&state.text);
    let target = if forward {
        boundaries
            .into_iter()
            .find(|offset| *offset > focus)
            .unwrap_or_else(|| utf16_len(&state.text))
    } else {
        boundaries
            .into_iter()
            .rev()
            .find(|offset| *offset < focus)
            .unwrap_or(0)
    };
    move_text_selection(state, target, extend);
}

fn selected_text(state: &WorldTextInputState) -> Option<String> {
    if state.selection.is_empty() {
        return None;
    }
    let (start, _) = utf16_to_byte(&state.text, state.selection.start);
    let (end, _) = utf16_to_byte(&state.text, state.selection.end);
    Some(state.text[start..end].to_owned())
}

fn text_with_caret(state: &WorldTextInputState) -> String {
    let focus = if state.selection_reversed {
        state.selection.start
    } else {
        state.selection.end
    };
    let (byte, _) = utf16_to_byte(&state.text, focus);
    let mut text = state.text.clone();
    text.insert(byte, '▏');
    text
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> NodeGraph<T, N, P, C> {
    fn active_input_state(&self) -> Option<&WorldTextInputState> {
        self.inline_text_state()
            .or_else(|| self.world_text_input.as_ref().map(|value| &value.2))
    }

    fn active_input_state_mut(&mut self) -> Option<&mut WorldTextInputState> {
        if self.catalog_menu.is_some() || self.group_editor.is_some() {
            self.inline_text_state_mut()
        } else {
            self.world_text_input.as_mut().map(|value| &mut value.2)
        }
    }

    fn replace_active_text(
        &mut self,
        replacement: Option<Range<usize>>,
        text: &str,
        marked_selection: Option<Range<usize>>,
        cx: &mut Context<Self>,
    ) {
        let inline = self.catalog_menu.is_some() || self.group_editor.is_some();
        let Some(state) = self.active_input_state_mut() else {
            return;
        };
        replace_text_state(state, replacement, text, marked_selection);
        if inline {
            if let Some(menu) = self.catalog_menu.as_mut() {
                menu.selected = 0;
            }
            cx.notify();
        } else {
            self.emit_world_text_changed(cx);
        }
    }

    fn active_text_world_rect(&self) -> Option<core::Rect> {
        let (node, control, _) = self.world_text_input.as_ref()?;
        if self.last_world_control.as_ref() != Some(&(node.clone(), control.clone())) {
            return None;
        }
        let hit = self
            .world_scene
            .hit_regions
            .iter()
            .rev()
            .find(|hit| &hit.id == control)?;
        match hit.shape {
            world::HitShape::Rect(rect) => Some(rect),
            world::HitShape::Circle { center, radius } => Some(core::Rect {
                origin: core::Point::new(center.x - radius, center.y - radius),
                size: core::Size {
                    width: radius * 2.0,
                    height: radius * 2.0,
                },
            }),
        }
    }

    fn emit_world_text_changed(&mut self, cx: &mut Context<Self>) {
        if let Some((node_id, control_id, state)) = self.world_text_input.as_ref() {
            cx.emit(core::GraphEvent::NodeControlTextChanged {
                node_id: node_id.clone(),
                control_id: control_id.clone(),
                text: state.text.clone(),
                selection: state.selection.clone(),
                selection_reversed: state.selection_reversed,
                marked: state.marked.clone(),
            });
            cx.notify();
        }
    }

    #[cfg(test)]
    fn replace_world_text(
        &mut self,
        replacement: Option<Range<usize>>,
        text: &str,
        marked_selection: Option<Range<usize>>,
    ) {
        let Some((_, _, state)) = self.world_text_input.as_mut() else {
            return;
        };
        replace_text_state(state, replacement, text, marked_selection);
    }
}

impl<T: PortType, N: core::NodeId, P: core::PortId, C: core::ConnectionId> EntityInputHandler
    for NodeGraph<T, N, P, C>
{
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let state = self.active_input_state()?;
        let (start_byte, start) = utf16_to_byte(&state.text, range.start);
        let (end_byte, end) = utf16_to_byte(&state.text, range.end.max(range.start));
        *adjusted = Some(start..end);
        Some(state.text[start_byte..end_byte].to_owned())
    }
    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let state = self.active_input_state()?;
        Some(UTF16Selection {
            range: state.selection.clone(),
            reversed: state.selection_reversed,
        })
    }
    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.active_input_state()?.marked.clone()
    }
    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let inline = self.catalog_menu.is_some() || self.group_editor.is_some();
        if let Some(state) = self.active_input_state_mut() {
            state.marked = None;
            if inline {
                cx.notify();
            } else {
                self.emit_world_text_changed(cx);
            }
        }
    }
    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_active_text(range, text, None, cx);
    }
    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_active_text(
            range,
            text,
            Some(selected.unwrap_or_else(|| utf16_len(text)..utf16_len(text))),
            cx,
        );
    }
    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        _: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let rect = self.active_text_world_rect()?;
        let canvas = self.canvas_bounds.get();
        let len = self
            .world_text_input
            .as_ref()
            .map(|v| utf16_len(&v.2.text))
            .unwrap_or(0)
            .max(1);
        let start = range.start.min(len) as f32 / len as f32;
        let end = range.end.min(len).max(range.start.min(len)) as f32 / len as f32;
        let a = self.graph.viewport.world_to_screen(core::Point::new(
            rect.origin.x + rect.size.width * start,
            rect.origin.y,
        ));
        Some(Bounds {
            origin: point(canvas.origin.x + px(a.x), canvas.origin.y + px(a.y)),
            size: gpui::Size {
                width: px((rect.size.width * (end - start)).max(1.0) * self.graph.viewport.zoom),
                height: px(rect.size.height * self.graph.viewport.zoom),
            },
        })
    }
    fn character_index_for_point(
        &mut self,
        point: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let rect = self.active_text_world_rect()?;
        let local = self.local_screen(point);
        let world = self.graph.viewport.screen_to_world(local);
        let fraction =
            ((world.x - rect.origin.x) / rect.size.width.max(f32::EPSILON)).clamp(0.0, 1.0);
        Some(
            (fraction * utf16_len(&self.world_text_input.as_ref()?.2.text) as f32).round() as usize,
        )
    }
    fn set_selected_text_range(
        &mut self,
        range: Range<usize>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inline = self.catalog_menu.is_some() || self.group_editor.is_some();
        if let Some(state) = self.active_input_state_mut() {
            state.selection = range;
            state.selection_reversed = false;
            clamp_world_text_state(state);
            if inline {
                cx.notify();
            } else {
                self.emit_world_text_changed(cx);
            }
        }
    }
    fn text_length_utf16(&mut self, _: &mut Window, _: &mut Context<Self>) -> Option<usize> {
        Some(utf16_len(&self.active_input_state()?.text))
    }
    fn accepts_text_input(&self, _: &mut Window, _: &mut Context<Self>) -> bool {
        self.inline_text_state().is_some() || self.active_text_world_rect().is_some()
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
        // Read exactly once for this root render, then share the same immutable aggregate.
        let theme = Arc::clone(cx.node_graph_theme());
        self.refresh_default_render_geometry(&theme);
        let viewport = self.graph.viewport.sanitized();

        let routes = self.connection_routes();
        let mut wires: Vec<_> = self
            .graph
            .connections
            .values()
            .filter(|connection| !self.connection_is_detached(&connection.id))
            .filter_map(|connection| {
                Some((
                    routes.get(&connection.id)?.clone(),
                    self.graph.selected_connections.contains(&connection.id),
                    false,
                ))
            })
            .collect();
        let dangling_stubs = self
            .dangling_connections
            .iter()
            .filter(|connection| !self.graph.connections.contains_key(&connection.id))
            .map(|connection| {
                let (surviving, direction) = if connection.missing_port == connection.source {
                    (
                        self.resolved_port_position(&connection.target)
                            .unwrap_or(connection.target_position),
                        -1.0,
                    )
                } else {
                    (
                        self.resolved_port_position(&connection.source)
                            .unwrap_or(connection.source_position),
                        1.0,
                    )
                };
                let end = core::Point::new(surviving.x + direction * 30.0, surviving.y);
                (vec![surviving, end], viewport.world_to_screen(end))
            })
            .collect::<Vec<_>>();
        let dangling_markers = dangling_stubs
            .iter()
            .map(|(_, marker)| *marker)
            .collect::<Vec<_>>();
        wires.extend(
            dangling_stubs
                .iter()
                .map(|(route, _)| (route.clone(), false, true)),
        );

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
        let wire_color: gpui::Hsla = rgb(theme.connection.stroke.rgb)
            .opacity(theme.connection.stroke.alpha)
            .into();
        let selected_wire_color: gpui::Hsla = rgb(theme.connection.stroke_selected.rgb)
            .opacity(theme.connection.stroke_selected.alpha)
            .into();
        let draft_color: gpui::Hsla = rgb(theme.connection.stroke_draft.rgb)
            .opacity(theme.connection.stroke_draft.alpha)
            .into();
        let wire_width = viewport.scale_length(theme.connection.stroke_width);
        let selected_wire_width = viewport.scale_length(theme.connection.stroke_width_selected);
        let dangling_color: gpui::Hsla = rgb(0xef4444).into();
        let canvas_bounds = self.canvas_bounds.clone();
        let captured_graph = cx.weak_entity();
        let corner_radius = if self.config.routing == RoutingMode::Bezier {
            0.0
        } else {
            self.config.route_corner_radius
        };
        let wire_move_theme = Arc::clone(&theme);
        let wire_up_theme = Arc::clone(&theme);
        let wire_layer = canvas(
            move |bounds, _, _| {
                canvas_bounds.set(bounds);
            },
            move |bounds, _, window, _cx| {
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
                            editor.handle_pointer_move(local, &wire_move_theme, cx);
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
                            editor.finish_left_gesture(false, &wire_up_theme, cx);
                        }
                    });
                });
                for (route, selected, dangling) in &wires {
                    let projected = route
                        .iter()
                        .map(|point| viewport.world_to_screen(*point))
                        .collect::<Vec<_>>();
                    let color = if *dangling {
                        dangling_color
                    } else if *selected {
                        selected_wire_color
                    } else {
                        wire_color
                    };
                    let width = if *selected || *dangling {
                        selected_wire_width
                    } else {
                        wire_width
                    };
                    if *dangling {
                        paint_dashed_route(window, bounds, &projected, color, width, 6.0, 4.0);
                    } else {
                        paint_route(window, bounds, projected, color, width, corner_radius);
                    }
                }
                if let Some((source, end)) = draft {
                    let start = viewport.world_to_screen(source);
                    let mid = start.x * 0.5 + end.x * 0.5;
                    paint_dashed_route(
                        window,
                        bounds,
                        &[
                            start,
                            core::Point::new(mid, start.y),
                            core::Point::new(mid, end.y),
                            end,
                        ],
                        draft_color,
                        wire_width,
                        6.0,
                        4.0,
                    );
                }
            },
        )
        .absolute()
        .size_full();

        let focused = focus_handle.is_focused(window);
        let focus_outline = theme.editor.focus_outline;
        let clip_world_content = theme.editor.clip_content;
        let clip_overlay_content =
            theme.editor.overlay_clip_content && theme.overlay.clip_to_editor;
        let mut root = div()
            .id(("node-graph-editor", cx.entity().entity_id()))
            .role(gpui::Role::Group)
            .aria_label("Node graph editor")
            .relative()
            .size_full()
            .when(
                focused && visible_border_width(focus_outline) > 0.0,
                |element| {
                    element
                        .border(px(visible_border_width(focus_outline)))
                        .when(focus_outline.style == style::LineStyle::Dashed, |element| {
                            element.border_dashed()
                        })
                        .border_color(
                            rgb(focus_outline.color.rgb).opacity(focus_outline.color.alpha),
                        )
                },
            )
            .bg(rgb(theme.editor.background.rgb).opacity(theme.editor.background.alpha))
            .track_focus(&focus_handle)
            .key_context("NodeGraph")
            .on_key_down(cx.listener(Self::handle_key_down));

        let mut group_labels = Vec::new();
        let mut custom_group_headers = Vec::new();
        let render_groups = self.groups.clone();
        for group in &render_groups {
            let group_label = self
                .group_editor
                .as_ref()
                .filter(|editor| editor.id == group.id)
                .map(|editor| text_with_caret(&editor.query))
                .or_else(|| group.label.clone());
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
            let horizontal_padding = theme.group.padding_x;
            let top_padding = theme.group.padding_top;
            let bottom_padding = theme.group.padding_bottom;
            let origin = viewport.world_to_screen(core::Point::new(
                left - horizontal_padding,
                top - top_padding,
            ));
            let width = viewport.scale_length(right - left + horizontal_padding * 2.0);
            let height = viewport.scale_length(bottom - top + top_padding + bottom_padding);
            let is_hovered = self.drag.as_ref().is_some_and(|drag| {
                if !drag.alter_groups {
                    return false;
                }
                drag.starts.first().is_some_and(|(dragged_id, _)| {
                    self.graph.nodes.get(dragged_id).is_some_and(|dragged| {
                        let center = viewport.world_to_screen(core::Point::new(
                            dragged.position.x + dragged.size.width * 0.5,
                            dragged.position.y + dragged.size.height * 0.5,
                        ));
                        center.x >= origin.x
                            && center.x <= origin.x + width
                            && center.y >= origin.y
                            && center.y <= origin.y + height
                    })
                })
            });
            let is_error = group.error || self.group_errors.contains(&group.id);
            let color = resolved_group_color(group.color, theme.group.default_color);
            let (border_color, background) = if is_error {
                (theme.group.error_border, theme.group.error_background)
            } else if is_hovered {
                (
                    style::Color::rgba(color.rgb, theme.group.hovered_border_opacity),
                    style::Color::rgba(color.rgb, theme.group.hovered_background_opacity),
                )
            } else {
                (
                    style::Color::rgba(color.rgb, theme.group.border_opacity),
                    style::Color::rgba(color.rgb, theme.group.background_opacity),
                )
            };
            root = root.child(
                div()
                    .absolute()
                    .left(px(origin.x))
                    .top(px(origin.y))
                    .w(px(width))
                    .h(px(height))
                    .rounded(px(viewport.scale_length(theme.group.border_radius)))
                    .border(px(viewport.scale_length(theme.group.border_width)))
                    .when(!is_hovered, |element| element.border_dashed())
                    .border_color(rgb(border_color.rgb).opacity(border_color.alpha))
                    .bg(rgb(background.rgb).opacity(background.alpha)),
            );
            let label_color = if is_error {
                theme.group.error_label_color
            } else {
                mix_color(color, style::Color::rgb(0xffffff), 0.7)
            };
            if let Some(renderer) = self.group_header_renderer.as_mut() {
                let bounds = core::Rect {
                    origin: core::Point::new(left - horizontal_padding, top - top_padding),
                    size: core::Size {
                        width: right - left + horizontal_padding * 2.0,
                        height: bottom - top + top_padding + bottom_padding,
                    },
                };
                let element = renderer.render_group_header(
                    GroupHeaderContext {
                        group: group.clone(),
                        bounds,
                        hovered: is_hovered,
                        error: is_error,
                        theme: Arc::clone(&theme),
                    },
                    window,
                    cx,
                );
                custom_group_headers.push((origin, width, height, element));
            } else if let Some(group_label) = group_label {
                group_labels.push((origin, group.id.clone(), group_label, label_color, is_error));
            }
        }
        root = root.child(wire_layer);
        for (origin, width, height, header) in custom_group_headers {
            root = root.child(
                div()
                    .absolute()
                    .left(px(origin.x))
                    .top(px(origin.y))
                    .w(px(width))
                    .h(px(height))
                    .child(header),
            );
        }
        for marker in dangling_markers {
            root = root.child(
                div()
                    .absolute()
                    .left(px(marker.x - 3.0))
                    .top(px(marker.y - 8.0))
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(dangling_color)
                    .child("?"),
            );
        }
        for (origin, group_id, label, color, _is_error) in group_labels {
            let editing = self
                .group_editor
                .as_ref()
                .is_some_and(|editor| editor.id == group_id);
            root = root.child(
                div()
                    .absolute()
                    .left(px(origin.x + viewport.scale_length(theme.group.label_left)))
                    .top(px(origin.y + viewport.scale_length(theme.group.label_top)))
                    .when(editing, |element| {
                        let background = theme.group.input_background;
                        element.bg(rgb(background.rgb).opacity(background.alpha))
                    })
                    .text_color(rgb(color.rgb).opacity(color.alpha))
                    .font_weight(FontWeight(theme.group.label_font_weight as f32))
                    .text_size(px(viewport.scale_length(theme.group.label_font_size)))
                    .child(label.to_uppercase())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            if editing {
                                this.focus(window, cx);
                                cx.stop_propagation();
                                window.prevent_default();
                                return;
                            }
                            if event.click_count >= 2
                                && let Some(group) =
                                    this.groups.iter().find(|group| group.id == group_id)
                            {
                                this.group_editor = Some(GroupEditor {
                                    id: group.id.clone(),
                                    query: WorldTextInputState::at_end(
                                        group.label.clone().unwrap_or_default(),
                                    ),
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
        self.active_escape_overlays.clear();
        self.active_outside_overlays.clear();
        self.active_backdrop_overlays.clear();
        self.active_overlay_dismiss_callbacks.clear();
        self.active_overlay_bounds.clear();
        let mut body_anchored_nodes = HashSet::new();
        let mut frame_world_scene = world::WorldScene::new();
        let mut frame_world_control_owners = HashMap::new();
        let mut frame_world_control_order = Vec::new();
        let mut nodes: Vec<_> = self.graph.nodes.values().cloned().collect();
        nodes.sort_by_cached_key(|node| format!("{:?}", node.id));
        for mut node in nodes {
            let id = node.id.clone();
            let model_size = self.resolved_node_size(&id).unwrap_or(node.size);
            if let Some(size) = self.resolved_node_size(&id) {
                node.size = size;
            }
            let position = viewport.world_to_screen(node.position);
            let selected = self.graph.selected_nodes.contains(&id);
            let visible = self.node_is_visible(&node);
            let resize_id = id.clone();
            let has_registered_body = self
                .node_type_registry
                .as_ref()
                .and_then(|registry| registry.get(&node.node_type))
                .is_some_and(|definition| {
                    definition.world_renderer.is_some() || definition.retained_renderer.is_some()
                });
            let has_custom_body = has_registered_body
                || self.node_body_renderer.is_some()
                || self.world_node_body_renderer.is_some();
            let mut ports: Vec<_> = self
                .graph
                .ports
                .values()
                .filter(|port| port.node == id)
                .cloned()
                .collect();
            if let Some(order) = self.defined_port_order.get(&id) {
                ports.sort_by_cached_key(|port| {
                    (
                        order
                            .iter()
                            .position(|id| id == &port.id)
                            .unwrap_or(usize::MAX),
                        format!("{:?}", port.id),
                    )
                });
            } else {
                ports.sort_by_cached_key(|port| format!("{:?}", port.id));
            }
            let port_states: Arc<HashMap<P, WorldPortVisualState>> = Arc::new(
                ports
                    .iter()
                    .filter_map(|port| {
                        self.port_visual_state(&port.id)
                            .map(|state| (port.id.clone(), state))
                    })
                    .collect(),
            );
            let port_presentations: Arc<HashMap<P, AnchorPresentation>> = Arc::new(
                ports
                    .iter()
                    .filter_map(|port| {
                        self.port_presentations
                            .get(&port.id)
                            .copied()
                            .map(|presentation| (port.id.clone(), presentation))
                    })
                    .collect(),
            );
            let registered_definition = self
                .node_type_registry
                .as_ref()
                .and_then(|registry| registry.get(&node.node_type));
            let mut port_slot_elements = Vec::new();
            if let (Some(registry), Some(definition)) =
                (self.node_type_registry.as_ref(), registered_definition)
            {
                let mut specs = definition.item.ports.clone();
                if let Some(producer) = &definition.dynamic_inputs {
                    specs.extend(producer(&node).into_iter().map(|port| CatalogPort {
                        id: port.key,
                        label: port.label,
                        direction: PortDirection::Input,
                        kind: port.kind,
                    }));
                }
                if let Some(producer) = &definition.dynamic_outputs {
                    specs.extend(producer(&node).into_iter().map(|port| CatalogPort {
                        id: port.key,
                        label: port.label,
                        direction: PortDirection::Output,
                        kind: port.kind,
                    }));
                }
                for port in &ports {
                    let Some(spec) = specs
                        .iter()
                        .find(|spec| (registry.id_for)(&node.id, &spec.id) == port.id)
                    else {
                        continue;
                    };
                    let renderer = definition.port_slots.get(&spec.id).cloned().or_else(|| {
                        registry
                            .port_type_slots
                            .iter()
                            .rev()
                            .find(|(kind, direction, _)| {
                                kind == &port.kind && direction == &port.direction
                            })
                            .map(|(_, _, renderer)| renderer.clone())
                    });
                    let Some(renderer) = renderer else {
                        continue;
                    };
                    let world_position = self
                        .resolved_port_position(&port.id)
                        .unwrap_or(port.position);
                    let top = viewport.scale_length(world_position.y - node.position.y - 9.0);
                    let content = renderer(port.label.clone());
                    let slot = div()
                        .absolute()
                        .top(px(top))
                        .when(port.direction == PortDirection::Input, |element| {
                            element.left(px(viewport.scale_length(26.0)))
                        })
                        .when(port.direction == PortDirection::Output, |element| {
                            element.right(px(viewport.scale_length(26.0)))
                        })
                        .child(content)
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_down(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
                        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                        .on_mouse_up(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_up(MouseButton::Middle, |_, _, cx| cx.stop_propagation())
                        .on_mouse_up(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                        .on_key_down(|_, _, cx| cx.stop_propagation());
                    port_slot_elements.push(slot);
                }
            }
            let mut body = if let Some(renderer) =
                registered_definition.and_then(|definition| definition.world_renderer.as_ref())
            {
                let scene = renderer
                    .borrow_mut()
                    .render_world_node(WorldNodeBodyContext {
                        node: node.clone(),
                        ports: ports.clone().into(),
                        port_states: port_states.clone(),
                        port_presentations: port_presentations.clone(),
                        state: WorldNodeVisualState { selected, visible },
                        theme: theme.clone(),
                    });
                for hit in &scene.hit_regions {
                    if matches!(hit.role, world::HitRole::Control) {
                        frame_world_control_owners.insert(hit.id.clone(), id.clone());
                        frame_world_control_order.push((id.clone(), hit.id.clone()));
                    }
                }
                frame_world_scene.primitives.extend(scene.primitives);
                frame_world_scene.hit_regions.extend(scene.hit_regions);
                NodeBody::new(div()).with_ports(PortPresentation::BodyAnchors)
            } else if let Some(renderer) =
                registered_definition.and_then(|definition| definition.retained_renderer.as_ref())
            {
                renderer.borrow_mut().render_node(
                    NodeBodyContext {
                        node: node.clone(),
                        ports: ports.clone().into(),
                        port_states: port_states.clone(),
                        port_presentations: port_presentations.clone(),
                        state: NodeVisualState {
                            selected,
                            visible,
                            zoom: viewport.zoom,
                        },
                        theme: theme.clone(),
                        graph: cx.weak_entity(),
                        canvas_bounds: self.canvas_bounds.clone(),
                        viewport,
                    },
                    window,
                    cx,
                )
            } else if let Some(renderer) = self.world_node_body_renderer.as_mut() {
                let scene = renderer.render_world_node(WorldNodeBodyContext {
                    node: node.clone(),
                    ports: ports.clone().into(),
                    port_states: port_states.clone(),
                    port_presentations: port_presentations.clone(),
                    state: WorldNodeVisualState { selected, visible },
                    theme: theme.clone(),
                });
                for hit in &scene.hit_regions {
                    if matches!(hit.role, world::HitRole::Control) {
                        frame_world_control_owners.insert(hit.id.clone(), id.clone());
                        frame_world_control_order.push((id.clone(), hit.id.clone()));
                    }
                }
                frame_world_scene.primitives.extend(scene.primitives);
                frame_world_scene.hit_regions.extend(scene.hit_regions);
                NodeBody::new(div()).with_ports(PortPresentation::BodyAnchors)
            } else if let Some(renderer) = self.node_body_renderer.as_mut() {
                renderer.render_node(
                    NodeBodyContext {
                        node: node.clone(),
                        ports: ports.clone().into(),
                        port_states: port_states.clone(),
                        port_presentations: port_presentations.clone(),
                        state: NodeVisualState {
                            selected,
                            visible,
                            zoom: viewport.zoom,
                        },
                        theme: theme.clone(),
                        graph: cx.weak_entity(),
                        canvas_bounds: self.canvas_bounds.clone(),
                        viewport,
                    },
                    window,
                    cx,
                )
            } else {
                let header = theme.node.header_border_bottom;
                let background = theme.node.header_background;
                let mut default_header = div()
                    .px(px(viewport.scale_length(theme.node.padding_x)))
                    .py(px(viewport.scale_length(theme.node.header_padding_y)))
                    .bg(rgb(background.rgb).opacity(background.alpha))
                    .child(node.title.clone());
                if header.width > 0.0 && header.style != style::LineStyle::None {
                    default_header = default_header
                        .border_b(px(viewport.scale_length(header.width)))
                        .border_color(rgb(header.color.rgb).opacity(header.color.alpha))
                        .when(header.style == style::LineStyle::Dashed, |element| {
                            element.border_dashed()
                        });
                }
                NodeBody::new(default_header.children(port_slot_elements))
            };
            if let Some(renderer) = self.node_overlay_renderer.as_mut() {
                body.overlays.extend(renderer.render_node_overlays(
                    WorldNodeBodyContext {
                        node: node.clone(),
                        ports: ports.clone().into(),
                        port_states: port_states.clone(),
                        port_presentations: port_presentations.clone(),
                        state: WorldNodeVisualState { selected, visible },
                        theme: theme.clone(),
                    },
                    window,
                    cx,
                ));
            }
            if body.ports == PortPresentation::BodyAnchors {
                body_anchored_nodes.insert(id.clone());
            }
            for overlay in body.overlays.drain(..) {
                let screen_offset =
                    overlay_screen_offset(overlay.offset, viewport) + overlay.screen_offset;
                let mut overlay_position = position + screen_offset;
                let mut measurement_id = None;
                if let Some(behavior) = &overlay.behavior {
                    if self.dismissed_overlays.contains(&behavior.id) {
                        continue;
                    }
                    if let Some(callback) = overlay.on_dismiss.as_ref() {
                        self.active_overlay_dismiss_callbacks
                            .insert(behavior.id.clone(), callback.clone());
                    }
                    if behavior.dismiss_on_escape
                        || behavior.dismiss_on_outside_click
                        || behavior.show_backdrop
                    {
                        self.active_dismissible_overlays.insert(behavior.id.clone());
                    }
                    if behavior.dismiss_on_escape {
                        self.active_escape_overlays.insert(behavior.id.clone());
                    }
                    if behavior.dismiss_on_outside_click {
                        self.active_outside_overlays.insert(behavior.id.clone());
                    }
                    if behavior.show_backdrop {
                        self.active_backdrop_overlays.insert(behavior.id.clone());
                    }
                    measurement_id = Some(behavior.id.clone());
                    let panel_size = self
                        .measured_overlay_sizes
                        .get(&behavior.id)
                        .copied()
                        .unwrap_or(behavior.estimated_size);
                    let canvas = self.canvas_bounds.get();
                    let canvas_size = core::Size {
                        width: f32::from(canvas.size.width),
                        height: f32::from(canvas.size.height),
                    };
                    overlay_position = if let Some(placement) =
                        self.overlay_placements.get(&behavior.id).copied()
                    {
                        let anchor = self
                            .overlay_pane_anchors
                            .get(&behavior.id)
                            .copied()
                            .or_else(|| {
                                self.overlay_anchor_controls
                                    .get(&behavior.id)
                                    .and_then(|control_id| {
                                        frame_world_scene
                                            .hit_regions
                                            .iter()
                                            .rev()
                                            .find(|hit| &hit.id == control_id)
                                    })
                                    .map(|hit| hit.shape.project(viewport.into()).bounds())
                            })
                            .unwrap_or(core::Rect {
                                origin: position + overlay_screen_offset(overlay.offset, viewport),
                                size: core::Size {
                                    width: viewport.scale_length(placement.anchor_size.width),
                                    height: viewport.scale_length(placement.anchor_size.height),
                                },
                            });
                        resolve_positioned_overlay(anchor, placement, panel_size, canvas_size)
                            + overlay.screen_offset
                    } else {
                        resolve_overlay_position(
                            position,
                            viewport.scale_length(node.size.width),
                            screen_offset,
                            behavior,
                            panel_size,
                            canvas_size,
                        )
                    };
                    if behavior.dismiss_on_outside_click {
                        self.active_overlay_bounds.push(core::Rect {
                            origin: overlay_position,
                            size: panel_size,
                        });
                    }
                }
                node_overlays.push((overlay_position, overlay.element, measurement_id));
            }
            let graph = cx.weak_entity();
            let measured_node = id.clone();
            let raw_body_element = body.element;
            let body_element = if has_custom_body {
                MeasuredElement::new(
                    NodeScaleElement::new(raw_body_element, viewport.zoom),
                    move |bounds, cx| {
                        let measured = core::Size {
                            width: f32::from(bounds.size.width) / viewport.zoom,
                            height: f32::from(bounds.size.height) / viewport.zoom,
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
                                    editor
                                        .render_geometry
                                        .node_sizes
                                        .insert(node_id.clone(), measured);
                                    cx.emit(core::GraphEvent::NodeResized {
                                        id: node_id,
                                        size: measured,
                                    });
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
            let node_background = theme.node.background;
            let header_accent_color = (!has_custom_body)
                .then(|| {
                    self.catalog
                        .iter()
                        .find(|item| item.id == node.node_type)
                        .and_then(|item| item.category_color)
                })
                .flatten();
            let node_radius = theme.node.border_radius;
            let selected_outline = theme.node.outline_selected;
            let node_border = theme.node.border;
            let dragging = self
                .drag
                .as_ref()
                .is_some_and(|drag| drag.starts.iter().any(|(node_id, _)| node_id == &id));
            let resizing = self.resize.as_ref().is_some_and(|resize| resize.id == id);
            let automatic_width = self.auto_width_nodes.contains(&id);
            let styled_automatic_width = theme.node.width;
            let node_cursor = resolved_node_cursor(&theme.node, dragging, self.resize.is_some());
            let mut node_shadows = if selected {
                &theme.node.shadow_selected
            } else {
                &theme.node.shadow
            }
            .iter()
            .map(|shadow| {
                BoxShadow::new(
                    px(viewport.scale_length(shadow.offset_x)),
                    px(viewport.scale_length(shadow.offset_y)),
                    rgb(shadow.color.rgb).opacity(shadow.color.alpha).into(),
                )
                .blur_radius(px(viewport.scale_length(shadow.blur * 0.5)))
                .spread_radius(px(viewport.scale_length(shadow.spread)))
            })
            .collect::<Vec<_>>();
            // GPUI has no outline primitive. A zero-offset, zero-blur outer shadow is
            // the non-layout-shifting equivalent for the solid Leptos selection outline.
            if selected
                && selected_outline.width > 0.0
                && selected_outline.style == style::LineStyle::Solid
            {
                node_shadows.push(
                    BoxShadow::new(
                        px(0.0),
                        px(0.0),
                        rgb(selected_outline.color.rgb)
                            .opacity(selected_outline.color.alpha)
                            .into(),
                    )
                    .spread_radius(px(viewport.scale_length(selected_outline.width))),
                );
            }
            root = root.child(
                div()
                    .absolute()
                    .left(px(position.x))
                    .top(px(position.y))
                    .when(!automatic_width, |element| {
                        element.w(px(viewport.scale_length(node.size.width)))
                    })
                    .when_some(
                        styled_automatic_width.filter(|_| automatic_width),
                        |element, width| element.w(px(viewport.scale_length(width))),
                    )
                    .when(automatic_width, |element| {
                        element.min_w(px(viewport.scale_length(theme.node.min_width)))
                    })
                    .h(px(viewport.scale_length(model_size.height)))
                    .rounded(px(viewport.scale_length(node_radius)))
                    .shadow(node_shadows)
                    .overflow_hidden()
                    .opacity(if dragging {
                        theme.node.opacity_dragging
                    } else {
                        1.0
                    })
                    .when(matches!(node_cursor, style::Cursor::Default), |element| {
                        element.cursor_default()
                    })
                    .when(matches!(node_cursor, style::Cursor::Grab), |element| {
                        element.cursor_grab()
                    })
                    .when(matches!(node_cursor, style::Cursor::Grabbing), |element| {
                        element.cursor_grabbing()
                    })
                    .when(matches!(node_cursor, style::Cursor::Crosshair), |element| {
                        element.cursor_crosshair()
                    })
                    .when(matches!(node_cursor, style::Cursor::EwResize), |element| {
                        element.cursor_ew_resize()
                    })
                    .when(
                        node_border.width > 0.0 && node_border.style != style::LineStyle::None,
                        |element| {
                            element
                                .border(px(viewport.scale_length(node_border.width)))
                                .border_color(
                                    rgb(node_border.color.rgb).opacity(node_border.color.alpha),
                                )
                                .when(node_border.style == style::LineStyle::Dashed, |element| {
                                    element.border_dashed()
                                })
                        },
                    )
                    .when(
                        selected
                            && selected_outline.width > 0.0
                            && selected_outline.style == style::LineStyle::Dashed,
                        |element| {
                            element
                                .border(px(viewport.scale_length(selected_outline.width)))
                                .border_dashed()
                                .border_color(
                                    rgb(selected_outline.color.rgb)
                                        .opacity(selected_outline.color.alpha),
                                )
                        },
                    )
                    .bg(rgb(node_background.rgb).opacity(node_background.alpha))
                    .text_color(
                        rgb(theme.node.header_color.rgb).opacity(theme.node.header_color.alpha),
                    )
                    .text_size(px(viewport.scale_length(theme.node.header_font_size)))
                    .when_some(header_accent_color, |element, color| {
                        element.child(
                            div()
                                .h(px(viewport.scale_length(theme.node.header_accent_height)))
                                .bg(rgb(color.rgb).opacity(color.alpha)),
                        )
                    })
                    .child(body_element)
                    .on_mouse_down(MouseButton::Left, {
                        let theme = Arc::clone(&theme);
                        cx.listener({
                            let theme = Arc::clone(&theme);
                            move |this, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.focus(window, cx);
                                let local = this.local_screen(event.position);
                                this.dismiss_overlays_before_world_pointer(local, cx);
                                if let Some(hit) = this
                                    .world_scene
                                    .hit_test_screen(local, this.graph.viewport)
                                    .cloned()
                                    && matches!(hit.role, world::HitRole::Control)
                                    && this.world_control_owners.get(&hit.id) == Some(&id)
                                {
                                    this.activate_world_control(id.clone(), hit.id.clone(), cx);
                                    cx.emit(core::GraphEvent::NodeControlPointerActivated {
                                        node_id: id.clone(),
                                        control_id: hit.id,
                                        world_position: this.graph.viewport.screen_to_world(local),
                                        click_count: event.click_count,
                                    });
                                    cx.notify();
                                    return;
                                }
                                this.blur_world_control(cx);
                                if let Some(port_id) = this.port_at_screen(
                                    local,
                                    this.graph
                                        .viewport
                                        .scale_length(theme.anchor.dot_size * 0.5),
                                ) {
                                    this.engage_port(&port_id, cx);
                                    return;
                                }
                                let cursor = this.graph.viewport.screen_to_world(local);
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
                                        primary: id.clone(),
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
                                if event.modifiers.alt && this.drag.is_some() {
                                    this.detach_node_from_groups_for_alt_drag(&id, cx);
                                }
                                cx.notify();
                            }
                        })
                    }),
            );
            if theme.node.resizable {
                let resize_theme = Arc::clone(&theme);
                root = root.child(
                    div()
                        .absolute()
                        .left(px(position.x + viewport.scale_length(node.size.width)
                            - viewport.scale_length(theme.node.resize_handle_width * 0.5)))
                        .top(px(position.y))
                        .w(px(viewport.scale_length(theme.node.resize_handle_width)))
                        .h(px(viewport.scale_length(node.size.height)))
                        .when(resizing, |element| {
                            let color = theme.node.resize_handle_color;
                            element.bg(rgb(color.rgb).opacity(color.alpha))
                        })
                        .when(
                            matches!(theme.node.cursor_resize, style::Cursor::Default),
                            |element| element.cursor_default(),
                        )
                        .when(
                            matches!(theme.node.cursor_resize, style::Cursor::Grab),
                            |element| element.cursor_grab(),
                        )
                        .when(
                            matches!(theme.node.cursor_resize, style::Cursor::Grabbing),
                            |element| element.cursor_grabbing(),
                        )
                        .when(
                            matches!(theme.node.cursor_resize, style::Cursor::Crosshair),
                            |element| element.cursor_crosshair(),
                        )
                        .when(
                            matches!(theme.node.cursor_resize, style::Cursor::EwResize),
                            |element| element.cursor_ew_resize(),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.focus(window, cx);
                                this.commit_group_editor(cx);
                                if event.click_count >= 2 {
                                    this.reset_node_width(&resize_id, &resize_theme, cx);
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
        }

        let mut default_port_scene = world::WorldScene::new();
        for port in self.graph.ports.values() {
            if body_anchored_nodes.contains(&port.node) {
                continue;
            }
            let id = port.id.clone();
            let world_position = self
                .resolved_port_position(&port.id)
                .unwrap_or(port.position);
            let position = viewport.world_to_screen(world_position);
            let connected = self.graph.connections.values().any(|connection| {
                !self.connection_is_detached(&connection.id)
                    && (connection.source == port.id || connection.target == port.id)
            });
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
            let presentation = self.port_presentation(&port.id);
            let mut port_paint = resolved_port_paint(
                &theme.anchor,
                connected,
                is_source,
                is_snap,
                compatible,
                self.draft.is_some(),
            );
            if !connected
                && !is_source
                && !is_snap
                && !compatible
                && let Some(color) = presentation.dot_color
            {
                port_paint.stroke = color;
                port_paint.fill = color;
            }
            let shape = presentation
                .dot_shape
                .unwrap_or(theme.anchor.default_dot_shape);
            let world_dot_radius = theme.anchor.dot_size * 0.5;
            if presentation.multi {
                let mut ghost_center = world_position;
                let offset = theme.anchor.dot_size * (5.0 / 24.0);
                ghost_center.x += if port.direction == PortDirection::Input {
                    offset
                } else {
                    -offset
                };
                let ghost_radius = world_dot_radius * 0.72;
                push_dot_shape(
                    &mut default_port_scene,
                    ghost_center,
                    ghost_radius,
                    shape,
                    port_paint.stroke,
                    port_paint.opacity * 0.45,
                );
                if port_paint.fill == style::Color::TRANSPARENT {
                    push_dot_shape(
                        &mut default_port_scene,
                        ghost_center,
                        (ghost_radius - theme.anchor.dot_border_width).max(0.0),
                        shape,
                        theme.node.background,
                        port_paint.opacity * 0.45,
                    );
                }
            }
            if shape != style::DotShape::Circle {
                push_dot_shape(
                    &mut default_port_scene,
                    world_position,
                    world_dot_radius,
                    shape,
                    port_paint.stroke,
                    port_paint.opacity,
                );
                if port_paint.fill == style::Color::TRANSPARENT {
                    push_dot_shape(
                        &mut default_port_scene,
                        world_position,
                        (world_dot_radius - theme.anchor.dot_border_width).max(0.0),
                        shape,
                        theme.node.background,
                        port_paint.opacity,
                    );
                }
            }
            let dot_size = viewport.scale_length(theme.anchor.dot_size);
            let dot_radius = dot_size * 0.5;
            let label_gap = dot_radius + viewport.scale_length(theme.anchor.row_gap);
            let compatible_glow = if port_paint.glow {
                theme
                    .anchor
                    .dot_compatible_glow
                    .iter()
                    .map(|shadow| {
                        BoxShadow::new(
                            px(viewport.scale_length(shadow.offset_x)),
                            px(viewport.scale_length(shadow.offset_y)),
                            rgb(shadow.color.rgb).opacity(shadow.color.alpha).into(),
                        )
                        .blur_radius(px(viewport.scale_length(shadow.blur * 0.5)))
                        .spread_radius(px(viewport.scale_length(shadow.spread)))
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            let row_padding_x = viewport.scale_length(theme.anchor.row_padding_x);
            let row_padding_y = viewport.scale_length(theme.anchor.row_padding_y);
            let label_x = if port.direction == PortDirection::Input {
                position.x + dot_radius - row_padding_x
            } else {
                position.x - 80.0 - label_gap - row_padding_x
            };
            let label_width =
                80.0 + viewport.scale_length(theme.anchor.row_gap) + row_padding_x * 2.0;
            let row_height = viewport.scale_length(theme.anchor.row_height) + row_padding_y * 2.0;
            let draft_active = self.draft.is_some();
            let label_right_id = id.clone();
            let label_down_id = id.clone();
            let label_up_id = id.clone();
            let label_up_theme = Arc::clone(&theme);
            let label_element = div()
                .absolute()
                .left(px(label_x))
                .top(px(position.y - row_height * 0.5))
                .w(px(label_width))
                .h(px(row_height))
                .flex()
                .items_center()
                .when(port.direction == PortDirection::Input, |element| {
                    element.pl(px(
                        viewport.scale_length(theme.anchor.row_gap) + row_padding_x
                    ))
                })
                .when(port.direction == PortDirection::Output, |element| {
                    element
                        .pr(px(
                            viewport.scale_length(theme.anchor.row_gap) + row_padding_x
                        ))
                        .justify_end()
                })
                .text_size(px(viewport.scale_length(theme.anchor.label_font_size)))
                .text_color(rgb(port_paint.label.rgb).opacity(port_paint.label.alpha))
                .opacity(port_paint.opacity)
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.focus(window, cx);
                        let local = this.local_screen(event.position);
                        this.open_anchor_menu(label_right_id.clone(), local, cx);
                    }),
                )
                .when(draft_active, |element| {
                    element
                        .cursor_crosshair()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.focus(window, cx);
                                this.engage_port(&label_down_id, cx);
                            }),
                        )
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                                cx.stop_propagation();
                                if this
                                    .draft
                                    .as_ref()
                                    .is_some_and(|draft| draft.origin != label_up_id)
                                {
                                    this.finish_draft(&label_up_id, cx);
                                }
                                this.finish_left_gesture(true, &label_up_theme, cx);
                            }),
                        )
                })
                .child(port.label.clone());
            let dot_up_theme = Arc::clone(&theme);
            root = root.child(label_element).child(
                div()
                    .absolute()
                    .left(px(position.x - dot_radius))
                    .top(px(position.y - dot_radius))
                    .w(px(dot_size))
                    .h(px(dot_size))
                    .when(shape == style::DotShape::Circle, |element| {
                        element
                            .rounded_full()
                            .shadow(compatible_glow)
                            .border(px(viewport.scale_length(theme.anchor.dot_border_width)))
                            .border_color(
                                rgb(port_paint.stroke.rgb).opacity(port_paint.stroke.alpha),
                            )
                            .bg(rgb(port_paint.fill.rgb).opacity(port_paint.fill.alpha))
                            .opacity(port_paint.opacity)
                    })
                    .cursor_crosshair()
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener({
                            let id = id.clone();
                            move |this, event: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.focus(window, cx);
                                let local = this.local_screen(event.position);
                                this.open_anchor_menu(id.clone(), local, cx);
                            }
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener({
                            let id = id.clone();
                            move |this, _: &MouseDownEvent, window, cx| {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.focus(window, cx);
                                this.engage_port(&id, cx);
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
                            this.finish_left_gesture(true, &dot_up_theme, cx);
                        }),
                    ),
            );
        }

        if !default_port_scene.primitives.is_empty() {
            root = root.child(
                div()
                    .absolute()
                    .size_full()
                    .child(world_scene_element(default_port_scene, viewport)),
            );
        }

        let semantic_world_controls: Vec<_> = frame_world_scene
            .project(viewport)
            .hit_regions
            .into_iter()
            .filter_map(|region| {
                if !matches!(region.role, world::HitRole::Control) {
                    return None;
                }
                let node_id = frame_world_control_owners.get(&region.id)?.clone();
                let active =
                    self.last_world_control.as_ref() == Some(&(node_id.clone(), region.id.clone()));
                let label = region.accessible_label.unwrap_or_else(|| region.id.clone());
                Some((
                    region.id,
                    label,
                    node_id,
                    region.shape.bounds(),
                    active,
                    region.accessible_role,
                    region.accessible_value,
                    region.accessible_numeric_value,
                    region.accessible_min_numeric_value,
                    region.accessible_max_numeric_value,
                    region.accessible_numeric_value_step,
                ))
            })
            .collect();
        self.world_scene = frame_world_scene.clone();
        self.world_control_owners = frame_world_control_owners;
        self.world_control_order = frame_world_control_order;
        let world_move_theme = Arc::clone(&theme);
        let world_right_theme = Arc::clone(&theme);
        let world_down_theme = Arc::clone(&theme);
        let world_up_theme = Arc::clone(&theme);
        if !frame_world_scene.primitives.is_empty() {
            root = root.child(
                div()
                    .absolute()
                    .size_full()
                    .occlude()
                    .child(world_scene_element(frame_world_scene, viewport))
                    .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                        let local = this.local_screen(event.position);
                        this.handle_pointer_move(local, &world_move_theme, cx);
                    }))
                    .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, window, cx| {
                        this.handle_scroll_wheel(event, window, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.focus(window, cx);
                            let local = this.local_screen(event.position);
                            if let Some(port_id) = this.port_at_screen(
                                local,
                                this.graph
                                    .viewport
                                    .scale_length(world_right_theme.anchor.dot_size * 0.5),
                            ) {
                                this.open_anchor_menu(port_id, local, cx);
                            } else {
                                this.anchor_menu = None;
                                cx.notify();
                            }
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Middle,
                        cx.listener(|this, event: &MouseDownEvent, window, cx| {
                            this.focus(window, cx);
                            this.panning = Some(this.local_screen(event.position));
                            cx.stop_propagation();
                            window.prevent_default();
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Middle,
                        cx.listener(|this, _: &MouseUpEvent, _, _| this.panning = None),
                    )
                    .on_mouse_up_out(
                        MouseButton::Middle,
                        cx.listener(|this, _: &MouseUpEvent, _, _| this.panning = None),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            this.focus(window, cx);
                            let local = this.local_screen(event.position);
                            this.dismiss_overlays_before_world_pointer(local, cx);
                            if event.modifiers.control {
                                this.panning = Some(local);
                                this.box_selection = None;
                                cx.stop_propagation();
                                window.prevent_default();
                                return;
                            }
                            if let Some(hit) = this
                                .world_scene
                                .hit_test_screen(local, this.graph.viewport)
                                .cloned()
                                && matches!(hit.role, world::HitRole::Control)
                                && let Some(node_id) = this.node_at_screen(local)
                                && this.world_control_owners.get(&hit.id) == Some(&node_id)
                            {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.focus(window, cx);
                                this.commit_group_editor(cx);
                                this.activate_world_control(node_id.clone(), hit.id.clone(), cx);
                                cx.emit(core::GraphEvent::NodeControlPointerActivated {
                                    node_id,
                                    control_id: hit.id,
                                    world_position: this.graph.viewport.screen_to_world(local),
                                    click_count: event.click_count,
                                });
                                cx.notify();
                                return;
                            }
                            this.blur_world_control(cx);
                            if let Some(port_id) = this.port_at_screen(
                                local,
                                this.graph
                                    .viewport
                                    .scale_length(world_down_theme.anchor.dot_size * 0.5),
                            ) {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.focus(window, cx);
                                this.engage_port(&port_id, cx);
                                return;
                            }
                            if let Some(node_id) = this.node_at_screen(local) {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.focus(window, cx);
                                let on_resize_edge = world_down_theme.node.resizable
                                    && this.graph.nodes.get(&node_id).is_some_and(|node| {
                                        let world = this.graph.viewport.screen_to_world(local);
                                        (world.x - (node.position.x + node.size.width)).abs()
                                            <= world_down_theme.node.resize_handle_width
                                    });
                                if on_resize_edge {
                                    if event.click_count >= 2 {
                                        this.reset_node_width(&node_id, &world_down_theme, cx);
                                    } else {
                                        this.begin_node_resize(
                                            &node_id,
                                            local.x,
                                            &world_down_theme,
                                        );
                                    }
                                } else {
                                    this.begin_node_drag(
                                        &node_id,
                                        local,
                                        event.modifiers.shift,
                                        event.modifiers.alt,
                                        cx,
                                    );
                                }
                            } else {
                                this.begin_canvas_selection(
                                    local,
                                    event.modifiers.shift,
                                    &world_down_theme,
                                    cx,
                                );
                                window.prevent_default();
                            }
                        }),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                            cx.stop_propagation();
                            let local = this.local_screen(event.position);
                            let preserve_click_draft = this
                                .draft
                                .as_ref()
                                .and_then(|draft| {
                                    let port = this.port_at_screen(
                                        local,
                                        this.graph
                                            .viewport
                                            .scale_length(world_up_theme.anchor.dot_size * 0.5),
                                    )?;
                                    Some(port == draft.origin)
                                })
                                .unwrap_or(false);
                            this.finish_left_gesture(preserve_click_draft, &world_up_theme, cx);
                        }),
                    ),
            );
        }

        for (
            control_id,
            label,
            node_id,
            bounds,
            active,
            semantic_role,
            semantic_value,
            numeric_value,
            minimum,
            maximum,
            step,
        ) in semantic_world_controls
        {
            let pointer_node_id = node_id.clone();
            let pointer_control_id = control_id.clone();
            let a11y_node_id = node_id.clone();
            let a11y_control_id = control_id.clone();
            let editor = cx.entity().downgrade();
            let control_theme = Arc::clone(&theme);
            root = root.child(
                div()
                    .id(control_id)
                    .absolute()
                    .left(px(bounds.origin.x))
                    .top(px(bounds.origin.y))
                    .w(px(bounds.size.width))
                    .h(px(bounds.size.height))
                    .role(match semantic_role {
                        world::AccessibleControlRole::Button => gpui::Role::Button,
                        world::AccessibleControlRole::TextInput => gpui::Role::TextInput,
                        world::AccessibleControlRole::ComboBox => gpui::Role::ComboBox,
                        world::AccessibleControlRole::Slider => gpui::Role::Slider,
                        world::AccessibleControlRole::SpinButton => gpui::Role::SpinButton,
                    })
                    .aria_label(label)
                    .when_some(semantic_value, |element, value| element.aria_value(value))
                    .when_some(numeric_value, |element, value| {
                        element.aria_numeric_value(value)
                    })
                    .when_some(minimum, |element, value| {
                        element.aria_min_numeric_value(value)
                    })
                    .when_some(maximum, |element, value| {
                        element.aria_max_numeric_value(value)
                    })
                    .when_some(step, |element, value| {
                        element.aria_numeric_value_step(value)
                    })
                    .when(active, |element| element.aria_active_descendant())
                    .on_a11y_action(gpui::AccessibleAction::Click, move |_, _, cx| {
                        let _ = editor.update(cx, |this, cx| {
                            this.activate_world_control(
                                a11y_node_id.clone(),
                                a11y_control_id.clone(),
                                cx,
                            );
                        });
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.focus(window, cx);
                            this.commit_group_editor(cx);
                            let local = this.local_screen(event.position);
                            this.dismiss_overlays_before_world_pointer(local, cx);
                            if let Some(port_id) = this.port_at_screen(
                                local,
                                this.graph
                                    .viewport
                                    .scale_length(control_theme.anchor.dot_size * 0.5),
                            ) {
                                this.engage_port(&port_id, cx);
                                return;
                            }
                            this.activate_world_control(
                                pointer_node_id.clone(),
                                pointer_control_id.clone(),
                                cx,
                            );
                            cx.emit(core::GraphEvent::NodeControlPointerActivated {
                                node_id: pointer_node_id.clone(),
                                control_id: pointer_control_id.clone(),
                                world_position: this.graph.viewport.screen_to_world(local),
                                click_count: event.click_count,
                            });
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
                    .border(px(visible_border_width(theme.selection_box.border)))
                    .when(
                        theme.selection_box.border.style == style::LineStyle::Dashed,
                        |element| element.border_dashed(),
                    )
                    .border_color(
                        rgb(theme.selection_box.border.color.rgb)
                            .opacity(theme.selection_box.border.color.alpha),
                    )
                    .bg(rgb(theme.selection_box.background.rgb)
                        .opacity(theme.selection_box.background.alpha)),
            );
        }

        let world_root = div()
            .absolute()
            .left_0()
            .top_0()
            .size_full()
            .when(clip_world_content, |element| element.overflow_hidden())
            .child(root);
        let mut overlay_root = div()
            .absolute()
            .left_0()
            .top_0()
            .size_full()
            .when(clip_overlay_content, |element| element.overflow_hidden());

        if !self.active_backdrop_overlays.is_empty() {
            let backdrop = theme.overlay.backdrop_background;
            overlay_root = overlay_root.child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .size_full()
                    .bg(rgb(theme.overlay.layer_background.rgb)
                        .opacity(theme.overlay.layer_background.alpha))
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .top_0()
                            .size_full()
                            .bg(rgb(backdrop.rgb).opacity(backdrop.alpha)),
                    )
                    .when(theme.overlay.backdrop_pointer_events, |element| {
                        element.occlude().on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _: &MouseDownEvent, _window, cx| {
                                this.dismiss_outside_overlays(cx);
                            }),
                        )
                    }),
            );
        }

        for (position, overlay, measurement_id) in node_overlays {
            let panel = theme.overlay.panel_background;
            let panel_border = theme.overlay.panel_border;
            let panel = div()
                .absolute()
                .left(px(position.x))
                .top(px(position.y))
                .bg(rgb(panel.rgb).opacity(panel.alpha))
                .border(px(visible_border_width(panel_border)))
                .when(panel_border.style == style::LineStyle::Dashed, |element| {
                    element.border_dashed()
                })
                .border_color(rgb(panel_border.color.rgb).opacity(panel_border.color.alpha))
                .when(theme.overlay.panel_pointer_events, |element| {
                    element.occlude()
                })
                .when(theme.editor.overlay_isolated, |element| {
                    element.on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                })
                .child(overlay);
            if let Some(measurement_id) = measurement_id {
                let graph = cx.weak_entity();
                overlay_root =
                    overlay_root.child(MeasuredElement::new(panel, move |bounds, cx| {
                        let measured = core::Size {
                            width: f32::from(bounds.size.width),
                            height: f32::from(bounds.size.height),
                        };
                        if !measured.width.is_finite()
                            || !measured.height.is_finite()
                            || measured.width <= 0.0
                            || measured.height <= 0.0
                        {
                            return;
                        }
                        let graph = graph.clone();
                        let measurement_id = measurement_id.clone();
                        cx.defer(move |cx| {
                            let _ = graph.update(cx, |editor, cx| {
                                let changed = editor
                                    .measured_overlay_sizes
                                    .get(&measurement_id)
                                    .is_none_or(|current| {
                                        (current.width - measured.width).abs() > 0.1
                                            || (current.height - measured.height).abs() > 0.1
                                    });
                                if changed {
                                    editor
                                        .measured_overlay_sizes
                                        .insert(measurement_id, measured);
                                    cx.notify();
                                }
                            });
                        });
                    }));
            } else {
                overlay_root = overlay_root.child(panel);
            }
        }

        if let Some(menu) = self.catalog_menu.as_ref() {
            let menu_style = &theme.menu;
            let raw_anchor = viewport.world_to_screen(menu.anchor_world);
            let canvas = self.canvas_bounds.get();
            let menu_width = menu_style.min_width;
            let anchor = core::Point::new(
                raw_anchor.x.clamp(
                    menu_style.viewport_margin,
                    (f32::from(canvas.size.width) - menu_width - menu_style.viewport_margin)
                        .max(menu_style.viewport_margin),
                ),
                raw_anchor.y.clamp(
                    menu_style.viewport_margin,
                    (f32::from(canvas.size.height)
                        - menu_style.max_height
                        - menu_style.viewport_margin)
                        .max(menu_style.viewport_margin),
                ),
            );
            let selected = menu.selected;
            let query_state = menu.query.clone();
            let query = query_state.text.clone();
            let query_display = text_with_caret(&query_state);
            let filtered = self.filtered_catalog_indices();
            let connect_from = menu.connect_from.clone();
            let border = menu_style.border;
            let input_border = menu_style.input_border;
            let search_padding = menu_style.search_padding;
            let input_padding_x = menu_style.input_padding_x;
            let menu_shadows = menu_style
                .shadow
                .iter()
                .map(|shadow| {
                    BoxShadow::new(
                        px(shadow.offset_x),
                        px(shadow.offset_y),
                        rgb(shadow.color.rgb).opacity(shadow.color.alpha).into(),
                    )
                    .blur_radius(px(shadow.blur * 0.5))
                    .spread_radius(px(shadow.spread))
                })
                .collect::<Vec<_>>();
            let menu_element = div()
                .absolute()
                .flex()
                .flex_col()
                .left(px(anchor.x))
                .top(px(anchor.y))
                .w(px(menu_width + visible_border_width(border) * 2.0))
                .max_h(px(
                    menu_style.max_height + visible_border_width(border) * 2.0
                ))
                .overflow_hidden()
                .rounded(px(menu_style.border_radius))
                .shadow(menu_shadows)
                .border(px(visible_border_width(border)))
                .when(border.style == style::LineStyle::Dashed, |element| {
                    element.border_dashed()
                })
                .border_color(rgb(border.color.rgb).opacity(border.color.alpha))
                .bg(rgb(menu_style.background.rgb).opacity(menu_style.background.alpha))
                .text_color(rgb(menu_style.item_color.rgb).opacity(menu_style.item_color.alpha))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .p(px(menu_style.search_padding))
                        .border_b(px(visible_border_width(menu_style.divider)))
                        .when(
                            menu_style.divider.style == style::LineStyle::Dashed,
                            |element| element.border_dashed(),
                        )
                        .border_color(
                            rgb(menu_style.divider.color.rgb)
                                .opacity(menu_style.divider.color.alpha),
                        )
                        .child(
                            div()
                                .id(("catalog-search", cx.entity().entity_id()))
                                .role(gpui::Role::TextInput)
                                .aria_label("Search nodes")
                                .aria_placeholder("Search nodes")
                                .aria_value(query.clone())
                                .flex()
                                .items_center()
                                .rounded(px(menu_style.input_border_radius))
                                .border(px(visible_border_width(input_border)))
                                .when(input_border.style == style::LineStyle::Dashed, |element| {
                                    element.border_dashed()
                                })
                                .border_color(
                                    rgb(input_border.color.rgb).opacity(input_border.color.alpha),
                                )
                                .bg(rgb(menu_style.input_background.rgb)
                                    .opacity(menu_style.input_background.alpha))
                                .px(px(menu_style.input_padding_x))
                                .py(px(menu_style.input_padding_y))
                                .text_size(px(menu_style.input_font_size))
                                .line_height(px(menu_style.input_font_size + 2.0))
                                .text_color(if query.is_empty() {
                                    rgb(menu_style.placeholder_color.rgb)
                                        .opacity(menu_style.placeholder_color.alpha)
                                } else {
                                    rgb(menu_style.input_color.rgb)
                                        .opacity(menu_style.input_color.alpha)
                                })
                                .child(if query.is_empty() {
                                    "Search nodes...".to_string()
                                } else {
                                    query_display.clone()
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                        cx.stop_propagation();
                                        window.prevent_default();
                                        this.focus(window, cx);
                                        let local = this.local_screen(event.position);
                                        if let Some(menu) = this.catalog_menu.as_mut() {
                                            let left = anchor.x
                                                + search_padding
                                                + visible_border_width(input_border)
                                                + input_padding_x;
                                            let width = (menu_width
                                                - search_padding * 2.0
                                                - input_padding_x * 2.0)
                                                .max(1.0);
                                            let fraction =
                                                ((local.x - left) / width).clamp(0.0, 1.0);
                                            let boundaries = text_boundaries(&menu.query.text);
                                            let index = ((boundaries.len().saturating_sub(1) as f32
                                                * fraction)
                                                .round()
                                                as usize)
                                                .min(boundaries.len().saturating_sub(1));
                                            move_text_selection(
                                                &mut menu.query,
                                                boundaries[index],
                                                event.modifiers.shift,
                                            );
                                            cx.notify();
                                        }
                                    }),
                                ),
                        ),
                );
            let mut list_element = div()
                .id("node-graph-catalog-list")
                .role(gpui::Role::Menu)
                .aria_label("Node catalog")
                .flex_1()
                .overflow_y_scroll()
                .track_scroll(&self.catalog_scroll_handle)
                .py(px(menu_style.list_padding_y));
            if filtered.is_empty() {
                list_element = list_element.child(
                    div()
                        .p_2()
                        .text_size(px(12.0))
                        .text_color(
                            rgb(menu_style.empty_color.rgb).opacity(menu_style.empty_color.alpha),
                        )
                        .child("No nodes found"),
                );
            }
            let mut previous_category: Option<String> = None;
            let mut entry_index = 0usize;
            for item_index in filtered {
                let item = &self.catalog[item_index];
                let compatible = connect_from
                    .as_ref()
                    .map(|origin| self.compatible_catalog_port_indices(item_index, origin))
                    .unwrap_or_default();
                let connects_draft = connect_from.is_some();
                let consumed = compatible.len().max(1);
                let base_entry = entry_index;
                entry_index += consumed;
                if previous_category.as_deref() != Some(item.category.as_str()) {
                    previous_category = Some(item.category.clone());
                    let category_color = catalog_category_color(item, menu_style.category_color);
                    list_element = list_element.child(
                        div()
                            .pt(px(4.0))
                            .pb(px(2.0))
                            .px(px(12.0))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(9.0))
                            .text_color(rgb(category_color.rgb).opacity(category_color.alpha))
                            .child(item.category.to_uppercase()),
                    );
                }
                let hover = menu_style.hover_background;
                let item_id = item_index;
                // A node row is informational while completing a draft. Even when
                // there is only one compatible pin, selection belongs to that pin.
                let mut item_element = div()
                    .id(("catalog-item", item_index))
                    .px(px(12.0))
                    .py(px(6.0))
                    .when(!connects_draft, |element| {
                        element
                            .role(gpui::Role::MenuItem)
                            .aria_label(item.label.clone())
                    })
                    .when(!connects_draft && base_entry == selected, |element| {
                        element
                            .bg(rgb(hover.rgb).opacity(hover.alpha))
                            .aria_active_descendant()
                    })
                    .child(div().text_size(px(12.0)).child(item.label.clone()))
                    .child(
                        div()
                            .mt(px(2.0))
                            .text_size(px(10.0))
                            .text_color(
                                rgb(menu_style.description_color.rgb)
                                    .opacity(menu_style.description_color.alpha),
                            )
                            .child(item.description.clone()),
                    );
                if !connects_draft {
                    item_element = item_element
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered
                                && let Some(menu) = this.catalog_menu.as_mut()
                                && menu.selected != base_entry
                            {
                                menu.selected = base_entry;
                                cx.notify();
                            }
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                window.prevent_default();
                                this.choose_catalog(item_id, None, cx);
                            }),
                        );
                } else {
                    list_element = list_element.child(item_element);
                    for (port_offset, port_index) in compatible.into_iter().enumerate() {
                        let entry = base_entry + port_offset;
                        let port = &item.ports[port_index];
                        let direction = match port.direction {
                            PortDirection::Input => "› ",
                            PortDirection::Output => "‹ ",
                        };
                        let label = format!("{direction}{}", port.label);
                        list_element = list_element.child(
                            div()
                                .id(format!("catalog-port-{item_id}-{port_index}"))
                                .role(gpui::Role::MenuItem)
                                .aria_label(port.label.clone())
                                .ml(px(20.0))
                                .mr(px(4.0))
                                .pl(px(12.0))
                                .pr(px(4.0))
                                .py(px(3.0))
                                .text_size(px(11.0))
                                .text_color(
                                    rgb(menu_style.port_color.rgb)
                                        .opacity(menu_style.port_color.alpha),
                                )
                                .when(entry == selected, |element| {
                                    element
                                        .bg(rgb(hover.rgb).opacity(hover.alpha))
                                        .aria_active_descendant()
                                })
                                .child(label)
                                .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                    if *hovered
                                        && let Some(menu) = this.catalog_menu.as_mut()
                                        && menu.selected != entry
                                    {
                                        menu.selected = entry;
                                        cx.notify();
                                    }
                                }))
                                .on_mouse_up(
                                    MouseButton::Left,
                                    cx.listener(move |this, _, window, cx| {
                                        cx.stop_propagation();
                                        window.prevent_default();
                                        this.choose_catalog(item_id, Some(port_index), cx);
                                    }),
                                ),
                        );
                    }
                    continue;
                }
                list_element = list_element.child(item_element);
            }
            overlay_root = overlay_root.child(menu_element.child(list_element));
        }

        if let Some(port_id) = self.hovered_port.as_ref()
            && let Some(port) = self.graph.ports.get(port_id)
        {
            let position = viewport.world_to_screen(
                self.resolved_port_position(port_id)
                    .unwrap_or(port.position),
            );
            let type_label = port.kind.type_label();
            let label = if port.direction == PortDirection::Output
                && !port.label.eq_ignore_ascii_case(&type_label)
            {
                format!("{} ({type_label})", port.label)
            } else {
                type_label
            };
            let width = label.chars().count() as f32 * 6.0 + 12.0;
            let dot_radius = viewport.scale_length(theme.anchor.dot_size * 0.5);
            let left = if port.direction == PortDirection::Output {
                position.x - dot_radius - 12.0 - width
            } else {
                position.x + dot_radius + 12.0
            };
            let border = theme.anchor.tooltip_border;
            let background = theme.anchor.tooltip_background;
            let color = theme.anchor.tooltip_color;
            overlay_root = overlay_root.child(
                div()
                    .absolute()
                    .left(px(left))
                    .top(px(position.y - 8.0))
                    .w(px(width))
                    .px(px(6.0))
                    .py(px(2.0))
                    .rounded(px(4.0))
                    .whitespace_nowrap()
                    .text_size(px(10.0))
                    .text_color(rgb(color.rgb).opacity(color.alpha))
                    .bg(rgb(background.rgb).opacity(background.alpha))
                    .when(visible_border_width(border) > 0.0, |element| {
                        element
                            .border(px(visible_border_width(border)))
                            .border_color(rgb(border.color.rgb).opacity(border.color.alpha))
                            .when(border.style == style::LineStyle::Dashed, |element| {
                                element.border_dashed()
                            })
                    })
                    .child(label),
            );
        }

        if self.world_text_input.is_some() {
            let input_graph = cx.weak_entity();
            let input_focus = focus_handle.clone();
            overlay_root = overlay_root.child(div().absolute().size_full().child(canvas(
                |_, _, _| (),
                move |bounds, _, window, cx| {
                    if let Some(graph) = input_graph.upgrade() {
                        window.handle_input(
                            &input_focus,
                            ElementInputHandler::new(bounds, graph),
                            cx,
                        );
                    }
                },
            )));
        }

        if let Some(menu) = self.anchor_menu.clone() {
            let menu_style = theme.menu.clone();
            let canvas = self.canvas_bounds.get();
            let menu_width = 180.0;
            let estimated_height = 8.0 + menu.items.len() as f32 * 30.0;
            let position = core::Point::new(
                menu.position.x.clamp(
                    menu_style.viewport_margin,
                    (f32::from(canvas.size.width) - menu_width - menu_style.viewport_margin)
                        .max(menu_style.viewport_margin),
                ),
                menu.position.y.clamp(
                    menu_style.viewport_margin,
                    (f32::from(canvas.size.height) - estimated_height - menu_style.viewport_margin)
                        .max(menu_style.viewport_margin),
                ),
            );
            let border = menu_style.border;
            let shadows = menu_style
                .shadow
                .iter()
                .map(|shadow| {
                    BoxShadow::new(
                        px(shadow.offset_x),
                        px(shadow.offset_y),
                        rgb(shadow.color.rgb).opacity(shadow.color.alpha).into(),
                    )
                    .blur_radius(px(shadow.blur * 0.5))
                    .spread_radius(px(shadow.spread))
                })
                .collect::<Vec<_>>();
            let mut menu_element = div()
                .absolute()
                .left(px(position.x))
                .top(px(position.y))
                .w(px(menu_width))
                .py(px(4.0))
                .overflow_hidden()
                .rounded(px(6.0))
                .shadow(shadows)
                .when(visible_border_width(border) > 0.0, |element| {
                    element
                        .border(px(visible_border_width(border)))
                        .border_color(rgb(border.color.rgb).opacity(border.color.alpha))
                        .when(border.style == style::LineStyle::Dashed, |element| {
                            element.border_dashed()
                        })
                })
                .bg(rgb(menu_style.background.rgb).opacity(menu_style.background.alpha))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
            for (index, item) in menu.items.iter().enumerate() {
                let enabled = item.enabled;
                let item_color = menu_style.item_color;
                menu_element = menu_element.child(
                    div()
                        .px(px(12.0))
                        .py(px(6.0))
                        .text_size(px(12.0))
                        .text_color(rgb(item_color.rgb).opacity(item_color.alpha))
                        .opacity(if enabled { 1.0 } else { 0.35 })
                        .when(enabled, |element| {
                            element.cursor_pointer().on_mouse_up(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    cx.stop_propagation();
                                    window.prevent_default();
                                    this.execute_anchor_menu_item(index, cx);
                                }),
                            )
                        })
                        .child(item.label.clone()),
                );
            }
            overlay_root = overlay_root.child(menu_element);
        }

        let root = div()
            .relative()
            .size_full()
            .child(world_root)
            .child(overlay_root);
        let root_right_theme = Arc::clone(&theme);
        let root_down_theme = Arc::clone(&theme);
        let root_move_theme = Arc::clone(&theme);
        let root_up_theme = Arc::clone(&theme);
        let root_up_out_theme = Arc::clone(&theme);

        root.on_mouse_down(
            MouseButton::Right,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                this.focus(window, cx);
                let local = this.local_screen(event.position);
                this.commit_group_editor(cx);
                if let Some(port_id) = this.port_at_screen(
                    local,
                    this.graph
                        .viewport
                        .scale_length(root_right_theme.anchor.dot_size * 0.5),
                ) {
                    this.open_anchor_menu(port_id, local, cx);
                } else {
                    this.anchor_menu = None;
                    cx.notify();
                }
            }),
        )
        .on_mouse_down(
            MouseButton::Middle,
            cx.listener(|this, event: &MouseDownEvent, window, cx| {
                this.anchor_menu = None;
                this.focus(window, cx);
                this.commit_group_editor(cx);
                this.panning = Some(this.local_screen(event.position));
                cx.stop_propagation();
                window.prevent_default();
            }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.anchor_menu = None;
                this.focus(window, cx);
                let local = this.local_screen(event.position);
                this.commit_group_editor(cx);
                if event.click_count >= 2 && this.draft.is_none() {
                    if let Some(group_id) = this.group_label_at(local, &root_down_theme)
                        && let Some(group) = this.groups.iter().find(|group| group.id == group_id)
                    {
                        this.group_editor = Some(GroupEditor {
                            id: group.id.clone(),
                            query: WorldTextInputState::at_end(
                                group.label.clone().unwrap_or_default(),
                            ),
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
                this.begin_canvas_selection(local, event.modifiers.shift, &root_down_theme, cx);
                window.prevent_default();
            }),
        )
        .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
            let local = this.local_screen(event.position);
            this.handle_pointer_move(local, &root_move_theme, cx);
        }))
        .on_mouse_up(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseUpEvent, _, cx| {
                let local = this.local_screen(event.position);
                let preserve_click_draft = this
                    .draft
                    .as_ref()
                    .and_then(|draft| {
                        let port = this.port_at_screen(
                            local,
                            this.graph
                                .viewport
                                .scale_length(root_up_theme.anchor.dot_size * 0.5),
                        )?;
                        Some(port == draft.origin)
                    })
                    .unwrap_or(false);
                this.finish_left_gesture(preserve_click_draft, &root_up_theme, cx);
            }),
        )
        .on_mouse_up_out(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseUpEvent, _, cx| {
                this.finish_left_gesture(false, &root_up_out_theme, cx)
            }),
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
            this.handle_scroll_wheel(event, window, cx);
        }))
    }
}

fn resolved_node_cursor(
    node: &style::NodeStyle,
    dragging: bool,
    any_node_resizing: bool,
) -> style::Cursor {
    if any_node_resizing {
        node.cursor_resize
    } else if dragging {
        node.cursor_dragging
    } else {
        node.cursor
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PortPaint {
    stroke: style::Color,
    fill: style::Color,
    label: style::Color,
    opacity: f32,
    glow: bool,
}

fn resolved_port_paint(
    anchor: &style::AnchorStyle,
    connected: bool,
    source: bool,
    snap: bool,
    compatible: bool,
    draft_active: bool,
) -> PortPaint {
    let highlighted = source || snap || compatible;
    let color = if highlighted {
        anchor.dot_compatible_color
    } else if connected {
        anchor.dot_connected_color
    } else {
        anchor.dot_color
    };
    PortPaint {
        stroke: color,
        fill: if connected || highlighted {
            color
        } else {
            style::Color::TRANSPARENT
        },
        label: if compatible {
            anchor.label_compatible_color
        } else {
            anchor.label_color
        },
        opacity: if draft_active && !highlighted {
            anchor.incompatible_opacity
        } else {
            1.0
        },
        glow: source || compatible,
    }
}

fn resolved_group_color(color: Option<style::Color>, default: style::Color) -> style::Color {
    color.unwrap_or(default)
}

fn visible_border_width(border: style::Border) -> f32 {
    if border.style == style::LineStyle::None {
        0.0
    } else {
        border.width.max(0.0)
    }
}

fn mix_color(a: style::Color, b: style::Color, a_weight: f32) -> style::Color {
    let weight = a_weight.clamp(0.0, 1.0);
    let mix_channel = |shift: u32| {
        let a = ((a.rgb >> shift) & 0xff_u32) as f32;
        let b = ((b.rgb >> shift) & 0xff_u32) as f32;
        (a * weight + b * (1.0 - weight)).round() as u32
    };
    style::Color::rgba(
        (mix_channel(16) << 16) | (mix_channel(8) << 8) | mix_channel(0),
        a.alpha * weight + b.alpha * (1.0 - weight),
    )
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

fn positioned_overlay_candidate(
    anchor: core::Rect,
    side: OverlaySide,
    align: OverlayAlign,
    gap: f32,
    panel_size: core::Size,
) -> core::Point {
    let aligned_x = match align {
        OverlayAlign::Start => anchor.origin.x,
        OverlayAlign::Center => anchor.origin.x + (anchor.size.width - panel_size.width) * 0.5,
        OverlayAlign::End => anchor.origin.x + anchor.size.width - panel_size.width,
    };
    let aligned_y = match align {
        OverlayAlign::Start => anchor.origin.y,
        OverlayAlign::Center => anchor.origin.y + (anchor.size.height - panel_size.height) * 0.5,
        OverlayAlign::End => anchor.origin.y + anchor.size.height - panel_size.height,
    };
    match side {
        OverlaySide::Top => core::Point::new(aligned_x, anchor.origin.y - panel_size.height - gap),
        OverlaySide::Right => {
            core::Point::new(anchor.origin.x + anchor.size.width + gap, aligned_y)
        }
        OverlaySide::Bottom => {
            core::Point::new(aligned_x, anchor.origin.y + anchor.size.height + gap)
        }
        OverlaySide::Left => core::Point::new(anchor.origin.x - panel_size.width - gap, aligned_y),
    }
}

fn opposite_overlay_side(side: OverlaySide) -> OverlaySide {
    match side {
        OverlaySide::Top => OverlaySide::Bottom,
        OverlaySide::Right => OverlaySide::Left,
        OverlaySide::Bottom => OverlaySide::Top,
        OverlaySide::Left => OverlaySide::Right,
    }
}

fn overlay_fits_primary_axis(
    position: core::Point,
    side: OverlaySide,
    panel_size: core::Size,
    canvas_size: core::Size,
) -> bool {
    match side {
        OverlaySide::Top | OverlaySide::Bottom => {
            position.y >= 0.0 && position.y + panel_size.height <= canvas_size.height
        }
        OverlaySide::Right | OverlaySide::Left => {
            position.x >= 0.0 && position.x + panel_size.width <= canvas_size.width
        }
    }
}

fn resolve_positioned_overlay(
    anchor: core::Rect,
    placement: OverlayPlacement,
    panel_size: core::Size,
    canvas_size: core::Size,
) -> core::Point {
    let gap = placement.gap.max(0.0);
    let side = placement.side;
    let mut position = positioned_overlay_candidate(anchor, side, placement.align, gap, panel_size);
    if placement.flip && !overlay_fits_primary_axis(position, side, panel_size, canvas_size) {
        let opposite = opposite_overlay_side(side);
        let opposite_position =
            positioned_overlay_candidate(anchor, opposite, placement.align, gap, panel_size);
        if overlay_fits_primary_axis(opposite_position, opposite, panel_size, canvas_size) {
            position = opposite_position;
        }
    }
    if placement.clamp_to_canvas && canvas_size.width > 0.0 && canvas_size.height > 0.0 {
        position.x = position
            .x
            .clamp(0.0, (canvas_size.width - panel_size.width).max(0.0));
        position.y = position
            .y
            .clamp(0.0, (canvas_size.height - panel_size.height).max(0.0));
    }
    position
}

fn resolve_overlay_position(
    node_origin: core::Point,
    node_width: f32,
    offset: core::Point,
    behavior: &OverlayBehavior,
    panel_size: core::Size,
    canvas_size: core::Size,
) -> core::Point {
    let mut position = node_origin + offset;
    if behavior.flip_horizontal
        && canvas_size.width > 0.0
        && position.x + panel_size.width > canvas_size.width
    {
        let gap = (offset.x - node_width).max(0.0);
        let flipped_x = node_origin.x - panel_size.width - gap;
        if flipped_x >= 0.0 {
            position.x = flipped_x;
        }
    }
    if behavior.clamp_to_canvas && canvas_size.width > 0.0 && canvas_size.height > 0.0 {
        position.x = position
            .x
            .clamp(0.0, (canvas_size.width - panel_size.width).max(0.0));
        position.y = position
            .y
            .clamp(0.0, (canvas_size.height - panel_size.height).max(0.0));
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

fn paint_dashed_route(
    window: &mut Window,
    bounds: Bounds<Pixels>,
    points: &[core::Point],
    color: gpui::Hsla,
    width: f32,
    dash: f32,
    gap: f32,
) {
    for segment in points.windows(2) {
        let start = segment[0];
        let end = segment[1];
        let delta = end - start;
        let length = (delta.x * delta.x + delta.y * delta.y).sqrt();
        if length <= f32::EPSILON {
            continue;
        }
        let unit = core::Point::new(delta.x / length, delta.y / length);
        let mut offset = 0.0;
        while offset < length {
            let dash_end = (offset + dash).min(length);
            paint_route(
                window,
                bounds,
                [
                    start + core::Point::new(unit.x * offset, unit.y * offset),
                    start + core::Point::new(unit.x * dash_end, unit.y * dash_end),
                ],
                color,
                width,
                0.0,
            );
            offset += dash + gap;
        }
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

    #[gpui::test]
    fn application_theme_is_explicit_and_preserves_arc_identity(cx: &mut gpui::TestAppContext) {
        let installed = Arc::new(NodeGraphTheme::leptos_demo());
        cx.update(|app| set_node_graph_theme(app, Arc::clone(&installed)));
        cx.update(|app| assert!(Arc::ptr_eq(app.node_graph_theme(), &installed)));

        let replacement = Arc::new(NodeGraphTheme::light());
        cx.update(|app| set_node_graph_theme(app, Arc::clone(&replacement)));
        cx.update(|app| assert!(Arc::ptr_eq(app.node_graph_theme(), &replacement)));
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
                    node_type: id.into(),
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
    fn registry_port_slots_match_leptos_one_label_argument_contract() {
        let specific_labels = Rc::new(RefCell::new(Vec::new()));
        let captured = specific_labels.clone();
        let definition = NodeTypeBuilder::<Kind, String, String, String>::new("math", "Math")
            .input("left", "Left", Kind)
            .port_slot("left", move |label| {
                captured.borrow_mut().push(label);
                div().into_any_element()
            })
            .build()
            .unwrap();
        (definition.port_slots["left"])("Left".into());
        assert_eq!(&*specific_labels.borrow(), &["Left"]);

        let global_labels = Rc::new(RefCell::new(Vec::new()));
        let captured = global_labels.clone();
        let mut registry: NodeTypeRegistry<Kind, String, String, String> =
            NodeTypeRegistry::new(|node: &String, key| format!("{node}:{key}"));
        registry.register_port_type_slot(Kind, PortDirection::Input, move |label| {
            captured.borrow_mut().push(label);
            div().into_any_element()
        });
        (registry.port_type_slots[0].2)("Global".into());
        assert_eq!(&*global_labels.borrow(), &["Global"]);
    }

    #[test]
    fn node_type_registry_preserves_catalog_and_static_port_order() {
        let definition = NodeTypeBuilder::<Kind, String, String, String>::new("math", "Math")
            .category("Core", Some(style::Color::rgb(0x123456)))
            .input("left", "Left", Kind)
            .output("result", "Result", Kind)
            .build()
            .unwrap();
        let mut registry = NodeTypeRegistry::new(|node: &String, key| format!("{node}:{key}"));
        registry.register(definition).unwrap();
        let catalog = registry.catalog();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].id, "math");
        assert_eq!(
            catalog[0]
                .ports
                .iter()
                .map(|port| port.id.as_str())
                .collect::<Vec<_>>(),
            ["left", "result"]
        );
        assert!(matches!(
            registry.register(
                NodeTypeBuilder::<Kind, String, String, String>::new("math", "Again")
                    .build()
                    .unwrap()
            ),
            Err(RegistryError::DuplicateNodeType(_))
        ));
    }

    #[gpui::test]
    fn registry_dynamic_port_shrink_and_grow_restores_strict_connection(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let enabled = Rc::new(Cell::new(true));
        let producer_enabled = enabled.clone();
        let definition = NodeTypeBuilder::<Kind, String, String, String>::new("a", "A")
            .dynamic_outputs(move |_| {
                producer_enabled
                    .get()
                    .then(|| DynamicPort {
                        key: "out".into(),
                        label: "Out".into(),
                        kind: Kind,
                    })
                    .into_iter()
                    .collect()
            })
            .build()
            .unwrap();
        let mut registry = NodeTypeRegistry::new(|node: &String, key| {
            if node == "a" && key == "out" {
                "out".to_string()
            } else {
                format!("{node}:{key}")
            }
        });
        registry.register(definition).unwrap();
        let editor =
            cx.new(|_| NodeGraph::new(interactive_graph()).with_node_type_registry(registry));
        editor.update(cx, |editor, cx| {
            assert!(!editor.refresh_node_types(cx).unwrap());
            assert_eq!(editor.defined_port_order["a"], [String::from("out")]);

            enabled.set(false);
            assert!(editor.refresh_node_types(cx).unwrap());
            assert!(!editor.graph.ports.contains_key("out"));
            assert!(editor.graph.connections.is_empty());
            assert_eq!(editor.dangling_connections.len(), 1);
            editor.graph.validate().unwrap();

            enabled.set(true);
            assert!(editor.refresh_node_types(cx).unwrap());
            assert!(editor.graph.ports.contains_key("out"));
            assert!(editor.graph.connections.contains_key("wire"));
            assert!(editor.dangling_connections.is_empty());
            editor.graph.validate().unwrap();
        });
    }

    #[gpui::test]
    fn controlled_registry_refresh_is_atomic_and_deduplicated(cx: &mut gpui::TestAppContext) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let enabled = Rc::new(Cell::new(true));
        let producer_enabled = enabled.clone();
        let definition = NodeTypeBuilder::<Kind, String, String, String>::new("a", "A")
            .dynamic_outputs(move |_| {
                producer_enabled
                    .get()
                    .then(|| DynamicPort {
                        key: "out".into(),
                        label: "Out".into(),
                        kind: Kind,
                    })
                    .into_iter()
                    .collect()
            })
            .build()
            .unwrap();
        let mut registry = NodeTypeRegistry::new(|_: &String, _: &str| "out".to_string());
        registry.register(definition).unwrap();
        let editor =
            cx.new(|_| NodeGraph::new(interactive_graph()).with_node_type_registry(registry));
        editor.update(cx, |editor, cx| {
            assert!(!editor.refresh_node_types(cx).unwrap());
            editor.config.mutation_mode = MutationMode::Controlled;
            enabled.set(false);
            assert!(editor.refresh_node_types(cx).unwrap());
            assert!(editor.graph.ports.contains_key("out"));
            assert!(editor.graph.connections.contains_key("wire"));
            assert_eq!(editor.dangling_connections.len(), 1);
            assert!(editor.pending_port_changes.contains_key("a"));
            assert!(!editor.refresh_node_types(cx).unwrap());

            let mut host = editor.graph.clone();
            host.connections.remove("wire");
            host.ports.remove("out");
            editor.set_graph(host, cx).unwrap();
            assert!(!editor.refresh_node_types(cx).unwrap());
            assert!(editor.defined_port_order["a"].is_empty());
            assert!(editor.pending_port_changes.is_empty());
        });
    }

    #[test]
    fn default_anchor_menu_tracks_live_and_broken_connections() {
        let mut editor = NodeGraph::new(interactive_graph());
        let items = editor.anchor_menu_items(&"out".into());
        assert!(items[0].enabled);
        assert!(!items[1].enabled);
        editor.remove_port_to_tombstones(&"in".into()).unwrap();
        let items = editor.anchor_menu_items(&"out".into());
        assert!(items[0].enabled);
        assert!(items[1].enabled);
    }

    #[test]
    fn node_drop_preserves_the_consumer_catalog_id() {
        let drop = NodeDrop::new("color_source");
        assert_eq!(drop.item_id, "color_source");
    }

    #[test]
    fn world_text_input_uses_utf16_selection_and_marked_replacement() {
        let mut editor = NodeGraph::new(interactive_graph());
        editor.last_world_control = Some(("a".into(), "a:factor".into()));
        editor.world_text_input = Some((
            "a".into(),
            "a:factor".into(),
            WorldTextInputState {
                text: "a💡b".into(),
                selection: 1..3,
                selection_reversed: false,
                marked: None,
            },
        ));
        assert_eq!(utf16_len("a💡b"), 4);
        assert_eq!(utf16_to_byte("a💡b", 2), (1, 1));
        editor.replace_world_text(None, "é", None);
        let state = &editor.world_text_input.as_ref().unwrap().2;
        assert_eq!(state.text, "aéb");
        assert_eq!(state.selection, 2..2);

        editor.replace_world_text(None, "漢", Some(0..1));
        let state = &editor.world_text_input.as_ref().unwrap().2;
        assert_eq!(state.text, "aé漢b");
        assert_eq!(state.marked, Some(2..3));
        assert_eq!(state.selection, 2..3);
        editor.replace_world_text(None, "字", None);
        let state = &editor.world_text_input.as_ref().unwrap().2;
        assert_eq!(state.text, "aé字b");
        assert_eq!(state.marked, None);
        assert_eq!(state.selection, 3..3);
    }

    #[test]
    fn client_canvas_conversion_uses_layout_origin_and_inverse_viewport() {
        let mut graph = interactive_graph();
        graph.viewport.zoom = 2.0;
        graph.viewport.pan = core::Point::new(30.0, -10.0);
        let editor = NodeGraph::new(graph);
        assert_eq!(editor.client_to_canvas(core::Point::new(0.0, 0.0)), None);
        editor.canvas_bounds.set(Bounds {
            origin: gpui::Point::new(px(100.0), px(50.0)),
            size: gpui::Size {
                width: px(800.0),
                height: px(600.0),
            },
        });
        let world = core::Point::new(20.0, 40.0);
        let client = editor.canvas_to_client(world).expect("laid out editor");
        assert_eq!(client, core::Point::new(170.0, 120.0));
        assert_eq!(editor.client_to_canvas(client), Some(world));
    }

    #[gpui::test]
    fn per_anchor_presentation_is_typed_validated_and_clearable(cx: &mut gpui::TestAppContext) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let editor = cx.new(|_| NodeGraph::new(interactive_graph()));
        editor.update(cx, |editor, cx| {
            let presentation = AnchorPresentation {
                dot_shape: Some(style::DotShape::Diamond),
                dot_color: Some(style::Color::rgba(0x123456, 0.6)),
                multi: true,
            };
            assert!(editor.set_port_presentation("out".into(), presentation, cx));
            assert_eq!(editor.port_presentation(&"out".into()), presentation);
            assert!(!editor.set_port_presentation("missing".into(), presentation, cx));
            assert!(editor.set_port_presentation("out".into(), AnchorPresentation::default(), cx,));
            assert_eq!(
                editor.port_presentation(&"out".into()),
                AnchorPresentation::default()
            );
        });
    }

    #[gpui::test]
    fn occupied_input_reroute_detaches_immediately(cx: &mut gpui::TestAppContext) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let editor = cx.new(|_| NodeGraph::new(interactive_graph()));
        let removed = Rc::new(RefCell::new(Vec::new()));
        let observed = removed.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(
                &editor,
                move |_, event: &core::GraphEvent<String, String, String, Kind>, _| {
                    if let core::GraphEvent::ConnectionRemoved { id } = event {
                        observed.borrow_mut().push(id.clone());
                    }
                },
            )
        });
        editor.update(cx, |editor, cx| editor.start_draft(&"in".into(), cx));
        editor.update(cx, |editor, _| {
            assert!(editor.graph.connections.is_empty());
            assert_eq!(
                editor.draft.as_ref().map(|draft| draft.origin.as_str()),
                Some("out")
            );
        });
        assert_eq!(&*removed.borrow(), &[String::from("wire")]);
    }

    #[gpui::test]
    fn ctrl_left_pan_release_preserves_an_in_flight_draft(cx: &mut gpui::TestAppContext) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let editor = cx.new(|_| NodeGraph::new(interactive_graph()));
        editor.update(cx, |editor, cx| {
            editor.start_draft(&"out".into(), cx);
            editor.panning = Some(core::Point::new(10.0, 10.0));
            let theme = Arc::clone(cx.node_graph_theme());
            editor.finish_left_gesture(false, &theme, cx);
            assert!(editor.panning.is_none());
            assert_eq!(
                editor.draft.as_ref().map(|draft| draft.origin.as_str()),
                Some("out")
            );
        });
    }

    #[gpui::test]
    fn controlled_reroute_hides_the_detached_snapshot_edge(cx: &mut gpui::TestAppContext) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let mut graph = NodeGraph::new(interactive_graph());
        graph.config.mutation_mode = MutationMode::Controlled;
        let editor = cx.new(|_| graph);
        let requests = Rc::new(RefCell::new(Vec::new()));
        let observed = requests.clone();
        let _subscription = cx.update(|cx| {
            cx.subscribe(
                &editor,
                move |_, event: &core::GraphEvent<String, String, String, Kind>, _| {
                    if let core::GraphEvent::MutationRequested { mutations } = event {
                        observed.borrow_mut().push(mutations.clone());
                    }
                },
            )
        });
        editor.update(cx, |editor, cx| editor.start_draft(&"in".into(), cx));
        editor.update(cx, |editor, _| {
            assert!(editor.graph.connections.contains_key("wire"));
            assert!(editor.connection_is_detached(&"wire".into()));
            assert_eq!(
                editor.draft.as_ref().map(|draft| draft.origin.as_str()),
                Some("out")
            );
        });
        assert_eq!(
            &*requests.borrow(),
            &[vec![core::GraphMutation::RemoveConnection {
                id: String::from("wire")
            }]]
        );
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
        let theme = NodeGraphTheme::default();
        let completion = editor.finalize_node_drag(
            &NodeDrag {
                primary: String::from("a"),
                offsets: vec![(String::from("a"), core::Point::new(0.0, 0.0))],
                starts: vec![(String::from("a"), core::Point::new(0.0, 0.0))],
                moved: true,
                alter_groups: false,
            },
            &theme,
        );
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
            primary: String::from("a"),
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
    fn catalog_category_color_uses_metadata_then_style_fallback() {
        let mut item = NodeCatalogItem {
            id: "custom".into(),
            label: "Custom".into(),
            category: "Arbitrary".into(),
            category_color: Some(style::Color::rgba(0x123456, 0.4)),
            description: String::new(),
            keywords: Vec::new(),
            ports: Vec::<CatalogPort<Kind>>::new(),
        };
        let fallback = style::Color::rgba(0xabcdef, 0.7);
        assert_eq!(
            catalog_category_color(&item, fallback),
            item.category_color.unwrap()
        );
        item.category_color = None;
        assert_eq!(catalog_category_color(&item, fallback), fallback);
    }

    #[gpui::test]
    fn catalog_search_supports_caret_selection_and_resets_highlight(cx: &mut gpui::TestAppContext) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let editor = cx.new(|_| {
            let mut editor = NodeGraph::new(interactive_graph());
            editor.catalog_menu = Some(CatalogMenu {
                anchor_world: core::Point::default(),
                query: WorldTextInputState::new("abc", 1..2),
                selected: 4,
                connect_from: None,
            });
            editor
        });
        editor.update(cx, |editor, cx| {
            assert!(editor.edit_inline_text_key("z", Some("Z"), false, false, cx));
            let menu = editor.catalog_menu.as_ref().unwrap();
            assert_eq!(menu.query.text, "aZc");
            assert_eq!(menu.query.selection, 2..2);
            assert_eq!(menu.selected, 0);

            assert!(editor.edit_inline_text_key("left", None, false, false, cx));
            assert!(editor.edit_inline_text_key("backspace", None, false, false, cx));
            assert_eq!(editor.catalog_menu.as_ref().unwrap().query.text, "Zc");
        });
    }

    #[gpui::test]
    fn group_label_editor_commits_when_focus_moves_away(cx: &mut gpui::TestAppContext) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let editor = cx.new(|_| {
            let mut editor = NodeGraph::new(interactive_graph()).with_groups(vec![GraphGroup {
                id: "group".into(),
                label: Some("Old".into()),
                color: Some(style::Color::rgb(0)),
                error: false,
                nodes: [String::from("a")].into_iter().collect(),
            }]);
            editor.group_editor = Some(GroupEditor {
                id: "group".into(),
                query: WorldTextInputState::at_end("Renamed"),
            });
            editor
        });
        editor.update(cx, |editor, cx| editor.commit_group_editor(cx));
        editor.update(cx, |editor, _| {
            assert!(editor.group_editor.is_none());
            assert_eq!(editor.groups[0].label.as_deref(), Some("Renamed"));
        });
    }

    #[test]
    fn catalog_filters_search_and_draft_compatibility() {
        let mut editor = NodeGraph::new(interactive_graph()).with_catalog(vec![
            NodeCatalogItem {
                id: "sink".into(),
                label: "Number Sink".into(),
                category: "Output".into(),
                category_color: None,
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
                category_color: None,
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
            query: WorldTextInputState::at_end("final"),
            selected: 0,
            connect_from: Some("out".into()),
        });
        assert_eq!(editor.filtered_catalog_indices(), vec![0]);
        // Draft menus select the compatible pin even when the node only has one.
        assert_eq!(editor.filtered_catalog_entries(), vec![(0, Some(0))]);
        editor.catalog_menu.as_mut().unwrap().query = WorldTextInputState::at_end("source");
        assert!(editor.filtered_catalog_indices().is_empty());
    }

    #[test]
    fn draft_catalog_expands_every_compatible_port() {
        let mut editor = NodeGraph::new(interactive_graph()).with_catalog(vec![NodeCatalogItem {
            id: "multi".into(),
            label: "Multi Sink".into(),
            category: "Output".into(),
            category_color: None,
            description: String::new(),
            keywords: Vec::new(),
            ports: vec![
                CatalogPort {
                    id: "left".into(),
                    label: "Left".into(),
                    direction: PortDirection::Input,
                    kind: Kind,
                },
                CatalogPort {
                    id: "right".into(),
                    label: "Right".into(),
                    direction: PortDirection::Input,
                    kind: Kind,
                },
            ],
        }]);
        editor.catalog_menu = Some(CatalogMenu {
            anchor_world: core::Point::default(),
            query: WorldTextInputState::at_end(String::new()),
            selected: 0,
            connect_from: Some("out".into()),
        });
        assert_eq!(
            editor.filtered_catalog_entries(),
            vec![(0, Some(0)), (0, Some(1))]
        );
        let event = editor.take_catalog_creation_event(0, Some(1)).unwrap();
        assert!(editor.catalog_menu.is_none());
        match event {
            core::GraphEvent::CreateNode {
                item_id,
                connect_from,
                connect_to,
                connect_direction,
                ..
            } => {
                assert_eq!(item_id, "multi");
                assert_eq!(connect_from.as_deref(), Some("out"));
                assert_eq!(connect_to.as_deref(), Some("right"));
                assert_eq!(connect_direction, Some(PortDirection::Output));
            }
            _ => panic!("catalog choice emitted the wrong event"),
        }
    }

    #[test]
    fn catalog_creation_metadata_uses_the_draft_origin_direction() {
        let item = NodeCatalogItem {
            id: "source".into(),
            label: "Source".into(),
            category: "Input".into(),
            category_color: None,
            description: String::new(),
            keywords: Vec::new(),
            ports: vec![CatalogPort {
                id: "value".into(),
                label: "Value".into(),
                direction: PortDirection::Output,
                kind: Kind,
            }],
        };
        let mut editor = NodeGraph::new(interactive_graph()).with_catalog(vec![item]);
        editor.catalog_menu = Some(CatalogMenu {
            anchor_world: core::Point::default(),
            query: WorldTextInputState::at_end(String::new()),
            selected: 0,
            connect_from: Some("in".into()),
        });
        match editor.take_catalog_creation_event(0, Some(0)).unwrap() {
            core::GraphEvent::CreateNode {
                connect_from,
                connect_to,
                connect_direction,
                ..
            } => {
                assert_eq!(connect_from.as_deref(), Some("in"));
                assert_eq!(connect_to.as_deref(), Some("value"));
                assert_eq!(connect_direction, Some(PortDirection::Input));
            }
            _ => panic!("catalog choice emitted the wrong event"),
        }
    }

    #[test]
    fn draft_catalog_rejects_an_explicit_incompatible_pin() {
        let mut editor = NodeGraph::new(interactive_graph()).with_catalog(vec![NodeCatalogItem {
            id: "mixed".into(),
            label: "Mixed".into(),
            category: "Utility".into(),
            category_color: None,
            description: String::new(),
            keywords: Vec::new(),
            ports: vec![
                CatalogPort {
                    id: "input".into(),
                    label: "Input".into(),
                    direction: PortDirection::Input,
                    kind: Kind,
                },
                CatalogPort {
                    id: "output".into(),
                    label: "Output".into(),
                    direction: PortDirection::Output,
                    kind: Kind,
                },
            ],
        }]);
        editor.catalog_menu = Some(CatalogMenu {
            anchor_world: core::Point::default(),
            query: WorldTextInputState::at_end(String::new()),
            selected: 0,
            connect_from: Some("out".into()),
        });
        assert!(editor.take_catalog_creation_event(0, Some(1)).is_none());
        assert!(editor.catalog_menu.is_some());
    }

    #[test]
    fn selected_groups_are_deterministic_for_ungroup_transactions() {
        let mut editor = NodeGraph::new(interactive_graph()).with_groups(vec![GraphGroup {
            id: "group".into(),
            label: Some("Group".into()),
            color: Some(style::Color::rgb(0)),
            error: false,
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
            label: Some("Group".into()),
            color: Some(style::Color::rgb(0)),
            error: false,
            nodes: [String::from("b")].into_iter().collect(),
        }]);
        assert_eq!(
            editor.group_label_at(core::Point::new(100.0, -25.0), &NodeGraphTheme::default()),
            Some(String::from("group"))
        );
        assert_eq!(
            editor.group_label_at(core::Point::new(20.0, 20.0), &NodeGraphTheme::default()),
            None
        );
    }

    #[gpui::test]
    fn alt_drag_detaches_only_the_primary_node_immediately(cx: &mut gpui::TestAppContext) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let mut graph = interactive_graph();
        graph.selected_nodes = [String::from("a"), String::from("b")].into_iter().collect();
        let editor = cx.new(|_| NodeGraph::new(graph));
        editor.update(cx, |editor, cx| {
            editor.groups.push(GraphGroup {
                id: "group".into(),
                label: Some("Group".into()),
                color: None,
                error: false,
                nodes: [String::from("a"), String::from("b")].into_iter().collect(),
            });
            editor.begin_node_drag(
                &String::from("a"),
                core::Point::new(10.0, 10.0),
                false,
                true,
                cx,
            );
            assert_eq!(
                editor.groups[0].nodes,
                [String::from("b")].into_iter().collect()
            );
            let drag = editor.drag.as_ref().unwrap();
            assert_eq!(drag.primary, "a");
            assert_eq!(drag.offsets.len(), 2);
        });
    }

    #[test]
    fn alt_drag_membership_adds_inside_and_removes_outside() {
        let mut editor = NodeGraph::new(interactive_graph()).with_groups(vec![GraphGroup {
            id: "group".into(),
            label: Some("Group".into()),
            color: Some(style::Color::rgb(0)),
            error: false,
            nodes: [String::from("b")].into_iter().collect(),
        }]);
        editor.graph.nodes.get_mut("a").unwrap().position = core::Point::new(80.0, 0.0);
        let changes =
            editor.update_group_memberships(&[String::from("a")], &NodeGraphTheme::default());
        assert_eq!(changes.len(), 1);
        assert!(editor.groups[0].nodes.contains("a"));
        editor.graph.nodes.get_mut("a").unwrap().position = core::Point::new(0.0, 0.0);
        editor.update_group_memberships(&[String::from("a")], &NodeGraphTheme::default());
        assert!(!editor.groups[0].nodes.contains("a"));
    }

    #[test]
    fn alt_drag_can_remove_the_last_group_member() {
        let mut editor = NodeGraph::new(interactive_graph()).with_groups(vec![GraphGroup {
            id: "group".into(),
            label: Some("Group".into()),
            color: Some(style::Color::rgb(0)),
            error: false,
            nodes: [String::from("a")].into_iter().collect(),
        }]);
        let changes =
            editor.update_group_memberships(&[String::from("a")], &NodeGraphTheme::default());
        assert_eq!(changes, vec![(String::from("group"), Vec::new())]);
        assert!(editor.groups[0].nodes.is_empty());
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
        assert_eq!(
            routes["wire"].first().copied(),
            editor.resolved_port_position(&String::from("out"))
        );
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
        assert_eq!(
            route.first().copied(),
            editor.resolved_port_position(&String::from("out"))
        );
        assert_eq!(
            route.last().copied(),
            editor.resolved_port_position(&String::from("in"))
        );
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
                node_type: "blocker".into(),
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
        let longest = route
            .windows(2)
            .max_by(|left, right| {
                left[0]
                    .distance(left[1])
                    .total_cmp(&right[0].distance(right[1]))
            })
            .unwrap();
        let midpoint = core::Point::new(
            (longest[0].x + longest[1].x) * 0.5,
            (longest[0].y + longest[1].y) * 0.5,
        );
        assert_eq!(editor.connection_at(midpoint, 4.0), Some("wire".into()));
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
    fn positioned_overlay_projects_anchor_extent_but_applies_screen_gap_once() {
        let node = core::Point::new(330.0, 50.0);
        let offset = core::Point::new(167.0, 29.0);
        for zoom in [0.5, 1.0, 2.0] {
            let viewport = Viewport {
                pan: core::Point::new(13.0, -7.0),
                zoom,
            };
            let node_screen = viewport.world_to_screen(node);
            let anchor = core::Rect {
                origin: node_screen + overlay_screen_offset(offset, viewport),
                size: core::Size {
                    width: viewport.scale_length(25.0),
                    height: viewport.scale_length(25.0),
                },
            };
            let position = resolve_positioned_overlay(
                anchor,
                OverlayPlacement {
                    side: OverlaySide::Right,
                    align: OverlayAlign::Start,
                    anchor_size: core::Size {
                        width: 25.0,
                        height: 25.0,
                    },
                    gap: 8.0,
                    flip: false,
                    clamp_to_canvas: false,
                },
                core::Size {
                    width: 200.0,
                    height: 70.0,
                },
                core::Size {
                    width: 2_000.0,
                    height: 2_000.0,
                },
            );
            assert_eq!(position.x, 13.0 + (330.0 + 167.0 + 25.0) * zoom + 8.0);
            assert_eq!(position.y, -7.0 + (50.0 + 29.0) * zoom);
        }
    }

    #[gpui::test]
    fn overlay_escape_and_outside_dismiss_policies_are_independent(cx: &mut gpui::TestAppContext) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let editor = cx.new(|_| NodeGraph::new(interactive_graph()));
        let dismiss_count = Rc::new(Cell::new(0));
        let observed = dismiss_count.clone();
        editor.update(cx, |editor, cx| {
            editor.active_dismissible_overlays = [String::from("escape"), String::from("outside")]
                .into_iter()
                .collect();
            editor.active_escape_overlays.insert("escape".into());
            editor.active_outside_overlays.insert("outside".into());
            editor.active_overlay_dismiss_callbacks.insert(
                "escape".into(),
                Rc::new(move || observed.set(observed.get() + 1)),
            );
            editor.dismiss_escape_overlays(cx);
            assert!(editor.is_overlay_dismissed("escape"));
            assert!(!editor.is_overlay_dismissed("outside"));
            assert!(editor.active_outside_overlays.contains("outside"));
            assert_eq!(dismiss_count.get(), 1);

            editor.dismiss_outside_overlays(cx);
            assert!(editor.is_overlay_dismissed("outside"));
        });
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
            dismiss_on_outside_click: true,
            show_backdrop: true,
        };
        assert_eq!(
            resolve_overlay_position(
                core::Point::new(250.0, 280.0),
                50.0,
                core::Point::new(60.0, 0.0),
                &behavior,
                behavior.estimated_size,
                core::Size {
                    width: 300.0,
                    height: 300.0,
                },
            ),
            core::Point::new(140.0, 250.0)
        );
        assert_eq!(
            resolve_overlay_position(
                core::Point::new(250.0, 280.0),
                50.0,
                core::Point::new(60.0, 0.0),
                &behavior,
                core::Size {
                    width: 140.0,
                    height: 70.0,
                },
                core::Size {
                    width: 300.0,
                    height: 300.0,
                },
            ),
            core::Point::new(100.0, 230.0)
        );
    }

    #[test]
    fn positioned_overlays_support_every_side_alignment_and_primary_axis_flip() {
        let anchor = core::Rect {
            origin: core::Point::new(100.0, 100.0),
            size: core::Size {
                width: 20.0,
                height: 30.0,
            },
        };
        let panel = core::Size {
            width: 40.0,
            height: 50.0,
        };
        let canvas = core::Size {
            width: 300.0,
            height: 300.0,
        };
        let place = |side, align| OverlayPlacement {
            side,
            align,
            anchor_size: anchor.size,
            gap: 8.0,
            flip: false,
            clamp_to_canvas: false,
        };
        assert_eq!(
            resolve_positioned_overlay(
                anchor,
                place(OverlaySide::Right, OverlayAlign::Start),
                panel,
                canvas,
            ),
            core::Point::new(128.0, 100.0)
        );
        assert_eq!(
            resolve_positioned_overlay(
                anchor,
                place(OverlaySide::Left, OverlayAlign::Center),
                panel,
                canvas,
            ),
            core::Point::new(52.0, 90.0)
        );
        assert_eq!(
            resolve_positioned_overlay(
                anchor,
                place(OverlaySide::Top, OverlayAlign::End),
                panel,
                canvas,
            ),
            core::Point::new(80.0, 42.0)
        );
        assert_eq!(
            resolve_positioned_overlay(
                anchor,
                place(OverlaySide::Bottom, OverlayAlign::Center),
                panel,
                canvas,
            ),
            core::Point::new(90.0, 138.0)
        );
        let edge_anchor = core::Rect {
            origin: core::Point::new(270.0, 100.0),
            size: core::Size {
                width: 20.0,
                height: 20.0,
            },
        };
        assert_eq!(
            resolve_positioned_overlay(
                edge_anchor,
                OverlayPlacement {
                    side: OverlaySide::Right,
                    align: OverlayAlign::Start,
                    anchor_size: edge_anchor.size,
                    gap: 8.0,
                    flip: true,
                    clamp_to_canvas: true,
                },
                core::Size {
                    width: 60.0,
                    height: 40.0,
                },
                canvas,
            ),
            core::Point::new(202.0, 100.0)
        );
    }

    #[test]
    fn dynamic_port_removal_uses_transient_tombstones_and_keeps_graph_strict() {
        let mut editor = NodeGraph::new(interactive_graph());
        editor.refresh_default_render_geometry(&NodeGraphTheme::default());
        editor.refresh_default_render_geometry(&NodeGraphTheme::default());
        let removed = editor.remove_port_to_tombstones(&"out".into()).unwrap();
        assert_eq!(removed, vec![String::from("wire")]);
        assert!(!editor.graph.ports.contains_key("out"));
        assert!(editor.graph.connections.is_empty());
        editor.graph.validate().unwrap();
        assert_eq!(editor.dangling_connections.len(), 1);
        assert_eq!(editor.dangling_connections[0].missing_port, "out");
        assert_eq!(
            editor.dangling_connections[0].source_position,
            core::Point::new(36.0, 43.4)
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
    fn controlled_tombstones_survive_host_reconciliation_until_restored() {
        let mut editor = NodeGraph::new(interactive_graph());
        editor.capture_tombstones_for_port(&"out".into());
        assert_eq!(editor.dangling_connections.len(), 1);
        assert!(editor.graph.connections.contains_key("wire"));

        editor.graph.connections.remove("wire");
        editor.graph.ports.remove("out");
        editor.prune_resolved_tombstones();
        assert_eq!(editor.dangling_connections.len(), 1);

        editor.graph.ports.insert(
            "out".into(),
            Port {
                id: "out".into(),
                node: "a".into(),
                label: "out".into(),
                direction: PortDirection::Output,
                kind: Kind,
                position: core::Point::new(50.0, 25.0),
            },
        );
        assert_eq!(
            editor.restorable_tombstone_connections(&"out".into()),
            vec![Connection {
                id: "wire".into(),
                source: "out".into(),
                target: "in".into(),
            }]
        );
        editor.graph.connections.insert(
            "wire".into(),
            Connection {
                id: "wire".into(),
                source: "out".into(),
                target: "in".into(),
            },
        );
        editor.prune_resolved_tombstones();
        assert!(editor.dangling_connections.is_empty());
    }

    #[test]
    fn custom_body_measurement_is_authoritative_transient_geometry() {
        let mut editor = NodeGraph::new(interactive_graph()).with_node_body_renderer(
            |_: NodeBodyContext<Kind, String, String, String>,
             _: &mut gpui::Window,
             _: &mut gpui::App| NodeBody::new(div()),
        );
        editor.render_geometry.node_sizes.insert(
            "a".into(),
            core::Size {
                width: 240.0,
                height: 180.0,
            },
        );
        assert_eq!(
            editor.resolved_node_size(&String::from("a")),
            Some(core::Size {
                width: 240.0,
                height: 180.0,
            })
        );
        assert_eq!(editor.graph.nodes["a"].size.width, 50.0);
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
        editor.refresh_default_render_geometry(&NodeGraphTheme::default());
        assert_eq!(
            editor.resolved_node_size(&"a".into()),
            Some(core::Size {
                width: 50.0,
                height: 59.4,
            })
        );
        assert_eq!(editor.render_bounds().unwrap().size.height, 59.4);
    }

    #[test]
    fn default_nodes_resolve_style_driven_shell_and_port_geometry() {
        let mut editor = NodeGraph::new(interactive_graph());
        editor.refresh_default_render_geometry(&NodeGraphTheme::default());
        let default_theme = NodeGraphTheme::default();
        editor.refresh_default_render_geometry(&default_theme);
        assert_eq!(
            editor.resolved_port_position(&String::from("out")),
            Some(core::Point::new(36.0, 43.4))
        );
        assert_eq!(
            editor.resolved_node_size(&String::from("a")),
            Some(core::Size {
                width: 50.0,
                height: 59.4,
            })
        );

        let mut theme = NodeGraphTheme::default();
        theme.anchor.dot_inset = 5.0;
        theme.anchor.row_height = 30.0;
        theme.node.ports_padding_y = 10.0;
        editor.refresh_default_render_geometry(&theme);
        assert_eq!(
            editor.resolved_port_position(&String::from("out")),
            Some(core::Point::new(45.0, 52.4))
        );
        assert_eq!(
            editor.resolved_node_size(&String::from("a")),
            Some(core::Size {
                width: 50.0,
                height: 77.4,
            })
        );

        editor.auto_width_nodes.insert(String::from("a"));
        editor.refresh_default_render_geometry(&theme);
        assert_eq!(
            editor.resolved_node_size(&String::from("a")),
            Some(core::Size {
                width: 160.0,
                height: 77.4,
            })
        );
        assert_eq!(
            editor.resolved_port_position(&String::from("out")),
            Some(core::Point::new(155.0, 52.4))
        );
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
        editor.refresh_default_render_geometry(&NodeGraphTheme::default());
        editor.refresh_default_render_geometry(&NodeGraphTheme::default());
        assert_eq!(
            editor.connection_at(core::Point::new(75.0, 43.4), 3.0),
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

    #[gpui::test]
    fn shift_marquee_replaces_nodes_after_preserving_the_blank_click(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(|app| set_node_graph_theme(app, NodeGraphTheme::default()));
        let mut graph = interactive_graph();
        graph.selected_nodes.insert("a".into());
        let editor = cx.new(|_| NodeGraph::new(graph));
        editor.update(cx, |editor, cx| {
            let theme = Arc::clone(cx.node_graph_theme());
            editor.begin_canvas_selection(core::Point::new(300.0, 300.0), true, &theme, cx);
            assert!(editor.graph.selected_nodes.contains("a"));
            editor.handle_pointer_move(core::Point::new(90.0, -10.0), &theme, cx);
            assert_eq!(
                editor.graph.selected_nodes,
                [String::from("b")].into_iter().collect()
            );
        });
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
    fn node_cursor_style_tracks_idle_drag_and_global_resize_state() {
        let node = style::NodeStyle {
            cursor: style::Cursor::Crosshair,
            cursor_dragging: style::Cursor::Grabbing,
            cursor_resize: style::Cursor::EwResize,
            ..Default::default()
        };
        assert_eq!(
            resolved_node_cursor(&node, false, false),
            style::Cursor::Crosshair
        );
        assert_eq!(
            resolved_node_cursor(&node, true, false),
            style::Cursor::Grabbing
        );
        // Matches Leptos: a resize can outrun its narrow handle, so it claims every shell.
        assert_eq!(
            resolved_node_cursor(&node, false, true),
            style::Cursor::EwResize
        );
        assert_eq!(
            resolved_node_cursor(&node, true, true),
            style::Cursor::EwResize
        );
    }

    #[test]
    fn port_paint_consumes_state_colors_glow_and_incompatible_opacity() {
        let anchor = style::AnchorStyle {
            dot_color: style::Color::rgb(0x010203),
            dot_connected_color: style::Color::rgb(0x111213),
            dot_compatible_color: style::Color::rgb(0x212223),
            label_color: style::Color::rgb(0x313233),
            label_compatible_color: style::Color::rgb(0x414243),
            incompatible_opacity: 0.17,
            ..Default::default()
        };

        let idle = resolved_port_paint(&anchor, false, false, false, false, false);
        assert_eq!(idle.stroke, anchor.dot_color);
        assert_eq!(idle.fill, style::Color::TRANSPARENT);
        assert_eq!(idle.label, anchor.label_color);
        assert_eq!(idle.opacity, 1.0);
        assert!(!idle.glow);

        let connected = resolved_port_paint(&anchor, true, false, false, false, false);
        assert_eq!(connected.stroke, anchor.dot_connected_color);
        assert_eq!(connected.fill, anchor.dot_connected_color);

        let compatible = resolved_port_paint(&anchor, false, false, false, true, true);
        assert_eq!(compatible.stroke, anchor.dot_compatible_color);
        assert_eq!(compatible.fill, anchor.dot_compatible_color);
        assert_eq!(compatible.label, anchor.label_compatible_color);
        assert_eq!(compatible.opacity, 1.0);
        assert!(compatible.glow);

        let incompatible = resolved_port_paint(&anchor, true, false, false, false, true);
        assert_eq!(incompatible.opacity, anchor.incompatible_opacity);
    }

    #[test]
    fn every_non_circle_dot_shape_has_stable_authored_geometry() {
        use style::DotShape;
        let center = core::Point::new(10.0, 20.0);
        let diamond = dot_shape_points(center, 4.0, DotShape::Diamond);
        let square = dot_shape_points(center, 4.0, DotShape::Square);
        let triangle = dot_shape_points(center, 4.0, DotShape::Triangle);
        let hexagon = dot_shape_points(center, 4.0, DotShape::Hexagon);
        let star = dot_shape_points(center, 4.0, DotShape::Star);

        assert_eq!(
            (
                diamond.len(),
                square.len(),
                triangle.len(),
                hexagon.len(),
                star.len()
            ),
            (4, 4, 3, 6, 8)
        );
        assert_eq!(diamond[0], core::Point::new(10.0, 16.0));
        assert_eq!(square[0], core::Point::new(6.0, 16.0));
        assert_eq!(triangle[1], core::Point::new(14.0, 20.0));
        assert_eq!(star[0], core::Point::new(10.0, 16.0));
        assert_eq!(star[2], core::Point::new(14.0, 20.0));
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
    #[test]
    fn runtime_border_resolution_honors_none_and_preserves_visible_widths() {
        let color = style::Color::rgb(0x123456);
        assert_eq!(visible_border_width(style::Border::none()), 0.0);
        assert_eq!(
            visible_border_width(style::Border {
                width: 3.0,
                style: style::LineStyle::None,
                color,
            }),
            0.0
        );
        assert_eq!(visible_border_width(style::Border::solid(2.0, color)), 2.0);
        assert_eq!(
            visible_border_width(style::Border {
                width: 4.0,
                style: style::LineStyle::Dashed,
                color,
            }),
            4.0
        );
    }

    #[test]
    fn group_runtime_resolves_default_color_and_leptos_label_mix() {
        let default = style::Color::rgba(0x8b5cf6, 0.6);
        assert_eq!(resolved_group_color(None, default), default);
        assert_eq!(
            resolved_group_color(Some(style::Color::rgb(0x010203)), default),
            style::Color::rgb(0x010203)
        );
        assert_eq!(
            mix_color(
                style::Color::rgb(0x000000),
                style::Color::rgb(0xffffff),
                0.7
            ),
            style::Color::rgb(0x4d4d4d)
        );
    }
}
