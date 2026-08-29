//! Keyboard input handlers split out of `run_app`. Each modal handler runs the
//! body of its `if app.show_* { … }` arm; the gate and the loop `continue` stay
//! in `run_app`. Mirrors the existing `handle_modal_key` (help modal) pattern.

use crate::edtui::clipboard::ClipboardTrait;
use crate::edtui::{EditorMode, RowIndex};
use crate::{App, FocusedPanel, SystemClipboard, is_repeatable_motion};
use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers, MouseEvent};
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

pub(crate) fn handle_command(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.command_active = false;
        }
        KeyCode::Enter => {
            app.command_active = false;
            let cmd = std::mem::take(&mut app.command_query);
            dispatch_command(app, &cmd);
        }
        KeyCode::Backspace => {
            app.command_query.pop();
        }
        KeyCode::Char(c) => {
            app.command_query.push(c);
        }
        _ => {}
    }
    app.update_highlights();
}

/// Parse and run an ex-style command line (the text typed after `:`).
pub(crate) fn dispatch_command(app: &mut App, input: &str) {
    let input = input.trim();
    let (cmd, arg) = match input.split_once(char::is_whitespace) {
        Some((c, a)) => (c, a.trim()),
        None => (input, ""),
    };
    match cmd {
        "" => {}
        // Vim-style quit aliases; calki auto-saves, so all three just exit.
        "q" | "q!" | "wq" => app.should_quit = true,
        "theme" => apply_theme_command(app, arg),
        other => app.set_status_message(format!("unknown command: {other}")),
    }
}

/// `:theme` — no arg lists available themes; an arg switches to that theme,
/// persists it to config, and re-renders live.
fn apply_theme_command(app: &mut App, name: &str) {
    if name.is_empty() {
        let themes = crate::theme::list_themes().join(", ");
        app.set_status_message(format!("themes: {themes}"));
        return;
    }
    if !crate::theme::list_themes().iter().any(|t| t == name) {
        app.set_status_message(format!("unknown theme '{name}'"));
        return;
    }
    match crate::theme::load_palette(name) {
        Ok((palette, problems)) => {
            app.palette = palette;
            app.config.theme = name.to_string();
            let _ = app.config.save();
            if problems.is_empty() {
                app.set_status_message(format!("theme: {name}"));
            } else {
                app.set_status_message(format!(
                    "theme: {name} (ignored unknown roles: {})",
                    problems.join(", ")
                ));
            }
        }
        Err(e) => app.set_status_message(format!("theme error: {e}")),
    }
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

pub(crate) fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    app.vim_multiplier = None; // Reset multiplier on mouse action
    if app.show_help {
        app.show_help = false;
    } else {
        let col = mouse.column;
        let row = mouse.row;
        let is_click = mouse.kind == event::MouseEventKind::Down(event::MouseButton::Left);

        // 1. Left Panel (Wiki Map)
        if app.left_panel_open
            && col >= app.left_area.x
            && col < app.left_area.x + app.left_area.width
            && row >= app.left_area.y
            && row < app.left_area.y + app.left_area.height
        {
            if app.config.mouse_focus_on_hover || is_click {
                app.focused_panel = FocusedPanel::WikiMap;
            }
            if is_click {
                let click_row = row as i32 - app.left_area.y as i32 - 1;
                if click_row >= 0 {
                    let row_map = app.get_left_panel_row_map();
                    if let Some(&idx) = row_map.get(&(click_row as usize)) {
                        app.selected_link_idx = idx;
                        let links = app.get_wiki_map_selectable_links();
                        if idx < links.len() {
                            let target_name = &links[idx];
                            let target_path = app.wiki_mgr.link_to_path(target_name);
                            let _ = app.save_current_note();
                            app.history_stack.push(app.active_path.clone());
                            let _ = app.load_note(target_path);
                            app.focused_panel = FocusedPanel::Editor;
                        }
                    }
                }
            }
        }
        // 2. Right Panel (Variables Inspector)
        else if app.right_panel_open
            && col >= app.right_area.x
            && col < app.right_area.x + app.right_area.width
            && row >= app.right_area.y
            && row < app.right_area.y + app.right_area.height
        {
            if app.config.mouse_focus_on_hover || is_click {
                app.focused_panel = FocusedPanel::Variables;
            }
            if is_click {
                let click_row = row as i32 - app.right_area.y as i32 - 1;
                if click_row >= 0 && (click_row as usize) < app.variables_cache.len() {
                    app.selected_var_idx = click_row as usize;
                }
            }
        }
        // 3. Middle Panel (Editor)
        else if col >= app.editor_area.x
            && col < app.editor_area.x + app.editor_area.width
            && row >= app.editor_area.y
            && row < app.editor_area.y + app.editor_area.height
        {
            if app.config.mouse_focus_on_hover || is_click {
                app.focused_panel = FocusedPanel::Editor;
            }
            if app.focused_panel == FocusedPanel::Editor {
                app.editor_event_handler
                    .on_mouse_event(mouse, &mut app.editor_state);
                if is_click && app.editor_state.mode == EditorMode::Normal {
                    app.follow_link_under_cursor();
                }
            }
        }
    }
    app.update_highlights();
}

