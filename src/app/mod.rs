//! Application input handling, split out of `run_app`.

pub mod input;

use crate::edtui::EditorMode;
use crate::{App, FocusedPanel, write_cursor_color_sequence, write_cursor_shape_sequence};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::prelude::*;
use std::time::Duration;

/// Emit the terminal cursor shape + color escape sequences for the editor mode.
fn emit_cursor_style<W: std::io::Write>(writer: &mut W, mode: EditorMode) {
    let shape_num = match mode {
        EditorMode::Normal => 1, // Blinking Block
        EditorMode::Insert => 5, // Blinking Bar
        EditorMode::Visual => 2, // Steady Block
        EditorMode::Search => 1, // Blinking Block
    };
    let _ = write_cursor_shape_sequence(writer, shape_num);

    let cursor_color = match mode {
        EditorMode::Normal => "#7aa2f7", // Blue
        EditorMode::Insert => "#9ece6a", // Green
        EditorMode::Visual => "#bb9af7", // Purple
        EditorMode::Search => "#ff9e64", // Orange
    };
    let _ = write_cursor_color_sequence(writer, cursor_color);
}

pub(crate) fn run_app<B: Backend + std::io::Write>(
    terminal: &mut Terminal<B>,
    app: &mut App,
) -> Result<(), String> {
    let mut last_key_was_z = false;
    loop {
        // Check for updates channel
        if let Some(ref rx) = app.update_receiver
            && let Ok(new_hash) = rx.try_recv()
        {
            app.update_available = Some(new_hash.clone());
            if app.config.ignored_update_hash.as_ref() != Some(&new_hash) {
                app.show_update_modal = true;
            }
            app.update_receiver = None; // Only check/notify once
        }

        app.update_highlights();
        terminal
            .draw(|f| crate::ui::ui(f, app))
            .map_err(|e| e.to_string())?;

        emit_cursor_style(terminal.backend_mut(), app.editor_state.mode);

        if event::poll(Duration::from_millis(50)).map_err(|e| e.to_string())? {
            match event::read().map_err(|e| e.to_string())? {
                Event::Key(key) => {
                    if key.kind == crossterm::event::KeyEventKind::Release {
                        continue;
                    }
                    // Global exits: Ctrl-q works anywhere, regardless of mode/panel
                    if key.code == KeyCode::Char('q')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break;
                    }

                    // If update modal is open
                    if app.show_update_modal {
                        crate::app::input::handle_update_modal(app, key);
                        continue;
                    }

                    // If export menu modal is open
                    if app.show_export_menu {
                        crate::app::input::handle_export_menu(app, key);
                        continue;
                    }

                    if app.search_active {
                        crate::app::input::handle_search(app, key);
                        continue;
                    }

                    if app.command_active {
                        crate::app::input::handle_command(app, key);
                        continue;
                    }

                    // If delete confirmation is open
                    if app.show_delete_confirm {
                        crate::app::input::handle_delete_confirm(app, key);
                        continue;
                    }

                    // If help or function guide modal is open, process scrolling or close modal
                    if crate::app::input::handle_modal_key(app, key) {
                        continue;
                    }

                    match crate::app::input::handle_global_keys(app, key, &mut last_key_was_z) {
                        crate::app::input::Flow::Break => break,
                        crate::app::input::Flow::Continue => continue,
                        crate::app::input::Flow::Pass => {}
                    }

                    // Input routing
                    match app.focused_panel {
                        FocusedPanel::Editor => crate::app::input::handle_editor_key(app, key),
                        FocusedPanel::WikiMap => crate::app::input::handle_wikimap_key(app, key),
                        FocusedPanel::Variables => {
                            crate::app::input::handle_variables_key(app, key)
                        }
                    }
                }
                Event::Mouse(mouse) => crate::app::input::handle_mouse(app, mouse),
                _ => {}
            }
        }
    }
    Ok(())
}
