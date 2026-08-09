use serde::{Deserialize, Serialize};

pub mod subway;
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
};

pub trait NodeId: Clone + Eq + Hash + Debug + 'static {}
pub trait PortId: Clone + Eq + Hash + Debug + 'static {}
pub trait ConnectionId: Clone + Eq + Hash + Debug + 'static {}
/// A serializable ID wrapper for applications that want a distinct ID newtype.
///
/// Implementations are provided for every ID role; applications that require
/// compile-time separation should use distinct inner newtypes (for example
/// `Id<MyNodeId>` and `Id<MyPortId>`).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Id<T>(pub T);
impl<T: Clone + Eq + Hash + Debug + 'static> NodeId for Id<T> {}
impl<T: Clone + Eq + Hash + Debug + 'static> PortId for Id<T> {}
impl<T: Clone + Eq + Hash + Debug + 'static> ConnectionId for Id<T> {}
macro_rules! impl_id_roles { ($($ty:ty),* $(,)?) => {$(
    impl NodeId for $ty {}
    impl PortId for $ty {}
    impl ConnectionId for $ty {}
)*}; }
impl_id_roles!(String, u8, u16, u32, u64, usize, i8, i16, i32, i64, isize);

pub trait PortType: Clone + PartialEq + Debug + 'static {
    fn compatible(source: &Self, target: &Self) -> bool;
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}
impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
    pub fn distance(self, o: Self) -> f32 {
        ((self.x - o.x).powi(2) + (self.y - o.y).powi(2)).sqrt()
    }
}
impl std::ops::Add for Point {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y)
    }
}
impl std::ops::Sub for Point {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y)
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}
impl Rect {
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.origin.x
            && p.y >= self.origin.y
            && p.x <= self.origin.x + self.size.width
            && p.y <= self.origin.y + self.size.height
    }
    pub fn intersects(&self, o: &Self) -> bool {
        self.origin.x < o.origin.x + o.size.width
            && self.origin.x + self.size.width > o.origin.x
            && self.origin.y < o.origin.y + o.size.height
            && self.origin.y + self.size.height > o.origin.y
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PortDirection {
    Input,
    Output,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    pub pan: Point,
    pub zoom: f32,
}
impl Default for Viewport {
    fn default() -> Self {
        Self {
            pan: Point::default(),
            zoom: 1.,
        }
    }
}
impl Viewport {
    fn finite_f32(value: f64) -> f32 {
        value.clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
    }
    fn finite_coordinate(value: f32) -> f32 {
        if value.is_finite() { value } else { 0.0 }
    }
    pub fn world_to_screen(self, p: Point) -> Point {
        let viewport = self.sanitized();
        let x = Self::finite_coordinate(p.x) as f64 * viewport.zoom as f64 + viewport.pan.x as f64;
        let y = Self::finite_coordinate(p.y) as f64 * viewport.zoom as f64 + viewport.pan.y as f64;
        Point::new(Self::finite_f32(x), Self::finite_f32(y))
    }
    pub fn screen_to_world(self, p: Point) -> Point {
        let viewport = self.sanitized();
        let x =
            (Self::finite_coordinate(p.x) as f64 - viewport.pan.x as f64) / viewport.zoom as f64;
        let y =
            (Self::finite_coordinate(p.y) as f64 - viewport.pan.y as f64) / viewport.zoom as f64;
        Point::new(Self::finite_f32(x), Self::finite_f32(y))
    }
    /// Scale a non-negative world-space length into a finite render-space length.
    pub fn scale_length(self, value: f32) -> f32 {
        if !value.is_finite() || value <= 0.0 {
            return 0.0;
        }
        Self::finite_f32(value as f64 * self.sanitized().zoom as f64)
    }
    /// Pan by the screen-space displacement from `previous` to `current` using
    /// wide intermediates so finite cursor coordinates cannot overflow the viewport.
    pub fn pan_between(&mut self, previous: Point, current: Point) -> bool {
        if !previous.x.is_finite()
            || !previous.y.is_finite()
            || !current.x.is_finite()
            || !current.y.is_finite()
        {
            return false;
        }
        let viewport = self.sanitized();
        let x = viewport.pan.x as f64 + current.x as f64 - previous.x as f64;
        let y = viewport.pan.y as f64 + current.y as f64 - previous.y as f64;
        self.zoom = viewport.zoom;
        self.pan = Point::new(Self::finite_f32(x), Self::finite_f32(y));
        true
    }
    /// Zoom while preserving the world point beneath `screen`.
    /// Invalid/non-positive factors and bounds are ignored rather than allowing
    /// NaN, infinity, or a zero zoom into the viewport.
    pub fn zoom_at(&mut self, screen: Point, factor: f32, min: f32, max: f32) {
        if !factor.is_finite()
            || factor <= 0.0
            || !min.is_finite()
            || !max.is_finite()
            || min <= 0.0
            || min > max
            || !screen.x.is_finite()
            || !screen.y.is_finite()
        {
            return;
        }
        let current = self.sanitized();
        let old_zoom = current.zoom as f64;
        let new_zoom = (old_zoom * factor as f64).clamp(min as f64, max as f64);
        let world_x = (screen.x as f64 - current.pan.x as f64) / old_zoom;
        let world_y = (screen.y as f64 - current.pan.y as f64) / old_zoom;
        let pan_x = screen.x as f64 - world_x * new_zoom;
        let pan_y = screen.y as f64 - world_y * new_zoom;

        // Calculate in f64 and saturate only at the public f32 boundary. This
        // keeps every field finite even when otherwise-valid f32 operands would
        // overflow an intermediate multiplication or subtraction.
        self.zoom = new_zoom as f32;
        self.pan = Point::new(Self::finite_f32(pan_x), Self::finite_f32(pan_y));
    }
    pub fn is_valid(self) -> bool {
        self.zoom.is_finite() && self.zoom > 0.0 && self.pan.x.is_finite() && self.pan.y.is_finite()
    }
    /// Return a render-safe viewport while retaining public DTO-compatible fields.
    pub fn sanitized(self) -> Self {
        Self {
            pan: if self.pan.x.is_finite() && self.pan.y.is_finite() {
                self.pan
            } else {
                Point::default()
            },
            zoom: if self.zoom.is_finite() && self.zoom > 0.0 {
                self.zoom
            } else {
                1.0
            },
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node<N> {
    pub id: N,
    pub title: String,
    pub position: Point,
    pub size: Size,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
/// A port whose `position` is in world coordinates. Graph mutation APIs keep it
/// translated with its owning node.
pub struct Port<N, P, T> {
    pub id: P,
    pub node: N,
    pub label: String,
    pub direction: PortDirection,
    pub kind: T,
    pub position: Point,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Connection<P, C> {
    pub id: C,
    pub source: P,
    pub target: P,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum GraphEvent<N: Eq + Hash, P, C: Eq + Hash> {
    NodesMoved {
        nodes: Vec<(N, Point)>,
    },
    NodeResized {
        id: N,
        size: Size,
    },
    ConnectionRequested {
        source: P,
        target: P,
    },
    ConnectionRemoved {
        id: C,
    },
    SelectionChanged {
        nodes: HashSet<N>,
        connections: HashSet<C>,
    },
    NodesDeleted {
        ids: Vec<N>,
    },
    NodesCopied {
        ids: Vec<N>,
    },
    NodesPasted {
        offset: Point,
    },
    Undo,
    Redo,
    GroupCreated {
        node_ids: Vec<N>,
    },
    GroupMembershipChanged {
        group_id: String,
        node_ids: Vec<N>,
    },
    GroupLabelChanged {
        group_id: String,
        label: String,
    },
    CreateNode {
        item_id: String,
        position: Point,
        connect_from: Option<P>,
        connect_to: Option<String>,
        connect_direction: Option<PortDirection>,
    },
    ViewportChanged {
        viewport: Viewport,
    },
    GraphReconciled,
}
/// Persisted, framework-free graph domain data. Selection and viewport are UI
/// session state and intentionally do not appear in this snapshot.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GraphSnapshot<N: Eq + Hash, P: Eq + Hash, C: Eq + Hash, T> {
    pub nodes: HashMap<N, Node<N>>,
    pub ports: HashMap<P, Port<N, P, T>>,
    pub connections: HashMap<C, Connection<P, C>>,
}
impl<N: Eq + Hash, P: Eq + Hash, C: Eq + Hash, T> Default for GraphSnapshot<N, P, C, T> {
    fn default() -> Self {
        Self {
            nodes: HashMap::new(),
            ports: HashMap::new(),
            connections: HashMap::new(),
        }
    }
}

/// Transient editor state. It is deliberately excluded from saved graphs.
#[derive(Clone, Debug, PartialEq)]
pub struct GraphUiState<N: Eq + Hash, C: Eq + Hash> {
    pub selected_nodes: HashSet<N>,
    pub selected_connections: HashSet<C>,
    pub viewport: Viewport,
}
impl<N: Eq + Hash, C: Eq + Hash> Default for GraphUiState<N, C> {
    fn default() -> Self {
        Self {
            selected_nodes: HashSet::new(),
            selected_connections: HashSet::new(),
            viewport: Viewport::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphState<N: Eq + Hash, P: Eq + Hash, C: Eq + Hash, T> {
    pub nodes: HashMap<N, Node<N>>,
    pub ports: HashMap<P, Port<N, P, T>>,
    pub connections: HashMap<C, Connection<P, C>>,
    #[serde(skip, default)]
    pub selected_nodes: HashSet<N>,
    #[serde(skip, default)]
    pub selected_connections: HashSet<C>,
    #[serde(skip, default)]
    pub viewport: Viewport,
}
impl<N: Eq + Hash, P: Eq + Hash, C: Eq + Hash, T> Default for GraphState<N, P, C, T> {
    fn default() -> Self {
        Self {
            nodes: HashMap::new(),
            ports: HashMap::new(),
            connections: HashMap::new(),
            selected_nodes: HashSet::new(),
            selected_connections: HashSet::new(),
            viewport: Viewport::default(),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphValidationError {
    pub problems: Vec<String>,
}
impl std::fmt::Display for GraphValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "invalid graph: {}", self.problems.join("; "))
    }
}
impl std::error::Error for GraphValidationError {}

impl<
    N: Clone + Eq + Hash + Debug,
    P: Clone + Eq + Hash + Debug,
    C: Clone + Eq + Hash + Debug,
    T: PortType,
> GraphState<N, P, C, T>
{
    /// Copy only persisted domain data, excluding viewport and selection.
    pub fn snapshot(&self) -> GraphSnapshot<N, P, C, T>
    where
        T: Clone,
    {
        GraphSnapshot {
            nodes: self.nodes.clone(),
            ports: self.ports.clone(),
            connections: self.connections.clone(),
        }
    }
    pub fn ui_state(&self) -> GraphUiState<N, C> {
        GraphUiState {
            selected_nodes: self.selected_nodes.clone(),
            selected_connections: self.selected_connections.clone(),
            viewport: self.viewport,
        }
    }
    pub fn from_snapshot(
        mut snapshot: GraphSnapshot<N, P, C, T>,
    ) -> Result<Self, GraphValidationError> {
        Self::canonicalize_snapshot(&mut snapshot);
        let graph = Self {
            nodes: snapshot.nodes,
            ports: snapshot.ports,
            connections: snapshot.connections,
            ..Default::default()
        };
        graph.validate()?;
        Ok(graph)
    }
    /// Replace domain data while retaining valid transient state. Embedded IDs
    /// are canonicalized from their map keys before validation.
    pub fn reconcile(
        &mut self,
        mut snapshot: GraphSnapshot<N, P, C, T>,
    ) -> Result<Vec<GraphEvent<N, P, C>>, GraphValidationError> {
        Self::canonicalize_snapshot(&mut snapshot);
        let candidate = Self {
            nodes: snapshot.nodes,
            ports: snapshot.ports,
            connections: snapshot.connections,
            ..Default::default()
        };
        candidate.validate()?;
        let old_selection = (
            self.selected_nodes.clone(),
            self.selected_connections.clone(),
        );
        self.nodes = candidate.nodes;
        self.ports = candidate.ports;
        self.connections = candidate.connections;
        self.selected_nodes.retain(|id| self.nodes.contains_key(id));
        self.selected_connections
            .retain(|id| self.connections.contains_key(id));
        let mut events = vec![GraphEvent::GraphReconciled];
        if old_selection
            != (
                self.selected_nodes.clone(),
                self.selected_connections.clone(),
            )
        {
            events.push(GraphEvent::SelectionChanged {
                nodes: self.selected_nodes.clone(),
                connections: self.selected_connections.clone(),
            });
        }
        Ok(events)
    }
    pub fn canonicalize_ids(&mut self) {
        for (id, node) in &mut self.nodes {
            node.id = id.clone();
        }
        for (id, port) in &mut self.ports {
            port.id = id.clone();
        }
        for (id, connection) in &mut self.connections {
            connection.id = id.clone();
        }
    }
    fn canonicalize_snapshot(snapshot: &mut GraphSnapshot<N, P, C, T>) {
        for (id, node) in &mut snapshot.nodes {
            node.id = id.clone();
        }
        for (id, port) in &mut snapshot.ports {
            port.id = id.clone();
        }
        for (id, connection) in &mut snapshot.connections {
            connection.id = id.clone();
        }
    }
    pub fn validate(&self) -> Result<(), GraphValidationError> {
        let mut problems = Vec::new();
        for (key, node) in &self.nodes {
            if key != &node.id {
                problems.push(format!(
                    "node key {:?} does not match embedded id {:?}",
                    key, node.id
                ));
            }
            if !node.position.x.is_finite()
                || !node.position.y.is_finite()
                || !node.size.width.is_finite()
                || !node.size.height.is_finite()
                || node.size.width < 0.0
                || node.size.height < 0.0
            {
                problems.push(format!("node {:?} has invalid geometry", key));
            }
        }
        for (key, port) in &self.ports {
            if key != &port.id {
                problems.push(format!(
                    "port key {:?} does not match embedded id {:?}",
                    key, port.id
                ));
            }
            if !self.nodes.contains_key(&port.node) {
                problems.push(format!(
                    "port {:?} references missing node {:?}",
                    key, port.node
                ));
            }
            if !port.position.x.is_finite() || !port.position.y.is_finite() {
                problems.push(format!("port {:?} has invalid position", key));
            }
        }
        for (key, connection) in &self.connections {
            if key != &connection.id {
                problems.push(format!(
                    "connection key {:?} does not match embedded id {:?}",
                    key, connection.id
                ));
            }
            if !self.ports.contains_key(&connection.source) {
                problems.push(format!(
                    "connection {:?} has missing source {:?}",
                    key, connection.source
                ));
            }
            if !self.ports.contains_key(&connection.target) {
                problems.push(format!(
                    "connection {:?} has missing target {:?}",
                    key, connection.target
                ));
            }
            if self.ports.contains_key(&connection.source)
                && self.ports.contains_key(&connection.target)
                && !self.compatible_target(&connection.source, &connection.target)
            {
                problems.push(format!(
                    "connection {:?} is not output-to-input compatible",
                    key
                ));
            }
        }
        if !self.viewport.is_valid() {
            problems.push("viewport is invalid".into());
        }
        if problems.is_empty() {
            Ok(())
        } else {
            Err(GraphValidationError { problems })
        }
    }
    /// Move nodes and every world-space port they own by the same per-node delta.
    /// The update is atomic: a missing node, non-finite coordinate, or translated
    /// port outside the finite `f32` range rejects the
    /// entire gesture without changing any node or port.
    pub fn move_nodes(&mut self, updates: &[(N, Point)]) -> Option<GraphEvent<N, P, C>> {
        if updates.is_empty() {
            return None;
        }

        // Preserve caller order while making duplicate IDs last-write-wins.
        let mut canonical: Vec<(N, Point)> = Vec::with_capacity(updates.len());
        for (id, position) in updates {
            if !position.x.is_finite() || !position.y.is_finite() {
                return None;
            }
            if let Some((_, existing)) = canonical.iter_mut().find(|(current, _)| current == id) {
                *existing = *position;
            } else {
                canonical.push((id.clone(), *position));
            }
        }

        let desired: HashMap<N, Point> = canonical.iter().cloned().collect();
        let mut deltas = HashMap::with_capacity(desired.len());
        for (id, position) in &canonical {
            let old = self.nodes.get(id)?.position;
            if !old.x.is_finite() || !old.y.is_finite() {
                return None;
            }
            deltas.insert(
                id.clone(),
                (
                    position.x as f64 - old.x as f64,
                    position.y as f64 - old.y as f64,
                ),
            );
        }

        let translated: Option<Vec<(P, Point)>> = self
            .ports
            .iter()
            .filter_map(|(port_id, port)| {
                let &(dx, dy) = deltas.get(&port.node)?;
                let x = port.position.x as f64 + dx;
                let y = port.position.y as f64 + dy;
                Some(
                    (port.position.x.is_finite()
                        && port.position.y.is_finite()
                        && x.abs() <= f32::MAX as f64
                        && y.abs() <= f32::MAX as f64)
                        .then(|| (port_id.clone(), Point::new(x as f32, y as f32))),
                )
            })
            .collect();
        let translated = translated?;

        for (id, position) in &canonical {
            self.nodes
                .get_mut(id)
                .expect("node was present during movement validation")
                .position = *position;
        }
        for (port_id, port_position) in translated {
            self.ports
                .get_mut(&port_id)
                .expect("port was present during movement validation")
                .position = port_position;
        }
        Some(GraphEvent::NodesMoved { nodes: canonical })
    }

    /// Move one node atomically. This is the single-node convenience wrapper for
    /// [`Self::move_nodes`].
    pub fn move_node(&mut self, id: &N, position: Point) -> Option<GraphEvent<N, P, C>> {
        self.move_nodes(&[(id.clone(), position)])
    }

    /// Delete nodes and reconcile all dependent ports, connections and selection.
    pub fn remove_nodes(&mut self, ids: &[N]) -> Vec<GraphEvent<N, P, C>> {
        let old_selection = (
            self.selected_nodes.clone(),
            self.selected_connections.clone(),
        );
        let removed: HashSet<N> = ids
            .iter()
            .filter(|id| self.nodes.remove(*id).is_some())
            .cloned()
            .collect();
        let removed_ports: HashSet<P> = self
            .ports
            .iter()
            .filter(|(_, p)| removed.contains(&p.node))
            .map(|(id, _)| id.clone())
            .collect();
        self.ports.retain(|id, _| !removed_ports.contains(id));
        let removed_connections: Vec<C> = self
            .connections
            .iter()
            .filter(|(_, c)| removed_ports.contains(&c.source) || removed_ports.contains(&c.target))
            .map(|(id, _)| id.clone())
            .collect();
        for id in &removed_connections {
            self.connections.remove(id);
        }
        self.selected_nodes.retain(|id| !removed.contains(id));
        self.selected_connections
            .retain(|id| self.connections.contains_key(id));
        let mut events: Vec<_> = removed_connections
            .into_iter()
            .map(|id| GraphEvent::ConnectionRemoved { id })
            .collect();
        if !removed.is_empty() {
            events.push(GraphEvent::NodesDeleted {
                ids: removed.into_iter().collect(),
            });
        }
        if old_selection
            != (
                self.selected_nodes.clone(),
                self.selected_connections.clone(),
            )
        {
            events.push(GraphEvent::SelectionChanged {
                nodes: self.selected_nodes.clone(),
                connections: self.selected_connections.clone(),
            });
        }
        events
    }

    pub fn compatible_target(&self, source: &P, target: &P) -> bool {
        let Some(a) = self.ports.get(source) else {
            return false;
        };
        let Some(b) = self.ports.get(target) else {
            return false;
        };
        a.node != b.node
            && a.direction == PortDirection::Output
            && b.direction == PortDirection::Input
            && T::compatible(&a.kind, &b.kind)
    }
    pub fn nodes_in_rect(&self, r: Rect) -> HashSet<N> {
        self.nodes
            .values()
            .filter(|n| {
                r.intersects(&Rect {
                    origin: n.position,
                    size: n.size,
                })
            })
            .map(|n| n.id.clone())
            .collect()
    }
    pub fn bounds(&self) -> Option<Rect> {
        let mut it = self.nodes.values();
        let first = it.next()?;
        let (mut x, mut y, mut r, mut b) = (
            first.position.x,
            first.position.y,
            first.position.x + first.size.width,
            first.position.y + first.size.height,
        );
        for n in it {
            x = x.min(n.position.x);
            y = y.min(n.position.y);
            r = r.max(n.position.x + n.size.width);
            b = b.max(n.position.y + n.size.height)
        }
        Some(Rect {
            origin: Point::new(x, y),
            size: Size {
                width: r - x,
                height: b - y,
            },
        })
    }
}
pub fn orthogonal_route(a: Point, b: Point) -> Vec<Point> {
    let mid = (a.x + b.x) / 2.;
    vec![a, Point::new(mid, a.y), Point::new(mid, b.y), b]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Clone, Debug, PartialEq)]
    struct K;
    impl PortType for K {
        fn compatible(a: &Self, b: &Self) -> bool {
            a == b
        }
    }
    #[test]
    fn zoom_keeps_cursor_world_point() {
        let mut v = Viewport::default();
        let p = Point::new(40., 20.);
        let before = v.screen_to_world(p);
        v.zoom_at(p, 2., 0.1, 5.);
        assert_eq!(before, v.screen_to_world(p));
    }
    #[test]
    fn orthogonal_has_right_angles() {
        let r = orthogonal_route(Point::new(0., 0.), Point::new(10., 10.));
        assert_eq!(r[1], Point::new(5., 0.));
        assert_eq!(r[2], Point::new(5., 10.));
    }
    #[test]
    fn rect_selection() {
        let mut g: GraphState<String, String, String, K> = Default::default();
        g.nodes.insert(
            "a".into(),
            Node {
                id: "a".into(),
                title: "A".into(),
                position: Point::new(2., 2.),
                size: Size {
                    width: 10.,
                    height: 10.,
                },
            },
        );
        assert!(
            g.nodes_in_rect(Rect {
                origin: Point::default(),
                size: Size {
                    width: 5.,
                    height: 5.
                }
            })
            .contains("a")
        );
    }
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    enum Flow {
        Any,
        Number,
    }
    impl PortType for Flow {
        fn compatible(source: &Self, target: &Self) -> bool {
            matches!(
                (source, target),
                (Flow::Number, Flow::Any) | (Flow::Number, Flow::Number)
            )
        }
    }
    fn connected_graph() -> GraphState<String, String, String, Flow> {
        let mut g = GraphState::default();
        for (id, x) in [("a", 0.), ("b", 100.)] {
            g.nodes.insert(
                id.into(),
                Node {
                    id: id.into(),
                    title: id.into(),
                    position: Point::new(x, 0.),
                    size: Size {
                        width: 50.,
                        height: 50.,
                    },
                },
            );
        }
        g.ports.insert(
            "out".into(),
            Port {
                id: "out".into(),
                node: "a".into(),
                label: "out".into(),
                direction: PortDirection::Output,
                kind: Flow::Number,
                position: Point::new(50., 25.),
            },
        );
        g.ports.insert(
            "in".into(),
            Port {
                id: "in".into(),
                node: "b".into(),
                label: "in".into(),
                direction: PortDirection::Input,
                kind: Flow::Any,
                position: Point::new(100., 25.),
            },
        );
        g.connections.insert(
            "wire".into(),
            Connection {
                id: "wire".into(),
                source: "out".into(),
                target: "in".into(),
            },
        );
        g
    }
    #[test]
    fn compatibility_is_strictly_directional_and_asymmetric() {
        let g = connected_graph();
        assert!(g.compatible_target(&"out".into(), &"in".into()));
        assert!(!g.compatible_target(&"in".into(), &"out".into()));
        assert!(!Flow::compatible(&Flow::Any, &Flow::Number));
    }
    #[test]
    fn moving_node_translates_only_its_ports() {
        let mut g = connected_graph();
        g.move_node(&"a".into(), Point::new(10., 20.)).unwrap();
        assert_eq!(g.ports["out"].position, Point::new(60., 45.));
        assert_eq!(g.ports["in"].position, Point::new(100., 25.));
    }
    #[test]
    fn snapshot_fixture_excludes_transient_state_and_round_trips() {
        let mut g = connected_graph();
        g.selected_nodes.insert("a".into());
        g.viewport.pan = Point::new(9., 8.);
        let value = serde_json::to_value(g.snapshot()).unwrap();
        assert_eq!(
            value.as_object().unwrap().keys().collect::<HashSet<_>>(),
            HashSet::from([
                &"nodes".to_string(),
                &"ports".to_string(),
                &"connections".to_string()
            ])
        );
        let fixture = serde_json::to_string(&value).unwrap();
        let decoded: GraphSnapshot<String, String, String, Flow> =
            serde_json::from_str(&fixture).unwrap();
        assert_eq!(decoded, g.snapshot());
        let state_json = serde_json::to_value(&g).unwrap();
        assert!(state_json.get("selected_nodes").is_none());
        assert!(state_json.get("viewport").is_none());
        let restored: GraphState<String, String, String, Flow> =
            serde_json::from_value(state_json).unwrap();
        assert!(restored.selected_nodes.is_empty());
        assert_eq!(restored.viewport, Viewport::default());
    }
    #[test]
    fn validation_rejects_dangling_and_mismatched_ids_then_canonicalizes() {
        let mut g = connected_graph();
        g.nodes.get_mut("a").unwrap().id = "wrong".into();
        assert!(
            g.validate()
                .unwrap_err()
                .problems
                .iter()
                .any(|p| p.contains("does not match"))
        );
        g.canonicalize_ids();
        assert!(g.validate().is_ok());
        g.ports.get_mut("in").unwrap().node = "missing".into();
        assert!(
            g.validate()
                .unwrap_err()
                .problems
                .iter()
                .any(|p| p.contains("missing node"))
        );
    }
    #[test]
    fn node_removal_cascades_and_reconciles_selection() {
        let mut g = connected_graph();
        g.selected_nodes.insert("a".into());
        g.selected_connections.insert("wire".into());
        let events = g.remove_nodes(&["a".into()]);
        assert!(!g.ports.contains_key("out"));
        assert!(g.connections.is_empty());
        assert!(g.selected_nodes.is_empty() && g.selected_connections.is_empty());
        assert!(
            events
                .iter()
                .any(|e| matches!(e, GraphEvent::ConnectionRemoved { id } if id == "wire"))
        );
    }
    #[test]
    fn viewport_refuses_invalid_zoom_inputs() {
        let mut v = Viewport::default();
        for factor in [0., -1., f32::NAN, f32::INFINITY] {
            v.zoom_at(Point::new(2., 3.), factor, 0.1, 4.);
            assert_eq!(v, Viewport::default());
        }
        v.zoom = 0.;
        assert_eq!(v.screen_to_world(Point::new(4., 2.)), Point::new(4., 2.));
        v.zoom_at(Point::new(4., 2.), 2., 0.1, 4.);
        assert!(v.is_valid());
    }
    #[test]
    fn move_node_is_atomic_and_uses_wide_intermediates() {
        let mut g = connected_graph();
        g.nodes.get_mut("a").unwrap().position = Point::new(-f32::MAX, 0.0);
        g.ports.get_mut("out").unwrap().position = Point::new(-f32::MAX, 25.0);
        assert!(
            g.move_node(&"a".into(), Point::new(f32::MAX, 10.0))
                .is_some()
        );
        assert_eq!(g.nodes["a"].position, Point::new(f32::MAX, 10.0));
        assert_eq!(g.ports["out"].position, Point::new(f32::MAX, 35.0));

        let before_node = g.nodes["a"].position;
        g.ports.get_mut("out").unwrap().position = Point::new(-f32::MAX, 35.0);
        let before_port = g.ports["out"].position;
        assert!(
            g.move_node(&"a".into(), Point::new(-f32::MAX, 20.0))
                .is_none()
        );
        assert_eq!(g.nodes["a"].position, before_node);
        assert_eq!(g.ports["out"].position, before_port);
    }
    #[test]
    fn multi_node_move_is_atomic_and_preserves_caller_order() {
        let mut graph = connected_graph();
        let event = graph
            .move_nodes(&[
                ("b".to_string(), Point::new(120.0, 10.0)),
                ("a".to_string(), Point::new(20.0, 5.0)),
            ])
            .expect("both finite nodes should move");
        let GraphEvent::NodesMoved { nodes } = event else {
            panic!("movement must report one batched event");
        };
        assert_eq!(
            nodes,
            vec![
                ("b".to_string(), Point::new(120.0, 10.0)),
                ("a".to_string(), Point::new(20.0, 5.0)),
            ]
        );
        assert_eq!(graph.ports["in"].position, Point::new(120.0, 35.0));
        assert_eq!(graph.ports["out"].position, Point::new(70.0, 30.0));

        graph.ports.get_mut("in").unwrap().position = Point::new(f32::MAX, 35.0);
        let before_a = graph.nodes["a"].position;
        let before_b = graph.nodes["b"].position;
        assert!(
            graph
                .move_nodes(&[
                    ("a".to_string(), Point::new(30.0, 5.0)),
                    ("b".to_string(), Point::new(f32::MAX, 10.0)),
                ])
                .is_none()
        );
        assert_eq!(graph.nodes["a"].position, before_a);
        assert_eq!(graph.nodes["b"].position, before_b);
    }

    #[test]
    fn viewport_operations_stay_finite_at_f32_extremes() {
        let mut v = Viewport {
            pan: Point::new(f32::MAX, -f32::MAX),
            zoom: f32::MAX,
        };
        let screen = v.world_to_screen(Point::new(f32::MAX, -f32::MAX));
        assert!(screen.x.is_finite() && screen.y.is_finite());
        let world = v.screen_to_world(Point::new(-f32::MAX, f32::MAX));
        assert!(world.x.is_finite() && world.y.is_finite());
        v.zoom_at(
            Point::new(f32::MAX, -f32::MAX),
            f32::MAX,
            f32::MIN_POSITIVE,
            f32::MAX,
        );
        assert!(v.is_valid());
        assert!(v.pan_between(
            Point::new(-f32::MAX, f32::MAX),
            Point::new(f32::MAX, -f32::MAX),
        ));
        assert!(v.is_valid());
        assert!(v.scale_length(f32::MAX).is_finite());
        assert_eq!(v.scale_length(f32::INFINITY), 0.0);
    }
    #[test]
    fn reconciliation_and_removal_report_pruned_selection() {
        let mut g = connected_graph();
        g.selected_nodes.insert("a".into());
        g.selected_connections.insert("wire".into());
        let mut snapshot = g.snapshot();
        snapshot.nodes.remove("a");
        snapshot.ports.remove("out");
        snapshot.connections.remove("wire");
        let events = g.reconcile(snapshot).unwrap();
        assert!(events.iter().any(|event| matches!(
            event,
            GraphEvent::SelectionChanged { nodes, connections }
                if nodes.is_empty() && connections.is_empty()
        )));

        let mut g = connected_graph();
        g.selected_nodes.insert("a".into());
        g.selected_connections.insert("wire".into());
        let events = g.remove_nodes(&["a".into()]);
        assert!(events.iter().any(|event| matches!(
            event,
            GraphEvent::SelectionChanged { nodes, connections }
                if nodes.is_empty() && connections.is_empty()
        )));
    }
    #[test]
    fn id_wrapper_serializes_and_implements_all_roles() {
        fn roles<T: NodeId + PortId + ConnectionId>() {}
        roles::<Id<String>>();
        let json = serde_json::to_string(&Id("x".to_string())).unwrap();
        assert_eq!(json, "\"x\"");
    }
}
