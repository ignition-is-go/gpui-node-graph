//! Deterministic world-space drawing commands for node bodies.
//!
//! This module intentionally contains no GPUI types.  A renderer can lay a node
//! out once in world units, then project the resulting scene immediately before
//! painting.  In particular, [`TextLines`] records line breaks rather than a
//! wrapping constraint, so changing the viewport cannot cause text reflow.

use node_graph_core as core;
use std::sync::Arc;

/// A renderer-independent sRGB color.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorldColor {
    /// The low 24 bits are `0xRRGGBB`.
    pub rgb: u32,
    pub alpha: f32,
}

impl WorldColor {
    pub const TRANSPARENT: Self = Self::rgba(0, 0.0);

    pub const fn rgb(rgb: u32) -> Self {
        Self {
            rgb: rgb & 0x00ff_ffff,
            alpha: 1.0,
        }
    }

    pub const fn rgba(rgb: u32, alpha: f32) -> Self {
        Self {
            rgb: rgb & 0x00ff_ffff,
            alpha,
        }
    }
}

/// Explicit, immutable-for-projection text lines.
///
/// `from_text` recognizes newlines exactly once.  Projection never measures or
/// wraps these strings.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextLines(Vec<String>);

impl TextLines {
    pub fn new(lines: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self(lines.into_iter().map(Into::into).collect())
    }

    pub fn from_text(text: impl AsRef<str>) -> Self {
        // `split` deliberately preserves a trailing empty line.
        Self::new(text.as_ref().split('\n'))
    }

    pub fn as_slice(&self) -> &[String] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<const N: usize> From<[&str; N]> for TextLines {
    fn from(lines: [&str; N]) -> Self {
        Self::new(lines)
    }
}

/// A paint command whose every length is expressed in world units.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldPrimitive {
    Quad {
        bounds: core::Rect,
        fill: WorldColor,
        corner_radius: f32,
    },
    BorderedQuad {
        bounds: core::Rect,
        fill: WorldColor,
        border: WorldColor,
        border_width: f32,
        corner_radius: f32,
    },
    Line {
        start: core::Point,
        end: core::Point,
        color: WorldColor,
        width: f32,
    },
    Text {
        origin: core::Point,
        lines: TextLines,
        color: WorldColor,
        font_size: f32,
        font_weight: u16,
        line_height: f32,
    },
    Circle {
        center: core::Point,
        radius: f32,
        fill: WorldColor,
    },
    Polygon {
        points: Arc<[core::Point]>,
        fill: WorldColor,
    },
}

/// Semantic purpose of an inverse hit region.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HitRole {
    NodeBody,
    Port,
    Control,
    Custom(String),
}

/// World-space geometry used for hit testing.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HitShape {
    Rect(core::Rect),
    Circle { center: core::Point, radius: f32 },
}

impl HitShape {
    pub fn contains(self, point: core::Point) -> bool {
        match self {
            Self::Rect(bounds) => bounds.contains(point),
            Self::Circle { center, radius } => radius >= 0.0 && center.distance(point) <= radius,
        }
    }

    pub fn project(self, transform: Transform) -> ScreenHitShape {
        match self {
            Self::Rect(bounds) => ScreenHitShape::Rect(transform.rect(bounds)),
            Self::Circle { center, radius } => ScreenHitShape::Circle {
                center: transform.point(center),
                radius: transform.length(radius),
            },
        }
    }
}

/// Semantic role exposed when a control hit region is projected into AccessKit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AccessibleControlRole {
    #[default]
    Button,
    TextInput,
    ComboBox,
    Slider,
    SpinButton,
}

/// An id-bearing interactive area, stored independently of paint commands.
#[derive(Clone, Debug, PartialEq)]
pub struct WorldHitRegion {
    pub id: String,
    pub role: HitRole,
    pub shape: HitShape,
    /// Human-readable name for projected semantic controls.
    pub accessible_label: Option<String>,
    pub accessible_role: AccessibleControlRole,
    pub accessible_value: Option<String>,
    pub accessible_numeric_value: Option<f64>,
    pub accessible_min_numeric_value: Option<f64>,
    pub accessible_max_numeric_value: Option<f64>,
    pub accessible_numeric_value_step: Option<f64>,
}

