//! Editor actions such as move, insert, delete
pub mod cpaste;
pub mod delete;
pub mod insert;
pub mod motion;
pub mod search;
pub mod select;
#[cfg(feature = "system-editor")]
pub mod system_editor;
use crate::edtui::state::selection::Selection;
use crate::edtui::{EditorMode, EditorState};
use cpaste::PasteOverSelection;
use delete::DeleteToEndOfLine;
use enum_dispatch::enum_dispatch;
use motion::{MoveToFirstRow, MoveToLastRow};
#[cfg(feature = "system-editor")]
pub use system_editor::OpenSystemEditor;

pub use self::cpaste::{CopyLine, CopySelection, Paste};
pub use self::delete::{
    DeleteChar, DeleteCharForward, DeleteLine, DeleteSelection, DeleteToFirstCharOfLine,
    DeleteWordBackward, DeleteWordForward, JoinLineWithLineBelow, RemoveChar, ReplaceChar,
};
pub use self::insert::{AppendNewline, InsertChar, InsertNewline, LineBreak};
pub use self::motion::{
    MoveBackward, MoveDown, MoveForward, MoveHalfPageDown, MoveHalfPageUp, MovePageDown,
    MovePageUp, MoveParagraphBackward, MoveParagraphForward, MoveToEndOfLine, MoveToFirst,
    MoveToMatchinBracket, MoveToStartOfLine, MoveUp, MoveWordBackward, MoveWordForward,
    MoveWordForwardToEndOfWord,
};
use self::search::StartSearch;
pub use self::search::{
    AppendCharToSearch, FindFirst, FindNext, FindPrevious, RemoveCharFromSearch,
    SelectCurrentSearch, StopSearch,
};
pub use self::select::{
    ChangeInnerBetween, ChangeInnerWord, ChangeSelection, SelectInnerBetween, SelectInnerWord,
    SelectLine,
};

#[enum_dispatch(Execute)]
#[derive(Clone, Debug)]
pub enum Action {
    SwitchMode(SwitchMode),
    MoveForward(MoveForward),
    MoveBackward(MoveBackward),
    MoveUp(MoveUp),
    MoveDown(MoveDown),
    MoveWordForward(MoveWordForward),
    MoveWordForwardToEndOfWord(MoveWordForwardToEndOfWord),
    MoveWordBackward(MoveWordBackward),
    MoveToStartOfLine(MoveToStartOfLine),
    MoveToFirst(MoveToFirst),
    MoveToEndOfLine(MoveToEndOfLine),
    MoveToFirstRow(MoveToFirstRow),
    MoveToLastRow(MoveToLastRow),
    MoveToMatchingBracket(MoveToMatchinBracket),
    MoveHalfPageDown(MoveHalfPageDown),
    MoveHalfPageUp(MoveHalfPageUp),
    MovePageDown(MovePageDown),
    MovePageUp(MovePageUp),
    MoveParagraphForward(MoveParagraphForward),
    MoveParagraphBackward(MoveParagraphBackward),
    InsertChar(InsertChar),
    LineBreak(LineBreak),
    AppendNewline(AppendNewline),
    InsertNewline(InsertNewline),
    ReplaceChar(ReplaceChar),
    RemoveChar(RemoveChar),
    DeleteChar(DeleteChar),
    DeleteCharForward(DeleteCharForward),
    DeleteLine(DeleteLine),
    DeleteToFirstCharOfLine(DeleteToFirstCharOfLine),
    DeleteToEndOfLine(DeleteToEndOfLine),
    DeleteWordForward(DeleteWordForward),
    DeleteWordBackward(DeleteWordBackward),
    DeleteSelection(DeleteSelection),
    JoinLineWithLineBelow(JoinLineWithLineBelow),
    SelectInnerBetween(SelectInnerBetween),
    SelectInnerWord(SelectInnerWord),
    ChangeInnerBetween(ChangeInnerBetween),
    ChangeInnerWord(ChangeInnerWord),
    ChangeSelection(ChangeSelection),
    SelectLine(SelectLine),
    Undo(Undo),
    Redo(Redo),
    Paste(Paste),
    PasteOverSelection(PasteOverSelection),
    CopySelection(CopySelection),
    CopyLine(CopyLine),
    Composed(Composed),
    StartSearch(StartSearch),
    StopSearch(StopSearch),
    FindFirst(FindFirst),
    FindNext(FindNext),
    FindPrevious(FindPrevious),
    SelectCurrentSearch(SelectCurrentSearch),
    AppendCharToSearch(AppendCharToSearch),
    RemoveCharFromSearch(RemoveCharFromSearch),
    ToggleCase(ToggleCase),
    #[cfg(feature = "system-editor")]
    OpenSystemEditor(OpenSystemEditor),
}

#[enum_dispatch]
pub trait Execute {
    fn execute(&mut self, state: &mut EditorState);
}

pub trait Chainable {
    fn chain<A: Into<Action>>(self, action: A) -> Composed;
}

impl<T: Into<Action>> Chainable for T {
    fn chain<A: Into<Action>>(self, action: A) -> Composed {
        Composed::new(self.into()).chain(action)
    }
}

#[derive(Clone, Debug)]
pub struct SwitchMode(pub EditorMode);

impl Execute for SwitchMode {
    fn execute(&mut self, state: &mut EditorState) {
        state.clamp_column();
        match self.0 {
            EditorMode::Normal => {
                state.selection = None;
            }
            EditorMode::Visual => {
                state.selection = Some(Selection::new(state.cursor, state.cursor));
            }
            EditorMode::Insert => {
                if ![EditorMode::Insert, EditorMode::Search].contains(&state.mode) {
                    state.capture();
                }
            }
            EditorMode::Search => {}
        }
        state.mode = self.0;
    }
}

