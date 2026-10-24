//! Markup concealment: hide declared column ranges on lines the cursor is not on.
//!
//! The host declares, per buffer row, half-open char ranges to hide
//! ([`EditorState::set_conceal_ranges`]); edtui knows nothing about what the
//! ranges mean. A row the cursor is on, or that an active selection touches,
//! always renders raw, so editing happens against real geometry.
//!
//! [`ColumnMap`] is the per-row buffer↔display column mapping that render and
//! mouse hit-testing share, the column counterpart of
//! [`super::fold::VisibleLines`] for rows.

use crate::edtui::EditorState;
use jagged::index::RowIndex;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ops::Range;

/// Hidden char ranges keyed by buffer row.
pub(crate) type ConcealRanges = HashMap<usize, Vec<Range<usize>>>;

/// Maps between buffer columns and display indices on one line.
///
/// A display index counts only the chars that are drawn. With nothing hidden
/// the mapping is the identity.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ColumnMap {
    Identity,
    Concealed {
        /// Buffer columns that are drawn, in order.
        visible: Vec<usize>,
        /// Number of chars in the buffer line.
        line_len: usize,
    },
}

impl ColumnMap {
    /// Builds the map for a line of `line_len` chars with `hidden` ranges.
    /// Ranges are clamped to the line; if nothing remains hidden the map is
    /// [`ColumnMap::Identity`].
    pub(crate) fn new(line_len: usize, hidden: &[Range<usize>]) -> Self {
        let mut is_hidden = vec![false; line_len];
        for range in hidden {
            let end = range.end.min(line_len);
            if range.start < end {
                is_hidden[range.start..end].fill(true);
            }
        }
        if !is_hidden.contains(&true) {
            return Self::Identity;
        }
        let visible = (0..line_len).filter(|&col| !is_hidden[col]).collect();
        Self::Concealed { visible, line_len }
    }

    pub(crate) fn is_identity(&self) -> bool {
        matches!(self, Self::Identity)
    }

    /// Whether the char at `buf_col` is hidden.
    pub(crate) fn is_hidden(&self, buf_col: usize) -> bool {
        match self {
            Self::Identity => false,
            Self::Concealed { visible, line_len } => {
                buf_col < *line_len && visible.binary_search(&buf_col).is_err()
            }
        }
    }

    /// The display index of `buf_col`. A hidden column maps to the next drawn
    /// char; columns past the end of the line keep their distance from it.
    pub(crate) fn to_display(&self, buf_col: usize) -> usize {
        match self {
            Self::Identity => buf_col,
            Self::Concealed { visible, line_len } => {
                if buf_col >= *line_len {
                    visible.len() + (buf_col - line_len)
                } else {
                    visible.partition_point(|&col| col < buf_col)
                }
            }
        }
    }

    /// The buffer column drawn at `display_idx`. Indices past the last drawn
    /// char map past the end of the line, mirroring [`Self::to_display`].
    pub(crate) fn to_buffer(&self, display_idx: usize) -> usize {
        match self {
            Self::Identity => display_idx,
            Self::Concealed { visible, line_len } => visible
                .get(display_idx)
                .copied()
                .unwrap_or_else(|| line_len + (display_idx - visible.len())),
        }
    }

    /// The chars of `line` that are drawn, in order.
    pub(crate) fn display_chars<'l>(&self, line: &'l [char]) -> Cow<'l, [char]> {
        match self {
            Self::Identity => Cow::Borrowed(line),
            Self::Concealed { visible, .. } => Cow::Owned(
                visible
                    .iter()
                    .filter_map(|&col| line.get(col).copied())
                    .collect(),
            ),
        }
    }
}

impl EditorState {
    /// Replaces the hidden char ranges, keyed by buffer row (half-open ranges
    /// of char indices). An empty map turns concealment off.
    pub fn set_conceal_ranges(&mut self, ranges: ConcealRanges) {
        self.conceal = ranges;
    }

    /// Fast-path guard: whether any row declares hidden ranges.
    pub(crate) fn has_conceal(&self) -> bool {
        !self.conceal.is_empty()
    }

    /// Whether `row` renders raw: the cursor row and any row of the active
    /// selection are never concealed.
    pub(crate) fn is_row_revealed(&self, row: usize) -> bool {
        row == self.cursor.row
            || self
                .selection
                .as_ref()
                .is_some_and(|selection| selection.contains_row(row))
    }