impl WorldHitRegion {
    pub fn new(id: impl Into<String>, role: HitRole, shape: HitShape) -> Self {
        Self {
            id: id.into(),
            role,
            shape,
            accessible_label: None,
            accessible_role: AccessibleControlRole::Button,
            accessible_value: None,
            accessible_numeric_value: None,
            accessible_min_numeric_value: None,
            accessible_max_numeric_value: None,
            accessible_numeric_value_step: None,
        }
    }

    pub fn with_accessible_label(mut self, label: impl Into<String>) -> Self {
        self.accessible_label = Some(label.into());
        self
    }

    pub fn with_accessible_role(mut self, role: AccessibleControlRole) -> Self {
        self.accessible_role = role;
        self
    }

    pub fn with_accessible_value(mut self, value: impl Into<String>) -> Self {
        self.accessible_value = Some(value.into());
        self
    }

    pub fn with_accessible_numeric_range(
        mut self,
        value: f64,
        minimum: f64,
        maximum: f64,
        step: f64,
    ) -> Self {
        self.accessible_numeric_value = Some(value);
        self.accessible_min_numeric_value = Some(minimum);
        self.accessible_max_numeric_value = Some(maximum);
        self.accessible_numeric_value_step = Some(step);
        self
    }
}

/// An ordered world-space display list and ordered hit list.
///
/// Later hit regions are considered visually above earlier ones.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldScene {
    pub primitives: Vec<WorldPrimitive>,
    pub hit_regions: Vec<WorldHitRegion>,
}

impl WorldScene {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, primitive: WorldPrimitive) {
        self.primitives.push(primitive);
    }

    pub fn push_hit_region(&mut self, region: WorldHitRegion) {
        self.hit_regions.push(region);
    }

    pub fn with_primitive(mut self, primitive: WorldPrimitive) -> Self {
        self.push(primitive);
        self
    }

    pub fn with_hit_region(mut self, region: WorldHitRegion) -> Self {
        self.push_hit_region(region);
        self
    }

    /// Return the topmost region containing a world point.
    pub fn hit_test(&self, world: core::Point) -> Option<&WorldHitRegion> {
        self.hit_regions
            .iter()
            .rev()
            .find(|region| region.shape.contains(world))
    }

    /// Inverse-project a screen point and return the topmost matching region.
    pub fn hit_test_screen(
        &self,
        screen: core::Point,
        transform: impl Into<Transform>,
    ) -> Option<&WorldHitRegion> {
        self.hit_test(transform.into().inverse_point(screen))
    }

    /// Project without modifying the world display list or its fixed text lines.
    pub fn project(&self, transform: impl Into<Transform>) -> ScreenScene {
        let transform = transform.into();
        ScreenScene {
            primitives: self
                .primitives
                .iter()
                .map(|p| p.project(transform))
                .collect(),
            hit_regions: self
                .hit_regions
                .iter()
                .map(|region| ScreenHitRegion {
                    id: region.id.clone(),
                    role: region.role.clone(),
                    shape: region.shape.project(transform),
                    accessible_label: region.accessible_label.clone(),
                    accessible_role: region.accessible_role,
                    accessible_value: region.accessible_value.clone(),
                    accessible_numeric_value: region.accessible_numeric_value,
                    accessible_min_numeric_value: region.accessible_min_numeric_value,
                    accessible_max_numeric_value: region.accessible_max_numeric_value,
                    accessible_numeric_value_step: region.accessible_numeric_value_step,
                })
                .collect(),
        }
    }
}

/// A small renderer-neutral adapter around [`core::Viewport`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub viewport: core::Viewport,
}

impl Transform {
    pub const fn new(pan: core::Point, zoom: f32) -> Self {
        Self {
            viewport: core::Viewport { pan, zoom },
        }
    }

    pub fn point(self, world: core::Point) -> core::Point {
        self.viewport.world_to_screen(world)
    }

    pub fn inverse_point(self, screen: core::Point) -> core::Point {
        self.viewport.screen_to_world(screen)
    }

    pub fn length(self, world: f32) -> f32 {
        self.viewport.scale_length(world)
    }

    pub fn rect(self, world: core::Rect) -> core::Rect {
        core::Rect {
            origin: self.point(world.origin),
            size: core::Size {
                width: self.length(world.size.width),
                height: self.length(world.size.height),
            },
        }
    }
}

impl From<core::Viewport> for Transform {
    fn from(viewport: core::Viewport) -> Self {
        Self { viewport }
    }
}

