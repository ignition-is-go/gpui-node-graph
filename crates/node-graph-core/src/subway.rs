//! Orthogonal "subway map" routing.
//!
//! Routes every connection as an axis-aligned polyline that goes AROUND node
//! bounding boxes instead of through them. Paths are found with A* over a
//! sparse routing grid built from the inflated node bounds (plus the anchor
//! stub coordinates), with a bend penalty so runs stay long and straight.
//! After routing, collinear segments that would sit exactly on top of each
//! other are nudged apart into parallel lanes — so distinct connections only
//! ever cross at 90° and never overlap along a shared corridor.
//!
//! Pure module: no reactive graph, no DOM, fully unit-testable. Rects and
//! connections are addressed by INDEX (their position in the input slices),
//! so the caller keeps its own id types.
//!
//! Ported from the Svelte client's `nodes/subwayRouter.ts`; the phase
//! structure, cost model and default tuning are kept identical so both
//! clients draw the same picture.

use std::collections::{HashMap, HashSet};

use crate::{Point, Rect};

/// Coordinate slop. Two coordinates within this are the same grid line.
const EPS: f32 = 0.5;

const DIR_NONE: usize = 0;
const DIR_H: usize = 1;
const DIR_V: usize = 2;

/// Slightly inflate the heuristic — trades exact optimality for speed.
/// Keep close to 1: a weight of w admits paths up to w× longer than optimal,
/// and route inefficiency is far more visible than solve time.
const HEURISTIC_WEIGHT: f32 = 1.1;

/// Horizontal travel off the goal row costs slightly more than on it.
/// L-shaped routes have two equal-cost corner orientations (turn at the source
/// vs turn at the target); without a bias A* picks arbitrarily per wire and
/// neighbouring wires swing opposite ways. The bias makes every wire do its
/// vertical run early — at its source escape column — and approach along the
/// input row, one consistent corner family.
const OFF_GOAL_ROW_BIAS: f32 = 0.04;

/// Furthest an escape fan may spread beyond a node's inflated boundary.
const ESCAPE_SPREAD_BUDGET: f32 = 64.0;

/// A node's bounding box in canvas space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SubwayRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl From<Rect> for SubwayRect {
    fn from(rect: Rect) -> Self {
        Self {
            x: rect.origin.x,
            y: rect.origin.y,
            w: rect.size.width,
            h: rect.size.height,
        }
    }
}

/// One wire to route, anchor to anchor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubwayConnection {
    /// Output-anchor position.
    pub start: Point,
    /// Input-anchor position.
    pub end: Point,
    /// Index into the rect slice, when the endpoint's node is known.
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

/// Tuning. [`Default`] matches the Svelte client.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SubwayOptions {
    /// Clearance kept around node bounds (and stub length off each anchor).
    pub clearance: f32,
    /// Offset between parallel lanes sharing a corridor.
    pub lane_gap: f32,
    /// Extra cost per 90° turn — higher means straighter routes.
    pub bend_penalty: f32,
    /// Bail out to elbow fallback when the routing grid gets this big.
    pub max_grid_cells: usize,
    /// Per-connection A* expansion cap before falling back to an elbow.
    pub max_expansions: usize,
    /// Whole-batch A* expansion budget — keeps worst-case latency bounded.
    pub max_total_expansions: usize,
    /// Cost of crossing an already-routed line (0 disables crossing avoidance).
    pub crossing_penalty: f32,
    /// Length multiplier for riding a corridor an unrelated route occupies —
    /// negative DISCOUNTS sharing, bundling routes into common trunks.
    pub overlap_factor: f32,
}

impl Default for SubwayOptions {
    fn default() -> Self {
        Self {
            clearance: 16.0,
            lane_gap: 8.0,
            bend_penalty: 60.0,
            max_grid_cells: 400_000,
            max_expansions: 30_000,
            max_total_expansions: 600_000,
            // A clean 90° crossing is part of the subway look, so it's priced
            // LOW — half a bend. High penalties (or corridor discounts) fund
            // long hook detours around busy bundles, which read far worse than
            // the crossings they avoid. Keep it below 2 × bend_penalty or A*
            // buys S-jogs to relocate crossings onto quieter vertices.
            crossing_penalty: 30.0,
            overlap_factor: 0.0,
        }
    }
}

/// Rounded anchor position, used to tell "same pin" wires apart from foreign
/// ones. Routes sharing a pin are one subway line, not competing lanes.
type AnchorKey = (i64, i64);

fn anchor_key(p: Point) -> AnchorKey {
    (p.x.round() as i64, p.y.round() as i64)
}

/// True when any segment of an orthogonal route comes within `pad` of the
/// rect. Segments are axis-aligned, so AABB overlap is exact.
pub fn route_intersects_rect(pts: &[Point], rect: Rect, pad: f32) -> bool {
    route_intersects_subway_rect(pts, rect.into(), pad)
}

fn route_intersects_subway_rect(pts: &[Point], rect: SubwayRect, pad: f32) -> bool {
    let left = rect.x - pad;
    let right = rect.x + rect.w + pad;
    let top = rect.y - pad;
    let bottom = rect.y + rect.h + pad;
    pts.windows(2).any(|w| {
        let (a, b) = (w[0], w[1]);
        a.x.max(b.x) >= left
            && a.x.min(b.x) <= right
            && a.y.max(b.y) >= top
            && a.y.min(b.y) <= bottom
    })
}

/// Diagnostic summary for a Subway batch.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SubwayRoutingStats {
    pub grid_width: usize,
    pub grid_height: usize,
    pub grid_cells: usize,
    pub expansions: usize,
    pub routed: usize,
    pub grid_fallbacks: usize,
    pub budget_fallbacks: usize,
    pub no_path_fallbacks: usize,
}

impl SubwayRoutingStats {
    pub fn fallbacks(self) -> usize {
        self.grid_fallbacks + self.budget_fallbacks + self.no_path_fallbacks
    }
}

/// Compute orthogonal routes for all connections at once.
///
/// Returns one waypoint list per connection, in input order, anchor to anchor
/// inclusive. Every connection always gets a route: those that can't be routed
/// around obstacles (or when the scene exceeds the grid budget) fall back to a
/// mid-point elbow, which the lane pass still keeps non-overlapping.
fn compute_subway_routes_internal(
    rects: &[SubwayRect],
    connections: &[SubwayConnection],
    options: &SubwayOptions,
) -> Vec<Vec<Point>> {
    compute_subway_routes_with_stats_internal(rects, connections, options).0
}

