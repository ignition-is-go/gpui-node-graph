//! Deterministic, framework-free obstacle-aware orthogonal routing.
//!
//! The router builds a sparse visibility grid from inflated obstacle edges and
//! endpoint coordinates, then runs A* with a bend penalty. It deliberately has
//! no knowledge of GPUI, graph IDs, or rendering. Callers may cache its output
//! using their own stable connection IDs.

use crate::{Point, Rect};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubwayOptions {
    /// Empty space retained between routes and node bounds.
    pub clearance: f32,
    /// Added cost for changing axis. Higher values favor fewer bends.
    pub bend_penalty: f32,
    /// Refuse grids above this size and return a deterministic fallback.
    pub max_grid_cells: usize,
    /// Per-route A* expansion budget.
    pub max_expansions: usize,
}

impl Default for SubwayOptions {
    fn default() -> Self {
        Self {
            clearance: 16.0,
            bend_penalty: 60.0,
            max_grid_cells: 400_000,
            max_expansions: 30_000,
        }
    }
}

/// Endpoint ownership is optional. When supplied, the router makes a short
/// horizontal escape from an output's right edge and into an input's left edge.
/// This matches the port orientation used by the editor while keeping the core
/// API independent from graph model types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubwayConnection {
    pub start: Point,
    pub end: Point,
    pub start_obstacle: Option<usize>,
    pub end_obstacle: Option<usize>,
}

