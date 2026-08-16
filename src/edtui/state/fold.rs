//! Header-section folding: collapse Markdown header sections in the editor.
//!
//! A folded section keeps its header row visible (rendered as a fold marker)
//! and hides the body rows below it, down to the next header of the same or
//! lower level (or end of buffer). Fold state lives on [`EditorState::folded`]
//! keyed by header row; extents are recomputed from live header positions on
//! every query so the model stays correct as the buffer changes.

use crate::edtui::EditorMode;
use crate::edtui::EditorState;
use crate::edtui::state::selection::set_selection_with_lines;
use crate::highlight::header_level;
use std::collections::HashSet;

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

/// The set of rows hidden by a list of fold regions. Shared by the render path
/// and [`EditorState::hidden_rows`] so the two derivations never drift.
pub(crate) fn hidden_rows_from(regions: &[FoldRegion]) -> HashSet<usize> {
    regions
        .iter()
        .flat_map(|r| (r.header_row + 1)..r.end_row)
        .collect()
}

/// The buffer↔screen line mapping: the ordered buffer rows that are actually
/// visible (hidden fold bodies removed; folded headers kept as markers).
///
/// Every consumer that translates between buffer rows and on-screen rows —
/// rendering, cursor motions, mouse hit-testing, viewport scrolling — routes
/// through this so folds are handled in exactly one place instead of each
/// caller re-deriving the row geometry (and getting it subtly wrong).
pub(crate) struct VisibleLines {
    /// Visible buffer rows in ascending order; the index is the on-screen
    /// ordinal (0 = first rendered row).
    rows: Vec<usize>,
}

impl VisibleLines {
    /// Number of visible rows.
    pub(crate) fn len(&self) -> usize {
        self.rows.len()
    }

    /// The buffer row shown at on-screen ordinal `visible_idx`, if any.
    pub(crate) fn to_buffer(&self, visible_idx: usize) -> Option<usize> {
        self.rows.get(visible_idx).copied()
    }

    /// The on-screen ordinal of `buffer_row`, or `None` if it is hidden.
    pub(crate) fn to_visible(&self, buffer_row: usize) -> Option<usize> {
        self.rows.binary_search(&buffer_row).ok()
    }

    /// The visible buffer row nearest to `buffer_row`: `buffer_row` itself if
    /// visible, otherwise the fold header just above it (the greatest visible
    /// row below it), falling back to the first visible row. Keeps the cursor
    /// out of hidden regions regardless of how it was placed.
    pub(crate) fn snap(&self, buffer_row: usize) -> usize {
        match self.rows.binary_search(&buffer_row) {
            Ok(_) => buffer_row,
            // `pos` is the insertion point: rows[pos-1] < buffer_row < rows[pos].
            Err(pos) if pos > 0 => self.rows[pos - 1],
            Err(_) => self.rows.first().copied().unwrap_or(buffer_row),
        }
    }

    /// On-screen ordinal of the first visible row at or after `buffer_row`
    /// (the viewport top may point at a hidden row; the render starts painting
    /// at the first visible row from there).
    pub(crate) fn ordinal_at_or_after(&self, buffer_row: usize) -> Option<usize> {
        match self.rows.binary_search(&buffer_row) {
            Ok(pos) => Some(pos),
            Err(pos) => (pos < self.rows.len()).then_some(pos),
        }
    }

    /// The next visible buffer row strictly after `buffer_row`, if any.
    pub(crate) fn next_after(&self, buffer_row: usize) -> Option<usize> {
        let pos = match self.rows.binary_search(&buffer_row) {
            Ok(p) => p + 1,
            Err(p) => p,
        };
        self.rows.get(pos).copied()
    }

