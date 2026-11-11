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

/// A single-line, clipped text field in the coordinate plane of its primitive.
/// Text offsets use UTF-16 code units, matching native/web platform input.
#[derive(Clone, Debug, PartialEq)]
pub struct TextField {
    pub id: String,
    pub bounds: core::Rect,
    pub text: String,
    pub selection: std::ops::Range<usize>,
    pub selection_reversed: bool,
    pub marked: Option<std::ops::Range<usize>>,
    pub font_family: Option<String>,
    pub font_size: f32,
    pub font_weight: u16,
    pub line_height: f32,
    pub padding_x: f32,
    pub caret_width: f32,
    pub color: WorldColor,
    pub selection_color: WorldColor,
    pub caret_color: WorldColor,
    pub align_right: bool,
    pub focused: bool,
}

impl TextField {
    pub fn new(id: impl Into<String>, bounds: core::Rect, text: impl Into<String>) -> Self {
        let text = text.into();
        let end = text.encode_utf16().count();
        Self {
            id: id.into(),
            bounds,
            text,
            selection: end..end,
            selection_reversed: false,
            marked: None,
            font_family: None,
            font_size: 11.0,
            font_weight: 400,
            line_height: 16.0,
            padding_x: 4.0,
            caret_width: 1.0,
            color: WorldColor::rgb(0xeeeeee),
            selection_color: WorldColor::rgba(0x00bfc4, 0.3),
            caret_color: WorldColor::rgb(0xeeeeee),
            align_right: false,
            focused: true,
        }
    }

    fn project(&self, transform: Transform) -> Self {
        Self {
            bounds: transform.rect(self.bounds),
            font_size: transform.length(self.font_size),
            line_height: transform.length(self.line_height),
            padding_x: transform.length(self.padding_x),
            caret_width: transform.length(self.caret_width),
            ..self.clone()
        }
    }
}

/// A single-line display label with trailing ellipsis and no editing state.
#[derive(Clone, Debug, PartialEq)]
pub struct TextLabel {
    pub bounds: core::Rect,
    pub text: String,
    pub font_family: Option<String>,
    pub font_size: f32,
    pub font_weight: u16,
    pub italic: bool,
    pub line_height: f32,
    pub color: WorldColor,
    pub align_right: bool,
}

impl TextLabel {
    pub fn new(bounds: core::Rect, text: impl Into<String>) -> Self {
        Self {
            bounds,
            text: text.into(),
            font_family: None,
            font_size: 11.0,
            font_weight: 400,
            italic: false,
            line_height: 14.0,
            color: WorldColor::rgb(0xeeeeee),
            align_right: false,
        }
    }

    fn project(&self, transform: Transform) -> Self {
        Self {
            bounds: transform.rect(self.bounds),
            font_size: transform.length(self.font_size),
            line_height: transform.length(self.line_height),
            ..self.clone()
        }
    }
}

/// A label/value row whose value takes its natural width before its label.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRow {
    pub bounds: core::Rect,
    pub label: String,
    pub value: String,
    pub label_color: WorldColor,
    pub value_color: WorldColor,
    pub font_family: Option<String>,
    pub font_size: f32,
    pub font_weight: u16,
    pub line_height: f32,
    pub gap: f32,
}

impl TextRow {
    pub fn new(bounds: core::Rect, label: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            bounds,
            label: label.into(),
            value: value.into(),
            label_color: WorldColor::rgb(0xbebebe),
            value_color: WorldColor::rgb(0xf2f2f2),
            font_family: None,
            font_size: 11.0,
            font_weight: 400,
            line_height: 14.0,
            gap: 6.0,
        }
    }

    fn project(&self, transform: Transform) -> Self {
        Self {
            bounds: transform.rect(self.bounds),
            font_size: transform.length(self.font_size),
            line_height: transform.length(self.line_height),
            gap: transform.length(self.gap),
            ..self.clone()
        }
    }
}

/// An independently styled span in an inline text pair.
#[derive(Clone, Debug, PartialEq)]
pub struct TextSpan {
    pub text: String,
    pub font_family: Option<String>,
    pub font_size: f32,
    pub font_weight: u16,
    pub italic: bool,
    pub line_height: f32,
    pub color: WorldColor,
}

