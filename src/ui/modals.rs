//! Overlay popups rendered on top of the workspace: the F1 help modal, the
//! delete-confirmation, update-available, and export menus. Extracted from `ui()`.

use crate::{App, centered_rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

pub(crate) fn render_help(f: &mut Frame, app: &mut App) {
    let area = centered_rect(85, 80, f.area());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(app.palette.help_section)) // Orange border
        .bg(app.palette.surface)
        .title(Span::styled(
            " calki Quick Reference & Help ",
            Style::default().fg(app.palette.config_key).bold(),
        ));

    // We construct the tab headers row:
    let tab_headers = [
        "1.\u{a0}General",
        "2.\u{a0}Math\u{a0}&\u{a0}Trig",
        "3.\u{a0}Complex\u{a0}&\u{a0}Symbolic",
        "4.\u{a0}Lists\u{a0}&\u{a0}Stats",
        "5.\u{a0}Constants",
        "6.\u{a0}Programming",
        "7.\u{a0}Markdown",
        "8.\u{a0}Vim\u{a0}Motions",
        "9.\u{a0}About",
    ];

    let mut header_spans = Vec::new();
    for (i, title) in tab_headers.iter().enumerate() {
        if i > 0 {
            header_spans.push(Span::styled(
                "   ",
                Style::default().fg(app.palette.fg_muted),
            ));
        }
        if i == app.help_tab_idx {
            header_spans.push(Span::styled(
                format!("▶\u{a0}{}\u{a0}◀", title),
                Style::default().fg(app.palette.config_key).bold(),
            ));
        } else {
            header_spans.push(Span::styled(
                format!("\u{a0}{}\u{a0}", title),
                Style::default().fg(app.palette.fg_muted),
            ));
        }
    }
    let tab_row = Line::from(header_spans);

    // Help text content based on active tab:
    let mut help_text = vec![tab_row, Line::from("")];

    let mut content = crate::ui::help_text::help_content(app.help_tab_idx, &app.palette);

    help_text.append(&mut content);
    help_text.push(Line::from(""));
    help_text.push(Line::from(vec![Span::styled(
        " Press h/l (Left/Right) to switch tabs  •  Press any other key to close ",
        Style::default().fg(app.palette.help_section).italic(),
    )]));

    let max_scroll = if help_text.len() > area.height as usize {
        (help_text.len() - area.height as usize) as u16
    } else {
        0
    };
    if app.help_scroll > max_scroll {
        app.help_scroll = max_scroll;
    }

    let paragraph = Paragraph::new(help_text)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((app.help_scroll, 0));
    f.render_widget(Clear, area); // Clear background
    f.render_widget(paragraph, area);
}

pub(crate) fn render_delete_confirm(f: &mut Frame, app: &App) {
    let area = centered_rect(60, 25, f.area());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(app.palette.error)) // Red border for danger
        .bg(app.palette.surface)
        .title(Span::styled(
            " Delete Wiki Page ",
            Style::default().fg(app.palette.error).bold(),
        ));

    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " Are you sure you want to delete ",
                Style::default().fg(app.palette.fg),
            ),
            Span::styled(
                format!("\"{}\"", app.delete_target_name),
                Style::default().bold().fg(app.palette.config_key),
            ),
            Span::styled("?", Style::default().fg(app.palette.fg)),
        ])
        .centered(),
        Line::from(" This will permanently remove the file from your disk. ").centered(),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [y] ",
                Style::default().fg(app.palette.keybind_label).bold(),
            ),
            Span::styled("Yes, delete it  ", Style::default().fg(app.palette.fg)),
            Span::styled(
                "  [any other key] ",
                Style::default().fg(app.palette.help_section).bold(),
            ),
            Span::styled("Cancel  ", Style::default().fg(app.palette.fg)),
        ])
        .centered(),
    ];

    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}

pub(crate) fn render_update_modal(f: &mut Frame, app: &App) {
    let area = centered_rect(65, 30, f.area());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(app.palette.border_focused)) // info border
        .bg(app.palette.surface)
        .title(Span::styled(
            " Update Available ",
            Style::default().fg(app.palette.config_key).bold(),
        ));

    let version_span = if let Some(ref version) = app.update_available {
        Span::styled(
            format!(" (v{}) is available on crates.io!", version),
            Style::default().fg(app.palette.fg),
        )
    } else {
        Span::styled(
            " is available on crates.io!",
            Style::default().fg(app.palette.fg),
        )
    };

    let text = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(" A new version of ", Style::default().fg(app.palette.fg)),
            Span::styled("calki", Style::default().bold().fg(app.palette.link)),
            version_span,
        ])
        .centered(),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [i] ",
                Style::default().fg(app.palette.keybind_label).bold(),
            ),
            Span::styled("Ignore this update  ", Style::default().fg(app.palette.fg)),
            Span::styled(
                "  [any other key] ",
                Style::default().fg(app.palette.help_section).bold(),
            ),
            Span::styled("Dismiss  ", Style::default().fg(app.palette.fg)),
        ])
        .centered(),
    ];

    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}

pub(crate) fn render_export_menu(f: &mut Frame, app: &App) {
    let area = centered_rect(65, 30, f.area());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(app.palette.link)) // Purple border for export
        .bg(app.palette.surface)
        .title(Span::styled(
            " Export Menu ",
            Style::default().fg(app.palette.link).bold(),
        ));

    let text = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            " Choose an export option:",
            Style::default().fg(app.palette.fg),
        )])
        .centered(),
        Line::from(""),
        Line::from(vec![
            Span::styled("  [1] ", Style::default().fg(app.palette.config_key).bold()),
            Span::styled(
                "Export current note to HTML",
                Style::default().fg(app.palette.fg),
            ),
        ])
        .centered(),
        Line::from(vec![
            Span::styled("  [2] ", Style::default().fg(app.palette.config_key).bold()),
            Span::styled(
                "Compile entire wiki to Markdown",
                Style::default().fg(app.palette.fg),
            ),
        ])
        .centered(),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "  [Esc] ",
                Style::default().fg(app.palette.help_section).bold(),
            ),
            Span::styled("Cancel", Style::default().fg(app.palette.fg)),
        ])
        .centered(),
    ];

    let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
    f.render_widget(Clear, area);
    f.render_widget(paragraph, area);
}
