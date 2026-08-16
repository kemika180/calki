//! Header-section folding: collapse Markdown header sections in the editor.
//!
//! A folded section keeps its header row visible (rendered as a fold marker)
//! and hides the body rows below it, down to the next header of the same or
//! lower level (or end of buffer). Fold state lives on [`EditorState::folded`]
//! keyed by header row; extents are recomputed from live header positions on
//! every query so the model stays correct as the buffer changes.

use crate::edtui::EditorState;
use crate::highlight::header_level;

/// A currently-collapsed header section.
///
/// The header itself (`header_row`) stays visible as a fold marker; the body
/// rows `header_row + 1 .. end_row` are hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FoldRegion {
    pub header_row: usize,
    pub end_row: usize,
}

impl FoldRegion {
    /// Number of hidden body lines below the header.
    pub(crate) fn hidden_count(&self) -> usize {
        self.end_row - self.header_row - 1
    }
}

impl EditorState {
    /// All Markdown header rows with their levels (1 for `#`, 2 for `##`, …),
    /// in ascending row order.
    fn header_rows(&self) -> Vec<(usize, usize)> {
        self.lines
            .iter_row()
            .enumerate()
            .filter_map(|(row, chars)| header_level(chars).map(|level| (row, level)))
            .collect()
    }

    /// Exclusive end row of the section owned by `headers[idx]`: the first row
    /// at or after a following header of level `<=` this header's level, or the
    /// buffer length if none follows.
    fn section_end(headers: &[(usize, usize)], idx: usize, num_rows: usize) -> usize {
        let level = headers[idx].1;
        headers[idx + 1..]
            .iter()
            .find(|(_, other_level)| *other_level <= level)
            .map_or(num_rows, |(row, _)| *row)
    }

    /// The header sections currently folded, each with its computed extent.
    ///
    /// Only headers still present in the buffer and enclosing at least one body
    /// row are returned; stale or empty entries are skipped (but not pruned).
    pub(crate) fn folded_regions(&self) -> Vec<FoldRegion> {
        if self.folded.is_empty() {
            return Vec::new();
        }
        let headers = self.header_rows();
        let num_rows = self.lines.len();
        headers
            .iter()
            .enumerate()
            .filter(|(_, (row, _))| self.folded.contains(row))
            .filter_map(|(idx, (row, _))| {
                let end_row = Self::section_end(&headers, idx, num_rows);
                (end_row > row + 1).then_some(FoldRegion {
                    header_row: *row,
                    end_row,
                })
            })
            .collect()
    }

    /// Toggle the fold of the header section enclosing (or at) the cursor.
    ///
    /// Targets the innermost section, i.e. the nearest header at or above the
    /// cursor row. When collapsing a section the cursor sits below, the cursor
    /// is pulled up to the header so it is never stranded inside hidden rows.
    pub(crate) fn toggle_fold_at_cursor(&mut self) {
        let cursor_row = self.cursor.row;
        let Some((header_row, _)) = self
            .header_rows()
            .into_iter()
            .take_while(|(row, _)| *row <= cursor_row)
            .last()
        else {
            return;
        };

        if self.folded.contains(&header_row) {
            self.folded.remove(&header_row);
        } else {
            self.folded.insert(header_row);
            if cursor_row > header_row {
                self.cursor.row = header_row;
                self.clamp_column();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::edtui::{EditorState, EditorView, Index2, Lines};
    use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget};

    fn state(text: &str) -> EditorState {
        EditorState::new(Lines::from(text))
    }

    /// Render `state` into a fixed area and return the visible text of each row.
    fn rendered_rows(state: &mut EditorState) -> Vec<String> {
        let area = Rect::new(0, 0, 40, 10);
        let mut buf = Buffer::empty(area);
        EditorView::new(state).render(area, &mut buf);
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
    fn folded_regions_computes_extent_to_next_same_level_header() {
        // # A (0), body (1,2), # B (3)
        let mut s = state("# A\nbody1\nbody2\n# B\nbody3");
        s.folded.insert(0);
        let regions = s.folded_regions();
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].header_row, 0);
        assert_eq!(regions[0].end_row, 3); // stops at "# B"
        assert_eq!(regions[0].hidden_count(), 2);
    }

    #[test]
    fn subheader_does_not_close_a_higher_level_section() {
        // # A (0), ## sub (1), body (2), # B (3): folding A swallows the ## section.
        let mut s = state("# A\n## sub\nbody\n# B");
        s.folded.insert(0);
        let regions = s.folded_regions();
        assert_eq!(regions[0].end_row, 3);
        assert_eq!(regions[0].hidden_count(), 2); // "## sub" + "body"
    }

    #[test]
    fn last_section_extends_to_end_of_buffer() {
        let mut s = state("intro\n# A\nb1\nb2");
        s.folded.insert(1);
        let regions = s.folded_regions();
        assert_eq!(regions[0].end_row, 4);
        assert_eq!(regions[0].hidden_count(), 2);
    }

    #[test]
    fn empty_section_is_not_a_region() {
        // Header immediately followed by another same-level header: nothing to hide.
        let mut s = state("# A\n# B");
        s.folded.insert(0);
        assert!(s.folded_regions().is_empty());
    }

    #[test]
    fn toggle_on_header_folds_and_unfolds() {
        let mut s = state("# A\nb1\nb2");
        s.cursor = Index2::new(0, 0);
        s.toggle_fold_at_cursor();
        assert!(s.folded.contains(&0));
        s.toggle_fold_at_cursor();
        assert!(s.folded.is_empty());
    }

    #[test]
    fn toggle_inside_section_folds_enclosing_header_and_pulls_cursor_up() {
        let mut s = state("# A\nb1\nb2");
        s.cursor = Index2::new(2, 0); // inside the body
        s.toggle_fold_at_cursor();
        assert!(s.folded.contains(&0));
        assert_eq!(s.cursor.row, 0); // cursor pulled to the header
    }

    #[test]
    fn toggle_targets_innermost_header() {
        // # A (0), ## sub (1), body (2). Cursor in body → folds ## sub, not # A.
        let mut s = state("# A\n## sub\nbody");
        s.cursor = Index2::new(2, 0);
        s.toggle_fold_at_cursor();
        assert!(s.folded.contains(&1));
        assert!(!s.folded.contains(&0));
    }

    #[test]
    fn render_hides_body_and_paints_marker() {
        let mut s = state("# A\nbody1\nbody2\n# B\nbody3");

        // Unfolded: every buffer line is painted.
        let rows = rendered_rows(&mut s);
        assert_eq!(&rows[0], "# A");
        assert_eq!(&rows[1], "body1");
        assert_eq!(&rows[3], "# B");

        // Folded: "# A"'s body collapses to a marker; "# B" shifts up into its place.
        s.folded.insert(0);
        let rows = rendered_rows(&mut s);
        assert_eq!(&rows[0], "▶ # A (2 lines folded)");
        assert_eq!(&rows[1], "# B");
        assert_eq!(&rows[2], "body3");
        // The hidden body text is gone from the viewport entirely.
        assert!(rows.iter().all(|r| r != "body1" && r != "body2"));
    }

    #[test]
    fn render_marker_singular_line_count() {
        let mut s = state("# A\nonly-body");
        s.folded.insert(0);
        let rows = rendered_rows(&mut s);
        assert_eq!(&rows[0], "▶ # A (1 line folded)");
    }
}
