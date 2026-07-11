//! Markdown + math syntax highlighting for the editor buffer.
//!
//! `compute_syntax_highlights` scans each line, paints a per-column style buffer
//! across the markdown/math categories, then emits `edtui::Highlight` ranges.
//! Relocated verbatim from `main.rs`; decomposition into per-category painters
//! follows in later commits.

use ratatui::prelude::*;
use std::ops::RangeInclusive;

use crate::edtui;
use crate::{
    HighlightToken, find_in_chars, find_in_chars_from, is_registered_unit,
    tokenize_line_for_highlighting, trim_char_slice, trim_start_slice,
};

pub(crate) fn compute_syntax_highlights<T: AsRef<[char]>>(
    lines_vecs: &[T],
    selected_var: Option<&str>,
) -> Vec<edtui::Highlight> {
    let mut highlights = Vec::new();
    let mut brace_level = 0;

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
                        if !fn_name.is_empty()
                            && fn_name.iter().all(|&c| c.is_alphanumeric() || c == '_')
                        {
                            defined_vars.insert(fn_name.iter().collect::<String>());
                            for arg in args_slice.split(|&c| c == ',') {
                                let arg_trimmed = trim_char_slice(arg);
                                if !arg_trimmed.is_empty()
                                    && arg_trimmed.iter().all(|&c| c.is_alphanumeric() || c == '_')
                                {
                                    defined_vars.insert(arg_trimmed.iter().collect::<String>());
                                }
                            }
                        }
                    }
                } else if !left_part.is_empty()
                    && left_part.iter().all(|&c| c.is_alphanumeric() || c == '_')
                {
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
        let mut arrow_idx: Option<usize> = None;

        let mut is_special_line = paint_header(line, &mut line_styles);
        if !is_special_line {
            is_special_line = paint_blockquote(line, &mut line_styles);
        }
        if !is_special_line {
            is_special_line = paint_hr(line, &mut line_styles);
        }
        if !is_special_line {
            is_special_line = paint_comment(line, &mut line_styles);
        }

        let mut line_braces = 0;
        if !is_special_line {
            let mut in_quote = false;
            let mut prev_char = None;
            let mut has_lbrace = false;
            let mut has_rbrace = false;
            for &c in line {
                if c == '"' && prev_char != Some('\\') {
                    in_quote = !in_quote;
                }
                if !in_quote {
                    if c == '{' {
                        line_braces += 1;
                        has_lbrace = true;
                    } else if c == '}' {
                        line_braces -= 1;
                        has_rbrace = true;
                    }
                }
                prev_char = Some(c);
            }
            let in_block = brace_level > 0 || has_lbrace || has_rbrace;

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

            let is_in_backticks =
                |col: usize| -> bool { backtick_ranges.iter().any(|r| r.contains(&col)) };

            // A. Base Block Math & Assignments (containing '=>' or '=') outside backticks
            arrow_idx = None;
            let mut search_idx = 0;
            while let Some(pos) = find_in_chars_from(line, "=>", search_idx) {
                if !is_in_backticks(pos) {
                    arrow_idx = Some(pos);
                    break;
                }
                search_idx = pos + 2;
            }

            let mut eq_idx = None;
            let mut has_main_assignment = false;
            let mut search_idx = 0;
            while let Some(pos) = find_in_chars_from(line, "=", search_idx) {
                if !is_in_backticks(pos) && arrow_idx != Some(pos) {
                    eq_idx = Some(pos);
                    break;
                }
                search_idx = pos + 1;
            }

            let mut processed = false;
            if let Some(arrow_pos) = arrow_idx {
                if let Some(eq_pos) = eq_idx
                    && eq_pos < arrow_pos
                {
                    let lhs = &line[..eq_pos];
                    let lhs_trimmed = trim_char_slice(lhs);
                    let is_lhs_valid = !lhs_trimmed.is_empty()
                        && lhs_trimmed.iter().all(|&c| c.is_alphanumeric() || c == '_');
                    let is_assignment = is_lhs_valid && {
                        let not_equality = eq_pos + 1 >= n || line[eq_pos + 1] != '=';
                        let not_comparison =
                            eq_pos == 0 || !matches!(line[eq_pos - 1], '!' | '<' | '>');
                        not_equality && not_comparison
                    };
                    let is_fn_def = !is_assignment && {
                        if lhs_trimmed.contains(&'(') && lhs_trimmed.last() == Some(&')') {
                            if let Some(lpar_pos) = lhs_trimmed.iter().position(|&c| c == '(') {
                                let fn_name = trim_char_slice(&lhs_trimmed[..lpar_pos]);
                                let args_slice = &lhs_trimmed[lpar_pos + 1..lhs_trimmed.len() - 1];
                                let fn_valid = !fn_name.is_empty()
                                    && fn_name.iter().all(|&c| c.is_alphanumeric() || c == '_');
                                let args_valid = args_slice.split(|&c| c == ',').all(|arg| {
                                    let arg_trimmed = trim_char_slice(arg);
                                    arg_trimmed.is_empty()
                                        || arg_trimmed
                                            .iter()
                                            .all(|&c| c.is_alphanumeric() || c == '_')
                                });
                                fn_valid && args_valid
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    };

                    if is_assignment || is_fn_def {
                        is_math_line = true;
                        // LHS (Cyan)
                        for col in 0..eq_pos {
                            line_styles[col] = Some(Style::default().fg(Color::Rgb(125, 207, 255)));
                        }
                        // '=' (Bold Orange)
                        line_styles[eq_pos] =
                            Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
                        // RHS expression up to '=>' (Teal Green)
                        for col in (eq_pos + 1)..arrow_pos {
                            line_styles[col] = Some(Style::default().fg(Color::Rgb(115, 218, 202)));
                        }
                        // '=>' (Bold Orange)
                        for col in arrow_pos..std::cmp::min(arrow_pos + 2, n) {
                            line_styles[col] =
                                Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
                        }
                        // The result after '=>' (Teal Green + Italic)
                        for col in (arrow_pos + 2)..n {
                            line_styles[col] =
                                Some(Style::default().fg(Color::Rgb(115, 218, 202)).italic());
                        }
                        processed = true;
                        has_main_assignment = true;
                    }
                }

                if !processed {
                    is_math_line = true;
                    // Expression before '=>' (Cyan/light blue)
                    for col in 0..arrow_pos {
                        line_styles[col] = Some(Style::default().fg(Color::Rgb(125, 207, 255)));
                    }
                    // Operator '=>' in Bold Orange
                    for col in arrow_pos..std::cmp::min(arrow_pos + 2, n) {
                        line_styles[col] =
                            Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
                    }
                    // The result after '=>' (Teal Green + Italic)
                    for col in (arrow_pos + 2)..n {
                        line_styles[col] =
                            Some(Style::default().fg(Color::Rgb(115, 218, 202)).italic());
                    }
                    processed = true;
                }
            } else if let Some(eq_pos) = eq_idx {
                let lhs = &line[..eq_pos];
                let lhs_trimmed = trim_char_slice(lhs);
                let is_lhs_valid = !lhs_trimmed.is_empty()
                    && lhs_trimmed.iter().all(|&c| c.is_alphanumeric() || c == '_');
                let is_assignment = is_lhs_valid && {
                    let not_equality = eq_pos + 1 >= n || line[eq_pos + 1] != '=';
                    let not_comparison =
                        eq_pos == 0 || !matches!(line[eq_pos - 1], '!' | '<' | '>');
                    not_equality && not_comparison
                };
                let is_fn_def = !is_assignment && {
                    if lhs_trimmed.contains(&'(') && lhs_trimmed.last() == Some(&')') {
                        if let Some(lpar_pos) = lhs_trimmed.iter().position(|&c| c == '(') {
                            let fn_name = trim_char_slice(&lhs_trimmed[..lpar_pos]);
                            let args_slice = &lhs_trimmed[lpar_pos + 1..lhs_trimmed.len() - 1];
                            let fn_valid = !fn_name.is_empty()
                                && fn_name.iter().all(|&c| c.is_alphanumeric() || c == '_');
                            let args_valid = args_slice.split(|&c| c == ',').all(|arg| {
                                let arg_trimmed = trim_char_slice(arg);
                                arg_trimmed.is_empty()
                                    || arg_trimmed.iter().all(|&c| c.is_alphanumeric() || c == '_')
                            });
                            fn_valid && args_valid
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                if is_assignment || is_fn_def {
                    is_math_line = true;
                    // LHS (Cyan)
                    for col in 0..eq_pos {
                        line_styles[col] = Some(Style::default().fg(Color::Rgb(125, 207, 255)));
                    }
                    // '=' (Bold Orange)
                    if eq_pos < n {
                        line_styles[eq_pos] =
                            Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
                    }
                    // RHS (Teal Green)
                    for col in (eq_pos + 1)..n {
                        line_styles[col] = Some(Style::default().fg(Color::Rgb(115, 218, 202)));
                    }
                    has_main_assignment = true;
                    processed = true;
                }
            }

            if !processed && in_block {
                is_math_line = true;
                for col in 0..n {
                    line_styles[col] = Some(Style::default().fg(Color::Rgb(125, 207, 255)));
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
                        line_styles[col] =
                            Some(Style::default().fg(Color::Rgb(255, 158, 100)).bold());
                    }
                    // After => (Italic Teal Green)
                    for col in (absolute_arrow + 2)..end_pos {
                        if col < n {
                            line_styles[col] =
                                Some(Style::default().fg(Color::Rgb(115, 218, 202)).italic());
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

            // C. Link Highlighting (Wiki Links, Markdown Links, Parentheses Links, Raw URLs)
            let mut link_ranges = Vec::new();
            let link_style = Style::default().fg(Color::Rgb(187, 154, 247)).underlined();

            // C1. Outgoing Wiki Links: [[Note Name]]
            let mut idx = 0;
            while let Some(start_pos) = find_in_chars_from(line, "[[", idx) {
                if let Some(end_pos) = find_in_chars_from(line, "]]", start_pos) {
                    let absolute_end = end_pos + 1;
                    for col in start_pos..=absolute_end {
                        if col < n {
                            line_styles[col] = Some(link_style);
                        }
                    }
                    link_ranges.push(start_pos..=absolute_end);
                    idx = absolute_end + 1;
                } else {
                    break;
                }
            }

            // C2. Markdown Links: [Text](URL)
            let mut m_pos = 0;
            while m_pos < line.len() {
                if line[m_pos] == '[' {
                    let start_bracket = m_pos;
                    let mut end_bracket = None;
                    let mut idx = m_pos + 1;
                    while idx < line.len() {
                        if line[idx] == ']' {
                            end_bracket = Some(idx);
                            break;
                        }
                        idx += 1;
                    }
                    if let Some(close_b) = end_bracket {
                        // Check if followed immediately by '('
                        if close_b + 1 < line.len() && line[close_b + 1] == '(' {
                            let start_paren = close_b + 1;
                            let mut end_paren = None;
                            let mut idx2 = start_paren + 1;
                            while idx2 < line.len() {
                                if line[idx2] == ')' {
                                    end_paren = Some(idx2);
                                    break;
                                }
                                idx2 += 1;
                            }
                            if let Some(close_p) = end_paren {
                                for col in start_bracket..=close_p {
                                    if col < n {
                                        line_styles[col] = Some(link_style);
                                    }
                                }
                                link_ranges.push(start_bracket..=close_p);
                                m_pos = close_p + 1;
                                continue;
                            }
                        }
                    }
                }
                m_pos += 1;
            }

            // C3. Parentheses Links: [(Link)]
            let mut p_pos = 0;
            while p_pos < line.len() {
                if p_pos + 1 < line.len() && line[p_pos] == '[' && line[p_pos + 1] == '(' {
                    let start_pos = p_pos;
                    let mut end_pos = None;
                    let mut idx = p_pos + 2;
                    while idx + 1 < line.len() {
                        if line[idx] == ')' && line[idx + 1] == ']' {
                            end_pos = Some(idx + 1);
                            break;
                        }
                        idx += 1;
                    }
                    if let Some(absolute_end) = end_pos {
                        for col in start_pos..=absolute_end {
                            if col < n {
                                line_styles[col] = Some(link_style);
                            }
                        }
                        link_ranges.push(start_pos..=absolute_end);
                        p_pos = absolute_end + 1;
                        continue;
                    }
                }
                p_pos += 1;
            }

            // C4. Raw HTTP/HTTPS URLs
            let mut u_pos = 0;
            while u_pos < line.len() {
                if u_pos + 7 < line.len()
                    && (line[u_pos..u_pos + 7] == ['h', 't', 't', 'p', ':', '/', '/']
                        || (u_pos + 8 < line.len()
                            && line[u_pos..u_pos + 8] == ['h', 't', 't', 'p', 's', ':', '/', '/']))
                {
                    let start_url = u_pos;
                    let mut end_url = u_pos;
                    while end_url < line.len() {
                        let c = line[end_url];
                        if c.is_whitespace() || c == ']' || c == ')' || c == '>' || c == '<' {
                            break;
                        }
                        end_url += 1;
                    }
                    let mut actual_end = end_url;
                    while actual_end > start_url
                        && matches!(line[actual_end - 1], '.' | ',' | ';' | '?' | '!')
                    {
                        actual_end -= 1;
                    }
                    for col in start_url..actual_end {
                        if col < n {
                            line_styles[col] = Some(link_style);
                        }
                    }
                    if actual_end > start_url {
                        link_ranges.push(start_url..=actual_end - 1);
                    }
                    u_pos = end_url;
                } else {
                    u_pos += 1;
                }
            }

            // D. Scan for units and highlight them
            let tokens = tokenize_line_for_highlighting(line);

            for i in 0..tokens.len() {
                if let HighlightToken::Identifier { start, end, name } = &tokens[i] {
                    // Check if this is a function call (followed by '(')
                    let mut is_function = false;
                    if i + 1 < tokens.len()
                        && let HighlightToken::Symbol { ch: '(', .. } = tokens[i + 1]
                    {
                        is_function = true;
                    }

                    if is_function {
                        let in_math_context = is_math_line
                            || backtick_ranges
                                .iter()
                                .any(|r| *start >= *r.start() && *end <= *r.end());
                        if in_math_context {
                            let overlaps_link = link_ranges.iter().any(|r| {
                                (*start >= *r.start() && *start <= *r.end())
                                    || (*end >= *r.start() && *end <= *r.end())
                            });
                            if !overlaps_link {
                                for col in *start..=*end {
                                    if col < n {
                                        line_styles[col] = Some(
                                            Style::default().fg(Color::Rgb(122, 162, 247)).bold(),
                                        ); // Blue #7aa2f7
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    let mut is_unit = false;
                    if is_registered_unit(name) {
                        is_unit = true;
                    } else if i > 0
                        && let HighlightToken::Number { .. } = tokens[i - 1]
                    {
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
                        let in_math_context = is_math_line
                            || backtick_ranges
                                .iter()
                                .any(|r| start >= r.start() && end <= r.end());
                        if in_math_context {
                            // Check if it overlaps with any link range
                            let overlaps_link = link_ranges.iter().any(|r| {
                                (start >= r.start() && start <= r.end())
                                    || (end >= r.start() && end <= r.end())
                            });
                            if !overlaps_link {
                                for col in *start..=*end {
                                    if col < n {
                                        line_styles[col] =
                                            Some(Style::default().fg(Color::Rgb(244, 143, 177))); // Rose / Pink #f48fb1
                                    }
                                }
                            }
                        }
                    }
                } else if let HighlightToken::Number { start, end, val: _ } = &tokens[i] {
                    let in_math_context = is_math_line
                        || backtick_ranges
                            .iter()
                            .any(|r| *start >= *r.start() && *end <= *r.end());
                    if in_math_context {
                        let overlaps_link = link_ranges.iter().any(|r| {
                            (start >= r.start() && start <= r.end())
                                || (end >= r.start() && end <= r.end())
                        });
                        if !overlaps_link {
                            for col in *start..=*end {
                                if col < n {
                                    let italic = line_styles[col]
                                        .map(|s| {
                                            s.add_modifier
                                                .contains(ratatui::style::Modifier::ITALIC)
                                        })
                                        .unwrap_or(false);
                                    let mut style = Style::default().fg(Color::Rgb(115, 218, 202)); // Teal #73daca
                                    if italic {
                                        style = style.italic();
                                    }
                                    line_styles[col] = Some(style);
                                }
                            }
                        }
                    }
                } else if let HighlightToken::Symbol {
                    start,
                    end,
                    ch: '%',
                } = &tokens[i]
                {
                    let mut is_infix = false;
                    if i + 1 < tokens.len() {
                        match &tokens[i + 1] {
                            HighlightToken::Number { .. }
                            | HighlightToken::Identifier { .. }
                            | HighlightToken::Symbol { ch: '(', .. }
                            | HighlightToken::Symbol { ch: '[', .. } => {
                                is_infix = true;
                            }
                            _ => {}
                        }
                    }
                    if !is_infix {
                        let in_math_context = is_math_line
                            || backtick_ranges
                                .iter()
                                .any(|r| *start >= *r.start() && *end <= *r.end());
                        if in_math_context {
                            let overlaps_link = link_ranges.iter().any(|r| {
                                (*start >= *r.start() && *start <= *r.end())
                                    || (*end >= *r.start() && *end <= *r.end())
                            });
                            if !overlaps_link {
                                for col in *start..=*end {
                                    if col < n {
                                        line_styles[col] =
                                            Some(Style::default().fg(Color::Rgb(244, 143, 177))); // Rose / Pink #f48fb1
                                    }
                                }
                            }
                        }
                    } else {
                        let in_math_context = is_math_line
                            || backtick_ranges
                                .iter()
                                .any(|r| *start >= *r.start() && *end <= *r.end());
                        if in_math_context {
                            let overlaps_link = link_ranges.iter().any(|r| {
                                (*start >= *r.start() && *start <= *r.end())
                                    || (*end >= *r.start() && *end <= *r.end())
                            });
                            if !overlaps_link {
                                for col in *start..=*end {
                                    if col < n {
                                        let italic = line_styles[col]
                                            .map(|s| {
                                                s.add_modifier
                                                    .contains(ratatui::style::Modifier::ITALIC)
                                            })
                                            .unwrap_or(false);
                                        let mut style =
                                            Style::default().fg(Color::Rgb(255, 158, 100));
                                        if italic {
                                            style = style.italic();
                                        }
                                        line_styles[col] = Some(style);
                                    }
                                }
                            }
                        }
                    }
                } else if let HighlightToken::In { start, end } = &tokens[i] {
                    let in_math_context = is_math_line
                        || backtick_ranges
                            .iter()
                            .any(|r| *start >= *r.start() && *end <= *r.end());
                    if in_math_context {
                        for col in *start..=*end {
                            if col < n {
                                let italic = line_styles[col]
                                    .map(|s| {
                                        s.add_modifier.contains(ratatui::style::Modifier::ITALIC)
                                    })
                                    .unwrap_or(false);
                                let mut style =
                                    Style::default().fg(Color::Rgb(255, 158, 100)).bold();
                                if italic {
                                    style = style.italic();
                                }
                                line_styles[col] = Some(style);
                            }
                        }
                    }
                } else if let HighlightToken::Symbol { start, end, ch } = &tokens[i] {
                    // Style operator symbols like +, -, *, /, ^, %, &, |, !, =, <, >, (, ), {, }, ;, ,, [, ]
                    let mut is_operator = matches!(
                        ch,
                        '+' | '-'
                            | '*'
                            | '/'
                            | '^'
                            | '&'
                            | '|'
                            | '!'
                            | '='
                            | '<'
                            | '>'
                            | '('
                            | ')'
                            | '{'
                            | '}'
                            | ','
                            | ';'
                            | '['
                            | ']'
                    );
                    if *ch == '%' {
                        // Only highlight '%' as an operator if it's infix (modulo)
                        let mut is_infix = false;
                        if i + 1 < tokens.len() {
                            match &tokens[i + 1] {
                                HighlightToken::Number { .. }
                                | HighlightToken::Identifier { .. }
                                | HighlightToken::Symbol { ch: '(', .. }
                                | HighlightToken::Symbol { ch: '[', .. } => {
                                    is_infix = true;
                                }
                                _ => {}
                            }
                        }
                        if is_infix {
                            is_operator = true;
                        }
                    }

                    if is_operator {
                        if *ch == '=' && has_main_assignment && eq_idx == Some(*start) {
                            // Skip main assignment operator (already styled as Bold Orange)
                            continue;
                        }
                        let in_math_context = is_math_line
                            || backtick_ranges
                                .iter()
                                .any(|r| *start >= *r.start() && *end <= *r.end());
                        if in_math_context {
                            let overlaps_link = link_ranges.iter().any(|r| {
                                (*start >= *r.start() && *start <= *r.end())
                                    || (*end >= *r.start() && *end <= *r.end())
                            });
                            if !overlaps_link {
                                for col in *start..=*end {
                                    if col < n {
                                        let italic = line_styles[col]
                                            .map(|s| {
                                                s.add_modifier
                                                    .contains(ratatui::style::Modifier::ITALIC)
                                            })
                                            .unwrap_or(false);
                                        let mut style =
                                            Style::default().fg(Color::Rgb(255, 158, 100));
                                        if italic {
                                            style = style.italic();
                                        }
                                        line_styles[col] = Some(style);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // E. Lists / Bullet points
            paint_list(line, &mut line_styles);

            // F/G/H. Inline emphasis (bold/italic/strikethrough); skipped on math lines.
            if !is_math_line {
                paint_bold(line, &mut line_styles, &backtick_ranges);
                paint_italic(line, &mut line_styles, &backtick_ranges);
                paint_strike(line, &mut line_styles, &backtick_ranges);
            }
        }

        // I. Selected Variable Highlight
        paint_selected_var(line, &mut line_styles, sv_chars.as_deref());

        // Force anything after '=>' to be italic
        if let Some(arrow_pos) = arrow_idx {
            for col in (arrow_pos + 2)..n {
                if let Some(s) = line_styles[col] {
                    line_styles[col] = Some(s.italic());
                } else {
                    line_styles[col] =
                        Some(Style::default().fg(Color::Rgb(115, 218, 202)).italic());
                }
            }
        }

        if !is_special_line {
            brace_level = std::cmp::max(0, brace_level + line_braces);
        }

        styles_to_highlights(&line_styles, row_idx, &mut highlights);
    }

    highlights
}

fn paint_header(line: &[char], line_styles: &mut [Option<Style>]) -> bool {
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
            line_styles.fill(Some(header_style));
            return true;
        }
    }
    false
}

fn paint_blockquote(line: &[char], line_styles: &mut [Option<Style>]) -> bool {
    let trimmed_start = trim_start_slice(line);
    if trimmed_start.first() == Some(&'>') {
        let start_col = line.len() - trimmed_start.len();
        let quote_style = Style::default().fg(Color::Rgb(158, 206, 106)).italic(); // Italic Green #9ece6a
        line_styles[start_col..].fill(Some(quote_style));
        return true;
    }
    false
}

fn paint_hr(line: &[char], line_styles: &mut [Option<Style>]) -> bool {
    let trimmed = trim_char_slice(line);
    if (trimmed == ['-', '-', '-'] || trimmed == ['*', '*', '*'] || trimmed == ['_', '_', '_'])
        && line.len() >= 3
    {
        let hr_style = Style::default().fg(Color::Rgb(86, 95, 137)).dim(); // Muted Gray dim
        line_styles.fill(Some(hr_style));
        return true;
    }
    false
}

fn paint_comment(line: &[char], line_styles: &mut [Option<Style>]) -> bool {
    let trimmed_start = trim_start_slice(line);
    if trimmed_start.len() >= 2 && trimmed_start[0] == '/' && trimmed_start[1] == '/' {
        let start_col = line.len() - trimmed_start.len();
        let comment_style = Style::default().fg(Color::Rgb(86, 95, 137)).italic(); // Muted Gray-Blue
        line_styles[start_col..].fill(Some(comment_style));
        return true;
    }
    false
}

/// Collapse a per-column style buffer into contiguous `edtui::Highlight` ranges
/// for `row_idx`, pushing them onto `highlights`.
fn styles_to_highlights(
    line_styles: &[Option<Style>],
    row_idx: usize,
    highlights: &mut Vec<edtui::Highlight>,
) {
    let n = line_styles.len();
    let mut start_col = None;
    let mut current_style = None;

    for (col, &style) in line_styles.iter().enumerate() {
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

fn paint_list(line: &[char], line_styles: &mut [Option<Style>]) {
    let n = line.len();
    let trimmed_start = trim_start_slice(line);
    let leading_spaces = line.len() - trimmed_start.len();
    let rest = trimmed_start;
    let mut list_marker_range = None;
    if rest.starts_with(&['*', ' '])
        || rest.starts_with(&['-', ' '])
        || rest.starts_with(&['+', ' '])
    {
        list_marker_range = Some(leading_spaces..leading_spaces + 1);
    } else {
        let digit_count = rest.iter().take_while(|&&c| c.is_ascii_digit()).count();
        if digit_count > 0
            && rest.get(digit_count) == Some(&'.')
            && rest.get(digit_count + 1) == Some(&' ')
        {
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
}

fn paint_bold(
    line: &[char],
    line_styles: &mut [Option<Style>],
    backtick_ranges: &[RangeInclusive<usize>],
) {
    let n = line.len();
    let is_in_backticks = |col: usize| -> bool { backtick_ranges.iter().any(|r| r.contains(&col)) };
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
                    let base = line_styles[col]
                        .unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
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
                    let base = line_styles[col]
                        .unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
                    line_styles[col] = Some(base.bold());
                }
            }
            b_pos2 = end_pos + 2;
        } else {
            break;
        }
    }
}

fn paint_italic(
    line: &[char],
    line_styles: &mut [Option<Style>],
    backtick_ranges: &[RangeInclusive<usize>],
) {
    let n = line.len();
    let is_in_backticks = |col: usize| -> bool { backtick_ranges.iter().any(|r| r.contains(&col)) };
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
                        let base = line_styles[col]
                            .unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
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
                        let base = line_styles[col]
                            .unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
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
}

fn paint_strike(
    line: &[char],
    line_styles: &mut [Option<Style>],
    backtick_ranges: &[RangeInclusive<usize>],
) {
    let n = line.len();
    let is_in_backticks = |col: usize| -> bool { backtick_ranges.iter().any(|r| r.contains(&col)) };
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
                    let base = line_styles[col]
                        .unwrap_or_else(|| Style::default().fg(Color::Rgb(169, 177, 214)));
                    line_styles[col] = Some(base.crossed_out());
                }
            }
            s_pos = end_pos + 2;
        } else {
            break;
        }
    }
}

fn paint_selected_var(line: &[char], line_styles: &mut [Option<Style>], sv_chars: Option<&[char]>) {
    let Some(sv_chars) = sv_chars else {
        return;
    };
    let n = line.len();
    let sv_len = sv_chars.len();
    let is_ident_char = |c: char| -> bool { c.is_alphanumeric() || c == '_' || c == '/' };
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
