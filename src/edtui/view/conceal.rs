//! Drops concealed chars from a row's styled spans (see
//! [`crate::edtui::state::conceal`]).
//!
//! Concealment runs after styling, so selection, search and highlight styles,
//! which are all keyed by buffer position, apply exactly as on a raw row.

use crate::edtui::state::conceal::ColumnMap;
use ratatui_core::text::Span;

/// Removes the chars `map` hides from `spans`. `first_col` is the buffer
/// column of the first char in `spans` (the horizontal scroll offset).
pub(crate) fn conceal_spans<'a>(
    spans: Vec<Span<'a>>,
    map: &ColumnMap,
    first_col: usize,
) -> Vec<Span<'a>> {
    if map.is_identity() {
        return spans;
    }

    let mut col = first_col;
    spans
        .into_iter()
        .filter_map(|span| {
            let content: String = span
                .content
                .chars()
                .filter(|_| {
                    let hidden = map.is_hidden(col);
                    col += 1;
                    !hidden
                })
                .collect();
            (!content.is_empty()).then(|| Span::styled(content, span.style))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edtui::state::conceal::{ConcealRanges, every_backtick_hidden};
    use crate::edtui::state::selection::Selection;
    use crate::edtui::{EditorMode, EditorState, EditorView, Index2, Lines};
    use ratatui_core::{
        buffer::Buffer,
        layout::Rect,
        style::{Color, Style},
        widgets::Widget,
    };

    #[test]
    fn conceal_spans_keeps_styles_and_drops_hidden() {
        let red = Style::default().fg(Color::Red);
        let spans = vec![Span::raw("a "), Span::styled("`b`", red), Span::raw(" c")];
        let map = ColumnMap::new(7, &[2..3, 4..5]);
        assert_eq!(
            conceal_spans(spans, &map, 0),
            vec![Span::raw("a "), Span::styled("b", red), Span::raw(" c")]
        );
    }

    #[test]
    fn conceal_spans_respects_first_col() {
        // Scrolled by 2: the spans start at buffer col 2 (the opening backtick).
        let spans = vec![Span::raw("`b` c")];
        let map = ColumnMap::new(7, &[2..3, 4..5]);
        assert_eq!(conceal_spans(spans, &map, 2), vec![Span::raw("b c")]);
    }

    fn concealed_state(text: &str) -> EditorState {
        let mut state = EditorState::new(Lines::from(text));
        state.set_conceal_ranges(every_backtick_hidden(text));
        state
    }

    fn render(state: &mut EditorState, width: u16) -> Buffer {
        let area = Rect::new(0, 0, width, 6);
        let mut buf = Buffer::empty(area);
        EditorView::new(state).render(area, &mut buf);
        buf
    }

    fn rows(buf: &Buffer) -> Vec<String> {
        let area = buf.area;
        (0..area.height)
            .map(|y| {
                (0..area.width)
                    .filter_map(|x| buf.cell((x, y)).map(|c| c.symbol()))
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn render_conceals_backticks_on_non_cursor_rows() {
        let mut state = concealed_state("one `a`\ntwo `b`\nthree `c`");
        state.cursor = Index2::new(1, 0);
        let buf = render(&mut state, 20);
        assert_eq!(rows(&buf)[..3], ["one a", "two `b`", "three c"]);
    }

    #[test]
    fn render_cursor_row_raw() {
        let mut state = concealed_state("one `a`\ntwo `b`");
        let buf = render(&mut state, 20);
        assert_eq!(rows(&buf)[..2], ["one `a`", "two b"]);
    }

    #[test]
    fn render_reveals_selection_rows() {
        let mut state = concealed_state("one `a`\ntwo `b`\nthree `c`");
        state.mode = EditorMode::Visual;
        state.selection = Some(Selection::new(Index2::new(1, 0), Index2::new(2, 2)));
        state.cursor = Index2::new(2, 2);
        let buf = render(&mut state, 20);
        assert_eq!(rows(&buf)[..3], ["one a", "two `b`", "three `c`"]);
    }

    #[test]
    fn render_unchanged_without_conceal_ranges() {
        let text = "one `a`\ntwo `b`";
        let mut plain = EditorState::new(Lines::from(text));
        let mut empty = EditorState::new(Lines::from(text));
        empty.set_conceal_ranges(ConcealRanges::new());
        assert_eq!(render(&mut plain, 20), render(&mut empty, 20));
    }

    #[test]
    fn render_conceal_with_word_wrap() {
        // Concealed "aa `bb` cc" is "aa bb cc", which wraps at width 6 as "aa bb" / "cc".
        let mut state = concealed_state("x\naa `bb` cc");
        let buf = render(&mut state, 6);
        assert_eq!(rows(&buf)[..3], ["x", "aa bb", "cc"]);
    }

    #[test]
    fn render_conceal_with_horizontal_scroll() {
        // No wrap, scrolled 2 cols: row 1 skips "ab" then hides both backticks.
        let mut state = concealed_state("0123456789\nab`cd`ef");
        state.view.wrap = false;
        state.view.viewport.x = 2;
        state.cursor = Index2::new(0, 9);
        let buf = render(&mut state, 8);
        assert_eq!(rows(&buf)[..2], ["23456789", "cdef"]);
    }

    #[test]
    fn cursor_screen_position_unaffected_by_concealed_rows_above() {
        let mut state = concealed_state("`a` `b`\nhello");
        state.cursor = Index2::new(1, 3);
        render(&mut state, 20);
        let pos = state.cursor_screen_position().unwrap();
        assert_eq!((pos.x, pos.y), (3, 1));
    }

    #[test]
    fn search_highlight_on_concealed_row() {
        // The search-match style lands on 'b' by buffer col, then the backticks go.
        let state = concealed_state("x\na `b` c");
        let line: Vec<char> = "a `b` c".chars().collect();
        let matched = Some(Selection::new(Index2::new(1, 3), Index2::new(1, 3)));
        let selections = [&matched];
        let style = Style::default().bg(Color::Blue);
        let spans = super::super::internal::line_into_spans_with_selections(
            &line,
            &selections,
            &[],
            1,
            0,
            &Style::default(),
            &style,
        );
        let concealed = conceal_spans(spans, &state.column_map(1), 0);
        let text: String = concealed.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "a b c");
        assert!(
            concealed
                .iter()
                .any(|s| s.content == "b" && s.style == style)
        );
    }

    #[test]
    fn cursor_stays_visible_wrap_mode_with_concealed_rows_above() {
        // Raw rows are 12 chars (2 screen rows at width 10); concealed they are
        // 10 (1 row). The viewport measures raw rows, so it over-estimates the
        // height above the cursor; the cursor must still be drawn.
        let text: String = (0..8).map(|_| "`aaaaaaaaaa`\n").collect::<String>() + "end";
        let mut state = concealed_state(&text);
        state.cursor = Index2::new(8, 1);
        let area = Rect::new(0, 0, 10, 6);
        let mut buf = Buffer::empty(area);
        // Two frames, as in the app: edtui only scrolls once it knows how
        // many rows the previous frame showed.
        for _ in 0..2 {
            buf = Buffer::empty(area);
            EditorView::new(&mut state).render(area, &mut buf);
        }
        let pos = state.cursor_screen_position().unwrap();
        assert!(
            pos.y < area.height - 1,
            "cursor at {pos:?} is off the text area"
        );
        assert_eq!(buf[(pos.x, pos.y)].symbol(), "n");
    }
}
