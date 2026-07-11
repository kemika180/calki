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
        let status_bg = Color::Rgb(22, 22, 30);
        let status_block = Block::default().bg(status_bg);

        let status_line = if let Some((msg, inst)) = &app.status_message {
            if inst.elapsed() < Duration::from_secs(5) {
                Line::from(vec![
                    Span::styled(
                        " ✔  ",
                        Style::default().fg(Color::Rgb(158, 206, 106)).bold(),
                    ),
                    Span::styled(msg, Style::default().fg(Color::Rgb(158, 206, 106))),
                ])
            } else {
                Line::from("")
            }
        } else if app.search_active {
            Line::from(vec![
                Span::styled(
                    " 🔍 Search: ",
                    Style::default().fg(Color::Rgb(255, 158, 100)).bold(),
                ),
                Span::styled(
                    &app.search_query,
                    Style::default().fg(Color::Rgb(125, 207, 255)),
                ),
                Span::styled("█", Style::default().fg(Color::Rgb(125, 207, 255)).bold()), // cursor
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
