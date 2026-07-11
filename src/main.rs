mod app;
mod currency;
mod highlight;
mod math;
mod ui;
mod wiki;

use crate::currency::{load_currency_rates, trigger_background_update};
use crate::math::evaluate_sheet;
use crate::math::units::get_unit_info;
use crate::wiki::WikiManager;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use ratatui::prelude::*;

pub mod edtui;

use crate::edtui::actions::Chainable;
use crate::edtui::clipboard::ClipboardTrait;
use crate::edtui::events::{KeyEventRegister, KeyInput};
use crate::edtui::{EditorEventHandler, EditorMode, EditorState, Lines, RowIndex};
use serde::{Deserialize, Serialize};
#[cfg(not(test))]
use std::io::Write;

struct SystemClipboard {
    #[allow(dead_code)]
    arboard_clip: Option<arboard::Clipboard>,
    internal: String,
}

impl SystemClipboard {
    fn new() -> Self {
        #[allow(unused_mut, unused_variables)]
        let arboard_clip = arboard::Clipboard::new().ok();
        Self {
            arboard_clip,
            internal: String::new(),
        }
    }
}

#[cfg(not(test))]
fn encode_base64(input: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(input.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < input.len() {
        let chunk = &input[i..std::cmp::min(i + 3, input.len())];
        let mut b = 0u32;
        for &val in chunk {
            b = (b << 8) | val as u32;
        }
        let pad = 3 - chunk.len();
        b <<= pad * 8;

        let c1 = (b >> 18) & 63;
        let c2 = (b >> 12) & 63;
        let c3 = (b >> 6) & 63;
        let c4 = b & 63;

        result.push(CHARSET[c1 as usize] as char);
        result.push(CHARSET[c2 as usize] as char);
        if pad < 2 {
            result.push(CHARSET[c3 as usize] as char);
        } else {
            result.push('=');
        }
        if pad < 1 {
            result.push(CHARSET[c4 as usize] as char);
        } else {
            result.push('=');
        }
        i += 3;
    }
    result
}

impl ClipboardTrait for SystemClipboard {
    fn set_text(&mut self, text: String) {
        self.internal = text.clone();

        #[cfg(not(test))]
        {
            // 1. Try local arboard system clipboard
            if let Some(ref mut clip) = self.arboard_clip {
                let _ = clip.set_text(text.clone());
            }

            // 2. Write to terminal using OSC 52 escape sequence
            let b64 = encode_base64(text.as_bytes());
            let osc52 = format!("\x1b]52;c;{}\x07", b64);

            // If in tmux, wrap it in tmux passthrough
            let is_tmux = std::env::var("TMUX").is_ok();
            let payload = if is_tmux {
                format!("\x1bPtmux;\x1b\x1b]52;c;{}\x07\x1b\\", b64)
            } else {
                osc52
            };

            let mut stdout = std::io::stdout();
            let _ = stdout.write_all(payload.as_bytes());
            let _ = stdout.flush();
        }
    }

    fn get_text(&mut self) -> String {
        #[cfg(not(test))]
        {
            // 1. Try local arboard system clipboard
            if let Some(ref mut clip) = self.arboard_clip
                && let Ok(txt) = clip.get_text()
            {
                self.internal = txt;
                return self.internal.clone();
            }
        }
        // Fall back to internal clipboard
        self.internal.clone()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct SessionState {
    active_path: String,
    cursor_row: usize,
    cursor_col: usize,
    focused_panel: String, // "WikiMap", "Editor", "Variables"
    left_panel_open: bool,
    right_panel_open: bool,
    #[serde(default)]
    history_stack: Vec<String>,
}

impl SessionState {
    fn load() -> Option<Self> {
        #[cfg(test)]
        {
            None
        }
        #[cfg(not(test))]
        {
            let mut path = crate::currency::get_config_path()?;
            path.push("session.json");
            let file = fs::File::open(path).ok()?;
            serde_json::from_reader(file).ok()
        }
    }

    fn save(&self) -> Option<()> {
        #[cfg(test)]
        {
            Some(())
        }
        #[cfg(not(test))]
        {
            let mut path = crate::currency::get_config_path()?;
            fs::create_dir_all(&path).ok()?;
            path.push("session.json");
            let file = fs::File::create(path).ok()?;
            serde_json::to_writer_pretty(file, self).ok()?;
            Some(())
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
struct AppConfig {
    scrolloff: usize,
    mouse_focus_on_hover: bool,
    expand_variables_on_select: bool,
    ignored_update_hash: Option<String>,
    line_numbers: String,
    word_wrap: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            scrolloff: 5,
            mouse_focus_on_hover: true,
            expand_variables_on_select: false,
            ignored_update_hash: None,
            line_numbers: "None".to_string(),
            word_wrap: true,
        }
    }
}

impl AppConfig {
    fn load() -> Self {
        #[cfg(test)]
        {
            AppConfig::default()
        }
        #[cfg(not(test))]
        {
            if let Some(mut path) = crate::currency::get_config_path() {
                path.push("config.json");
                if path.exists()
                    && let Ok(content) = fs::read_to_string(path)
                    && let Ok(config) = serde_json::from_str::<AppConfig>(&content)
                {
                    return config;
                }
            }
            AppConfig::default()
        }
    }

    fn save(&self) -> Option<()> {
        #[cfg(test)]
        {
            Some(())
        }
        #[cfg(not(test))]
        {
            let mut path = crate::currency::get_config_path()?;
            fs::create_dir_all(&path).ok()?;
            path.push("config.json");
            let file = fs::File::create(path).ok()?;
            serde_json::to_writer_pretty(file, self).ok()?;
            Some(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum FocusedPanel {
    WikiMap,
    Editor,
    Variables,
}

struct App {
    wiki_mgr: WikiManager,
    active_path: PathBuf,
    history_stack: Vec<PathBuf>,

    // Editor widget state
    editor_state: EditorState,
    editor_event_handler: EditorEventHandler,

    // Toggles & Focus
    left_panel_open: bool,
    right_panel_open: bool,
    focused_panel: FocusedPanel,

    // Caches
    variables_cache: Vec<(String, String)>,
    backlinks: Vec<String>,
    outgoing: Vec<String>,
    selected_link_idx: usize,            // Selected link in Wiki Map panel
    selected_var_idx: usize,             // Selected variable in Variables panel
    show_help: bool,                     // Whether to display the help modal
    help_tab_idx: usize,                 // Active tab in help modal
    help_scroll: u16,                    // Scroll offset in help modal
    show_delete_confirm: bool,           // Whether to display the delete confirmation modal
    delete_target_name: String,          // Name of page to delete
    delete_target_path: Option<PathBuf>, // Path of page to delete

    // Exchange rates
    exchange_rates: HashMap<String, f64>,

    // Panel screen areas for mouse clicks
    left_area: Rect,
    editor_area: Rect,
    right_area: Rect,
    replace_next_char: bool,
    vim_multiplier: Option<usize>,
    config: AppConfig,

    // Global Wiki Search
    search_query: String,
    search_active: bool,
    search_results: Vec<String>,
    show_search_results: bool,

    // Status Message / Toast
    status_message: Option<(String, std::time::Instant)>,

    // Update checking
    update_receiver: Option<std::sync::mpsc::Receiver<String>>,
    update_available: Option<String>,
    show_update_modal: bool,
    show_export_menu: bool,
}

fn trim_char_slice(mut slice: &[char]) -> &[char] {
    while let Some((first, rest)) = slice.split_first() {
        if first.is_whitespace() {
            slice = rest;
        } else {
            break;
        }
    }
    while let Some((last, rest)) = slice.split_last() {
        if last.is_whitespace() {
            slice = rest;
        } else {
            break;
        }
    }
    slice
}

fn trim_start_slice(mut slice: &[char]) -> &[char] {
    while let Some((first, rest)) = slice.split_first() {
        if first.is_whitespace() {
            slice = rest;
        } else {
            break;
        }
    }
    slice
}

fn is_repeatable_motion(key: crossterm::event::KeyEvent) -> bool {
    key.modifiers.is_empty()
        && matches!(
            key.code,
            KeyCode::Char('j')
                | KeyCode::Char('k')
                | KeyCode::Char('h')
                | KeyCode::Char('l')
                | KeyCode::Char('w')
                | KeyCode::Char('b')
                | KeyCode::Char('e')
                | KeyCode::Char('x')
                | KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::PageUp
                | KeyCode::PageDown
        )
}

fn is_newer_version(local: &str, remote: &str) -> bool {
    let local_parts: Vec<u32> = local.split('.').filter_map(|s| s.parse().ok()).collect();
    let remote_parts: Vec<u32> = remote.split('.').filter_map(|s| s.parse().ok()).collect();

    if local_parts.len() == 3 && remote_parts.len() == 3 {
        for i in 0..3 {
            if remote_parts[i] > local_parts[i] {
                return true;
            } else if remote_parts[i] < local_parts[i] {
                return false;
            }
        }
    }
    false
}

fn check_for_updates() -> Option<std::sync::mpsc::Receiver<String>> {
    #[cfg(test)]
    {
        None
    }
    #[cfg(not(test))]
    {
        let (tx, rx) = std::sync::mpsc::channel();

        std::thread::spawn(move || {
            let url = "https://crates.io/api/v1/crates/calki";
            let agent = ureq::AgentBuilder::new()
                .timeout(std::time::Duration::from_secs(3))
                .build();

            let response = match agent.get(url).set("User-Agent", "calki-app-updater").call() {
                Ok(res) => res,
                Err(_) => return,
            };

            let json_val: serde_json::Value = match response.into_json() {
                Ok(json) => json,
                Err(_) => return,
            };

            if let Some(remote_version) = json_val["crate"]["max_version"].as_str() {
                let remote_version = remote_version.trim().to_string();
                let local_version = env!("CARGO_PKG_VERSION");
                if is_newer_version(local_version, &remote_version) {
                    let _ = tx.send(remote_version);
                }
            }
        });

        Some(rx)
    }
}

impl App {
    fn new(wiki_root: PathBuf) -> Result<Self, String> {
        let wiki_mgr = WikiManager::new(wiki_root);
        let home_path = wiki_mgr.init_wiki()?;

        let session = SessionState::load();
        let active_path = if let Some(ref s) = session {
            let path = PathBuf::from(&s.active_path);
            if path.exists() {
                path
            } else {
                home_path.clone()
            }
        } else {
            home_path.clone()
        };

        let file_content = fs::read_to_string(&active_path)
            .map_err(|e| format!("Failed to read active note: {}", e))?;

        let rates_cache = load_currency_rates();

        let mut editor_event_handler = EditorEventHandler::default();

        // Register custom Vim "c" (change) motions:
        // 1. cw (Change Word)
        editor_event_handler.key_handler.insert(
            KeyEventRegister::n(vec![KeyInput::new('c'), KeyInput::new('w')]),
            edtui::actions::DeleteWordForward(1)
                .chain(edtui::actions::SwitchMode(EditorMode::Insert)),
        );

        // 2. cc (Change Line)
        editor_event_handler.key_handler.insert(
            KeyEventRegister::n(vec![KeyInput::new('c'), KeyInput::new('c')]),
            edtui::actions::MoveToStartOfLine()
                .chain(edtui::actions::delete::DeleteToEndOfLine)
                .chain(edtui::actions::SwitchMode(EditorMode::Insert)),
        );

        // 3. C (Change to End of Line)
        editor_event_handler.key_handler.insert(
            KeyEventRegister::n(vec![KeyInput::shift('C')]),
            edtui::actions::delete::DeleteToEndOfLine
                .chain(edtui::actions::SwitchMode(EditorMode::Insert)),
        );

        // 4. ^ (Move to First Non-Blank Character)
        editor_event_handler.key_handler.insert(
            KeyEventRegister::n(vec![KeyInput::new('^')]),
            edtui::actions::motion::MoveToFirst(),
        );
        editor_event_handler.key_handler.insert(
            KeyEventRegister::v(vec![KeyInput::new('^')]),
            edtui::actions::motion::MoveToFirst(),
        );

        let mut editor_state = EditorState::new(Lines::from(file_content.as_str()));
        editor_state.set_clipboard(SystemClipboard::new());

        // Restore cursor from local wiki manager cursors registry as default
        if let Some((row, col)) = wiki_mgr.get_cursor_position(&active_path) {
            let row_count = editor_state.lines.len();
            if row_count > 0 {
                let target_row = row.min(row_count - 1);
                let col_count = editor_state
                    .lines
                    .get(RowIndex::new(target_row))
                    .map(|r| r.len())
                    .unwrap_or(0);
                let target_col = col.min(col_count);
                editor_state.cursor = edtui::Index2::new(target_row, target_col);
            }
        }

        let left_panel_open = session.as_ref().map(|s| s.left_panel_open).unwrap_or(true);
        let right_panel_open = session.as_ref().map(|s| s.right_panel_open).unwrap_or(true);
        let mut focused_panel = session
            .as_ref()
            .map(|s| match s.focused_panel.as_str() {
                "WikiMap" => FocusedPanel::WikiMap,
                "Variables" => FocusedPanel::Variables,
                _ => FocusedPanel::Editor,
            })
            .unwrap_or(FocusedPanel::Editor);
        if focused_panel == FocusedPanel::WikiMap && !left_panel_open {
            focused_panel = FocusedPanel::Editor;
        }
        if focused_panel == FocusedPanel::Variables && !right_panel_open {
            focused_panel = FocusedPanel::Editor;
        }

        let config = AppConfig::load();
        let _ = config.save();

        let update_receiver = check_for_updates();

        let history_stack = if let Some(ref s) = session {
            s.history_stack.iter().map(PathBuf::from).collect()
        } else {
            Vec::new()
        };

        let mut app = Self {
            wiki_mgr,
            active_path,
            history_stack,
            editor_state,
            editor_event_handler,
            left_panel_open,
            right_panel_open,
            focused_panel,
            variables_cache: Vec::new(),
            backlinks: Vec::new(),
            outgoing: Vec::new(),
            selected_link_idx: 0,
            selected_var_idx: 0,
            show_help: false,
            help_tab_idx: 0,
            help_scroll: 0,
            show_delete_confirm: false,
            delete_target_name: String::new(),
            delete_target_path: None,
            exchange_rates: rates_cache.rates,
            left_area: Rect::default(),
            editor_area: Rect::default(),
            right_area: Rect::default(),
            replace_next_char: false,
            vim_multiplier: None,
            config,
            search_query: String::new(),
            search_active: false,
            search_results: Vec::new(),
            show_search_results: false,
            status_message: None,
            update_receiver,
            update_available: None,
            show_update_modal: false,
            show_export_menu: false,
        };

        if let Some(ref s) = session {
            let row_count = app.editor_state.lines.len();
            if row_count > 0 {
                let target_row = s.cursor_row.min(row_count - 1);
                let col_count = app
                    .editor_state
                    .lines
                    .get(RowIndex::new(target_row))
                    .map(|r| r.len())
                    .unwrap_or(0);
                let target_col = if col_count > 0 {
                    s.cursor_col.min(col_count.saturating_sub(1))
                } else {
                    0
                };
                app.editor_state.cursor = edtui::Index2::new(target_row, target_col);
            }
        }

        app.re_evaluate_calculations();
        app.update_wiki_map();
        Ok(app)
    }

    // Converts editor lines back to String
    fn get_editor_text(&self) -> String {
        self.editor_state
            .lines
            .iter_row()
            .map(|row| row.iter().collect::<String>())
            .collect::<Vec<String>>()
            .join("\n")
    }

    // Runs math evaluation and updates buffer and variables cache
    fn re_evaluate_calculations(&mut self) {
        // We test with a dummy frame cursor call or cargo check import
        let current_text = self.get_editor_text();
        let (updated_text, vars) = evaluate_sheet(&current_text, &self.exchange_rates);
        self.variables_cache = vars;

        if updated_text != current_text {
            // Keep cursor position
            let cursor = self.editor_state.cursor;
            self.editor_state.lines = Lines::from(updated_text.as_str());

            // Clamp cursor to new buffer dimensions
            let max_row = self.editor_state.lines.len().saturating_sub(1);
            let mut target_row = cursor.row;
            if target_row > max_row {
                target_row = max_row;
            }
            self.editor_state.cursor.row = target_row;

            let row_len = self
                .editor_state
                .lines
                .get(RowIndex::new(target_row))
                .map(|r| r.len())
                .unwrap_or(0);
            let max_col = row_len.saturating_sub(1);
            if self.editor_state.cursor.col > max_col {
                self.editor_state.cursor.col = max_col;
            }
        }
    }

    // Updates outgoing links and backlinks caches
    fn update_wiki_map(&mut self) {
        self.outgoing = self.wiki_mgr.scan_outgoing_links(&self.active_path);
        self.backlinks = self.wiki_mgr.scan_backlinks(&self.active_path);

        let total_links = self.backlinks.len() + self.outgoing.len();
        if self.selected_link_idx >= total_links {
            self.selected_link_idx = total_links.saturating_sub(1);
        }
    }

    // Updates outgoing links only (useful when editing active file, avoiding directory-wide backlink scans)
    fn update_outgoing_links(&mut self) {
        self.outgoing = self.wiki_mgr.scan_outgoing_links(&self.active_path);

        let total_links = self.backlinks.len() + self.outgoing.len();
        if self.selected_link_idx >= total_links {
            self.selected_link_idx = total_links.saturating_sub(1);
        }
    }

    // Updates highlights based on syntax highlighting and selected variable
    fn update_highlights(&mut self) {
        let vecs: Vec<&[char]> = self
            .editor_state
            .lines
            .iter_row()
            .map(|r| r.as_slice())
            .collect();
        let selected_var =
            if self.focused_panel == FocusedPanel::Variables && !self.variables_cache.is_empty() {
                if self.selected_var_idx >= self.variables_cache.len() {
                    self.selected_var_idx = self.variables_cache.len().saturating_sub(1);
                }
                Some(self.variables_cache[self.selected_var_idx].0.as_str())
            } else {
                None
            };

        self.editor_state.highlights =
            crate::highlight::compute_syntax_highlights(&vecs, selected_var);
    }

    // Saves current editor state to the active note file
    fn save_current_note(&self) -> Result<(), String> {
        let content = self.get_editor_text();
        let row = self.editor_state.cursor.row;
        let col = self.editor_state.cursor.col;
        self.wiki_mgr
            .save_cursor_position(&self.active_path, row, col);
        fs::write(&self.active_path, content).map_err(|e| format!("Failed to write note: {}", e))
    }

    // Load a note file into the editor, handling onboarding or template creation
    fn load_note(&mut self, path: PathBuf) -> Result<(), String> {
        self.active_path = path;

        if !self.active_path.exists() {
            let title = self.wiki_mgr.path_to_title(&self.active_path);
            let default_template = format!(
                "# {}\n\nCreate your calculations here...\n\nSee [[Home]] to go back.\n",
                title
            );
            fs::write(&self.active_path, default_template)
                .map_err(|e| format!("Failed to create new note: {}", e))?;
        }

        let content = fs::read_to_string(&self.active_path)
            .map_err(|e| format!("Failed to read note: {}", e))?;

        let mut editor_state = EditorState::new(Lines::from(content.as_str()));
        editor_state.set_clipboard(SystemClipboard::new());

        // Restore cursor from local wiki manager cursors registry
        if let Some((row, col)) = self.wiki_mgr.get_cursor_position(&self.active_path) {
            let row_count = editor_state.lines.len();
            if row_count > 0 {
                let target_row = row.min(row_count - 1);
                let col_count = editor_state
                    .lines
                    .get(RowIndex::new(target_row))
                    .map(|r| r.len())
                    .unwrap_or(0);
                let target_col = col.min(col_count);
                editor_state.cursor = edtui::Index2::new(target_row, target_col);
            }
        }

        self.editor_state = editor_state;
        self.re_evaluate_calculations();
        self.update_wiki_map();
        Ok(())
    }

    // Follows link under editor cursor if exists
    fn follow_link_under_cursor(&mut self) -> bool {
        let row_idx = self.editor_state.cursor.row;
        let col_idx = self.editor_state.cursor.col;

        let line_str: String = match self.editor_state.lines.get(RowIndex::new(row_idx)) {
            Some(row) => row.iter().collect(),
            None => return false,
        };

        if let Some(link) = get_any_link_under_cursor(&line_str, col_idx) {
            match link {
                LinkType::Wiki(link_name) => {
                    let target_path = self.wiki_mgr.link_to_path(&link_name);
                    let _ = self.save_current_note();
                    self.history_stack.push(self.active_path.clone());
                    let _ = self.load_note(target_path);
                    return true;
                }
                LinkType::Markdown(target) | LinkType::RawUrl(target) => {
                    if target.starts_with("http://") || target.starts_with("https://") {
                        let _ = open_system_link(&target);
                        self.set_status_message(format!("Opening link: {}", target));
                        return true;
                    }

                    let active_dir = self
                        .active_path
                        .parent()
                        .unwrap_or_else(|| self.wiki_mgr.root_dir());
                    let clean_target = if target.starts_with("file://") {
                        target.trim_start_matches("file://").to_string()
                    } else {
                        target
                    };

                    let path = PathBuf::from(&clean_target);
                    let resolved_path = if path.is_absolute() {
                        path
                    } else {
                        active_dir.join(path)
                    };

                    if resolved_path.extension().is_some_and(|ext| ext == "md") {
                        let _ = self.save_current_note();
                        self.history_stack.push(self.active_path.clone());
                        let _ = self.load_note(resolved_path);
                        return true;
                    } else if resolved_path.exists() {
                        let _ = open_system_link(&resolved_path.to_string_lossy());
                        self.set_status_message(format!(
                            "Opening file: {}",
                            resolved_path.display()
                        ));
                        return true;
                    } else {
                        let target_path = self.wiki_mgr.link_to_path(&clean_target);
                        let _ = self.save_current_note();
                        self.history_stack.push(self.active_path.clone());
                        let _ = self.load_note(target_path);
                        return true;
                    }
                }
            }
        }
        false
    }

    // Toggles todo checklist item [ ] <=> [x] at the current cursor row,
    // or converts a plain list item (starting with -, *, +) into a todo item.
    fn toggle_todo_at_cursor(&mut self) -> bool {
        let row = self.editor_state.cursor.row;
        if let Some(line) = self.editor_state.lines.get_mut(RowIndex::new(row)) {
            // 1. Search for existing checkbox [ ] or [x] or [X]
            let mut found = false;
            let mut i = 0;
            while i + 2 < line.len() {
                if line[i] == '[' && line[i + 2] == ']' {
                    let mark = line[i + 1];
                    if mark == ' ' {
                        line[i + 1] = 'x';
                        found = true;
                        break;
                    } else if mark == 'x' || mark == 'X' {
                        line[i + 1] = ' ';
                        found = true;
                        break;
                    }
                }
                i += 1;
            }

            if found {
                self.re_evaluate_calculations();
                let _ = self.save_current_note();
                self.update_highlights();
                return true;
            }

            // 2. If not found, check if it starts with a bullet/numbered list prefix and insert `[ ] `
            let line_str: String = line.iter().collect();
            let trimmed = line_str.trim_start();
            let leading_spaces = line_str.len() - trimmed.len();

            if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
                let insert_pos = leading_spaces + 2;
                let checklist = ['[', ' ', ']', ' '];
                for (offset, &c) in checklist.iter().enumerate() {
                    line.insert(insert_pos + offset, c);
                }
                self.re_evaluate_calculations();
                let _ = self.save_current_note();
                self.update_highlights();
                return true;
            }
        }
        false
    }

    // Navigates back in page history
    fn go_back(&mut self) -> bool {
        if let Some(prev_path) = self.history_stack.pop() {
            let _ = self.save_current_note();
            let _ = self.load_note(prev_path);
            true
        } else {
            // Fallback: go one level up to home.md if we aren't already there
            let home_path = self.wiki_mgr.root_dir().join("home.md");
            if self.active_path != home_path && home_path.exists() {
                let _ = self.save_current_note();
                let _ = self.load_note(home_path);
                true
            } else {
                false
            }
        }
    }

    // Converts Visual Mode selection to wiki link [[ ... ]]
    fn wrap_selection_in_link(&mut self) {
        if let Some(ref selection) = self.editor_state.selection {
            let start = selection.start;
            let end = selection.end;

            // Sort start/end coordinates to get correct text boundaries
            let (start_idx, end_idx) =
                if start.row < end.row || (start.row == end.row && start.col <= end.col) {
                    (start, end)
                } else {
                    (end, start)
                };

            let lines_str = self.get_editor_text();

            // Map 2D coordinate to 1D char index
            let start_offset = index2_to_char_offset(&self.editor_state.lines, start_idx);
            let end_offset = index2_to_char_offset(&self.editor_state.lines, end_idx) + 1;

            let chars: Vec<char> = lines_str.chars().collect();
            if start_offset <= end_offset && end_offset <= chars.len() {
                let selection_text: String = chars[start_offset..end_offset].iter().collect();

                // Wrap in double brackets
                let new_lines_str = format!(
                    "{}[[{}]]{}",
                    chars[..start_offset].iter().collect::<String>(),
                    selection_text,
                    chars[end_offset..].iter().collect::<String>()
                );

                self.editor_state.lines = Lines::from(new_lines_str.as_str());
                self.editor_state.mode = EditorMode::Normal;
                self.editor_state.selection = None;

                // Position cursor inside the new link
                self.editor_state.cursor.row = start_idx.row;
                self.editor_state.cursor.col = start_idx.col + 2;

                self.re_evaluate_calculations();
                self.update_outgoing_links();
            }
        }
    }

    fn insert_text_at_cursor(&mut self, text: &str) {
        let cursor_idx = self.editor_state.cursor;
        let lines_str = self.get_editor_text();
        let offset = index2_to_char_offset(&self.editor_state.lines, cursor_idx);

        let chars: Vec<char> = lines_str.chars().collect();
        if offset <= chars.len() {
            let new_lines_str = format!(
                "{}{}{}",
                chars[..offset].iter().collect::<String>(),
                text,
                chars[offset..].iter().collect::<String>()
            );
            self.editor_state.lines = Lines::from(new_lines_str.as_str());

            // Move cursor forward
            self.editor_state.cursor.row = cursor_idx.row;
            self.editor_state.cursor.col = cursor_idx.col + text.chars().count();

            self.re_evaluate_calculations();
            self.update_outgoing_links();
        }
    }

    // Get flat list of links in the Wiki Map
    fn get_wiki_map_selectable_links(&self) -> Vec<String> {
        let mut links = Vec::new();
        for link in &self.backlinks {
            links.push(link.clone());
        }
        for link in &self.outgoing {
            links.push(link.clone());
        }
        if self.show_search_results {
            for link in &self.search_results {
                links.push(link.clone());
            }
        }
        links
    }

    fn set_status_message<S: Into<String>>(&mut self, msg: S) {
        self.status_message = Some((msg.into(), std::time::Instant::now()));
    }

    fn perform_wiki_search(&mut self) {
        let query = self.search_query.trim().to_lowercase();
        self.search_results.clear();
        if query.is_empty() {
            self.show_search_results = false;
            return;
        }

        self.show_search_results = true;
        let entries = match fs::read_dir(self.wiki_mgr.root_dir()) {
            Ok(iter) => iter,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file()
                && path.extension().and_then(|s| s.to_str()) == Some("md")
                && let Ok(content) = fs::read_to_string(&path)
                && content.to_lowercase().contains(&query)
            {
                let title = self.wiki_mgr.path_to_title(&path);
                self.search_results.push(title);
            }
        }
        self.selected_link_idx = 0;
    }

    fn get_left_panel_row_map(&self) -> HashMap<usize, usize> {
        let mut row_map = HashMap::new();
        let mut current_row = 1;

        let mut current_link_idx = 0;
        for _ in &self.backlinks {
            row_map.insert(current_row, current_link_idx);
            current_row += 1;
            current_link_idx += 1;
        }
        if self.backlinks.is_empty() {
            current_row += 1;
        }

        current_row += 2; // spacer + header

        for _ in &self.outgoing {
            row_map.insert(current_row, current_link_idx);
            current_row += 1;
            current_link_idx += 1;
        }
        if self.outgoing.is_empty() {
            current_row += 1;
        }

        if self.show_search_results {
            current_row += 2; // spacer + header
            for _ in &self.search_results {
                row_map.insert(current_row, current_link_idx);
                current_row += 1;
                current_link_idx += 1;
            }
            if self.search_results.is_empty() {
                // no-op
            }
        }
        row_map
    }

    fn export_current_note_to_html(&self) -> Result<PathBuf, String> {
        let export_dir = self.wiki_mgr.root_dir().join("export");
        if !export_dir.exists() {
            fs::create_dir_all(&export_dir)
                .map_err(|e| format!("Failed to create export directory: {}", e))?;
        }

        let current_text = self.get_editor_text();
        let (evaluated, _) = evaluate_sheet(&current_text, &self.exchange_rates);
        let title = self.wiki_mgr.path_to_title(&self.active_path);

        let html_content = markdown_to_html(&evaluated, &title);

        let stem = self
            .active_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("note");
        let output_path = export_dir.join(format!("{}.html", stem));

        fs::write(&output_path, html_content)
            .map_err(|e| format!("Failed to write HTML file: {}", e))?;

        Ok(output_path)
    }

    fn compile_wiki_to_markdown(&self) -> Result<PathBuf, String> {
        let export_dir = self.wiki_mgr.root_dir().join("export");
        if !export_dir.exists() {
            fs::create_dir_all(&export_dir)
                .map_err(|e| format!("Failed to create export directory: {}", e))?;
        }

        let entries = fs::read_dir(self.wiki_mgr.root_dir())
            .map_err(|e| format!("Failed to read wiki directory: {}", e))?;

        let mut paths = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md") {
                if path.parent() == Some(&export_dir) {
                    continue;
                }
                paths.push(path);
            }
        }

        paths.sort_by(|a, b| {
            let a_name = a.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let b_name = b.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if a_name == "home.md" {
                std::cmp::Ordering::Less
            } else if b_name == "home.md" {
                std::cmp::Ordering::Greater
            } else {
                a_name.cmp(b_name)
            }
        });

        let mut compiled = String::new();
        compiled.push_str("# calki Compiled Wiki 🧮 📝\n\n");
        compiled.push_str("compiled from all notes in the wiki.\n\n---\n\n");

        for path in paths {
            let content = fs::read_to_string(&path)
                .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;

            let (evaluated, _) = evaluate_sheet(&content, &self.exchange_rates);
            compiled.push_str(&evaluated);
            compiled.push_str("\n\n---\n\n");
        }

        let output_path = export_dir.join("wiki_compiled.md");
        fs::write(&output_path, compiled)
            .map_err(|e| format!("Failed to write compiled markdown: {}", e))?;

        Ok(output_path)
    }
}

// Maps Index2 row/col to 1D character offset in String
fn index2_to_char_offset(lines: &Lines, idx: edtui::Index2) -> usize {
    let mut offset = 0;
    for (r, row) in lines.iter_row().enumerate() {
        if r < idx.row {
            offset += row.len() + 1; // +1 for newline character
        } else if r == idx.row {
            offset += idx.col;
            break;
        }
    }
    offset
}

#[derive(Debug, Clone, PartialEq)]
enum LinkType {
    Wiki(String),
    Markdown(String),
    RawUrl(String),
}

fn open_system_link(target: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(&["/C", "start", "", target])
            .spawn()?;
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(target).spawn()?;
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        std::process::Command::new("xdg-open").arg(target).spawn()?;
    }
    Ok(())
}

fn get_any_link_under_cursor(line: &str, col: usize) -> Option<LinkType> {
    // Try to find exact match (tolerance 0) first
    if let Some(link) = get_any_link_under_cursor_with_tolerance(line, col, 0) {
        return Some(link);
    }
    // Fall back to a tolerance of 3 characters (highly useful on Windows Terminal
    // where mouse coordinate alignments can be off due to padding or scaling)
    get_any_link_under_cursor_with_tolerance(line, col, 3)
}

fn get_any_link_under_cursor_with_tolerance(
    line: &str,
    col: usize,
    tolerance: usize,
) -> Option<LinkType> {
    let chars: Vec<char> = line.chars().collect();

    // 1. Check Markdown Link [Text](URL)
    let mut pos = 0;
    while pos < chars.len() {
        if chars[pos] == '[' {
            let start_bracket = pos;
            let mut end_bracket = None;
            let mut idx = pos + 1;
            // Find closing bracket
            while idx < chars.len() {
                if chars[idx] == ']' {
                    end_bracket = Some(idx);
                    break;
                }
                idx += 1;
            }
            if let Some(close_b) = end_bracket {
                // Check if followed immediately by '('
                if close_b + 1 < chars.len() && chars[close_b + 1] == '(' {
                    let start_paren = close_b + 1;
                    let mut end_paren = None;
                    let mut idx2 = start_paren + 1;
                    while idx2 < chars.len() {
                        if chars[idx2] == ')' {
                            end_paren = Some(idx2);
                            break;
                        }
                        idx2 += 1;
                    }
                    if let Some(close_p) = end_paren {
                        let start_with_tol = start_bracket.saturating_sub(tolerance);
                        let end_with_tol = close_p + tolerance;
                        if col >= start_with_tol && col <= end_with_tol {
                            let url: String = chars[start_paren + 1..close_p].iter().collect();
                            return Some(LinkType::Markdown(url.trim().to_string()));
                        }
                        pos = close_p + 1;
                        continue;
                    }
                }
            }
        }
        pos += 1;
    }

    // 2. Check Parentheses Link [(URL)]
    pos = 0;
    while pos < chars.len() {
        if pos + 1 < chars.len() && chars[pos] == '[' && chars[pos + 1] == '(' {
            let start_pos = pos;
            let mut end_pos = None;
            let mut idx = pos + 2;
            while idx + 1 < chars.len() {
                if chars[idx] == ')' && chars[idx + 1] == ']' {
                    end_pos = Some(idx + 1);
                    break;
                }
                idx += 1;
            }
            if let Some(absolute_end) = end_pos {
                let start_with_tol = start_pos.saturating_sub(tolerance);
                let end_with_tol = absolute_end + tolerance;
                if col >= start_with_tol && col <= end_with_tol {
                    let url: String = chars[start_pos + 2..absolute_end - 1].iter().collect();
                    return Some(LinkType::Markdown(url.trim().to_string()));
                }
                pos = absolute_end + 1;
                continue;
            }
        }
        pos += 1;
    }

    // 3. Check Wiki Link [[Wiki Link]]
    pos = 0;
    while pos < chars.len() {
        if pos + 1 < chars.len() && chars[pos] == '[' && chars[pos + 1] == '[' {
            let start_pos = pos;
            let mut end_pos = None;
            let mut idx = pos + 2;
            while idx + 1 < chars.len() {
                if chars[idx] == ']' && chars[idx + 1] == ']' {
                    end_pos = Some(idx + 1);
                    break;
                }
                idx += 1;
            }
            if let Some(absolute_end) = end_pos {
                let start_with_tol = start_pos.saturating_sub(tolerance);
                let end_with_tol = absolute_end + tolerance;
                if col >= start_with_tol && col <= end_with_tol {
                    let content: String = chars[start_pos + 2..absolute_end - 1].iter().collect();
                    return Some(LinkType::Wiki(content.trim().to_string()));
                }
                pos = absolute_end + 1;
            } else {
                break;
            }
        } else {
            pos += 1;
        }
    }

    // 4. Check Raw HTTP/HTTPS URL
    pos = 0;
    while pos < chars.len() {
        if pos + 7 < chars.len()
            && (chars[pos..pos + 7] == ['h', 't', 't', 'p', ':', '/', '/']
                || (pos + 8 < chars.len()
                    && chars[pos..pos + 8] == ['h', 't', 't', 'p', 's', ':', '/', '/']))
        {
            let start_url = pos;
            let mut end_url = pos;
            while end_url < chars.len() {
                let c = chars[end_url];
                if c.is_whitespace() || c == ']' || c == ')' || c == '>' || c == '<' {
                    break;
                }
                end_url += 1;
            }
            let start_with_tol = start_url.saturating_sub(tolerance);
            let end_with_tol = end_url + tolerance;
            if col >= start_with_tol && col < end_with_tol {
                let url: String = chars[start_url..end_url].iter().collect();
                let mut url_str = url;
                while url_str.ends_with('.')
                    || url_str.ends_with(',')
                    || url_str.ends_with(';')
                    || url_str.ends_with('?')
                    || url_str.ends_with('!')
                {
                    url_str.pop();
                }
                return Some(LinkType::RawUrl(url_str));
            }
            pos = end_url;
        } else {
            pos += 1;
        }
    }

    None
}

// Find all whole-word occurrences of the variable name in the note text
#[cfg(test)]
fn find_word_occurrences(lines_vecs: &[Vec<char>], word: &str) -> Vec<edtui::Highlight> {
    let mut highlights = Vec::new();
    if word.is_empty() {
        return highlights;
    }
    let word_chars: Vec<char> = word.chars().collect();
    let word_len = word_chars.len();

    let is_ident_char = |c: char| -> bool { c.is_alphanumeric() || c == '_' || c == '/' };

    for (row_idx, line) in lines_vecs.iter().enumerate() {
        if line.len() < word_len {
            continue;
        }
        for start_idx in 0..=(line.len() - word_len) {
            // Check substring match
            if line[start_idx..(start_idx + word_len)] == word_chars {
                // Check word boundaries
                let before_ok = if start_idx > 0 {
                    !is_ident_char(line[start_idx - 1])
                } else {
                    true
                };
                let after_ok = if start_idx + word_len < line.len() {
                    !is_ident_char(line[start_idx + word_len])
                } else {
                    true
                };

                if before_ok && after_ok {
                    highlights.push(edtui::Highlight {
                        start: edtui::Index2 {
                            row: row_idx,
                            col: start_idx,
                        },
                        end: edtui::Index2 {
                            row: row_idx,
                            col: start_idx + word_len - 1,
                        },
                        style: Style::default()
                            .bg(Color::Rgb(167, 82, 142))
                            .fg(Color::Rgb(224, 230, 242))
                            .bold(),
                    });
                }
            }
        }
    }
    highlights
}

fn main() -> Result<(), io::Error> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, event::EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run trigger update for currency exchange rates
    trigger_background_update();

    let wiki_root = if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".calki").join("wiki")
    } else {
        PathBuf::from("./wiki")
    };
    let mut app = match App::new(wiki_root) {
        Ok(a) => a,
        Err(e) => {
            // Restore terminal on startup failure
            let _ = disable_raw_mode();
            let mut stdout = io::stdout();
            let _ = execute!(stdout, LeaveAlternateScreen, event::DisableMouseCapture);
            eprintln!("Initialization failed: {}", e);
            return Ok(());
        }
    };

    let result = run_app(&mut terminal, &mut app);

    // Save final state and restore terminal
    let _ = app.save_current_note();
    let session = SessionState {
        active_path: app.active_path.to_string_lossy().to_string(),
        cursor_row: app.editor_state.cursor.row,
        cursor_col: app.editor_state.cursor.col,
        focused_panel: match app.focused_panel {
            FocusedPanel::WikiMap => "WikiMap".to_string(),
            FocusedPanel::Editor => "Editor".to_string(),
            FocusedPanel::Variables => "Variables".to_string(),
        },
        left_panel_open: app.left_panel_open,
        right_panel_open: app.right_panel_open,
        history_stack: app
            .history_stack
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    };
    let _ = session.save();
    let _ = write_cursor_shape_sequence(terminal.backend_mut(), 0);
    let _ = write_cursor_color_sequence(terminal.backend_mut(), "");
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        event::DisableMouseCapture
    )?;
    disable_raw_mode()?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {}", err);
    }
    Ok(())
}

