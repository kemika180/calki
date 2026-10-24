//! Decides which markup the editor conceals on lines the cursor is not on.
//!
//! The editor widget only knows "hide these char ranges on this row" (see
//! `edtui::state::conceal`); this module picks the ranges. Currently that is
//! the backticks around inline code. Fenced code blocks (```` ``` ```` or `~~~`) and
//! lines the highlighter styles as a whole (headers, quotes, rules, comments)
//! are left raw.

use crate::highlight::{inline_code_ranges, is_special_line};
use crate::trim_start_slice;
use std::collections::HashMap;
use std::ops::Range;

/// Hidden char ranges keyed by buffer row, for
/// `EditorState::set_conceal_ranges`.
pub(crate) fn compute_conceal_ranges<T: AsRef<[char]>>(
    lines: &[T],
) -> HashMap<usize, Vec<Range<usize>>> {
    let mut ranges = HashMap::new();
    let mut open_fence: Option<Fence> = None;
    for (row, line) in lines.iter().enumerate() {
        let line = line.as_ref();
        if let Some(fence) = &open_fence {
            if fence.is_closed_by(line) {
                open_fence = None;
            }
            continue;
        }
        if let Some(fence) = Fence::opened_by(line) {
            open_fence = Some(fence);
            continue;
        }
        if is_special_line(line) {
            continue;
        }
        let hidden = inline_code_delimiters(line);
        if !hidden.is_empty() {
            ranges.insert(row, hidden);
        }
    }
    ranges
}

/// An open fenced code block, following CommonMark: a run of 3+ backticks or
/// tildes opens it, and only a run of the same char, at least as long and
/// with nothing after it, closes it.
struct Fence {
    marker: char,
    len: usize,
}

impl Fence {
    /// The fence `line` opens, if any. A backtick fence's info string may not
    /// contain a backtick, so ```` ```ls``` ```` is inline code, not a fence.
    fn opened_by(line: &[char]) -> Option<Self> {
        let (marker, len, rest) = fence_run(line)?;
        if marker == '`' && rest.contains(&'`') {
            return None;
        }
        Some(Self { marker, len })
    }

    fn is_closed_by(&self, line: &[char]) -> bool {
        fence_run(line).is_some_and(|(marker, len, rest)| {
            marker == self.marker && len >= self.len && rest.iter().all(|c| c.is_whitespace())
        })
    }
}

/// The fence char, run length and remainder of `line` if, after leading
/// whitespace, it starts with 3+ backticks or tildes.
fn fence_run(line: &[char]) -> Option<(char, usize, &[char])> {
    let trimmed = trim_start_slice(line);
    let marker = *trimmed.first().filter(|&&c| c == '`' || c == '~')?;
    let len = trimmed.iter().take_while(|&&c| c == marker).count();
    (len >= 3).then(|| (marker, len, &trimmed[len..]))
}

/// The opening and closing backtick of each single-backtick inline-code span
/// with a non-empty body. Spans whose delimiters touch another backtick
/// (`` ``x`` ``, ```` ```x``` ````) are left alone.
fn inline_code_delimiters(line: &[char]) -> Vec<Range<usize>> {
    let is_tick = |col: Option<usize>| col.and_then(|c| line.get(c)) == Some(&'`');
    inline_code_ranges(line)
        .into_iter()
        .filter(|span| {
            let (start, end) = (*span.start(), *span.end());
            end > start + 1 && !is_tick(start.checked_sub(1)) && !is_tick(Some(end + 1))
        })
        .flat_map(|span| {
            let (start, end) = (*span.start(), *span.end());
            [start..start + 1, end..end + 1]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<Vec<char>> {
        text.lines().map(|l| l.chars().collect()).collect()
    }

    fn ranges(text: &str) -> HashMap<usize, Vec<Range<usize>>> {
        compute_conceal_ranges(&lines(text))
    }

    #[test]
    fn conceals_single_pair() {
        assert_eq!(ranges("a `b` c"), HashMap::from([(0, vec![2..3, 4..5])]));
    }

    #[test]
    fn multiple_spans_per_line() {
        assert_eq!(
            ranges("`a` and `bc`"),
            HashMap::from([(0, vec![0..1, 2..3, 8..9, 11..12])])
        );
    }

    #[test]
    fn skips_empty_span() {
        assert!(ranges("empty `` here").is_empty());
    }

    #[test]
    fn skips_double_backtick_delimiters() {
        assert!(ranges("a ``b`` c").is_empty());
        assert!(ranges("x ```y``` z").is_empty());
    }

    #[test]
    fn skips_fence_lines_and_bodies() {
        let text = "before `a`\n```rust\nlet `x` = 1;\n```\nafter `b`";
        let found = ranges(text);
        assert_eq!(found.keys().copied().collect::<Vec<_>>().len(), 2);
        assert!(found.contains_key(&0));
        assert!(found.contains_key(&4));
    }

    #[test]
    fn indented_fence_is_a_fence() {
        let found = ranges("  ```\n`a`\n  ```\n`b`");
        assert!(!found.contains_key(&1));
        assert!(found.contains_key(&3));
    }

    #[test]
    fn skips_special_lines() {
        assert!(ranges("# Title `x`").is_empty());
        assert!(ranges("> quote `x`").is_empty());
        assert!(ranges("// comment `x`").is_empty());
    }

    #[test]
    fn unmatched_not_concealed() {
        assert!(ranges("half `open").is_empty());
        assert_eq!(ranges("`a` `b"), HashMap::from([(0, vec![0..1, 2..3])]));
    }

    #[test]
    fn multibyte_char_columns() {
        // Columns are char indices: 'é' is one column.
        assert_eq!(ranges("é `ü`"), HashMap::from([(0, vec![2..3, 4..5])]));
    }

    #[test]
    fn inline_triple_backticks_do_not_open_a_fence() {
        // "```ls``` lists files" is inline code under CommonMark, not a fence.
        let found = ranges("```ls``` lists files\nthen `x`");
        assert!(found.contains_key(&1));
    }

    #[test]
    fn tilde_fence_body_is_left_raw() {
        let found = ranges("~~~sh\necho `date`\n~~~\nafter `b`");
        assert!(!found.contains_key(&1));
        assert!(found.contains_key(&3));
    }

    #[test]
    fn shorter_or_different_run_does_not_close_fence() {
        let text = "````md\n```\nin `a`\n```\n~~~~\n````\nout `b`";
        let found = ranges(text);
        assert!(!found.contains_key(&2));
        assert!(found.contains_key(&6));
    }

    #[test]
    fn fence_with_trailing_text_does_not_close() {
        let found = ranges("```\n``` not a close\nstill `in`\n```\nout `x`");
        assert!(!found.contains_key(&2));
        assert!(found.contains_key(&4));
    }
}
