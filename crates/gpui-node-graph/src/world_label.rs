use crate::{
    WorldTextPaint, core,
    world::{InlineText, TextLabel, TextRow, TextSpan},
};
use gpui::{TextRun, Window, px, rgb};
use unicode_segmentation::UnicodeSegmentation;

fn ellipsize(text: &str, width: f32, mut measure: impl FnMut(&str) -> f32) -> String {
    if measure(text) <= width {
        return text.to_owned();
    }
    let mut boundaries: Vec<_> = text
        .grapheme_indices(true)
        .map(|(index, _)| index)
        .collect();
    boundaries.push(text.len());
    let mut low = 0;
    let mut high = boundaries.len() - 1;
    while low + 1 < high {
        let middle = low + (high - low) / 2;
        let candidate = format!("{}…", &text[..boundaries[middle]]);
        if measure(&candidate) <= width {
            low = middle;
        } else {
            high = middle;
        }
    }
    format!("{}…", &text[..boundaries[low]])
}

fn shape(label: &TextLabel, window: &Window, text: &str) -> gpui::ShapedLine {
    let mut font = window.text_style().font();
    if let Some(family) = &label.font_family {
        font.family = family.clone().into();
    }
    font.weight = gpui::FontWeight(label.font_weight as f32);
    font.style = if label.italic {
        gpui::FontStyle::Italic
    } else {
        gpui::FontStyle::Normal
    };
    let run = TextRun {
        len: text.len(),
        font: font.clone(),
        color: rgb(label.color.rgb).opacity(label.color.alpha).into(),
        ..Default::default()
    };
    window
        .text_system()
        .shape_line(text.to_owned().into(), px(label.font_size), &[run], None)
}

pub(crate) fn paint_data(label: &TextLabel, window: &Window) -> WorldTextPaint {
    let width = label.bounds.size.width.max(0.0);
    let text = ellipsize(&label.text, width, |candidate| {
        f32::from(shape(label, window, candidate).width)
    });
    let line = shape(label, window, &text);
    let inset = if label.align_right {
        (width - f32::from(line.width)).max(0.0)
    } else {
        0.0
    };
    WorldTextPaint {
        clip: Some(label.bounds),
        origin: core::Point::new(
            label.bounds.origin.x + inset,
            label.bounds.origin.y + (label.bounds.size.height - label.line_height) * 0.5,
        ),
        line_height: label.line_height,
        line,
    }
}

fn row_widths(width: f32, value_width: f32, gap: f32) -> (f32, f32) {
    let width = width.max(0.0);
    let available = (width - gap.max(0.0)).max(0.0);
    let value_width = value_width.max(0.0).min(available);
    ((available - value_width).max(0.0), value_width)
}

pub(crate) fn row_paint_data(row: &TextRow, window: &Window) -> Vec<WorldTextPaint> {
    let mut value = TextLabel::new(row.bounds, &row.value);
    value.font_family = row.font_family.clone();
    value.font_size = row.font_size;
    value.font_weight = row.font_weight;
    value.line_height = row.line_height;
    value.color = row.value_color;
    let natural_width = f32::from(shape(&value, window, &row.value).width);
    let gap = if row.label.is_empty() { 0.0 } else { row.gap };
    let (label_width, value_width) = row_widths(row.bounds.size.width, natural_width, gap);
    value.bounds.origin.x += row.bounds.size.width - value_width;
    value.bounds.size.width = value_width;
    let mut result = Vec::with_capacity(2);
    if label_width > 0.0 && !row.label.is_empty() {
        let label = TextLabel {
            bounds: core::Rect {
                origin: row.bounds.origin,
                size: core::Size {
                    width: label_width,
                    height: row.bounds.size.height,
                },
            },
            text: row.label.clone(),
            color: row.label_color,
            align_right: false,
            ..value.clone()
        };
        result.push(paint_data(&label, window));
    }
    if value_width > 0.0 {
        result.push(paint_data(&value, window));
    }
    result
}

fn span_label(span: &TextSpan, bounds: core::Rect) -> TextLabel {
    TextLabel {
        bounds,
        text: span.text.clone(),
        font_family: span.font_family.clone(),
        font_size: span.font_size,
        font_weight: span.font_weight,
        italic: span.italic,
        line_height: span.line_height,
        color: span.color,
        align_right: false,
    }
}

fn inline_widths(width: f32, leading: f32, trailing: f32, gap: f32) -> (f32, f32) {
    let gap = gap.max(0.0);
    let leading = leading.max(0.0).min((width - trailing - gap).max(0.0));
    (leading, leading + gap)
}

fn baseline(line_height: f32, ascent: f32, descent: f32) -> f32 {
    (line_height + ascent - descent) * 0.5
}

fn inline_origins(height: f32, leading: (f32, f32), trailing: (f32, f32)) -> (f32, f32) {
    let above = leading.1.max(trailing.1);
    let below = (leading.0 - leading.1).max(trailing.0 - trailing.1);
    let common = (height - above - below) * 0.5 + above;
    (common - leading.1, common - trailing.1)
}

fn intersect(bounds: core::Rect, clip: core::Rect) -> core::Rect {
    let x = bounds.origin.x.max(clip.origin.x);
    let y = bounds.origin.y.max(clip.origin.y);
    core::Rect {
        origin: core::Point::new(x, y),
        size: core::Size {
            width: ((bounds.origin.x + bounds.size.width).min(clip.origin.x + clip.size.width) - x)
                .max(0.0),
            height: ((bounds.origin.y + bounds.size.height).min(clip.origin.y + clip.size.height)
                - y)
                .max(0.0),
        },
    }
}