fn write_cursor_shape_sequence<W: std::io::Write>(
    writer: &mut W,
    shape_num: u8,
) -> std::io::Result<()> {
    let raw_seq = format!("\x1b[{} q", shape_num);

    let inside_tmux = std::env::var("TMUX").is_ok();
    let term = std::env::var("TERM").unwrap_or_default();
    let inside_screen = term.contains("screen");

    if inside_tmux {
        let tmux_seq = format!("\x1bPtmux;\x1b\x1b[{} q\x1b\\", shape_num);
        writer.write_all(tmux_seq.as_bytes())?;
    } else if inside_screen {
        let screen_seq = format!("\x1bP\x1b\x1b[{} q\x1b\\", shape_num);
        writer.write_all(screen_seq.as_bytes())?;
    } else {
        writer.write_all(raw_seq.as_bytes())?;
    }
    writer.flush()?;
    Ok(())
}

fn write_cursor_color_sequence<W: std::io::Write>(
    writer: &mut W,
    color_str: &str,
) -> std::io::Result<()> {
    let raw_seq = if color_str.is_empty() {
        "\x1b]112\x07".to_string()
    } else {
        format!("\x1b]12;{}\x07", color_str)
    };

    let inside_tmux = std::env::var("TMUX").is_ok();
    let term = std::env::var("TERM").unwrap_or_default();
    let inside_screen = term.contains("screen");

    if inside_tmux {
        let wrapped_payload = raw_seq.replace("\x1b", "\x1b\x1b");
        let tmux_seq = format!("\x1bPtmux;\x1b{}\x1b\\", wrapped_payload);
        writer.write_all(tmux_seq.as_bytes())?;
    } else if inside_screen {
        let wrapped_payload = raw_seq.replace("\x1b", "\x1b\x1b");
        let screen_seq = format!("\x1bP{}\x1b\\", wrapped_payload);
        writer.write_all(screen_seq.as_bytes())?;
    } else {
        writer.write_all(raw_seq.as_bytes())?;
    }
    writer.flush()?;
    Ok(())
}

