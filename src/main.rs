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
            if path.exists() {
                if let Ok(content) = fs::read_to_string(path) {
                    if let Ok(config) = serde_json::from_str::<AppConfig>(&content) {
                        return config;
                    }
                }
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
    show_function_guide: bool, // Whether to display the function guide modal
    help_scroll: u16,
    function_guide_scroll: u16,
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
            show_function_guide: false,
            help_scroll: 0,
            function_guide_scroll: 0,
            show_delete_confirm: false,
            delete_target_name: String::new(),
            delete_target_path: None,
            exchange_rates: rates_cache.rates,
            left_area: Rect::default(),
            editor_area: Rect::default(),
            right_area: Rect::default(),
            replace_next_char: false,
            config,
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

fn compute_syntax_highlights(lines_vecs: &Vec<Vec<char>>, selected_var: Option<&str>) -> Vec<edtui::Highlight> {
    let mut highlights = Vec::new();

    for (row_idx, line) in lines_vecs.iter().enumerate() {
        let line_str: String = line.iter().collect();
        let n = line.len();
        let mut line_styles: Vec<Option<Style>> = vec![None; n];
        let mut is_special_line = false;

        // 1. Markdown Headers (lines starting with '#' followed by space or more '#')
        if line_str.starts_with('#') {
            let header_len = line_str.chars().take_while(|&c| c == '#').count();
            if line_str.chars().nth(header_len) == Some(' ') || line_str.len() == header_len {
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
        if !is_special_line && line_str.trim_start().starts_with('>') {
            let start_col = line_str.len() - line_str.trim_start().len();
            let quote_style = Style::default().fg(Color::Rgb(158, 206, 106)).italic(); // Italic Green #9ece6a
            for col in start_col..n {
                line_styles[col] = Some(quote_style);
            }
            is_special_line = true;
        }

        // 1c. Horizontal Rule
        let trimmed = line_str.trim();
        if !is_special_line && (trimmed == "---" || trimmed == "***" || trimmed == "___") && line_str.len() >= 3 {
            let hr_style = Style::default().fg(Color::Rgb(86, 95, 137)).dim(); // Muted Gray dim
            for col in 0..n {
                line_styles[col] = Some(hr_style);
            }
            is_special_line = true;
        }

        // 3. Comments
        if !is_special_line && line_str.trim_start().starts_with("//") {
            let start_col = line_str.len() - line_str.trim_start().len();
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
                let lhs_str: String = lhs.iter().collect();
                let lhs_trimmed = lhs_str.trim();
                let is_lhs_valid = !lhs_trimmed.is_empty() 
                    && lhs_trimmed.chars().all(|c| c.is_alphanumeric() || c == '_');
                
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
                } else if i > 0 {
                    if let HighlightToken::Number { .. } = tokens[i - 1] {
                        is_unit = true;
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
            }
        }

        // E. Lists / Bullet points (style bullet or number in bold orange)
        let trimmed_len = line_str.trim_start().len();
        let leading_spaces = line_str.len() - trimmed_len;
        let rest = &line_str[leading_spaces..];
        let mut list_marker_range = None;
        if rest.starts_with("* ") || rest.starts_with("- ") || rest.starts_with("+ ") {
            list_marker_range = Some(leading_spaces..leading_spaces + 1);
        } else {
            let digit_count = rest.chars().take_while(|c| c.is_ascii_digit()).count();
            if digit_count > 0 && rest.chars().nth(digit_count) == Some('.') && rest.chars().nth(digit_count + 1) == Some(' ') {
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
        if let Some(sv) = selected_var {
            if !sv.is_empty() {
                let sv_chars: Vec<char> = sv.chars().collect();
                let sv_len = sv_chars.len();
                let is_ident_char = |c: char| -> bool {
                    c.is_alphanumeric() || c == '_' || c == '/'
                };
                if n >= sv_len {
                    for start_idx in 0..=(n - sv_len) {
                        if line[start_idx..(start_idx + sv_len)] == sv_chars {
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
            _ => {
                app.show_help = false;
            }
        }
        return true;
    }
    if app.show_function_guide {
        match key.code {
            KeyCode::Char('k') | KeyCode::Char('K') | KeyCode::Up => {
                app.function_guide_scroll = app.function_guide_scroll.saturating_sub(1);
            }
            KeyCode::Char('j') | KeyCode::Char('J') | KeyCode::Down => {
                app.function_guide_scroll = app.function_guide_scroll.saturating_add(1);
            }
            KeyCode::Char('y') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                app.function_guide_scroll = app.function_guide_scroll.saturating_sub(1);
            }
            KeyCode::Char('e') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                app.function_guide_scroll = app.function_guide_scroll.saturating_add(1);
            }
            KeyCode::PageUp => {
                app.function_guide_scroll = app.function_guide_scroll.saturating_sub(10);
            }
            KeyCode::PageDown => {
                app.function_guide_scroll = app.function_guide_scroll.saturating_add(10);
            }
            _ => {
                app.show_function_guide = false;
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
                if key.code == KeyCode::F(1) {
                    app.show_function_guide = !app.show_function_guide;
                    if app.show_function_guide {
                        app.function_guide_scroll = 0;
                    }
                    continue;
                }
                if key.code == KeyCode::Char('~') && !is_insert_mode {
                    app.show_help = !app.show_help;
                    if app.show_help {
                        app.help_scroll = 0;
                    }
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
                    } else if app.show_function_guide {
                        app.show_function_guide = false;
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
            Line::from(vec![Span::styled("── Global & Navigation ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" F1          ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Toggle Function Guide", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" F2 / F3     ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Toggle Wiki Map / Variables Panel", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" ~           ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Toggle Keyboard Shortcuts", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" Esc         ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Escape modes / Return focus to Editor", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" Ctrl-q      ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Exit the program (from any mode/panel)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" ZZ          ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Save and Exit (Normal mode in Editor)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" Shift-H / L ", Style::default().fg(Color::Rgb(158, 206, 106)).bold()),
                Span::styled("Move Focus Left / Right between active panels", Style::default().fg(Color::Rgb(169, 177, 214))),
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

    // Function Guide popup modal (opened via F1)
    if app.show_function_guide {
        let area = centered_rect(80, 85, f.area());
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Rgb(158, 206, 106))) // Green border
            .bg(Color::Rgb(22, 22, 30))
            .title(Span::styled(" calki Function Guide ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()));

        let guide_text = vec![
            Line::from(vec![Span::styled("── Basic Math & Rounding ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" abs(x)             ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Absolute value of x", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" sqrt(x)            ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Square root of x (x >= 0)", Style::default().fg(Color::Rgb(169, 177, 214))),
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
                Span::styled(" min(x, y)          ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Minimum of two compatible values", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" max(x, y)          ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Maximum of two compatible values", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" mod(x, y)          ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Modulo / remainder function (or infix x % y)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),

            Line::from(vec![Span::styled("── Trigonometry & Exponential ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" sin / cos / tan    ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Trigonometric sine, cosine, tangent", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" asin / acos / atan ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Inverse trigonometric arc sine, cosine, tangent", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" sinh / cosh / tanh ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Hyperbolic sine, cosine, tangent", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" log(x) / ln(x)     ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Base-10 log / Natural log of x", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" exp(x)             ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Exponential e^x", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),

            Line::from(vec![Span::styled("── Statistics ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" sum(x, ...)        ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Sum of compatible values (e.g. sum(10m, 200cm) => 12m)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" mean / average     ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Average/mean value of arguments", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" median(x, ...)     ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Median value of arguments", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" stddev / stdev     ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Sample standard deviation (Bessel's corrected)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" variance / var     ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Sample variance of arguments", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" count(x, ...)      ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Count the number of scalar items across lists/scalars", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),

            Line::from(vec![Span::styled("── Financial (End-of-period) ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" pmt(rate, nper, pv)", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Loan payment per period", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" fv(r, nper, pmt, [pv])", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Future value of investment", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" pv(r, nper, pmt, [fv])", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Present value of investment", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),

            Line::from(vec![Span::styled("── Logic & Comparisons ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" if(cond, then, else)", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Returns 'then' if cond != 0, else 'else'", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" and(a, b, ...)     ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Logical AND (1 if all non-zero, else 0)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" or(a, b, ...)      ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Logical OR (1 if any non-zero, else 0)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" not(x)             ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Logical NOT (1 if x == 0, else 0)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" eq(a, b) / ne(a, b)", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Equal / Not equal (handles units conversion)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" lt / lte / gt / gte", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Less / Less-Equal / Greater / Greater-Equal", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),

            Line::from(vec![Span::styled("── Vectors & Matrices ──", Style::default().bold().fg(Color::Rgb(255, 158, 100)))]),
            Line::from(vec![
                Span::styled(" len(list)          ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Length of list/vector", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" plot(list)         ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Draws an ASCII/Unicode sparkline trend of values", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" vdot(v1, v2)       ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Dot product of two vectors", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" vadd(v1, v2)       ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Element-wise vector/matrix addition", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" vsub(v1, v2)       ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Element-wise vector/matrix subtraction", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" transpose(m)       ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Transpose of a matrix or vector", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(vec![
                Span::styled(" matmul(m1, m2)     ", Style::default().fg(Color::Rgb(125, 207, 255)).bold()),
                Span::styled("Matrix multiplication (supports 1D/2D numpy-like)", Style::default().fg(Color::Rgb(169, 177, 214))),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled(" Press any key to close this guide ", Style::default().fg(Color::Rgb(255, 158, 100)).italic()),
            ]),
        ];

        let max_scroll = if guide_text.len() > area.height as usize {
            (guide_text.len() - area.height as usize) as u16
        } else {
            0
        };
        if app.function_guide_scroll > max_scroll {
            app.function_guide_scroll = max_scroll;
        }

        let paragraph = Paragraph::new(guide_text)
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((app.function_guide_scroll, 0));
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
        assert_eq!(app.show_help, false);

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
}

