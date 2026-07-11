//! Keyboard input handlers split out of `run_app`. Each modal handler runs the
//! body of its `if app.show_* { … }` arm; the gate and the loop `continue` stay
//! in `run_app`. Mirrors the existing `handle_modal_key` (help modal) pattern.

use crate::edtui::EditorMode;
use crate::edtui::clipboard::ClipboardTrait;
use crate::{App, FocusedPanel, SystemClipboard, is_repeatable_motion};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::fs;

pub(crate) fn handle_update_modal(app: &mut App, key: KeyEvent) {
    if let KeyCode::Char('i') | KeyCode::Char('I') = key.code
        && let Some(ref hash) = app.update_available
    {
        app.config.ignored_update_hash = Some(hash.clone());
        let _ = app.config.save();
    }
    app.show_update_modal = false;
    app.update_highlights();
}

pub(crate) fn handle_export_menu(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('1') => {
            app.show_export_menu = false;
            match app.export_current_note_to_html() {
                Ok(path) => {
                    let filename = path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("note.html");
                    app.set_status_message(format!(
                        "Exported {} to {}",
                        filename,
                        path.to_string_lossy()
                    ));
                }
                Err(e) => {
                    app.set_status_message(format!("Export failed: {}", e));
                }
            }
        }
        KeyCode::Char('2') => {
            app.show_export_menu = false;
            match app.compile_wiki_to_markdown() {
                Ok(path) => {
                    app.set_status_message(format!("Compiled wiki to {}", path.to_string_lossy()));
                }
                Err(e) => {
                    app.set_status_message(format!("Compile failed: {}", e));
                }
            }
        }
        _ => {
            app.show_export_menu = false;
        }
    }
    app.update_highlights();
}

pub(crate) fn handle_search(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.search_active = false;
            app.show_search_results = false;
            app.search_results.clear();
        }
        KeyCode::Enter => {
            app.search_active = false;
            app.perform_wiki_search();
        }
        KeyCode::Backspace => {
            app.search_query.pop();
        }
        KeyCode::Char(c) => {
            app.search_query.push(c);
        }
        _ => {}
    }
    app.update_highlights();
}

pub(crate) fn handle_delete_confirm(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('y') | KeyCode::Char('Y') => {
            if let Some(path) = app.delete_target_path.take() {
                let _ = fs::remove_file(&path);
                app.wiki_mgr.remove_registry_entry(&path);
                if path == app.active_path {
                    let home_path = app
                        .wiki_mgr
                        .init_wiki()
                        .unwrap_or_else(|_| app.wiki_mgr.link_to_path("home"));
                    let _ = app.load_note(home_path);
                    app.history_stack.clear();
                } else {
                    app.history_stack.retain(|p| p != &path);
                    let current = app.active_path.clone();
                    let _ = app.load_note(current);
                }
            }
            app.show_delete_confirm = false;
        }
        _ => {
            app.delete_target_path = None;
            app.show_delete_confirm = false;
        }
    }
    app.update_highlights();
}

