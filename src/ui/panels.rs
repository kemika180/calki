//! The wiki-map (left) and variables (right) side panels. Extracted from `ui()`;
//! the editor panel and the `left_area`/`right_area` layout write-back stay in
//! the orchestrator (mouse routing depends on the latter).

use crate::edtui::{self, EditorMode, EditorView, RowIndex};
use crate::{App, FocusedPanel, estimate_line_height};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem};

pub(crate) fn render_wiki_map(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focused_panel == FocusedPanel::WikiMap;
    let border_type = if is_focused {
        BorderType::Double
    } else {
        BorderType::Plain
    };
    let border_color = if is_focused {
        app.palette.border_focused
    } else {
        app.palette.border_dim
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
        .bg(app.palette.bg)
        .title(Span::styled(
            " Wiki Map ",
            Style::default().fg(app.palette.fg).bold(),
        ));

    let mut list_items = Vec::new();
    list_items.push(ListItem::new("◀ Backlinks").bold().fg(app.palette.h3)); // Royal Blue #7aa2f7

    let mut current_link_idx = 0;
    for link in &app.backlinks {
        let is_selected = is_focused && current_link_idx == app.selected_link_idx;
        let style = if is_selected {
            Style::default()
                .bg(app.palette.panel_sel_bg)
                .fg(app.palette.border_focused)
                .bold()
        } else {
            Style::default().fg(app.palette.fg)
        };
        let prefix = if is_selected { " ▶ " } else { " - " };
        list_items.push(ListItem::new(format!("{}{}", prefix, link)).style(style));
        current_link_idx += 1;
    }
    if app.backlinks.is_empty() {
        list_items.push(
            ListItem::new("  (none)")
                .fg(app.palette.border_dim)
                .italic(),
        );
    }

    list_items.push(ListItem::new("")); // Spacer

    list_items.push(ListItem::new("▶ Outgoing").bold().fg(app.palette.h3)); // Royal Blue #7aa2f7
    for link in &app.outgoing {
        let is_selected = is_focused && current_link_idx == app.selected_link_idx;
        let style = if is_selected {
            Style::default()
                .bg(app.palette.panel_sel_bg)
                .fg(app.palette.border_focused)
                .bold()
        } else {
            Style::default().fg(app.palette.fg)
        };
        let prefix = if is_selected { " ▶ " } else { " - " };
        list_items.push(ListItem::new(format!("{}{}", prefix, link)).style(style));
        current_link_idx += 1;
    }
    if app.outgoing.is_empty() {
        list_items.push(
            ListItem::new("  (none)")
                .fg(app.palette.border_dim)
                .italic(),
        );
    }

    if app.show_search_results {
        list_items.push(ListItem::new("")); // Spacer
        list_items.push(
            ListItem::new("🔍 Search Results")
                .bold()
                .fg(app.palette.help_section),
        ); // Orange
        for link in &app.search_results {
            let is_selected = is_focused && current_link_idx == app.selected_link_idx;
            let style = if is_selected {
                Style::default()
                    .bg(app.palette.panel_sel_bg)
                    .fg(app.palette.border_focused)
                    .bold()
            } else {
                Style::default().fg(app.palette.fg)
            };
            let prefix = if is_selected { " ▶ " } else { " - " };
            list_items.push(ListItem::new(format!("{}{}", prefix, link)).style(style));
            current_link_idx += 1;
        }
        if app.search_results.is_empty() {
            list_items.push(
                ListItem::new("  (no matches)")
                    .fg(app.palette.border_dim)
                    .italic(),
            );
        }
    }

    let list = List::new(list_items).block(block);
    f.render_widget(list, area);
}

pub(crate) fn render_variables(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.focused_panel == FocusedPanel::Variables;
    let border_type = if is_focused {
        BorderType::Double
    } else {
        BorderType::Plain
    };
    let border_color = if is_focused {
        app.palette.border_focused
    } else {
        app.palette.border_dim
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
        .bg(app.palette.bg)
        .title(Span::styled(
            " Variables ",
            Style::default().fg(app.palette.fg).bold(),
        ));

    let mut list_items = Vec::new();
    for (idx, (name, val)) in app.variables_cache.iter().enumerate() {
        let is_selected = is_focused && idx == app.selected_var_idx;
        let is_error = val.contains("[Error");
        let val_style = if is_error {
            Style::default().fg(app.palette.error).bold()
        } else {
            Style::default().fg(app.palette.math_number)
        };

        let prefix = if is_selected { "▶ " } else { "  " };
        let prefix_style = if is_selected {
            Style::default().fg(app.palette.config_key).bold()
        } else {
            Style::default()
        };

        let name_style = if is_selected {
            Style::default().fg(app.palette.config_key).bold()
        } else {
            Style::default().fg(app.palette.fg).bold()
        };

        let item_line = Line::from(vec![
            Span::styled(prefix, prefix_style),
            Span::styled(format!("{}: ", name), name_style),
            Span::styled(val, val_style),
        ]);

        let mut item = ListItem::new(item_line);
        if is_selected {
            item = item.style(Style::default().bg(app.palette.panel_sel_bg));
        }
        list_items.push(item);
    }
    if app.variables_cache.is_empty() {
        list_items.push(
            ListItem::new("  (no bindings)")
                .fg(app.palette.border_dim)
                .italic(),
        );
    }

    let list = List::new(list_items).block(block);
    f.render_widget(list, area);
}

pub(crate) fn render_editor(f: &mut Frame, app: &mut App, editor_area: Rect) {
    let is_focused = app.focused_panel == FocusedPanel::Editor;
    let border_type = if is_focused {
        BorderType::Double
    } else {
        BorderType::Plain
    };
    let border_color = if is_focused {
        app.palette.border_focused
    } else {
        app.palette.border_dim
    };

    let mode_str = match app.editor_state.mode {
        EditorMode::Normal => "NORMAL",
        EditorMode::Insert => "INSERT",
        EditorMode::Visual => "VISUAL",
        EditorMode::Search => "SEARCH",
    };
    let mode_color = match app.editor_state.mode {
        EditorMode::Normal => app.palette.cursor_normal,
        EditorMode::Insert => app.palette.cursor_insert,
        EditorMode::Visual => app.palette.cursor_visual,
        EditorMode::Search => app.palette.cursor_search,
    };
    let note_title = app.wiki_mgr.path_to_title(&app.active_path);
    let title_top = Line::from(vec![
        Span::styled(" calki: ", Style::default().fg(app.palette.fg).bold()),
        Span::styled(note_title, Style::default().fg(app.palette.fg).bold()),
        Span::styled(" ", Style::default()),
    ]);

    let title_bottom_left = Line::from(vec![
        Span::styled(" [", Style::default().fg(app.palette.fg)),
        Span::styled(mode_str, Style::default().fg(mode_color).bold()),
        Span::styled("] ", Style::default().fg(app.palette.fg)),
    ]);

    let total_lines = app.editor_state.lines.len();
    let scroll_pct = if total_lines <= 1 {
        0
    } else {
        (app.editor_state.cursor.row * 100) / (total_lines - 1)
    };
    let border_char = if is_focused { "═" } else { "─" };
    let title_bottom_right = Line::from(vec![
        Span::styled(
            format!(
                " Line: {}, Col: {} ",
                app.editor_state.cursor.row + 1,
                app.editor_state.cursor.col + 1
            ),
            Style::default().fg(app.palette.fg),
        ),
        Span::styled(border_char.repeat(3), Style::default().fg(border_color)),
        Span::styled(
            format!(" {:>3}% ", scroll_pct),
            Style::default().fg(app.palette.fg),
        ),
    ])
    .right_aligned();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
        .bg(app.palette.bg)
        .title(title_top)
        .title_bottom(title_bottom_left)
        .title_bottom(title_bottom_right);

    let inner_editor_area = block.inner(editor_area);
    f.render_widget(block, editor_area);

    let editor_theme = edtui::EditorTheme::default()
        .hide_status_line()
        .hide_cursor()
        .base(
            Style::default()
                .bg(app.palette.editor_bg)
                .fg(app.palette.editor_fg),
        )
        .line_numbers_style(
            Style::default()
                .bg(app.palette.editor_bg)
                .fg(app.palette.line_number),
        )
        .selection_style(
            Style::default()
                .bg(app.palette.selection_bg)
                .fg(app.palette.selection_fg),
        );
    let viewport_height = inner_editor_area.height as usize;
    let scrolloff = std::cmp::min(app.config.scrolloff, viewport_height / 2);
    app.editor_state.set_viewport_height(viewport_height);

    let (x_offset, mut y_offset) = app.editor_state.viewport_offset();
    let cursor_row = app.editor_state.cursor.row;

    if app.config.word_wrap {
        // Calculate line wrapping width
        let line_num_config = match app.config.line_numbers.as_str() {
            "Absolute" => edtui::LineNumbers::Absolute,
            "Relative" => edtui::LineNumbers::Relative,
            _ => edtui::LineNumbers::None,
        };
        let line_number_width = if line_num_config != edtui::LineNumbers::None {
            app.editor_state.lines.len().max(1).to_string().len() + 1
        } else {
            0
        };
        let text_width = inner_editor_area.width as usize;
        let wrap_width = text_width.saturating_sub(line_number_width);

        // Rows hidden inside collapsed folds occupy no vertical space, so the
        // scrolloff math below measures physical height over visible rows only.
        let hidden = app.editor_state.hidden_rows();

        // Helper to get line height
        let get_line_height = |row: usize| -> usize {
            if hidden.contains(&row) {
                return 0;
            }
            if let Some(line) = app.editor_state.lines.get(RowIndex::new(row)) {
                estimate_line_height(line.as_slice(), wrap_width, 4)
            } else {
                1
            }
        };

        // Calculate physical_top(y_offset, cursor_row)
        let get_physical_top = |start_y: usize, end_row: usize| -> usize {
            let mut sum = 0;
            for row in start_y..end_row {
                sum += get_line_height(row);
            }
            sum
        };

        // Constraint check: too far up?
        if cursor_row < y_offset {
            y_offset = cursor_row;
        }

        while y_offset > 0 && get_physical_top(y_offset, cursor_row) < scrolloff {
            y_offset -= 1;
        }

        // Ensure scrolloff below cursor:
        let cursor_height = get_line_height(cursor_row);
        let target_limit = viewport_height.saturating_sub(scrolloff);

        while y_offset < cursor_row {
            let phys_top = get_physical_top(y_offset, cursor_row);
            if phys_top + cursor_height > target_limit {
                y_offset += 1;
            } else {
                break;
            }
        }

        // Make sure the cursor row itself is visible in the viewport even if scrolloff is too large
        while y_offset < cursor_row {
            let phys_top = get_physical_top(y_offset, cursor_row);
            if phys_top + cursor_height > viewport_height {
                y_offset += 1;
            } else {
                break;
            }
        }
    } else if app.editor_state.folded.is_empty() {
        if cursor_row < y_offset + scrolloff {
            y_offset = cursor_row.saturating_sub(scrolloff);
        } else if cursor_row >= y_offset + viewport_height.saturating_sub(scrolloff) {
            y_offset = (cursor_row + scrolloff + 1).saturating_sub(viewport_height);
        }
    } else {
        // Fold-aware: keep `scrolloff` *visible* rows around the cursor so a
        // collapsed section above it doesn't inflate the scroll distance.
        let visible = app.editor_state.visible_lines();
        let cursor_v = visible.to_visible(cursor_row).unwrap_or(0);
        let top_v = visible.ordinal_at_or_after(y_offset).unwrap_or(0);
        let new_top_v = if cursor_v < top_v + scrolloff {
            cursor_v.saturating_sub(scrolloff)
        } else if cursor_v >= top_v + viewport_height.saturating_sub(scrolloff) {
            (cursor_v + scrolloff + 1).saturating_sub(viewport_height)
        } else {
            top_v
        };
        y_offset = visible.to_buffer(new_top_v).unwrap_or(y_offset);
    }

    app.editor_state.set_viewport_offset(x_offset, y_offset);

    let line_num_config = match app.config.line_numbers.as_str() {
        "Absolute" => edtui::LineNumbers::Absolute,
        "Relative" => edtui::LineNumbers::Relative,
        _ => edtui::LineNumbers::None,
    };

    let editor_widget = EditorView::new(&mut app.editor_state)
        .theme(editor_theme)
        .line_numbers(line_num_config)
        .wrap(app.config.word_wrap);
    f.render_widget(editor_widget, inner_editor_area);
    if is_focused
        && !app.show_help
        && !app.show_update_modal
        && !app.show_delete_confirm
        && let Some(pos) = app.editor_state.cursor_screen_position()
    {
        f.set_cursor_position(pos);
    }
}
