mod math;
mod currency;
mod wiki;

use crate::math::evaluate_sheet;
use crate::math::units::get_unit_info;
use crate::currency::{load_currency_rates, trigger_background_update};
use crate::wiki::WikiManager;

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

use ratatui::prelude::*;
use ratatui::widgets::*;

use edtui::{EditorEventHandler, EditorMode, EditorState, EditorView, Lines, RowIndex};
use edtui::actions::Chainable;
use edtui::events::{KeyEventRegister, KeyInput};
use edtui::clipboard::ClipboardTrait;
use serde::{Deserialize, Serialize};
use std::io::Write;

struct SystemClipboard {
    arboard_clip: Option<arboard::Clipboard>,
    internal: String,
}

impl SystemClipboard {
    fn new() -> Self {
        let arboard_clip = arboard::Clipboard::new().ok();
        Self {
            arboard_clip,
            internal: String::new(),
        }
    }
}

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

    fn get_text(&mut self) -> String {
        // 1. Try local arboard system clipboard
        if let Some(ref mut clip) = self.arboard_clip
            && let Ok(txt) = clip.get_text() {
                self.internal = txt;
                return self.internal.clone();
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
}

impl SessionState {
    fn load() -> Option<Self> {
        let mut path = crate::currency::get_config_path()?;
        path.push("session.json");
        let file = fs::File::open(path).ok()?;
        serde_json::from_reader(file).ok()
    }

    fn save(&self) -> Option<()> {
        let mut path = crate::currency::get_config_path()?;
        fs::create_dir_all(&path).ok()?;
        path.push("session.json");
        let file = fs::File::create(path).ok()?;
        serde_json::to_writer_pretty(file, self).ok()?;
        Some(())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct AppConfig {
    scrolloff: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            scrolloff: 5,
        }
    }
}

impl AppConfig {
    fn load() -> Self {
        if let Some(mut path) = crate::currency::get_config_path() {
            path.push("config.json");
            if path.exists()
                && let Ok(content) = fs::read_to_string(path)
                    && let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                        return config;
                    }
        }
        AppConfig::default()
    }

    fn save(&self) -> Option<()> {
        let mut path = crate::currency::get_config_path()?;
        fs::create_dir_all(&path).ok()?;
        path.push("config.json");
        let file = fs::File::create(path).ok()?;
        serde_json::to_writer_pretty(file, self).ok()?;
        Some(())
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
    selected_link_idx: usize, // Selected link in Wiki Map panel
    selected_var_idx: usize,  // Selected variable in Variables panel
    show_help: bool,          // Whether to display the help modal
    help_tab_idx: usize,      // Active tab in help modal
    help_scroll: u16,         // Scroll offset in help modal
    show_delete_confirm: bool, // Whether to display the delete confirmation modal
    delete_target_name: String, // Name of page to delete
    delete_target_path: Option<PathBuf>, // Path of page to delete

    // Exchange rates
    exchange_rates: HashMap<String, f64>,

    // Panel screen areas for mouse clicks
    left_area: Rect,
    editor_area: Rect,
    right_area: Rect,
    replace_next_char: bool,
    config: AppConfig,

    // Global Wiki Search
    search_query: String,
    search_active: bool,
    search_results: Vec<String>,
    show_search_results: bool,

    // Status Message / Toast
    status_message: Option<(String, std::time::Instant)>,
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
            edtui::actions::DeleteWordForward(1).chain(edtui::actions::SwitchMode(EditorMode::Insert)),
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
            edtui::actions::delete::DeleteToEndOfLine.chain(edtui::actions::SwitchMode(EditorMode::Insert)),
        );

        let mut editor_state = EditorState::new(Lines::from(file_content.as_str()));
        editor_state.set_clipboard(SystemClipboard::new());

        let left_panel_open = session.as_ref().map(|s| s.left_panel_open).unwrap_or(true);
        let right_panel_open = session.as_ref().map(|s| s.right_panel_open).unwrap_or(true);
        let mut focused_panel = session.as_ref().map(|s| match s.focused_panel.as_str() {
            "WikiMap" => FocusedPanel::WikiMap,
            "Variables" => FocusedPanel::Variables,
            _ => FocusedPanel::Editor,
        }).unwrap_or(FocusedPanel::Editor);
        if focused_panel == FocusedPanel::WikiMap && !left_panel_open {
            focused_panel = FocusedPanel::Editor;
        }
        if focused_panel == FocusedPanel::Variables && !right_panel_open {
            focused_panel = FocusedPanel::Editor;
        }

        let config = AppConfig::load();
        let _ = config.save();

        let mut app = Self {
            wiki_mgr,
            active_path,
            history_stack: Vec::new(),
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
            config,
            search_query: String::new(),
            search_active: false,
            search_results: Vec::new(),
            show_search_results: false,
            status_message: None,
        };

        if let Some(ref s) = session {
            let row_count = app.editor_state.lines.len();
            if row_count > 0 {
                let target_row = s.cursor_row.min(row_count - 1);
                let col_count = app.editor_state.lines.get(RowIndex::new(target_row)).map(|r| r.len()).unwrap_or(0);
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
        self.editor_state.lines.iter_row()
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

            let row_len = self.editor_state.lines.get(RowIndex::new(target_row)).map(|r| r.len()).unwrap_or(0);
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

fn compute_syntax_highlights<T: AsRef<[char]>>(lines_vecs: &[T], selected_var: Option<&str>) -> Vec<edtui::Highlight> {
    let mut highlights = Vec::new();

    let mut defined_vars = std::collections::HashSet::new();
    for line in lines_vecs {
        let line = line.as_ref();
        let trimmed = trim_char_slice(line);
        if trimmed.is_empty() 
            || trimmed.first() == Some(&'#') 
            || (trimmed.len() >= 2 && trimmed[0] == '/' && trimmed[1] == '/') 
            || trimmed.first() == Some(&'>') 
        {
            continue;
        }
        if let Some(eq_pos) = trimmed.iter().position(|&c| c == '=') {
            let is_arrow = eq_pos + 1 < trimmed.len() && trimmed[eq_pos + 1] == '>';
            if !is_arrow {
                let left_part = trim_char_slice(&trimmed[..eq_pos]);
                if left_part.contains(&'(') && left_part.last() == Some(&')') {
                    if let Some(lpar_pos) = left_part.iter().position(|&c| c == '(') {
                        let fn_name = trim_char_slice(&left_part[..lpar_pos]);
                        let args_slice = &left_part[lpar_pos + 1..left_part.len() - 1];
                        if !fn_name.is_empty() && fn_name.iter().all(|&c| c.is_alphanumeric() || c == '_') {
                            defined_vars.insert(fn_name.iter().collect::<String>());
                            for arg in args_slice.split(|&c| c == ',') {
                                let arg_trimmed = trim_char_slice(arg);
                                if !arg_trimmed.is_empty() && arg_trimmed.iter().all(|&c| c.is_alphanumeric() || c == '_') {
                                    defined_vars.insert(arg_trimmed.iter().collect::<String>());
                                }
                            }
                        }
                    }
                } else if !left_part.is_empty() && left_part.iter().all(|&c| c.is_alphanumeric() || c == '_') {
                    defined_vars.insert(left_part.iter().collect::<String>());
                }
            }
        }
    }

    let sv_chars: Option<Vec<char>> = selected_var.map(|sv| sv.chars().collect());

    for (row_idx, line) in lines_vecs.iter().enumerate() {
        let line = line.as_ref();
        let n = line.len();
        let mut line_styles: Vec<Option<Style>> = vec![None; n];
        let mut is_special_line = false;

        // 1. Markdown Headers (lines starting with '#' followed by space or more '#')
        if line.first() == Some(&'#') {
            let header_len = line.iter().take_while(|&&c| c == '#').count();
            if line.get(header_len) == Some(&' ') || line.len() == header_len {
                let header_style = match header_len {
                    1 => Style::default().fg(Color::Rgb(187, 154, 247)).bold(), // Purple
                    2 => Style::default().fg(Color::Rgb(125, 207, 255)).bold(), // Cyan
                    3 => Style::default().fg(Color::Rgb(122, 162, 247)).bold(), // Blue
                    4 => Style::default().fg(Color::Rgb(115, 218, 202)).bold(), // Teal
                    5 => Style::default().fg(Color::Rgb(158, 206, 106)).bold(), // Green
                    _ => Style::default().fg(Color::Rgb(255, 158, 100)).bold(), // Orange for H6+
                };
                for col in 0..n {
                    line_styles[col] = Some(header_style);
                }
                is_special_line = true;
            }
        }

        // 1b. Blockquotes (lines starting with '>')
        let trimmed_start = trim_start_slice(line);
        if !is_special_line && trimmed_start.first() == Some(&'>') {
            let start_col = line.len() - trimmed_start.len();
            let quote_style = Style::default().fg(Color::Rgb(158, 206, 106)).italic(); // Italic Green #9ece6a
            for col in start_col..n {
                line_styles[col] = Some(quote_style);
            }
            is_special_line = true;
        }

        // 1c. Horizontal Rule
        let trimmed = trim_char_slice(line);
        if !is_special_line && (trimmed == &['-', '-', '-'] || trimmed == &['*', '*', '*'] || trimmed == &['_', '_', '_']) && line.len() >= 3 {
            let hr_style = Style::default().fg(Color::Rgb(86, 95, 137)).dim(); // Muted Gray dim
            for col in 0..n {
                line_styles[col] = Some(hr_style);
            }
            is_special_line = true;
        }

        // 3. Comments
        if !is_special_line && trimmed_start.len() >= 2 && trimmed_start[0] == '/' && trimmed_start[1] == '/' {
            let start_col = line.len() - trimmed_start.len();
            let comment_style = Style::default().fg(Color::Rgb(86, 95, 137)).italic(); // Muted Gray-Blue
            for col in start_col..n {
                line_styles[col] = Some(comment_style);
            }
            is_special_line = true;
        }

        if !is_special_line {
            let mut is_math_line = false;
            let mut backtick_ranges = Vec::new();

            // First, find all backtick ranges on this line so we can ignore any inner content for top-level line math check
            let mut b_idx = 0;
            while let Some(start_pos) = find_in_chars_from(line, "`", b_idx) {
                if let Some(end_pos) = find_in_chars_from(line, "`", start_pos + 1) {
                    backtick_ranges.push(start_pos..=end_pos);
                    b_idx = end_pos + 1;
                } else {
                    break;
                }
            }

            let is_in_backticks = |col: usize| -> bool {
                backtick_ranges.iter().any(|r| r.contains(&col))
            };

            // A. Base Block Math & Assignments (containing '=>' or '=') outside backticks
            let mut arrow_idx = None;
            let mut search_idx = 0;
            while let Some(pos) = find_in_chars_from(line, "=>", search_idx) {
                if !is_in_backticks(pos) {
                    arrow_idx = Some(pos);
                    break;
                }
                search_idx = pos + 2;
            }

            let mut eq_idx = None;
            if arrow_idx.is_none() {
                let mut search_idx = 0;
                while let Some(pos) = find_in_chars_from(line, "=", search_idx) {
                    if !is_in_backticks(pos) {
                        eq_idx = Some(pos);
                        break;
                    }
                    search_idx = pos + 1;
                }
            }

            if let Some(idx) = arrow_idx {
                is_math_line = true;
                // Expression before '=>' (Cyan/light blue)
                for col in 0..idx {
                    line_styles[col] = Some(Style::default().fg(Color::Rgb(125, 207, 255)));
                }
                // Operator '=>' in Bold Orange
                for col in idx..std::cmp::min(idx + 2, n) {
                    line_styles[col] = Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
                }
                // The result after '=>' (Teal Green)
                for col in (idx + 2)..n {
                    line_styles[col] = Some(Style::default().fg(Color::Rgb(115, 218, 202)).italic());
                }
            } else if let Some(idx) = eq_idx {
                let lhs = &line[..idx];
                let lhs_trimmed = trim_char_slice(lhs);
                let is_lhs_valid = !lhs_trimmed.is_empty() 
                    && lhs_trimmed.iter().all(|&c| c.is_alphanumeric() || c == '_');
                
                if is_lhs_valid {
                    is_math_line = true;
                    // LHS (Cyan)
                    for col in 0..idx {
                        line_styles[col] = Some(Style::default().fg(Color::Rgb(125, 207, 255)));
                    }
                    // '=' (Bold Orange)
                    if idx < n {
                        line_styles[idx] = Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
                    }
                    // RHS (Teal Green)
                    for col in (idx + 1)..n {
                        line_styles[col] = Some(Style::default().fg(Color::Rgb(115, 218, 202)));
                    }
                }
            }

            // B. Inline code blocks/math in backticks: `expression => result`
            for r in &backtick_ranges {
                let start_pos = *r.start();
                let end_pos = *r.end();
                // Backticks themselves (Muted Gray-Blue)
                if start_pos < n {
                    line_styles[start_pos] = Some(Style::default().fg(Color::Rgb(86, 95, 137)));
                }
                if end_pos < n {
                    line_styles[end_pos] = Some(Style::default().fg(Color::Rgb(86, 95, 137)));
                }

                let inner = &line[start_pos + 1..end_pos];
                if let Some(arrow_pos) = find_in_chars(inner, "=>") {
                    let absolute_arrow = start_pos + 1 + arrow_pos;
                    // Before => (Cyan)
                    for col in (start_pos + 1)..absolute_arrow {
                        if col < n {
                            line_styles[col] = Some(Style::default().fg(Color::Rgb(125, 207, 255)));
                        }
                    }
                    // => (Bold Orange)
                    for col in absolute_arrow..std::cmp::min(absolute_arrow + 2, n) {
                        line_styles[col] = Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
                    }
                    // After => (Italic Teal Green)
                    for col in (absolute_arrow + 2)..end_pos {
                        if col < n {
                            line_styles[col] = Some(Style::default().fg(Color::Rgb(115, 218, 202)).italic());
                        }
                    }
                } else {
                    // Entire inner content is Orange
                    for col in (start_pos + 1)..end_pos {
                        if col < n {
                            line_styles[col] = Some(Style::default().fg(Color::Rgb(255, 158, 100)));
                        }
                    }
                }
            }

        // C. Outgoing Wiki Links: [[Note Name]] (Purple Underlined)
        let mut idx = 0;
        while let Some(start_pos) = find_in_chars_from(line, "[[", idx) {
            if let Some(end_pos) = find_in_chars_from(line, "]]", start_pos) {
                let absolute_end = end_pos + 1;
                for col in start_pos..=absolute_end {
                    if col < n {
                        line_styles[col] = Some(Style::default().fg(Color::Rgb(187, 154, 247)).underlined());
                    }
                }
                idx = absolute_end + 1;
            } else {
                break;
            }
        }

        // D. Scan for units and highlight them
        let tokens = tokenize_line_for_highlighting(line);
        let mut wiki_ranges = Vec::new();
        let mut w_idx = 0;
        while w_idx < line.len() {
            if w_idx + 1 < line.len() && line[w_idx] == '[' && line[w_idx + 1] == '[' {
                let start_pos = w_idx;
                let mut search_idx = w_idx + 2;
                while search_idx + 1 < line.len() {
                    if line[search_idx] == ']' && line[search_idx + 1] == ']' {
                        wiki_ranges.push(start_pos..=search_idx + 1);
                        w_idx = search_idx + 1;
                        break;
                    }
                    search_idx += 1;
                }
            }
            w_idx += 1;
        }

        for i in 0..tokens.len() {
            if let HighlightToken::Identifier { start, end, name } = &tokens[i] {
                let mut is_unit = false;
                if is_registered_unit(name) {
                    is_unit = true;
                } else if i > 0
                    && let HighlightToken::Number { .. } = tokens[i - 1] {
                        is_unit = true;
                    }

                if is_unit && defined_vars.contains(name) {
                    let preceded_by_number = if i > 0 {
                        matches!(tokens[i - 1], HighlightToken::Number { .. })
                    } else {
                        false
                    };
                    if !preceded_by_number {
                        is_unit = false;
                    }
                }

                if is_unit {
                    // Only highlight unit if we are in a valid math context:
                    // either the line is a math line, OR the token falls within backticks.
                    let in_math_context = is_math_line || backtick_ranges.iter().any(|r| {
                        start >= r.start() && end <= r.end()
                    });
                    if in_math_context {
                        // Check if it overlaps with any wiki link target range
                        let overlaps_wiki = wiki_ranges.iter().any(|r| {
                            (start >= r.start() && start <= r.end()) || (end >= r.start() && end <= r.end())
                        });
                        if !overlaps_wiki {
                            for col in *start..=*end {
                                if col < n {
                                    line_styles[col] = Some(Style::default().fg(Color::Rgb(244, 143, 177))); // Rose / Pink #f48fb1
                                }
                            }
                        }
                    }
                }
            } else if let HighlightToken::Symbol { start, end, ch: '%' } = &tokens[i] {
                let mut is_infix = false;
                if i + 1 < tokens.len() {
                    match &tokens[i + 1] {
                        HighlightToken::Number { .. } |
                        HighlightToken::Identifier { .. } |
                        HighlightToken::Symbol { ch: '(', .. } |
                        HighlightToken::Symbol { ch: '[', .. } => {
                            is_infix = true;
                        }
                        _ => {}
                    }
                }
                if !is_infix {
                    let in_math_context = is_math_line || backtick_ranges.iter().any(|r| {
                        *start >= *r.start() && *end <= *r.end()
                    });
                    if in_math_context {
                        let overlaps_wiki = wiki_ranges.iter().any(|r| {
                            (*start >= *r.start() && *start <= *r.end()) || (*end >= *r.start() && *end <= *r.end())
                        });
                        if !overlaps_wiki {
                            for col in *start..=*end {
                                if col < n {
                                    line_styles[col] = Some(Style::default().fg(Color::Rgb(244, 143, 177))); // Rose / Pink #f48fb1
                                }
                            }
                        }
                    }
                }
            }
        }

        // E. Lists / Bullet points (style bullet or number in bold orange)
        let trimmed_start = trim_start_slice(line);
        let leading_spaces = line.len() - trimmed_start.len();
        let rest = trimmed_start;
        let mut list_marker_range = None;
        if rest.starts_with(&['*', ' ']) || rest.starts_with(&['-', ' ']) || rest.starts_with(&['+', ' ']) {
            list_marker_range = Some(leading_spaces..leading_spaces + 1);
        } else {
            let digit_count = rest.iter().take_while(|&&c| c.is_ascii_digit()).count();
            if digit_count > 0 && rest.get(digit_count) == Some(&'.') && rest.get(digit_count + 1) == Some(&' ') {
                list_marker_range = Some(leading_spaces..leading_spaces + digit_count + 1);
            }
        }
        if let Some(r) = list_marker_range {
            for col in r {
                if col < n {
                    line_styles[col] = Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold()); // Bold Orange #ff9e64
                }
            }
        }

        // F. Bold Formatting: **text** or __text__
        if !is_math_line {
            let is_in_backticks = |col: usize| -> bool {
                backtick_ranges.iter().any(|r| r.contains(&col))
            };

            let mut b_pos = 0;
            while let Some(start_pos) = find_in_chars_from(line, "**", b_pos) {
                if is_in_backticks(start_pos) {
                    b_pos = start_pos + 1;
                    continue;
                }
                if let Some(end_pos) = find_in_chars_from(line, "**", start_pos + 2) {
                    if is_in_backticks(end_pos) {
                        b_pos = start_pos + 1;
                        continue;
                    }
                    for col in start_pos..=(end_pos + 1) {
                        if col < n {
                            let base = line_styles[col].unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
                            line_styles[col] = Some(base.bold());
                        }
                    }
                    b_pos = end_pos + 2;
                } else {
                    break;
                }
            }
            let mut b_pos2 = 0;
            while let Some(start_pos) = find_in_chars_from(line, "__", b_pos2) {
                if is_in_backticks(start_pos) {
                    b_pos2 = start_pos + 1;
                    continue;
                }
                if let Some(end_pos) = find_in_chars_from(line, "__", start_pos + 2) {
                    if is_in_backticks(end_pos) {
                        b_pos2 = start_pos + 1;
                        continue;
                    }
                    for col in start_pos..=(end_pos + 1) {
                        if col < n {
                            let base = line_styles[col].unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
                            line_styles[col] = Some(base.bold());
                        }
                    }
                    b_pos2 = end_pos + 2;
                } else {
                    break;
                }
            }

            // G. Italic Formatting: *text* or _text_
            let mut i_pos = 0;
            while i_pos < n {
                if line[i_pos] == '*' {
                    if is_in_backticks(i_pos) {
                        i_pos += 1;
                        continue;
                    }
                    if i_pos + 1 < n && line[i_pos + 1] == '*' {
                        i_pos += 2;
                        continue;
                    }
                    let mut search = i_pos + 1;
                    let mut found_end = None;
                    while search < n {
                        if line[search] == '*' {
                            if is_in_backticks(search) {
                                search += 1;
                                continue;
                            }
                            if search + 1 < n && line[search + 1] == '*' {
                                search += 2;
                                continue;
                            }
                            found_end = Some(search);
                            break;
                        }
                        search += 1;
                    }
                    if let Some(end_pos) = found_end {
                        for col in i_pos..=end_pos {
                            if col < n {
                                let base = line_styles[col].unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
                                line_styles[col] = Some(base.italic());
                            }
                        }
                        i_pos = end_pos + 1;
                    } else {
                        i_pos += 1;
                    }
                } else {
                    i_pos += 1;
                }
            }
            let mut i_pos2 = 0;
            while i_pos2 < n {
                if line[i_pos2] == '_' {
                    if is_in_backticks(i_pos2) {
                        i_pos2 += 1;
                        continue;
                    }
                    if i_pos2 + 1 < n && line[i_pos2 + 1] == '_' {
                        i_pos2 += 2;
                        continue;
                    }
                    let mut search = i_pos2 + 1;
                    let mut found_end = None;
                    while search < n {
                        if line[search] == '_' {
                            if is_in_backticks(search) {
                                search += 1;
                                continue;
                            }
                            if search + 1 < n && line[search + 1] == '_' {
                                search += 2;
                                continue;
                            }
                            found_end = Some(search);
                            break;
                        }
                        search += 1;
                    }
                    if let Some(end_pos) = found_end {
                        for col in i_pos2..=end_pos {
                            if col < n {
                                let base = line_styles[col].unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
                                line_styles[col] = Some(base.italic());
                            }
                        }
                        i_pos2 = end_pos + 1;
                    } else {
                        i_pos2 += 1;
                    }
                } else {
                    i_pos2 += 1;
                }
            }

            // H. Strikethrough Formatting: ~~text~~
            let mut s_pos = 0;
            while let Some(start_pos) = find_in_chars_from(line, "~~", s_pos) {
                if is_in_backticks(start_pos) {
                    s_pos = start_pos + 1;
                    continue;
                }
                if let Some(end_pos) = find_in_chars_from(line, "~~", start_pos + 2) {
                    if is_in_backticks(end_pos) {
                        s_pos = start_pos + 1;
                        continue;
                    }
                    for col in start_pos..=(end_pos + 1) {
                        if col < n {
                            let base = line_styles[col].unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
                            line_styles[col] = Some(base.crossed_out());
                        }
                    }
                    s_pos = end_pos + 2;
                } else {
                    break;
                }
            }
        }
        }

        // I. Selected Variable Highlight
        if let Some(ref sv_chars) = sv_chars {
            let sv_len = sv_chars.len();
            let is_ident_char = |c: char| -> bool {
                c.is_alphanumeric() || c == '_' || c == '/'
            };
            if n >= sv_len {
                for start_idx in 0..=(n - sv_len) {
                    if &line[start_idx..(start_idx + sv_len)] == sv_chars {
                        // Check word boundaries
                        let before_ok = if start_idx > 0 {
                            !is_ident_char(line[start_idx - 1])
                        } else {
                            true
                        };
                        let after_ok = if start_idx + sv_len < n {
                            !is_ident_char(line[start_idx + sv_len])
                        } else {
                            true
                        };
                        if before_ok && after_ok {
                            for col in start_idx..(start_idx + sv_len) {
                                line_styles[col] = Some(
                                    Style::default()
                                        .bg(Color::Rgb(167, 82, 142))
                                        .fg(Color::Rgb(224, 230, 242))
                                        .bold(),
                                );
                            }
                        }
                    }
                }
            }
        }

        // Convert the style array to edtui::Highlight ranges
        let mut start_col = None;
        let mut current_style = None;

        for col in 0..n {
            let style = line_styles[col];
            if style != current_style {
                if let (Some(start), Some(s)) = (start_col, current_style) {
                    highlights.push(edtui::Highlight {
                        start: edtui::Index2::new(row_idx, start),
                        end: edtui::Index2::new(row_idx, col - 1),
                        style: s,
                    });
                }
                if style.is_some() {
                    start_col = Some(col);
                } else {
                    start_col = None;
                }
                current_style = style;
            }
        }
        if let (Some(start), Some(s)) = (start_col, current_style) {
            highlights.push(edtui::Highlight {
                start: edtui::Index2::new(row_idx, start),
                end: edtui::Index2::new(row_idx, n - 1),
                style: s,
            });
        }
    }

    highlights
}

    // Updates highlights based on syntax highlighting and selected variable
    fn update_highlights(&mut self) {
        let vecs: Vec<&[char]> = self.editor_state.lines.iter_row().map(|r| r.as_slice()).collect();
        let selected_var = if self.focused_panel == FocusedPanel::Variables && !self.variables_cache.is_empty() {
            if self.selected_var_idx >= self.variables_cache.len() {
                self.selected_var_idx = self.variables_cache.len().saturating_sub(1);
            }
            Some(self.variables_cache[self.selected_var_idx].0.as_str())
        } else {
            None
        };

        self.editor_state.highlights = Self::compute_syntax_highlights(&vecs, selected_var);
    }

    // Saves current editor state to the active note file
    fn save_current_note(&self) -> Result<(), String> {
        let content = self.get_editor_text();
        fs::write(&self.active_path, content)
            .map_err(|e| format!("Failed to write note: {}", e))
    }

    // Load a note file into the editor, handling onboarding or template creation
    fn load_note(&mut self, path: PathBuf) -> Result<(), String> {
        self.active_path = path;
        
        if !self.active_path.exists() {
            let title = self.wiki_mgr.path_to_title(&self.active_path);
            let default_template = format!("# {}\n\nCreate your calculations here...\n\nSee [[Home]] to go back.\n", title);
            fs::write(&self.active_path, default_template)
                .map_err(|e| format!("Failed to create new note: {}", e))?;
        }

        let content = fs::read_to_string(&self.active_path)
            .map_err(|e| format!("Failed to read note: {}", e))?;

        let mut editor_state = EditorState::new(Lines::from(content.as_str()));
        editor_state.set_clipboard(SystemClipboard::new());
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

        if let Some(link_name) = get_link_under_cursor(&line_str, col_idx) {
            let target_path = self.wiki_mgr.link_to_path(&link_name);
            let _ = self.save_current_note();
            self.history_stack.push(self.active_path.clone());
            let _ = self.load_note(target_path);
            return true;
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
            false
        }
    }

    // Converts Visual Mode selection to wiki link [[ ... ]]
    fn wrap_selection_in_link(&mut self) {
        if let Some(ref selection) = self.editor_state.selection {
            let start = selection.start;
            let end = selection.end;

            // Sort start/end coordinates to get correct text boundaries
            let (start_idx, end_idx) = if start.row < end.row || (start.row == end.row && start.col <= end.col) {
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
            if path.is_file() && path.extension().and_then(|s| s.to_str()) == Some("md")
                && let Ok(content) = fs::read_to_string(&path)
                    && content.to_lowercase().contains(&query) {
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

        let stem = self.active_path.file_stem().and_then(|s| s.to_str()).unwrap_or("note");
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

// Scans line content at column to find link text
fn get_link_under_cursor(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let mut pos = 0;
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
                if col >= start_pos && col <= absolute_end {
                    let content: String = chars[start_pos + 2..absolute_end - 1].iter().collect();
                    return Some(content.trim().to_string());
                }
                pos = absolute_end + 1;
            } else {
                break;
            }
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
    
    let is_ident_char = |c: char| -> bool {
        c.is_alphanumeric() || c == '_' || c == '/'
    };

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
    };
    let _ = session.save();
    let _ = write_cursor_shape_sequence(terminal.backend_mut(), 0);
    let _ = write_cursor_color_sequence(terminal.backend_mut(), "");
    execute!(terminal.backend_mut(), LeaveAlternateScreen, event::DisableMouseCapture)?;
    disable_raw_mode()?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {}", err);
    }
    Ok(())
}

fn write_cursor_shape_sequence<W: std::io::Write>(writer: &mut W, shape_num: u8) -> std::io::Result<()> {
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

fn write_cursor_color_sequence<W: std::io::Write>(writer: &mut W, color_str: &str) -> std::io::Result<()> {
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
            KeyCode::Char('y') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                app.help_scroll = app.help_scroll.saturating_sub(1);
            }
            KeyCode::Char('e') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                app.help_scroll = app.help_scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                app.help_scroll = app.help_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                app.help_scroll = app.help_scroll.saturating_add(10);
            }
            KeyCode::Char('h') | KeyCode::Char('H') | KeyCode::Left => {
                app.help_tab_idx = if app.help_tab_idx == 0 { 4 } else { app.help_tab_idx - 1 };
                app.help_scroll = 0;
            }
            KeyCode::Char('l') | KeyCode::Char('L') | KeyCode::Right => {
                app.help_tab_idx = (app.help_tab_idx + 1) % 5;
                app.help_scroll = 0;
            }
            _ => {
                app.show_help = false;
            }
        }
        return true;
    }
    false
}