fn compute_subway_routes_with_stats_internal(
    rects: &[SubwayRect],
    connections: &[SubwayConnection],
    options: &SubwayOptions,
) -> (Vec<Vec<Point>>, SubwayRoutingStats) {
    let mut routes: Vec<Vec<Point>> = vec![Vec::new(); connections.len()];
    let mut stats = SubwayRoutingStats::default();
    if connections.is_empty() {
        return (routes, stats);
    }

    // Unmeasured nodes (zero size) are not obstacles.
    let valid: Vec<usize> = (0..rects.len())
        .filter(|&i| rects[i].w > 0.0 && rects[i].h > 0.0)
        .collect();
    let valid_rects: Vec<SubwayRect> = valid.iter().map(|&i| rects[i]).collect();

    // Per-side inflation, clamped to half the gap to the nearest neighbour on
    // that side. With a fixed margin, tightly packed rows produce OVERLAPPING
    // obstacles — no corridor exists, every route between them strands and
    // falls back to a through-the-boxes elbow. Clamping guarantees a corridor
    // between any two non-touching boxes.
    let inflations = compute_inflations(&valid_rects, options.clearance);
    // rect index → index into `valid_rects` / `inflations`
    let mut slot: HashMap<usize, usize> = HashMap::new();
    for (slot_idx, &rect_idx) in valid.iter().enumerate() {
        slot.insert(rect_idx, slot_idx);
    }
    let inflation_of = |rect_idx: Option<usize>| -> Option<&SideInflation> {
        rect_idx.and_then(|i| slot.get(&i)).map(|&s| &inflations[s])
    };
    let rect_of = |rect_idx: Option<usize>| -> Option<SubwayRect> {
        rect_idx.and_then(|i| slot.get(&i)).map(|&s| valid_rects[s])
    };

    // Anchor stubs: step out to the node's inflated boundary in the direction
    // the anchor faces, so the path starts ON a routable corridor. Anchor
    // centers sit slightly INSIDE the node bounds, so stepping from the anchor
    // itself would land inside the node's own obstacle and strand the search.
    let mut jobs: Vec<Job> = connections
        .iter()
        .enumerate()
        .map(|(i, c)| Job {
            index: i,
            conn: *c,
            stub_start: make_stub(
                c.start,
                rect_of(c.start_obstacle),
                inflation_of(c.start_obstacle),
                options.clearance,
            ),
            stub_end: make_stub(
                c.end,
                rect_of(c.end_obstacle),
                inflation_of(c.end_obstacle),
                options.clearance,
            ),
        })
        .collect();

    // Escape lanes: the FIRST bend out of a pin (and the last one into a pin)
    // is a special case — all wires sharing a node side get their own
    // staggered stub column, ranked so the corners nest: among down-turning
    // wires the topmost pin turns furthest out, among up-turners the
    // bottommost does. Same-pin wires share one column (they're a trunk).
    apply_escape_lanes(
        &mut jobs,
        &slot,
        &valid_rects,
        &inflations,
        options.lane_gap,
    );

    let inflated: Vec<InflatedRect> = valid_rects
        .iter()
        .zip(inflations.iter())
        .map(|(r, s)| InflatedRect {
            left: r.x - s.left,
            right: r.x + r.w + s.right,
            top: r.y - s.top,
            bottom: r.y + r.h + s.bottom,
        })
        .collect();

    // Routing grid: every inflated rect edge plus every stub coordinate is a
    // candidate corridor. Paths travel along these lines only.
    let mut xs_raw = Vec::with_capacity(inflated.len() * 2 + jobs.len() * 2);
    let mut ys_raw = Vec::with_capacity(inflated.len() * 2 + jobs.len() * 2);
    for r in &inflated {
        xs_raw.push(r.left);
        xs_raw.push(r.right);
        ys_raw.push(r.top);
        ys_raw.push(r.bottom);
    }
    for j in &jobs {
        xs_raw.push(j.stub_start.x);
        xs_raw.push(j.stub_end.x);
        ys_raw.push(j.stub_start.y);
        ys_raw.push(j.stub_end.y);
    }
    let xs = dedupe_sorted(xs_raw);
    let ys = dedupe_sorted(ys_raw);
    let (nx, ny) = (xs.len(), ys.len());
    stats.grid_width = nx;
    stats.grid_height = ny;
    stats.grid_cells = nx.saturating_mul(ny);

    if nx < 2 || ny < 2 || stats.grid_cells > options.max_grid_cells {
        for j in &jobs {
            routes[j.index] = elbow_route(&j.conn, j.stub_start, j.stub_end);
        }
        stats.grid_fallbacks = jobs.len();
        nudge_overlaps(&mut routes, options.lane_gap, &valid_rects);
        return (routes, stats);
    }

    // Edge blockers: h_blocked[yi * (nx-1) + xi] — the horizontal grid edge
    // from xs[xi]→xs[xi+1] along ys[yi] passes through a node interior.
    // Running exactly ON an inflated boundary is allowed (that IS the corridor).
    let mut h_blocked = vec![false; ny * (nx - 1)];
    let mut v_blocked = vec![false; nx * (ny - 1)];
    for r in &inflated {
        // Rows whose y is strictly inside the rect
        let ry0 = upper_bound(&ys, r.top + EPS);
        let ry1 = lower_bound(&ys, r.bottom - EPS) as isize - 1;
        // Horizontal edges overlapping the rect's open x-interval
        let hx0 = (upper_bound(&xs, r.left + EPS) as isize - 1).max(0);
        let hx1 = (lower_bound(&xs, r.right - EPS) as isize - 1).min(nx as isize - 2);
        for yi in ry0 as isize..=ry1 {
            let base = yi as usize * (nx - 1);
            for xi in hx0..=hx1 {
                h_blocked[base + xi as usize] = true;
            }
        }
        // Columns whose x is strictly inside the rect
        let cx0 = upper_bound(&xs, r.left + EPS);
        let cx1 = lower_bound(&xs, r.right - EPS) as isize - 1;
        // Vertical edges overlapping the rect's open y-interval
        let vy0 = (upper_bound(&ys, r.top + EPS) as isize - 1).max(0);
        let vy1 = (lower_bound(&ys, r.bottom - EPS) as isize - 1).min(ny as isize - 2);
        for xi in cx0 as isize..=cx1 {
            let base = xi as usize * (ny - 1);
            for yi in vy0..=vy1 {
                v_blocked[base + yi as usize] = true;
            }
        }
    }

    // Scratch buffers shared across all A* runs (generation-stamped so they
    // don't need clearing between connections).
    let states = nx * ny * 3;
    let mut scratch = AStarScratch {
        g_score: vec![0.0; states],
        came_from: vec![-1; states],
        generation: vec![0; states],
        current_gen: 0,
        heap: MinHeap::default(),
        expansions_used: 0,
    };

    // Crossing avoidance: routes are laid down shortest-first, each leaving an
    // occupancy imprint; later routes pay to cross or ride a foreign line, so
    // pairs cross only where their endpoints force it. Same-pin routes are
    // exempt — they SHOULD share a trunk corridor.
    let mut occ = Occupancy::default();
    let mut order: Vec<usize> = (0..jobs.len()).collect();
    order.sort_by(|&a, &b| {
        let span =
            |j: &Job| (j.stub_end.x - j.stub_start.x).abs() + (j.stub_end.y - j.stub_start.y).abs();
        span(&jobs[a])
            .partial_cmp(&span(&jobs[b]))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });

    for oi in order {
        let j = &jobs[oi];
        let sxi = index_of(&xs, j.stub_start.x);
        let syi = index_of(&ys, j.stub_start.y);
        let exi = index_of(&xs, j.stub_end.x);
        let eyi = index_of(&ys, j.stub_end.y);

        let budget = options.max_expansions.min(
            options
                .max_total_expansions
                .saturating_sub(scratch.expansions_used),
        );

        let s_key = anchor_key(j.conn.start);
        let e_key = anchor_key(j.conn.end);

        let expansions_before = scratch.expansions_used;
        let mut path = None;
        if budget > 0
            && let (Some(sxi), Some(syi), Some(exi), Some(eyi)) = (sxi, syi, exi, eyi)
        {
            path = astar(
                &xs,
                &ys,
                &h_blocked,
                &v_blocked,
                (sxi, syi),
                (exi, eyi),
                options,
                2.0 * options.clearance + EPS,
                budget,
                &mut scratch,
                if options.crossing_penalty > 0.0 || options.overlap_factor != 0.0 {
                    Some(&occ)
                } else {
                    None
                },
                s_key,
                e_key,
            );
        }

        match path {
            Some(path) => {
                stats.routed += 1;
                let mut full = Vec::with_capacity(path.len() + 2);
                full.push(j.conn.start);
                full.extend(path);
                full.push(j.conn.end);
                let full = simplify(&full);
                mark_occupancy(&mut occ, &xs, &ys, &full, s_key, e_key);
                routes[j.index] = full;
            }
            None => {
                if budget == 0 || scratch.expansions_used - expansions_before >= budget {
                    stats.budget_fallbacks += 1;
                } else {
                    stats.no_path_fallbacks += 1;
                }
                routes[j.index] = elbow_route(&j.conn, j.stub_start, j.stub_end);
            }
        }
    }

    stats.expansions = scratch.expansions_used;
    nudge_overlaps(&mut routes, options.lane_gap, &valid_rects);
    (routes, stats)
}

struct Job {
    index: usize,
    conn: SubwayConnection,
    stub_start: Point,
    stub_end: Point,
}

#[derive(Clone, Copy)]
struct InflatedRect {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
}

#[derive(Clone, Copy)]
struct SideInflation {
    left: f32,
    right: f32,
    top: f32,
    bottom: f32,
    /// Horizontal distance to the nearest vertically-overlapping neighbour —
    /// bounds how far escape-lane stubs may extend (infinite when clear).
    clear_left: f32,
    clear_right: f32,
}

/// Per-rect, per-side obstacle inflation: starts at `margin` and shrinks to
/// half the gap toward any neighbour closer than 2×margin on that side, so
/// obstacle bounds never swallow the free space between two boxes.
fn compute_inflations(rects: &[SubwayRect], margin: f32) -> Vec<SideInflation> {
    let mut inf = vec![
        SideInflation {
            left: margin,
            right: margin,
            top: margin,
            bottom: margin,
            clear_left: f32::INFINITY,
            clear_right: f32::INFINITY,
        };
        rects.len()
    ];
    for i in 0..rects.len() {
        for j in (i + 1)..rects.len() {
            let (a, b) = (rects[i], rects[j]);
            // Horizontal gaps only constrain when the rects are vertically
            // close enough for their inflated bands to interact, and vice versa.
            let v_close = a.y - margin < b.y + b.h + margin && b.y - margin < a.y + a.h + margin;
            if v_close {
                if a.x + a.w <= b.x {
                    let gap = b.x - (a.x + a.w);
                    inf[i].clear_right = inf[i].clear_right.min(gap);
                    inf[j].clear_left = inf[j].clear_left.min(gap);
                    let half = (gap / 2.0).max(0.0);
                    if half < margin {
                        inf[i].right = inf[i].right.min(half);
                        inf[j].left = inf[j].left.min(half);
                    }
                } else if b.x + b.w <= a.x {
                    let gap = a.x - (b.x + b.w);
                    inf[j].clear_right = inf[j].clear_right.min(gap);
                    inf[i].clear_left = inf[i].clear_left.min(gap);
                    let half = (gap / 2.0).max(0.0);
                    if half < margin {
                        inf[j].right = inf[j].right.min(half);
                        inf[i].left = inf[i].left.min(half);
                    }
                }
            }
            let h_close = a.x - margin < b.x + b.w + margin && b.x - margin < a.x + a.w + margin;
            if h_close {
                if a.y + a.h <= b.y {
                    let half = ((b.y - (a.y + a.h)) / 2.0).max(0.0);
                    if half < margin {
                        inf[i].bottom = inf[i].bottom.min(half);
                        inf[j].top = inf[j].top.min(half);
                    }
                } else if b.y + b.h <= a.y {
                    let half = ((a.y - (b.y + b.h)) / 2.0).max(0.0);
                    if half < margin {
                        inf[j].bottom = inf[j].bottom.min(half);
                        inf[i].top = inf[i].top.min(half);
                    }
                }
            }
        }
    }
    inf
}