    /// The previous visible buffer row strictly before `buffer_row`, if any.
    pub(crate) fn prev_before(&self, buffer_row: usize) -> Option<usize> {
        let pos = match self.rows.binary_search(&buffer_row) {
            Ok(p) | Err(p) => p,
        };
        (pos > 0).then(|| self.rows[pos - 1])
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

    /// The rows currently hidden inside collapsed folds.
    pub(crate) fn hidden_rows(&self) -> HashSet<usize> {
        hidden_rows_from(&self.folded_regions())
    }

    /// The current buffer↔screen line mapping (see [`VisibleLines`]).
    pub(crate) fn visible_lines(&self) -> VisibleLines {
        let regions = self.folded_regions();
        let rows = if regions.is_empty() {
            (0..self.lines.len()).collect()
        } else {
            let hidden = hidden_rows_from(&regions);
            (0..self.lines.len())
                .filter(|r| !hidden.contains(r))
                .collect()
        };
        VisibleLines { rows }
    }

    /// If `row` falls inside a collapsed fold, the visible header row of that
    /// fold; otherwise `row` unchanged. Used to keep the cursor out of hidden
    /// regions after absolute jumps.
    pub(crate) fn visible_anchor_row(&self, row: usize) -> usize {
        self.folded_regions()
            .iter()
            .find(|r| row > r.header_row && row < r.end_row)
            .map_or(row, |r| r.header_row)
    }

    /// Snap the cursor onto the nearest visible row if it landed inside a
    /// collapsed fold — the single normalization every input path runs after
    /// mutating the cursor, so no motion, search, or jump can strand the cursor
    /// on a hidden row. Cheap no-op when nothing is folded.
    pub(crate) fn normalize_cursor_visible(&mut self) {
        if self.folded.is_empty() {
            return;
        }
        let snapped = self.visible_lines().snap(self.cursor.row);
        if snapped != self.cursor.row {
            self.cursor.row = snapped;
            self.clamp_column();
            if self.mode == EditorMode::Visual {
                set_selection_with_lines(&mut self.selection, self.cursor, &self.lines);
            }
        }
    }

    /// Drop fold entries whose row is no longer a Markdown header — e.g. after
    /// the buffer was rebuilt or lines were inserted/deleted above a fold,
    /// shifting header positions. Folds still anchored on a header are kept.
    ///
    /// This prevents stale row indices from lingering (and a later header
    /// reappearing at a stale index from surprise-folding). Called wherever the
    /// buffer may have changed structurally.
    pub(crate) fn reconcile_folds(&mut self) {
        if self.folded.is_empty() {
            return;
        }
        let headers: HashSet<usize> = self.header_rows().into_iter().map(|(row, _)| row).collect();
        self.folded.retain(|row| headers.contains(row));
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

    #[test]
    fn move_down_skips_folded_body() {
        use crate::edtui::actions::{Execute, MoveDown, MoveUp};
        // # A (0), b1 (1), b2 (2), # B (3), c1 (4); fold A → 1,2 hidden.
        let mut s = state("# A\nb1\nb2\n# B\nc1");
        s.folded.insert(0);
        s.cursor = Index2::new(0, 0);

        MoveDown(1).execute(&mut s);
        assert_eq!(s.cursor.row, 3); // jumped over hidden b1/b2 to "# B"

        MoveUp(1).execute(&mut s);
        assert_eq!(s.cursor.row, 0); // back to the header, not into the fold
    }

    #[test]
    fn reconcile_prunes_entries_no_longer_on_a_header() {
        let mut s = state("# A\nbody\n# B\nbody");
        s.folded.insert(0);
        s.folded.insert(2);

        // Row 2 stops being a header (edited to plain text); row 0 unchanged.
        s.lines = Lines::from("# A\nbody\nBudget\nbody");
        s.reconcile_folds();

        assert!(s.folded.contains(&0)); // still a header → kept
        assert!(!s.folded.contains(&2)); // no longer a header → pruned
    }

    #[test]
    fn reconcile_keeps_folds_when_headers_unchanged() {
        let mut s = state("# A\nb1\n# B\nb2");
        s.folded.insert(0);
        s.folded.insert(2);
        // A content-only rebuild (math result appended) leaves headers in place.
        s.lines = Lines::from("# A\nb1 => 1\n# B\nb2 => 2");
        s.reconcile_folds();
        assert!(s.folded.contains(&0));
        assert!(s.folded.contains(&2));
    }

    #[test]
    fn visible_lines_maps_between_buffer_and_screen() {
        // # A (0), b1 (1), b2 (2), # B (3), c1 (4); fold A hides rows 1,2.
        let mut s = state("# A\nb1\nb2\n# B\nc1");
        s.folded.insert(0);
        let vl = s.visible_lines();

        assert_eq!(vl.len(), 3); // rows 0, 3, 4 visible
        assert_eq!(vl.to_buffer(0), Some(0)); // screen row 0 → "# A"
        assert_eq!(vl.to_buffer(1), Some(3)); // screen row 1 → "# B"
        assert_eq!(vl.to_buffer(2), Some(4)); // screen row 2 → "c1"
        assert_eq!(vl.to_buffer(3), None);

        assert_eq!(vl.to_visible(3), Some(1)); // "# B" is the 2nd visible row
        assert_eq!(vl.to_visible(1), None); // hidden body row has no screen ordinal

        // Hidden rows snap up to their fold header; visible rows are unchanged.
        assert_eq!(vl.snap(1), 0);
        assert_eq!(vl.snap(2), 0);
        assert_eq!(vl.snap(3), 3);
    }

    #[test]
    fn normalize_cursor_pulls_out_of_fold() {
        let mut s = state("# A\nb1\nb2\n# B\nc1");
        s.folded.insert(0);
        s.cursor = Index2::new(2, 0); // stranded inside the fold (any jump/search)
        s.normalize_cursor_visible();
        assert_eq!(s.cursor.row, 0); // snapped onto the visible header

        // No-op when the cursor is already visible.
        s.cursor = Index2::new(3, 0);
        s.normalize_cursor_visible();
        assert_eq!(s.cursor.row, 3);
    }

    #[test]
    fn move_to_last_row_snaps_out_of_trailing_fold() {
        use crate::edtui::actions::Execute;
        use crate::edtui::actions::motion::MoveToLastRow;
        // intro (0), # A (1), b1 (2), b2 (3); fold A → 2,3 hidden, last row hidden.
        let mut s = state("intro\n# A\nb1\nb2");
        s.folded.insert(1);
        s.cursor = Index2::new(0, 0);

        MoveToLastRow().execute(&mut s);
        assert_eq!(s.cursor.row, 1); // snapped to the visible header, not hidden row 3
    }
}
