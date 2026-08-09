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

    /// Attempt to lex a date or date-time literal at `start_idx`. Matches
    /// `YYYY-MM-DD` (two-digit month/day) with an optional `T`/space +
    /// `HH:MM[:SS]`. Returns `None` and consumes nothing on no match, so
    /// unpadded/spaced forms like `2026 - 8` fall through to arithmetic.
    fn try_lex_datetime(&mut self, start_idx: usize) -> Option<Result<Token, String>> {
        let rest = &self.input[start_idx..];
        let b = rest.as_bytes();
        let d = |i: usize| b.get(i).is_some_and(|c| c.is_ascii_digit());

        // YYYY-MM-DD, and not followed immediately by another digit.
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

        let year: i16 = rest[0..4].parse().ok()?;
        let month: i8 = rest[5..7].parse().ok()?;
        let day: i8 = rest[8..10].parse().ok()?;

        // Optional time: `T` or a single space, then `HH:MM` (+ optional `:SS`).
        let mut len = 10;
        let mut time: Option<(i8, i8, i8)> = None;
        if (b.get(10) == Some(&b'T') || b.get(10) == Some(&b' '))
            && d(11)
            && d(12)
            && b.get(13) == Some(&b':')
            && d(14)
            && d(15)
        {
            let hour: i8 = rest[11..13].parse().ok()?;
            let minute: i8 = rest[14..16].parse().ok()?;
            let mut second: i8 = 0;
            len = 16;
            if b.get(16) == Some(&b':') && d(17) && d(18) {
                second = rest[17..19].parse().ok()?;
                len = 19;
            }
            time = Some((hour, minute, second));
        }

        let is_date_only = time.is_none();
        let (hour, minute, second) = time.unwrap_or((0, 0, 0));
        let token = civil_to_epoch(year, month, day, hour, minute, second).map(
            |(epoch_secs, tz_offset_secs)| Token::DateTime {
                epoch_secs,
                is_date_only,
                tz_offset_secs,
            },
        );

        // Commit: advance past the matched literal (ASCII, one byte per char).
        for _ in 0..len {
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

/// Resolve a civil (wall-clock) date/time in the system-local zone to
/// `(epoch_seconds_utc, utc_offset_seconds)`. The offset is captured so the
/// value renders back in the same zone it was written in.
fn civil_to_epoch(
    year: i16,
    month: i8,
    day: i8,
    hour: i8,
    minute: i8,
    second: i8,
) -> Result<(f64, i32), String> {
    let dt = jiff::civil::DateTime::new(year, month, day, hour, minute, second, 0)
        .map_err(|e| format!("Invalid date/time: {e}"))?;
    let zoned = dt
        .to_zoned(jiff::tz::TimeZone::system())
        .map_err(|e| format!("Invalid date/time: {e}"))?;
    Ok((
        zoned.timestamp().as_second() as f64,
        zoned.offset().seconds(),
    ))
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
