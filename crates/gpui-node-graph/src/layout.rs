//! Deterministic geometry for the editor-owned default node chrome.
//!
//! The Leptos reference measures the header, body, and the ports section rather
//! than asking graph consumers to maintain absolute socket coordinates.  This
//! module keeps that measurement boundary explicit: callers supply measured
//! section metrics and receive node-relative anchor positions.  No persisted
//! graph geometry is mutated.

use crate::{
    core::{Point, PortDirection, Rect, Size},
    style::{AnchorLayout, NodeGraphTheme},
};

/// Measured node sections used to lay out the default port rows.
///
/// Heights are border-box heights in world units. `ports_y_offset` is measured
/// from the node's top edge to the ports section's top edge. It is kept
/// separately because borders, consumer chrome, or an empty body can make it
/// differ slightly from `header_height + body_height`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NodeSectionMetrics {
    pub width: f32,
    pub header_height: f32,
    pub body_height: f32,
    pub ports_y_offset: f32,
}

impl NodeSectionMetrics {
    /// Construct contiguous header/body/ports sections.
    pub fn contiguous(width: f32, header_height: f32, body_height: f32) -> Self {
        Self {
            width,
            header_height,
            body_height,
            ports_y_offset: header_height + body_height,
        }
    }

    /// Override the measured ports-section offset.
    pub fn with_ports_y_offset(mut self, ports_y_offset: f32) -> Self {
        self.ports_y_offset = ports_y_offset;
        self
    }
}

/// Node-relative result of laying out the default header, body, and ports.
/// `port_offsets` has the same order as the directions passed to
/// [`layout_default_node`].
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultNodeLayout {
    pub header: Rect,
    pub body: Rect,
    pub ports: Rect,
    pub size: Size,
    pub port_offsets: Vec<Point>,
}

/// Lay out editor-owned default port rows from measured sections and style.
///
/// In [`AnchorLayout::Columns`], inputs and outputs each have an independent row
/// counter, so opposite directions share rows. In [`AnchorLayout::Stacked`], all
/// input rows precede all output rows. This matches the reference registry's
/// deterministic row-slot calculation.
pub fn layout_default_node(
    style: &NodeGraphTheme,
    metrics: NodeSectionMetrics,
    directions: &[PortDirection],
) -> DefaultNodeLayout {
    let width = non_negative(metrics.width);
    let header_height = non_negative(metrics.header_height);
    let body_height = non_negative(metrics.body_height);
    let ports_y = non_negative(metrics.ports_y_offset);
    let padding_y = non_negative(style.node.ports_padding_y);
    let row_height = non_negative(style.anchor.row_height);
    let inset = non_negative(style.anchor.dot_inset).min(width);

    let input_count = directions
        .iter()
        .filter(|direction| **direction == PortDirection::Input)
        .count();
    let output_count = directions.len() - input_count;
    let rows = match style.node.anchor_layout {
        AnchorLayout::Columns => input_count.max(output_count),
        AnchorLayout::Stacked => input_count + output_count,
    };
    let ports_height = padding_y * 2.0 + row_height * rows as f32;
    let first_y = ports_y + padding_y + row_height * 0.5;

    let mut next_input = 0usize;
    let mut next_output = 0usize;
    let port_offsets = directions
        .iter()
        .map(|direction| {
            let row = match (*direction, style.node.anchor_layout) {
                (PortDirection::Input, _) => {
                    let row = next_input;
                    next_input += 1;
                    row
                }
                (PortDirection::Output, AnchorLayout::Columns) => {
                    let row = next_output;
                    next_output += 1;
                    row
                }
                (PortDirection::Output, AnchorLayout::Stacked) => {
                    let row = input_count + next_output;
                    next_output += 1;
                    row
                }
            };
            Point::new(
                if *direction == PortDirection::Input {
                    inset
                } else {
                    width - inset
                },
                first_y + row as f32 * row_height,
            )
        })
        .collect();

    let section_bottom = (header_height + body_height).max(ports_y + ports_height);
    DefaultNodeLayout {
        header: rect(0.0, width, header_height),
        body: rect(header_height, width, body_height),
        ports: rect(ports_y, width, ports_height),
        size: Size {
            width,
            height: section_bottom,
        },
        port_offsets,
    }
}

fn non_negative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn rect(y: f32, width: f32, height: f32) -> Rect {
    Rect {
        origin: Point::new(0.0, y),
        size: Size { width, height },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_share_rows_and_use_measured_ports_offset() {
        let style = NodeGraphTheme::default();
        let directions = [
            PortDirection::Input,
            PortDirection::Output,
            PortDirection::Input,
        ];
        let layout = layout_default_node(
            &style,
            NodeSectionMetrics::contiguous(160.0, 28.0, 30.0),
            &directions,
        );

        assert_eq!(layout.header, rect(0.0, 160.0, 28.0));
        assert_eq!(layout.body, rect(28.0, 160.0, 30.0));
        assert_eq!(layout.ports, rect(58.0, 160.0, 56.0));
        assert_eq!(
            layout.port_offsets,
            vec![
                Point::new(14.0, 74.0),
                Point::new(146.0, 74.0),
                Point::new(14.0, 98.0),
            ]
        );
        assert_eq!(
            layout.size,
            Size {
                width: 160.0,
                height: 114.0
            }
        );
    }

    #[test]
    fn stacked_places_outputs_after_all_inputs() {
        let mut style = NodeGraphTheme::default();
        style.node.anchor_layout = AnchorLayout::Stacked;
        let layout = layout_default_node(
            &style,
            NodeSectionMetrics::contiguous(160.0, 28.0, 30.0),
            &[
                PortDirection::Output,
                PortDirection::Input,
                PortDirection::Input,
            ],
        );
        assert_eq!(
            layout.port_offsets,
            vec![
                Point::new(146.0, 122.0),
                Point::new(14.0, 74.0),
                Point::new(14.0, 98.0),
            ]
        );
        assert_eq!(layout.ports.size.height, 80.0);
    }

    #[test]
    fn explicit_ports_measurement_wins_and_invalid_style_is_safe() {
        let mut style = NodeGraphTheme::default();
        style.anchor.row_height = f32::NAN;
        style.anchor.dot_inset = f32::INFINITY;
        let layout = layout_default_node(
            &style,
            NodeSectionMetrics::contiguous(100.0, 20.0, 10.0).with_ports_y_offset(42.0),
            &[PortDirection::Output],
        );
        assert_eq!(layout.ports.origin.y, 42.0);
        assert_eq!(layout.port_offsets, vec![Point::new(100.0, 46.0)]);
        assert!(layout.size.width.is_finite());
        assert!(layout.size.height.is_finite());
    }
}
