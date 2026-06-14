use crate::helper::char_width;
use ratatui_core::text::Span;

#[derive(Default)]
pub(crate) struct LineWrapper;

impl LineWrapper {
    /// Splits a given line width into multiple smaller widths, ensuring each width
    /// is no larger than the specified maximum width.
    pub(crate) fn determine_split(line_width: usize, max_width: usize) -> Vec<usize> {
        if line_width == 0 {
            return vec![0];
        }

        let mut remaining_width = line_width;
        let mut split_widths = Vec::new();

        while remaining_width > 0 {
            let current_chunk = std::cmp::min(remaining_width, max_width);
            split_widths.push(current_chunk);
            remaining_width = remaining_width.saturating_sub(max_width);
        }

        split_widths
    }

    pub(crate) fn wrap_line(line: &[char], max_width: usize, tab_width: usize) -> Vec<Vec<char>> {
        if line.is_empty() {
            return vec![Vec::new()];
        }
        if max_width == 0 {
            return vec![line.to_vec()];
        }

        let mut lines = Vec::new();
        let mut current_line = Vec::new();
        let mut current_width = 0;
        let mut last_space_idx: Option<usize> = None;

        let mut i = 0;
        while i < line.len() {
            let ch = line[i];
            let ch_w = char_width(ch, tab_width);

            if current_width + ch_w > max_width {
                if let Some(space_idx) = last_space_idx {
                    // Wrap at the last space
                    let wrapped_part = current_line[..space_idx].to_vec();
                    lines.push(wrapped_part);

                    let remaining_part = current_line[space_idx + 1..].to_vec();
                    current_line = remaining_part;

                    current_width = 0;
                    last_space_idx = None;
                    for j in 0..current_line.len() {
                        let c = current_line[j];
                        current_width += char_width(c, tab_width);
                        if c == ' ' {
                            last_space_idx = Some(j);
                        }
                    }
                    continue;
                } else {
                    // Force wrap
                    lines.push(current_line.clone());
                    current_line.clear();
                    current_width = 0;
                    last_space_idx = None;

                    current_line.push(ch);
                    current_width += ch_w;
                    if ch == ' ' {
                        last_space_idx = Some(0);
                    }
                }
            } else {
                current_line.push(ch);
                current_width += ch_w;
                if ch == ' ' {
                    last_space_idx = Some(current_line.len() - 1);
                }
            }
            i += 1;
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        lines
    }

    pub(crate) fn wrap_spans(
        spans: Vec<Span<'_>>,
        max_width: usize,
        tab_width: usize,
    ) -> Vec<Vec<Span<'_>>> {
        if spans.is_empty() {
            return Vec::new();
        }
        if max_width == 0 {
            return vec![spans];
        }

        // Flatten spans to (char, Style) pairs
        let mut char_styles = Vec::new();
        for span in spans {
            for ch in span.content.chars() {
                char_styles.push((ch, span.style));
            }
        }

        // Run wrapping algorithm
        let mut lines = Vec::new();
        let mut current_line = Vec::new();
        let mut current_width = 0;
        let mut last_space_idx: Option<usize> = None;

        let mut i = 0;
        while i < char_styles.len() {
            let (ch, style) = char_styles[i];
            let ch_w = char_width(ch, tab_width);

            if current_width + ch_w > max_width {
                if let Some(space_idx) = last_space_idx {
                    // Wrap at the last space
                    let wrapped_part = current_line[..space_idx].to_vec();
                    lines.push(wrapped_part);

                    let remaining_part = current_line[space_idx + 1..].to_vec();
                    current_line = remaining_part;

                    current_width = 0;
                    last_space_idx = None;
                    for j in 0..current_line.len() {
                        let (c, _) = current_line[j];
                        current_width += char_width(c, tab_width);
                        if c == ' ' {
                            last_space_idx = Some(j);
                        }
                    }
                    continue;
                } else {
                    // Force wrap
                    lines.push(current_line.clone());
                    current_line.clear();
                    current_width = 0;
                    last_space_idx = None;

                    current_line.push((ch, style));
                    current_width += ch_w;
                    if ch == ' ' {
                        last_space_idx = Some(0);
                    }
                }
            } else {
                current_line.push((ch, style));
                current_width += ch_w;
                if ch == ' ' {
                    last_space_idx = Some(current_line.len() - 1);
                }
            }
            i += 1;
        }

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        // Reconstruct spans from wrapped lines
        let mut wrapped_spans = Vec::new();
        for line in lines {
            let mut line_spans = Vec::new();
            if line.is_empty() {
                wrapped_spans.push(line_spans);
                continue;
            }

            let mut current_style = line[0].1;
            let mut current_text = String::new();

            for (ch, style) in line {
                if style == current_style {
                    current_text.push(ch);
                } else {
                    line_spans.push(Span::styled(current_text.clone(), current_style));
                    current_text.clear();
                    current_text.push(ch);
                    current_style = style;
                }
            }
            if !current_text.is_empty() {
                line_spans.push(Span::styled(current_text, current_style));
            }
            wrapped_spans.push(line_spans);
        }

        wrapped_spans
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wrap_spans_force() {
        let spans = vec![Span::raw("Hello"), Span::raw("World")];
        let wrapped_spans = LineWrapper::wrap_spans(spans, 3, 0);

        assert_eq!(wrapped_spans[0], vec![Span::styled("Hel", Style::default())]);
        assert_eq!(wrapped_spans[1], vec![Span::styled("loW", Style::default())]);
        assert_eq!(wrapped_spans[2], vec![Span::styled("orl", Style::default())]);
        assert_eq!(wrapped_spans[3], vec![Span::styled("d", Style::default())]);
    }

    #[test]
    fn test_wrap_spans_word_boundary() {
        let spans = vec![Span::raw("Hello "), Span::raw("World")];
        let wrapped_spans = LineWrapper::wrap_spans(spans, 7, 0);

        assert_eq!(wrapped_spans[0], vec![Span::styled("Hello", Style::default())]);
        assert_eq!(wrapped_spans[1], vec![Span::styled("World", Style::default())]);
    }

    #[test]
    fn test_wrap_spans_with_emoji() {
        let spans = vec![Span::raw("Hell🙂!")];
        let wrapped_spans = LineWrapper::wrap_spans(spans, 4, 0);

        assert_eq!(wrapped_spans[0], vec![Span::styled("Hell", Style::default())]);
        assert_eq!(wrapped_spans[1], vec![Span::styled("🙂!", Style::default())]);
    }
}