/// +1 when the anchor sits on the right half of its node (output side), -1 otherwise.
fn stub_direction(anchor: Point, rect: Option<SubwayRect>) -> f32 {
    match rect {
        Some(r) if r.w > 0.0 => {
            if anchor.x >= r.x + r.w / 2.0 {
                1.0
            } else {
                -1.0
            }
        }
        _ => 1.0,
    }
}

/// Stub point on the node's inflated boundary, horizontal from the anchor.
fn make_stub(
    anchor: Point,
    rect: Option<SubwayRect>,
    inflation: Option<&SideInflation>,
    margin: f32,
) -> Point {
    let dir = stub_direction(anchor, rect);
    let Some(r) = rect.filter(|r| r.w > 0.0) else {
        return Point::new(anchor.x + dir * margin, anchor.y);
    };
    let x = if dir > 0.0 {
        (anchor.x + EPS).max(r.x + r.w + inflation.map_or(margin, |s| s.right))
    } else {
        (anchor.x - EPS).min(r.x - inflation.map_or(margin, |s| s.left))
    };
    Point::new(x, anchor.y)
}

/// Stagger stub columns per (node, side) so first/last bends nest. Mutates the
/// jobs' stub points; runs before the routing grid is built so the staggered
/// columns become grid corridors.
fn apply_escape_lanes(
    jobs: &mut [Job],
    slot: &HashMap<usize, usize>,
    valid_rects: &[SubwayRect],
    inflations: &[SideInflation],
    lane_gap: f32,
) {
    struct EscapeRef {
        job: usize,
        /// false = stub_start, true = stub_end
        is_end: bool,
        node_slot: usize,
        pin_y: f32,
        away_y: f32,
        dir: f32,
        anchor: AnchorKey,
    }

    let mut groups: HashMap<(usize, i8), Vec<EscapeRef>> = HashMap::new();
    for (ji, j) in jobs.iter().enumerate() {
        for is_end in [false, true] {
            let (rect_idx, pin, away) = if is_end {
                (j.conn.end_obstacle, j.conn.end, j.conn.start)
            } else {
                (j.conn.start_obstacle, j.conn.start, j.conn.end)
            };
            let Some(&s) = rect_idx.and_then(|i| slot.get(&i)) else {
                continue;
            };
            let rect = valid_rects[s];
            let dir = stub_direction(pin, Some(rect));
            groups
                .entry((s, if dir > 0.0 { 1 } else { -1 }))
                .or_default()
                .push(EscapeRef {
                    job: ji,
                    is_end,
                    node_slot: s,
                    pin_y: pin.y,
                    away_y: away.y,
                    dir,
                    anchor: anchor_key(pin),
                });
        }
    }

    // Deterministic group order — HashMap iteration isn't stable and the
    // ranking below feeds geometry.
    let mut keys: Vec<(usize, i8)> = groups.keys().copied().collect();
    keys.sort();

    for key in keys {
        let group = &groups[&key];
        // Rank unique anchors — wires sharing a pin ride one trunk column.
        let mut anchors: HashMap<AnchorKey, (f32, f32, usize)> = HashMap::new();
        for m in group {
            let e = anchors.entry(m.anchor).or_insert((m.pin_y, 0.0, 0));
            e.1 += m.away_y;
            e.2 += 1;
        }
        if anchors.len() < 2 {
            continue;
        }

        let min_y = anchors.values().map(|a| a.0).fold(f32::INFINITY, f32::min);
        let max_y = anchors
            .values()
            .map(|a| a.0)
            .fold(f32::NEG_INFINITY, f32::max);

        // Nesting depth: a wire turning down must clear every pin below it, so
        // the topmost down-turner sits outermost; mirrored for up-turners.
        let mut ranked: Vec<(AnchorKey, f32, f32)> = anchors
            .iter()
            .map(|(&k, &(pin_y, away_sum, n))| {
                let away = away_sum / n as f32;
                let depth = if away > pin_y {
                    max_y - pin_y
                } else {
                    pin_y - min_y
                };
                (k, depth, pin_y)
            })
            .collect();
        ranked.sort_by(|p, q| {
            p.1.partial_cmp(&q.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(p.2.partial_cmp(&q.2).unwrap_or(std::cmp::Ordering::Equal))
                .then(p.0.cmp(&q.0))
        });
        let rank_of: HashMap<AnchorKey, usize> =
            ranked.iter().enumerate().map(|(i, r)| (r.0, i)).collect();

        for m in group {
            let rank = rank_of[&m.anchor];
            if rank == 0 {
                continue;
            }
            let s = inflations[m.node_slot];
            let (inf, clear) = if m.dir > 0.0 {
                (s.right, s.clear_right)
            } else {
                (s.left, s.clear_left)
            };
            let max_spread = ESCAPE_SPREAD_BUDGET.min(clear - inf - 4.0).max(0.0);
            let ext = (rank as f32 * lane_gap).min(max_spread);
            if ext > 0.0 {
                let job = &mut jobs[m.job];
                let stub = if m.is_end {
                    &mut job.stub_end
                } else {
                    &mut job.stub_start
                };
                stub.x += m.dir * ext;
            }
        }
    }
}

/// Mid-x elbow through the stub points — used when A* can't find a route.
fn elbow_route(c: &SubwayConnection, stub_start: Point, stub_end: Point) -> Vec<Point> {
    let mid_x = (stub_start.x + stub_end.x) / 2.0;
    simplify(&[
        c.start,
        stub_start,
        Point::new(mid_x, stub_start.y),
        Point::new(mid_x, stub_end.y),
        stub_end,
        c.end,
    ])
}

// =============================================================================
// A* over the sparse grid (direction-aware so bend penalties are exact)
// =============================================================================

struct AStarScratch {
    g_score: Vec<f32>,
    came_from: Vec<i32>,
    /// Generation stamp — an entry is only valid when generation[i] == current_gen.
    generation: Vec<i32>,
    current_gen: i32,
    heap: MinHeap,
    /// Running batch total, checked against `max_total_expansions`.
    expansions_used: usize,
}

/// Occupancy imprint of already-routed connections, for crossing avoidance.
#[derive(Default)]
struct Occupancy {
    /// Horizontal grid edges covered by horizontal runs (edge idx → anchors).
    h_edge: HashMap<usize, HashSet<AnchorKey>>,
    /// Vertical grid edges covered by vertical runs.
    v_edge: HashMap<usize, HashSet<AnchorKey>>,
    /// Vertices interior to horizontal runs (vertex idx → anchors).
    h_vertex: HashMap<usize, HashSet<AnchorKey>>,
    /// Vertices interior to vertical runs.
    v_vertex: HashMap<usize, HashSet<AnchorKey>>,
}

/// True when the occupant set contains a route unrelated to both anchors.
fn has_foreign(set: &HashSet<AnchorKey>, k1: AnchorKey, k2: AnchorKey) -> bool {
    set.iter().any(|k| *k != k1 && *k != k2)
}

fn add_occ(map: &mut HashMap<usize, HashSet<AnchorKey>>, idx: usize, k1: AnchorKey, k2: AnchorKey) {
    let set = map.entry(idx).or_default();
    set.insert(k1);
    set.insert(k2);
}

#[allow(clippy::too_many_arguments)]
fn astar(
    xs: &[f32],
    ys: &[f32],
    h_blocked: &[bool],
    v_blocked: &[bool],
    start: (usize, usize),
    goal: (usize, usize),
    options: &SubwayOptions,
    escape_cap: f32,
    max_expansions: usize,
    scratch: &mut AStarScratch,
    occ: Option<&Occupancy>,
    s_key: AnchorKey,
    e_key: AnchorKey,
) -> Option<Vec<Point>> {
    let (nx, ny) = (xs.len(), ys.len());
    let (sxi, syi) = start;
    let (exi, eyi) = goal;
    let state_of = |xi: usize, yi: usize, dir: usize| (yi * nx + xi) * 3 + dir;

    // Clamp the search to a window around the endpoints. Detours rarely need
    // to wander further; if a route genuinely can't fit it falls back to an
    // elbow, which the lane pass still keeps non-overlapping.
    const WINDOW_PAD: f32 = 800.0;
    let xi_min = lower_bound(xs, xs[sxi].min(xs[exi]) - WINDOW_PAD);
    let xi_max = upper_bound(xs, xs[sxi].max(xs[exi]) + WINDOW_PAD) - 1;
    let yi_min = lower_bound(ys, ys[syi].min(ys[eyi]) - WINDOW_PAD);
    let yi_max = upper_bound(ys, ys[syi].max(ys[eyi]) + WINDOW_PAD) - 1;

    scratch.current_gen += 1;
    let generation = scratch.current_gen;
    scratch.heap.clear();

    let (ex, ey) = (xs[exi], ys[eyi]);

    let start_state = state_of(sxi, syi, DIR_NONE);
    scratch.g_score[start_state] = 0.0;
    scratch.came_from[start_state] = -1;
    scratch.generation[start_state] = generation;
    scratch.heap.push(
        start_state,
        ((ex - xs[sxi]).abs() + (ey - ys[syi]).abs()) * HEURISTIC_WEIGHT,
    );

    let mut expansions = 0usize;
    while let Some(current) = scratch.heap.pop() {
        if expansions >= max_expansions {
            return None;
        }
        scratch.expansions_used += 1;
        expansions += 1;

        let dir = current % 3;
        let cell = current / 3;
        let xi = cell % nx;
        let yi = cell / nx;

        if xi == exi && yi == eyi {
            return Some(reconstruct(xs, ys, nx, scratch, generation, current));
        }

        let g = scratch.g_score[current];
        // The first hop out of the start (and the final hop into the goal) may
        // cross a SHORT blocked edge — anchors of tightly packed nodes can sit
        // inside a NEIGHBOUR's inflated bounds, and without an escape hatch the
        // search would strand there and fall back to a through-everything
        // elbow. The length cap stops the exception from tunnelling whole nodes.
        let is_start = current == start_state;

        // Horizontal neighbours
        for step in [-1isize, 1] {
            let nxi = xi as isize + step;
            if nxi < xi_min as isize || nxi > xi_max as isize {
                continue;
            }
            let nxi = nxi as usize;
            let edge = yi * (nx - 1) + if step == 1 { xi } else { nxi };
            let len = (xs[nxi] - xs[xi]).abs();
            if h_blocked[edge] {
                let may_escape = is_start || (nxi == exi && yi == eyi);
                if !may_escape || len > escape_cap {
                    continue;
                }
            }
            let mut cost =
                len * if yi == eyi {
                    1.0
                } else {
                    1.0 + OFF_GOAL_ROW_BIAS
                } + if dir == DIR_V {
                    options.bend_penalty
                } else {
                    0.0
                };
            if let Some(occ) = occ {
                if occ
                    .h_edge
                    .get(&edge)
                    .is_some_and(|s| has_foreign(s, s_key, e_key))
                {
                    cost += len * options.overlap_factor;
                }
                if occ
                    .v_vertex
                    .get(&(yi * nx + nxi))
                    .is_some_and(|s| has_foreign(s, s_key, e_key))
                {
                    cost += options.crossing_penalty;
                }
            }
            let next = state_of(nxi, yi, DIR_H);
            let ng = g + cost;
            if scratch.generation[next] != generation || ng < scratch.g_score[next] {
                scratch.g_score[next] = ng;
                scratch.came_from[next] = current as i32;
                scratch.generation[next] = generation;
                let h = ((ex - xs[nxi]).abs() + (ey - ys[yi]).abs()) * HEURISTIC_WEIGHT;
                scratch.heap.push(next, ng + h);
            }
        }

        // Vertical neighbours
        for step in [-1isize, 1] {
            let nyi = yi as isize + step;
            if nyi < yi_min as isize || nyi > yi_max as isize {
                continue;
            }
            let nyi = nyi as usize;
            let edge = xi * (ny - 1) + if step == 1 { yi } else { nyi };
            let len = (ys[nyi] - ys[yi]).abs();
            if v_blocked[edge] {
                let may_escape = is_start || (xi == exi && nyi == eyi);
                if !may_escape || len > escape_cap {
                    continue;
                }
            }
            let mut cost = len
                + if dir == DIR_H {
                    options.bend_penalty
                } else {
                    0.0
                };
            if let Some(occ) = occ {
                if occ
                    .v_edge
                    .get(&edge)
                    .is_some_and(|s| has_foreign(s, s_key, e_key))
                {
                    cost += len * options.overlap_factor;
                }
                if occ
                    .h_vertex
                    .get(&(nyi * nx + xi))
                    .is_some_and(|s| has_foreign(s, s_key, e_key))
                {
                    cost += options.crossing_penalty;
                }
            }
            let next = state_of(xi, nyi, DIR_V);
            let ng = g + cost;
            if scratch.generation[next] != generation || ng < scratch.g_score[next] {
                scratch.g_score[next] = ng;
                scratch.came_from[next] = current as i32;
                scratch.generation[next] = generation;
                let h = ((ex - xs[xi]).abs() + (ey - ys[nyi]).abs()) * HEURISTIC_WEIGHT;
                scratch.heap.push(next, ng + h);
            }
        }
    }

    None
}

/// Record a routed path's grid edges and interior vertices so later routes can
/// price crossings and corridor sharing. Segments whose endpoints are off-grid
/// (the anchor→stub jogs) are skipped — they're a stub long.
fn mark_occupancy(
    occ: &mut Occupancy,
    xs: &[f32],
    ys: &[f32],
    pts: &[Point],
    s_key: AnchorKey,
    e_key: AnchorKey,
) {
    let nx = xs.len();
    let ny = ys.len();
    for w in pts.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (a.y - b.y).abs() < EPS && (a.x - b.x).abs() >= EPS {
            let Some(yi) = index_of(ys, a.y) else {
                continue;
            };
            let (Some(i1), Some(i2)) = (index_of(xs, a.x.min(b.x)), index_of(xs, a.x.max(b.x)))
            else {
                continue;
            };
            for e in i1..i2 {
                add_occ(&mut occ.h_edge, yi * (nx - 1) + e, s_key, e_key);
            }
            for v in (i1 + 1)..i2 {
                add_occ(&mut occ.h_vertex, yi * nx + v, s_key, e_key);
            }
        } else if (a.x - b.x).abs() < EPS && (a.y - b.y).abs() >= EPS {
            let Some(xi) = index_of(xs, a.x) else {
                continue;
            };
            let (Some(i1), Some(i2)) = (index_of(ys, a.y.min(b.y)), index_of(ys, a.y.max(b.y)))
            else {
                continue;
            };
            for e in i1..i2 {
                add_occ(&mut occ.v_edge, xi * (ny - 1) + e, s_key, e_key);
            }
            for v in (i1 + 1)..i2 {
                add_occ(&mut occ.v_vertex, v * nx + xi, s_key, e_key);
            }
        }
    }
}

