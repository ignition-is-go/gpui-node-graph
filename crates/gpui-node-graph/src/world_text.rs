//! Shared native/web text-field painting and shaped glyph hit testing.
use crate::{core, utf16_to_byte, world};
use gpui::{
    App, Bounds, ContentMask, ShapedLine, TextAlign, TextRun, Window, fill, point, px, rgb,
};
use std::{cell::RefCell, collections::HashMap, ops::Range, rc::Rc};

pub(crate) type Layouts = Rc<RefCell<HashMap<String, TextInputPaint>>>;

#[derive(Clone)]
pub(crate) struct TextInputPaint {
    field: world::TextField,
    line: ShapedLine,
    origin: gpui::Point<gpui::Pixels>,
}

impl TextInputPaint {
    pub(crate) fn new(field: world::TextField, window: &Window) -> Self {
        let mut font = window.text_style().font();
        if let Some(family) = &field.font_family {
            font.family = family.clone().into();
        }
        font.weight = gpui::FontWeight(field.font_weight as f32);
        let run = TextRun {
            len: field.text.len(),
            font,
            color: rgb(field.color.rgb).opacity(field.color.alpha).into(),
            ..Default::default()
        };
        let line = window.text_system().shape_line(
            field.text.clone().into(),
            px(field.font_size),
            &[run],
            None,
        );
        let inner_width = (field.bounds.size.width - field.padding_x * 2.0).max(1.0);
        let focus = if field.selection_reversed {
            field.selection.start
        } else {
            field.selection.end
        };
        let focus_byte = utf16_to_byte(&field.text, focus).0;
        let focus_x = f32::from(line.x_for_index(focus_byte));
        let line_width = f32::from(line.width);
        let scroll = (focus_x - inner_width)
            .max(0.0)
            .min((line_width - inner_width).max(0.0));
        let right_offset = if field.align_right && line_width < inner_width {
            inner_width - line_width
        } else {
            0.0
        };
        let origin = point(
            px(field.bounds.origin.x + field.padding_x + right_offset - scroll),
            px(field.bounds.origin.y + (field.bounds.size.height - field.line_height) * 0.5),
        );
        Self {
            field,
            line,
            origin,
        }
    }

    pub(crate) fn index_at(&self, position: core::Point) -> usize {
        let byte = self
            .line
            .closest_index_for_x(px(position.x) - self.origin.x);
        self.field.text[..byte].encode_utf16().count()
    }

    pub(crate) fn range_bounds(&self, range: Range<usize>) -> Bounds<gpui::Pixels> {
        let start = utf16_to_byte(&self.field.text, range.start).0;
        let end = utf16_to_byte(&self.field.text, range.end.max(range.start)).0;
        let left = self.line.x_for_index(start);
        let right = self.line.x_for_index(end);
        Bounds::new(
            point(self.origin.x + left, self.origin.y),
            gpui::size(
                (right - left).max(px(self.field.caret_width)),
                px(self.field.line_height),
            ),
        )
    }

    pub(crate) fn paint(&self, offset: core::Point, window: &mut Window, cx: &mut App) {
        let offset = point(px(offset.x), px(offset.y));
        let mask = ContentMask {
            bounds: Bounds::new(
                point(
                    px(self.field.bounds.origin.x),
                    px(self.field.bounds.origin.y),
                ) + offset,
                gpui::size(
                    px(self.field.bounds.size.width),
                    px(self.field.bounds.size.height),
                ),
            ),
        };
        window.with_content_mask(Some(mask), |window| {
            if self.field.focused && !self.field.selection.is_empty() {
                let mut bounds = self.range_bounds(self.field.selection.clone());
                bounds.origin += offset;
                window.paint_quad(fill(
                    bounds,
                    rgb(self.field.selection_color.rgb).opacity(self.field.selection_color.alpha),
                ));
            }
            let _ = self.line.paint(
                self.origin + offset,
                px(self.field.line_height),
                TextAlign::Left,
                None,
                window,
                cx,
            );
            if let Some(marked) = &self.field.marked {
                let mut bounds = self.range_bounds(marked.clone());
                bounds.origin += offset;
                bounds.origin.y += bounds.size.height - px(self.field.caret_width);
                bounds.size.height = px(self.field.caret_width);
                window.paint_quad(fill(
                    bounds,
                    rgb(self.field.caret_color.rgb).opacity(self.field.caret_color.alpha),
                ));
            }
            if self.field.focused && self.field.selection.is_empty() {
                let mut bounds = self.range_bounds(self.field.selection.clone());
                bounds.origin += offset;
                bounds.size.width = px(self.field.caret_width);
                window.paint_quad(fill(
                    bounds,
                    rgb(self.field.caret_color.rgb).opacity(self.field.caret_color.alpha),
                ));
            }
        });
    }
}
