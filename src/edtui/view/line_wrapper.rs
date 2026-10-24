use crate::edtui::helper::char_width;
use ratatui_core::{style::Style, text::Span};
use std::ops::Range;

#[derive(Default)]
pub(crate) struct LineWrapper;

impl LineWrapper {
    /// Word-wraps `chars` into screen rows no wider than `max_width`.
    ///
    /// Returns one half-open range of indices into `chars` per screen row.
    /// When a row breaks at a space, that space is not part of either row, so
    /// it shows up as a one-index gap between consecutive ranges. An empty
    /// input yields a single empty row.
    pub(crate) fn wrap_ranges(
        chars: &[char],
        max_width: usize,
        tab_width: usize,
    ) -> Vec<Range<usize>> {
        if chars.is_empty() {
            return std::iter::once(0..0).collect();
        }
        if max_width == 0 {
            return std::iter::once(0..chars.len()).collect();
        }

        let mut ranges = Vec::new();
        let mut row_start = 0;
        let mut row_width = 0;
        let mut last_space: Option<usize> = None;

        let mut i = 0;
        while i < chars.len() {
            let ch = chars[i];
            let ch_w = char_width(ch, tab_width);

            if row_width + ch_w <= max_width {
                row_width += ch_w;
                if ch == ' ' {
                    last_space = Some(i);
                }
                i += 1;
                continue;
            }

            if let Some(space) = last_space {
                // Break at the last space; the space itself is dropped. The
                // current char is reconsidered against the new row.
                ranges.push(row_start..space);
                row_start = space + 1;
                row_width = 0;
                last_space = None;
                for (j, &c) in chars.iter().enumerate().take(i).skip(row_start) {
                    row_width += char_width(c, tab_width);
                    if c == ' ' {
                        last_space = Some(j);
                    }
                }
            } else {
                // No space to break at: force a break before the current char.
                ranges.push(row_start..i);
                row_start = i;
                row_width = ch_w;
                last_space = (ch == ' ').then_some(i);
                i += 1;
            }
        }

        if row_start < chars.len() {
            ranges.push(row_start..chars.len());
        }

        ranges
    }

    /// Word-wraps styled spans into screen rows, keeping the char ranges that
    /// [`Self::wrap_ranges`] produced so positions can be mapped back.
    pub(crate) fn wrap_spans(
        spans: Vec<Span<'_>>,
        max_width: usize,
        tab_width: usize,
    ) -> WrappedLine<'static> {
        let char_styles: Vec<(char, Style)> = spans
            .iter()
            .flat_map(|span| span.content.chars().map(|ch| (ch, span.style)))
            .collect();
        let chars: Vec<char> = char_styles.iter().map(|&(ch, _)| ch).collect();
        let ranges = Self::wrap_ranges(&chars, max_width, tab_width);
        let rows = ranges
            .iter()
            .map(|range| styled_row(&char_styles[range.clone()]))
            .collect();

        WrappedLine { rows, ranges }
    }
}

/// A line split into screen rows.
///
/// `rows[i]` holds the spans drawn on screen row `i`, and `ranges[i]` the
/// indices of the chars it shows in the unwrapped input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WrappedLine<'a> {
    pub(crate) rows: Vec<Vec<Span<'a>>>,
    pub(crate) ranges: Vec<Range<usize>>,
}

/// Merges consecutive chars with the same style into spans.
fn styled_row(chars: &[(char, Style)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let Some(&(_, first_style)) = chars.first() else {
        return spans;
    };

    let mut current_style = first_style;
    let mut current_text = String::new();
    for &(ch, style) in chars {
        if style != current_style {
            spans.push(Span::styled(
                std::mem::take(&mut current_text),
                current_style,
            ));
            current_style = style;
        }
        current_text.push(ch);
    }
    spans.push(Span::styled(current_text, current_style));

    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui_core::style::Style;

    #[test]
    fn test_wrap_spans_force() {
        let spans = vec![Span::raw("Hello"), Span::raw("World")];
        let wrapped_spans = LineWrapper::wrap_spans(spans, 3, 0).rows;

        assert_eq!(
            wrapped_spans[0],
            vec![Span::styled("Hel", Style::default())]
        );
        assert_eq!(
            wrapped_spans[1],
            vec![Span::styled("loW", Style::default())]
        );
        assert_eq!(
            wrapped_spans[2],
            vec![Span::styled("orl", Style::default())]
        );
        assert_eq!(wrapped_spans[3], vec![Span::styled("d", Style::default())]);
    }

    #[test]
    fn test_wrap_spans_word_boundary() {
        let spans = vec![Span::raw("Hello "), Span::raw("World")];
        let wrapped_spans = LineWrapper::wrap_spans(spans, 7, 0).rows;

        assert_eq!(
            wrapped_spans[0],
            vec![Span::styled("Hello", Style::default())]
        );
        assert_eq!(
            wrapped_spans[1],
            vec![Span::styled("World", Style::default())]
        );
    }

    #[test]
    fn test_wrap_spans_with_emoji() {
        let spans = vec![Span::raw("Hell🙂!")];
        let wrapped_spans = LineWrapper::wrap_spans(spans, 4, 0).rows;

        assert_eq!(
            wrapped_spans[0],
            vec![Span::styled("Hell", Style::default())]
        );
        assert_eq!(
            wrapped_spans[1],
            vec![Span::styled("🙂!", Style::default())]
        );
    }

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn wrap_ranges_word_break_skips_space() {
        let ranges = LineWrapper::wrap_ranges(&chars("Hello World"), 7, 0);
        assert_eq!(ranges, vec![0..5, 6..11]);
    }

    #[test]
    fn wrap_ranges_force_wrap_is_contiguous() {
        let ranges = LineWrapper::wrap_ranges(&chars("HelloWorld"), 3, 0);
        assert_eq!(ranges, vec![0..3, 3..6, 6..9, 9..10]);
    }

    #[test]
    fn wrap_ranges_empty_line_is_one_empty_row() {
        assert_eq!(LineWrapper::wrap_ranges(&[], 10, 0), vec![0..0]);
    }

    #[test]
    fn wrap_ranges_multiple_breaks_gaps_are_spaces() {
        let line = chars("aa bb cc dd");
        let ranges = LineWrapper::wrap_ranges(&line, 6, 0);
        assert_eq!(ranges, vec![0..5, 6..11]);

        let ranges = LineWrapper::wrap_ranges(&line, 3, 0);
        assert_eq!(ranges, vec![0..2, 3..5, 6..8, 9..11]);
        for pair in ranges.windows(2) {
            assert_eq!(line[pair[0].end], ' ');
            assert_eq!(pair[0].end + 1, pair[1].start);
        }
    }

    #[test]
    fn wrap_spans_rows_match_ranges() {
        let spans = vec![Span::raw("Hello "), Span::raw("World")];
        let wrapped = LineWrapper::wrap_spans(spans, 7, 0);
        assert_eq!(wrapped.ranges, vec![0..5, 6..11]);
        assert_eq!(wrapped.rows.len(), wrapped.ranges.len());
    }
}