impl TextSpan {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            font_family: None,
            font_size: 11.0,
            font_weight: 400,
            italic: false,
            line_height: 14.0,
            color: WorldColor::rgb(0xeeeeee),
        }
    }

    fn project(&self, transform: Transform) -> Self {
        Self {
            font_size: transform.length(self.font_size),
            line_height: transform.length(self.line_height),
            ..self.clone()
        }
    }
}

/// Baseline-aligned adjacent text. The leading span ellipsizes to preserve the
/// trailing span's natural width. Layout bounds and the paint clip are independent.
#[derive(Clone, Debug, PartialEq)]
pub struct InlineText {
    pub bounds: core::Rect,
    pub clip: Option<core::Rect>,
    pub leading: TextSpan,
    pub trailing: TextSpan,
    pub gap: f32,
}

impl InlineText {
    pub fn new(bounds: core::Rect, leading: TextSpan, trailing: TextSpan) -> Self {
        Self {
            bounds,
            clip: Some(bounds),
            leading,
            trailing,
            gap: 6.0,
        }
    }

    fn project(&self, transform: Transform) -> Self {
        Self {
            bounds: transform.rect(self.bounds),
            clip: self.clip.map(|clip| transform.rect(clip)),
            leading: self.leading.project(transform),
            trailing: self.trailing.project(transform),
            gap: transform.length(self.gap),
        }
    }
}

/// Filled vector commands expressed in the path's local coordinate system.
#[derive(Clone, Debug, PartialEq)]
pub enum VectorCommand {
    Move(core::Point),
    Line(core::Point),
    Cubic {
        control_a: core::Point,
        control_b: core::Point,
        end: core::Point,
    },
    Arc {
        radii: core::Point,
        rotation: f32,
        large: bool,
        sweep: bool,
        end: core::Point,
    },
    Close,
}

/// Unquantized, encoded sRGB channels and alpha at a normalized ramp position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GradientStop {
    pub position: f32,
    pub rgba: [f32; 4],
}

