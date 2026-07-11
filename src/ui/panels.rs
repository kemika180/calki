//! The wiki-map (left) and variables (right) side panels. Extracted from `ui()`;
//! the editor panel and the `left_area`/`right_area` layout write-back stay in
//! the orchestrator (mouse routing depends on the latter).

use crate::edtui::{self, EditorMode, EditorView, RowIndex};
use crate::{App, FocusedPanel, estimate_line_height};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, List, ListItem};

pub(crate) fn render_wiki_map(
    f: &mut Frame,
    app: &App,
    area: Rect,
    bg_color: Color,
    text_fg_color: Color,
    border_focused_color: Color,
    border_dim_color: Color,
) {
    let is_focused = app.focused_panel == FocusedPanel::WikiMap;
    let border_type = if is_focused {
        BorderType::Double
    } else {
        BorderType::Plain
    };
    let border_color = if is_focused {
        border_focused_color
    } else {
        border_dim_color
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
        .bg(bg_color)
        .title(Span::styled(
            " Wiki Map ",
            Style::default().fg(text_fg_color).bold(),
        ));

    let mut list_items = Vec::new();
    list_items.push(
        ListItem::new("◀ Backlinks")
            .bold()
            .fg(Color::Rgb(122, 162, 247)),
    ); // Royal Blue #7aa2f7

    let mut current_link_idx = 0;
    for link in &app.backlinks {
        let is_selected = is_focused && current_link_idx == app.selected_link_idx;
        let style = if is_selected {
            Style::default()
                .bg(Color::Rgb(59, 66, 97))
                .fg(Color::Rgb(125, 207, 255))
                .bold()
        } else {
            Style::default().fg(text_fg_color)
        };
        let prefix = if is_selected { " ▶ " } else { " - " };
        list_items.push(ListItem::new(format!("{}{}", prefix, link)).style(style));
        current_link_idx += 1;
    }
    if app.backlinks.is_empty() {
        list_items.push(ListItem::new("  (none)").fg(border_dim_color).italic());
    }

    list_items.push(ListItem::new("")); // Spacer

    list_items.push(
        ListItem::new("▶ Outgoing")
            .bold()
            .fg(Color::Rgb(122, 162, 247)),
    ); // Royal Blue #7aa2f7
    for link in &app.outgoing {
        let is_selected = is_focused && current_link_idx == app.selected_link_idx;
        let style = if is_selected {
            Style::default()
                .bg(Color::Rgb(59, 66, 97))
                .fg(Color::Rgb(125, 207, 255))
                .bold()
        } else {
            Style::default().fg(text_fg_color)
        };
        let prefix = if is_selected { " ▶ " } else { " - " };
        list_items.push(ListItem::new(format!("{}{}", prefix, link)).style(style));
        current_link_idx += 1;
    }
    if app.outgoing.is_empty() {
        list_items.push(ListItem::new("  (none)").fg(border_dim_color).italic());
    }

    if app.show_search_results {
        list_items.push(ListItem::new("")); // Spacer
        list_items.push(
            ListItem::new("🔍 Search Results")
                .bold()
                .fg(Color::Rgb(255, 158, 100)),
        ); // Orange
        for link in &app.search_results {
            let is_selected = is_focused && current_link_idx == app.selected_link_idx;
            let style = if is_selected {
                Style::default()
                    .bg(Color::Rgb(59, 66, 97))
                    .fg(Color::Rgb(125, 207, 255))
                    .bold()
            } else {
                Style::default().fg(text_fg_color)
            };
            let prefix = if is_selected { " ▶ " } else { " - " };
            list_items.push(ListItem::new(format!("{}{}", prefix, link)).style(style));
            current_link_idx += 1;
        }
        if app.search_results.is_empty() {
            list_items.push(
                ListItem::new("  (no matches)")
                    .fg(border_dim_color)
                    .italic(),
            );
        }
    }

    let list = List::new(list_items).block(block);
    f.render_widget(list, area);
}

pub(crate) fn render_variables(
    f: &mut Frame,
    app: &App,
    area: Rect,
    bg_color: Color,
    text_fg_color: Color,
    border_focused_color: Color,
    border_dim_color: Color,
) {
    let is_focused = app.focused_panel == FocusedPanel::Variables;
    let border_type = if is_focused {
        BorderType::Double
    } else {
        BorderType::Plain
    };
    let border_color = if is_focused {
        border_focused_color
    } else {
        border_dim_color
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
        .bg(bg_color)
        .title(Span::styled(
            " Variables ",
            Style::default().fg(text_fg_color).bold(),
        ));

    let mut list_items = Vec::new();
    for (idx, (name, val)) in app.variables_cache.iter().enumerate() {
        let is_selected = is_focused && idx == app.selected_var_idx;
        let is_error = val.contains("[Error");
        let val_style = if is_error {
            Style::default().fg(Color::Rgb(247, 118, 142)).bold() // Red #f7768e
        } else {
            Style::default().fg(Color::Rgb(115, 218, 202)) // Teal #73daca
        };

        let prefix = if is_selected { "▶ " } else { "  " };
        let prefix_style = if is_selected {
            Style::default().fg(Color::Rgb(125, 207, 255)).bold()
        } else {
            Style::default()
        };

        let name_style = if is_selected {
            Style::default().fg(Color::Rgb(125, 207, 255)).bold()
        } else {
            Style::default().fg(text_fg_color).bold()
        };

        let item_line = Line::from(vec![
            Span::styled(prefix, prefix_style),
            Span::styled(format!("{}: ", name), name_style),
            Span::styled(val, val_style),
        ]);

        let mut item = ListItem::new(item_line);
        if is_selected {
            item = item.style(Style::default().bg(Color::Rgb(59, 66, 97)));
        }
        list_items.push(item);
    }
    if app.variables_cache.is_empty() {
        list_items.push(
            ListItem::new("  (no bindings)")
                .fg(border_dim_color)
                .italic(),
        );
    }

    let list = List::new(list_items).block(block);
    f.render_widget(list, area);
}

pub(crate) fn render_editor(
    f: &mut Frame,
    app: &mut App,
    editor_area: Rect,
    bg_color: Color,
    text_fg_color: Color,
    border_focused_color: Color,
    border_dim_color: Color,
) {
    let is_focused = app.focused_panel == FocusedPanel::Editor;
    let border_type = if is_focused {
        BorderType::Double
    } else {
        BorderType::Plain
    };
    let border_color = if is_focused {
        border_focused_color
    } else {
        border_dim_color
    };

    let mode_str = match app.editor_state.mode {
        EditorMode::Normal => "NORMAL",
        EditorMode::Insert => "INSERT",
        EditorMode::Visual => "VISUAL",
        EditorMode::Search => "SEARCH",
    };
    let mode_color = match app.editor_state.mode {
        EditorMode::Normal => Color::Rgb(122, 162, 247), // Blue
        EditorMode::Insert => Color::Rgb(158, 206, 106), // Green
        EditorMode::Visual => Color::Rgb(187, 154, 247), // Purple
        EditorMode::Search => Color::Rgb(255, 158, 100), // Orange
    };
    let note_title = app.wiki_mgr.path_to_title(&app.active_path);
    let title_top = Line::from(vec![
        Span::styled(" calki: ", Style::default().fg(text_fg_color).bold()),
        Span::styled(note_title, Style::default().fg(text_fg_color).bold()),
        Span::styled(" ", Style::default()),
    ]);

    let title_bottom_left = Line::from(vec![
        Span::styled(" [", Style::default().fg(text_fg_color)),
        Span::styled(mode_str, Style::default().fg(mode_color).bold()),
        Span::styled("] ", Style::default().fg(text_fg_color)),
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
            Style::default().fg(text_fg_color),
        ),
        Span::styled(border_char.repeat(3), Style::default().fg(border_color)),
        Span::styled(
            format!(" {:>3}% ", scroll_pct),
            Style::default().fg(text_fg_color),
        ),
    ])
    .right_aligned();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(Style::default().fg(border_color))
        .bg(bg_color)
        .title(title_top)
        .title_bottom(title_bottom_left)
        .title_bottom(title_bottom_right);

    let inner_editor_area = block.inner(editor_area);
    f.render_widget(block, editor_area);

    let editor_theme = edtui::EditorTheme::default()
        .hide_status_line()
        .hide_cursor()
        .selection_style(
            Style::default()
                .bg(Color::Rgb(167, 82, 142))
                .fg(Color::Rgb(224, 230, 242)),
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

        // Helper to get line height
        let get_line_height = |row: usize| -> usize {
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
    } else {
        if cursor_row < y_offset + scrolloff {
            y_offset = cursor_row.saturating_sub(scrolloff);
        } else if cursor_row >= y_offset + viewport_height.saturating_sub(scrolloff) {
            y_offset = (cursor_row + scrolloff + 1).saturating_sub(viewport_height);
        }
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