fn reconstruct(
    xs: &[f32],
    ys: &[f32],
    nx: usize,
    scratch: &AStarScratch,
    generation: i32,
    end: usize,
) -> Vec<Point> {
    let mut pts = Vec::new();
    let mut cur = end as i32;
    while cur >= 0 && scratch.generation[cur as usize] == generation {
        let state = cur as usize;
        let cell = state / 3;
        let xi = cell % nx;
        let yi = cell / nx;
        pts.push(Point::new(xs[xi], ys[yi]));
        cur = scratch.came_from[state];
    }
    pts.reverse();
    simplify(&pts)
}

/// Drop duplicate and collinear interior points.
fn simplify(pts: &[Point]) -> Vec<Point> {
    let mut out: Vec<Point> = Vec::with_capacity(pts.len());
    for &p in pts {
        if let Some(prev) = out.last()
            && (prev.x - p.x).abs() < EPS
            && (prev.y - p.y).abs() < EPS
        {
            continue;
        }
        while out.len() >= 2 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let collinear_x = (a.x - b.x).abs() < EPS && (b.x - p.x).abs() < EPS;
            let collinear_y = (a.y - b.y).abs() < EPS && (b.y - p.y).abs() < EPS;
            if collinear_x || collinear_y {
                out.pop();
            } else {
                break;
            }
        }
        out.push(p);
    }
    out
}

// =============================================================================
// Lane nudging — separate collinear overlapping segments across paths
// =============================================================================

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Axis {
    X,
    Y,
}