/// Control-flow signal returned by the global-key handler back to the run loop.
pub(crate) enum Flow {
    Continue,
    Break,
    Pass,
}

pub(crate) fn handle_global_keys(app: &mut App, key: KeyEvent, last_key_was_z: &mut bool) -> Flow {
    // ZZ exit sequence for Vim users (Normal mode in Editor)
    let is_z = app.focused_panel == FocusedPanel::Editor
        && app.editor_state.mode == EditorMode::Normal
        && !key.modifiers.contains(KeyModifiers::CONTROL)
        && !key.modifiers.contains(KeyModifiers::ALT)
        && (key.code == KeyCode::Char('Z')
            || (key.code == KeyCode::Char('z') && key.modifiers.contains(KeyModifiers::SHIFT)));

    if is_z {
        if *last_key_was_z {
            return Flow::Break;
        }
        *last_key_was_z = true;
        return Flow::Continue;
    } else {
        *last_key_was_z = false;
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
        return Flow::Continue;
    }

    // Global help modal toggle (F1 works in any mode, ~ works only when not in insert mode)
    let is_insert_mode =
        app.focused_panel == FocusedPanel::Editor && app.editor_state.mode == EditorMode::Insert;

    // Trigger 'r' replacement in Normal mode
    if app.focused_panel == FocusedPanel::Editor
        && app.editor_state.mode == EditorMode::Normal
        && key.code == KeyCode::Char('r')
        && key.modifiers.is_empty()
    {
        app.replace_next_char = true;
        return Flow::Continue;
    }
    if key.code == KeyCode::F(1) {
        app.show_help = !app.show_help;
        if app.show_help {
            app.help_tab_idx = 0;
            app.help_scroll = 0;
        }
        return Flow::Continue;
    }
    // Global search toggle '/'
    if key.code == KeyCode::Char('/') && !is_insert_mode && !app.search_active {
        app.search_active = true;
        app.search_query.clear();
        app.show_search_results = false;
        return Flow::Continue;
    }
    // Ex-style command line ':'
    if key.code == KeyCode::Char(':')
        && !is_insert_mode
        && !app.search_active
        && !app.command_active
    {
        app.command_active = true;
        app.command_query.clear();
        return Flow::Continue;
    }
    // Ctrl-s: Save current note explicitly
    if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
        match app.save_current_note() {
            Ok(()) => {
                app.set_status_message("Saved current note".to_string());
            }
            Err(e) => {
                app.set_status_message(format!("Save failed: {}", e));
            }
        }
        app.update_highlights();
        return Flow::Continue;
    }
    // Ctrl-e: Open Export Menu
    if key.code == KeyCode::Char('e') && key.modifiers.contains(KeyModifiers::CONTROL) {
        app.show_export_menu = true;
        app.update_highlights();
        return Flow::Continue;
    }
    // Global panel toggles
    if key.code == KeyCode::F(2) {
        app.left_panel_open = !app.left_panel_open;
        if !app.left_panel_open && app.focused_panel == FocusedPanel::WikiMap {
            app.focused_panel = FocusedPanel::Editor;
        }
        app.update_highlights();
        return Flow::Continue;
    }
    if key.code == KeyCode::F(3) {
        app.right_panel_open = !app.right_panel_open;
        if !app.right_panel_open && app.focused_panel == FocusedPanel::Variables {
            app.focused_panel = FocusedPanel::Editor;
        }
        app.update_highlights();
        return Flow::Continue;
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
        return Flow::Continue;
    }

    // Focus switching via Shift-H / Shift-L / Ctrl-h / Ctrl-l
    let is_switch_left = (key.code == KeyCode::Char('h')
        && key.modifiers.contains(KeyModifiers::CONTROL))
        || ((key.code == KeyCode::Char('H')
            || (key.code == KeyCode::Char('h') && key.modifiers.contains(KeyModifiers::SHIFT)))
            && (app.focused_panel != FocusedPanel::Editor
                || app.editor_state.mode == EditorMode::Normal
                || app.editor_state.mode == EditorMode::Visual));

    let is_switch_right = (key.code == KeyCode::Char('l')
        && key.modifiers.contains(KeyModifiers::CONTROL))
        || ((key.code == KeyCode::Char('L')
            || (key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::SHIFT)))
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
        return Flow::Continue;
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
        return Flow::Continue;
    }
    Flow::Pass
}

