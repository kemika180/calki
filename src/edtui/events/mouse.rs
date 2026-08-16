use crossterm::event::{MouseEvent as CTMouseEvent, MouseEventKind};
use jagged::Index2;

use crate::edtui::{
    EditorMode, EditorState,
    actions::{Execute, SwitchMode},
    helper::char_width,
    state::selection::set_selection,
    view::line_wrapper::LineWrapper,
};

/// The number of lines to scroll per scroll wheel event.
const SCROLL_LINES: usize = 1;

/// Handles a mouse event.
#[derive(Clone, Debug, Default)]
pub struct MouseEventHandler {}

impl MouseEventHandler {
    pub fn on_event<E>(event: E, state: &mut EditorState)
    where
        E: Into<MouseEvent>,
    {
        let event = event.into();
        if event == MouseEvent::None {
            return;
        }

        // Handle scroll events
        match event {
            MouseEvent::ScrollUp(mouse) => {
                if Self::is_position_within_bounds(&mouse, state) {
                    Self::handle_scroll_up(state);
                }
                return;
            }
            MouseEvent::ScrollDown(mouse) => {
                if Self::is_position_within_bounds(&mouse, state) {
                    Self::handle_scroll_down(state);
                }
                return;
            }
            _ => {}
        }

        // Check if the mouse event is within the editor's screen area
        if !Self::is_within_bounds(&event, state) {
            return;
        }

        if let MouseEvent::Down(_) = event {
            state.selection = None;
            if state.mode == EditorMode::Visual {
                SwitchMode(EditorMode::Normal).execute(state);
            }
        }

        if let MouseEvent::Drag(_) = event {
            if state.mode != EditorMode::Visual {
                SwitchMode(EditorMode::Visual).execute(state);
            }
            set_selection(&mut state.selection, state.cursor);
        }

        match event {
            MouseEvent::Down(mouse) | MouseEvent::Up(mouse) | MouseEvent::Drag(mouse) => {
                let lines = &state.lines;
                let cursor = mouse_position_to_cursor_position(state, &mouse, state.view.tab_width);
                let last_row = lines.last_row_index();
                let last_col = lines.last_col_index(cursor.row);

                // row is out of bounds
                if last_row < cursor.row {
                    let last_col = lines.last_col_index(last_row);
                    state.cursor = Index2::new(last_row, last_col);
                // col is out of bounds
                } else if last_col < cursor.col {
                    state.cursor = Index2::new(cursor.row, last_col);
                } else {
                    state.cursor = cursor;
                }

                if let MouseEvent::Drag(_) = event {
                    set_selection(&mut state.selection, state.cursor);
                }
            }
            MouseEvent::ScrollUp(_) | MouseEvent::ScrollDown(_) | MouseEvent::None => (),
        };
    }

    fn handle_scroll_up(state: &mut EditorState) {
        // Step by one *visible* row so scrolling walks over collapsed folds
        // instead of stepping cursor/viewport into hidden rows.
        let visible = state.visible_lines();
        if let Some(prev_top) = visible.prev_before(state.view.viewport.y) {
            state.view.viewport.y = prev_top;
        }
        if let Some(prev_cursor) = visible.prev_before(state.cursor.row) {
            state.cursor.row = prev_cursor;
        }
        state.clamp_column();
        if state.mode == EditorMode::Visual {
            set_selection(&mut state.selection, state.cursor);
        }
    }

    fn handle_scroll_down(state: &mut EditorState) {
        let visible = state.visible_lines();
        // Only scroll the viewport if a visible row still sits below the last
        // one currently shown.
        let top_ordinal = visible
            .ordinal_at_or_after(state.view.viewport.y)
            .unwrap_or(0);
        let last_shown = top_ordinal + state.view.num_rows.saturating_sub(1);
        if last_shown + 1 < visible.len()
            && let Some(next_top) = visible.next_after(state.view.viewport.y)
        {
            state.view.viewport.y = next_top;
        }
        if let Some(next_cursor) = visible.next_after(state.cursor.row) {
            state.cursor.row = next_cursor;
        }
        state.clamp_column();
        if state.mode == EditorMode::Visual {
            set_selection(&mut state.selection, state.cursor);
        }
    }