#[derive(Clone, Copy)]
struct LaneSegment {
    /// Which route, and which segment within it (pts[i] → pts[i+1]).
    route: usize,
    i: usize,
    /// Corridor coordinate (x for verticals, y for horizontals).
    coord: f32,
    /// Range along the segment axis.
    lo: f32,
    hi: f32,
    /// First/last segments are pinned to their anchor — never shifted.
    pinned: bool,
    /// Perpendicular position of the neighbours, used to order lanes.
    sort_key: f32,
    /// Which side the route connects on at the lo end of the segment: sign of
    /// (neighbour waypoint − corridor) along the perpendicular axis, 0 when the
    /// segment ends at an anchor. Drives nesting order.
    lo_dir: f32,
    /// Same at the hi end.
    hi_dir: f32,
}

/// Pairwise nesting preference used as a vote, not directly as a sort
/// comparator: three bracket-shaped runs can form a preference cycle.
fn lane_nesting_preference(a: &LaneSegment, b: &LaneSegment) -> f32 {
    let mut preference = 0.0;
    if a.hi < b.hi - EPS {
        preference += a.hi_dir;
    } else if b.hi < a.hi - EPS {
        preference -= b.hi_dir;
    }
    if a.lo > b.lo + EPS {
        preference += a.lo_dir;
    } else if b.lo > a.lo + EPS {
        preference -= b.lo_dir;
    }
    preference
}

fn aggregate_nesting_scores(representatives: &[LaneSegment]) -> Vec<f32> {
    representatives
        .iter()
        .map(|a| {
            representatives
                .iter()
                .map(|b| lane_nesting_preference(a, b))
                .sum()
        })
        .collect()
}

#[derive(Clone, Copy)]
struct LaneOrderKey {
    nesting_score: f32,
    sort_key: f32,
    coord: f32,
    stable_key: (usize, usize),
}

fn compare_lane_order(a: LaneOrderKey, b: LaneOrderKey) -> std::cmp::Ordering {
    a.nesting_score
        .total_cmp(&b.nesting_score)
        .then(a.sort_key.total_cmp(&b.sort_key))
        .then(a.coord.total_cmp(&b.coord))
        .then(a.stable_key.cmp(&b.stable_key))
}

struct Shift {
    seg: LaneSegment,
    axis: Axis,
    offset: f32,
}

/// Find groups of segments that share a corridor (same or nearly-same x for
/// verticals / y for horizontals) AND overlap along it, then fan them out into
/// parallel lanes `lane_gap` apart. Proximity-based: segments within two lanes'
/// width of each other count as sharing the corridor, so near-misses (e.g.
/// elbow midlines a few px apart) separate too. Segments whose routes share a
/// start or end anchor merge onto ONE trunk line instead of fanning (a fan from
/// a single pin is one subway line, not parallel lanes). Shifts that would push
/// a segment into a node box are skipped. Runs to a fixpoint — a shifted
/// segment can land next to a segment from another corridor group, so detection
/// repeats until no segment moves.
fn nudge_overlaps(routes: &mut [Vec<Point>], lane_gap: f32, rects: &[SubwayRect]) {
    const MAX_PASSES: usize = 3;
    for _ in 0..MAX_PASSES {
        let mut v_segs: Vec<LaneSegment> = Vec::new();
        let mut h_segs: Vec<LaneSegment> = Vec::new();

        for (ri, pts) in routes.iter().enumerate() {
            if pts.len() < 2 {
                continue;
            }
            for i in 0..pts.len() - 1 {
                let a = pts[i];
                let b = pts[i + 1];
                let pinned = i == 0 || i == pts.len() - 2;
                let prev_pt = if i >= 1 { Some(pts[i - 1]) } else { None };
                let next_pt = pts.get(i + 2).copied();
                if (a.x - b.x).abs() < EPS && (a.y - b.y).abs() >= EPS {
                    let prev = prev_pt.map_or(a.x, |p| p.x);
                    let next = next_pt.map_or(a.x, |p| p.x);
                    // lo end = smaller y; its connecting neighbour is prev when
                    // the segment travels downward (a above b), next otherwise
                    let travel_down = b.y > a.y;
                    let (lo_nbr, hi_nbr) = if travel_down {
                        (prev_pt, next_pt)
                    } else {
                        (next_pt, prev_pt)
                    };
                    v_segs.push(LaneSegment {
                        route: ri,
                        i,
                        coord: a.x,
                        lo: a.y.min(b.y),
                        hi: a.y.max(b.y),
                        pinned,
                        sort_key: (prev + next) / 2.0,
                        lo_dir: lo_nbr.map_or(0.0, |p| (p.x - a.x).signum_or_zero()),
                        hi_dir: hi_nbr.map_or(0.0, |p| (p.x - a.x).signum_or_zero()),
                    });
                } else if (a.y - b.y).abs() < EPS && (a.x - b.x).abs() >= EPS {
                    let prev = prev_pt.map_or(a.y, |p| p.y);
                    let next = next_pt.map_or(a.y, |p| p.y);
                    let travel_right = b.x > a.x;
                    let (lo_nbr, hi_nbr) = if travel_right {
                        (prev_pt, next_pt)
                    } else {
                        (next_pt, prev_pt)
                    };
                    h_segs.push(LaneSegment {
                        route: ri,
                        i,
                        coord: a.y,
                        lo: a.x.min(b.x),
                        hi: a.x.max(b.x),
                        pinned,
                        sort_key: (prev + next) / 2.0,
                        lo_dir: lo_nbr.map_or(0.0, |p| (p.y - a.y).signum_or_zero()),
                        hi_dir: hi_nbr.map_or(0.0, |p| (p.y - a.y).signum_or_zero()),
                    });
                }
            }
        }

        let mut shifts: Vec<Shift> = Vec::new();
        collect_shifts(&mut v_segs, Axis::X, &mut shifts, lane_gap, rects, routes);
        collect_shifts(&mut h_segs, Axis::Y, &mut shifts, lane_gap, rects, routes);

        if shifts.is_empty() {
            break;
        }

        for Shift { seg, axis, offset } in shifts {
            for k in [seg.i, seg.i + 1] {
                let p = &mut routes[seg.route][k];
                match axis {
                    Axis::X => p.x += offset,
                    Axis::Y => p.y += offset,
                }
            }
        }
    }
}

trait SignumOrZero {
    fn signum_or_zero(self) -> f32;
}
impl SignumOrZero for f32 {
    /// `f32::signum` returns ±1 for zero; lane nesting needs a true zero.
    fn signum_or_zero(self) -> f32 {
        if self == 0.0 { 0.0 } else { self.signum() }
    }
}

/// Sweep-cluster segments of one orientation: first by corridor proximity
/// (coords within one lane_gap), then by overlap along the corridor; assign
/// lane offsets within each overlapping cluster.
fn collect_shifts(
    segs: &mut [LaneSegment],
    axis: Axis,
    out: &mut Vec<Shift>,
    lane_gap: f32,
    rects: &[SubwayRect],
    routes: &[Vec<Point>],
) {
    if segs.len() < 2 {
        return;
    }
    segs.sort_by(|a, b| {
        a.coord
            .partial_cmp(&b.coord)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then((a.route, a.i).cmp(&(b.route, b.i)))
    });

    let mut corridor: Vec<LaneSegment> = Vec::new();
    let mut corridor_max = f32::NEG_INFINITY;
    for seg in segs.iter() {
        if !corridor.is_empty() && seg.coord - corridor_max > lane_gap - EPS {
            if corridor.len() > 1 {
                cluster_by_overlap(&mut corridor, axis, out, lane_gap, rects, routes);
            }
            corridor.clear();
            corridor_max = f32::NEG_INFINITY;
        }
        corridor.push(*seg);
        corridor_max = corridor_max.max(seg.coord);
    }
    if corridor.len() > 1 {
        cluster_by_overlap(&mut corridor, axis, out, lane_gap, rects, routes);
    }
}

/// Within one corridor, find clusters that overlap along the run axis.
fn cluster_by_overlap(
    corridor: &mut [LaneSegment],
    axis: Axis,
    out: &mut Vec<Shift>,
    lane_gap: f32,
    rects: &[SubwayRect],
    routes: &[Vec<Point>],
) {
    corridor.sort_by(|a, b| {
        a.lo.partial_cmp(&b.lo)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then((a.route, a.i).cmp(&(b.route, b.i)))
    });

    let mut cluster: Vec<LaneSegment> = Vec::new();
    let mut cluster_hi = f32::NEG_INFINITY;
    for seg in corridor.iter() {
        // Strict overlap only — touching at a shared endpoint is a junction,
        // not an overlapping run, and shouldn't fan out.
        if !cluster.is_empty() && seg.lo < cluster_hi - EPS {
            cluster.push(*seg);
            cluster_hi = cluster_hi.max(seg.hi);
        } else {
            if cluster.len() > 1 {
                assign_lanes(&cluster, axis, out, lane_gap, rects, routes);
            }
            cluster.clear();
            cluster.push(*seg);
            cluster_hi = seg.hi;
        }
    }
    if cluster.len() > 1 {
        assign_lanes(&cluster, axis, out, lane_gap, rects, routes);
    }
}

