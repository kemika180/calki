#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    /// A date/time literal (`2026-08-01`, `2026-08-01T09:30`). `epoch_secs` is
    /// the instant in UTC; `tz_offset_secs` is the system-local UTC offset at
    /// that instant, captured so the value renders back in the original zone.
    DateTime {
        epoch_secs: f64,
        is_date_only: bool,
        tz_offset_secs: i32,
    },
    Identifier(String),
    Percentage, // %
    Bang,       // ! (postfix factorial)
    Plus,       // +
    Minus,      // -
    Star,       // *
    Slash,      // /
    Caret,      // ^
    Equal,      // =
    Arrow,      // =>
    In,         // 'in' or 'to'
    LPar,       // (
    RPar,       // )
    Comma,      // ,
    LBrack,     // [
    RBrack,     // ]

    // Comparisons
    Less,      // <
    LessEq,    // <=
    Greater,   // >
    GreaterEq, // >=
    DoubleEq,  // ==
    NotEq,     // !=

    // Logical
    And, // and
    Or,  // or
    Not, // not

    // Bitwise
    Ampersand, // &
    Pipe,      // |
    Tilde,     // ~
    LShift,    // <<
    RShift,    // >>

    // Braces / Statement Separators
    LBrace,    // {
    RBrace,    // }
    Semicolon, // ;

    // Types
    StringLiteral(String),

    // Keywords
    Else,
    Switch,
    Default,
}