pub(crate) fn handle_editor_key(app: &mut App, key: KeyEvent) {
    let prev_mode = app.editor_state.mode;

    // Check for multiplier digits if in Normal or Visual mode
    if (prev_mode == EditorMode::Normal || prev_mode == EditorMode::Visual)
        && !app.replace_next_char
        && let KeyCode::Char(c) = key.code
        && c.is_ascii_digit()
        && (c != '0' || app.vim_multiplier.is_some())
        && key.modifiers.is_empty()
    {
        let digit = c.to_digit(10).unwrap() as usize;
        let current = app.vim_multiplier.unwrap_or(0);
        app.vim_multiplier = Some(current * 10 + digit);
        return;
    }

    let mut count = 1;
    if let Some(c) = app.vim_multiplier {
        if (prev_mode == EditorMode::Normal || prev_mode == EditorMode::Visual)
            && is_repeatable_motion(key)
        {
            count = c;
        }
        app.vim_multiplier = None; // Reset multiplier
    }

    // Intercept Enter key inside Visual Mode
    if key.code == KeyCode::Enter && prev_mode == EditorMode::Visual {
        app.vim_multiplier = None;
        app.wrap_selection_in_link();
        return;
    }

    // Intercept Enter key in Normal Mode
    if key.code == KeyCode::Enter
        && prev_mode == EditorMode::Normal
        && app.follow_link_under_cursor()
    {
        app.vim_multiplier = None;
        return;
    }

    // Intercept 't' in Normal Mode to toggle todo item at current row
    if key.code == KeyCode::Char('t')
        && prev_mode == EditorMode::Normal
        && app.toggle_todo_at_cursor()
    {
        app.vim_multiplier = None;
        return;
    }

    // Intercept Backspace or Ctrl-o in Normal Mode to go back
    if (key.code == KeyCode::Backspace
        || (key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL)))
        && prev_mode == EditorMode::Normal
        && app.go_back()
    {
        app.vim_multiplier = None;
        return;
    }

    // Intercept Ctrl-d in Normal Mode to delete current page
    if key.code == KeyCode::Char('d')
        && key.modifiers.contains(KeyModifiers::CONTROL)
        && prev_mode == EditorMode::Normal
    {
        app.vim_multiplier = None;
        let current_title = app.wiki_mgr.path_to_title(&app.active_path);
        app.delete_target_name = current_title;
        app.delete_target_path = Some(app.active_path.clone());
        app.show_delete_confirm = true;
        return;
    }

    // Discard unsupported KeyCodes to prevent panic in edtui
    match key.code {
        KeyCode::Char(_)
        | KeyCode::Esc
        | KeyCode::Backspace
        | KeyCode::Enter
        | KeyCode::Tab
        | KeyCode::Delete
        | KeyCode::Left
        | KeyCode::Right
        | KeyCode::Up
        | KeyCode::Down
        | KeyCode::Home
        | KeyCode::End
        | KeyCode::PageUp
        | KeyCode::PageDown => {}
        _ => {
            app.vim_multiplier = None;
            return;
        }
    }

    // Send event to Editor state
    let lines_before = if prev_mode == EditorMode::Normal {
        Some(app.editor_state.lines.clone())
    } else {
        None
    };

    for _ in 0..count {
        app.editor_event_handler
            .on_key_event(key, &mut app.editor_state);
    }

    // Trigger math calculation update on exiting Insert Mode or on Normal Mode edits
    if prev_mode == EditorMode::Insert && app.editor_state.mode == EditorMode::Normal {
        app.re_evaluate_calculations();
        app.update_outgoing_links();
    } else if let Some(ref before) = lines_before
        && before != &app.editor_state.lines
    {
        app.re_evaluate_calculations();
        app.update_outgoing_links();
    }
    app.update_highlights();
}

pub(crate) fn handle_wikimap_key(app: &mut App, key: KeyEvent) {
    let links = app.get_wiki_map_selectable_links();
    if !links.is_empty() {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') if app.selected_link_idx > 0 => {
                app.selected_link_idx -= 1;
            }
            KeyCode::Down | KeyCode::Char('j') if app.selected_link_idx < links.len() - 1 => {
                app.selected_link_idx += 1;
            }
            KeyCode::Enter => {
                let target_name = &links[app.selected_link_idx];
                let target_path = app.wiki_mgr.link_to_path(target_name);
                let _ = app.save_current_note();
                app.history_stack.push(app.active_path.clone());
                let _ = app.load_note(target_path);
                app.focused_panel = FocusedPanel::Editor; // return focus
            }
            KeyCode::Char('d') | KeyCode::Char('x') | KeyCode::Delete => {
                let target_name = &links[app.selected_link_idx];
                let target_path = app.wiki_mgr.link_to_path(target_name);
                if target_path.exists() {
                    app.delete_target_name = target_name.clone();
                    app.delete_target_path = Some(target_path);
                    app.show_delete_confirm = true;
                }
            }
            KeyCode::Esc => {
                app.focused_panel = FocusedPanel::Editor;
            }
            _ => {}
        }
    } else {
        if key.code == KeyCode::Esc {
            app.focused_panel = FocusedPanel::Editor;
        }
    }
    app.update_highlights();
}

pub(crate) fn handle_variables_key(app: &mut App, key: KeyEvent) {
    let vars_len = app.variables_cache.len();
    match key.code {
        KeyCode::Up | KeyCode::Char('k') if vars_len > 0 && app.selected_var_idx > 0 => {
            app.selected_var_idx -= 1;
        }
        KeyCode::Down | KeyCode::Char('j')
            if vars_len > 0 && app.selected_var_idx < vars_len - 1 =>
        {
            app.selected_var_idx += 1;
        }
        KeyCode::Char('y') if vars_len > 0 && app.selected_var_idx < vars_len => {
            let (_, ref val) = app.variables_cache[app.selected_var_idx];
            let mut clip = SystemClipboard::new();
            clip.set_text(val.clone());
        }
        KeyCode::Enter | KeyCode::Char('i') if vars_len > 0 && app.selected_var_idx < vars_len => {
            let name = app.variables_cache[app.selected_var_idx].0.clone();
            app.insert_text_at_cursor(&name);
            app.focused_panel = FocusedPanel::Editor;
        }
        KeyCode::Esc => {
            app.focused_panel = FocusedPanel::Editor;
        }
        KeyCode::Char('h') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.focused_panel = FocusedPanel::Editor;
        }
        _ => {}
    }
    app.update_highlights();
}
