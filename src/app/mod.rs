//! Application input handling, split out of `run_app`.

pub mod input;

use crate::edtui::{EditorMode, RowIndex};
use crate::{
    App, FocusedPanel, handle_modal_key, write_cursor_color_sequence, write_cursor_shape_sequence,
};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use ratatui::prelude::*;
use std::time::Duration;

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

        let shape_num = match app.editor_state.mode {
            EditorMode::Normal => 1, // Blinking Block
            EditorMode::Insert => 5, // Blinking Bar
            EditorMode::Visual => 2, // Steady Block
            EditorMode::Search => 1, // Blinking Block
        };
        let _ = write_cursor_shape_sequence(terminal.backend_mut(), shape_num);

        let cursor_color = match app.editor_state.mode {
            EditorMode::Normal => "#7aa2f7", // Blue
            EditorMode::Insert => "#9ece6a", // Green
            EditorMode::Visual => "#bb9af7", // Purple
            EditorMode::Search => "#ff9e64", // Orange
        };
        let _ = write_cursor_color_sequence(terminal.backend_mut(), cursor_color);

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

                    // If delete confirmation is open
                    if app.show_delete_confirm {
                        crate::app::input::handle_delete_confirm(app, key);
                        continue;
                    }

                    // If help or function guide modal is open, process scrolling or close modal
                    if handle_modal_key(app, key) {
                        continue;
                    }

                    // ZZ exit sequence for Vim users (Normal mode in Editor)
                    let is_z = app.focused_panel == FocusedPanel::Editor
                        && app.editor_state.mode == EditorMode::Normal
                        && !key.modifiers.contains(KeyModifiers::CONTROL)
                        && !key.modifiers.contains(KeyModifiers::ALT)
                        && (key.code == KeyCode::Char('Z')
                            || (key.code == KeyCode::Char('z')
                                && key.modifiers.contains(KeyModifiers::SHIFT)));

                    if is_z {
                        if last_key_was_z {
                            break;
                        }
                        last_key_was_z = true;
                        continue;
                    } else {
                        last_key_was_z = false;
                    }

                    // Intercept character for Vim 'r' replacement
                    if app.replace_next_char {
                        app.replace_next_char = false;
                        if let KeyCode::Char(c) = key.code {
                            let row = app.editor_state.cursor.row;
                            let col = app.editor_state.cursor.col;
                            if let Some(line) = app.editor_state.lines.get_mut(RowIndex::new(row))
                                && col < line.len()
                            {
                                line[col] = c;
                                app.re_evaluate_calculations();
                                let _ = app.save_current_note();
                            }
                        }
                        app.update_highlights();
                        continue;
                    }

                    // Global help modal toggle (F1 works in any mode, ~ works only when not in insert mode)
                    let is_insert_mode = app.focused_panel == FocusedPanel::Editor
                        && app.editor_state.mode == EditorMode::Insert;

                    // Trigger 'r' replacement in Normal mode
                    if app.focused_panel == FocusedPanel::Editor
                        && app.editor_state.mode == EditorMode::Normal
                        && key.code == KeyCode::Char('r')
                        && key.modifiers.is_empty()
                    {
                        app.replace_next_char = true;
                        continue;
                    }
                    if key.code == KeyCode::F(1) {
                        app.show_help = !app.show_help;
                        if app.show_help {
                            app.help_tab_idx = 0;
                            app.help_scroll = 0;
                        }
                        continue;
                    }
                    // Global search toggle '/'
                    if key.code == KeyCode::Char('/') && !is_insert_mode && !app.search_active {
                        app.search_active = true;
                        app.search_query.clear();
                        app.show_search_results = false;
                        continue;
                    }
                    // Ctrl-s: Save current note explicitly
                    if key.code == KeyCode::Char('s')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        match app.save_current_note() {
                            Ok(()) => {
                                app.set_status_message("Saved current note".to_string());
                            }
                            Err(e) => {
                                app.set_status_message(format!("Save failed: {}", e));
                            }
                        }
                        app.update_highlights();
                        continue;
                    }
                    // Ctrl-e: Open Export Menu
                    if key.code == KeyCode::Char('e')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        app.show_export_menu = true;
                        app.update_highlights();
                        continue;
                    }
                    // Global panel toggles
                    if key.code == KeyCode::F(2) {
                        app.left_panel_open = !app.left_panel_open;
                        if !app.left_panel_open && app.focused_panel == FocusedPanel::WikiMap {
                            app.focused_panel = FocusedPanel::Editor;
                        }
                        app.update_highlights();
                        continue;
                    }
                    if key.code == KeyCode::F(3) {
                        app.right_panel_open = !app.right_panel_open;
                        if !app.right_panel_open && app.focused_panel == FocusedPanel::Variables {
                            app.focused_panel = FocusedPanel::Editor;
                        }
                        app.update_highlights();
                        continue;
                    }
                    if key.code == KeyCode::F(4) {
                        app.config.word_wrap = !app.config.word_wrap;
                        let _ = app.config.save();
                        let status = if app.config.word_wrap {
                            "enabled"
                        } else {
                            "disabled"
                        };
                        app.set_status_message(format!("Word wrapping {}", status));
                        app.update_highlights();
                        continue;
                    }

                    // Focus switching via Shift-H / Shift-L / Ctrl-h / Ctrl-l
                    let is_switch_left = (key.code == KeyCode::Char('h')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                        || ((key.code == KeyCode::Char('H')
                            || (key.code == KeyCode::Char('h')
                                && key.modifiers.contains(KeyModifiers::SHIFT)))
                            && (app.focused_panel != FocusedPanel::Editor
                                || app.editor_state.mode == EditorMode::Normal
                                || app.editor_state.mode == EditorMode::Visual));

                    let is_switch_right = (key.code == KeyCode::Char('l')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                        || ((key.code == KeyCode::Char('L')
                            || (key.code == KeyCode::Char('l')
                                && key.modifiers.contains(KeyModifiers::SHIFT)))
                            && (app.focused_panel != FocusedPanel::Editor
                                || app.editor_state.mode == EditorMode::Normal
                                || app.editor_state.mode == EditorMode::Visual));

                    if is_switch_left {
                        app.vim_multiplier = None;
                        match app.focused_panel {
                            FocusedPanel::Editor => {
                                if app.left_panel_open {
                                    app.focused_panel = FocusedPanel::WikiMap;
                                }
                            }
                            FocusedPanel::Variables => {
                                app.focused_panel = FocusedPanel::Editor;
                            }
                            FocusedPanel::WikiMap => {}
                        }
                        app.update_highlights();
                        continue;
                    }
                    if is_switch_right {
                        app.vim_multiplier = None;
                        match app.focused_panel {
                            FocusedPanel::Editor => {
                                if app.right_panel_open {
                                    app.focused_panel = FocusedPanel::Variables;
                                    app.selected_var_idx = 0;
                                }
                            }
                            FocusedPanel::WikiMap => {
                                app.focused_panel = FocusedPanel::Editor;
                            }
                            FocusedPanel::Variables => {}
                        }
                        app.update_highlights();
                        continue;
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