/// True when placing this segment at `target` would cross a node box.
fn lane_blocked(seg: &LaneSegment, axis: Axis, target: f32, rects: &[SubwayRect]) -> bool {
    rects.iter().any(|r| match axis {
        Axis::X => {
            target > r.x + 1.0
                && target < r.x + r.w - 1.0
                && seg.hi > r.y + 1.0
                && seg.lo < r.y + r.h - 1.0
        }
        Axis::Y => {
            target > r.y + 1.0
                && target < r.y + r.h - 1.0
                && seg.hi > r.x + 1.0
                && seg.lo < r.x + r.w - 1.0
        }
    })
}

fn assign_lanes(
    cluster: &[LaneSegment],
    axis: Axis,
    out: &mut Vec<Shift>,
    lane_gap: f32,
    rects: &[SubwayRect],
    routes: &[Vec<Point>],
) {
    // Routes sharing a start or end anchor are ONE subway line — union their
    // segments into trunk units that ride a single coordinate instead of
    // fanning into separate lanes.
    let n = cluster.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut by_key: HashMap<(bool, AnchorKey), usize> = HashMap::new();
    for i in 0..n {
        let pts = &routes[cluster[i].route];
        let Some((&first, &last)) = pts.first().zip(pts.last()) else {
            continue;
        };
        for key in [(false, anchor_key(first)), (true, anchor_key(last))] {
            match by_key.get(&key) {
                None => {
                    by_key.insert(key, i);
                }
                Some(&seen) => {
                    let a = find(&mut parent, seen);
                    let b = find(&mut parent, i);
                    if a != b {
                        parent[b] = a;
                    }
                }
            }
        }
    }
    let mut unit_map: HashMap<usize, Vec<LaneSegment>> = HashMap::new();
    // Insertion order is kept separately: lane assignment below is
    // order-sensitive and `HashMap` iteration isn't stable.
    let mut unit_order: Vec<usize> = Vec::new();
    for (i, seg) in cluster.iter().enumerate() {
        let root = find(&mut parent, i);
        let unit = unit_map.entry(root).or_insert_with(|| {
            unit_order.push(root);
            Vec::new()
        });
        unit.push(*seg);
    }

    struct LaneUnit {
        segs: Vec<LaneSegment>,
        pinned: bool,
        coord: f32,
        sort_key: f32,
        /// Longest member — its end-connections stand in for the unit when
        /// deciding nesting order.
        rep: LaneSegment,
    }
    let units: Vec<LaneUnit> = unit_order
        .iter()
        .map(|root| {
            let segs = unit_map[root].clone();
            let mut rep = segs[0];
            for s in &segs {
                if s.hi - s.lo > rep.hi - rep.lo {
                    rep = *s;
                }
            }
            LaneUnit {
                pinned: segs.iter().any(|s| s.pinned),
                coord: segs.iter().map(|s| s.coord).sum::<f32>() / segs.len() as f32,
                sort_key: segs.iter().map(|s| s.sort_key).sum::<f32>() / segs.len() as f32,
                rep,
                segs,
            }
        })
        .collect();

    // When the full lane offset would push a segment into a node box (tight
    // corridors between packed nodes), back off progressively — partial
    // separation beats either tunnelling a box or staying coincident.
    let push_shift = |seg: &LaneSegment, target: f32, out: &mut Vec<Shift>| {
        let want = target - seg.coord;
        if want.abs() <= EPS / 2.0 {
            return;
        }
        for f in [1.0, 0.75, 0.5, 0.25] {
            let t = seg.coord + want * f;
            if !lane_blocked(seg, axis, t, rects) {
                out.push(Shift {
                    seg: *seg,
                    axis,
                    offset: want * f,
                });
                return;
            }
        }
    };

    // Pinned units hold their anchor line; their movable members snap onto it
    // so a trunk shared with a stub reads as one continuous line.
    let pinned_units: Vec<&LaneUnit> = units.iter().filter(|u| u.pinned).collect();
    for u in &pinned_units {
        let anchored: Vec<&LaneSegment> = u.segs.iter().filter(|s| s.pinned).collect();
        let line = anchored.iter().map(|s| s.coord).sum::<f32>() / anchored.len() as f32;
        for seg in &u.segs {
            if !seg.pinned {
                push_shift(seg, line, out);
            }
        }
    }

    // Lane targets are absolute positions centred on the cluster, so
    // near-coincident corridors collapse into an evenly spaced fan. Units are
    // ordered by where their segments come from / go to, so parallel runs peel
    // off in a stable order and avoid gratuitous criss-crossing.
    let movable_units: Vec<&LaneUnit> = units.iter().filter(|unit| !unit.pinned).collect();
    let representatives: Vec<_> = movable_units.iter().map(|unit| unit.rep).collect();
    let nesting_scores = aggregate_nesting_scores(&representatives);
    let mut movable: Vec<_> = movable_units.into_iter().zip(nesting_scores).collect();
    movable.sort_by(|(a, a_score), (b, b_score)| {
        compare_lane_order(
            LaneOrderKey {
                nesting_score: *a_score,
                sort_key: a.sort_key,
                coord: a.coord,
                stable_key: (a.rep.route, a.rep.i),
            },
            LaneOrderKey {
                nesting_score: *b_score,
                sort_key: b.sort_key,
                coord: b.coord,
                stable_key: (b.rep.route, b.rep.i),
            },
        )
    });
    let movable: Vec<_> = movable.into_iter().map(|(unit, _)| unit).collect();
    if movable.is_empty() {
        return;
    }

    if !pinned_units.is_empty() {
        // Fan movable units around the pinned centre, alternating sides.
        let center = pinned_units.iter().map(|u| u.coord).sum::<f32>() / pinned_units.len() as f32;
        for (k, u) in movable.iter().enumerate() {
            let lane = (k / 2) as f32 + 1.0;
            let side = if k % 2 == 0 { 1.0 } else { -1.0 };
            let target = center + side * lane * lane_gap;
            for seg in &u.segs {
                push_shift(seg, target, out);
            }
        }
        return;
    }

    let n = movable.len();
    let center = movable.iter().map(|u| u.coord).sum::<f32>() / n as f32;

    // Free corridor band: the widest interval around the cluster centre —
    // across the cluster's combined run-range — that touches no node box. Lanes
    // are spread evenly inside it (graceful fan-out, in nesting order so no two
    // lanes swap), compressing spacing only when the corridor is genuinely
    // narrower than the full fan.
    let mut range_lo = f32::INFINITY;
    let mut range_hi = f32::NEG_INFINITY;
    for u in &movable {
        for s in &u.segs {
            range_lo = range_lo.min(s.lo);
            range_hi = range_hi.max(s.hi);
        }
    }
    const PAD: f32 = 2.0;
    let mut band_lo = f32::NEG_INFINITY;
    let mut band_hi = f32::INFINITY;
    for r in rects {
        let (r_lo, r_hi, s_lo, s_hi) = match axis {
            Axis::X => (r.x, r.x + r.w, r.y, r.y + r.h),
            Axis::Y => (r.y, r.y + r.h, r.x, r.x + r.w),
        };
        if s_hi <= range_lo + 1.0 || s_lo >= range_hi - 1.0 {
            continue;
        }
        if r_hi <= center {
            band_lo = band_lo.max(r_hi);
        } else if r_lo >= center {
            band_hi = band_hi.min(r_lo);
        }
        // A rect spanning the centre means the cluster already runs through it
        // (shouldn't happen) — ignore rather than produce an empty band.
    }

    let span = (n as f32 - 1.0) * lane_gap;
    let mut spacing = lane_gap;
    let mut start = center - span / 2.0;
    let lo = band_lo + PAD;
    let hi = band_hi - PAD;
    if lo.is_finite() && hi.is_finite() {
        let width = hi - lo;
        if width <= 0.0 {
            spacing = 0.0;
            start = center;
        } else {
            spacing = if n > 1 {
                lane_gap.min(width / (n as f32 - 1.0))
            } else {
                0.0
            };
            start = center - (spacing * (n as f32 - 1.0)) / 2.0;
            start = start.max(lo).min(hi - spacing * (n as f32 - 1.0));
        }
    } else if lo.is_finite() {
        start = start.max(lo);
    } else if hi.is_finite() {
        start = start.min(hi - span);
    }

    for (k, u) in movable.iter().enumerate() {
        let target = start + k as f32 * spacing;
        for seg in &u.segs {
            let offset = target - seg.coord;
            if offset.abs() <= EPS / 2.0 {
                continue;
            }
            if lane_blocked(seg, axis, target, rects) {
                continue;
            }
            out.push(Shift {
                seg: *seg,
                axis,
                offset,
            });
        }
    }
}

// =============================================================================
// Small utilities
// =============================================================================