    /// Checks if the mouse event occurred within the editor's screen area.
    fn is_within_bounds(event: &MouseEvent, state: &EditorState) -> bool {
        let mouse = match event {
            MouseEvent::Down(pos) | MouseEvent::Up(pos) | MouseEvent::Drag(pos) => pos,
            MouseEvent::ScrollUp(pos) | MouseEvent::ScrollDown(pos) => pos,
            MouseEvent::None => return false,
        };

        Self::is_position_within_bounds(mouse, state)
    }

    fn is_position_within_bounds(mouse: &MousePosition, state: &EditorState) -> bool {
        let area = &state.view.screen_area;
        let x: usize = area.x.into();
        let y: usize = area.y.into();
        let width: usize = area.width.into();
        let height: usize = area.height.into();

        mouse.col >= x && mouse.col < x + width && mouse.row >= y && mouse.row < y + height
    }
}

fn mouse_position_to_cursor_position(
    state: &EditorState,
    mouse: &MousePosition,
    tab_width: usize,
) -> Index2 {
    let mut col_index = state.view.viewport.x;

    // Global -> editor coordinates
    let mut mouse = Index2::new(
        mouse.row.saturating_sub(state.view.screen_area.y.into()),
        mouse.col.saturating_sub(state.view.screen_area.x.into()),
    );

    // Screen rows map to *visible* buffer rows: collapsed folds remove rows from
    // the painted sequence, so `mouse.row` counts visible rows from the first
    // visible row at/after the viewport top.
    let visible = state.visible_lines();
    let top_ordinal = visible
        .ordinal_at_or_after(state.view.viewport.y)
        .unwrap_or(0);
    let last_row = state.lines.last_row_index();

    if !state.view.wrap {
        let row_index = visible
            .to_buffer(top_ordinal + mouse.row)
            .unwrap_or(last_row);
        return Index2::new(row_index, mouse.col.saturating_add(col_index));
    }

    let mut row_screen_index = 0;
    let mut row_index = visible.to_buffer(top_ordinal).unwrap_or(0);
    for screen_row in top_ordinal.. {
        let Some(buffer_row) = visible.to_buffer(screen_row) else {
            break;
        };
        row_index = buffer_row;
        let Some(line) = state.lines.get(jagged::index::RowIndex::new(buffer_row)) else {
            break;
        };
        let wrapped_line = LineWrapper::wrap_line(
            line,
            state.view.screen_area.width.into(),
            state.view.tab_width,
        );
        let wrapped_line_len = wrapped_line.len().max(1);
        if row_screen_index + wrapped_line_len > mouse.row {
            mouse.row = mouse.row.saturating_sub(row_screen_index);
            col_index = find_cursor_column_in_wrapped_line(&wrapped_line, &mouse, tab_width);
            break;
        }
        row_screen_index += wrapped_line_len;
    }

    Index2::new(row_index, col_index)
}

fn find_cursor_column_in_wrapped_line(
    line: &[Vec<char>],
    mouse: &Index2,
    tab_width: usize,
) -> usize {
    let Some(l) = line.get(mouse.row) else {
        return 0;
    };

    let col_offset: usize = line.iter().take(mouse.row).map(Vec::len).sum();
    let mut current_width = 0;
    let mut col_index = 0;

    for &ch in l {
        let char_width = char_width(ch, tab_width);

        if current_width + char_width > mouse.col {
            break;
        }

        current_width += char_width;
        col_index += 1;
    }

    col_offset + col_index
}

/// Represents a mouse event.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum MouseEvent {
    /// A mouse press event.
    Down(MousePosition),

    /// A mouse release event.
    Up(MousePosition),

    /// A mouse Drag event.
    Drag(MousePosition),

    /// A scroll up (wheel up) event.
    ScrollUp(MousePosition),

    /// A scroll down (wheel down) event.
    ScrollDown(MousePosition),

    /// A mouse event that is not handled by the editor.
    None,
}