impl From<&core::Viewport> for Transform {
    fn from(viewport: &core::Viewport) -> Self {
        Self {
            viewport: *viewport,
        }
    }
}

impl From<&Transform> for Transform {
    fn from(transform: &Transform) -> Self {
        *transform
    }
}

impl WorldPrimitive {
    pub fn project(&self, transform: Transform) -> ScreenPrimitive {
        match self {
            Self::Quad {
                bounds,
                fill,
                corner_radius,
            } => ScreenPrimitive::Quad {
                bounds: transform.rect(*bounds),
                fill: *fill,
                corner_radius: transform.length(*corner_radius),
            },
            Self::BorderedQuad {
                bounds,
                fill,
                border,
                border_width,
                corner_radius,
            } => ScreenPrimitive::BorderedQuad {
                bounds: transform.rect(*bounds),
                fill: *fill,
                border: *border,
                border_width: transform.length(*border_width),
                corner_radius: transform.length(*corner_radius),
            },
            Self::Line {
                start,
                end,
                color,
                width,
            } => ScreenPrimitive::Line {
                start: transform.point(*start),
                end: transform.point(*end),
                color: *color,
                width: transform.length(*width),
            },
            Self::Text {
                origin,
                lines,
                color,
                font_size,
                font_weight,
                line_height,
            } => ScreenPrimitive::Text {
                origin: transform.point(*origin),
                lines: lines.clone(),
                color: *color,
                font_size: transform.length(*font_size),
                font_weight: *font_weight,
                line_height: transform.length(*line_height),
            },
            Self::Circle {
                center,
                radius,
                fill,
            } => ScreenPrimitive::Circle {
                center: transform.point(*center),
                radius: transform.length(*radius),
                fill: *fill,
            },
            Self::Polygon { points, fill } => ScreenPrimitive::Polygon {
                points: points
                    .iter()
                    .map(|point| transform.point(*point))
                    .collect::<Vec<_>>()
                    .into(),
                fill: *fill,
            },
        }
    }
}

