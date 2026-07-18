//! Bottom status line: transient success messages and the search prompt.
//! Extracted from `ui()`.

use crate::App;
use ratatui::prelude::*;
use ratatui::widgets::{Block, Paragraph};
use std::time::Duration;

pub(crate) fn render_status_line(
    f: &mut Frame,
    app: &mut App,
    status_area: Rect,
    show_bottom_bar: bool,
) {
    if show_bottom_bar {
        let status_bg = app.palette.surface;
        let status_block = Block::default().bg(status_bg);

        let status_line = if let Some((msg, inst)) = &app.status_message {
            if inst.elapsed() < Duration::from_secs(5) {
                Line::from(vec![
                    Span::styled(
                        " ✔  ",
                        Style::default().fg(app.palette.keybind_label).bold(),
                    ),
                    Span::styled(msg, Style::default().fg(app.palette.keybind_label)),
                ])
            } else {
                Line::from("")
            }
        } else if app.search_active {
            Line::from(vec![
                Span::styled(
                    " 🔍 Search: ",
                    Style::default().fg(app.palette.help_section).bold(),
                ),
                Span::styled(
                    &app.search_query,
                    Style::default().fg(app.palette.config_key),
                ),
                Span::styled("█", Style::default().fg(app.palette.config_key).bold()), // cursor
            ])
        } else if app.command_active {
            Line::from(vec![
                Span::styled(" : ", Style::default().fg(app.palette.help_section).bold()),
                Span::styled(
                    &app.command_query,
                    Style::default().fg(app.palette.config_key),
                ),
                Span::styled("█", Style::default().fg(app.palette.config_key).bold()), // cursor
            ])
        } else {
            Line::from("")
        };

        let p = Paragraph::new(status_line).block(status_block);
        f.render_widget(p, status_area);
    } else {
        if app.status_message.is_some() {
            app.status_message = None;
        }
    }
}
