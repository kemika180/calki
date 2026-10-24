use crossterm::event::{MouseEvent as CTMouseEvent, MouseEventKind};
use jagged::Index2;
use std::ops::Range;

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

        // Hit-test against the geometry the user is looking at, i.e. before
        // this event changes the selection or mode (which decide which rows
        // render concealed). A release doesn't move the cursor: press and
        // drag already put it there, and the press may have revealed (and so
        // reflowed) the row under the pointer.
        let hit = match event {
            MouseEvent::Down(mouse) | MouseEvent::Drag(mouse) => Some(
                mouse_position_to_cursor_position(state, &mouse, state.view.tab_width),
            ),
            _ => None,
        };

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

        if let Some(cursor) = hit {
            let lines = &state.lines;
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
    let mouse = Index2::new(
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
        let line = state
            .lines
            .get(jagged::index::RowIndex::new(row_index))
            .map_or(&[][..], Vec::as_slice);
        let map = state.column_map(row_index);
        let display = map.display_chars(line);
        let start = map.to_display(col_index);
        let scrolled = display.get(start..).unwrap_or_default();
        let offset = column_at_screen_x(scrolled.iter().copied(), mouse.col, tab_width);
        return Index2::new(row_index, map.to_buffer(start + offset));
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
        // Wrap the chars as drawn, then map the hit back to a buffer column.
        let map = state.column_map(buffer_row);
        let display = map.display_chars(line);
        let ranges =
            LineWrapper::wrap_ranges(&display, state.view.screen_area.width.into(), tab_width);
        if row_screen_index + ranges.len() > mouse.row {
            let wrapped_row = mouse.row - row_screen_index;
            let display_col =
                column_in_wrapped_row(&display, &ranges, wrapped_row, mouse.col, tab_width);
            col_index = map.to_buffer(display_col);
            break;
        }
        row_screen_index += ranges.len();
    }

    Index2::new(row_index, col_index)
}

/// Maps a click at screen column `x` on screen row `wrapped_row` of a wrapped
/// line to a buffer column, using the char ranges from
/// [`LineWrapper::wrap_ranges`]. A click past the row's last char lands just
/// after it.
fn column_in_wrapped_row(
    line: &[char],
    ranges: &[Range<usize>],
    wrapped_row: usize,
    x: usize,
    tab_width: usize,
) -> usize {
    let Some(range) = ranges.get(wrapped_row) else {
        return 0;
    };
    let row_chars = line.get(range.clone()).unwrap_or_default();
    range.start + column_at_screen_x(row_chars.iter().copied(), x, tab_width)
}

