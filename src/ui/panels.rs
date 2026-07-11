//! The wiki-map (left) and variables (right) side panels. Extracted from `ui()`;
//! the editor panel and the `left_area`/`right_area` layout write-back stay in
//! the orchestrator (mouse routing depends on the latter).

use crate::{App, FocusedPanel};
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
