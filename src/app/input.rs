//! Keyboard input handlers split out of `run_app`. Each modal handler runs the
//! body of its `if app.show_* { … }` arm; the gate and the loop `continue` stay
//! in `run_app`. Mirrors the existing `handle_modal_key` (help modal) pattern.

use crate::App;
use crossterm::event::{KeyCode, KeyEvent};
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