impl GradientStop {
    /// Reject nonfinite source channels before color-space conversion can hide
    /// them. Finite position and RGBA components clamp to `0..=1`.
    pub fn normalized(self) -> Option<Self> {
        (self.position.is_finite() && self.rgba.into_iter().all(f32::is_finite)).then(|| Self {
            position: self.position.clamp(0.0, 1.0),
            rgba: self.rgba.map(|channel| channel.clamp(0.0, 1.0)),
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerRadii {
    pub top_left: f32,
    pub top_right: f32,
    pub bottom_right: f32,
    pub bottom_left: f32,
}

impl CornerRadii {
    pub fn uniform(radius: f32) -> Self {
        Self {
            top_left: radius,
            top_right: radius,
            bottom_right: radius,
            bottom_left: radius,
        }
    }

    fn project(self, transform: Transform) -> Self {
        Self {
            top_left: transform.length(self.top_left),
            top_right: transform.length(self.top_right),
            bottom_right: transform.length(self.bottom_right),
            bottom_left: transform.length(self.bottom_left),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BorderWidths {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl BorderWidths {
    pub fn uniform(width: f32) -> Self {
        Self {
            top: width,
            right: width,
            bottom: width,
            left: width,
        }
    }

    fn project(self, transform: Transform) -> Self {
        Self {
            top: transform.length(self.top),
            right: transform.length(self.right),
            bottom: transform.length(self.bottom),
            left: transform.length(self.left),
        }
    }
}

/// A framed horizontal ramp with premultiplied sRGB interpolation.
/// Stops remain shared and unquantized when its geometry is projected.
#[derive(Clone, Debug, PartialEq)]
pub struct GradientQuad {
    pub bounds: core::Rect,
    pub stops: Arc<[GradientStop]>,
    pub corner_radii: CornerRadii,
    pub border_widths: BorderWidths,
    pub border: WorldColor,
}

impl GradientQuad {
    fn project(&self, transform: Transform) -> Self {
        Self {
            bounds: transform.rect(self.bounds),
            stops: self.stops.clone(),
            corner_radii: self.corner_radii.project(transform),
            border_widths: self.border_widths.project(transform),
            border: self.border,
        }
    }
}

/// A paint command positioned in world space.
#[derive(Clone, Debug, PartialEq)]
pub enum WorldPrimitive {
    VectorPath {
        origin: core::Point,
        scale: f32,
        commands: Arc<[VectorCommand]>,
        fill: WorldColor,
    },
    InlineText(InlineText),
    TextRow(TextRow),
    TextLabel(TextLabel),
    TextInput(TextField),
    HorizontalGradient(GradientQuad),
    Quad {
        bounds: core::Rect,
        fill: WorldColor,
        corner_radius: f32,
    },
    Checkerboard {
        bounds: core::Rect,
        base: WorldColor,
        alternate: WorldColor,
        foreground: WorldColor,
        cell_size: f32,
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
    /// Consume a primary press and blur the old control without activating or focusing another one.
    PointerBarrier,
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

/// Pointer activation for controls sharing a node's selection and drag surface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PointerActivation {
    #[default]
    Press,
    DoubleClick,
}

impl PointerActivation {
    pub fn accepts(self, click_count: usize) -> bool {
        self == Self::Press || click_count >= 2
    }
}

/// How an interactive area handles the middle mouse button.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MiddlePointerBehavior {
    #[default]
    PassThrough,
    Capture,
    Block,
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
    /// Opt in to middle-button gestures instead of canvas panning/inspection.
    pub middle_pointer: MiddlePointerBehavior,
    pub pointer_activation: PointerActivation,
    pub context_menu: bool,
    /// Optional world-space restrictions within the primary hit shape.
    pub context_menu_hit_shapes: Option<Arc<[HitShape]>>,
    /// Preserve existing overlays when this target consumes a pointer press.
    pub preserves_overlays: bool,
    /// Consume right/middle presses without changing focus or starting graph gestures.
    pub auxiliary_pointer_barrier: bool,
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
            middle_pointer: MiddlePointerBehavior::PassThrough,
            pointer_activation: PointerActivation::Press,
            context_menu: false,
            context_menu_hit_shapes: None,
            preserves_overlays: false,
            auxiliary_pointer_barrier: false,
            accessible_value: None,
            accessible_numeric_value: None,
            accessible_min_numeric_value: None,
            accessible_max_numeric_value: None,
            accessible_numeric_value_step: None,
        }
    }

    /// Request context over the primary shape without activating or capturing this control.
    /// Replaces any context-only hit restrictions.
    pub fn with_context_menu(mut self) -> Self {
        self.context_menu = true;
        self.context_menu_hit_shapes = None;
        self
    }

    /// Restrict context requests to these world-space shapes within the primary shape.
    /// An empty collection claims no context requests; primary activation is unchanged.
    pub fn with_context_menu_hit_shapes(mut self, shapes: impl Into<Arc<[HitShape]>>) -> Self {
        self.context_menu = true;
        self.context_menu_hit_shapes = Some(shapes.into());
        self
    }

    pub fn requests_context_menu_at(&self, point: core::Point) -> bool {
        self.context_menu
            && self.shape.contains(point)
            && self
                .context_menu_hit_shapes
                .as_ref()
                .is_none_or(|shapes| shapes.iter().any(|shape| shape.contains(point)))
    }

    pub fn with_middle_pointer(mut self, enabled: bool) -> Self {
        self.middle_pointer = if enabled {
            MiddlePointerBehavior::Capture
        } else {
            MiddlePointerBehavior::PassThrough
        };
        self
    }

    /// Consume middle presses without activating or focusing this control.
    pub fn with_inert_middle_pointer(mut self) -> Self {
        self.middle_pointer = MiddlePointerBehavior::Block;
        self
    }

    pub fn with_auxiliary_pointer_barrier(mut self) -> Self {
        self.auxiliary_pointer_barrier = true;
        self
    }

    pub fn preserving_overlays(mut self) -> Self {
        self.preserves_overlays = true;
        self
    }

    pub fn with_pointer_activation(mut self, activation: PointerActivation) -> Self {
        self.pointer_activation = activation;
        self
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

    pub fn primary_control_at_screen(
        &self,
        screen: core::Point,
        viewport: core::Viewport,
        click_count: usize,
    ) -> Option<&WorldHitRegion> {
        self.primary_hit_at_screen(screen, viewport, click_count)
            .filter(|hit| hit.role == HitRole::Control)
    }

    pub fn primary_hit_at_screen(
        &self,
        screen: core::Point,
        viewport: core::Viewport,
        click_count: usize,
    ) -> Option<&WorldHitRegion> {
        self.hit_test_screen(screen, viewport).filter(|hit| {
            hit.role == HitRole::PointerBarrier
                || (hit.role == HitRole::Control && hit.pointer_activation.accepts(click_count))
        })
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
            Self::VectorPath {
                origin,
                scale,
                commands,
                fill,
            } => ScreenPrimitive::VectorPath {
                origin: transform.point(*origin),
                scale: transform.length(*scale),
                commands: commands.clone(),
                fill: *fill,
            },
            Self::InlineText(text) => ScreenPrimitive::InlineText(text.project(transform)),
            Self::TextInput(field) => ScreenPrimitive::TextInput(field.project(transform)),
            Self::TextLabel(label) => ScreenPrimitive::TextLabel(label.project(transform)),
            Self::TextRow(row) => ScreenPrimitive::TextRow(row.project(transform)),
            Self::HorizontalGradient(gradient) => {
                ScreenPrimitive::HorizontalGradient(gradient.project(transform))
            }
            Self::Quad {
                bounds,
                fill,
                corner_radius,
            } => ScreenPrimitive::Quad {
                bounds: transform.rect(*bounds),
                fill: *fill,
                corner_radius: transform.length(*corner_radius),
            },
            Self::Checkerboard {
                bounds,
                base,
                alternate,
                foreground,
                cell_size,
                corner_radius,
            } => ScreenPrimitive::Checkerboard {
                bounds: transform.rect(*bounds),
                base: *base,
                alternate: *alternate,
                foreground: *foreground,
                cell_size: transform.length(*cell_size),
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
    VectorPath {
        origin: core::Point,
        scale: f32,
        commands: Arc<[VectorCommand]>,
        fill: WorldColor,
    },
    InlineText(InlineText),
    TextRow(TextRow),
    TextLabel(TextLabel),
    TextInput(TextField),
    HorizontalGradient(GradientQuad),
    Quad {
        bounds: core::Rect,
        fill: WorldColor,
        corner_radius: f32,
    },
    Checkerboard {
        bounds: core::Rect,
        base: WorldColor,
        alternate: WorldColor,
        foreground: WorldColor,
        cell_size: f32,
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

    #[test]
    fn double_click_controls_leave_single_presses_to_the_node() {
        let mut scene = WorldScene::new().with_hit_region(
            WorldHitRegion::new(
                "title",
                HitRole::Control,
                HitShape::Rect(core::Rect {
                    origin: core::Point::new(10.0, 10.0),
                    size: core::Size {
                        width: 100.0,
                        height: 20.0,
                    },
                }),
            )
            .with_pointer_activation(PointerActivation::DoubleClick),
        );
        for zoom in [0.5, 1.0, 2.0] {
            let viewport = core::Viewport {
                zoom,
                pan: core::Point::new(7.0, -3.0),
            };
            let screen = viewport.world_to_screen(core::Point::new(20.0, 15.0));
            assert!(
                scene
                    .primary_control_at_screen(screen, viewport, 1)
                    .is_none()
            );
            assert_eq!(
                scene
                    .primary_control_at_screen(screen, viewport, 2)
                    .unwrap()
                    .id,
                "title"
            );
        }
        scene.hit_regions[0].pointer_activation = PointerActivation::Press;
        assert_eq!(
            scene
                .primary_control_at_screen(
                    core::Point::new(20.0, 15.0),
                    core::Viewport::default(),
                    1
                )
                .unwrap()
                .id,
            "title"
        );
    }

    #[test]
    fn label_projection_keeps_the_complete_document() {
        let label = TextLabel::new(
            core::Rect {
                origin: core::Point::new(12.0, 24.0),
                size: core::Size {
                    width: 160.0,
                    height: 16.0,
                },
            },
            "file/café/👩‍🚀.data",
        );
        let ScreenPrimitive::TextLabel(projected) = WorldPrimitive::TextLabel(label)
            .project(Transform::new(core::Point::new(7.0, -3.0), 2.0))
        else {
            panic!("label primitive");
        };
        assert_eq!(projected.bounds.origin, core::Point::new(31.0, 45.0));
        assert_eq!(
            projected.bounds.size,
            core::Size {
                width: 320.0,
                height: 32.0
            }
        );
        assert_eq!(projected.text, "file/café/👩‍🚀.data");
        assert_eq!(projected.font_size, 22.0);
    }

    #[test]
    fn inline_projection_scales_layout_without_losing_span_styles() {
        let mut leading = TextSpan::new("Mullion / café");
        leading.italic = true;
        let mut trailing = TextSpan::new("HRLV_Content");
        trailing.font_family = Some("Noto Sans Mono".to_owned());
        trailing.font_size = 10.0;
        let mut inline = InlineText::new(rect(12.0, 42.0, 276.0, 14.0), leading, trailing);
        inline.clip = Some(rect(0.0, 42.0, 300.0, 14.0));
        let ScreenPrimitive::InlineText(projected) =
            WorldPrimitive::InlineText(inline).project(Transform::new(p(7.0, -3.0), 2.0))
        else {
            panic!("inline primitive");
        };
        assert_eq!(projected.bounds, rect(31.0, 81.0, 552.0, 28.0));
        assert_eq!(projected.clip, Some(rect(7.0, 81.0, 600.0, 28.0)));
        assert_eq!(projected.leading.text, "Mullion / café");
        assert!(projected.leading.italic);
        assert_eq!(projected.leading.font_size, 22.0);
        assert_eq!(projected.trailing.text, "HRLV_Content");
        assert_eq!(
            projected.trailing.font_family.as_deref(),
            Some("Noto Sans Mono")
        );
        assert_eq!(projected.trailing.font_size, 20.0);
        assert_eq!(projected.trailing.line_height, 28.0);
        assert_eq!(projected.gap, 12.0);
    }

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

#[cfg(test)]
mod text_field_tests {
    use super::*;

    #[test]
    fn text_field_projection_scales_geometry_without_reflowing_or_changing_utf16_state() {
        let mut field = TextField::new(
            "field",
            core::Rect {
                origin: core::Point::new(12.0, 24.0),
                size: core::Size {
                    width: 160.0,
                    height: 20.0,
                },
            },
            "a💡b",
        );
        field.selection = 1..3;
        field.marked = Some(1..3);
        field.font_family = Some("Inter".into());
        let original = field.clone();
        for zoom in [0.1, 0.740818, 1.0, 1.349859, 2.0, 5.0] {
            let projected = WorldPrimitive::TextInput(field.clone())
                .project(Transform::new(core::Point::new(7.0, -3.0), zoom));
            let ScreenPrimitive::TextInput(projected) = projected else {
                panic!("text field primitive")
            };
            assert!((projected.bounds.origin.x - (12.0 * zoom + 7.0)).abs() < 0.00001);
            assert!((projected.bounds.origin.y - (24.0 * zoom - 3.0)).abs() < 0.00001);
            assert_eq!(projected.bounds.size.width, 160.0 * zoom);
            assert_eq!(projected.font_size, 11.0 * zoom);
            assert_eq!(projected.padding_x, 4.0 * zoom);
            assert_eq!(projected.caret_width, zoom);
            assert_eq!(projected.text, "a💡b");
            assert_eq!(projected.selection, 1..3);
            assert_eq!(projected.marked, Some(1..3));
            assert_eq!(projected.font_family.as_deref(), Some("Inter"));
        }
        assert_eq!(field, original);
    }
}

#[cfg(test)]
mod vector_tests {
    use super::*;
    #[test]
    fn vector_projection_shares_geometry_and_scales_once() {
        let commands: Arc<[VectorCommand]> = vec![
            VectorCommand::Move(core::Point::new(2., 3.)),
            VectorCommand::Close,
        ]
        .into();
        let primitive = WorldPrimitive::VectorPath {
            origin: core::Point::new(10., 20.),
            scale: 1. / 3.,
            commands: commands.clone(),
            fill: WorldColor::rgb(0x888888),
        };
        let ScreenPrimitive::VectorPath {
            origin,
            scale,
            commands: projected,
            ..
        } = primitive.project(Transform::new(core::Point::new(7., -3.), 2.))
        else {
            panic!("vector path")
        };
        assert_eq!(origin, core::Point::new(27., 37.));
        assert_eq!(scale, 2. / 3.);
        assert!(Arc::ptr_eq(&commands, &projected));
    }
}

#[cfg(test)]
mod checkerboard_tests {
    use super::*;

    #[test]
    fn checkerboard_projection_scales_cells_and_corners_with_its_bounds() {
        let primitive = WorldPrimitive::Checkerboard {
            bounds: core::Rect {
                origin: core::Point::new(10., 20.),
                size: core::Size {
                    width: 14.,
                    height: 14.,
                },
            },
            base: WorldColor::rgb(0xc0c0c0),
            alternate: WorldColor::rgb(0x808080),
            foreground: WorldColor::rgba(0x336699, 0.42),
            cell_size: 6.,
            corner_radius: 4.,
        };
        for (zoom, origin, extent, cell, radius) in [
            (0.5, core::Point::new(12., 7.), 7., 3., 2.),
            (2., core::Point::new(27., 37.), 28., 12., 8.),
        ] {
            assert_eq!(
                primitive.project(Transform::new(core::Point::new(7., -3.), zoom)),
                ScreenPrimitive::Checkerboard {
                    bounds: core::Rect {
                        origin,
                        size: core::Size {
                            width: extent,
                            height: extent
                        },
                    },
                    base: WorldColor::rgb(0xc0c0c0),
                    alternate: WorldColor::rgb(0x808080),
                    foreground: WorldColor::rgba(0x336699, 0.42),
                    cell_size: cell,
                    corner_radius: radius,
                }
            );
        }
    }
}

#[cfg(test)]
mod gradient_tests {
    use super::*;

    #[test]
    fn gradient_rgba_normalizes_before_lossy_color_space_conversion() {
        assert_eq!(
            GradientStop {
                position: 2.0,
                rgba: [-1.0, 2.0, 0.1234567, 0.654321]
            }
            .normalized(),
            Some(GradientStop {
                position: 1.0,
                rgba: [0.0, 1.0, 0.1234567, 0.654321]
            })
        );
        for invalid in [
            GradientStop {
                position: 0.0,
                rgba: [f32::NAN, 0.0, 0.0, 1.0],
            },
            GradientStop {
                position: 0.0,
                rgba: [0.0, f32::INFINITY, 0.0, 1.0],
            },
            GradientStop {
                position: 0.0,
                rgba: [0.0, 0.0, f32::NEG_INFINITY, 1.0],
            },
            GradientStop {
                position: 0.0,
                rgba: [0.0, 0.0, 0.0, f32::NAN],
            },
            GradientStop {
                position: f32::NAN,
                rgba: [0.0, 0.0, 0.0, 1.0],
            },
        ] {
            assert_eq!(invalid.normalized(), None);
        }
    }

    #[test]
    fn gradient_projection_scales_frame_and_preserves_shared_float_stops() {
        let stops: Arc<[GradientStop]> = vec![
            GradientStop {
                position: 0.25,
                rgba: [0.1234567, 0.0, 1.0, 0.456789],
            },
            GradientStop {
                position: 0.75,
                rgba: [1.0, 0.2, 0.0, 0.0],
            },
        ]
        .into();
        let primitive = WorldPrimitive::HorizontalGradient(GradientQuad {
            bounds: core::Rect {
                origin: core::Point::new(10.0, 20.0),
                size: core::Size {
                    width: 100.0,
                    height: 40.0,
                },
            },
            stops: stops.clone(),
            corner_radii: CornerRadii {
                top_left: 4.0,
                top_right: 3.0,
                bottom_right: 2.0,
                bottom_left: 1.0,
            },
            border_widths: BorderWidths {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 0.0,
            },
            border: WorldColor::rgba(0x102030, 0.42),
        });
        for (zoom, origin, width, height, corners, borders) in [
            (
                0.5,
                core::Point::new(12.0, 7.0),
                50.0,
                20.0,
                CornerRadii {
                    top_left: 2.0,
                    top_right: 1.5,
                    bottom_right: 1.0,
                    bottom_left: 0.5,
                },
                BorderWidths {
                    top: 0.5,
                    right: 1.0,
                    bottom: 1.5,
                    left: 0.0,
                },
            ),
            (
                2.0,
                core::Point::new(27.0, 37.0),
                200.0,
                80.0,
                CornerRadii {
                    top_left: 8.0,
                    top_right: 6.0,
                    bottom_right: 4.0,
                    bottom_left: 2.0,
                },
                BorderWidths {
                    top: 2.0,
                    right: 4.0,
                    bottom: 6.0,
                    left: 0.0,
                },
            ),
        ] {
            let ScreenPrimitive::HorizontalGradient(projected) =
                primitive.project(Transform::new(core::Point::new(7.0, -3.0), zoom))
            else {
                panic!("expected gradient")
            };
            assert_eq!(
                projected.bounds,
                core::Rect {
                    origin,
                    size: core::Size { width, height }
                }
            );
            assert_eq!(projected.corner_radii, corners);
            assert_eq!(projected.border_widths, borders);
            assert_eq!(projected.border, WorldColor::rgba(0x102030, 0.42));
            assert_eq!(
                projected.stops[0],
                GradientStop {
                    position: 0.25,
                    rgba: [0.1234567, 0.0, 1.0, 0.456789]
                }
            );
            assert!(Arc::ptr_eq(&stops, &projected.stops));
        }
    }
}

#[cfg(test)]
mod context_policy_tests {
    use super::*;

    #[test]
    fn context_geometry_restricts_without_extending_the_primary_region() {
        let region = WorldHitRegion::new(
            "gradient",
            HitRole::Control,
            HitShape::Rect(core::Rect {
                origin: core::Point::new(10., 10.),
                size: core::Size {
                    width: 30.,
                    height: 30.,
                },
            }),
        );
        assert!(!region.requests_context_menu_at(core::Point::new(20., 20.)));
        let restricted = region.with_context_menu_hit_shapes(vec![
            HitShape::Circle {
                center: core::Point::new(20., 20.),
                radius: 4.,
            },
            HitShape::Circle {
                center: core::Point::new(100., 100.),
                radius: 5.,
            },
        ]);
        assert!(restricted.requests_context_menu_at(core::Point::new(24., 20.)));
        assert!(!restricted.requests_context_menu_at(core::Point::new(25., 20.)));
        assert!(!restricted.requests_context_menu_at(core::Point::new(100., 100.)));
        assert!(restricted.shape.contains(core::Point::new(25., 20.)));
        let mut disabled = restricted.clone();
        disabled.context_menu = false;
        assert!(!disabled.requests_context_menu_at(core::Point::new(20., 20.)));
        let full = restricted.with_context_menu();
        assert!(full.context_menu_hit_shapes.is_none());
        assert!(full.requests_context_menu_at(core::Point::new(25., 20.)));
        let empty = full.with_context_menu_hit_shapes(Vec::<HitShape>::new());
        assert!(!empty.requests_context_menu_at(core::Point::new(20., 20.)));
    }

    #[test]
    fn context_menu_is_opt_in_and_preserves_other_hit_policies() {
        let original = WorldHitRegion::new(
            "gradient",
            HitRole::Control,
            HitShape::Circle {
                center: core::Point::new(20., 30.),
                radius: 7.,
            },
        )
        .with_middle_pointer(true)
        .with_auxiliary_pointer_barrier();
        assert!(!original.context_menu);
        let requested = original.clone().with_context_menu();
        assert!(requested.context_menu);
        assert_eq!(
            WorldHitRegion {
                context_menu: false,
                ..requested
            },
            original
        );
    }
}