impl From<CTMouseEvent> for MouseEvent {
    fn from(event: CTMouseEvent) -> Self {
        match event.kind {
            MouseEventKind::Down(_) => Self::Down(MousePosition::new(event.row, event.column)),
            MouseEventKind::Up(_) => Self::Up(MousePosition::new(event.row, event.column)),
            MouseEventKind::Drag(_) => Self::Drag(MousePosition::new(event.row, event.column)),
            MouseEventKind::ScrollUp => Self::ScrollUp(MousePosition::new(event.row, event.column)),
            MouseEventKind::ScrollDown => {
                Self::ScrollDown(MousePosition::new(event.row, event.column))
            }
            _ => Self::None,
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub struct MousePosition {
    /// The row that the event occurred on.
    pub(crate) row: usize,
    /// The column that the event occurred on.
    pub(crate) col: usize,
}

impl MousePosition {
    /// Creates a new `MousePosition` instance.
    fn new(row: u16, col: u16) -> Self {
        Self {
            row: row.into(),
            col: col.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edtui::Lines;

    #[test]
    fn test_mouse_scrolling_preserves_relative_cursor_row() {
        let mut state = EditorState::default();
        state.lines = Lines::from("line1\nline2\nline3\nline4\nline5\nline6");
        // Set viewport to show 3 lines: line2, line3, line4.
        state.view.num_rows = 3;
        state.view.viewport.y = 1; // line2
        state.cursor.row = 2; // line3 (visual row 1)

        // Set screen area so bounds checks pass
        state.view.screen_area = ratatui::layout::Rect::new(0, 0, 80, 24);

        // Scroll down
        MouseEventHandler::on_event(MouseEvent::ScrollDown(MousePosition::new(0, 0)), &mut state);
        assert_eq!(state.view.viewport.y, 2);
        assert_eq!(state.cursor.row, 3); // still visual row 1 relative to viewport

        // Scroll up
        MouseEventHandler::on_event(MouseEvent::ScrollUp(MousePosition::new(0, 0)), &mut state);
        assert_eq!(state.view.viewport.y, 1);
        assert_eq!(state.cursor.row, 2);

        // Scroll up again
        MouseEventHandler::on_event(MouseEvent::ScrollUp(MousePosition::new(0, 0)), &mut state);
        assert_eq!(state.view.viewport.y, 0);
        assert_eq!(state.cursor.row, 1);

        // Scroll up again (cannot scroll viewport since viewport.y is 0, so cursor moves up)
        MouseEventHandler::on_event(MouseEvent::ScrollUp(MousePosition::new(0, 0)), &mut state);
        assert_eq!(state.view.viewport.y, 0);
        assert_eq!(state.cursor.row, 0);
    }

    #[test]
    fn click_below_fold_maps_to_correct_buffer_row() {
        // # A (0), b1 (1), b2 (2), # B (3), c1 (4); fold A → visible rows 0,3,4.
        let mut state = EditorState::new(Lines::from("# A\nb1\nb2\n# B\nc1"));
        state.view.screen_area = ratatui::layout::Rect::new(0, 0, 80, 24);
        state.view.viewport.y = 0;
        state.folded.insert(0, "# A".to_string());

        // Screen row 1 is "# B" (buffer row 3), NOT the hidden "b1" (row 1).
        let hit = mouse_position_to_cursor_position(
            &state,
            &MousePosition::new(1, 0),
            state.view.tab_width,
        );
        assert_eq!(hit.row, 3);
        // Screen row 2 is "c1" (buffer row 4).
        let hit2 = mouse_position_to_cursor_position(
            &state,
            &MousePosition::new(2, 0),
            state.view.tab_width,
        );
        assert_eq!(hit2.row, 4);
    }

    #[test]
    fn scroll_down_steps_over_fold() {
        // Fold A hides rows 1,2 → visible sequence [0, 3, 4, 5].
        let mut state = EditorState::new(Lines::from("# A\nb1\nb2\n# B\nc1\nc2"));
        state.view.screen_area = ratatui::layout::Rect::new(0, 0, 80, 24);
        state.view.num_rows = 2;
        state.view.viewport.y = 0;
        state.cursor.row = 0;
        state.folded.insert(0, "# A".to_string());

        MouseEventHandler::on_event(MouseEvent::ScrollDown(MousePosition::new(0, 0)), &mut state);
        // Cursor advances to the next VISIBLE row (# B = 3), never into hidden 1/2.
        assert_eq!(state.cursor.row, 3);
    }
}