fn dedupe_sorted(mut values: Vec<f32>) -> Vec<f32> {
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out: Vec<f32> = Vec::with_capacity(values.len());
    for v in values {
        if out.last().is_none_or(|&last| v - last > EPS) {
            out.push(v);
        }
    }
    out
}

/// First index with arr[i] >= v.
fn lower_bound(arr: &[f32], v: f32) -> usize {
    arr.partition_point(|&x| x < v)
}

/// First index with arr[i] > v.
fn upper_bound(arr: &[f32], v: f32) -> usize {
    arr.partition_point(|&x| x <= v)
}

/// Index of v in the deduped sorted array (within EPS).
fn index_of(arr: &[f32], v: f32) -> Option<usize> {
    let i = lower_bound(arr, v - EPS);
    if i < arr.len() && (arr[i] - v).abs() <= EPS {
        Some(i)
    } else {
        None
    }
}

/// Binary min-heap keyed by f-score. Ties break on the state index so a solve
/// is reproducible run to run.
#[derive(Default)]
struct MinHeap {
    items: Vec<(f32, usize)>,
}

impl MinHeap {
    fn clear(&mut self) {
        self.items.clear();
    }

    fn less(a: (f32, usize), b: (f32, usize)) -> bool {
        match a.0.partial_cmp(&b.0) {
            Some(std::cmp::Ordering::Less) => true,
            Some(std::cmp::Ordering::Equal) => a.1 < b.1,
            _ => false,
        }
    }

    fn push(&mut self, item: usize, score: f32) {
        self.items.push((score, item));
        let mut i = self.items.len() - 1;
        while i > 0 {
            let parent = (i - 1) / 2;
            if !Self::less(self.items[i], self.items[parent]) {
                break;
            }
            self.items.swap(i, parent);
            i = parent;
        }
    }

    fn pop(&mut self) -> Option<usize> {
        if self.items.is_empty() {
            return None;
        }
        let top = self.items[0].1;
        let last = self.items.pop().unwrap();
        if !self.items.is_empty() {
            self.items[0] = last;
            let mut i = 0;
            loop {
                let (l, r) = (i * 2 + 1, i * 2 + 2);
                let mut smallest = i;
                if l < self.items.len() && Self::less(self.items[l], self.items[smallest]) {
                    smallest = l;
                }
                if r < self.items.len() && Self::less(self.items[r], self.items[smallest]) {
                    smallest = r;
                }
                if smallest == i {
                    break;
                }
                self.items.swap(i, smallest);
                i = smallest;
            }
        }
        Some(top)
    }
}

/// Compute obstacle-aware routes for a complete connection batch.
pub fn compute_subway_routes(
    rects: &[Rect],
    connections: &[SubwayConnection],
    options: &SubwayOptions,
) -> Vec<Vec<Point>> {
    let rects: Vec<SubwayRect> = rects.iter().copied().map(Into::into).collect();
    compute_subway_routes_internal(&rects, connections, options)
}