impl SubwayConnection {
    pub const fn new(start: Point, end: Point) -> Self {
        Self {
            start,
            end,
            start_obstacle: None,
            end_obstacle: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteKind {
    Routed,
    GridLimitFallback,
    ExpansionLimitFallback,
    NoPathFallback,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SubwayRoute {
    pub points: Vec<Point>,
    pub kind: RouteKind,
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

impl Bounds {
    fn from_rect(rect: Rect, clearance: f32) -> Option<Self> {
        let left = rect.origin.x - clearance;
        let right = rect.origin.x + rect.size.width + clearance;
        let top = rect.origin.y - clearance;
        let bottom = rect.origin.y + rect.size.height + clearance;
        (rect.size.width > 0.0
            && rect.size.height > 0.0
            && [left, right, top, bottom].iter().all(|v| v.is_finite()))
        .then_some(Self {
            left,
            right,
            top,
            bottom,
        })
    }

    fn strictly_contains(self, p: Point) -> bool {
        p.x > self.left && p.x < self.right && p.y > self.top && p.y < self.bottom
    }
}

#[derive(Clone, Copy, Debug)]
struct QueueEntry {
    estimate: f32,
    cost: f32,
    state: usize,
}

impl PartialEq for QueueEntry {
    fn eq(&self, other: &Self) -> bool {
        self.estimate.to_bits() == other.estimate.to_bits()
            && self.cost.to_bits() == other.cost.to_bits()
            && self.state == other.state
    }
}
impl Eq for QueueEntry {}
impl PartialOrd for QueueEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueueEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max heap, so reverse every key. State is the final
        // stable tie breaker, making output independent of hash/random state.
        other
            .estimate
            .total_cmp(&self.estimate)
            .then_with(|| other.cost.total_cmp(&self.cost))
            .then_with(|| other.state.cmp(&self.state))
    }
}

const NONE: usize = 0;
const HORIZONTAL: usize = 1;
const VERTICAL: usize = 2;

/// Route one connection. Invalid tuning values are sanitized; non-finite
/// endpoints cannot form a useful grid and therefore receive the fallback.
pub fn compute_subway_route(
    obstacles: &[Rect],
    connection: SubwayConnection,
    options: SubwayOptions,
) -> SubwayRoute {
    let fallback = |kind| SubwayRoute {
        points: midpoint_fallback(connection.start, connection.end),
        kind,
    };
    if !finite_point(connection.start) || !finite_point(connection.end) {
        return fallback(RouteKind::NoPathFallback);
    }
    let clearance = if options.clearance.is_finite() {
        options.clearance.max(0.0)
    } else {
        0.0
    };
    let bend_penalty = if options.bend_penalty.is_finite() {
        options.bend_penalty.max(0.0)
    } else {
        0.0
    };
    let inflated: Vec<_> = obstacles
        .iter()
        .filter_map(|r| Bounds::from_rect(*r, clearance))
        .collect();

    // The obstacle indices refer to the original slice. Resolve them directly,
    // because invalid/zero-sized rectangles are intentionally not obstacles.
    let owner_bounds = |index: Option<usize>| {
        index
            .and_then(|i| obstacles.get(i))
            .and_then(|r| Bounds::from_rect(*r, clearance))
    };
    let route_start = owner_bounds(connection.start_obstacle)
        .map(|b| Point::new(b.right, connection.start.y))
        .unwrap_or(connection.start);
    let route_end = owner_bounds(connection.end_obstacle)
        .map(|b| Point::new(b.left, connection.end.y))
        .unwrap_or(connection.end);

    let mut xs = vec![route_start.x, route_end.x];
    let mut ys = vec![route_start.y, route_end.y];
    for b in &inflated {
        xs.extend([b.left, b.right]);
        ys.extend([b.top, b.bottom]);
    }
    // Outer corridors preserve a route even when overlapping obstacles form a
    // wall along all of their own edge coordinates.
    if !inflated.is_empty() {
        let extra = clearance.max(1.0);
        let outer = [
            inflated
                .iter()
                .map(|b| b.left)
                .fold(route_start.x.min(route_end.x), f32::min)
                - extra,
            inflated
                .iter()
                .map(|b| b.right)
                .fold(route_start.x.max(route_end.x), f32::max)
                + extra,
            inflated
                .iter()
                .map(|b| b.top)
                .fold(route_start.y.min(route_end.y), f32::min)
                - extra,
            inflated
                .iter()
                .map(|b| b.bottom)
                .fold(route_start.y.max(route_end.y), f32::max)
                + extra,
        ];
        xs.extend(outer[..2].iter().copied().filter(|value| value.is_finite()));
        ys.extend(outer[2..].iter().copied().filter(|value| value.is_finite()));
    }

    sort_dedup(&mut xs);
    sort_dedup(&mut ys);
    let Some(cells) = xs.len().checked_mul(ys.len()) else {
        return fallback(RouteKind::GridLimitFallback);
    };
    if cells > options.max_grid_cells {
        return fallback(RouteKind::GridLimitFallback);
    }

    let sx = coordinate_index(&xs, route_start.x);
    let sy = coordinate_index(&ys, route_start.y);
    let gx = coordinate_index(&xs, route_end.x);
    let gy = coordinate_index(&ys, route_end.y);
    let node = |x: usize, y: usize| y * xs.len() + x;
    let start_node = node(sx, sy);
    let goal_node = node(gx, gy);
    let Some(state_count) = cells.checked_mul(3) else {
        return fallback(RouteKind::GridLimitFallback);
    };
    let mut costs = vec![f32::INFINITY; state_count];
    let mut previous = vec![usize::MAX; state_count];
    let start_state = start_node * 3 + NONE;
    costs[start_state] = 0.0;
    let mut queue = BinaryHeap::new();
    queue.push(QueueEntry {
        estimate: manhattan(route_start, route_end),
        cost: 0.0,
        state: start_state,
    });
    let mut expansions = 0usize;
    let mut goal_state = None;

    while let Some(entry) = queue.pop() {
        if entry.cost != costs[entry.state] {
            continue;
        }
        let grid_node = entry.state / 3;
        let incoming = entry.state % 3;
        if grid_node == goal_node {
            goal_state = Some(entry.state);
            break;
        }
        if expansions >= options.max_expansions {
            return fallback(RouteKind::ExpansionLimitFallback);
        }
        expansions += 1;
        let x = grid_node % xs.len();
        let y = grid_node / xs.len();
        // Fixed order plus the heap tie breaker is part of determinism.
        let neighbors = [
            x.checked_sub(1).map(|nx| (nx, y, HORIZONTAL)),
            (x + 1 < xs.len()).then_some((x + 1, y, HORIZONTAL)),
            y.checked_sub(1).map(|ny| (x, ny, VERTICAL)),
            (y + 1 < ys.len()).then_some((x, y + 1, VERTICAL)),
        ];
        let here = Point::new(xs[x], ys[y]);
        for (nx, ny, direction) in neighbors.into_iter().flatten() {
            let there = Point::new(xs[nx], ys[ny]);
            if inflated
                .iter()
                .any(|b| b.strictly_contains(there) || segment_crosses_interior(here, there, *b))
            {
                continue;
            }
            let next = node(nx, ny) * 3 + direction;
            let bend = if incoming != NONE && incoming != direction {
                bend_penalty
            } else {
                0.0
            };
            let next_cost = entry.cost + manhattan(here, there) + bend;
            if next_cost < costs[next] {
                costs[next] = next_cost;
                previous[next] = entry.state;
                queue.push(QueueEntry {
                    estimate: next_cost + manhattan(there, route_end),
                    cost: next_cost,
                    state: next,
                });
            }
        }
    }
    let Some(mut cursor) = goal_state else {
        return fallback(RouteKind::NoPathFallback);
    };
    let mut points = Vec::new();
    loop {
        let n = cursor / 3;
        points.push(Point::new(xs[n % xs.len()], ys[n / xs.len()]));
        if cursor == start_state {
            break;
        }
        cursor = previous[cursor];
        if cursor == usize::MAX {
            return fallback(RouteKind::NoPathFallback);
        }
    }
    points.reverse();
    if route_start != connection.start {
        points.insert(0, connection.start);
    }
    if route_end != connection.end {
        points.push(connection.end);
    }
    simplify(&mut points);
    SubwayRoute {
        points,
        kind: RouteKind::Routed,
    }
}

fn finite_point(p: Point) -> bool {
    p.x.is_finite() && p.y.is_finite()
}
fn manhattan(a: Point, b: Point) -> f32 {
    (a.x - b.x).abs() + (a.y - b.y).abs()
}
fn coordinate_index(values: &[f32], needle: f32) -> usize {
    values
        .binary_search_by(|v| v.total_cmp(&needle))
        .expect("endpoint coordinate inserted")
}
fn sort_dedup(values: &mut Vec<f32>) {
    values.sort_by(f32::total_cmp);
    values.dedup_by(|a, b| a.to_bits() == b.to_bits() || *a == *b);
}
fn segment_crosses_interior(a: Point, b: Point, r: Bounds) -> bool {
    if a.y == b.y {
        a.y > r.top && a.y < r.bottom && a.x.max(b.x) > r.left && a.x.min(b.x) < r.right
    } else {
        a.x > r.left && a.x < r.right && a.y.max(b.y) > r.top && a.y.min(b.y) < r.bottom
    }
}
fn simplify(points: &mut Vec<Point>) {
    points.dedup();
    let mut i = 1;
    while i + 1 < points.len() {
        let a = points[i - 1];
        let b = points[i];
        let c = points[i + 1];
        if (a.x == b.x && b.x == c.x) || (a.y == b.y && b.y == c.y) {
            points.remove(i);
        } else {
            i += 1;
        }
    }
}
fn midpoint_fallback(a: Point, b: Point) -> Vec<Point> {
    let mid = a.x * 0.5 + b.x * 0.5;
    let mut points = vec![a, Point::new(mid, a.y), Point::new(mid, b.y), b];
    simplify(&mut points);
    points
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
        Rect {
            origin: Point::new(x, y),
            size: crate::Size { width, height },
        }
    }
    fn assert_orthogonal(points: &[Point]) {
        assert!(points.len() >= 2);
        assert!(
            points
                .windows(2)
                .all(|p| p[0].x == p[1].x || p[0].y == p[1].y),
            "{points:?}"
        );
    }

    #[test]
    fn avoids_inflated_obstacle_deterministically() {
        let obstacle = rect(40.0, -10.0, 20.0, 20.0);
        let connection = SubwayConnection::new(Point::new(0.0, 0.0), Point::new(100.0, 0.0));
        let options = SubwayOptions {
            clearance: 5.0,
            bend_penalty: 10.0,
            ..Default::default()
        };
        let first = compute_subway_route(&[obstacle], connection, options);
        let second = compute_subway_route(&[obstacle], connection, options);
        assert_eq!(first, second);
        assert_eq!(first.kind, RouteKind::Routed);
        assert_orthogonal(&first.points);
        let inflated = Bounds::from_rect(obstacle, 5.0).unwrap();
        assert!(
            first
                .points
                .windows(2)
                .all(|p| !segment_crosses_interior(p[0], p[1], inflated)),
            "{:?}",
            first.points
        );
        assert!(first.points.iter().any(|p| p.y.abs() >= 15.0));
    }

    #[test]
    fn endpoint_owners_get_horizontal_escape_stubs() {
        let obstacles = [rect(0.0, 0.0, 30.0, 30.0), rect(100.0, 0.0, 30.0, 30.0)];
        let route = compute_subway_route(
            &obstacles,
            SubwayConnection {
                start: Point::new(30.0, 15.0),
                end: Point::new(100.0, 15.0),
                start_obstacle: Some(0),
                end_obstacle: Some(1),
            },
            SubwayOptions {
                clearance: 8.0,
                ..Default::default()
            },
        );
        assert_eq!(route.kind, RouteKind::Routed);
        assert_eq!(
            route.points,
            vec![Point::new(30.0, 15.0), Point::new(100.0, 15.0)]
        );
    }

    #[test]
    fn expansion_budget_has_stable_fallback() {
        let connection = SubwayConnection::new(Point::new(0.0, 0.0), Point::new(100.0, 100.0));
        let route = compute_subway_route(
            &[rect(40.0, 40.0, 20.0, 20.0)],
            connection,
            SubwayOptions {
                max_expansions: 0,
                ..Default::default()
            },
        );
        assert_eq!(route.kind, RouteKind::ExpansionLimitFallback);
        assert_eq!(
            route.points,
            vec![
                Point::new(0.0, 0.0),
                Point::new(50.0, 0.0),
                Point::new(50.0, 100.0),
                Point::new(100.0, 100.0)
            ]
        );
    }

    #[test]
    fn empty_scene_collapses_collinear_waypoints() {
        let route = compute_subway_route(
            &[],
            SubwayConnection::new(Point::new(2.0, 3.0), Point::new(9.0, 3.0)),
            SubwayOptions::default(),
        );
        assert_eq!(
            route.points,
            vec![Point::new(2.0, 3.0), Point::new(9.0, 3.0)]
        );
    }
}
