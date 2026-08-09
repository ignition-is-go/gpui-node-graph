use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    hash::Hash,
};

pub trait NodeId: Clone + Eq + Hash + Debug + 'static {}
pub trait PortId: Clone + Eq + Hash + Debug + 'static {}
pub trait ConnectionId: Clone + Eq + Hash + Debug + 'static {}
impl<T: Clone + Eq + Hash + Debug + 'static> NodeId for Id<T> {}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Id<T>(pub T);

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
    pub fn world_to_screen(self, p: Point) -> Point {
        Point::new(p.x * self.zoom + self.pan.x, p.y * self.zoom + self.pan.y)
    }
    pub fn screen_to_world(self, p: Point) -> Point {
        Point::new(
            (p.x - self.pan.x) / self.zoom,
            (p.y - self.pan.y) / self.zoom,
        )
    }
    pub fn zoom_at(&mut self, screen: Point, factor: f32, min: f32, max: f32) {
        let world = self.screen_to_world(screen);
        self.zoom = (self.zoom * factor).clamp(min, max);
        self.pan = screen - Point::new(world.x * self.zoom, world.y * self.zoom)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node<N> {
    pub id: N,
    pub title: String,
    pub position: Point,
    pub size: Size,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Port<N, P, T> {
    pub id: P,
    pub node: N,
    pub label: String,
    pub direction: PortDirection,
    pub kind: T,
    pub position: Point,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
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
    Undo,
    Redo,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GraphState<N: Eq + Hash, P: Eq + Hash, C: Eq + Hash, T> {
    pub nodes: HashMap<N, Node<N>>,
    pub ports: HashMap<P, Port<N, P, T>>,
    pub connections: HashMap<C, Connection<P, C>>,
    pub selected_nodes: HashSet<N>,
    pub selected_connections: HashSet<C>,
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
impl<N: Clone + Eq + Hash, P: Clone + Eq + Hash, C: Clone + Eq + Hash, T: PortType>
    GraphState<N, P, C, T>
{
    pub fn compatible_target(&self, source: &P, target: &P) -> bool {
        let Some(a) = self.ports.get(source) else {
            return false;
        };
        let Some(b) = self.ports.get(target) else {
            return false;
        };
        a.node != b.node
            && a.direction != b.direction
            && match a.direction {
                PortDirection::Output => T::compatible(&a.kind, &b.kind),
                PortDirection::Input => T::compatible(&b.kind, &a.kind),
            }
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
}