fn handle_modal_key(app: &mut App, key: crossterm::event::KeyEvent) -> bool {
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
                    8
                } else {
                    app.help_tab_idx - 1
                };
                app.help_scroll = 0;
            }
            KeyCode::Char('l') | KeyCode::Char('L') | KeyCode::Right => {
                app.help_tab_idx = (app.help_tab_idx + 1) % 9;
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
            KeyCode::Esc | KeyCode::F(1) | KeyCode::Char('q') | KeyCode::Char('Q') => {
                app.show_help = false;
            }
            _ => {}
        }
        return true;
    }
    false
}

fn run_app<B: Backend + std::io::Write>(
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
        terminal.draw(|f| ui(f, app)).map_err(|e| e.to_string())?;

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

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn estimate_line_height(line: &[char], max_width: usize, tab_width: usize) -> usize {
    use unicode_width::UnicodeWidthChar;
    if line.is_empty() {
        return 1;
    }
    if max_width == 0 {
        return 1;
    }

    let mut num_lines = 0;
    let mut current_width = 0;
    let mut last_space_idx_in_chunk: Option<usize> = None;
    let mut chunk_len = 0;

    let mut i = 0;
    while i < line.len() {
        let ch = line[i];
        let ch_w = if ch == '\t' {
            tab_width
        } else {
            ch.width().unwrap_or(0)
        };

        if current_width + ch_w > max_width {
            if let Some(space_idx) = last_space_idx_in_chunk {
                num_lines += 1;
                // Backtrack to after space
                let characters_in_next_line = chunk_len - 1 - space_idx;
                current_width = 0;
                last_space_idx_in_chunk = None;
                chunk_len = 0;
                // recalculate width of characters after space
                let backtrack_start = i - characters_in_next_line;
                for &c in &line[backtrack_start..=i] {
                    let c_w = if c == '\t' {
                        tab_width
                    } else {
                        c.width().unwrap_or(0)
                    };
                    current_width += c_w;
                    if c == ' ' {
                        last_space_idx_in_chunk = Some(chunk_len);
                    }
                    chunk_len += 1;
                }
            } else {
                // Force wrap
                num_lines += 1;
                current_width = ch_w;
                last_space_idx_in_chunk = if ch == ' ' { Some(0) } else { None };
                chunk_len = 1;
            }
        } else {
            current_width += ch_w;
            if ch == ' ' {
                last_space_idx_in_chunk = Some(chunk_len);
            }
            chunk_len += 1;
        }
        i += 1;
    }
    if current_width > 0 {
        num_lines += 1;
    }
    num_lines.max(1)
}

fn ui(f: &mut Frame, app: &mut App) {
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

    // Tokyo Night Palette mappings
    let bg_color = Color::Rgb(26, 27, 38);
    let border_focused_color = Color::Rgb(125, 207, 255); // Cyan #7dcfff
    let border_dim_color = Color::Rgb(86, 95, 137); // Muted Gray #565f89
    let text_fg_color = Color::Rgb(169, 177, 214); // Soft Gray #a9b1d6

    // RENDER 1: Left Panel (Wiki Map)
    if app.left_panel_open {
        crate::ui::panels::render_wiki_map(
            f,
            app,
            left_area,
            bg_color,
            text_fg_color,
            border_focused_color,
            border_dim_color,
        );
    }

    // RENDER 2: Middle Panel (Editor)
    crate::ui::panels::render_editor(
        f,
        app,
        editor_area,
        bg_color,
        text_fg_color,
        border_focused_color,
        border_dim_color,
    );

    // RENDER 3: Right Panel (Variables Inspector)
    if app.right_panel_open {
        crate::ui::panels::render_variables(
            f,
            app,
            right_area,
            bg_color,
            text_fg_color,
            border_focused_color,
            border_dim_color,
        );
    }

    // Unified Help popup modal with tabs (opened via F1, ?, ~)
    if app.show_help {
        crate::ui::modals::render_help(f, app);
    }
    if app.show_delete_confirm {
        crate::ui::modals::render_delete_confirm(f, app, text_fg_color);
    }
    if app.show_update_modal {
        crate::ui::modals::render_update_modal(f, app, text_fg_color);
    }
    if app.show_export_menu {
        crate::ui::modals::render_export_menu(f, text_fg_color);
    }

    crate::ui::status::render_status_line(f, app, status_area, show_bottom_bar);
}
fn find_in_chars(chars: &[char], sub: &str) -> Option<usize> {
    let sub_chars: Vec<char> = sub.chars().collect();
    if sub_chars.is_empty() {
        return Some(0);
    }
    chars
        .windows(sub_chars.len())
        .position(|window| window == sub_chars)
}

fn find_in_chars_from(chars: &[char], sub: &str, start_idx: usize) -> Option<usize> {
    if start_idx >= chars.len() {
        return None;
    }
    let sub_chars: Vec<char> = sub.chars().collect();
    if sub_chars.is_empty() {
        return Some(start_idx);
    }
    chars[start_idx..]
        .windows(sub_chars.len())
        .position(|window| window == sub_chars)
        .map(|pos| start_idx + pos)
}

#[derive(Debug, Clone, PartialEq)]
enum HighlightToken {
    Number {
        start: usize,
        end: usize,
        val: f64,
    },
    Identifier {
        start: usize,
        end: usize,
        name: String,
    },
    Symbol {
        start: usize,
        end: usize,
        ch: char,
    },
    Arrow {
        start: usize,
        end: usize,
    },
    In {
        start: usize,
        end: usize,
    },
}

fn is_registered_unit(word: &str) -> bool {
    if crate::math::units::is_custom_unit(word) || get_unit_info(word).is_some() || word == "$" {
        return true;
    }
    // Check compound unit: e.g. miles/kWh or kWh/hr or $/kWh or miles*day
    let parts: Vec<&str> = word.split(['/', '*']).collect();
    if parts.len() > 1 {
        for part in parts {
            let clean = part.trim_end_matches(|c: char| c.is_ascii_digit() || c == '^');
            if get_unit_info(clean).is_none() && clean != "$" {
                return false;
            }
        }
        return true;
    }
    false
}

fn tokenize_line_for_highlighting(line: &[char]) -> Vec<HighlightToken> {
    let mut tokens = Vec::new();
    let mut i = 0;
    let len = line.len();

    while i < len {
        let ch = line[i];
        if ch.is_whitespace() {
            i += 1;
            continue;
        }

        if ch.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut has_decimal = false;
            while i < len {
                let n_ch = line[i];
                if n_ch.is_ascii_digit() {
                    i += 1;
                } else if n_ch == '.' && !has_decimal {
                    if i + 1 < len && line[i + 1].is_ascii_digit() {
                        has_decimal = true;
                        i += 2;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            let end = i;
            let val_str: String = line[start..end].iter().collect();
            let val = val_str.parse::<f64>().unwrap_or(0.0);
            tokens.push(HighlightToken::Number {
                start,
                end: end.saturating_sub(1),
                val,
            });
        } else if ch == '=' {
            let start = i;
            i += 1;
            if i < len && line[i] == '>' {
                i += 1;
                tokens.push(HighlightToken::Arrow {
                    start,
                    end: start + 1,
                });
            } else {
                tokens.push(HighlightToken::Symbol {
                    start,
                    end: start,
                    ch: '=',
                });
            }
        } else if ch == '$' {
            let start = i;
            i += 1;
            tokens.push(HighlightToken::Identifier {
                start,
                end: start,
                name: "$".to_string(),
            });
        } else if ch.is_alphabetic() || ch == '_' {
            let start = i;
            i += 1;
            while i < len {
                let n_ch = line[i];
                if n_ch.is_alphanumeric() || n_ch == '_' || n_ch == '/' {
                    i += 1;
                } else {
                    break;
                }
            }
            let end = i;
            let name: String = line[start..end].iter().collect();
            if name == "in"
                || name == "to"
                || name == "if"
                || name == "else"
                || name == "switch"
                || name == "default"
                || name == "for"
                || name == "while"
            {
                tokens.push(HighlightToken::In {
                    start,
                    end: end.saturating_sub(1),
                });
            } else {
                tokens.push(HighlightToken::Identifier {
                    start,
                    end: end.saturating_sub(1),
                    name,
                });
            }
        } else {
            let start = i;
            i += 1;
            tokens.push(HighlightToken::Symbol {
                start,
                end: start,
                ch,
            });
        }
    }
    tokens
}

fn markdown_to_html(md: &str, title: &str) -> String {
    let mut html = String::new();
    let mut in_list = false;

    for line in md.lines() {
        let trimmed = line.trim();

        if in_list && !trimmed.starts_with('*') && !trimmed.starts_with('-') {
            html.push_str("</ul>\n");
            in_list = false;
        }

        if trimmed.is_empty() {
            html.push_str("<p></p>\n");
            continue;
        }

        if let Some(stripped) = trimmed.strip_prefix("# ") {
            html.push_str(&format!("<h1>{}</h1>\n", parse_inline_elements(stripped)));
        } else if let Some(stripped) = trimmed.strip_prefix("## ") {
            html.push_str(&format!("<h2>{}</h2>\n", parse_inline_elements(stripped)));
        } else if let Some(stripped) = trimmed.strip_prefix("### ") {
            html.push_str(&format!("<h3>{}</h3>\n", parse_inline_elements(stripped)));
        } else if let Some(stripped) = trimmed.strip_prefix("#### ") {
            html.push_str(&format!("<h4>{}</h4>\n", parse_inline_elements(stripped)));
        } else if let Some(stripped) = trimmed.strip_prefix('>') {
            html.push_str(&format!(
                "<blockquote>{}</blockquote>\n",
                parse_inline_elements(stripped.trim())
            ));
        } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            html.push_str("<hr/>\n");
        } else if trimmed.starts_with('*') || trimmed.starts_with('-') {
            if !in_list {
                html.push_str("<ul>\n");
                in_list = true;
            }
            let stripped = trimmed
                .strip_prefix('*')
                .or_else(|| trimmed.strip_prefix('-'))
                .unwrap_or(trimmed);
            html.push_str(&format!(
                "<li>{}</li>\n",
                parse_inline_elements(stripped.trim())
            ));
        } else if trimmed.contains("=>") && !trimmed.contains('`') {
            if let Some(pos) = trimmed.find("=>") {
                let expr = trimmed[..pos].trim();
                let val = trimmed[pos + 2..].trim();
                let val_class = if val.contains("[Error") {
                    "val error"
                } else {
                    "val"
                };
                html.push_str(&format!(
                    "<div class=\"math-block\"><span class=\"expr\">{}</span> <span class=\"arrow\">=&gt;</span> <span class=\"{}\">{}</span></div>\n",
                    parse_inline_elements(expr),
                    val_class,
                    parse_inline_elements(val)
                ));
            } else {
                html.push_str(&format!("<p>{}</p>\n", parse_inline_elements(trimmed)));
            }
        } else {
            html.push_str(&format!("<p>{}</p>\n", parse_inline_elements(trimmed)));
        }
    }

    if in_list {
        html.push_str("</ul>\n");
    }

    let template = get_html_template();
    template
        .replace("{title}", title)
        .replace("{content}", &html)
}

fn parse_inline_elements(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '`' {
            let mut inner = String::new();
            let mut closed = false;
            for next_ch in chars.by_ref() {
                if next_ch == '`' {
                    closed = true;
                    break;
                }
                inner.push(next_ch);
            }

            if closed {
                if let Some(arrow_pos) = inner.find("=>") {
                    let expr = &inner[..arrow_pos].trim();
                    let val = &inner[arrow_pos + 2..].trim();
                    let val_class = if val.contains("[Error") {
                        "val error"
                    } else {
                        "val"
                    };
                    result.push_str(&format!(
                        "<code class=\"math-eval\"><span class=\"expr\">{}</span> =&gt; <span class=\"{}\">{}</span></code>",
                        html_escape(expr),
                        val_class,
                        html_escape(val)
                    ));
                } else {
                    result.push_str(&format!("<code>{}</code>", html_escape(&inner)));
                }
            } else {
                result.push('`');
                result.push_str(&inner);
            }
        } else if ch == '[' && chars.peek() == Some(&'[') {
            chars.next();
            let mut link_name = String::new();
            let mut closed = false;
            while let Some(next_ch) = chars.next() {
                if next_ch == ']' && chars.peek() == Some(&']') {
                    chars.next();
                    closed = true;
                    break;
                }
                link_name.push(next_ch);
            }
            if closed {
                let link_name_trimmed = link_name.trim();
                let clean_name = link_name_trimmed
                    .to_lowercase()
                    .replace(|c: char| !c.is_alphanumeric() && c != ' ', "")
                    .replace(' ', "-");
                let href = format!("{}.html", clean_name);
                result.push_str(&format!(
                    "<a href=\"{}\" class=\"wiki-link\">{}</a>",
                    href,
                    html_escape(link_name_trimmed)
                ));
            } else {
                result.push_str("[[");
                result.push_str(&link_name);
            }
        } else {
            match ch {
                '<' => result.push_str("&lt;"),
                '>' => result.push_str("&gt;"),
                '&' => result.push_str("&amp;"),
                '"' => result.push_str("&quot;"),
                _ => result.push(ch),
            }
        }
    }
    result
}

fn html_escape(text: &str) -> String {
    let mut escaped = String::new();
    for ch in text.chars() {
        match ch {
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '&' => escaped.push_str("&amp;"),
            '"' => escaped.push_str("&quot;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn get_html_template() -> &'static str {
    r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{title}</title>
    <link rel="preconnect" href="https://fonts.googleapis.com">
    <link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
    <link href="https://fonts.googleapis.com/css2?family=Outfit:wght@300;400;600;700&family=Fira+Code:wght@400;500&display=swap" rel="stylesheet">
    <style>
        :root {
            --bg-color: #1a1b26;
            --panel-bg: #24283b;
            --text-color: #a9b1d6;
            --text-muted: #565f89;
            --accent-purple: #bb9af7;
            --accent-blue: #7aa2f7;
            --accent-cyan: #7dcfff;
            --accent-green: #9ece6a;
            --accent-orange: #ff9e64;
            --accent-red: #f7768e;
            --border-color: #3b426b;
        }
        body {
            background-color: var(--bg-color);
            color: var(--text-color);
            font-family: 'Outfit', sans-serif;
            line-height: 1.6;
            margin: 0;
            padding: 40px 20px;
        }
        .container {
            max-width: 800px;
            margin: 0 auto;
            background: var(--panel-bg);
            padding: 40px;
            border-radius: 16px;
            box-shadow: 0 8px 30px rgba(0,0,0,0.3);
            border: 1px solid var(--border-color);
        }
        h1, h2, h3, h4, h5, h6 {
            color: #ffffff;
            margin-top: 1.5em;
            margin-bottom: 0.5em;
            font-weight: 700;
        }
        h1 {
            font-size: 2.5rem;
            border-bottom: 2px solid var(--border-color);
            padding-bottom: 0.3em;
            margin-top: 0;
            background: linear-gradient(45deg, var(--accent-purple), var(--accent-cyan));
            -webkit-background-clip: text;
            -webkit-text-fill-color: transparent;
        }
        h2 {
            font-size: 1.8rem;
            color: var(--accent-blue);
        }
        h3 {
            font-size: 1.4rem;
            color: var(--accent-purple);
        }
        p {
            margin: 0 0 1em;
        }
        a, .wiki-link {
            color: var(--accent-cyan);
            text-decoration: none;
            border-bottom: 1px dashed var(--accent-cyan);
            transition: all 0.2s ease;
        }
        a:hover, .wiki-link:hover {
            color: var(--accent-blue);
            border-bottom-style: solid;
        }
        ul, ol {
            margin: 0 0 1.5em;
            padding-left: 20px;
        }
        li {
            margin-bottom: 0.5em;
        }
        code {
            font-family: 'Fira Code', monospace;
            background-color: var(--bg-color);
            padding: 2px 6px;
            border-radius: 4px;
            font-size: 0.9em;
            color: var(--accent-orange);
            border: 1px solid var(--border-color);
        }
        .math-block {
            font-family: 'Fira Code', monospace;
            background-color: var(--bg-color);
            padding: 12px 18px;
            border-radius: 8px;
            margin: 1em 0;
            border-left: 4px solid var(--accent-cyan);
            display: flex;
            justify-content: space-between;
            align-items: center;
        }
        .math-block .expr {
            color: var(--accent-cyan);
        }
        .math-block .val {
            color: var(--accent-green);
            font-weight: 600;
        }
        .math-eval {
            font-family: 'Fira Code', monospace;
            background-color: var(--bg-color);
            padding: 2px 6px;
            border-radius: 4px;
            border: 1px solid var(--border-color);
        }
        .math-eval .expr {
            color: var(--accent-cyan);
        }
        .math-eval .val {
            color: var(--accent-green);
            font-weight: bold;
        }
        .error {
            color: var(--accent-red) !important;
            font-weight: bold;
        }
        hr {
            border: none;
            border-top: 1px solid var(--border-color);
            margin: 2em 0;
        }
        blockquote {
            border-left: 4px solid var(--accent-green);
            margin: 1em 0;
            padding-left: 15px;
            color: var(--accent-green);
            font-style: italic;
        }
    </style>
</head>
<body>
    <div class="container">
        {content}
    </div>
</body>
</html>"#
}

#[cfg(test)]
mod main_tests {
    use super::*;

    #[test]
    fn test_wrap_selection_in_link() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_wrap_selection");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();
        let mut app = App::new(wiki_root.clone()).unwrap();
        app.editor_state = EditorState::new(edtui::Lines::from("Welcome 🧮 price = 100"));
        app.editor_state.cursor = edtui::Index2::new(0, 10);

        // Simulate visual mode selection left-to-right
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('v'),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut app.editor_state,
        );
        for _ in 0..4 {
            app.editor_event_handler.on_key_event(
                crossterm::event::KeyEvent::new(
                    crossterm::event::KeyCode::Char('l'),
                    crossterm::event::KeyModifiers::NONE,
                ),
                &mut app.editor_state,
            );
        }

        assert!(app.editor_state.selection.is_some());
        app.wrap_selection_in_link();
        let text = app.get_editor_text();
        assert_eq!(text, "Welcome 🧮 [[price]] = 100");
        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_find_word_occurrences() {
        let lines = vec![
            "price = 100".chars().collect::<Vec<char>>(),
            "tax = price * 0.10".chars().collect::<Vec<char>>(),
            "price_rate = 1.05".chars().collect::<Vec<char>>(),
            "total = price + tax".chars().collect::<Vec<char>>(),
        ];

        let highlights = find_word_occurrences(&lines, "price");
        assert_eq!(highlights.len(), 3);

        assert_eq!(highlights[0].start.row, 0);
        assert_eq!(highlights[0].start.col, 0);
        assert_eq!(highlights[0].end.row, 0);
        assert_eq!(highlights[0].end.col, 4);

        assert_eq!(highlights[1].start.row, 1);
        assert_eq!(highlights[1].start.col, 6);
        assert_eq!(highlights[1].end.row, 1);
        assert_eq!(highlights[1].end.col, 10);

        assert_eq!(highlights[2].start.row, 3);
        assert_eq!(highlights[2].start.col, 8);
        assert_eq!(highlights[2].end.row, 3);
        assert_eq!(highlights[2].end.col, 12);
    }

    #[test]
    fn test_custom_change_bindings() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_change_keys");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        app.editor_state = EditorState::new(edtui::Lines::from("hello world"));

        // 1. Test cw (Change Word) at start of "hello"
        app.editor_state.cursor = edtui::Index2::new(0, 0);
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('c'),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut app.editor_state,
        );
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('w'),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut app.editor_state,
        );
        assert_eq!(app.editor_state.mode, EditorMode::Insert);
        let text = app.get_editor_text();
        assert_eq!(text, "world");

        // 2. Test cc (Change Line)
        app.editor_state = EditorState::new(edtui::Lines::from("hello world"));
        app.editor_state.mode = EditorMode::Normal;
        app.editor_state.cursor = edtui::Index2::new(0, 4);
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('c'),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut app.editor_state,
        );
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('c'),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut app.editor_state,
        );
        assert_eq!(app.editor_state.mode, EditorMode::Insert);
        let text = app.get_editor_text();
        assert_eq!(text, "");

        // 3. Test C (Change to End of Line)
        app.editor_state = EditorState::new(edtui::Lines::from("hello world"));
        app.editor_state.mode = EditorMode::Normal;
        app.editor_state.cursor = edtui::Index2::new(0, 5); // index of space
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(
                crossterm::event::KeyCode::Char('C'),
                crossterm::event::KeyModifiers::SHIFT,
            ),
            &mut app.editor_state,
        );
        assert_eq!(app.editor_state.mode, EditorMode::Insert);
        let text = app.get_editor_text();
        assert_eq!(text, "hello");

        // 4. Test SystemClipboard
        let mut clipboard = SystemClipboard::new();
        clipboard.set_text("test_clip_val".to_string());
        assert_eq!(clipboard.get_text(), "test_clip_val");

        // 5. Test SessionState serialization/deserialization
        let state = SessionState {
            active_path: "some_path.md".to_string(),
            cursor_row: 10,
            cursor_col: 20,
            focused_panel: "Variables".to_string(),
            left_panel_open: false,
            right_panel_open: true,
            history_stack: vec![],
        };
        let serialized = serde_json::to_string(&state).unwrap();
        let deserialized: SessionState = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.active_path, "some_path.md");
        assert_eq!(deserialized.cursor_row, 10);
        assert_eq!(deserialized.cursor_col, 20);
        assert_eq!(deserialized.focused_panel, "Variables");
        assert!(!deserialized.left_panel_open);
        assert!(deserialized.right_panel_open);

        // Clean up
        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_f1_crash() {
        let wiki_root = std::env::current_dir().unwrap().join("test_wiki_temp_f1");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();
        let mut app = App::new(wiki_root.clone()).unwrap();

        let codes_to_test = vec![
            KeyCode::Char('a'),
            KeyCode::Esc,
            KeyCode::Backspace,
            KeyCode::Enter,
            KeyCode::Tab,
            KeyCode::Delete,
            KeyCode::Left,
            KeyCode::Right,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Home,
            KeyCode::End,
            KeyCode::PageUp,
            KeyCode::PageDown,
        ];

        for code in codes_to_test {
            app.editor_state = EditorState::new(edtui::Lines::from("hello world"));
            let key = crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE);
            // We want to verify that these do NOT panic
            app.editor_event_handler
                .on_key_event(key, &mut app.editor_state);
        }

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_mouse_routing() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_mouse");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();
        let mut app = App::new(wiki_root.clone()).unwrap();

        // Define areas
        app.left_area = ratatui::layout::Rect::new(0, 0, 20, 30);
        app.editor_area = ratatui::layout::Rect::new(20, 0, 50, 30);
        app.right_area = ratatui::layout::Rect::new(70, 0, 20, 30);

        // Clicking editor panel sets focus to Editor
        let mouse_event = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 30,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        let col = mouse_event.column;
        let row = mouse_event.row;
        if col >= app.editor_area.x
            && col < app.editor_area.x + app.editor_area.width
            && row >= app.editor_area.y
            && row < app.editor_area.y + app.editor_area.height
        {
            app.focused_panel = FocusedPanel::Editor;
        }
        assert_eq!(app.focused_panel, FocusedPanel::Editor);

        // Clicking right panel sets focus to Variables
        let mouse_event_right = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 80,
            row: 5,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        let col = mouse_event_right.column;
        let row = mouse_event_right.row;
        if col >= app.right_area.x
            && col < app.right_area.x + app.right_area.width
            && row >= app.right_area.y
            && row < app.right_area.y + app.right_area.height
        {
            app.focused_panel = FocusedPanel::Variables;
        }
        assert_eq!(app.focused_panel, FocusedPanel::Variables);

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_mouse_click_follow_link() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_click_link");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        // Create the target note file
        let target_path = wiki_root.join("target-note.md");
        std::fs::write(&target_path, "# Target Note Content").unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();

        // Define areas
        app.left_area = ratatui::layout::Rect::new(0, 0, 20, 30);
        app.editor_area = ratatui::layout::Rect::new(20, 0, 50, 30);
        app.right_area = ratatui::layout::Rect::new(70, 0, 20, 30);

        // Put editor in Normal Mode and write text with a wiki link
        app.editor_state = EditorState::new(edtui::Lines::from("Go to [[Target Note]] now."));
        app.editor_state.mode = EditorMode::Normal;
        app.editor_state.view.screen_area = ratatui::layout::Rect::new(20, 0, 50, 30);
        app.editor_state.view.wrap = false;

        // Verify that click at (30, 0) routes to Editor, updates cursor, and triggers following the link.
        let mouse_event = crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: 30, // x = 20 (editor x) + 10 = 30 (inside [[Target Note]])
            row: 0,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };

        let col = mouse_event.column;
        let row = mouse_event.row;
        let is_click = mouse_event.kind
            == crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left);

        if col >= app.editor_area.x
            && col < app.editor_area.x + app.editor_area.width
            && row >= app.editor_area.y
            && row < app.editor_area.y + app.editor_area.height
        {
            if app.config.mouse_focus_on_hover || is_click {
                app.focused_panel = FocusedPanel::Editor;
            }
            if app.focused_panel == FocusedPanel::Editor {
                app.editor_event_handler
                    .on_mouse_event(mouse_event, &mut app.editor_state);
                if is_click && app.editor_state.mode == EditorMode::Normal {
                    app.follow_link_under_cursor();
                }
            }
        }

        // Verify that the active path has switched to the target path
        assert_eq!(app.active_path, target_path);
        assert_eq!(app.get_editor_text(), "# Target Note Content");

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_follow_link_types() {
        let line = "Go to [Google](https://google.com) or visit https://rust-lang.org now.";

        // Col 10 is inside [Google]
        assert!(
            matches!(get_any_link_under_cursor(line, 10), Some(LinkType::Markdown(url)) if url == "https://google.com")
        );

        // Col 25 is inside https://google.com URL part
        assert!(
            matches!(get_any_link_under_cursor(line, 25), Some(LinkType::Markdown(url)) if url == "https://google.com")
        );

        // Col 45 is inside https://rust-lang.org
        assert!(
            matches!(get_any_link_under_cursor(line, 45), Some(LinkType::RawUrl(url)) if url == "https://rust-lang.org")
        );

        // Col 0 is outside any link (6 characters away from [Google] which starts at Col 6)
        assert_eq!(get_any_link_under_cursor(line, 0), None);

        // Tolerance check: starts at Col 6. Col 3 is 3 characters away, should match due to tolerance.
        assert!(
            matches!(get_any_link_under_cursor(line, 3), Some(LinkType::Markdown(url)) if url == "https://google.com")
        );

        // Col 2 is 4 characters away, should NOT match.
        assert_eq!(get_any_link_under_cursor(line, 2), None);

        // Wiki Link tolerance checking
        let wiki_line = "See [[My Page]] for info.";
        // [[My Page]] is at index 4 to 14.
        assert!(
            matches!(get_any_link_under_cursor(wiki_line, 8), Some(LinkType::Wiki(name)) if name == "My Page")
        );
        // Col 1 is 3 characters away from index 4. Should match.
        assert!(
            matches!(get_any_link_under_cursor(wiki_line, 1), Some(LinkType::Wiki(name)) if name == "My Page")
        );
        // Col 0 is 4 characters away from index 4. Should NOT match.
        assert_eq!(get_any_link_under_cursor(wiki_line, 0), None);
    }

    #[test]
    fn test_expand_variables_on_select() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_expand");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        app.left_panel_open = true;
        app.right_panel_open = true;

        // Default configuration
        assert!(!app.config.expand_variables_on_select);

        // When option is false, right width is always 25
        app.focused_panel = FocusedPanel::Variables;
        let right_width_unexpanded = if app.right_panel_open {
            if app.config.expand_variables_on_select && app.focused_panel == FocusedPanel::Variables
            {
                45
            } else {
                25
            }
        } else {
            0
        };
        assert_eq!(right_width_unexpanded, 25);

        // Enable option
        app.config.expand_variables_on_select = true;

        // When focused on Variables panel, right width should expand to 45
        let right_width_expanded = if app.right_panel_open {
            if app.config.expand_variables_on_select && app.focused_panel == FocusedPanel::Variables
            {
                45
            } else {
                25
            }
        } else {
            0
        };
        assert_eq!(right_width_expanded, 45);

        // When focused elsewhere, right width should remain 25
        app.focused_panel = FocusedPanel::Editor;
        let right_width_editor_focused = if app.right_panel_open {
            if app.config.expand_variables_on_select && app.focused_panel == FocusedPanel::Variables
            {
                45
            } else {
                25
            }
        } else {
            0
        };
        assert_eq!(right_width_editor_focused, 25);

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_n_motions_multiplier() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_multiplier");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        app.editor_state = EditorState::new(edtui::Lines::from(
            "line 1\nline 2\nline 3\nline 4\nline 5\nline 6",
        ));
        app.editor_state.cursor = edtui::Index2::new(0, 0);
        app.editor_state.mode = EditorMode::Normal;
        app.focused_panel = FocusedPanel::Editor;

        // Check helper directly
        let j_key = crossterm::event::KeyEvent::new(
            KeyCode::Char('j'),
            crossterm::event::KeyModifiers::NONE,
        );
        let k_key = crossterm::event::KeyEvent::new(
            KeyCode::Char('k'),
            crossterm::event::KeyModifiers::NONE,
        );
        let a_key = crossterm::event::KeyEvent::new(
            KeyCode::Char('a'),
            crossterm::event::KeyModifiers::NONE,
        );
        assert!(is_repeatable_motion(j_key));
        assert!(is_repeatable_motion(k_key));
        assert!(!is_repeatable_motion(a_key));

        // Test digits accumulation in normal mode
        let key_5 = crossterm::event::KeyEvent::new(
            KeyCode::Char('5'),
            crossterm::event::KeyModifiers::NONE,
        );
        let key_2 = crossterm::event::KeyEvent::new(
            KeyCode::Char('2'),
            crossterm::event::KeyModifiers::NONE,
        );

        // Simulate event loop logic:
        let prev_mode = app.editor_state.mode;
        // Key press '5'
        if (prev_mode == EditorMode::Normal || prev_mode == EditorMode::Visual)
            && !app.replace_next_char
            && let KeyCode::Char(c) = key_5.code
            && c.is_ascii_digit()
            && (c != '0' || app.vim_multiplier.is_some())
            && key_5.modifiers.is_empty()
        {
            let digit = c.to_digit(10).unwrap() as usize;
            let current = app.vim_multiplier.unwrap_or(0);
            app.vim_multiplier = Some(current * 10 + digit);
        }
        assert_eq!(app.vim_multiplier, Some(5));

        // Key press '2'
        if (prev_mode == EditorMode::Normal || prev_mode == EditorMode::Visual)
            && !app.replace_next_char
            && let KeyCode::Char(c) = key_2.code
            && c.is_ascii_digit()
            && (c != '0' || app.vim_multiplier.is_some())
            && key_2.modifiers.is_empty()
        {
            let digit = c.to_digit(10).unwrap() as usize;
            let current = app.vim_multiplier.unwrap_or(0);
            app.vim_multiplier = Some(current * 10 + digit);
        }
        assert_eq!(app.vim_multiplier, Some(52));

        // Simulate repeatable motion execution for 'j' (move down 52 times, but saturates at lines count - 1 = 5)
        let mut count = 1;
        if let Some(c) = app.vim_multiplier {
            if (prev_mode == EditorMode::Normal || prev_mode == EditorMode::Visual)
                && is_repeatable_motion(j_key)
            {
                count = c;
            }
            app.vim_multiplier = None;
        }
        assert_eq!(count, 52);
        assert_eq!(app.vim_multiplier, None);

        for _ in 0..count {
            app.editor_event_handler
                .on_key_event(j_key, &mut app.editor_state);
        }
        assert_eq!(app.editor_state.cursor.row, 5);

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_note_cursor_preservation() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_preservation");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        let path1 = wiki_root.join("note1.md");
        let path2 = wiki_root.join("note2.md");

        // Load note 1, move cursor and save
        let _ = app.load_note(path1.clone());
        app.editor_state.cursor = edtui::Index2::new(2, 4); // row 2, col 4 (valid length text)
        let _ = app.save_current_note();

        // Load note 2, cursor starts at 0, 0
        let _ = app.load_note(path2.clone());
        assert_eq!(app.editor_state.cursor.row, 0);
        assert_eq!(app.editor_state.cursor.col, 0);

        // Switch back to note 1, cursor should be restored to 2, 4!
        let _ = app.load_note(path1.clone());
        assert_eq!(app.editor_state.cursor.row, 2);
        assert_eq!(app.editor_state.cursor.col, 4);

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_update_available_flow() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_update_flow");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        app.update_available = None;
        app.show_update_modal = false;

        // Setup a mock update channel
        let (tx, rx) = std::sync::mpsc::channel();
        app.update_receiver = Some(rx);

        // Send a mock hash
        let new_hash = "mock_hash_12345".to_string();
        tx.send(new_hash.clone()).unwrap();

        // Simulate update check in run_app loop
        if let Some(ref rx) = app.update_receiver
            && let Ok(new_hash) = rx.try_recv()
        {
            app.update_available = Some(new_hash.clone());
            if app.config.ignored_update_hash.as_ref() != Some(&new_hash) {
                app.show_update_modal = true;
            }
            app.update_receiver = None;
        }

        assert_eq!(app.update_available, Some(new_hash.clone()));
        assert!(app.show_update_modal);
        assert!(app.update_receiver.is_none());

        // Now ignore this hash and verify it is bypassed next time
        app.config.ignored_update_hash = Some(new_hash.clone());
        app.show_update_modal = false;

        // Re-setup channel and send the same hash
        let (tx2, rx2) = std::sync::mpsc::channel();
        app.update_receiver = Some(rx2);
        tx2.send(new_hash.clone()).unwrap();

        if let Some(ref rx) = app.update_receiver
            && let Ok(new_hash) = rx.try_recv()
        {
            app.update_available = Some(new_hash.clone());
            if app.config.ignored_update_hash.as_ref() != Some(&new_hash) {
                app.show_update_modal = true;
            }
            app.update_receiver = None;
        }

        assert!(!app.show_update_modal); // ignored, so should stay false!

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_compute_syntax_highlights_units() {
        let lines = vec![
            "commute = 88 miles".chars().collect::<Vec<char>>(),
            "level2 = 6 kWh / hr".chars().collect::<Vec<char>>(),
            "price = $0.20 / kWh".chars().collect::<Vec<char>>(),
            "subaru_eff = 274 miles / 74.7 kWh => 3.668 miles/kWh"
                .chars()
                .collect::<Vec<char>>(),
            "subaru_power = commute / subaru_eff => 23.9912 kWh"
                .chars()
                .collect::<Vec<char>>(),
            "subaru_power / level2 => 3.9985 hr"
                .chars()
                .collect::<Vec<char>>(),
            "apples = 10 count".chars().collect::<Vec<char>>(),
            "// comment with 88 miles".chars().collect::<Vec<char>>(),
            "# Header with 88 miles".chars().collect::<Vec<char>>(),
            "[[miles]] = 10".chars().collect::<Vec<char>>(),
            "Let's go for 10 miles.".chars().collect::<Vec<char>>(),
            "We run at `10m/s => 10 m/s`."
                .chars()
                .collect::<Vec<char>>(),
            "monthly_cost = annual_cost / 12 month/year => $244.3901/month"
                .chars()
                .collect::<Vec<char>>(),
            "599584916 m/s in c => 2 c".chars().collect::<Vec<char>>(),
        ];

        let highlights = crate::highlight::compute_syntax_highlights(&lines, None);

        let pink = Color::Rgb(244, 143, 177);
        let unit_highlights: Vec<&edtui::Highlight> = highlights
            .iter()
            .filter(|h| h.style.fg == Some(pink))
            .collect();

        // Let's assert that in row 0, we have the expected disjoint highlights:
        // - "commute" at [0, 7] is Cyan
        // - "=" at [8, 8] is Orange
        // - " 88 " at [9, 12] is Teal
        // - "miles" at [13, 17] is Pink
        let cyan = Color::Rgb(125, 207, 255);
        let orange = Color::Rgb(255, 158, 100);
        let teal = Color::Rgb(115, 218, 202);

        assert!(highlights.iter().any(|h| h.start.row == 0
            && h.start.col == 0
            && h.end.col == 7
            && h.style.fg == Some(cyan)));
        assert!(highlights.iter().any(|h| h.start.row == 0
            && h.start.col == 8
            && h.end.col == 8
            && h.style.fg == Some(orange)));
        assert!(highlights.iter().any(|h| h.start.row == 0
            && h.start.col == 9
            && h.end.col == 12
            && h.style.fg == Some(teal)));
        assert!(highlights.iter().any(|h| h.start.row == 0
            && h.start.col == 13
            && h.end.col == 17
            && h.style.fg == Some(pink)));

        // line 1: "level2 = 6 kWh / hr" -> "kWh" is at [11, 13], "hr" is at [17, 18]
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 1 && h.start.col == 11 && h.end.col == 13)
        );
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 1 && h.start.col == 17 && h.end.col == 18)
        );

        // line 2: "price = $0.20 / kWh" -> "$" is at [8, 8], "kWh" is at [16, 18]
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 2 && h.start.col == 8 && h.end.col == 8)
        );
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 2 && h.start.col == 16 && h.end.col == 18)
        );

        // line 3: "subaru_eff = 274 miles / 74.7 kWh => 3.668 miles/kWh"
        // "miles" at [17, 21], "kWh" at [30, 32], "miles/kWh" at [43, 51]
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 3 && h.start.col == 17 && h.end.col == 21)
        );
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 3 && h.start.col == 30 && h.end.col == 32)
        );
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 3 && h.start.col == 43 && h.end.col == 51)
        );

        // line 4: "subaru_power = commute / subaru_eff => 23.9912 kWh" -> "kWh" at [47, 49]
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 4 && h.start.col == 47 && h.end.col == 49)
        );

        // line 5: "subaru_power / level2 => 3.9985 hr" -> "hr" at [32, 33]
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 5 && h.start.col == 32 && h.end.col == 33)
        );

        // line 6: "apples = 10 count" -> "count" follows a number, so it's a unit, at [12, 16]
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 6 && h.start.col == 12 && h.end.col == 16)
        );

        // line 7: comment should have no yellow highlights
        assert!(!unit_highlights.iter().any(|h| h.start.row == 7));

        // line 8: header should have no yellow highlights
        assert!(!unit_highlights.iter().any(|h| h.start.row == 8));

        // line 9: "[[miles]] = 10" -> "miles" is inside wiki link, should NOT have yellow unit highlight
        assert!(
            !unit_highlights
                .iter()
                .any(|h| h.start.row == 9 && h.start.col == 2 && h.end.col == 6)
        );

        // line 10: "Let's go for 10 miles." -> plain text block, "s" and "miles" should NOT be highlighted as units
        assert!(!unit_highlights.iter().any(|h| h.start.row == 10));

        // line 11: "We run at `10m/s => 10 m/s`." -> "m/s" inside backticks SHOULD be highlighted as unit
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 11 && h.start.col == 13 && h.end.col == 15)
        );
        assert!(
            unit_highlights
                .iter()
                .any(|h| h.start.row == 11 && h.start.col == 23 && h.end.col == 25)
        );

        // line 12: "monthly_cost = annual_cost / 12 month/year => $244.3901/month"
        let pink = Color::Rgb(244, 143, 177);

        // 1. LHS: "monthly_cost" starts at col 0, is Cyan, not italic
        let row12_lhs = highlights
            .iter()
            .find(|h| h.start.row == 12 && h.start.col == 0)
            .unwrap();
        assert_eq!(row12_lhs.style.fg, Some(cyan));
        assert!(
            !row12_lhs
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)
        );

        // 2. RHS expression: " annual_cost / 1" starts at col 14, is Teal, not italic
        let row12_rhs = highlights
            .iter()
            .find(|h| h.start.row == 12 && h.start.col == 14)
            .unwrap();
        assert_eq!(row12_rhs.style.fg, Some(teal));
        assert!(
            !row12_rhs
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)
        );

        // 3. Result: "$" starts at col 46, is Pink, and IS italic
        let row12_res_dollar = highlights
            .iter()
            .find(|h| h.start.row == 12 && h.start.col == 46)
            .unwrap();
        assert_eq!(row12_res_dollar.style.fg, Some(pink));
        assert!(
            row12_res_dollar
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)
        );

        // 4. Result: "244.3901/" starts at col 47, is Teal, and IS italic
        let row12_res_num = highlights
            .iter()
            .find(|h| h.start.row == 12 && h.start.col == 47)
            .unwrap();
        assert_eq!(row12_res_num.style.fg, Some(teal));
        assert!(
            row12_res_num
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)
        );

        // 5. Result: "month" starts at col 56, is Pink, and IS italic
        let row12_res_month = highlights
            .iter()
            .find(|h| h.start.row == 12 && h.start.col == 56)
            .unwrap();
        assert_eq!(row12_res_month.style.fg, Some(pink));
        assert!(
            row12_res_month
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)
        );

        // 6. RHS division operator '/' at col 27 is Orange, not italic
        let row12_div = highlights
            .iter()
            .find(|h| h.start.row == 12 && h.start.col == 27)
            .unwrap();
        assert_eq!(row12_div.style.fg, Some(orange));
        assert!(
            !row12_div
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)
        );

        // 7. line 13: "599584916 m/s in c => 2 c" -> "in" starts at col 14, is Bold Orange, not italic
        let row13_in = highlights
            .iter()
            .find(|h| h.start.row == 13 && h.start.col == 14)
            .unwrap();
        assert_eq!(row13_in.style.fg, Some(orange));
        assert!(
            row13_in
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)
        );
        assert!(
            !row13_in
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::ITALIC)
        );
    }

    #[test]
    fn test_vim_r_replacement() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_vim_r");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        app.editor_state = EditorState::new(edtui::Lines::from("hello world"));
        app.editor_state.mode = EditorMode::Normal;

        // Place cursor at 'w' (index 6)
        app.editor_state.cursor = edtui::Index2::new(0, 6);

        app.replace_next_char = true;

        let row = app.editor_state.cursor.row;
        let col = app.editor_state.cursor.col;
        if let Some(line) = app.editor_state.lines.get_mut(RowIndex::new(row)) {
            line[col] = 'x';
        }
        app.replace_next_char = false;

        let text = app.get_editor_text();
        assert_eq!(text, "hello xorld");

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_todo_toggling() {
        let wiki_root = std::env::current_dir().unwrap().join("test_wiki_temp_todo");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        app.editor_state = EditorState::new(edtui::Lines::from(
            "- [ ] todo item\n* list item\n- [x] done item",
        ));
        app.editor_state.mode = EditorMode::Normal;

        // 1. Toggle unchecked to checked
        app.editor_state.cursor = edtui::Index2::new(0, 0);
        let res1 = app.toggle_todo_at_cursor();
        assert!(res1);
        assert_eq!(
            app.get_editor_text(),
            "- [x] todo item\n* list item\n- [x] done item"
        );

        // 2. Convert plain list item to todo checkbox
        app.editor_state.cursor = edtui::Index2::new(1, 0);
        let res2 = app.toggle_todo_at_cursor();
        assert!(res2);
        assert_eq!(
            app.get_editor_text(),
            "- [x] todo item\n* [ ] list item\n- [x] done item"
        );

        // 3. Toggle checked to unchecked
        app.editor_state.cursor = edtui::Index2::new(2, 0);
        let res3 = app.toggle_todo_at_cursor();
        assert!(res3);
        assert_eq!(
            app.get_editor_text(),
            "- [x] todo item\n* [ ] list item\n- [ ] done item"
        );

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_navigation_history_fallback() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_history");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        let home_path = wiki_root.join("home.md");
        let other_path = wiki_root.join("other.md");
        std::fs::write(&other_path, "# Other Note\n").unwrap();

        // Load other note first
        app.load_note(other_path.clone()).unwrap();
        assert_eq!(app.active_path, other_path);
        assert!(app.history_stack.is_empty());

        // Call go_back() with empty history stack: should fall back to home.md
        let res = app.go_back();
        assert!(res);
        assert_eq!(app.active_path, home_path);

        // Call go_back() when already on home_path: should return false
        let res2 = app.go_back();
        assert!(!res2);

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_is_newer_version() {
        assert!(is_newer_version("0.3.0", "0.3.1"));
        assert!(is_newer_version("0.3.0", "0.4.0"));
        assert!(is_newer_version("0.3.0", "1.0.0"));
        assert!(!is_newer_version("0.3.0", "0.3.0"));
        assert!(!is_newer_version("0.3.0", "0.2.9"));
        assert!(!is_newer_version("0.3.0", "0.2.0"));
    }

    #[test]
    fn test_normal_mode_edit_recalculation() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_normal_recalc");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        app.editor_state = EditorState::new(edtui::Lines::from("a = 10\na + 5 => 15"));
        app.editor_state.mode = EditorMode::Normal;
        app.re_evaluate_calculations();
        assert_eq!(app.variables_cache.len(), 1);
        assert_eq!(app.variables_cache[0], ("a".to_string(), "10".to_string()));

        // Simulate deleting a character in Normal Mode using key handler ('x' key)
        app.editor_state.cursor = edtui::Index2::new(0, 0); // over 'a'

        let lines_before = Some(app.editor_state.lines.clone());

        // Process key 'x'
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(
                KeyCode::Char('x'),
                crossterm::event::KeyModifiers::NONE,
            ),
            &mut app.editor_state,
        );

        // In the run_app logic, it detects that the lines changed in Normal Mode:
        if let Some(ref before) = lines_before
            && before != &app.editor_state.lines
        {
            app.re_evaluate_calculations();
        }

        // The variable 'a' is deleted, so calculations should be re-evaluated and variable cache updated
        assert!(app.variables_cache.is_empty());

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_compute_syntax_highlights_markdown() {
        let lines = vec![
            "# h1".chars().collect::<Vec<char>>(),
            "## h2".chars().collect::<Vec<char>>(),
            "### h3".chars().collect::<Vec<char>>(),
            "#### h4".chars().collect::<Vec<char>>(),
            "##### h5".chars().collect::<Vec<char>>(),
            "###### h6".chars().collect::<Vec<char>>(),
            "> this is a blockquote".chars().collect::<Vec<char>>(),
            "---".chars().collect::<Vec<char>>(),
            "* first bullet".chars().collect::<Vec<char>>(),
            "10. first number".chars().collect::<Vec<char>>(),
            "This is **bold** text".chars().collect::<Vec<char>>(),
            "This is *italic* text".chars().collect::<Vec<char>>(),
            "This is ~~strikethrough~~ text"
                .chars()
                .collect::<Vec<char>>(),
        ];

        let highlights = crate::highlight::compute_syntax_highlights(&lines, None);

        // Heading Level 1: Purple
        assert_eq!(
            highlights
                .iter()
                .find(|h| h.start.row == 0 && h.start.col == 0 && h.end.col == 3)
                .unwrap()
                .style,
            Style::default().fg(Color::Rgb(187, 154, 247)).bold()
        );
        // Heading Level 2: Cyan
        assert_eq!(
            highlights
                .iter()
                .find(|h| h.start.row == 1 && h.start.col == 0 && h.end.col == 4)
                .unwrap()
                .style,
            Style::default().fg(Color::Rgb(125, 207, 255)).bold()
        );
        // Heading Level 3: Blue
        assert_eq!(
            highlights
                .iter()
                .find(|h| h.start.row == 2 && h.start.col == 0 && h.end.col == 5)
                .unwrap()
                .style,
            Style::default().fg(Color::Rgb(122, 162, 247)).bold()
        );
        // Heading Level 4: Teal
        assert_eq!(
            highlights
                .iter()
                .find(|h| h.start.row == 3 && h.start.col == 0 && h.end.col == 6)
                .unwrap()
                .style,
            Style::default().fg(Color::Rgb(115, 218, 202)).bold()
        );
        // Heading Level 5: Green
        assert_eq!(
            highlights
                .iter()
                .find(|h| h.start.row == 4 && h.start.col == 0 && h.end.col == 7)
                .unwrap()
                .style,
            Style::default().fg(Color::Rgb(158, 206, 106)).bold()
        );
        // Heading Level 6+: Orange
        assert_eq!(
            highlights
                .iter()
                .find(|h| h.start.row == 5 && h.start.col == 0 && h.end.col == 8)
                .unwrap()
                .style,
            Style::default().fg(Color::Rgb(255, 158, 100)).bold()
        );

        // Row 6: Blockquote (Italic Green Color::Rgb(158, 206, 106))
        assert!(highlights.iter().any(|h| h.start.row == 6
            && h.start.col == 0
            && h.end.col == 21
            && h.style.fg == Some(Color::Rgb(158, 206, 106))));

        // Row 7: HR (Dim Gray Color::Rgb(86, 95, 137))
        assert!(highlights.iter().any(|h| h.start.row == 7
            && h.start.col == 0
            && h.end.col == 2
            && h.style.fg == Some(Color::Rgb(86, 95, 137))));

        // Row 8: Bullet list (* at [0, 0] is Bold Orange Color::Rgb(255, 158, 100))
        assert!(highlights.iter().any(|h| h.start.row == 8
            && h.start.col == 0
            && h.end.col == 0
            && h.style.fg == Some(Color::Rgb(255, 158, 100))));

        // Row 9: Number list ("10." at [0, 2] is Bold Orange)
        assert!(highlights.iter().any(|h| h.start.row == 9
            && h.start.col == 0
            && h.end.col == 2
            && h.style.fg == Some(Color::Rgb(255, 158, 100))));

        // Row 10: Bold ("**bold**" at [8, 15] is bold)
        let bold_hl = highlights
            .iter()
            .find(|h| h.start.row == 10 && h.start.col == 8 && h.end.col == 15)
            .unwrap();
        assert_eq!(
            bold_hl.style,
            Style::default().fg(Color::Rgb(169, 177, 214)).bold()
        );

        // Row 11: Italic ("*italic*" at [8, 15] is italic)
        let italic_hl = highlights
            .iter()
            .find(|h| h.start.row == 11 && h.start.col == 8 && h.end.col == 15)
            .unwrap();
        assert_eq!(
            italic_hl.style,
            Style::default().fg(Color::Rgb(169, 177, 214)).italic()
        );

        // Row 12: Crossed out ("~~strikethrough~~" at [8, 24] is crossed out)
        let strike_hl = highlights
            .iter()
            .find(|h| h.start.row == 12 && h.start.col == 8 && h.end.col == 24)
            .unwrap();
        assert_eq!(
            strike_hl.style,
            Style::default().fg(Color::Rgb(169, 177, 214)).crossed_out()
        );
    }

    #[test]
    fn test_compute_syntax_highlights_new_formatting() {
        let lines = vec![
            "my_func(123) => 123".chars().collect::<Vec<char>>(),
            "`45.6`".chars().collect::<Vec<char>>(),
            "[(https://google.com)]".chars().collect::<Vec<char>>(),
            "[Google](https://google.com)"
                .chars()
                .collect::<Vec<char>>(),
            "[[Google](https://google.com)]"
                .chars()
                .collect::<Vec<char>>(),
            "https://google.com".chars().collect::<Vec<char>>(),
            "for x in range(1, 11) {".chars().collect::<Vec<char>>(),
            "    sum = sum + x;".chars().collect::<Vec<char>>(),
            "    sum;".chars().collect::<Vec<char>>(),
            "} => 55".chars().collect::<Vec<char>>(),
        ];
        let highlights = crate::highlight::compute_syntax_highlights(&lines, None);

        let purple = Color::Rgb(187, 154, 247);
        let blue = Color::Rgb(122, 162, 247);
        let teal = Color::Rgb(115, 218, 202);
        let orange = Color::Rgb(255, 158, 100);

        // Row 0: "my_func(123) => 123"
        // "my_func" is at col 0..=6. Should be blue and bold.
        let hl_func = highlights
            .iter()
            .find(|h| h.start.row == 0 && h.start.col == 0 && h.end.col == 6)
            .unwrap();
        assert_eq!(hl_func.style, Style::default().fg(blue).bold());

        // "123" (inside) is at col 8..=10. Should be Teal.
        let hl_num1 = highlights
            .iter()
            .find(|h| h.start.row == 0 && h.start.col == 8 && h.end.col == 10)
            .unwrap();
        assert_eq!(hl_num1.style.fg, Some(teal));

        // "(" is at col 7, ")" is at col 11. Should be Orange.
        let hl_paren1 = highlights
            .iter()
            .find(|h| h.start.row == 0 && h.start.col == 7 && h.end.col == 7)
            .unwrap();
        assert_eq!(hl_paren1.style.fg, Some(orange));
        let hl_paren2 = highlights
            .iter()
            .find(|h| h.start.row == 0 && h.start.col == 11 && h.end.col == 11)
            .unwrap();
        assert_eq!(hl_paren2.style.fg, Some(orange));

        // Row 1: "`45.6`"
        // "45.6" is at col 1..=4. Should be Teal.
        let hl_num2 = highlights
            .iter()
            .find(|h| h.start.row == 1 && h.start.col == 1 && h.end.col == 4)
            .unwrap();
        assert_eq!(hl_num2.style.fg, Some(teal));

        // Row 2: "[(https://google.com)]" - should be purple & underlined
        let hl_link1 = highlights
            .iter()
            .find(|h| h.start.row == 2 && h.start.col == 0 && h.end.col == 21)
            .unwrap();
        assert_eq!(hl_link1.style, Style::default().fg(purple).underlined());

        // Row 3: "[Google](https://google.com)" - should be purple & underlined
        let hl_link2 = highlights
            .iter()
            .find(|h| h.start.row == 3 && h.start.col == 0 && h.end.col == 27)
            .unwrap();
        assert_eq!(hl_link2.style, Style::default().fg(purple).underlined());

        // Row 4: "[[Google](https://google.com)]" - should be purple & underlined
        let hl_link3 = highlights
            .iter()
            .find(|h| h.start.row == 4 && h.start.col == 0 && h.end.col == 28)
            .unwrap();
        assert_eq!(hl_link3.style, Style::default().fg(purple).underlined());

        // Row 5: "https://google.com" - should be purple & underlined
        let hl_link4 = highlights
            .iter()
            .find(|h| h.start.row == 5 && h.start.col == 0 && h.end.col == 17)
            .unwrap();
        assert_eq!(hl_link4.style, Style::default().fg(purple).underlined());

        // Row 6: "for x in range(1, 11) {"
        // "for" at col 0..=2 should be bold orange
        let hl_for = highlights
            .iter()
            .find(|h| h.start.row == 6 && h.start.col == 0 && h.end.col == 2)
            .unwrap();
        assert_eq!(hl_for.style, Style::default().fg(orange).bold());
        // "in" at col 6..=7 should be bold orange
        let hl_in = highlights
            .iter()
            .find(|h| h.start.row == 6 && h.start.col == 6 && h.end.col == 7)
            .unwrap();
        assert_eq!(hl_in.style, Style::default().fg(orange).bold());
        // "range" at col 9..=13 should be bold blue
        let hl_range = highlights
            .iter()
            .find(|h| h.start.row == 6 && h.start.col == 9 && h.end.col == 13)
            .unwrap();
        assert_eq!(hl_range.style, Style::default().fg(blue).bold());

        // Row 7: "    sum = sum + x;"
        // "=" at col 8 should be orange
        let hl_eq = highlights
            .iter()
            .find(|h| h.start.row == 7 && h.start.col == 8 && h.end.col == 8)
            .unwrap();
        assert_eq!(hl_eq.style.fg, Some(orange));

        // Row 8: "    sum;"
        // ";" at col 7 should be orange
        let hl_semi = highlights
            .iter()
            .find(|h| h.start.row == 8 && h.start.col == 7 && h.end.col == 7)
            .unwrap();
        assert_eq!(hl_semi.style.fg, Some(orange));

        // Row 9: "} => 55"
        // "}" at col 0 should be orange
        let hl_rbrace = highlights
            .iter()
            .find(|h| h.start.row == 9 && h.start.col == 0 && h.end.col == 0)
            .unwrap();
        assert_eq!(hl_rbrace.style.fg, Some(orange));
        // "=>" at col 2..=3 should be bold orange
        let hl_arrow = highlights
            .iter()
            .find(|h| h.start.row == 9 && h.start.col == 2 && h.end.col == 3)
            .unwrap();
        assert_eq!(hl_arrow.style, Style::default().fg(orange).bold());
    }

    #[test]
    fn test_compute_syntax_highlights_selected_var() {
        let lines = vec![
            "price = 100".chars().collect::<Vec<char>>(),
            "total = price * 2".chars().collect::<Vec<char>>(),
        ];

        let highlights = crate::highlight::compute_syntax_highlights(&lines, Some("price"));

        // In row 0, "price" at col 0..=4 should be highlighted with the selected variable style: bg(167, 82, 142), fg(224, 230, 242), bold.
        let hl_r0 = highlights
            .iter()
            .find(|h| h.start.row == 0 && h.start.col == 0 && h.end.col == 4)
            .unwrap();
        assert_eq!(
            hl_r0.style,
            Style::default()
                .bg(Color::Rgb(167, 82, 142))
                .fg(Color::Rgb(224, 230, 242))
                .bold()
        );

        // In row 1, "price" at col 8..=12 should also be highlighted with the selected variable style.
        let hl_r1 = highlights
            .iter()
            .find(|h| h.start.row == 1 && h.start.col == 8 && h.end.col == 12)
            .unwrap();
        assert_eq!(
            hl_r1.style,
            Style::default()
                .bg(Color::Rgb(167, 82, 142))
                .fg(Color::Rgb(224, 230, 242))
                .bold()
        );
    }

    #[test]
    fn test_compute_syntax_highlights_defined_vars_with_unit_names() {
        let lines = vec![
            "m = 10".chars().collect::<Vec<char>>(),
            "y = m * 2".chars().collect::<Vec<char>>(),
            "z = 5 m".chars().collect::<Vec<char>>(),
        ];

        let highlights = crate::highlight::compute_syntax_highlights(&lines, None);

        let pink = Color::Rgb(244, 143, 177);

        // Row 0: "m = 10" -> "m" is the LHS variable, should NOT be pink
        assert!(
            !highlights
                .iter()
                .any(|h| h.start.row == 0 && h.start.col == 0 && h.style.fg == Some(pink))
        );

        // Row 1: "y = m * 2" -> "m" at index 4 is used as variable, should NOT be pink
        assert!(!highlights.iter().any(|h| h.start.row == 1
            && h.start.col <= 4
            && h.end.col >= 4
            && h.style.fg == Some(pink)));

        // Row 2: "z = 5 m" -> "m" at index 6 is preceded by number "5", so it acts as unit, MUST be pink
        assert!(highlights.iter().any(|h| h.start.row == 2
            && h.start.col <= 6
            && h.end.col >= 6
            && h.style.fg == Some(pink)));
    }

    #[test]
    fn test_compute_syntax_highlights_percentage() {
        let lines = vec![
            "val = 10%".chars().collect::<Vec<char>>(),
            "mod_val = 10 % 3".chars().collect::<Vec<char>>(),
        ];

        let highlights = crate::highlight::compute_syntax_highlights(&lines, None);

        let pink = Color::Rgb(244, 143, 177);

        // Row 0: "val = 10%" -> "%" at index 8 is acting as a postfix percentage (unit), MUST be pink
        assert!(highlights.iter().any(|h| h.start.row == 0
            && h.start.col <= 8
            && h.end.col >= 8
            && h.style.fg == Some(pink)));

        // Row 1: "mod_val = 10 % 3" -> "%" at index 13 is acting as infix modulo (symbol), should NOT be pink
        assert!(!highlights.iter().any(|h| h.start.row == 1
            && h.start.col <= 13
            && h.end.col >= 13
            && h.style.fg == Some(pink)));
    }

    #[test]
    fn test_compute_syntax_highlights_no_markdown_in_equations() {
        let lines = vec![
            "gas_cost = gas_usage * rate".chars().collect::<Vec<char>>(),
            "We bought items for `price_val * quantity_val =>` total"
                .chars()
                .collect::<Vec<char>>(),
            "testing inline `price * quantity => 500` before tax"
                .chars()
                .collect::<Vec<char>>(),
        ];

        let highlights = crate::highlight::compute_syntax_highlights(&lines, None);

        let has_italic_text = highlights
            .iter()
            .any(|h| h.start.row == 0 && h.style.add_modifier.contains(Modifier::ITALIC));
        assert!(
            !has_italic_text,
            "Markdown italics should be ignored on math lines"
        );

        let has_italic_backticks_text = highlights.iter().any(|h| {
            h.start.row == 1
                && h.style.add_modifier.contains(Modifier::ITALIC)
                && h.style.fg == Some(Color::Rgb(169, 177, 214))
        });
        assert!(
            !has_italic_backticks_text,
            "Markdown italics should be ignored inside backtick blocks"
        );

        // Verify that in "testing inline `price * quantity => 500` before tax",
        // the text outside the backticks is not styled with the math colors (Teal/Cyan/etc.).
        let has_spill_highlight = highlights.iter().any(|h| {
            h.start.row == 2
                && (h.start.col < 15 || h.start.col > 37)
                && h.style.fg == Some(Color::Rgb(125, 207, 255))
        });
        assert!(
            !has_spill_highlight,
            "Math highlighting should not spill outside backticks"
        );
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

        // Press '0' should not close modal and do nothing
        let handled = handle_modal_key(
            &mut app,
            crossterm::event::KeyEvent::new(
                KeyCode::Char('0'),
                crossterm::event::KeyModifiers::NONE,
            ),
        );
        assert!(handled);
        assert!(app.show_help);

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
    fn test_app_config_load_save() {
        // Test defaults
        let default_config = AppConfig::default();
        assert_eq!(default_config.scrolloff, 5);
        assert!(default_config.mouse_focus_on_hover);
        assert!(!default_config.expand_variables_on_select);
        assert_eq!(default_config.ignored_update_hash, None);
        assert_eq!(default_config.line_numbers, "None");
        assert!(default_config.word_wrap);

        // Test serialization and deserialization
        let custom_config = AppConfig {
            scrolloff: 8,
            mouse_focus_on_hover: false,
            expand_variables_on_select: true,
            ignored_update_hash: Some("test_hash_val".to_string()),
            line_numbers: "Absolute".to_string(),
            word_wrap: false,
        };
        let serialized = serde_json::to_string_pretty(&custom_config).unwrap();
        let deserialized: AppConfig = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.scrolloff, 8);
        assert!(!deserialized.mouse_focus_on_hover);
        assert!(deserialized.expand_variables_on_select);
        assert_eq!(
            deserialized.ignored_update_hash,
            Some("test_hash_val".to_string())
        );
        assert_eq!(deserialized.line_numbers, "Absolute");
        assert!(!deserialized.word_wrap);

        // Test fallback defaults during deserialization (e.g. if fields are missing in JSON)
        let partial_json = r#"{"scrolloff": 12}"#;
        let deserialized_partial: AppConfig = serde_json::from_str(partial_json).unwrap();
        assert_eq!(deserialized_partial.scrolloff, 12);
        assert!(deserialized_partial.mouse_focus_on_hover); // Default true fallback
        assert!(!deserialized_partial.expand_variables_on_select); // Default false fallback
        assert_eq!(deserialized_partial.line_numbers, "None"); // Default None fallback
        assert!(deserialized_partial.word_wrap); // Default true fallback
    }

    #[test]
    fn test_search_and_export() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_search_export");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }

        let mut app = App::new(wiki_root.clone()).unwrap();

        // 1. Create a dummy note with some search keyword
        let dummy_path = wiki_root.join("dummy-note.md");
        std::fs::write(
            &dummy_path,
            "# Dummy Note\nThis is a unique_keyword inside a note.",
        )
        .unwrap();

        // 2. Perform search
        app.search_query = "unique_keyword".to_string();
        app.perform_wiki_search();

        assert!(app.show_search_results);
        assert_eq!(app.search_results.len(), 1);
        assert_eq!(app.search_results[0], "Dummy Note");

        // 3. Export HTML
        let html_path = app.export_current_note_to_html().unwrap();
        assert!(html_path.exists());
        let html_content = std::fs::read_to_string(&html_path).unwrap();
        assert!(html_content.contains("<!DOCTYPE html>"));

        // 4. Compile Wiki to Markdown
        let md_path = app.compile_wiki_to_markdown().unwrap();
        assert!(md_path.exists());
        let md_content = std::fs::read_to_string(&md_path).unwrap();
        assert!(md_content.contains("# calki Compiled Wiki"));

        // Clean up
        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_export_menu_dialog() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_export_menu");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();

        let mut app = App::new(wiki_root.clone()).unwrap();
        app.show_export_menu = false;

        app.show_export_menu = true;
        assert!(app.show_export_menu);

        app.show_export_menu = false;
        assert!(!app.show_export_menu);

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_scroll_level_indicator() {
        let wiki_root = std::env::current_dir()
            .unwrap()
            .join("test_wiki_temp_scroll_level");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();
        let mut app = App::new(wiki_root.clone()).unwrap();

        // Case 1: 0 or 1 lines
        app.editor_state = EditorState::new(edtui::Lines::from(""));
        let total_lines = app.editor_state.lines.len();
        let scroll_pct = if total_lines <= 1 {
            0
        } else {
            (app.editor_state.cursor.row * 100) / (total_lines - 1)
        };
        assert_eq!(scroll_pct, 0);
        assert_eq!(format!("{:>3}%", scroll_pct), "  0%");

        // Case 2: Multi-lines
        app.editor_state =
            EditorState::new(edtui::Lines::from("line 1\nline 2\nline 3\nline 4\nline 5"));
        let total_lines = app.editor_state.lines.len();
        assert_eq!(total_lines, 5);

        // top line
        app.editor_state.cursor.row = 0;
        let pct = (app.editor_state.cursor.row * 100) / (total_lines - 1);
        assert_eq!(pct, 0);
        assert_eq!(format!("{:>3}%", pct), "  0%");

        // middle line
        app.editor_state.cursor.row = 2;
        let pct = (app.editor_state.cursor.row * 100) / (total_lines - 1);
        assert_eq!(pct, 50);
        assert_eq!(format!("{:>3}%", pct), " 50%");

        // bottom line
        app.editor_state.cursor.row = 4;
        let pct = (app.editor_state.cursor.row * 100) / (total_lines - 1);
        assert_eq!(pct, 100);
        assert_eq!(format!("{:>3}%", pct), "100%");

        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_estimate_line_height() {
        let chars_empty: Vec<char> = vec![];
        assert_eq!(estimate_line_height(&chars_empty, 10, 4), 1);

        let chars_short: Vec<char> = "hello".chars().collect();
        assert_eq!(estimate_line_height(&chars_short, 10, 4), 1);

        let chars_exact: Vec<char> = "hello world".chars().collect();
        assert_eq!(estimate_line_height(&chars_exact, 5, 4), 3);

        let chars_long: Vec<char> = "hello world from rust".chars().collect();
        assert_eq!(estimate_line_height(&chars_long, 7, 4), 4);
    }
}
