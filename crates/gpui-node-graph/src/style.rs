//! Strongly typed visual configuration for the GPUI node graph.
//!
//! The defaults in this module are a direct translation of the visible values
//! used by `leptos-node-graph`.  Lengths are pixels, font sizes are pixels, and
//! colors retain a 24-bit RGB value plus an independent alpha component.  This
//! keeps the configuration useful to GPUI without carrying CSS strings.

/// A 24-bit RGB color and an alpha value in the inclusive range `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub rgb: u32,
    pub alpha: f32,
}

impl Color {
    pub const fn rgb(rgb: u32) -> Self {
        Self { rgb, alpha: 1.0 }
    }

    pub const fn rgba(rgb: u32, alpha: f32) -> Self {
        Self { rgb, alpha }
    }

    pub const TRANSPARENT: Self = Self::rgba(0, 0.0);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LineStyle {
    None,
    Solid,
    Dashed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub width: f32,
    pub style: LineStyle,
    pub color: Color,
}

impl Border {
    pub const fn solid(width: f32, color: Color) -> Self {
        Self {
            width,
            style: LineStyle::Solid,
            color,
        }
    }

    pub const fn none() -> Self {
        Self {
            width: 0.0,
            style: LineStyle::None,
            color: Color::TRANSPARENT,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub offset_x: f32,
    pub offset_y: f32,
    pub blur: f32,
    pub spread: f32,
    pub color: Color,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cursor {
    Default,
    Grab,
    Grabbing,
    Crosshair,
    EwResize,
}

/// Controls how a node's inputs and outputs are arranged.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnchorLayout {
    #[default]
    Columns,
    Stacked,
}

/// Shape of a port socket.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DotShape {
    #[default]
    Circle,
    Diamond,
    Square,
    Hexagon,
    Triangle,
    Star,
}

/// Root editor and unscaled overlay-layer appearance.
#[derive(Clone, Debug, PartialEq)]
pub struct EditorStyle {
    /// The Leptos editor is transparent; the demo supplies its dark wrapper.
    pub background: Color,
    pub focus_outline: Border,
    pub clip_content: bool,
    pub overlay_clip_content: bool,
    pub overlay_isolated: bool,
}

impl Default for EditorStyle {
    fn default() -> Self {
        Self {
            background: Color::TRANSPARENT,
            focus_outline: Border::none(),
            clip_content: true,
            overlay_clip_content: true,
            overlay_isolated: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NodeStyle {
    pub background: Color,
    pub border: Border,
    pub outline_selected: Border,
    pub border_radius: f32,
    pub shadow: Vec<Shadow>,
    pub shadow_selected: Vec<Shadow>,
    pub min_width: f32,
    /// `None` has the same meaning as the Leptos default empty CSS width.
    pub width: Option<f32>,
    pub opacity_dragging: f32,
    pub padding_x: f32,
    pub header_padding_y: f32,
    pub body_padding_y: f32,
    pub ports_padding_y: f32,
    pub header_border_bottom: Border,
    pub header_background: Color,
    pub header_color: Color,
    pub header_font_size: f32,
    pub header_accent_height: f32,
    pub field_label_color: Color,
    pub field_label_font_size: f32,
    pub field_gap: f32,
    pub field_label_min_width: f32,
    pub body_border_bottom: Border,
    pub anchor_layout: AnchorLayout,
    pub cursor: Cursor,
    pub cursor_dragging: Cursor,
    pub cursor_resize: Cursor,
    pub resizable: bool,
    pub resize_handle_width: f32,
    pub resize_handle_color: Color,
    pub resize_min_width: f32,
    pub resize_max_width: Option<f32>,
}

impl Default for NodeStyle {
    fn default() -> Self {
        Self {
            background: Color::rgb(0x1e1e22),
            border: Border::solid(1.0, Color::rgb(0x3f3f46)),
            outline_selected: Border::solid(1.5, Color::rgb(0xef4444)),
            border_radius: 8.0,
            shadow: vec![Shadow {
                offset_x: 0.0,
                offset_y: 4.0,
                blur: 12.0,
                spread: 0.0,
                color: Color::rgba(0x000000, 0.4),
            }],
            shadow_selected: vec![
                Shadow {
                    offset_x: 0.0,
                    offset_y: 0.0,
                    blur: 0.0,
                    spread: 1.0,
                    color: Color::rgb(0xef4444),
                },
                Shadow {
                    offset_x: 0.0,
                    offset_y: 4.0,
                    blur: 16.0,
                    spread: 0.0,
                    color: Color::rgba(0xef4444, 0.25),
                },
            ],
            min_width: 160.0,
            width: None,
            opacity_dragging: 0.92,
            padding_x: 10.0,
            header_padding_y: 6.0,
            body_padding_y: 6.0,
            ports_padding_y: 4.0,
            header_border_bottom: Border::solid(1.0, Color::rgb(0x27272a)),
            header_background: Color::rgb(0x232327),
            header_color: Color::rgb(0xa1a1aa),
            header_font_size: 12.0,
            header_accent_height: 2.0,
            field_label_color: Color::rgb(0x71717a),
            field_label_font_size: 10.0,
            field_gap: 6.0,
            field_label_min_width: 38.0,
            body_border_bottom: Border::solid(1.0, Color::rgb(0x27272a)),
            anchor_layout: AnchorLayout::Columns,
            cursor: Cursor::Grab,
            cursor_dragging: Cursor::Grabbing,
            cursor_resize: Cursor::EwResize,
            resizable: true,
            resize_handle_width: 6.0,
            resize_handle_color: Color::rgb(0x71717a),
            resize_min_width: 120.0,
            resize_max_width: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnchorStyle {
    pub dot_size: f32,
    pub dot_border_width: f32,
    pub dot_color: Color,
    pub dot_connected_color: Color,
    pub dot_compatible_color: Color,
    /// Shape-following drop shadows, in drawing order.
    pub dot_compatible_glow: Vec<Shadow>,
    pub label_font_size: f32,
    pub label_color: Color,
    pub label_compatible_color: Color,
    pub row_height: f32,
    pub first_port_y: f32,
    pub dot_inset: f32,
    pub row_padding_x: f32,
    pub row_padding_y: f32,
    pub row_gap: f32,
    pub incompatible_opacity: f32,
    pub tooltip_background: Color,
    pub tooltip_border: Border,
    pub tooltip_color: Color,
    pub default_dot_shape: DotShape,
}

impl Default for AnchorStyle {
    fn default() -> Self {
        Self {
            dot_size: 8.0,
            dot_border_width: 1.5,
            dot_color: Color::rgb(0x71717a),
            dot_connected_color: Color::rgb(0x71717a),
            dot_compatible_color: Color::rgb(0x22d3ee),
            dot_compatible_glow: vec![
                Shadow {
                    offset_x: 0.0,
                    offset_y: 0.0,
                    blur: 2.0,
                    spread: 0.0,
                    color: Color::rgb(0x22d3ee),
                },
                Shadow {
                    offset_x: 0.0,
                    offset_y: 0.0,
                    blur: 5.0,
                    spread: 0.0,
                    color: Color::rgba(0x22d3ee, 0.45),
                },
            ],
            label_font_size: 11.0,
            label_color: Color::rgb(0xa1a1aa),
            label_compatible_color: Color::rgb(0x22d3ee),
            row_height: 24.0,
            first_port_y: 0.0,
            dot_inset: 14.0,
            row_padding_x: 10.0,
            row_padding_y: 0.0,
            row_gap: 6.0,
            incompatible_opacity: 0.25,
            tooltip_background: Color::rgb(0x27272a),
            tooltip_border: Border::solid(1.0, Color::rgb(0x3f3f46)),
            tooltip_color: Color::rgb(0xa1a1aa),
            default_dot_shape: DotShape::Circle,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionStyle {
    pub stroke: Color,
    pub stroke_selected: Color,
    pub stroke_draft: Color,
    pub stroke_width: f32,
    pub stroke_width_selected: f32,
}

impl Default for ConnectionStyle {
    fn default() -> Self {
        Self {
            stroke: Color::rgb(0x71717a),
            stroke_selected: Color::rgb(0xef4444),
            stroke_draft: Color::rgb(0x22d3ee),
            stroke_width: 2.0,
            stroke_width_selected: 3.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectionBoxStyle {
    pub border: Border,
    pub background: Color,
}

impl Default for SelectionBoxStyle {
    fn default() -> Self {
        Self {
            border: Border::solid(1.0, Color::rgba(0x6366f1, 0.6)),
            background: Color::rgba(0x6366f1, 0.1),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupStyle {
    pub default_color: Color,
    pub border_radius: f32,
    pub border_width: f32,
    pub border_opacity: f32,
    pub background_opacity: f32,
    pub hovered_border_opacity: f32,
    pub hovered_background_opacity: f32,
    pub error_border: Color,
    pub error_background: Color,
    pub error_label_color: Color,
    pub label_font_size: f32,
    pub label_font_weight: u16,
    pub label_top: f32,
    pub label_left: f32,
    pub label_letter_spacing_em: f32,
    pub input_background: Color,
}

impl Default for GroupStyle {
    fn default() -> Self {
        Self {
            default_color: Color::rgb(0x8b5cf6),
            border_radius: 8.0,
            border_width: 1.0,
            border_opacity: 0.5,
            background_opacity: 0.1,
            hovered_border_opacity: 0.8,
            hovered_background_opacity: 0.2,
            error_border: Color::rgba(0xef4444, 0.5),
            error_background: Color::rgba(0xef4444, 0.08),
            error_label_color: Color::rgba(0xef4444, 0.8),
            label_font_size: 10.0,
            label_font_weight: 600,
            label_top: 6.0,
            label_left: 10.0,
            label_letter_spacing_em: 0.05,
            input_background: Color::TRANSPARENT,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MenuStyle {
    pub background: Color,
    pub border: Border,
    pub border_radius: f32,
    pub shadow: Vec<Shadow>,
    pub input_background: Color,
    pub input_border: Border,
    pub input_border_radius: f32,
    pub input_color: Color,
    pub placeholder_color: Color,
    pub category_color: Color,
    pub item_color: Color,
    pub description_color: Color,
    pub hover_background: Color,
    pub port_color: Color,
    pub empty_color: Color,
    pub divider: Border,
    pub min_width: f32,
    pub max_height: f32,
    pub viewport_margin: f32,
    pub search_padding: f32,
    pub input_font_size: f32,
    pub input_padding_x: f32,
    pub input_padding_y: f32,
    pub list_padding_y: f32,
}

/// Leptos calls this section `NodeMenuStyle`; retain that spelling as an
/// ergonomic compatibility alias.
pub type NodeMenuStyle = MenuStyle;

impl Default for MenuStyle {
    fn default() -> Self {
        Self {
            background: Color::rgb(0x1e1e22),
            border: Border::solid(1.0, Color::rgb(0x3f3f46)),
            border_radius: 8.0,
            shadow: vec![Shadow {
                offset_x: 0.0,
                offset_y: 8.0,
                blur: 24.0,
                spread: 0.0,
                color: Color::rgba(0x000000, 0.5),
            }],
            input_background: Color::rgb(0x27272a),
            input_border: Border::solid(1.0, Color::rgb(0x3f3f46)),
            input_border_radius: 4.0,
            input_color: Color::rgb(0xd4d4d8),
            placeholder_color: Color::rgb(0x71717a),
            category_color: Color::rgb(0x52525b),
            item_color: Color::rgb(0xd4d4d8),
            description_color: Color::rgb(0x71717a),
            hover_background: Color::rgba(0x6366f1, 0.15),
            port_color: Color::rgb(0xa1a1aa),
            empty_color: Color::rgb(0x71717a),
            divider: Border::solid(1.0, Color::rgb(0x27272a)),
            min_width: 220.0,
            max_height: 360.0,
            viewport_margin: 8.0,
            search_padding: 8.0,
            input_font_size: 12.0,
            input_padding_x: 8.0,
            input_padding_y: 6.0,
            list_padding_y: 4.0,
        }
    }
}

/// Styling for the pane-level, unscaled overlay facility.  Leptos intentionally
/// gives the pane, backdrop, and panel no paint by default; consumers paint the
/// panel content itself.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayStyle {
    pub layer_background: Color,
    pub backdrop_background: Color,
    pub panel_background: Color,
    pub panel_border: Border,
    pub clip_to_editor: bool,
    pub panel_pointer_events: bool,
    pub backdrop_pointer_events: bool,
}

impl Default for OverlayStyle {
    fn default() -> Self {
        Self {
            layer_background: Color::TRANSPARENT,
            backdrop_background: Color::TRANSPARENT,
            panel_background: Color::TRANSPARENT,
            panel_border: Border::none(),
            clip_to_editor: true,
            panel_pointer_events: true,
            backdrop_pointer_events: true,
        }
    }
}

/// Complete public style set for one editor.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct GraphStyle {
    pub editor: EditorStyle,
    pub node: NodeStyle,
    pub anchor: AnchorStyle,
    pub connection: ConnectionStyle,
    pub selection_box: SelectionBoxStyle,
    pub group: GroupStyle,
    pub menu: MenuStyle,
    pub overlay: OverlayStyle,
}

impl GraphStyle {
    /// The exact style overrides applied by the Leptos demo, including its
    /// full-window `#18181b` wrapper background.
    pub fn leptos_demo() -> Self {
        let mut style = Self::default();
        style.editor.background = Color::rgb(0x18181b);
        style.connection.stroke_selected = Color::rgb(0xdddddd);
        style.selection_box.border = Border::solid(1.0, Color::rgba(0xffffff, 0.1));
        style.selection_box.background = Color::rgba(0xffffff, 0.025);
        style.node.header_padding_y = 4.0;
        style.node.body_padding_y = 2.0;
        style.node.border_radius = 2.0; // 0.125rem at the browser's 16px root size
        style.node.outline_selected = Border::solid(1.0, Color::rgb(0xff0000));
        style.node.border = Border::none();
        style.node.background = Color::rgb(0x111111);
        style.node.header_background = Color::rgb(0x111111);
        style.node.header_border_bottom = Border::none();
        style.node.body_border_bottom = Border::none();
        style.anchor.row_height = 20.0;
        style.overlay.backdrop_pointer_events = false;
        style
    }
}

/// Return the complete Leptos demo preset.
pub fn leptos_demo() -> GraphStyle {
    GraphStyle::leptos_demo()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_helpers_preserve_rgb_and_alpha() {
        assert_eq!(
            Color::rgb(0x22d3ee),
            Color {
                rgb: 0x22d3ee,
                alpha: 1.0
            }
        );
        assert_eq!(Color::rgba(0xffffff, 0.025).alpha, 0.025);
        assert_eq!(Color::TRANSPARENT, Color::rgba(0, 0.0));
    }

    #[test]
    fn graph_default_is_composed_from_section_defaults() {
        let graph = GraphStyle::default();
        assert_eq!(graph.editor, EditorStyle::default());
        assert_eq!(graph.node, NodeStyle::default());
        assert_eq!(graph.anchor, AnchorStyle::default());
        assert_eq!(graph.connection, ConnectionStyle::default());
        assert_eq!(graph.selection_box, SelectionBoxStyle::default());
        assert_eq!(graph.group, GroupStyle::default());
        assert_eq!(graph.menu, MenuStyle::default());
        assert_eq!(graph.overlay, OverlayStyle::default());
    }

    #[test]
    fn node_defaults_match_leptos_theme() {
        let node = NodeStyle::default();
        assert_eq!(node.background, Color::rgb(0x1e1e22));
        assert_eq!(node.border, Border::solid(1.0, Color::rgb(0x3f3f46)));
        assert_eq!(
            node.outline_selected,
            Border::solid(1.5, Color::rgb(0xef4444))
        );
        assert_eq!(
            (node.border_radius, node.min_width, node.width),
            (8.0, 160.0, None)
        );
        assert_eq!(
            (
                node.padding_x,
                node.header_padding_y,
                node.body_padding_y,
                node.ports_padding_y
            ),
            (10.0, 6.0, 6.0, 4.0)
        );
        assert_eq!(
            (
                node.resizable,
                node.resize_handle_width,
                node.resize_min_width,
                node.resize_max_width
            ),
            (true, 6.0, 120.0, None)
        );
        assert_eq!(node.anchor_layout, AnchorLayout::Columns);
        assert_eq!(
            (node.cursor, node.cursor_dragging, node.cursor_resize),
            (Cursor::Grab, Cursor::Grabbing, Cursor::EwResize)
        );
        assert_eq!(node.shadow.len(), 1);
        assert_eq!(node.shadow_selected.len(), 2);
    }

    #[test]
    fn anchor_defaults_match_leptos_theme() {
        let anchor = AnchorStyle::default();
        assert_eq!(
            (anchor.dot_size, anchor.dot_border_width, anchor.row_height),
            (8.0, 1.5, 24.0)
        );
        assert_eq!(
            (
                anchor.first_port_y,
                anchor.dot_inset,
                anchor.row_padding_x,
                anchor.row_gap
            ),
            (0.0, 14.0, 10.0, 6.0)
        );
        assert_eq!(anchor.dot_compatible_color, Color::rgb(0x22d3ee));
        assert_eq!(anchor.dot_compatible_glow.len(), 2);
        assert_eq!(anchor.incompatible_opacity, 0.25);
        assert_eq!(anchor.default_dot_shape, DotShape::Circle);
        assert_eq!(
            anchor.tooltip_border,
            Border::solid(1.0, Color::rgb(0x3f3f46))
        );
    }

    #[test]
    fn wire_selection_and_group_defaults_match_leptos() {
        let wire = ConnectionStyle::default();
        assert_eq!(
            (wire.stroke, wire.stroke_selected, wire.stroke_draft),
            (
                Color::rgb(0x71717a),
                Color::rgb(0xef4444),
                Color::rgb(0x22d3ee)
            )
        );
        assert_eq!((wire.stroke_width, wire.stroke_width_selected), (2.0, 3.0));
        let selection = SelectionBoxStyle::default();
        assert_eq!(
            selection.border,
            Border::solid(1.0, Color::rgba(0x6366f1, 0.6))
        );
        assert_eq!(selection.background, Color::rgba(0x6366f1, 0.1));
        let group = GroupStyle::default();
        assert_eq!(group.default_color, Color::rgb(0x8b5cf6));
        assert_eq!((group.border_opacity, group.background_opacity), (0.5, 0.1));
        assert_eq!(group.error_background, Color::rgba(0xef4444, 0.08));
        assert_eq!(
            (group.label_font_size, group.label_font_weight),
            (10.0, 600)
        );
    }

    #[test]
    fn menu_and_overlay_defaults_match_leptos() {
        let menu = MenuStyle::default();
        assert_eq!(menu.background, Color::rgb(0x1e1e22));
        assert_eq!(menu.hover_background, Color::rgba(0x6366f1, 0.15));
        assert_eq!(
            (
                menu.border_radius,
                menu.min_width,
                menu.max_height,
                menu.viewport_margin
            ),
            (8.0, 220.0, 360.0, 8.0)
        );
        assert_eq!(menu.shadow[0].color, Color::rgba(0, 0.5));
        let overlay = OverlayStyle::default();
        assert_eq!(overlay.panel_background, Color::TRANSPARENT);
        assert_eq!(overlay.panel_border, Border::none());
        assert!(
            overlay.clip_to_editor
                && overlay.panel_pointer_events
                && overlay.backdrop_pointer_events
        );
    }

    #[test]
    fn leptos_demo_changes_only_documented_overrides() {
        let defaults = GraphStyle::default();
        let demo = GraphStyle::leptos_demo();
        assert_eq!(demo.editor.background, Color::rgb(0x18181b));
        assert_eq!(demo.connection.stroke, defaults.connection.stroke);
        assert_eq!(demo.connection.stroke_selected, Color::rgb(0xdddddd));
        assert_eq!(
            demo.selection_box.border,
            Border::solid(1.0, Color::rgba(0xffffff, 0.1))
        );
        assert_eq!(demo.selection_box.background, Color::rgba(0xffffff, 0.025));
        assert_eq!(
            (
                demo.node.header_padding_y,
                demo.node.body_padding_y,
                demo.node.border_radius
            ),
            (4.0, 2.0, 2.0)
        );
        assert_eq!(demo.node.border, Border::none());
        assert_eq!(demo.node.background, Color::rgb(0x111111));
        assert_eq!(demo.node.header_background, Color::rgb(0x111111));
        assert_eq!(demo.node.header_border_bottom, Border::none());
        assert_eq!(demo.node.body_border_bottom, Border::none());
        assert_eq!(demo.anchor.row_height, 20.0);
        assert_eq!(demo.menu, defaults.menu);
        assert_eq!(demo.group, defaults.group);
        let mut expected_overlay = defaults.overlay;
        expected_overlay.backdrop_pointer_events = false;
        assert_eq!(demo.overlay, expected_overlay);
    }

    #[test]
    fn styles_are_independently_cloneable_and_comparable() {
        let original = GraphStyle::default();
        let mut changed = original.clone();
        changed.node.resize_max_width = Some(480.0);
        changed.anchor.default_dot_shape = DotShape::Hexagon;
        assert_ne!(original, changed);
        assert_eq!(original, original.clone());
    }
}