pub struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    input: &'a str,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            chars: input.char_indices().peekable(),
            input,
        }
    }

    pub fn lex(mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();

        while let Some(&(idx, ch)) = self.chars.peek() {
            if ch.is_whitespace() {
                self.chars.next();
                continue;
            }

            match ch {
                '+' => {
                    self.chars.next();
                    tokens.push(Token::Plus);
                }
                '-' => {
                    self.chars.next();
                    tokens.push(Token::Minus);
                }
                '*' => {
                    self.chars.next();
                    tokens.push(Token::Star);
                }
                '/' => {
                    self.chars.next();
                    tokens.push(Token::Slash);
                }
                '^' => {
                    self.chars.next();
                    tokens.push(Token::Caret);
                }
                '&' => {
                    self.chars.next();
                    tokens.push(Token::Ampersand);
                }
                '|' => {
                    self.chars.next();
                    tokens.push(Token::Pipe);
                }
                '~' => {
                    self.chars.next();
                    tokens.push(Token::Tilde);
                }
                '(' => {
                    self.chars.next();
                    tokens.push(Token::LPar);
                }
                ')' => {
                    self.chars.next();
                    tokens.push(Token::RPar);
                }
                ',' => {
                    self.chars.next();
                    tokens.push(Token::Comma);
                }
                '[' => {
                    self.chars.next();
                    tokens.push(Token::LBrack);
                }
                ']' => {
                    self.chars.next();
                    tokens.push(Token::RBrack);
                }
                '%' => {
                    self.chars.next();
                    tokens.push(Token::Percentage);
                }
                '{' => {
                    self.chars.next();
                    tokens.push(Token::LBrace);
                }
                '}' => {
                    self.chars.next();
                    tokens.push(Token::RBrace);
                }
                ';' => {
                    self.chars.next();
                    tokens.push(Token::Semicolon);
                }
                '"' => {
                    self.chars.next();
                    let mut content = String::new();
                    let mut closed = false;
                    while let Some(&(_, ch)) = self.chars.peek() {
                        self.chars.next();
                        if ch == '"' {
                            closed = true;
                            break;
                        }
                        content.push(ch);
                    }
                    if !closed {
                        return Err("Unterminated string literal".to_string());
                    }
                    tokens.push(Token::StringLiteral(content));
                }
                '=' => {
                    self.chars.next();
                    if let Some(&(_, '>')) = self.chars.peek() {
                        self.chars.next();
                        tokens.push(Token::Arrow);
                    } else if let Some(&(_, '=')) = self.chars.peek() {
                        self.chars.next();
                        tokens.push(Token::DoubleEq);
                    } else {
                        tokens.push(Token::Equal);
                    }
                }
                '<' => {
                    self.chars.next();
                    if let Some(&(_, '=')) = self.chars.peek() {
                        self.chars.next();
                        tokens.push(Token::LessEq);
                    } else if let Some(&(_, '<')) = self.chars.peek() {
                        self.chars.next();
                        tokens.push(Token::LShift);
                    } else {
                        tokens.push(Token::Less);
                    }
                }
                '>' => {
                    self.chars.next();
                    if let Some(&(_, '=')) = self.chars.peek() {
                        self.chars.next();
                        tokens.push(Token::GreaterEq);
                    } else if let Some(&(_, '>')) = self.chars.peek() {
                        self.chars.next();
                        tokens.push(Token::RShift);
                    } else {
                        tokens.push(Token::Greater);
                    }
                }
                '!' => {
                    self.chars.next();
                    if let Some(&(_, '=')) = self.chars.peek() {
                        self.chars.next();
                        tokens.push(Token::NotEq);
                    } else {
                        tokens.push(Token::Bang);
                    }
                }
                '$' => {
                    // Treat currency sign as an identifier for standard parsing
                    self.chars.next();
                    tokens.push(Token::Identifier("$".to_string()));
                }
                _ if ch.is_ascii_digit() => {
                    // Date/time literal (`YYYY-MM-DD` [+ `T`/space `HH:MM[:SS]`])
                    // takes precedence over subtraction: `2026-08-01` is a date,
                    // `2026 - 8` (unpadded / spaced) stays arithmetic.
                    if let Some(result) = self.try_lex_datetime(idx) {
                        tokens.push(result?);
                        continue;
                    }
                    // Check for hex or binary prefix
                    let mut chars_clone = self.chars.clone();
                    chars_clone.next(); // consume current digit (which would be '0' for hex/bin)
                    if ch == '0'
                        && let Some((_, next_ch)) = chars_clone.peek()
                    {
                        if *next_ch == 'x' || *next_ch == 'X' {
                            self.chars.next();
                            self.chars.next();
                            let token = self.lex_hex_number(idx + 2)?;
                            tokens.push(token);
                            continue;
                        } else if *next_ch == 'b' || *next_ch == 'B' {
                            self.chars.next();
                            self.chars.next();
                            let token = self.lex_bin_number(idx + 2)?;
                            tokens.push(token);
                            continue;
                        }
                    }
                    let token = self.lex_number(idx)?;
                    tokens.push(token);
                }
                _ if ch.is_alphabetic() || ch == '_' => {
                    let token = self.lex_identifier(idx);
                    tokens.push(token);
                }
                _ => {
                    return Err(format!("Unexpected character '{}' at position {}", ch, idx));
                }
            }
        }

        Ok(tokens)
    }

    fn lex_hex_number(&mut self, start_idx: usize) -> Result<Token, String> {
        let mut end_idx = start_idx;
        while let Some(&(idx, ch)) = self.chars.peek() {
            if ch.is_ascii_hexdigit() {
                self.chars.next();
                end_idx = idx + ch.len_utf8();
            } else {
                break;
            }
        }
        if end_idx == start_idx {
            return Err(format!(
                "Empty hexadecimal literal at position {}",
                start_idx
            ));
        }
        let hex_str = &self.input[start_idx..end_idx];
        match i64::from_str_radix(hex_str, 16) {
            Ok(val) => Ok(Token::Number(val as f64)),
            Err(e) => Err(format!("Failed to parse hex number '{}': {}", hex_str, e)),
        }
    }

    fn lex_bin_number(&mut self, start_idx: usize) -> Result<Token, String> {
        let mut end_idx = start_idx;
        while let Some(&(idx, ch)) = self.chars.peek() {
            if ch == '0' || ch == '1' {
                self.chars.next();
                end_idx = idx + ch.len_utf8();
            } else {
                break;
            }
        }
        if end_idx == start_idx {
            return Err(format!("Empty binary literal at position {}", start_idx));
        }
        let bin_str = &self.input[start_idx..end_idx];
        match i64::from_str_radix(bin_str, 2) {
            Ok(val) => Ok(Token::Number(val as f64)),
            Err(e) => Err(format!(
                "Failed to parse binary number '{}': {}",
                bin_str, e
            )),
        }
    }

    /// Attempt to lex a date, time, or date-time literal at `start_idx`,
    /// optionally followed by a timezone. Recognizes: `YYYY-MM-DD` (two-digit
    /// month/day) with an optional `T`/space + time; a standalone time (`09:00`,
    /// `9am`, `9:30pm`) anchored to today's date; and a trailing zone (`… PST`,
    /// `… America/New_York`, `… UTC+2`). A civil time with no zone is read in the
    /// system-local zone. Returns `None` (consuming nothing) on no match, so
    /// `2026 - 8` and bare numbers fall through to arithmetic.
    fn try_lex_datetime(&mut self, start_idx: usize) -> Option<Result<Token, String>> {
        let b = &self.input.as_bytes()[start_idx..];
        let mut pos = 0usize;

        let date = scan_date(b);
        if let Some((_, _, _, len)) = date {
            pos += len;
        }

        // Time part: after a `T`/space separator when a date preceded it, else
        // as a standalone time.
        let mut time: Option<(i8, i8, i8)> = None;
        if date.is_some() {
            if (b.get(pos) == Some(&b'T') || b.get(pos) == Some(&b' '))
                && let Some((h, m, s, tlen)) = scan_time(&b[pos + 1..])
            {
                time = Some((h, m, s));
                pos += 1 + tlen;
            }
        } else if let Some((h, m, s, tlen)) = scan_time(&b[pos..]) {
            time = Some((h, m, s));
            pos += tlen;
        }

        // Neither a date nor a time → not a date/time literal.
        if date.is_none() && time.is_none() {
            return None;
        }

        // Optional trailing timezone. `scan_zone` requires a leading letter, so
        // `date + 3 days` arithmetic is never mistaken for a zone.
        let mut zone_tz = None;
        if b.get(pos) == Some(&b' ')
            && let Some((z, zl)) = scan_zone(&b[pos + 1..])
            && let Ok(tz) = crate::math::datetime::resolve_timezone(&z)
        {
            zone_tz = Some(tz);
            pos += 1 + zl;
        }

        let tz = zone_tz.unwrap_or_else(jiff::tz::TimeZone::system);
        let (year, month, day) = match date {
            Some((y, mo, d, _)) => (y, mo, d),
            None => crate::math::datetime::today_in_zone(&tz),
        };
        let (hour, minute, second) = time.unwrap_or((0, 0, 0));
        let is_date_only = time.is_none();

        let token = crate::math::datetime::civil_to_epoch_in_zone(
            year, month, day, hour, minute, second, &tz,
        )
        .map(|(epoch_secs, tz_offset_secs)| Token::DateTime {
            epoch_secs,
            is_date_only,
            tz_offset_secs,
        });

        // Commit: advance past the matched literal (ASCII, one byte per char).
        for _ in 0..pos {
            self.chars.next();
        }
        Some(token)
    }

    fn lex_number(&mut self, start_idx: usize) -> Result<Token, String> {
        let mut end_idx = start_idx;
        let mut has_decimal = false;

        while let Some(&(idx, ch)) = self.chars.peek() {
            if ch.is_ascii_digit() {
                self.chars.next();
                end_idx = idx + ch.len_utf8();
            } else if ch == '.' && !has_decimal {
                // Peek ahead to ensure there is a digit after the dot
                self.chars.next();
                if let Some(&(_, next_ch)) = self.chars.peek() {
                    if next_ch.is_ascii_digit() {
                        has_decimal = true;
                        end_idx = idx + ch.len_utf8();
                    } else {
                        // The dot is not followed by a digit (e.g., standard punctuation or end of input)
                        // Treat the number as finished before the dot
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let num_str = &self.input[start_idx..end_idx];
        match num_str.parse::<f64>() {
            Ok(val) => Ok(Token::Number(val)),
            Err(e) => Err(format!("Failed to parse number '{}': {}", num_str, e)),
        }
    }

    fn lex_identifier(&mut self, start_idx: usize) -> Token {
        let mut end_idx = start_idx;

        while let Some(&(idx, ch)) = self.chars.peek() {
            if ch.is_alphanumeric() || ch == '_' || ch == '/' {
                // We allow '/' inside unit identifiers (e.g., m/s or km/h)
                self.chars.next();
                end_idx = idx + ch.len_utf8();
            } else {
                break;
            }
        }

        let ident_str = &self.input[start_idx..end_idx];
        match ident_str {
            "in" | "to" => Token::In,
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
            "else" => Token::Else,
            "switch" => Token::Switch,
            "default" => Token::Default,
            _ => Token::Identifier(ident_str.to_string()),
        }
    }
}

/// Scan `YYYY-MM-DD` at the start of `b` (two-digit month/day, not immediately
/// followed by a further digit). Returns `(year, month, day, bytes_consumed)`.
fn scan_date(b: &[u8]) -> Option<(i16, i8, i8, usize)> {
    let d = |i: usize| b.get(i).is_some_and(|c| c.is_ascii_digit());
    if !(d(0)
        && d(1)
        && d(2)
        && d(3)
        && b.get(4) == Some(&b'-')
        && d(5)
        && d(6)
        && b.get(7) == Some(&b'-')
        && d(8)
        && d(9))
        || d(10)
    {
        return None;
    }
    let s = std::str::from_utf8(&b[..10]).ok()?;
    let year: i16 = s[0..4].parse().ok()?;
    let month: i8 = s[5..7].parse().ok()?;
    let day: i8 = s[8..10].parse().ok()?;
    Some((year, month, day, 10))
}

/// Scan a time-of-day at the start of `b`: `HH:MM[:SS]` or a bare hour, either
/// optionally suffixed with `am`/`pm` (case-insensitive). A bare hour is only a
/// time when it carries an `am`/`pm` suffix, so plain numbers stay numbers.
/// Returns `(hour, minute, second, bytes_consumed)` in 24-hour form.
fn scan_time(b: &[u8]) -> Option<(i8, i8, i8, usize)> {
    let d = |i: usize| b.get(i).is_some_and(|c| c.is_ascii_digit());
    if !d(0) {
        return None;
    }
    // Hour: 1 or 2 digits.
    let (hour_end, mut pos) = if d(1) { (2, 2) } else { (1, 1) };
    let mut hour: i32 = std::str::from_utf8(&b[0..hour_end]).ok()?.parse().ok()?;
    let mut minute: i32 = 0;
    let mut second: i32 = 0;
    let mut had_colon = false;
    if b.get(pos) == Some(&b':') && d(pos + 1) && d(pos + 2) {
        minute = std::str::from_utf8(&b[pos + 1..pos + 3])
            .ok()?
            .parse()
            .ok()?;
        pos += 3;
        had_colon = true;
        if b.get(pos) == Some(&b':') && d(pos + 1) && d(pos + 2) {
            second = std::str::from_utf8(&b[pos + 1..pos + 3])
                .ok()?
                .parse()
                .ok()?;
            pos += 3;
        }
    }
    // Optional am/pm, but not when it runs into a longer word (`9american`).
    let is_pm = match (b.get(pos), b.get(pos + 1)) {
        (Some(h), Some(m))
            if (h | 0x20) == b'a'
                && (m | 0x20) == b'm'
                && !b.get(pos + 2).is_some_and(|c| c.is_ascii_alphanumeric()) =>
        {
            pos += 2;
            Some(false)
        }
        (Some(h), Some(m))
            if (h | 0x20) == b'p'
                && (m | 0x20) == b'm'
                && !b.get(pos + 2).is_some_and(|c| c.is_ascii_alphanumeric()) =>
        {
            pos += 2;
            Some(true)
        }
        _ => None,
    };
    // A bare hour with neither a colon nor am/pm is just a number.
    if !had_colon && is_pm.is_none() {
        return None;
    }
    if let Some(pm) = is_pm {
        if !(1..=12).contains(&hour) {
            return None;
        }
        hour = match (pm, hour) {
            (false, 12) => 0,
            (false, h) => h,
            (true, 12) => 12,
            (true, h) => h + 12,
        };
    } else if hour > 23 {
        return None;
    }
    if minute > 59 || second > 59 {
        return None;
    }
    Some((hour as i8, minute as i8, second as i8, pos))
}

/// Scan a timezone token at the start of `b`: an identifier-ish run
/// (`[A-Za-z_/]+`) optionally followed by a contiguous signed whole-hour offset
/// (`+2`, `-5`). Must start with a letter so it can't consume `+ 3 days`
/// arithmetic. Returns `(token, bytes_consumed)`; the caller validates it.
fn scan_zone(b: &[u8]) -> Option<(String, usize)> {
    if !b.first().is_some_and(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut pos = 0;
    while b
        .get(pos)
        .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_' || *c == b'/')
    {
        pos += 1;
    }
    if (b.get(pos) == Some(&b'+') || b.get(pos) == Some(&b'-'))
        && b.get(pos + 1).is_some_and(|c| c.is_ascii_digit())
    {
        pos += 2;
        if b.get(pos).is_some_and(|c| c.is_ascii_digit()) {
            pos += 1;
        }
    }
    let s = std::str::from_utf8(&b[..pos]).ok()?.to_string();
    Some((s, pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_lexing() {
        let lexer = Lexer::new("x = 10 + 20.5 =>");
        let tokens = lexer.lex().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("x".to_string()),
                Token::Equal,
                Token::Number(10.0),
                Token::Plus,
                Token::Number(20.5),
                Token::Arrow,
            ]
        );
    }

    #[test]
    fn test_units_lexing() {
        let lexer = Lexer::new("10m + 50cm in feet");
        let tokens = lexer.lex().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Number(10.0),
                Token::Identifier("m".to_string()),
                Token::Plus,
                Token::Number(50.0),
                Token::Identifier("cm".to_string()),
                Token::In,
                Token::Identifier("feet".to_string()),
            ]
        );
    }

    #[test]
    fn test_currency_symbol_lexing() {
        let lexer = Lexer::new("$100 to EUR");
        let tokens = lexer.lex().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Identifier("$".to_string()),
                Token::Number(100.0),
                Token::In,
                Token::Identifier("EUR".to_string()),
            ]
        );
    }

    #[test]
    fn test_derived_units_lexing() {
        let lexer = Lexer::new("50km/h");
        let tokens = lexer.lex().unwrap();
        assert_eq!(
            tokens,
            vec![Token::Number(50.0), Token::Identifier("km/h".to_string()),]
        );
    }

    #[test]
    fn test_percentages_lexing() {
        let lexer = Lexer::new("100 - 15%");
        let tokens = lexer.lex().unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Number(100.0),
                Token::Minus,
                Token::Number(15.0),
                Token::Percentage,
            ]
        );
    }
}