/// Counts how many chars fit fully before screen column `x`, i.e. the index of
/// the char drawn at `x` (or the char count if `x` is past the end).
fn column_at_screen_x(chars: impl Iterator<Item = char>, x: usize, tab_width: usize) -> usize {
    let mut width = 0;
    let mut count = 0;
    for ch in chars {
        let w = char_width(ch, tab_width);
        if width + w > x {
            break;
        }
        width += w;
        count += 1;
    }
    count
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

    fn wrapped_state(text: &str, width: u16) -> EditorState {
        let mut state = EditorState::new(Lines::from(text));
        state.view.screen_area = ratatui::layout::Rect::new(0, 0, width, 24);
        state.view.wrap = true;
        state
    }

    fn hit(state: &EditorState, row: u16, col: u16) -> Index2 {
        mouse_position_to_cursor_position(
            state,
            &MousePosition::new(row, col),
            state.view.tab_width,
        )
    }

    #[test]
    fn click_after_word_wrap_maps_to_buffer_col() {
        // "Hello" / "World" at width 7; 'W' is buffer col 6.
        let state = wrapped_state("Hello World", 7);
        assert_eq!(hit(&state, 1, 0), Index2::new(0, 6));
        assert_eq!(hit(&state, 1, 4), Index2::new(0, 10));
        assert_eq!(hit(&state, 0, 4), Index2::new(0, 4));
    }

    #[test]
    fn click_third_wrapped_row_no_cumulative_drift() {
        // "aa" / "bb" / "cc" / "dd" at width 3; each break drops one space.
        let state = wrapped_state("aa bb cc dd", 3);
        assert_eq!(hit(&state, 2, 0), Index2::new(0, 6));
        assert_eq!(hit(&state, 3, 1), Index2::new(0, 10));
    }

    #[test]
    fn click_past_row_end_maps_to_break_col() {
        // Past the end of "Hello" lands on the dropped space (col 5).
        let state = wrapped_state("Hello World", 7);
        assert_eq!(hit(&state, 0, 6), Index2::new(0, 5));
    }

    #[test]
    fn click_on_later_line_after_wrapped_line() {
        // Row 0 wraps onto two screen rows, so screen row 2 is buffer row 1.
        let state = wrapped_state("Hello World\nnext", 7);
        assert_eq!(hit(&state, 2, 2), Index2::new(1, 2));
    }

    fn unwrapped_state(text: &str) -> EditorState {
        let mut state = wrapped_state(text, 80);
        state.view.wrap = false;
        state
    }

    #[test]
    fn click_no_wrap_after_tab() {
        // Tab width 2: '\t' covers screen cols 0-1, so 'b' at col 1 is drawn at x=2.
        let state = unwrapped_state("\tbc");
        assert_eq!(hit(&state, 0, 2), Index2::new(0, 1));
        assert_eq!(hit(&state, 0, 3), Index2::new(0, 2));
    }

    #[test]
    fn click_no_wrap_on_second_cell_of_wide_char() {
        // '😀' covers cols 1-2; both cells map to it, 'b' is at x=3.
        let state = unwrapped_state("a😀b");
        assert_eq!(hit(&state, 0, 1), Index2::new(0, 1));
        assert_eq!(hit(&state, 0, 2), Index2::new(0, 1));
        assert_eq!(hit(&state, 0, 3), Index2::new(0, 2));
    }

    #[test]
    fn click_no_wrap_with_horizontal_scroll() {
        let mut state = unwrapped_state("0123456789");
        state.view.viewport.x = 4;
        assert_eq!(hit(&state, 0, 0), Index2::new(0, 4));
        assert_eq!(hit(&state, 0, 3), Index2::new(0, 7));
    }

    /// State with the backticks of every row hidden (cursor on row 0).
    fn concealed_state(text: &str, width: u16, wrap: bool) -> EditorState {
        let mut state = wrapped_state(text, width);
        state.view.wrap = wrap;
        let ranges = text
            .lines()
            .enumerate()
            .map(|(row, line)| {
                let ticks = line.chars().enumerate().filter(|&(_, c)| c == '`');
                (row, ticks.map(|(i, _)| i..i + 1).collect())
            })
            .collect();
        state.set_conceal_ranges(ranges);
        state
    }

    fn down(state: &mut EditorState, row: u16, col: u16) {
        MouseEventHandler::on_event(MouseEvent::Down(MousePosition::new(row, col)), state);
    }

    #[test]
    fn click_concealed_row_maps_to_visible_char() {
        // Row 1 "a `bc` d" is drawn as "a bc d"; x=3 is 'c', buffer col 4.
        let state = concealed_state("x\na `bc` d", 80, false);
        assert_eq!(hit(&state, 1, 3), Index2::new(1, 4));
        assert_eq!(hit(&state, 1, 2), Index2::new(1, 3));
        assert_eq!(hit(&state, 1, 5), Index2::new(1, 7));
    }

    #[test]
    fn click_concealed_row_wrapped() {
        // "aa `bb` cc" is drawn as "aa bb cc", wrapped at 6 to "aa bb" / "cc".
        let state = concealed_state("x\naa `bb` cc", 6, true);
        assert_eq!(hit(&state, 1, 3), Index2::new(1, 4));
        assert_eq!(hit(&state, 2, 1), Index2::new(1, 9));
    }

    #[test]
    fn click_concealed_row_no_wrap_scrolled() {
        // Scrolled 2 cols, row 1 is drawn as "cdef"; x=1 is 'd', buffer col 4.
        let mut state = concealed_state("0123456789\nab`cd`ef", 8, false);
        state.view.viewport.x = 2;
        assert_eq!(hit(&state, 1, 0), Index2::new(1, 3));
        assert_eq!(hit(&state, 1, 1), Index2::new(1, 4));
        assert_eq!(hit(&state, 1, 2), Index2::new(1, 6));
    }

    #[test]
    fn click_past_end_of_concealed_row_clamps() {
        // "a `b`" has 5 chars; past the end clamps to the last (hidden) one.
        let mut state = concealed_state("x\na `b`", 80, false);
        down(&mut state, 1, 30);
        assert_eq!(state.cursor, Index2::new(1, 4));
    }

    #[test]
    fn click_after_visual_selection_uses_revealed_geometry() {
        // Rows 1-2 are selected, so they are drawn raw: x=3 on row 1 is 'b' (col 3).
        let mut state = concealed_state("x\na `bc` d\ny `z`", 80, false);
        state.mode = EditorMode::Visual;
        state.selection = Some(crate::edtui::state::selection::Selection::new(
            Index2::new(1, 0),
            Index2::new(2, 2),
        ));
        state.cursor = Index2::new(2, 2);
        down(&mut state, 1, 3);
        assert_eq!(state.cursor, Index2::new(1, 3));
        assert_eq!(state.mode, EditorMode::Normal);
    }

    /// Renders `state` and checks that clicking any drawn char puts the cursor
    /// on that same char in the buffer.
    fn assert_cells_round_trip(state: &mut EditorState, width: u16) {
        use crate::edtui::{EditorTheme, EditorView};
        use ratatui_core::{buffer::Buffer, layout::Rect, widgets::Widget};

        let area = Rect::new(0, 0, width, 12);
        let mut buf = Buffer::empty(area);
        EditorView::new(state)
            .theme(EditorTheme::default().hide_status_line())
            .render(area, &mut buf);

        let mut checked = 0;
        for y in 0..area.height {
            for x in 0..area.width {
                let symbol = buf[(x, y)].symbol().to_string();
                if symbol.trim().is_empty() {
                    continue;
                }
                let pos = hit(state, y, x);
                let ch = state
                    .lines
                    .get(jagged::index::RowIndex::new(pos.row))
                    .and_then(|line| line.get(pos.col))
                    .map(char::to_string);
                assert_eq!(
                    ch.as_deref(),
                    Some(symbol.as_str()),
                    "cell ({x}, {y}) -> {pos:?}"
                );
                checked += 1;
            }
        }
        assert!(checked > 0);
    }

    #[test]
    fn every_rendered_cell_round_trips_through_mouse() {
        let text =
            "first `code` line\nsome `x` and `yy` words that wrap around\n😀 `wide` b\nplain";
        for wrap in [true, false] {
            let mut state = concealed_state(text, 14, wrap);
            state.cursor = Index2::new(3, 0);
            assert_cells_round_trip(&mut state, 14);
        }
    }

    #[test]
    fn release_after_click_on_concealed_row_keeps_cursor() {
        // Press maps x=3 on "a bc d" to 'c' (col 4) and reveals the row; the
        // release at the same cell must not re-map it against the raw row.
        let mut state = concealed_state("x\na `bc` d", 80, false);
        down(&mut state, 1, 3);
        assert_eq!(state.cursor, Index2::new(1, 4));
        MouseEventHandler::on_event(MouseEvent::Up(MousePosition::new(1, 3)), &mut state);
        assert_eq!(state.cursor, Index2::new(1, 4));
    }
}