/// Compute a complete batch and return aggregate routing diagnostics.
pub fn compute_subway_routes_with_stats(
    rects: &[Rect],
    connections: &[SubwayConnection],
    options: &SubwayOptions,
) -> (Vec<Vec<Point>>, SubwayRoutingStats) {
    let rects: Vec<SubwayRect> = rects.iter().copied().map(Into::into).collect();
    compute_subway_routes_with_stats_internal(&rects, connections, options)
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

/// Backwards-compatible single-connection entry point.
pub fn compute_subway_route(
    rects: &[Rect],
    connection: SubwayConnection,
    options: &SubwayOptions,
) -> SubwayRoute {
    let (mut routes, stats) = compute_subway_routes_with_stats(rects, &[connection], options);
    let kind = if stats.grid_fallbacks > 0 {
        RouteKind::GridLimitFallback
    } else if stats.budget_fallbacks > 0 {
        RouteKind::ExpansionLimitFallback
    } else if stats.no_path_fallbacks > 0 {
        RouteKind::NoPathFallback
    } else {
        RouteKind::Routed
    };
    SubwayRoute {
        points: routes.pop().unwrap_or_default(),
        kind,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: f32, y: f32) -> Point {
        Point::new(x, y)
    }

    /// Every waypoint pair is axis-aligned (no diagonals ever leave the router).
    fn assert_orthogonal(route: &[Point]) {
        for w in route.windows(2) {
            let dx = (w[0].x - w[1].x).abs();
            let dy = (w[0].y - w[1].y).abs();
            assert!(
                dx < EPS || dy < EPS,
                "diagonal segment {:?} → {:?}",
                w[0],
                w[1]
            );
        }
    }

    /// True when a segment passes through the rect's interior (not just its edge).
    fn crosses_interior(route: &[Point], r: SubwayRect) -> bool {
        let pad = 1.0;
        route.windows(2).any(|w| {
            let (a, b) = (w[0], w[1]);
            a.x.min(b.x) < r.x + r.w - pad
                && a.x.max(b.x) > r.x + pad
                && a.y.min(b.y) < r.y + r.h - pad
                && a.y.max(b.y) > r.y + pad
        })
    }

    #[test]
    fn routes_are_orthogonal_and_touch_both_anchors() {
        let rects = [
            SubwayRect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 60.0,
            },
            SubwayRect {
                x: 300.0,
                y: 200.0,
                w: 100.0,
                h: 60.0,
            },
        ];
        let conns = [SubwayConnection {
            start: p(100.0, 30.0),
            end: p(300.0, 230.0),
            start_obstacle: Some(0),
            end_obstacle: Some(1),
        }];
        let routes = compute_subway_routes_internal(&rects, &conns, &SubwayOptions::default());
        assert_eq!(routes.len(), 1);
        let r = &routes[0];
        assert!(r.len() >= 2);
        assert_eq!(r[0], conns[0].start);
        assert_eq!(*r.last().unwrap(), conns[0].end);
        assert_orthogonal(r);
    }

    #[test]
    fn route_goes_around_a_blocking_node() {
        // A wall sits directly between the two endpoints, on their shared row.
        let rects = [
            SubwayRect {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 40.0,
            },
            SubwayRect {
                x: 150.0,
                y: -60.0,
                w: 60.0,
                h: 160.0,
            },
            SubwayRect {
                x: 300.0,
                y: 0.0,
                w: 80.0,
                h: 40.0,
            },
        ];
        let conns = [SubwayConnection {
            start: p(80.0, 20.0),
            end: p(300.0, 20.0),
            start_obstacle: Some(0),
            end_obstacle: Some(2),
        }];
        let routes = compute_subway_routes_internal(&rects, &conns, &SubwayOptions::default());
        let r = &routes[0];
        assert_orthogonal(r);
        assert!(
            !crosses_interior(r, rects[1]),
            "route tunnels the wall: {r:?}"
        );
        // Going around costs bends — a straight shot would be 2 points.
        assert!(r.len() > 2, "expected a detour, got {r:?}");
    }

    #[test]
    fn parallel_wires_do_not_share_a_corridor() {
        // Two independent node pairs stacked so their natural mid-x elbows would
        // land on the same vertical corridor.
        let rects = [
            SubwayRect {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 40.0,
            },
            SubwayRect {
                x: 400.0,
                y: 300.0,
                w: 80.0,
                h: 40.0,
            },
            SubwayRect {
                x: 0.0,
                y: 100.0,
                w: 80.0,
                h: 40.0,
            },
            SubwayRect {
                x: 400.0,
                y: 400.0,
                w: 80.0,
                h: 40.0,
            },
        ];
        let conns = [
            SubwayConnection {
                start: p(80.0, 20.0),
                end: p(400.0, 320.0),
                start_obstacle: Some(0),
                end_obstacle: Some(1),
            },
            SubwayConnection {
                start: p(80.0, 120.0),
                end: p(400.0, 420.0),
                start_obstacle: Some(2),
                end_obstacle: Some(3),
            },
        ];
        let routes = compute_subway_routes_internal(&rects, &conns, &SubwayOptions::default());

        // Collect vertical runs of each route and check no pair is collinear
        // AND overlapping — that's the "never run on top of each other" rule.
        let verticals = |r: &Vec<Point>| -> Vec<(f32, f32, f32)> {
            r.windows(2)
                .filter(|w| (w[0].x - w[1].x).abs() < EPS && (w[0].y - w[1].y).abs() >= EPS)
                .map(|w| (w[0].x, w[0].y.min(w[1].y), w[0].y.max(w[1].y)))
                .collect()
        };
        for a in verticals(&routes[0]) {
            for b in verticals(&routes[1]) {
                let same_corridor = (a.0 - b.0).abs() < EPS;
                let overlaps = a.1 < b.2 - EPS && b.1 < a.2 - EPS;
                assert!(
                    !(same_corridor && overlaps),
                    "vertical runs overlap: {a:?} vs {b:?}"
                );
            }
        }
    }

    #[test]
    fn wires_from_one_pin_share_their_trunk() {
        // A fan-out from a single output pin is ONE subway line: the segments
        // leaving the pin must stay on a common column, not fan into lanes.
        let rects = [
            SubwayRect {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 120.0,
            },
            SubwayRect {
                x: 300.0,
                y: 0.0,
                w: 80.0,
                h: 40.0,
            },
            SubwayRect {
                x: 300.0,
                y: 200.0,
                w: 80.0,
                h: 40.0,
            },
        ];
        let start = p(80.0, 60.0);
        let conns = [
            SubwayConnection {
                start,
                end: p(300.0, 20.0),
                start_obstacle: Some(0),
                end_obstacle: Some(1),
            },
            SubwayConnection {
                start,
                end: p(300.0, 220.0),
                start_obstacle: Some(0),
                end_obstacle: Some(2),
            },
        ];
        let routes = compute_subway_routes_internal(&rects, &conns, &SubwayOptions::default());
        for r in &routes {
            assert_eq!(r[0], start, "route must start at the shared pin");
            assert_orthogonal(r);
        }
        // Both leave along the same escape column.
        assert!(
            (routes[0][1].x - routes[1][1].x).abs() < EPS,
            "trunk split: {:?} vs {:?}",
            routes[0][1],
            routes[1][1]
        );
    }

    #[test]
    fn missing_rects_still_route() {
        // Endpoints with unknown/unmeasured nodes fall back to plain stubs.
        let conns = [SubwayConnection {
            start: p(0.0, 0.0),
            end: p(200.0, 100.0),
            start_obstacle: None,
            end_obstacle: None,
        }];
        let routes = compute_subway_routes_internal(&[], &conns, &SubwayOptions::default());
        assert_eq!(routes.len(), 1);
        assert_orthogonal(&routes[0]);
        assert_eq!(routes[0][0], conns[0].start);
        assert_eq!(*routes[0].last().unwrap(), conns[0].end);
    }

    #[test]
    fn solving_is_deterministic() {
        let rects = [
            SubwayRect {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 40.0,
            },
            SubwayRect {
                x: 200.0,
                y: 90.0,
                w: 80.0,
                h: 40.0,
            },
            SubwayRect {
                x: 120.0,
                y: -40.0,
                w: 40.0,
                h: 200.0,
            },
        ];
        let conns = [
            SubwayConnection {
                start: p(80.0, 20.0),
                end: p(200.0, 110.0),
                start_obstacle: Some(0),
                end_obstacle: Some(1),
            },
            SubwayConnection {
                start: p(80.0, 30.0),
                end: p(200.0, 100.0),
                start_obstacle: Some(0),
                end_obstacle: Some(1),
            },
        ];
        let a = compute_subway_routes_internal(&rects, &conns, &SubwayOptions::default());
        let b = compute_subway_routes_internal(&rects, &conns, &SubwayOptions::default());
        assert_eq!(a, b);
    }

    #[test]
    fn simplify_drops_duplicates_and_collinear_points() {
        let pts = [
            p(0.0, 0.0),
            p(0.0, 0.0),
            p(10.0, 0.0),
            p(20.0, 0.0),
            p(20.0, 10.0),
        ];
        assert_eq!(
            simplify(&pts),
            vec![p(0.0, 0.0), p(20.0, 0.0), p(20.0, 10.0)]
        );
    }

    #[test]
    fn route_intersects_rect_detects_near_misses() {
        let route = [p(0.0, 0.0), p(100.0, 0.0), p(100.0, 100.0)];
        let far = SubwayRect {
            x: 200.0,
            y: 200.0,
            w: 10.0,
            h: 10.0,
        };
        let near = SubwayRect {
            x: 50.0,
            y: 5.0,
            w: 10.0,
            h: 10.0,
        };
        assert!(!route_intersects_subway_rect(&route, far, 4.0));
        assert!(route_intersects_subway_rect(&route, near, 8.0));
    }

    #[test]
    fn aggregate_nesting_order_resolves_pairwise_preference_cycle() {
        let segment = |route, lo, hi, lo_dir, hi_dir| LaneSegment {
            route,
            i: 0,
            coord: 10.0,
            lo,
            hi,
            pinned: false,
            sort_key: 0.0,
            lo_dir,
            hi_dir,
        };
        // These three brackets vote A < B, B < C, and C < A. Feeding those
        // votes directly to sort_by was the non-transitive live-WASM panic.
        let a = segment(0, 1.0, 3.0, -1.0, 1.0);
        let b = segment(1, 0.0, 3.0, -1.0, -1.0);
        let c = segment(2, 1.0, 4.0, 0.0, -1.0);
        assert!(lane_nesting_preference(&a, &b) < 0.0);
        assert!(lane_nesting_preference(&b, &c) < 0.0);
        assert!(lane_nesting_preference(&c, &a) < 0.0);

        let permutations = [
            vec![a, b, c],
            vec![a, c, b],
            vec![b, a, c],
            vec![b, c, a],
            vec![c, a, b],
            vec![c, b, a],
        ];
        for representatives in permutations {
            let scores = aggregate_nesting_scores(&representatives);
            let mut ranked: Vec<_> = representatives.into_iter().zip(scores).collect();
            ranked.sort_by(|(a, a_score), (b, b_score)| {
                compare_lane_order(
                    LaneOrderKey {
                        nesting_score: *a_score,
                        sort_key: a.sort_key,
                        coord: a.coord,
                        stable_key: (a.route, a.i),
                    },
                    LaneOrderKey {
                        nesting_score: *b_score,
                        sort_key: b.sort_key,
                        coord: b.coord,
                        stable_key: (b.route, b.i),
                    },
                )
            });
            assert_eq!(
                ranked
                    .into_iter()
                    .map(|(segment, _)| segment.route)
                    .collect::<Vec<_>>(),
                vec![0, 1, 2]
            );
        }
    }

    #[test]
    fn oversized_grid_falls_back_to_elbows() {
        let rects = [
            SubwayRect {
                x: 0.0,
                y: 0.0,
                w: 80.0,
                h: 40.0,
            },
            SubwayRect {
                x: 300.0,
                y: 0.0,
                w: 80.0,
                h: 40.0,
            },
        ];
        let conns = [SubwayConnection {
            start: p(80.0, 20.0),
            end: p(300.0, 20.0),
            start_obstacle: Some(0),
            end_obstacle: Some(1),
        }];
        let opts = SubwayOptions {
            max_grid_cells: 1,
            ..Default::default()
        };
        let routes = compute_subway_routes_internal(&rects, &conns, &opts);
        assert_orthogonal(&routes[0]);
        assert_eq!(routes[0][0], conns[0].start);
        assert_eq!(*routes[0].last().unwrap(), conns[0].end);
    }

    #[test]
    fn dense_public_batch_is_deterministic_and_orthogonal() {
        let rects = [Rect {
            origin: Point::new(140.0, -40.0),
            size: crate::Size {
                width: 80.0,
                height: 320.0,
            },
        }];
        let connections: Vec<_> = (0..24)
            .map(|i| SubwayConnection {
                start: Point::new(0.0, i as f32 * 10.0),
                end: Point::new(360.0, (23 - i) as f32 * 10.0),
                start_obstacle: None,
                end_obstacle: None,
            })
            .collect();
        let options = SubwayOptions {
            lane_gap: 5.0,
            crossing_penalty: 20.0,
            overlap_factor: 0.25,
            max_total_expansions: 200_000,
            ..SubwayOptions::default()
        };
        let first = compute_subway_routes(&rects, &connections, &options);
        let second = compute_subway_routes(&rects, &connections, &options);
        assert_eq!(first, second);
        assert_eq!(first.len(), connections.len());
        for (route, connection) in first.iter().zip(&connections) {
            assert_eq!(route.first(), Some(&connection.start));
            assert_eq!(route.last(), Some(&connection.end));
            assert_orthogonal(route);
            assert!(!route_intersects_rect(route, rects[0], 0.0));
        }
    }

    #[test]
    fn dense_batch_obeys_one_total_expansion_budget() {
        let connections: Vec<_> = (0..32)
            .map(|i| {
                SubwayConnection::new(
                    Point::new(0.0, i as f32 * 7.0),
                    Point::new(500.0, (31 - i) as f32 * 7.0),
                )
            })
            .collect();
        let options = SubwayOptions {
            max_expansions: 10_000,
            max_total_expansions: 48,
            lane_gap: 4.0,
            crossing_penalty: 12.0,
            overlap_factor: -0.1,
            ..SubwayOptions::default()
        };
        let (routes, stats) = compute_subway_routes_with_stats(&[], &connections, &options);
        assert_eq!(routes.len(), connections.len());
        assert!(stats.expansions <= options.max_total_expansions);
        assert!(stats.budget_fallbacks > 0);
        assert_eq!(stats.routed + stats.fallbacks(), connections.len());
    }
}