/// A projected paint command.  Its geometry is in screen pixels.
#[derive(Clone, Debug, PartialEq)]
pub enum ScreenPrimitive {
    Quad {
        bounds: core::Rect,
        fill: WorldColor,
        corner_radius: f32,
    },
    BorderedQuad {
        bounds: core::Rect,
        fill: WorldColor,
        border: WorldColor,
        border_width: f32,
        corner_radius: f32,
    },
    Line {
        start: core::Point,
        end: core::Point,
        color: WorldColor,
        width: f32,
    },
    Text {
        origin: core::Point,
        lines: TextLines,
        color: WorldColor,
        font_size: f32,
        font_weight: u16,
        line_height: f32,
    },
    Circle {
        center: core::Point,
        radius: f32,
        fill: WorldColor,
    },
    Polygon {
        points: Arc<[core::Point]>,
        fill: WorldColor,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScreenHitShape {
    Rect(core::Rect),
    Circle { center: core::Point, radius: f32 },
}

impl ScreenHitShape {
    pub fn contains(self, point: core::Point) -> bool {
        match self {
            Self::Rect(bounds) => bounds.contains(point),
            Self::Circle { center, radius } => radius >= 0.0 && center.distance(point) <= radius,
        }
    }

    pub fn bounds(self) -> core::Rect {
        match self {
            Self::Rect(bounds) => bounds,
            Self::Circle { center, radius } => core::Rect {
                origin: core::Point::new(center.x - radius, center.y - radius),
                size: core::Size {
                    width: radius * 2.0,
                    height: radius * 2.0,
                },
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenHitRegion {
    pub id: String,
    pub role: HitRole,
    pub shape: ScreenHitShape,
    pub accessible_label: Option<String>,
    pub accessible_role: AccessibleControlRole,
    pub accessible_value: Option<String>,
    pub accessible_numeric_value: Option<f64>,
    pub accessible_min_numeric_value: Option<f64>,
    pub accessible_max_numeric_value: Option<f64>,
    pub accessible_numeric_value_step: Option<f64>,
}

/// The result of projecting a [`WorldScene`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScreenScene {
    pub primitives: Vec<ScreenPrimitive>,
    pub hit_regions: Vec<ScreenHitRegion>,
}

impl ScreenScene {
    pub fn hit_test(&self, screen: core::Point) -> Option<&ScreenHitRegion> {
        self.hit_regions
            .iter()
            .rev()
            .find(|region| region.shape.contains(screen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZOOMS: [f32; 6] = [0.1, 0.740818, 1.0, 1.349859, 2.0, 5.0];

    fn p(x: f32, y: f32) -> core::Point {
        core::Point::new(x, y)
    }
    fn rect(x: f32, y: f32, width: f32, height: f32) -> core::Rect {
        core::Rect {
            origin: p(x, y),
            size: core::Size { width, height },
        }
    }
    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 0.0002, "{actual} != {expected}");
    }
    fn close_point(actual: core::Point, expected: core::Point) {
        close(actual.x, expected.x);
        close(actual.y, expected.y);
    }

    fn scene() -> WorldScene {
        WorldScene::new()
            .with_primitive(WorldPrimitive::Quad {
                bounds: rect(10.0, 20.0, 120.0, 80.0),
                fill: WorldColor::rgb(0x112233),
                corner_radius: 8.0,
            })
            .with_primitive(WorldPrimitive::Line {
                start: p(15.0, 40.0),
                end: p(125.0, 40.0),
                color: WorldColor::rgb(0xffffff),
                width: 2.0,
            })
            .with_primitive(WorldPrimitive::Text {
                origin: p(20.0, 50.0),
                lines: TextLines::from_text("first line\nsecond line\n"),
                color: WorldColor::rgb(0xabcdef),
                font_size: 12.0,
                font_weight: 400,
                line_height: 16.0,
            })
            .with_primitive(WorldPrimitive::Circle {
                center: p(118.0, 82.0),
                radius: 5.0,
                fill: WorldColor::rgb(0xff00ff),
            })
            .with_hit_region(WorldHitRegion::new(
                "body",
                HitRole::NodeBody,
                HitShape::Rect(rect(10.0, 20.0, 120.0, 80.0)),
            ))
            .with_hit_region(WorldHitRegion::new(
                "port:a",
                HitRole::Port,
                HitShape::Circle {
                    center: p(118.0, 82.0),
                    radius: 7.0,
                },
            ))
    }

    #[test]
    fn fixed_lines_and_world_layout_are_identical_at_every_zoom() {
        let world = scene();
        let original = world.clone();
        for zoom in ZOOMS {
            let projected = world.project(Transform::new(p(37.0, -19.0), zoom));
            assert_eq!(world, original, "projection at {zoom} mutated layout");
            assert_eq!(projected.primitives.len(), world.primitives.len());
            match (&world.primitives[2], &projected.primitives[2]) {
                (
                    WorldPrimitive::Text {
                        lines: world_lines, ..
                    },
                    ScreenPrimitive::Text {
                        lines: screen_lines,
                        ..
                    },
                ) => {
                    assert_eq!(world_lines.as_slice(), ["first line", "second line", ""]);
                    assert_eq!(screen_lines, world_lines, "line breaks changed at {zoom}");
                }
                _ => panic!("text command changed variant"),
            }
        }
    }

    #[test]
    fn every_paint_coordinate_scales_and_pans() {
        let world = scene();
        let pan = p(37.0, -19.0);
        for zoom in ZOOMS {
            let screen = world.project(Transform::new(pan, zoom));
            match &screen.primitives[0] {
                ScreenPrimitive::Quad {
                    bounds,
                    corner_radius,
                    ..
                } => {
                    close_point(bounds.origin, p(10.0 * zoom + pan.x, 20.0 * zoom + pan.y));
                    close(bounds.size.width, 120.0 * zoom);
                    close(bounds.size.height, 80.0 * zoom);
                    close(*corner_radius, 8.0 * zoom);
                }
                _ => panic!(),
            }
            match &screen.primitives[1] {
                ScreenPrimitive::Line {
                    start, end, width, ..
                } => {
                    close_point(*start, p(15.0 * zoom + pan.x, 40.0 * zoom + pan.y));
                    close_point(*end, p(125.0 * zoom + pan.x, 40.0 * zoom + pan.y));
                    close(*width, 2.0 * zoom);
                }
                _ => panic!(),
            }
            match &screen.primitives[2] {
                ScreenPrimitive::Text {
                    origin,
                    font_size,
                    line_height,
                    ..
                } => {
                    close_point(*origin, p(20.0 * zoom + pan.x, 50.0 * zoom + pan.y));
                    close(*font_size, 12.0 * zoom);
                    close(*line_height, 16.0 * zoom);
                }
                _ => panic!(),
            }
            match &screen.primitives[3] {
                ScreenPrimitive::Circle { center, radius, .. } => {
                    close_point(*center, p(118.0 * zoom + pan.x, 82.0 * zoom + pan.y));
                    close(*radius, 5.0 * zoom);
                }
                _ => panic!(),
            }
        }
    }

    #[test]
    fn projected_and_inverse_hits_agree_at_every_zoom_and_pan() {
        let world = scene();
        for zoom in ZOOMS {
            let transform = Transform::new(p(-53.25, 91.5), zoom);
            let port_screen = transform.point(p(118.0, 82.0));
            let body_screen = transform.point(p(30.0, 30.0));
            assert_eq!(
                world.hit_test_screen(port_screen, transform).unwrap().id,
                "port:a"
            );
            assert_eq!(
                world.hit_test_screen(body_screen, transform).unwrap().id,
                "body"
            );

            let projected = world.project(transform);
            assert_eq!(projected.hit_test(port_screen).unwrap().id, "port:a");
            assert_eq!(projected.hit_test(body_screen).unwrap().id, "body");
            close_point(transform.inverse_point(port_screen), p(118.0, 82.0));

            let outside = transform.point(p(200.0, 200.0));
            assert!(world.hit_test_screen(outside, transform).is_none());
            assert!(projected.hit_test(outside).is_none());
        }
    }

    #[test]
    fn projection_accepts_core_viewport_directly() {
        for zoom in ZOOMS {
            let viewport = core::Viewport {
                pan: p(4.0, 9.0),
                zoom,
            };
            let screen = scene().project(viewport);
            match &screen.primitives[3] {
                ScreenPrimitive::Circle { center, radius, .. } => {
                    close_point(*center, viewport.world_to_screen(p(118.0, 82.0)));
                    close(*radius, viewport.scale_length(5.0));
                }
                _ => panic!(),
            }
        }
    }

    #[test]
    fn polygon_projection_scales_every_authored_vertex() {
        let points: Arc<[core::Point]> = vec![p(1.0, 2.0), p(4.0, 2.0), p(2.5, 5.0)].into();
        let primitive = WorldPrimitive::Polygon {
            points: points.clone(),
            fill: WorldColor::rgb(0xff00ff),
        };
        let transform = Transform::new(p(7.0, -3.0), 1.349859);
        let ScreenPrimitive::Polygon { points: screen, .. } = primitive.project(transform) else {
            panic!();
        };
        for (actual, expected) in screen.iter().zip(points.iter()) {
            close_point(*actual, transform.point(*expected));
        }
    }

    #[test]
    fn semantic_labels_and_bounds_survive_projection() {
        let projected = WorldScene::new()
            .with_hit_region(
                WorldHitRegion::new(
                    "button",
                    HitRole::Control,
                    HitShape::Circle {
                        center: p(5.0, 7.0),
                        radius: 2.0,
                    },
                )
                .with_accessible_label("Mix")
                .with_accessible_role(AccessibleControlRole::Slider)
                .with_accessible_value("75 percent")
                .with_accessible_numeric_range(0.75, 0.0, 1.0, 0.05),
            )
            .project(Transform::new(p(0.25, -0.5), 1.349859));
        let region = &projected.hit_regions[0];
        assert_eq!(region.accessible_label.as_deref(), Some("Mix"));
        assert_eq!(region.accessible_role, AccessibleControlRole::Slider);
        assert_eq!(region.accessible_value.as_deref(), Some("75 percent"));
        assert_eq!(region.accessible_numeric_value, Some(0.75));
        assert_eq!(region.accessible_min_numeric_value, Some(0.0));
        assert_eq!(region.accessible_max_numeric_value, Some(1.0));
        assert_eq!(region.accessible_numeric_value_step, Some(0.05));
        let bounds = region.shape.bounds();
        close_point(bounds.origin, p(4.299577, 6.249295));
        close(bounds.size.width, 5.399436);
        close(bounds.size.height, 5.399436);
    }

    #[test]
    fn hit_order_is_stable_and_topmost_wins() {
        let world = scene();
        assert_eq!(world.hit_test(p(118.0, 82.0)).unwrap().id, "port:a");
        assert_eq!(
            world
                .hit_regions
                .iter()
                .map(|r| r.id.as_str())
                .collect::<Vec<_>>(),
            ["body", "port:a"]
        );
    }
}