    /// The column mapping `row` is drawn with right now.
    pub(crate) fn column_map(&self, row: usize) -> ColumnMap {
        let Some(hidden) = self.conceal.get(&row) else {
            return ColumnMap::Identity;
        };
        if self.is_row_revealed(row) {
            return ColumnMap::Identity;
        }
        let line_len = self.lines.get(RowIndex::new(row)).map_or(0, Vec::len);
        ColumnMap::new(line_len, hidden)
    }
}

/// Test helper: hides every backtick on every row of `text`.
#[cfg(test)]
pub(crate) fn every_backtick_hidden(text: &str) -> ConcealRanges {
    text.lines()
        .enumerate()
        .filter_map(|(row, line)| {
            let ticks: Vec<Range<usize>> = line
                .chars()
                .enumerate()
                .filter(|&(_, c)| c == '`')
                .map(|(i, _)| i..i + 1)
                .collect();
            (!ticks.is_empty()).then_some((row, ticks))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edtui::state::selection::Selection;
    use crate::edtui::{Index2, Lines};

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    /// "a `b` c" with both backticks (cols 2 and 4) hidden.
    fn backtick_map() -> ColumnMap {
        ColumnMap::new(7, &[2..3, 4..5])
    }

    #[test]
    fn identity_when_no_ranges() {
        assert!(ColumnMap::new(5, &[]).is_identity());
        assert!(ColumnMap::new(5, std::slice::from_ref(&(2..2))).is_identity());
    }

    #[test]
    fn identity_when_ranges_out_of_bounds() {
        assert!(ColumnMap::new(3, &[3..5, 10..12]).is_identity());
    }

    #[test]
    fn hides_backticks() {
        let map = backtick_map();
        let line = chars("a `b` c");
        let shown: String = map.display_chars(&line).iter().collect();
        assert_eq!(shown, "a b c");
        assert!(map.is_hidden(2));
        assert!(map.is_hidden(4));
        assert!(!map.is_hidden(3));
        assert!(!map.is_hidden(7));
    }

    #[test]
    fn to_display_of_hidden_col_is_next_visible() {
        let map = backtick_map();
        // Display: a(0) ' '(1) b(2) ' '(3) c(4); col 2 is hidden → next is 'b'.
        assert_eq!(map.to_display(2), 2);
        assert_eq!(map.to_display(3), 2);
        assert_eq!(map.to_display(5), 3);
    }

    #[test]
    fn to_buffer_past_end_is_line_len() {
        let map = backtick_map();
        assert_eq!(map.to_buffer(5), 7);
        assert_eq!(map.to_buffer(6), 8);
        assert_eq!(map.to_display(7), 5);
    }

    #[test]
    fn roundtrip_visible_cols() {
        let map = backtick_map();
        for col in [0, 1, 3, 5, 6] {
            assert_eq!(map.to_buffer(map.to_display(col)), col);
        }
        for idx in 0..5 {
            assert_eq!(map.to_display(map.to_buffer(idx)), idx);
        }
    }

    #[test]
    fn multibyte_char_indices() {
        // "é`ü`" — indices are chars, not bytes.
        let line = chars("é`ü`x");
        let map = ColumnMap::new(line.len(), &[1..2, 3..4]);
        let shown: String = map.display_chars(&line).iter().collect();
        assert_eq!(shown, "éüx");
        assert_eq!(map.to_buffer(2), 4);
    }

    fn concealed_state() -> EditorState {
        let mut state = EditorState::new(Lines::from("a `b` c\nd `e` f\ng `h` i"));
        let ranges: ConcealRanges = (0..3).map(|row| (row, vec![2..3, 4..5])).collect();
        state.set_conceal_ranges(ranges);
        state
    }

    #[test]
    fn cursor_row_revealed() {
        let mut state = concealed_state();
        state.cursor = Index2::new(1, 0);
        assert!(state.column_map(1).is_identity());
        assert!(!state.column_map(0).is_identity());
        assert!(!state.column_map(2).is_identity());
    }

    #[test]
    fn selection_rows_revealed() {
        let mut state = concealed_state();
        state.selection = Some(Selection::new(Index2::new(1, 3), Index2::new(2, 1)));
        state.cursor = Index2::new(2, 1);
        assert!(!state.column_map(0).is_identity());
        assert!(state.column_map(1).is_identity());
        assert!(state.column_map(2).is_identity());
    }

    #[test]
    fn column_map_fast_path_when_empty() {
        let mut state = concealed_state();
        assert!(state.has_conceal());
        state.set_conceal_ranges(ConcealRanges::new());
        assert!(!state.has_conceal());
        assert!(state.column_map(2).is_identity());
    }
}