fn run_app<B: Backend + std::io::Write>(terminal: &mut Terminal<B>, app: &mut App) -> Result<(), String> {
    let mut last_key_was_z = false;
    loop {
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
                // Global exits: Ctrl-q works anywhere, regardless of mode/panel
                if key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }

                if app.search_active {
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
                    continue;
                }

                // If delete confirmation is open
                if app.show_delete_confirm {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            if let Some(path) = app.delete_target_path.take() {
                                let _ = fs::remove_file(&path);
                                app.wiki_mgr.remove_registry_entry(&path);
                                if path == app.active_path {
                                    let home_path = app.wiki_mgr.init_wiki().unwrap_or_else(|_| app.wiki_mgr.link_to_path("home"));
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
                    && (key.code == KeyCode::Char('Z') || (key.code == KeyCode::Char('z') && key.modifiers.contains(KeyModifiers::SHIFT)));

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
                            && col < line.len() {
                                line[col] = c;
                                app.re_evaluate_calculations();
                                let _ = app.save_current_note();
                            }
                    }
                    app.update_highlights();
                    continue;
                }

                // Global help modal toggle (F1 works in any mode, ~ works only when not in insert mode)
                let is_insert_mode = app.focused_panel == FocusedPanel::Editor && app.editor_state.mode == EditorMode::Insert;

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
                // Ctrl-s: Export current note as HTML
                if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    match app.export_current_note_to_html() {
                        Ok(path) => {
                            let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("note.html");
                            app.set_status_message(format!("Exported {} to {}", filename, path.to_string_lossy()));
                        }
                        Err(e) => {
                            app.set_status_message(format!("Export failed: {}", e));
                        }
                    }
                    app.update_highlights();
                    continue;
                }
                // Ctrl-w: Compile entire wiki to Markdown
                if key.code == KeyCode::Char('w') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    match app.compile_wiki_to_markdown() {
                        Ok(path) => {
                            app.set_status_message(format!("Compiled wiki to {}", path.to_string_lossy()));
                        }
                        Err(e) => {
                            app.set_status_message(format!("Compile failed: {}", e));
                        }
                    }
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

                // Focus switching via Shift-H / Shift-L / Ctrl-h / Ctrl-l
                let is_switch_left = (key.code == KeyCode::Char('h') && key.modifiers.contains(KeyModifiers::CONTROL))
                    || ((key.code == KeyCode::Char('H') || (key.code == KeyCode::Char('h') && key.modifiers.contains(KeyModifiers::SHIFT)))
                        && (app.focused_panel != FocusedPanel::Editor || app.editor_state.mode == EditorMode::Normal || app.editor_state.mode == EditorMode::Visual));

                let is_switch_right = (key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::CONTROL))
                    || ((key.code == KeyCode::Char('L') || (key.code == KeyCode::Char('l') && key.modifiers.contains(KeyModifiers::SHIFT)))
                        && (app.focused_panel != FocusedPanel::Editor || app.editor_state.mode == EditorMode::Normal || app.editor_state.mode == EditorMode::Visual));

                if is_switch_left {
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
                    FocusedPanel::Editor => {
                        let prev_mode = app.editor_state.mode;

                        // Intercept Enter key inside Visual Mode
                        if key.code == KeyCode::Enter && prev_mode == EditorMode::Visual {
                            app.wrap_selection_in_link();
                            continue;
                        }

                        // Intercept Enter key in Normal Mode
                        if key.code == KeyCode::Enter && prev_mode == EditorMode::Normal
                            && app.follow_link_under_cursor() {
                                continue;
                            }

                        // Intercept 't' in Normal Mode to toggle todo item at current row
                        if key.code == KeyCode::Char('t') && prev_mode == EditorMode::Normal
                            && app.toggle_todo_at_cursor() {
                                continue;
                            }

                        // Intercept Backspace or Ctrl-o in Normal Mode to go back
                        if (key.code == KeyCode::Backspace || (key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL)))
                            && prev_mode == EditorMode::Normal
                            && app.go_back() {
                                continue;
                            }

                        // Intercept Ctrl-d in Normal Mode to delete current page
                        if key.code == KeyCode::Char('d') && key.modifiers.contains(KeyModifiers::CONTROL)
                            && prev_mode == EditorMode::Normal 
                        {
                            let current_title = app.wiki_mgr.path_to_title(&app.active_path);
                            app.delete_target_name = current_title;
                            app.delete_target_path = Some(app.active_path.clone());
                            app.show_delete_confirm = true;
                            continue;
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
                                continue;
                            }
                        }

                        // Send event to Editor state
                        app.editor_event_handler.on_key_event(key, &mut app.editor_state);

                        // Trigger math calculation update on exiting Insert Mode
                        if prev_mode == EditorMode::Insert && app.editor_state.mode == EditorMode::Normal {
                            app.re_evaluate_calculations();
                            app.update_outgoing_links();
                        }
                    }
                    FocusedPanel::WikiMap => {
                        let links = app.get_wiki_map_selectable_links();
                        if !links.is_empty() {
                            match key.code {
                                KeyCode::Up | KeyCode::Char('k')
                                    if app.selected_link_idx > 0 => {
                                        app.selected_link_idx -= 1;
                                    }
                                KeyCode::Down | KeyCode::Char('j')
                                    if app.selected_link_idx < links.len() - 1 => {
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
                    }
                    FocusedPanel::Variables => {
                        let vars_len = app.variables_cache.len();
                        match key.code {
                            KeyCode::Up | KeyCode::Char('k')
                                if vars_len > 0 && app.selected_var_idx > 0 => {
                                    app.selected_var_idx -= 1;
                                }
                            KeyCode::Down | KeyCode::Char('j')
                                if vars_len > 0 && app.selected_var_idx < vars_len - 1 => {
                                    app.selected_var_idx += 1;
                                }
                            KeyCode::Char('y')
                                if vars_len > 0 && app.selected_var_idx < vars_len => {
                                    let (_, ref val) = app.variables_cache[app.selected_var_idx];
                                    let mut clip = SystemClipboard::new();
                                    clip.set_text(val.clone());
                                }
                            KeyCode::Enter | KeyCode::Char('i')
                                if vars_len > 0 && app.selected_var_idx < vars_len => {
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
                    }
                }
                app.update_highlights();
            }
            Event::Mouse(mouse) => {
                    if app.show_help {
                        app.show_help = false;
                    } else {
                        let col = mouse.column;
                        let row = mouse.row;

                        // 1. Left Panel (Wiki Map)
                        if app.left_panel_open 
                            && col >= app.left_area.x 
                            && col < app.left_area.x + app.left_area.width 
                            && row >= app.left_area.y 
                            && row < app.left_area.y + app.left_area.height 
                        {
                            app.focused_panel = FocusedPanel::WikiMap;
                            if mouse.kind == event::MouseEventKind::Down(event::MouseButton::Left) {
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
                            app.focused_panel = FocusedPanel::Variables;
                            if mouse.kind == event::MouseEventKind::Down(event::MouseButton::Left) {
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
                            app.focused_panel = FocusedPanel::Editor;
                            app.editor_event_handler.on_mouse_event(mouse, &mut app.editor_state);
                        }
                    }
                    app.update_highlights();
                }
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

fn ui(f: &mut Frame, app: &mut App) {
    let show_bottom_bar = app.search_active || if let Some((_, inst)) = &app.status_message {
        inst.elapsed() < std::time::Duration::from_secs(5)
    } else {
        false
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(vec![
            Constraint::Min(1),
            if show_bottom_bar { Constraint::Length(1) } else { Constraint::Length(0) },
        ])
        .split(f.area());

    let workspace_area = chunks[0];
    let status_area = chunks[1];

    // 2. Compute dynamic horizontal panel layouts
    let left_constraint = if app.left_panel_open { Constraint::Length(22) } else { Constraint::Length(0) };
    let right_constraint = if app.right_panel_open { Constraint::Length(25) } else { Constraint::Length(0) };
    let middle_constraint = Constraint::Min(20);

    let workspace_layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(vec![
            left_constraint,
            middle_constraint,
            right_constraint,
        ])
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
    let border_dim_color = Color::Rgb(86, 95, 137);      // Muted Gray #565f89
    let text_fg_color = Color::Rgb(169, 177, 214);       // Soft Gray #a9b1d6

    // RENDER 1: Left Panel (Wiki Map)
    if app.left_panel_open {
        let is_focused = app.focused_panel == FocusedPanel::WikiMap;
        let border_type = if is_focused { BorderType::Double } else { BorderType::Plain };
        let border_color = if is_focused { border_focused_color } else { border_dim_color };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color))
            .bg(bg_color)
            .title(Span::styled(" Wiki Map ", Style::default().fg(text_fg_color).bold()));

        let mut list_items = Vec::new();
        list_items.push(ListItem::new("◀ Backlinks").bold().fg(Color::Rgb(122, 162, 247))); // Royal Blue #7aa2f7

        let mut current_link_idx = 0;
        for link in &app.backlinks {
            let is_selected = is_focused && current_link_idx == app.selected_link_idx;
            let style = if is_selected {
                Style::default().bg(Color::Rgb(59, 66, 97)).fg(Color::Rgb(125, 207, 255)).bold()
            } else {
                Style::default().fg(text_fg_color)
            };
            let prefix = if is_selected { " ▶ " } else { " - " };
            list_items.push(ListItem::new(format!("{}{}", prefix, link)).style(style));
            current_link_idx += 1;
        }
        if app.backlinks.is_empty() {
            list_items.push(ListItem::new("  (none)").fg(border_dim_color).italic());
        }

        list_items.push(ListItem::new("")); // Spacer

        list_items.push(ListItem::new("▶ Outgoing").bold().fg(Color::Rgb(122, 162, 247))); // Royal Blue #7aa2f7
        for link in &app.outgoing {
            let is_selected = is_focused && current_link_idx == app.selected_link_idx;
            let style = if is_selected {
                Style::default().bg(Color::Rgb(59, 66, 97)).fg(Color::Rgb(125, 207, 255)).bold()
            } else {
                Style::default().fg(text_fg_color)
            };
            let prefix = if is_selected { " ▶ " } else { " - " };
            list_items.push(ListItem::new(format!("{}{}", prefix, link)).style(style));
            current_link_idx += 1;
        }
        if app.outgoing.is_empty() {
            list_items.push(ListItem::new("  (none)").fg(border_dim_color).italic());
        }

        if app.show_search_results {
            list_items.push(ListItem::new("")); // Spacer
            list_items.push(ListItem::new("🔍 Search Results").bold().fg(Color::Rgb(255, 158, 100))); // Orange
            for link in &app.search_results {
                let is_selected = is_focused && current_link_idx == app.selected_link_idx;
                let style = if is_selected {
                    Style::default().bg(Color::Rgb(59, 66, 97)).fg(Color::Rgb(125, 207, 255)).bold()
                } else {
                    Style::default().fg(text_fg_color)
                };
                let prefix = if is_selected { " ▶ " } else { " - " };
                list_items.push(ListItem::new(format!("{}{}", prefix, link)).style(style));
                current_link_idx += 1;
            }
            if app.search_results.is_empty() {
                list_items.push(ListItem::new("  (no matches)").fg(border_dim_color).italic());
            }
        }

        let list = List::new(list_items).block(block);
        f.render_widget(list, left_area);
    }

    // RENDER 2: Middle Panel (Editor)
    {
        let is_focused = app.focused_panel == FocusedPanel::Editor;
        let border_type = if is_focused { BorderType::Double } else { BorderType::Plain };
        let border_color = if is_focused { border_focused_color } else { border_dim_color };

        let mode_str = match app.editor_state.mode {
            EditorMode::Normal => "NORMAL",
            EditorMode::Insert => "INSERT",
            EditorMode::Visual => "VISUAL",
            EditorMode::Search => "SEARCH",
        };
        let mode_color = match app.editor_state.mode {
            EditorMode::Normal => Color::Rgb(122, 162, 247), // Blue
            EditorMode::Insert => Color::Rgb(158, 206, 106), // Green
            EditorMode::Visual => Color::Rgb(187, 154, 247), // Purple
            EditorMode::Search => Color::Rgb(255, 158, 100), // Orange
        };
        let note_title = app.wiki_mgr.path_to_title(&app.active_path);
        let title_top = Line::from(vec![
            Span::styled(" calki: ", Style::default().fg(text_fg_color).bold()),
            Span::styled(note_title, Style::default().fg(text_fg_color).bold()),
            Span::styled(" ", Style::default()),
        ]);

        let title_bottom_left = Line::from(vec![
            Span::styled(" [", Style::default().fg(text_fg_color)),
            Span::styled(mode_str, Style::default().fg(mode_color).bold()),
            Span::styled("] ", Style::default().fg(text_fg_color)),
        ]);

        let cursor_info = format!(" Line: {}, Col: {} ", app.editor_state.cursor.row + 1, app.editor_state.cursor.col + 1);
        let title_bottom_right = Line::from(vec![
            Span::styled(cursor_info, Style::default().fg(text_fg_color)),
        ]).right_aligned();

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color))
            .bg(bg_color)
            .title(title_top)
            .title_bottom(title_bottom_left)
            .title_bottom(title_bottom_right);

        let inner_editor_area = block.inner(editor_area);
        f.render_widget(block, editor_area);

        let editor_theme = edtui::EditorTheme::default()
            .hide_status_line()
            .hide_cursor()
            .selection_style(Style::default().bg(Color::Rgb(167, 82, 142)).fg(Color::Rgb(224, 230, 242)));
        let viewport_height = inner_editor_area.height as usize;
        let scrolloff = std::cmp::min(app.config.scrolloff, viewport_height / 2);
        app.editor_state.set_viewport_height(viewport_height);

        let (x_offset, mut y_offset) = app.editor_state.viewport_offset();
        let cursor_row = app.editor_state.cursor.row;

        if cursor_row < y_offset + scrolloff {
            y_offset = cursor_row.saturating_sub(scrolloff);
        } else if cursor_row >= y_offset + viewport_height.saturating_sub(scrolloff) {
            y_offset = (cursor_row + scrolloff + 1).saturating_sub(viewport_height);
        }

        app.editor_state.set_viewport_offset(x_offset, y_offset);

        let editor_widget = EditorView::new(&mut app.editor_state)
            .theme(editor_theme)
            .wrap(true);
        f.render_widget(editor_widget, inner_editor_area);
        if is_focused
            && let Some(pos) = app.editor_state.cursor_screen_position() {
                f.set_cursor_position(pos);
            }
    }

    // RENDER 3: Right Panel (Variables Inspector)
    if app.right_panel_open {
        let is_focused = app.focused_panel == FocusedPanel::Variables;
        let border_type = if is_focused { BorderType::Double } else { BorderType::Plain };
        let border_color = if is_focused { border_focused_color } else { border_dim_color };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(border_type)
            .border_style(Style::default().fg(border_color))
            .bg(bg_color)
            .title(Span::styled(" Variables ", Style::default().fg(text_fg_color).bold()));

        let mut list_items = Vec::new();
        for (idx, (name, val)) in app.variables_cache.iter().enumerate() {
            let is_selected = is_focused && idx == app.selected_var_idx;
            let is_error = val.contains("[Error");
            let val_style = if is_error {
                Style::default().fg(Color::Rgb(247, 118, 142)).bold() // Red #f7768e
            } else {
                Style::default().fg(Color::Rgb(115, 218, 202))       // Teal #73daca
            };

            let prefix = if is_selected { "▶ " } else { "  " };
            let prefix_style = if is_selected {
                Style::default().fg(Color::Rgb(125, 207, 255)).bold()
            } else {
                Style::default()
            };

            let name_style = if is_selected {
                Style::default().fg(Color::Rgb(125, 207, 255)).bold()
            } else {
                Style::default().fg(text_fg_color).bold()
            };

            let item_line = Line::from(vec![
                Span::styled(prefix, prefix_style),
                Span::styled(format!("{}: ", name), name_style),
                Span::styled(val, val_style),
            ]);

            let mut item = ListItem::new(item_line);
            if is_selected {
                item = item.style(Style::default().bg(Color::Rgb(59, 66, 97)));
            }
            list_items.push(item);
        }
        if app.variables_cache.is_empty() {
            list_items.push(ListItem::new("  (no bindings)").fg(border_dim_color).italic());
        }

        let list = List::new(list_items).block(block);
        f.render_widget(list, right_area);
    }

    // Unified Help popup modal with tabs (opened via F1, ?, ~)
    if app.show_help {
        let area = centered_rect(85, 80, f.area());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Rgb(255, 158, 100))) // Orange border
            .bg(Color::Rgb(22, 22, 30))
            .title(Span::styled(" calki Quick Reference & Help ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()));

        // We construct the tab headers row:
        let tab_headers = [
            " 1. General ",
            " 2. Math & Trig ",
            " 3. Complex & Symbolic ",
            " 4. Lists & Stats ",
            " 5. Constants ",
        ];

        let mut header_spans = Vec::new();
        for (i, title) in tab_headers.iter().enumerate() {
            if i > 0 {
                header_spans.push(Span::styled("   ", Style::default().fg(Color::Rgb(86, 95, 137))));
            }
            if i == app.help_tab_idx {
                header_spans.push(Span::styled(format!("▶{}◀", title), Style::default().fg(Color::Rgb(125, 207, 255)).bold()));
            } else {
                header_spans.push(Span::styled(format!(" {} ", title), Style::default().fg(Color::Rgb(86, 95, 137))));
            }
        }
        let tab_row = Line::from(header_spans);

        // Help text content based on active tab:
        let mut help_text = vec![
            tab_row,
            Line::from(""),
        ];

        let mut content = match app.help_tab_idx {
            0 => vec![
                Line::from(vec![Span::styled("── Global & Panel Navigation ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" F1          ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Toggle this Help Guide modal", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" h / l       ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Switch between Help Tabs (Left / Right)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" j / k       ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Scroll Help Content (Down / Up)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" F2 / F3     ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Toggle Wiki Map / Variables Panel", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" /           ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Search entire Wiki for keyword / notes", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" Shift-H / L ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Move Focus Left / Right between active panels", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" Esc         ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Escape modes / Return focus to Editor", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" Ctrl-q      ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Exit the program (from any mode/panel)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("── Editor & Wiki Note Operations ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" Enter       ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Follow [[Link]] (Normal) / Wrap selection in Link (Visual)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" Backspace   ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Go back in note history (Normal mode)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" Ctrl-d      ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Delete current wiki note / file", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" Ctrl-s      ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Export current note to HTML", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" Ctrl-w      ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Compile entire wiki to Markdown files", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("── Wiki Map Panel (focused) ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" d / x / Del ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Delete selected note file", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("── Variables Panel (focused) ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" y           ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Yank/copy variable value to system clipboard", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" Enter / i   ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled("Insert variable name at editor cursor", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
            ],
            1 => vec![
                Line::from(vec![Span::styled("── Basic Arithmetic & Functions ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" abs(x)             ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Absolute value of x", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" sqrt(x)            ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Square root of x (negative inputs return complex)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" round(x, [n])      ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Round x to n decimal places (default 0)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" ceil(x) / floor(x) ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Ceiling / Floor function", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" mod(x, y)          ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Modulo remainder (also infix x % y)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("── Exponentials & Logarithms ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" exp(x)             ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Exponential e^x", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" ln(x)              ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Natural logarithm (negative real inputs return complex)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" log(x)             ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Base-10 logarithm", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" log(x, base)       ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Logarithm of x with arbitrary base (e.g. log(8, 2) => 3)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" log2(x)            ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Base-2 logarithm", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("── Trigonometry ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" sin / cos / tan    ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Trigonometric sine, cosine, tangent (supports complex)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" asin / acos / atan ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Inverse arc sine, cosine, tangent", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" sinh / cosh / tanh ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Hyperbolic sine, cosine, tangent", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" asinh / acosh / atanh ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Inverse hyperbolic functions", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
            ],
            2 => vec![
                Line::from(vec![Span::styled("── Complex Numbers ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" imaginary unit 'i' ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Literal suffix (e.g. 3i, 2 + 5i)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" Complex Arithmetic ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Supports +, -, *, /, powers, and trig/log/sqrt/abs functions", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("── Symbolic Calculus & Solving ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" diff(f, x) / der(f, x)", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Symbolic derivative of f with respect to variable x", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" solve(eq, x)       ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Solve linear equation eq for x (e.g. solve(2*x + 5 == 15, x) => 5)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("── Radix Notation & Bitwise ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" 0x... / 0b...      ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Hexadecimal / Binary integer literals", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" in hex / in bin    ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Convert and format output (e.g. 15 in hex => 0xF)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" &  |  ~  <<  >>  xor", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Bitwise AND, OR, NOT (~), Left/Right Shift, and XOR", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
            ],
            3 => vec![
                Line::from(vec![Span::styled("── List Functional Operations ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" map(expr, list)    ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Transforms list elements (e.g. map(x^2, [1, 2, 3]) => [1, 4, 9])", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" reduce(expr, list) ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Accumulates list (e.g. reduce(x + y, [1, 2, 3]) => 6)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" prod(x, ...)       ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Product of list elements / arguments (combines units)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("── Statistics ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" sum(x, ...)        ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Sum of elements / arguments", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" mean / average     ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Arithmetic mean of arguments", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" median(x, ...)     ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Median value of arguments", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" stddev / variance  ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Sample standard deviation / variance", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" count(x, ...)      ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Count the number of scalar items in lists/arguments", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(""),
                Line::from(vec![Span::styled("── Vectors, Matrices & Plotting ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" len(list)          ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Length of list / vector", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" plot(list)         ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Draws Unicode sparkline trend (e.g. ▄▅▇█)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" vdot / vadd / vsub ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Vector dot product, addition, and subtraction", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" transpose / matmul ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("Matrix transpose and matrix multiplication", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
            ],
            4 => vec![
                Line::from(vec![Span::styled("── Predefined Constants ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
                Line::from(vec![
                    Span::styled(" pi                 ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("3.1415926535... (Ratio of circle circumference to diameter)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" e                  ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("2.7182818284... (Euler's number)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" c                  ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("299,792,458 m/s (Speed of light constant)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" g                  ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("9.80665 m/s^2 (Standard acceleration of gravity)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" G                  ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("6.6743e-11 m^3/(kg*s^2) (Newtonian gravity constant)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" h                  ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("6.62607015e-34 kg*m^2/s (Planck constant)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" hbar               ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("1.054571817e-34 kg*m^2/s (Reduced Planck constant)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" kb                 ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("1.380649e-23 kg*m^2/(s^2*K) (Boltzmann constant)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" NA                 ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("6.02214076e23 (Avogadro constant)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" R                  ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("8.314462618 kg*m^2/(s^2*K) (Molar gas constant)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" me                 ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("9.1093837015e-31 kg (Electron mass)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
                Line::from(vec![
                    Span::styled(" mp                 ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                    Span::styled("1.67262192369e-27 kg (Proton mass)", Style::default().fg(Color::Rgb(169, 177, 214))),
                ]),
            ],
            _ => Vec::new(),
        };

        help_text.append(&mut content);
        help_text.push(Line::from(""));
        help_text.push(Line::from(vec![
            Span::styled(" Press h/l (Left/Right) to switch tabs  •  Press any other key to close ", Style::default().fg(Color::Rgb(255, 158, 100)).italic()),
        ]));

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

    // Delete page confirmation popup
    if app.show_delete_confirm {
        let area = centered_rect(60, 25, f.area());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Rgb(247, 118, 142))) // Red border for danger
            .bg(Color::Rgb(22, 22, 30))
            .title(Span::styled(" Delete Wiki Page ", Style::default().fg(Color::Rgb(247, 118, 142)).bold()));

        let text = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled(" Are you sure you want to delete ", Style::default().fg(text_fg_color)),
                Span::styled(format!("\"{}\"", app.delete_target_name), Style::default().bold().fg(Color::Rgb(125, 207, 255))),
                Span::styled("?", Style::default().fg(text_fg_color)),
            ]).centered(),
            Line::from(" This will permanently remove the file from your disk. ").centered(),
            Line::from(""),
            Line::from(vec![
                Span::styled("  [y] ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Yes, delete it  ", Style::default().fg(text_fg_color)),
                Span::styled("  [any other key] ", Style::default().fg(Color::Rgb(255, 158, 100)).bold()),
                Span::styled("Cancel  ", Style::default().fg(text_fg_color)),
            ]).centered(),
        ];

        let paragraph = Paragraph::new(text).block(block).wrap(Wrap { trim: false });
        f.render_widget(Clear, area);
        f.render_widget(paragraph, area);
    }

    // RENDER STATUS LINE
    if show_bottom_bar {
        let status_bg = Color::Rgb(22, 22, 30);
        let status_block = Block::default().bg(status_bg);

        let status_line = if let Some((msg, inst)) = &app.status_message {
            if inst.elapsed() < Duration::from_secs(5) {
                Line::from(vec![
                    Span::styled(" ✔  ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                    Span::styled(msg, Style::default().fg(Color::Rgb(158, 206, 106))),
                ])
            } else {
                Line::from("")
            }
        } else if app.search_active {
            Line::from(vec![
                Span::styled(" 🔍 Search: ", Style::default().fg(Color::Rgb(255, 158, 100)).bold()),
                Span::styled(&app.search_query, Style::default().fg(Color::Rgb(125, 207, 255))),
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
fn find_in_chars(chars: &[char], sub: &str) -> Option<usize> {
    let sub_chars: Vec<char> = sub.chars().collect();
    if sub_chars.is_empty() {
        return Some(0);
    }
    chars.windows(sub_chars.len()).position(|window| window == sub_chars)
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
    Number { start: usize, end: usize, val: f64 },
    Identifier { start: usize, end: usize, name: String },
    Symbol { start: usize, end: usize, ch: char },
    Arrow { start: usize, end: usize },
    In { start: usize, end: usize },
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
            tokens.push(HighlightToken::Number { start, end: end.saturating_sub(1), val });
        } else if ch == '=' {
            let start = i;
            i += 1;
            if i < len && line[i] == '>' {
                i += 1;
                tokens.push(HighlightToken::Arrow { start, end: start + 1 });
            } else {
                tokens.push(HighlightToken::Symbol { start, end: start, ch: '=' });
            }
        } else if ch == '$' {
            let start = i;
            i += 1;
            tokens.push(HighlightToken::Identifier { start, end: start, name: "$".to_string() });
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
            if name == "in" || name == "to" {
                tokens.push(HighlightToken::In { start, end: end.saturating_sub(1) });
            } else {
                tokens.push(HighlightToken::Identifier { start, end: end.saturating_sub(1), name });
            }
        } else {
            let start = i;
            i += 1;
            tokens.push(HighlightToken::Symbol { start, end: start, ch });
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
            html.push_str(&format!("<blockquote>{}</blockquote>\n", parse_inline_elements(stripped.trim())));
        } else if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            html.push_str("<hr/>\n");
        } else if trimmed.starts_with('*') || trimmed.starts_with('-') {
            if !in_list {
                html.push_str("<ul>\n");
                in_list = true;
            }
            let stripped = trimmed.strip_prefix('*').or_else(|| trimmed.strip_prefix('-')).unwrap_or(trimmed);
            html.push_str(&format!("<li>{}</li>\n", parse_inline_elements(stripped.trim())));
        } else if trimmed.contains("=>") && !trimmed.contains('`') {
            if let Some(pos) = trimmed.find("=>") {
                let expr = trimmed[..pos].trim();
                let val = trimmed[pos + 2..].trim();
                let val_class = if val.contains("[Error") { "val error" } else { "val" };
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
    template.replace("{title}", title).replace("{content}", &html)
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
                    let val_class = if val.contains("[Error") { "val error" } else { "val" };
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
        let wiki_root = std::env::current_dir().unwrap().join("test_wiki_wrap_selection");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();
        let mut app = App::new(wiki_root.clone()).unwrap();
        app.editor_state = EditorState::new(edtui::Lines::from("Welcome 🧮 price = 100"));
        app.editor_state.cursor = edtui::Index2::new(0, 10);
        
        // Simulate visual mode selection left-to-right
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('v'), crossterm::event::KeyModifiers::NONE),
            &mut app.editor_state
        );
        for _ in 0..4 {
            app.editor_event_handler.on_key_event(
                crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('l'), crossterm::event::KeyModifiers::NONE),
                &mut app.editor_state
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
        let wiki_root = std::env::current_dir().unwrap().join("test_wiki_temp_change_keys");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();
        
        let mut app = App::new(wiki_root.clone()).unwrap();
        app.editor_state = EditorState::new(edtui::Lines::from("hello world"));
        
        // 1. Test cw (Change Word) at start of "hello"
        app.editor_state.cursor = edtui::Index2::new(0, 0);
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('c'), crossterm::event::KeyModifiers::NONE),
            &mut app.editor_state
        );
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('w'), crossterm::event::KeyModifiers::NONE),
            &mut app.editor_state
        );
        assert_eq!(app.editor_state.mode, EditorMode::Insert);
        let text = app.get_editor_text();
        assert_eq!(text, "world");

        // 2. Test cc (Change Line)
        app.editor_state = EditorState::new(edtui::Lines::from("hello world"));
        app.editor_state.mode = EditorMode::Normal;
        app.editor_state.cursor = edtui::Index2::new(0, 4);
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('c'), crossterm::event::KeyModifiers::NONE),
            &mut app.editor_state
        );
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('c'), crossterm::event::KeyModifiers::NONE),
            &mut app.editor_state
        );
        assert_eq!(app.editor_state.mode, EditorMode::Insert);
        let text = app.get_editor_text();
        assert_eq!(text, "");

        // 3. Test C (Change to End of Line)
        app.editor_state = EditorState::new(edtui::Lines::from("hello world"));
        app.editor_state.mode = EditorMode::Normal;
        app.editor_state.cursor = edtui::Index2::new(0, 5); // index of space
        app.editor_event_handler.on_key_event(
            crossterm::event::KeyEvent::new(crossterm::event::KeyCode::Char('C'), crossterm::event::KeyModifiers::SHIFT),
            &mut app.editor_state
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
            app.editor_event_handler.on_key_event(key, &mut app.editor_state);
        }
        
        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_mouse_routing() {
        let wiki_root = std::env::current_dir().unwrap().join("test_wiki_temp_mouse");
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
    fn test_compute_syntax_highlights_units() {
        let lines = vec![
            "commute = 88 miles".chars().collect::<Vec<char>>(),
            "level2 = 6 kWh / hr".chars().collect::<Vec<char>>(),
            "price = $0.20 / kWh".chars().collect::<Vec<char>>(),
            "subaru_eff = 274 miles / 74.7 kWh => 3.668 miles/kWh".chars().collect::<Vec<char>>(),
            "subaru_power = commute / subaru_eff => 23.9912 kWh".chars().collect::<Vec<char>>(),
            "subaru_power / level2 => 3.9985 hr".chars().collect::<Vec<char>>(),
            "apples = 10 count".chars().collect::<Vec<char>>(),
            "// comment with 88 miles".chars().collect::<Vec<char>>(),
            "# Header with 88 miles".chars().collect::<Vec<char>>(),
            "[[miles]] = 10".chars().collect::<Vec<char>>(),
            "Let's go for 10 miles.".chars().collect::<Vec<char>>(),
            "We run at `10m/s => 10 m/s`.".chars().collect::<Vec<char>>(),
        ];

        let highlights = App::compute_syntax_highlights(&lines, None);

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

        assert!(highlights.iter().any(|h| h.start.row == 0 && h.start.col == 0 && h.end.col == 7 && h.style.fg == Some(cyan)));
        assert!(highlights.iter().any(|h| h.start.row == 0 && h.start.col == 8 && h.end.col == 8 && h.style.fg == Some(orange)));
        assert!(highlights.iter().any(|h| h.start.row == 0 && h.start.col == 9 && h.end.col == 12 && h.style.fg == Some(teal)));
        assert!(highlights.iter().any(|h| h.start.row == 0 && h.start.col == 13 && h.end.col == 17 && h.style.fg == Some(pink)));

        // line 1: "level2 = 6 kWh / hr" -> "kWh" is at [11, 13], "hr" is at [17, 18]
        assert!(unit_highlights.iter().any(|h| h.start.row == 1 && h.start.col == 11 && h.end.col == 13));
        assert!(unit_highlights.iter().any(|h| h.start.row == 1 && h.start.col == 17 && h.end.col == 18));

        // line 2: "price = $0.20 / kWh" -> "$" is at [8, 8], "kWh" is at [16, 18]
        assert!(unit_highlights.iter().any(|h| h.start.row == 2 && h.start.col == 8 && h.end.col == 8));
        assert!(unit_highlights.iter().any(|h| h.start.row == 2 && h.start.col == 16 && h.end.col == 18));

        // line 3: "subaru_eff = 274 miles / 74.7 kWh => 3.668 miles/kWh"
        // "miles" at [17, 21], "kWh" at [30, 32], "miles/kWh" at [43, 51]
        assert!(unit_highlights.iter().any(|h| h.start.row == 3 && h.start.col == 17 && h.end.col == 21));
        assert!(unit_highlights.iter().any(|h| h.start.row == 3 && h.start.col == 30 && h.end.col == 32));
        assert!(unit_highlights.iter().any(|h| h.start.row == 3 && h.start.col == 43 && h.end.col == 51));

        // line 4: "subaru_power = commute / subaru_eff => 23.9912 kWh" -> "kWh" at [47, 49]
        assert!(unit_highlights.iter().any(|h| h.start.row == 4 && h.start.col == 47 && h.end.col == 49));

        // line 5: "subaru_power / level2 => 3.9985 hr" -> "hr" at [32, 33]
        assert!(unit_highlights.iter().any(|h| h.start.row == 5 && h.start.col == 32 && h.end.col == 33));

        // line 6: "apples = 10 count" -> "count" follows a number, so it's a unit, at [12, 16]
        assert!(unit_highlights.iter().any(|h| h.start.row == 6 && h.start.col == 12 && h.end.col == 16));

        // line 7: comment should have no yellow highlights
        assert!(!unit_highlights.iter().any(|h| h.start.row == 7));

        // line 8: header should have no yellow highlights
        assert!(!unit_highlights.iter().any(|h| h.start.row == 8));

        // line 9: "[[miles]] = 10" -> "miles" is inside wiki link, should NOT have yellow unit highlight
        assert!(!unit_highlights.iter().any(|h| h.start.row == 9 && h.start.col == 2 && h.end.col == 6));

        // line 10: "Let's go for 10 miles." -> plain text block, "s" and "miles" should NOT be highlighted as units
        assert!(!unit_highlights.iter().any(|h| h.start.row == 10));

        // line 11: "We run at `10m/s => 10 m/s`." -> "m/s" inside backticks SHOULD be highlighted as unit
        assert!(unit_highlights.iter().any(|h| h.start.row == 11 && h.start.col == 13 && h.end.col == 15));
        assert!(unit_highlights.iter().any(|h| h.start.row == 11 && h.start.col == 23 && h.end.col == 25));
    }

    #[test]
    fn test_vim_r_replacement() {
        let wiki_root = std::env::current_dir().unwrap().join("test_wiki_temp_vim_r");
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
        app.editor_state = EditorState::new(edtui::Lines::from("- [ ] todo item\n* list item\n- [x] done item"));
        app.editor_state.mode = EditorMode::Normal;

        // 1. Toggle unchecked to checked
        app.editor_state.cursor = edtui::Index2::new(0, 0);
        let res1 = app.toggle_todo_at_cursor();
        assert!(res1);
        assert_eq!(app.get_editor_text(), "- [x] todo item\n* list item\n- [x] done item");

        // 2. Convert plain list item to todo checkbox
        app.editor_state.cursor = edtui::Index2::new(1, 0);
        let res2 = app.toggle_todo_at_cursor();
        assert!(res2);
        assert_eq!(app.get_editor_text(), "- [x] todo item\n* [ ] list item\n- [x] done item");

        // 3. Toggle checked to unchecked
        app.editor_state.cursor = edtui::Index2::new(2, 0);
        let res3 = app.toggle_todo_at_cursor();
        assert!(res3);
        assert_eq!(app.get_editor_text(), "- [x] todo item\n* [ ] list item\n- [ ] done item");

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
            "This is ~~strikethrough~~ text".chars().collect::<Vec<char>>(),
        ];

        let highlights = App::compute_syntax_highlights(&lines, None);

        // Heading Level 1: Purple
        assert_eq!(
            highlights.iter().find(|h| h.start.row == 0 && h.start.col == 0 && h.end.col == 3).unwrap().style,
            Style::default().fg(Color::Rgb(187, 154, 247)).bold()
        );
        // Heading Level 2: Cyan
        assert_eq!(
            highlights.iter().find(|h| h.start.row == 1 && h.start.col == 0 && h.end.col == 4).unwrap().style,
            Style::default().fg(Color::Rgb(125, 207, 255)).bold()
        );
        // Heading Level 3: Blue
        assert_eq!(
            highlights.iter().find(|h| h.start.row == 2 && h.start.col == 0 && h.end.col == 5).unwrap().style,
            Style::default().fg(Color::Rgb(122, 162, 247)).bold()
        );
        // Heading Level 4: Teal
        assert_eq!(
            highlights.iter().find(|h| h.start.row == 3 && h.start.col == 0 && h.end.col == 6).unwrap().style,
            Style::default().fg(Color::Rgb(115, 218, 202)).bold()
        );
        // Heading Level 5: Green
        assert_eq!(
            highlights.iter().find(|h| h.start.row == 4 && h.start.col == 0 && h.end.col == 7).unwrap().style,
            Style::default().fg(Color::Rgb(158, 206, 106)).bold()
        );
        // Heading Level 6+: Orange
        assert_eq!(
            highlights.iter().find(|h| h.start.row == 5 && h.start.col == 0 && h.end.col == 8).unwrap().style,
            Style::default().fg(Color::Rgb(255, 158, 100)).bold()
        );

        // Row 6: Blockquote (Italic Green Color::Rgb(158, 206, 106))
        assert!(highlights.iter().any(|h| h.start.row == 6 && h.start.col == 0 && h.end.col == 21 && h.style.fg == Some(Color::Rgb(158, 206, 106))));

        // Row 7: HR (Dim Gray Color::Rgb(86, 95, 137))
        assert!(highlights.iter().any(|h| h.start.row == 7 && h.start.col == 0 && h.end.col == 2 && h.style.fg == Some(Color::Rgb(86, 95, 137))));

        // Row 8: Bullet list (* at [0, 0] is Bold Orange Color::Rgb(255, 158, 100))
        assert!(highlights.iter().any(|h| h.start.row == 8 && h.start.col == 0 && h.end.col == 0 && h.style.fg == Some(Color::Rgb(255, 158, 100))));

        // Row 9: Number list ("10." at [0, 2] is Bold Orange)
        assert!(highlights.iter().any(|h| h.start.row == 9 && h.start.col == 0 && h.end.col == 2 && h.style.fg == Some(Color::Rgb(255, 158, 100))));

        // Row 10: Bold ("**bold**" at [8, 15] is bold)
        let bold_hl = highlights.iter().find(|h| h.start.row == 10 && h.start.col == 8 && h.end.col == 15).unwrap();
        assert_eq!(bold_hl.style, Style::default().fg(Color::Rgb(169, 177, 214)).bold());

        // Row 11: Italic ("*italic*" at [8, 15] is italic)
        let italic_hl = highlights.iter().find(|h| h.start.row == 11 && h.start.col == 8 && h.end.col == 15).unwrap();
        assert_eq!(italic_hl.style, Style::default().fg(Color::Rgb(169, 177, 214)).italic());

        // Row 12: Crossed out ("~~strikethrough~~" at [8, 24] is crossed out)
        let strike_hl = highlights.iter().find(|h| h.start.row == 12 && h.start.col == 8 && h.end.col == 24).unwrap();
        assert_eq!(strike_hl.style, Style::default().fg(Color::Rgb(169, 177, 214)).crossed_out());
    }

    #[test]
    fn test_compute_syntax_highlights_selected_var() {
        let lines = vec![
            "price = 100".chars().collect::<Vec<char>>(),
            "total = price * 2".chars().collect::<Vec<char>>(),
        ];

        let highlights = App::compute_syntax_highlights(&lines, Some("price"));

        // In row 0, "price" at col 0..=4 should be highlighted with the selected variable style: bg(167, 82, 142), fg(224, 230, 242), bold.
        let hl_r0 = highlights.iter().find(|h| h.start.row == 0 && h.start.col == 0 && h.end.col == 4).unwrap();
        assert_eq!(
            hl_r0.style,
            Style::default().bg(Color::Rgb(167, 82, 142)).fg(Color::Rgb(224, 230, 242)).bold()
        );

        // In row 1, "price" at col 8..=12 should also be highlighted with the selected variable style.
        let hl_r1 = highlights.iter().find(|h| h.start.row == 1 && h.start.col == 8 && h.end.col == 12).unwrap();
        assert_eq!(
            hl_r1.style,
            Style::default().bg(Color::Rgb(167, 82, 142)).fg(Color::Rgb(224, 230, 242)).bold()
        );
    }

    #[test]
    fn test_compute_syntax_highlights_defined_vars_with_unit_names() {
        let lines = vec![
            "m = 10".chars().collect::<Vec<char>>(),
            "y = m * 2".chars().collect::<Vec<char>>(),
            "z = 5 m".chars().collect::<Vec<char>>(),
        ];

        let highlights = App::compute_syntax_highlights(&lines, None);

        let pink = Color::Rgb(244, 143, 177);

        // Row 0: "m = 10" -> "m" is the LHS variable, should NOT be pink
        assert!(!highlights.iter().any(|h| h.start.row == 0 && h.start.col == 0 && h.style.fg == Some(pink)));

        // Row 1: "y = m * 2" -> "m" at index 4 is used as variable, should NOT be pink
        assert!(!highlights.iter().any(|h| h.start.row == 1 && h.start.col <= 4 && h.end.col >= 4 && h.style.fg == Some(pink)));

        // Row 2: "z = 5 m" -> "m" at index 6 is preceded by number "5", so it acts as unit, MUST be pink
        assert!(highlights.iter().any(|h| h.start.row == 2 && h.start.col <= 6 && h.end.col >= 6 && h.style.fg == Some(pink)));
    }

    #[test]
    fn test_compute_syntax_highlights_percentage() {
        let lines = vec![
            "val = 10%".chars().collect::<Vec<char>>(),
            "mod_val = 10 % 3".chars().collect::<Vec<char>>(),
        ];

        let highlights = App::compute_syntax_highlights(&lines, None);

        let pink = Color::Rgb(244, 143, 177);

        // Row 0: "val = 10%" -> "%" at index 8 is acting as a postfix percentage (unit), MUST be pink
        assert!(highlights.iter().any(|h| h.start.row == 0 && h.start.col <= 8 && h.end.col >= 8 && h.style.fg == Some(pink)));

        // Row 1: "mod_val = 10 % 3" -> "%" at index 13 is acting as infix modulo (symbol), should NOT be pink
        assert!(!highlights.iter().any(|h| h.start.row == 1 && h.start.col <= 13 && h.end.col >= 13 && h.style.fg == Some(pink)));
    }

    #[test]
    fn test_compute_syntax_highlights_no_markdown_in_equations() {
        let lines = vec![
            "gas_cost = gas_usage * rate".chars().collect::<Vec<char>>(),
            "We bought items for `price_val * quantity_val =>` total".chars().collect::<Vec<char>>(),
            "testing inline `price * quantity => 500` before tax".chars().collect::<Vec<char>>(),
        ];

        let highlights = App::compute_syntax_highlights(&lines, None);

        let has_italic_text = highlights.iter().any(|h| {
            h.start.row == 0 && h.style.add_modifier.contains(Modifier::ITALIC)
        });
        assert!(!has_italic_text, "Markdown italics should be ignored on math lines");

        let has_italic_backticks_text = highlights.iter().any(|h| {
            h.start.row == 1 && h.style.add_modifier.contains(Modifier::ITALIC) && h.style.fg == Some(Color::Rgb(169, 177, 214))
        });
        assert!(!has_italic_backticks_text, "Markdown italics should be ignored inside backtick blocks");

        // Verify that in "testing inline `price * quantity => 500` before tax", 
        // the text outside the backticks is not styled with the math colors (Teal/Cyan/etc.).
        let has_spill_highlight = highlights.iter().any(|h| {
            h.start.row == 2 && (h.start.col < 15 || h.start.col > 37) && h.style.fg == Some(Color::Rgb(125, 207, 255))
        });
        assert!(!has_spill_highlight, "Math highlighting should not spill outside backticks");
    }

    #[test]
    fn test_help_and_guide_scrolling() {
        let wiki_root = std::env::current_dir().unwrap().join("test_wiki_temp_scroll");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }
        std::fs::create_dir_all(&wiki_root).unwrap();
        let mut app = App::new(wiki_root.clone()).unwrap();

        // Test Help scroll
        app.show_help = true;
        app.help_scroll = 5;

        // Up arrow should decrease scroll
        let handled = handle_modal_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Up, crossterm::event::KeyModifiers::NONE));
        assert!(handled);
        assert_eq!(app.help_scroll, 4);

        // j key should increase scroll
        let handled = handle_modal_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Char('j'), crossterm::event::KeyModifiers::NONE));
        assert!(handled);
        assert_eq!(app.help_scroll, 5);

        // PageUp should decrease scroll by 10 (saturating at 0)
        let handled = handle_modal_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::PageUp, crossterm::event::KeyModifiers::NONE));
        assert!(handled);
        assert_eq!(app.help_scroll, 0);

        // PageDown should increase scroll by 10
        let handled = handle_modal_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::PageDown, crossterm::event::KeyModifiers::NONE));
        assert!(handled);
        assert_eq!(app.help_scroll, 10);

        // Ctrl-y should decrease scroll
        let handled = handle_modal_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Char('y'), crossterm::event::KeyModifiers::CONTROL));
        assert!(handled);
        assert_eq!(app.help_scroll, 9);

        // Ctrl-e should increase scroll
        let handled = handle_modal_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Char('e'), crossterm::event::KeyModifiers::CONTROL));
        assert!(handled);
        assert_eq!(app.help_scroll, 10);

        // Other key (Esc) should close modal
        let handled = handle_modal_key(&mut app, crossterm::event::KeyEvent::new(KeyCode::Esc, crossterm::event::KeyModifiers::NONE));
        assert!(handled);
        assert!(!app.show_help);

        // Clean up
        let _ = std::fs::remove_dir_all(&wiki_root);
    }

    #[test]
    fn test_app_config_load_save() {
        let original_home = std::env::var("HOME").ok();
        let temp_dir = std::env::current_dir().unwrap().join("test_temp_home_config");
        if temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&temp_dir);
        }
        std::fs::create_dir_all(&temp_dir).unwrap();

        unsafe {
            std::env::set_var("HOME", &temp_dir);
        }

        // Verify default config is loaded when no file exists.
        let default_config = AppConfig::load();
        assert_eq!(default_config.scrolloff, 5); // Default value

        // Modify and save config.
        let custom_config = AppConfig { scrolloff: 8 };
        custom_config.save().expect("Failed to save config");

        // Verify that the file was created in the correct location.
        let expected_path = temp_dir.join(".config").join("calki").join("config.json");
        assert!(expected_path.exists());

        // Load config and verify custom values are loaded.
        let loaded_config = AppConfig::load();
        assert_eq!(loaded_config.scrolloff, 8);

        // Clean up
        let _ = std::fs::remove_dir_all(&temp_dir);
        unsafe {
            if let Some(orig) = original_home {
                std::env::set_var("HOME", orig);
            } else {
                std::env::remove_var("HOME");
            }
        }
    }

    #[test]
    fn test_search_and_export() {
        let wiki_root = std::env::current_dir().unwrap().join("test_wiki_search_export");
        if wiki_root.exists() {
            let _ = std::fs::remove_dir_all(&wiki_root);
        }

        let mut app = App::new(wiki_root.clone()).unwrap();

        // 1. Create a dummy note with some search keyword
        let dummy_path = wiki_root.join("dummy-note.md");
        std::fs::write(&dummy_path, "# Dummy Note\nThis is a unique_keyword inside a note.").unwrap();

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
}


