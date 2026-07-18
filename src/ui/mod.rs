//! Terminal UI rendering, split out of the monolithic `ui()` function.

pub mod help_text;
pub mod modals;
pub mod panels;
pub mod status;

use crate::{App, FocusedPanel};
use ratatui::prelude::*;

pub(crate) fn ui(f: &mut Frame, app: &mut App) {
    let show_bottom_bar = app.search_active
        || if let Some((_, inst)) = &app.status_message {
            inst.elapsed() < std::time::Duration::from_secs(5)
        } else {
            false
        };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Min(1),
            if show_bottom_bar {
                Constraint::Length(1)
            } else {
                Constraint::Length(0)
            },
        ])
        .split(f.area());

    let workspace_area = chunks[0];
    let status_area = chunks[1];

    // 2. Compute dynamic horizontal panel layouts
    let left_constraint = if app.left_panel_open {
        Constraint::Length(22)
    } else {
        Constraint::Length(0)
    };
    let right_width = if app.right_panel_open {
        if app.config.expand_variables_on_select && app.focused_panel == FocusedPanel::Variables {
            45
        } else {
            25
        }
    } else {
        0
    };
    let right_constraint = Constraint::Length(right_width);
    let middle_constraint = Constraint::Min(20);

    let workspace_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![left_constraint, middle_constraint, right_constraint])
        .split(workspace_area);

    let left_area = workspace_layout[0];
    let editor_area = workspace_layout[1];
    let right_area = workspace_layout[2];

    app.left_area = left_area;
    app.editor_area = editor_area;
    app.right_area = right_area;

    // RENDER 1: Left Panel (Wiki Map)
    if app.left_panel_open {
        crate::ui::panels::render_wiki_map(f, app, left_area);
    }

    // RENDER 2: Middle Panel (Editor)
    crate::ui::panels::render_editor(f, app, editor_area);

    // RENDER 3: Right Panel (Variables Inspector)
    if app.right_panel_open {
        crate::ui::panels::render_variables(f, app, right_area);
    }

    // Unified Help popup modal with tabs (opened via F1, ?, ~)
    if app.show_help {
        crate::ui::modals::render_help(f, app);
    }
    if app.show_delete_confirm {
        crate::ui::modals::render_delete_confirm(f, app);
    }
    if app.show_update_modal {
        crate::ui::modals::render_update_modal(f, app);
    }
    if app.show_export_menu {
        crate::ui::modals::render_export_menu(f, app);
    }

    crate::ui::status::render_status_line(f, app, status_area, show_bottom_bar);
}