pub(crate) fn inline_paint_data(inline: &InlineText, window: &Window) -> Vec<WorldTextPaint> {
    let leading = span_label(&inline.leading, inline.bounds);
    let trailing = span_label(&inline.trailing, inline.bounds);
    let leading_line = shape(&leading, window, &leading.text);
    let trailing_line = shape(&trailing, window, &trailing.text);
    let (leading_width, trailing_x) = inline_widths(
        inline.bounds.size.width.max(0.0),
        f32::from(leading_line.width),
        f32::from(trailing_line.width),
        inline.gap,
    );
    let text = ellipsize(&leading.text, leading_width, |candidate| {
        f32::from(shape(&leading, window, candidate).width)
    });
    let leading_line = shape(&leading, window, &text);
    let (leading_y, trailing_y) = inline_origins(
        inline.bounds.size.height,
        (
            leading.line_height,
            baseline(
                leading.line_height,
                f32::from(leading_line.ascent),
                f32::from(leading_line.descent),
            ),
        ),
        (
            trailing.line_height,
            baseline(
                trailing.line_height,
                f32::from(trailing_line.ascent),
                f32::from(trailing_line.descent),
            ),
        ),
    );
    let mut result = Vec::with_capacity(2);
    if leading_width > 0.0 {
        let leading_bounds = core::Rect {
            origin: inline.bounds.origin,
            size: core::Size {
                width: leading_width,
                height: inline.bounds.size.height,
            },
        };
        result.push(WorldTextPaint {
            clip: Some(
                inline
                    .clip
                    .map_or(leading_bounds, |clip| intersect(leading_bounds, clip)),
            ),
            origin: core::Point::new(inline.bounds.origin.x, inline.bounds.origin.y + leading_y),
            line_height: leading.line_height,
            line: leading_line,
        });
    }
    result.push(WorldTextPaint {
        clip: inline.clip,
        origin: core::Point::new(
            inline.bounds.origin.x + trailing_x,
            inline.bounds.origin.y + trailing_y,
        ),
        line_height: trailing.line_height,
        line: trailing_line,
    });
    result
}

#[cfg(test)]
mod tests {
    use super::{baseline, ellipsize, inline_origins, inline_widths, intersect, row_widths};

    #[test]
    fn values_take_natural_width_before_labels() {
        assert_eq!(row_widths(248.0, 14.0, 6.0), (228.0, 14.0));
        assert_eq!(row_widths(248.0, 800.0, 6.0), (0.0, 242.0));
        assert_eq!(row_widths(0.0, 14.0, 6.0), (0.0, 0.0));
        assert_eq!(row_widths(248.0, 800.0, 0.0), (0.0, 248.0));
    }

    #[test]
    fn inline_labels_pack_adjacent_and_reserve_the_trailing_span() {
        assert_eq!(inline_widths(276.0, 37.5, 72.0, 6.0), (37.5, 43.5));
        assert_eq!(inline_widths(100.0, 200.0, 72.0, 6.0), (22.0, 28.0));
        assert_eq!(inline_widths(40.0, 37.5, 72.0, 6.0), (0.0, 6.0));
        assert_eq!(inline_widths(100.0, 200.0, 72.0, -6.0), (28.0, 28.0));
    }

    #[test]
    fn differently_sized_fonts_share_a_baseline() {
        let leading_baseline = baseline(14.0, 10.0, 3.0);
        let trailing_baseline = baseline(14.0, 8.0, 2.0);
        assert_eq!(leading_baseline, 10.5);
        assert_eq!(trailing_baseline, 10.0);
        assert_eq!(
            inline_origins(14.5, (14.0, leading_baseline), (14.0, trailing_baseline)),
            (0.0, 0.5)
        );
    }

    #[test]
    fn the_outer_clip_also_limits_the_leading_span() {
        let bounds = crate::core::Rect {
            origin: crate::core::Point::new(12.0, 42.0),
            size: crate::core::Size {
                width: 100.0,
                height: 14.0,
            },
        };
        let clip = crate::core::Rect {
            origin: crate::core::Point::new(20.0, 44.0),
            size: crate::core::Size {
                width: 30.0,
                height: 8.0,
            },
        };
        assert_eq!(intersect(bounds, clip), clip);
    }

    fn width(text: &str) -> f32 {
        text.chars().count() as f32
    }

    #[test]
    fn labels_keep_punctuation_and_full_documents() {
        let original = "file/path.data";
        assert_eq!(ellipsize(original, 6.0, width), "file/…");
        assert_eq!(ellipsize(original, 14.0, width), original);
        assert_eq!(original, "file/path.data");
        assert_eq!(ellipsize("", 0.0, width), "");
        assert_eq!(ellipsize("abc", 0.0, width), "…");
    }

    #[test]
    fn ellipsis_does_not_split_combining_marks_or_joined_emoji() {
        assert_eq!(ellipsize("a\u{301}bc", 3.0, width), "a\u{301}…");
        assert_eq!(ellipsize("👩‍🚀xyz", 3.0, width), "…");
        assert_eq!(ellipsize("👩‍🚀xyz", 4.0, width), "👩‍🚀…");
    }
}
