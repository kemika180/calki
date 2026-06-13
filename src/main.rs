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

use edtui::{EditorEventHandler, EditorMode, EditorState, EditorView, Lines};
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
    let mut result = String::with_capacity((input.len() + 2) / 3 * 4);
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
        if let Some(ref mut clip) = self.arboard_clip {
            if let Ok(txt) = clip.get_text() {
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
            show_delete_confirm: false,
            delete_target_name: String::new(),
            delete_target_path: None,
            exchange_rates: rates_cache.rates,
            left_area: Rect::default(),
            editor_area: Rect::default(),
            right_area: Rect::default(),
            replace_next_char: false,
        };

        if let Some(ref s) = session {
            let vecs = app.editor_state.lines.clone().into_vecs();
            let row_count = vecs.len();
            if row_count > 0 {
                let target_row = s.cursor_row.min(row_count - 1);
                let col_count = vecs[target_row].len();
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
        let vecs = self.editor_state.lines.clone().into_vecs();
        vecs.iter()
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

            let vecs = self.editor_state.lines.clone().into_vecs();
            let row_len = vecs.get(target_row).map(|r| r.len()).unwrap_or(0);
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

fn compute_syntax_highlights(lines_vecs: &Vec<Vec<char>>) -> Vec<edtui::Highlight> {
    let mut highlights = Vec::new();

    for (row_idx, line) in lines_vecs.iter().enumerate() {
        let line_str: String = line.iter().collect();

        // 1. Markdown Headers (lines starting with '#' followed by space or more '#')
        if line_str.starts_with('#') {
            let header_len = line_str.chars().take_while(|&c| c == '#').count();
            if line_str.chars().nth(header_len) == Some(' ') || line_str.len() == header_len {
                highlights.push(edtui::Highlight {
                    start: edtui::Index2::new(row_idx, 0),
                    end: edtui::Index2::new(row_idx, line.len().saturating_sub(1)),
                    style: Style::default().fg(Color::Rgb(122, 162, 247)).bold(), // Blue
                });
                continue;
            }
        }

        // 3. Comments
        if line_str.trim_start().starts_with("//") {
            let start_col = line_str.len() - line_str.trim_start().len();
            highlights.push(edtui::Highlight {
                start: edtui::Index2::new(row_idx, start_col),
                end: edtui::Index2::new(row_idx, line.len().saturating_sub(1)),
                style: Style::default().fg(Color::Rgb(86, 95, 137)).italic(), // Muted Gray-Blue
            });
            continue;
        }

        let n = line.len();
        let mut line_styles: Vec<Option<Style>> = vec![None; n];

        // A. Base Block Math & Assignments (containing '=>' or '=')
        if let Some(arrow_idx) = find_in_chars(line, "=>") {
            // Expression before '=>' (Cyan/light blue)
            for col in 0..arrow_idx {
                line_styles[col] = Some(Style::default().fg(Color::Rgb(125, 207, 255)));
            }
            // Operator '=>' in Bold Orange
            for col in arrow_idx..std::cmp::min(arrow_idx + 2, n) {
                line_styles[col] = Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
            }
            // The result after '=>' (Teal Green)
            for col in (arrow_idx + 2)..n {
                line_styles[col] = Some(Style::default().fg(Color::Rgb(115, 218, 202)).italic());
            }
        } else if let Some(eq_idx) = find_in_chars(line, "=") {
            let lhs = &line[..eq_idx];
            let lhs_str: String = lhs.iter().collect();
            let lhs_trimmed = lhs_str.trim();
            let is_lhs_valid = !lhs_trimmed.is_empty() 
                && lhs_trimmed.chars().all(|c| c.is_alphanumeric() || c == '_');
            
            if is_lhs_valid {
                // LHS (Cyan)
                for col in 0..eq_idx {
                    line_styles[col] = Some(Style::default().fg(Color::Rgb(125, 207, 255)));
                }
                // '=' (Bold Orange)
                if eq_idx < n {
                    line_styles[eq_idx] = Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
                }
                // RHS (Teal Green)
                for col in (eq_idx + 1)..n {
                    line_styles[col] = Some(Style::default().fg(Color::Rgb(115, 218, 202)));
                }
            }
        }

        // B. Inline code blocks/math in backticks: `expression => result`
        let mut b_idx = 0;
        while let Some(start_pos) = find_in_chars_from(line, "`", b_idx) {
            if let Some(end_pos) = find_in_chars_from(line, "`", start_pos + 1) {
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
                b_idx = end_pos + 1;
            } else {
                break;
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

        // D. Scan for units and highlight them in Tokyo Night Yellow (#e0af68)
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
                } else if i > 0 {
                    if let HighlightToken::Number { .. } = tokens[i - 1] {
                        is_unit = true;
                    }
                }

                if is_unit {
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
        let vecs = self.editor_state.lines.clone().into_vecs();
        let mut highlights = Self::compute_syntax_highlights(&vecs);

        if self.focused_panel == FocusedPanel::Variables && !self.variables_cache.is_empty() {
            if self.selected_var_idx >= self.variables_cache.len() {
                self.selected_var_idx = self.variables_cache.len().saturating_sub(1);
            }
            let (ref name, _) = self.variables_cache[self.selected_var_idx];
            let mut var_highlights = find_word_occurrences(&vecs, name);
            highlights.append(&mut var_highlights);
        }

        self.editor_state.highlights = highlights;
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

        let vecs = self.editor_state.lines.clone().into_vecs();
        let row_chars = match vecs.get(row_idx) {
            Some(row) => row,
            None => return false,
        };
        let line_str: String = row_chars.iter().collect();

        if let Some(link_name) = get_link_under_cursor(&line_str, col_idx) {
            let target_path = self.wiki_mgr.link_to_path(&link_name);
            let _ = self.save_current_note();
            self.history_stack.push(self.active_path.clone());
            let _ = self.load_note(target_path);
            return true;
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
                self.update_wiki_map();
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
            self.update_wiki_map();
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
        links
    }
}

// Maps Index2 row/col to 1D character offset in String
fn index2_to_char_offset(lines: &Lines, idx: edtui::Index2) -> usize {
    let vecs = lines.clone().into_vecs();
    let mut offset = 0;
    for (r, row) in vecs.iter().enumerate() {
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
fn find_word_occurrences(lines_vecs: &Vec<Vec<char>>, word: &str) -> Vec<edtui::Highlight> {
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

fn run_app<B: Backend + std::io::Write>(terminal: &mut Terminal<B>, app: &mut App) -> Result<(), String> {
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
                // If delete confirmation is open
                if app.show_delete_confirm {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Char('Y') => {
                            if let Some(path) = app.delete_target_path.take() {
                                let _ = fs::remove_file(&path);
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

                // If help modal is open, any key press closes it
                if app.show_help {
                    app.show_help = false;
                    continue;
                }

                // Intercept character for Vim 'r' replacement
                if app.replace_next_char {
                    app.replace_next_char = false;
                    if let KeyCode::Char(c) = key.code {
                        let row = app.editor_state.cursor.row;
                        let col = app.editor_state.cursor.col;
                        let mut vecs = app.editor_state.lines.clone().into_vecs();
                        if let Some(line) = vecs.get_mut(row) {
                            if col < line.len() {
                                line[col] = c;
                                let new_text: String = vecs.iter()
                                    .map(|l| l.iter().collect::<String>())
                                    .collect::<Vec<String>>()
                                    .join("\n");
                                app.editor_state.lines = Lines::from(new_text.as_str());
                                app.re_evaluate_calculations();
                                let _ = app.save_current_note();
                            }
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
                if key.code == KeyCode::F(1) || (key.code == KeyCode::Char('~') && !is_insert_mode) {
                    app.show_help = !app.show_help;
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

                // Global exits
                if app.focused_panel == FocusedPanel::Editor 
                    && app.editor_state.mode == EditorMode::Normal
                    && (key.code == KeyCode::Char('q') && key.modifiers.contains(KeyModifiers::CONTROL)) {
                        break;
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
                        if key.code == KeyCode::Enter && prev_mode == EditorMode::Normal {
                            if app.follow_link_under_cursor() {
                                continue;
                            }
                        }

                        // Intercept Backspace or Ctrl-o in Normal Mode to go back
                        if (key.code == KeyCode::Backspace || (key.code == KeyCode::Char('o') && key.modifiers.contains(KeyModifiers::CONTROL)))
                            && prev_mode == EditorMode::Normal 
                        {
                            if app.go_back() {
                                continue;
                            }
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
                            app.update_wiki_map();
                        }
                    }
                    FocusedPanel::WikiMap => {
                        let links = app.get_wiki_map_selectable_links();
                        if !links.is_empty() {
                            match key.code {
                                KeyCode::Up | KeyCode::Char('k') => {
                                    if app.selected_link_idx > 0 {
                                        app.selected_link_idx -= 1;
                                    }
                                }
                                KeyCode::Down | KeyCode::Char('j') => {
                                    if app.selected_link_idx < links.len() - 1 {
                                        app.selected_link_idx += 1;
                                    }
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
                            KeyCode::Up | KeyCode::Char('k') => {
                                if vars_len > 0 && app.selected_var_idx > 0 {
                                    app.selected_var_idx -= 1;
                                }
                            }
                            KeyCode::Down | KeyCode::Char('j') => {
                                if vars_len > 0 && app.selected_var_idx < vars_len - 1 {
                                    app.selected_var_idx += 1;
                                }
                            }
                            KeyCode::Char('y') => {
                                if vars_len > 0 && app.selected_var_idx < vars_len {
                                    let (_, ref val) = app.variables_cache[app.selected_var_idx];
                                    let mut clip = SystemClipboard::new();
                                    clip.set_text(val.clone());
                                }
                            }
                            KeyCode::Enter | KeyCode::Char('i') => {
                                if vars_len > 0 && app.selected_var_idx < vars_len {
                                    let name = app.variables_cache[app.selected_var_idx].0.clone();
                                    app.insert_text_at_cursor(&name);
                                    app.focused_panel = FocusedPanel::Editor;
                                }
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
                                if click_row > 0 {
                                    let backlinks_len = app.backlinks.len();
                                    let mut selected = None;
                                    if (click_row as usize) <= backlinks_len {
                                        selected = Some((click_row - 1) as usize);
                                    } else {
                                        let outgoing_start_row = backlinks_len + 2;
                                        let click_idx = (click_row as usize).saturating_sub(outgoing_start_row);
                                        if click_idx < app.outgoing.len() {
                                            selected = Some(backlinks_len + click_idx);
                                        }
                                    }
                                    if let Some(idx) = selected {
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
    let workspace_area = f.area();

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
        let editor_widget = EditorView::new(&mut app.editor_state).theme(editor_theme);
        f.render_widget(editor_widget, inner_editor_area);
        if is_focused {
            if let Some(pos) = app.editor_state.cursor_screen_position() {
                f.set_cursor_position(pos);
            }
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

    // Help popup modal (opened via ~)
    if app.show_help {
        let area = centered_rect(70, 75, f.area());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Rgb(255, 158, 100))) // Orange border
            .bg(Color::Rgb(22, 22, 30)) // Darker bg for popups
            .title(Span::styled(" Keyboard Shortcuts & Help ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()));

        let help_text = vec![
            Line::from(vec![
                Span::styled("calki Shortcuts Cheat Sheet", Style::default().bold().fg(Color::Rgb(125, 207, 255))),
            ]),
            Line::from(""),
            
            Line::from(vec![Span::styled("── Global & Navigation ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" F1 / ~      ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Toggle this Help Menu", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" Esc         ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Exit Help / Escape modes / Return focus to Editor", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" Shift-H / L ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Move Focus Left / Right between active panels", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" F2 / F3     ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Toggle Left Wiki Map / Right Variables Panel", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),

            Line::from(vec![Span::styled("── Editor & Wiki Navigation ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" Enter       ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Follow [[Link]] (Normal) / Wrap selection (Visual)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" Backspace   ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Go back in note history (Normal)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" Ctrl-d      ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Delete current note/file (Normal)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),

            Line::from(vec![Span::styled("── Wiki Map Panel (when focused) ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" d/x/Delete  ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Delete selected note/file", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),

            Line::from(vec![Span::styled("── Variables Panel (when focused) ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" y           ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Yank/copy variable value to clipboard", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" Enter / i   ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Insert variable name at editor cursor", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(" Press any key to close this help window ", Style::default().fg(Color::Rgb(255, 158, 100)).italic()),
            ]),
        ];

        let paragraph = Paragraph::new(help_text).block(block).wrap(Wrap { trim: false });
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
    if get_unit_info(word).is_some() || word == "$" {
        return true;
    }
    // Check compound unit: e.g. miles/kWh or kWh/hr or $/kWh or miles*day
    let parts: Vec<&str> = word.split(|c| c == '/' || c == '*').collect();
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
        assert_eq!(deserialized.left_panel_open, false);
        assert_eq!(deserialized.right_panel_open, true);

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
        ];

        let highlights = App::compute_syntax_highlights(&lines);

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
        let mut vecs = app.editor_state.lines.clone().into_vecs();
        vecs[row][col] = 'x';
        let new_text: String = vecs.iter()
            .map(|l| l.iter().collect::<String>())
            .collect::<Vec<String>>()
            .join("\n");
        app.editor_state.lines = Lines::from(new_text.as_str());
        app.replace_next_char = false;

        let text = app.get_editor_text();
        assert_eq!(text, "hello xorld");

        let _ = std::fs::remove_dir_all(&wiki_root);
    }
}