#[derive(Clone, Debug)]
pub struct Undo;

impl Execute for Undo {
    fn execute(&mut self, state: &mut EditorState) {
        state.undo();
    }
}

#[derive(Clone, Debug)]
pub struct Redo;

impl Execute for Redo {
    fn execute(&mut self, state: &mut EditorState) {
        state.redo();
    }
}

#[derive(Clone, Debug)]
pub struct ToggleCase;

impl Execute for ToggleCase {
    fn execute(&mut self, state: &mut EditorState) {
        if state.lines.is_empty() {
            return;
        }
        state.capture();

        if state.mode == EditorMode::Visual {
            if let Some(selection) = &state.selection {
                let start = selection.start();
                let end = selection.end();

                for row in start.row..=end.row {
                    let Some(len_col) = state.lines.len_col(row) else {
                        continue;
                    };
                    let col_start = if row == start.row { start.col } else { 0 };
                    let col_end = if row == end.row { end.col } else { len_col.saturating_sub(1) };

                    for col in col_start..=col_end {
                        let index = crate::edtui::Index2::new(row, col);
                        if let Some(ch) = state.lines.get_mut(index) {
                            *ch = if ch.is_uppercase() {
                                ch.to_lowercase().next().unwrap_or(*ch)
                            } else {
                                ch.to_uppercase().next().unwrap_or(*ch)
                            };
                        }
                    }
                }

                // Clear selection and switch back to normal mode
                state.selection = None;
                state.mode = EditorMode::Normal;
                state.clamp_column();
            }
        } else {
            // Normal mode: toggle char under cursor, and move cursor forward by 1 character
            let row = state.cursor.row;
            let col = state.cursor.col;
            if let Some(ch) = state.lines.get_mut(state.cursor) {
                *ch = if ch.is_uppercase() {
                    ch.to_lowercase().next().unwrap_or(*ch)
                } else {
                    ch.to_uppercase().next().unwrap_or(*ch)
                };

                // Move cursor right by 1 character, clamped to end of line
                if let Some(len_col) = state.lines.len_col(row) {
                    let max_col = len_col.saturating_sub(1);
                    state.cursor.col = (col + 1).min(max_col);
                }
            }
        }
    }
}

/// Executes multiple actions one after the other.
#[derive(Clone, Debug)]
pub struct Composed(pub Vec<Action>);

impl Composed {
    #[must_use]
    pub fn new<A: Into<Action>>(action: A) -> Self {
        Self(vec![action.into()])
    }

    #[must_use]
    pub fn chain<A: Into<Action>>(mut self, action: A) -> Self {
        self.0.push(action.into());
        self
    }
}

impl Execute for Composed {
    fn execute(&mut self, state: &mut EditorState) {
        for action in &mut self.0 {
            action.execute(state);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::edtui::clipboard::InternalClipboard;
    use crate::edtui::Lines;

    use super::*;
    fn test_state() -> EditorState {
        let mut state = EditorState::new(Lines::from("Hello World!\n\n123."));
        state.set_clipboard(InternalClipboard::default());
        state
    }

    #[test]
    fn test_switch_mode() {
        let mut state = test_state();
        assert_eq!(state.mode, EditorMode::Normal);

        SwitchMode(EditorMode::Insert).execute(&mut state);
        assert_eq!(state.mode, EditorMode::Insert);

        SwitchMode(EditorMode::Visual).execute(&mut state);
        assert_eq!(state.mode, EditorMode::Visual);
    }

    #[test]
    fn test_chainable_actions() {
        let mut state = test_state();
        assert_eq!(state.mode, EditorMode::Normal);

        // Test the new chainable syntax: SwitchMode().chain().chain()
        let mut action = SwitchMode(EditorMode::Insert)
            .chain(MoveToEndOfLine())
            .chain(SwitchMode(EditorMode::Visual));

        action.execute(&mut state);

        // Verify the final state after chaining
        assert_eq!(state.mode, EditorMode::Visual);
        assert!(state.selection.is_some());
    }

    #[test]
    fn test_toggle_case() {
        let mut state = test_state(); // "Hello World!\n\n123."
        state.cursor = crate::edtui::Index2::new(0, 0); // 'H'

        ToggleCase.execute(&mut state);
        assert_eq!(state.lines, Lines::from("hello World!\n\n123."));
        assert_eq!(state.cursor.col, 1); // cursor moved to 'e'

        ToggleCase.execute(&mut state);
        assert_eq!(state.lines, Lines::from("hEllo World!\n\n123."));
        assert_eq!(state.cursor.col, 2); // cursor moved to 'l'

        // Test visual mode case toggle
        state.cursor = crate::edtui::Index2::new(0, 2); // 'l'
        SwitchMode(EditorMode::Visual).execute(&mut state);
        // Extend selection to 'o' (col 4)
        state.cursor.col = 4;
        state.selection = Some(crate::edtui::state::selection::Selection::new(
            crate::edtui::Index2::new(0, 2),
            crate::edtui::Index2::new(0, 4)
        ));

        ToggleCase.execute(&mut state);
        // "hEllo World!" -> col 2,3,4 are "llo". Toggled -> "LLO".
        assert_eq!(state.lines, Lines::from("hELLO World!\n\n123."));
        assert_eq!(state.mode, EditorMode::Normal);
    }
}
