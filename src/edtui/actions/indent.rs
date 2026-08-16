//! Visual-mode indentation: shift the selected lines by one indent level.
//!
//! Indentation is expressed in spaces (`tab_width` spaces per level), never a
//! literal tab, matching the editor's space-based indent convention.

use super::Execute;
use crate::edtui::{EditorMode, EditorState, Index2};
use jagged::index::RowIndex;

/// Indent every line in the visual selection by one level.
#[derive(Clone, Debug)]
pub struct IndentSelection;

impl Execute for IndentSelection {
    fn execute(&mut self, state: &mut EditorState) {
        shift_selection(state, Shift::Right);
    }
}

/// Dedent every line in the visual selection by one level.
#[derive(Clone, Debug)]
pub struct DedentSelection;

impl Execute for DedentSelection {
    fn execute(&mut self, state: &mut EditorState) {
        shift_selection(state, Shift::Left);
    }
}

#[derive(Clone, Copy)]
enum Shift {
    Right,
    Left,
}

/// Shift every selected line by one indent level, then collapse the selection
/// and return to normal mode with the cursor on the first shifted line (Vim
/// behaviour). No-op outside visual mode or with no selection.
fn shift_selection(state: &mut EditorState, dir: Shift) {
    if state.mode != EditorMode::Visual {
        return;
    }
    let Some(selection) = &state.selection else {
        return;
    };
    let start = selection.start();
    let end = selection.end();
    state.capture();

    let width = state.view.tab_width.max(1);
    for row in start.row..=end.row {
        let Some(line) = state.lines.get_mut(RowIndex::new(row)) else {
            continue;
        };
        match dir {
            // Don't add trailing whitespace to otherwise-blank lines.
            Shift::Right if !line.is_empty() => {
                line.splice(0..0, std::iter::repeat_n(' ', width));
            }
            Shift::Right => {}
            Shift::Left => remove_one_indent(line, width),
        }
    }

    state.selection = None;
    state.mode = EditorMode::Normal;
    state.cursor = Index2::new(start.row, 0);
    state.clamp_column();
}

/// Remove one indent level from the front of `line`: a single leading tab, or
/// up to `width` leading spaces.
fn remove_one_indent(line: &mut Vec<char>, width: usize) {
    if line.first() == Some(&'\t') {
        line.remove(0);
        return;
    }
    let spaces = line.iter().take(width).take_while(|&&c| c == ' ').count();
    line.drain(0..spaces);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edtui::{Lines, actions::SwitchMode, state::selection::Selection};

    /// Put the editor in visual mode with rows `from..=to` selected.
    fn visual_select(text: &str, from: usize, to: usize) -> EditorState {
        let mut state = EditorState::new(Lines::from(text));
        state.view.tab_width = 2;
        SwitchMode(EditorMode::Visual).execute(&mut state);
        state.selection = Some(Selection::new(Index2::new(from, 0), Index2::new(to, 0)));
        state
    }

    fn line(state: &EditorState, row: usize) -> String {
        state
            .lines
            .get(RowIndex::new(row))
            .map(|c| c.iter().collect())
            .unwrap_or_default()
    }

    #[test]
    fn indent_prepends_tab_width_spaces_to_each_selected_line() {
        let mut s = visual_select("foo\nbar\nbaz", 0, 1);
        IndentSelection.execute(&mut s);
        assert_eq!(line(&s, 0), "  foo");
        assert_eq!(line(&s, 1), "  bar");
        assert_eq!(line(&s, 2), "baz"); // outside the selection, untouched
        assert_eq!(s.mode, EditorMode::Normal);
        assert!(s.selection.is_none());
    }

    #[test]
    fn indent_uses_spaces_never_a_tab() {
        let mut s = visual_select("x", 0, 0);
        IndentSelection.execute(&mut s);
        assert_eq!(line(&s, 0), "  x");
        assert!(!line(&s, 0).contains('\t'));
    }

    #[test]
    fn indent_skips_blank_lines() {
        let mut s = visual_select("a\n\nb", 0, 2);
        IndentSelection.execute(&mut s);
        assert_eq!(line(&s, 0), "  a");
        assert_eq!(line(&s, 1), ""); // blank line gains no trailing whitespace
        assert_eq!(line(&s, 2), "  b");
    }

    #[test]
    fn dedent_removes_up_to_tab_width_leading_spaces() {
        let mut s = visual_select("    four\n  two\nzero", 0, 2);
        DedentSelection.execute(&mut s);
        assert_eq!(line(&s, 0), "  four"); // 4 → 2 (one level)
        assert_eq!(line(&s, 1), "two"); // 2 → 0
        assert_eq!(line(&s, 2), "zero"); // nothing to remove
    }

    #[test]
    fn dedent_removes_a_single_leading_tab() {
        let mut s = visual_select("\ttabbed", 0, 0);
        DedentSelection.execute(&mut s);
        assert_eq!(line(&s, 0), "tabbed");
    }

    #[test]
    fn indent_then_dedent_round_trips() {
        let mut s = visual_select("alpha\nbeta", 0, 1);
        IndentSelection.execute(&mut s);
        // Re-select (indent collapsed the selection) and dedent back.
        SwitchMode(EditorMode::Visual).execute(&mut s);
        s.selection = Some(Selection::new(Index2::new(0, 0), Index2::new(1, 0)));
        DedentSelection.execute(&mut s);
        assert_eq!(line(&s, 0), "alpha");
        assert_eq!(line(&s, 1), "beta");
    }
}