pub(crate) fn handle_modal_key(app: &mut App, key: KeyEvent) -> bool {
    if app.show_help {
        match key.code {
            KeyCode::Char('k') | KeyCode::Char('K') | KeyCode::Up => {
                app.help_scroll = app.help_scroll.saturating_sub(1);
            }
            KeyCode::Char('j') | KeyCode::Char('J') | KeyCode::Down => {
                app.help_scroll = app.help_scroll.saturating_add(1);
            }
            KeyCode::Char('y')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                app.help_scroll = app.help_scroll.saturating_sub(1);
            }
            KeyCode::Char('e')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                app.help_scroll = app.help_scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                app.help_scroll = app.help_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                app.help_scroll = app.help_scroll.saturating_add(10);
            }
            KeyCode::Char('h') | KeyCode::Char('H') | KeyCode::Left => {
                app.help_tab_idx = if app.help_tab_idx == 0 {
                    9
                } else {
                    app.help_tab_idx - 1
                };
                app.help_scroll = 0;
            }
            KeyCode::Char('l') | KeyCode::Char('L') | KeyCode::Right => {
                app.help_tab_idx = (app.help_tab_idx + 1) % 10;
                app.help_scroll = 0;
            }
            KeyCode::Char('1') => {
                app.help_tab_idx = 0;
                app.help_scroll = 0;
            }
            KeyCode::Char('2') => {
                app.help_tab_idx = 1;
                app.help_scroll = 0;
            }
            KeyCode::Char('3') => {
                app.help_tab_idx = 2;
                app.help_scroll = 0;
            }
            KeyCode::Char('4') => {
                app.help_tab_idx = 3;
                app.help_scroll = 0;
            }
            KeyCode::Char('5') => {
                app.help_tab_idx = 4;
                app.help_scroll = 0;
            }
            KeyCode::Char('6') => {
                app.help_tab_idx = 5;
                app.help_scroll = 0;
            }
            KeyCode::Char('7') => {
                app.help_tab_idx = 6;
                app.help_scroll = 0;
            }
            KeyCode::Char('8') => {
                app.help_tab_idx = 7;
                app.help_scroll = 0;
            }
            KeyCode::Char('9') => {
                app.help_tab_idx = 8;
                app.help_scroll = 0;
            }
            KeyCode::Char('0') => {
                app.help_tab_idx = 9;
                app.help_scroll = 0;
            }
            KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('q') | KeyCode::Char('Q') => {
                app.show_help = false;
            }
            _ => {}
        }
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use std::path::PathBuf;

    /// Build a real `App` rooted at a throwaway wiki dir. Returns the app and the
    /// dir so the caller can clean it up.
    fn test_app(name: &str) -> (App, PathBuf) {
        let root = std::env::current_dir().unwrap().join(name);
        if root.exists() {
            let _ = std::fs::remove_dir_all(&root);
        }
        std::fs::create_dir_all(&root).unwrap();
        let app = App::new(root.clone()).unwrap();
        (app, root)
    }

    fn plain(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn global_f1_toggles_help_and_continues() {
        let (mut app, root) = test_app("test_gk_f1");
        let mut z = false;
        assert!(!app.show_help);
        let flow = handle_global_keys(&mut app, plain(KeyCode::F(1)), &mut z);
        assert!(matches!(flow, Flow::Continue));
        assert!(app.show_help);
        assert_eq!(app.help_tab_idx, 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn global_slash_opens_search() {
        let (mut app, root) = test_app("test_gk_slash");
        app.focused_panel = FocusedPanel::Editor;
        let mut z = false;
        let flow = handle_global_keys(&mut app, plain(KeyCode::Char('/')), &mut z);
        assert!(matches!(flow, Flow::Continue));
        assert!(app.search_active);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn global_f2_toggles_left_panel() {
        let (mut app, root) = test_app("test_gk_f2");
        let before = app.left_panel_open;
        let mut z = false;
        let flow = handle_global_keys(&mut app, plain(KeyCode::F(2)), &mut z);
        assert!(matches!(flow, Flow::Continue));
        assert_eq!(app.left_panel_open, !before);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn global_ctrl_e_opens_export_menu() {
        let (mut app, root) = test_app("test_gk_ctrle");
        let mut z = false;
        let flow = handle_global_keys(&mut app, ctrl('e'), &mut z);
        assert!(matches!(flow, Flow::Continue));
        assert!(app.show_export_menu);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn global_zz_first_arms_then_second_breaks() {
        let (mut app, root) = test_app("test_gk_zz");
        app.focused_panel = FocusedPanel::Editor;
        app.editor_state.mode = EditorMode::Normal;
        let z_key = KeyEvent::new(KeyCode::Char('Z'), KeyModifiers::NONE);
        let mut z = false;
        // First Z: arms the sequence, continues.
        let flow = handle_global_keys(&mut app, z_key, &mut z);
        assert!(matches!(flow, Flow::Continue));
        assert!(z, "first Z should set last_key_was_z");
        // Second Z: breaks the loop.
        let flow = handle_global_keys(&mut app, z_key, &mut z);
        assert!(matches!(flow, Flow::Break));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn global_ordinary_key_passes_through_and_resets_z() {
        let (mut app, root) = test_app("test_gk_pass");
        app.focused_panel = FocusedPanel::Editor;
        app.editor_state.mode = EditorMode::Normal;
        let mut z = true; // pretend a Z was pending
        let flow = handle_global_keys(&mut app, plain(KeyCode::Char('j')), &mut z);
        assert!(matches!(flow, Flow::Pass));
        assert!(!z, "a non-Z key must reset the zz latch");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn global_shift_l_focuses_variables_when_open() {
        let (mut app, root) = test_app("test_gk_shiftl");
        app.focused_panel = FocusedPanel::Editor;
        app.editor_state.mode = EditorMode::Normal;
        app.right_panel_open = true;
        let mut z = false;
        let flow = handle_global_keys(&mut app, plain(KeyCode::Char('L')), &mut z);
        assert!(matches!(flow, Flow::Continue));
        assert_eq!(app.focused_panel, FocusedPanel::Variables);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_help_and_guide_scrolling() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_scroll");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();
        let mut app = App::new(wiki_root.clone()).unwrap();

        // Test Help scroll
        app.show_help = true;
        app.help_scroll = 5;

        // Up arrow should decrease scroll
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Up, crossterm::event::KeyModifiers::NONE),
        );
        assert!(handled);
        assert_eq!(app.help_scroll, 4);

        // j key should increase scroll
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('j'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert_eq!(app.help_scroll, 5);

        // PageUp should decrease scroll by 10 (saturating at 0)
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::PageUp, crossterm::event::KeyModifiers::NONE),
        );
        assert!(handled);
        assert_eq!(app.help_scroll, 0);

        // PageDown should increase scroll by 10
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::PageDown,
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert_eq!(app.help_scroll, 10);

        // Ctrl-y should decrease scroll
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('y'),
                crossterm::event::KeyModifiers::CONTROL,
            ),
        );
        assert!(handled);
        assert_eq!(app.help_scroll, 9);

        // Ctrl-e should increase scroll
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('e'),
                crossterm::event::KeyModifiers::CONTROL,
            ),
        );
        assert!(handled);
        assert_eq!(app.help_scroll, 10);

        // Press '2' should switch to tab 1 and reset scroll to 0
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('2'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert!(app.show_help);
        assert_eq!(app.help_tab_idx, 1);
        assert_eq!(app.help_scroll, 0);

        // Press '5' should switch to tab 4
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('5'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert!(app.show_help);
        assert_eq!(app.help_tab_idx, 4);

        // Press '1' should switch to tab 0
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('1'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert!(app.show_help);
        assert_eq!(app.help_tab_idx, 0);

        // Press '6' should switch to tab 5
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('6'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert!(app.show_help);
        assert_eq!(app.help_tab_idx, 5);

        // Press '7' should switch to tab 6
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('7'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert!(app.show_help);
        assert_eq!(app.help_tab_idx, 6);

        // Press '8' should switch to tab 7
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('8'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert!(app.show_help);
        assert_eq!(app.help_tab_idx, 7);

        // Press '9' should switch to tab 8
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('9'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert!(app.show_help);
        assert_eq!(app.help_tab_idx, 8);

        // Press '0' should switch to the 10th tab (About, index 9)
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('0'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert!(app.show_help);
        assert_eq!(app.help_tab_idx, 9);

        // Other key (Esc) should close modal
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE),
        );
        assert!(handled);
        assert!(!app.show_help);

        // Clean up
        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn global_colon_opens_command_line() {
        let (mut app, root) = test_app("test_gk_colon");
        let mut z = false;
        assert!(!app.command_active);
        let flow = handle_global_keys(&mut app, plain(KeyCode::Char(':')), &mut z);
        assert!(matches!(flow, Flow::Continue));
        assert!(app.command_active);
        assert!(app.command_query.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn command_theme_switches_palette_and_persists() {
        let (mut app, root) = test_app("test_cmd_theme_ok");
        assert_eq!(app.palette, crate::theme::tokyo_night_night());
        dispatch_command(&mut app, "theme dracula");
        assert_ne!(app.palette, crate::theme::tokyo_night_night());
        assert_eq!(app.config.theme, "dracula");
        assert_eq!(
            app.palette,
            crate::theme::load_palette("dracula").unwrap().0
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn command_theme_unknown_leaves_palette_unchanged() {
        let (mut app, root) = test_app("test_cmd_theme_bad");
        dispatch_command(&mut app, "theme not-a-real-theme");
        assert_eq!(app.palette, crate::theme::tokyo_night_night());
        assert_eq!(app.config.theme, "tokyo-night");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn command_unknown_reports_error() {
        let (mut app, root) = test_app("test_cmd_unknown");
        dispatch_command(&mut app, "frobnicate");
        assert_eq!(app.palette, crate::theme::tokyo_night_night());
        let (msg, _) = app.status_message.as_ref().unwrap();
        assert!(msg.contains("unknown command"), "got: {msg}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn command_quit_aliases_set_should_quit() {
        for cmd in ["q", "q!", "wq"] {
            let (mut app, root) = test_app(&format!("test_cmd_{}", cmd.replace('!', "bang")));
            assert!(!app.should_quit);
            dispatch_command(&mut app, cmd);
            assert!(app.should_quit, "`:{cmd}` should request quit");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn command_handle_enter_dispatches_and_closes() {
        let (mut app, root) = test_app("test_cmd_enter");
        app.command_active = true;
        app.command_query = "theme kemika-purple".to_string();
        handle_command(&mut app, plain(KeyCode::Enter));
        assert!(!app.command_active);
        assert_eq!(app.config.theme, "kemika-purple");
        assert_eq!(
            app.palette,
            crate::theme::load_palette("kemika-purple").unwrap().0
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
