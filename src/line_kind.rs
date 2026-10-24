//! Block-level kind of each Markdown line: fenced code, header, quote, rule,
//! comment or plain text.
//!
//! This is the one place that decides line structure, so the highlighter and
//! the concealer agree on it. Fenced code blocks follow CommonMark: a run of
//! 3+ backticks or tildes opens one, and only a run of the same char, at
//! least as long and with nothing after it, closes it. An unclosed fence runs
//! to the end of the buffer.

use crate::{trim_char_slice, trim_start_slice};
use std::rc::Rc;

/// Fence languages whose bodies calki still treats as math.
const MATH_LANGS: [&str; 2] = ["calki", "math"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LineKind {
    /// The line that opens or closes a fenced code block.
    FenceDelimiter,
    /// A line inside a fenced code block. `lang` is the first word of the
    /// opening fence's info string (```` ```python ```` → `python`).
    Code {
        lang: Option<Rc<str>>,
    },
    /// An ATX header and its level (1 for `#`).
    Header(usize),
    Blockquote,
    Rule,
    Comment,
    Text,
}

impl LineKind {
    /// Code in a fence tagged with a language other than calki's math, which
    /// calki must not style as markdown or math.
    pub(crate) fn is_foreign_code(&self) -> bool {
        match self {
            Self::Code { lang: Some(lang) } => {
                !MATH_LANGS.iter().any(|m| lang.eq_ignore_ascii_case(m))
            }
            _ => false,
        }
    }
}

/// The kind of every line in `lines`, in order.
pub(crate) fn classify_lines<T: AsRef<[char]>>(lines: &[T]) -> Vec<LineKind> {
    let mut kinds = Vec::with_capacity(lines.len());
    let mut open: Option<(Fence, Option<Rc<str>>)> = None;
    for line in lines {
        let line = line.as_ref();
        let kind = if let Some((fence, lang)) = &open {
            if fence.is_closed_by(line) {
                open = None;
                LineKind::FenceDelimiter
            } else {
                LineKind::Code { lang: lang.clone() }
            }
        } else if let Some((fence, lang)) = Fence::opened_by(line) {
            open = Some((fence, lang));
            LineKind::FenceDelimiter
        } else {
            classify_line(line)
        };
        kinds.push(kind);
    }
    kinds
}

/// The kind of a line outside any fenced code block.
fn classify_line(line: &[char]) -> LineKind {
    if let Some(level) = header_level(line) {
        LineKind::Header(level)
    } else if is_blockquote(line) {
        LineKind::Blockquote
    } else if is_hr(line) {
        LineKind::Rule
    } else if is_comment(line) {
        LineKind::Comment
    } else {
        LineKind::Text
    }
}

/// Markdown ATX-header level of `line` (1 for `#`, 2 for `##`, …), or `None` if
/// the line is not a header. A header is a run of leading `#` followed by a
/// space or end-of-line. Shared with header-section folding.
pub(crate) fn header_level(line: &[char]) -> Option<usize> {
    if line.first() != Some(&'#') {
        return None;
    }
    let header_len = line.iter().take_while(|&&c| c == '#').count();
    if line.get(header_len) == Some(&' ') || line.len() == header_len {
        Some(header_len)
    } else {
        None
    }
}

fn is_blockquote(line: &[char]) -> bool {
    trim_start_slice(line).first() == Some(&'>')
}

fn is_hr(line: &[char]) -> bool {
    let trimmed = trim_char_slice(line);
    (trimmed == ['-', '-', '-'] || trimmed == ['*', '*', '*'] || trimmed == ['_', '_', '_'])
        && line.len() >= 3
}

fn is_comment(line: &[char]) -> bool {
    trim_start_slice(line).starts_with(&['/', '/'])
}

/// An open fenced code block: its marker char and run length.
struct Fence {
    marker: char,
    len: usize,
}

impl Fence {
    /// The fence `line` opens and its language, if any. A backtick fence's
    /// info string may not contain a backtick, so ```` ```ls``` ```` is inline
    /// code, not a fence.
    fn opened_by(line: &[char]) -> Option<(Self, Option<Rc<str>>)> {
        let (marker, len, rest) = fence_run(line)?;
        if marker == '`' && rest.contains(&'`') {
            return None;
        }
        let lang: String = trim_start_slice(rest)
            .iter()
            .take_while(|c| !c.is_whitespace())
            .collect();
        let lang = (!lang.is_empty()).then(|| Rc::from(lang));
        Some((Self { marker, len }, lang))
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

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(text: &str) -> Vec<LineKind> {
        let lines: Vec<Vec<char>> = text.lines().map(|l| l.chars().collect()).collect();
        classify_lines(&lines)
    }

    fn code(lang: Option<&str>) -> LineKind {
        LineKind::Code {
            lang: lang.map(Rc::from),
        }
    }

    fn chars(s: &str) -> Vec<char> {
        s.chars().collect()
    }

    #[test]
    fn classifies_line_kinds_outside_fences() {
        use LineKind::*;
        assert_eq!(
            kinds("# Title\n## Sub\n  > quote\n---\n // note\nplain `code`\n#tag\n/ no"),
            vec![
                Header(1),
                Header(2),
                Blockquote,
                Rule,
                Comment,
                Text,
                Text,
                Text
            ]
        );
    }

    #[test]
    fn header_level_detects_atx_levels() {
        assert_eq!(header_level(&chars("# Title")), Some(1));
        assert_eq!(header_level(&chars("## Sub")), Some(2));
        assert_eq!(header_level(&chars("###### Deep")), Some(6));
        // Bare header with no trailing text still counts.
        assert_eq!(header_level(&chars("#")), Some(1));
        assert_eq!(header_level(&chars("###")), Some(3));
    }

    #[test]
    fn header_level_rejects_non_headers() {
        assert_eq!(header_level(&chars("")), None);
        assert_eq!(header_level(&chars("not a header")), None);
        // `#` must be followed by a space or end-of-line, not another glyph.
        assert_eq!(header_level(&chars("#tag")), None);
        assert_eq!(header_level(&chars("##bold")), None);
        // Leading whitespace disqualifies (ATX headers start at column 0).
        assert_eq!(header_level(&chars(" # indented")), None);
    }

    #[test]
    fn fence_body_records_language() {
        assert_eq!(
            kinds("```python  extra\n# not a header\n```\nafter"),
            vec![
                LineKind::FenceDelimiter,
                code(Some("python")),
                LineKind::FenceDelimiter,
                LineKind::Text,
            ]
        );
        assert_eq!(kinds("~~~\nx\n~~~")[1], code(None));
    }

    #[test]
    fn unclosed_fence_runs_to_end() {
        assert_eq!(
            kinds("```\na\nb"),
            vec![LineKind::FenceDelimiter, code(None), code(None)]
        );
    }

    #[test]
    fn closing_fence_rules() {
        // Shorter, different-marker and trailing-text runs don't close.
        let found = kinds("````md\n```\n~~~~\n``` x\n````\nout");
        assert_eq!(
            found[1..4],
            [code(Some("md")), code(Some("md")), code(Some("md"))]
        );
        assert_eq!(found[4], LineKind::FenceDelimiter);
        assert_eq!(found[5], LineKind::Text);
    }

    #[test]
    fn inline_triple_backticks_are_not_a_fence() {
        assert_eq!(kinds("```ls``` lists files"), vec![LineKind::Text]);
    }

    #[test]
    fn foreign_code_is_tagged_with_a_non_math_language() {
        assert!(code(Some("python")).is_foreign_code());
        assert!(!code(Some("Calki")).is_foreign_code());
        assert!(!code(Some("math")).is_foreign_code());
        assert!(!code(None).is_foreign_code());
        assert!(!LineKind::Text.is_foreign_code());
    }
}
