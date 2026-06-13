use crate::math::lexer::Token;

#[derive(Clone, Debug, PartialEq)]
pub struct Quantity {
    pub value: f64,
    pub unit: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Number(f64),
    Quantity(f64, String),
    Variable(String),
    Percentage(Box<Expr>),
    BinaryOp(Op, Box<Expr>, Box<Expr>),
    FnCall(String, Vec<Expr>),
    Convert(Box<Expr>, String),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    Text(String),
    Assignment {
        name: String,
        expr: Expr,
        raw_prefix: String,
        current_result: Option<String>,
    },
    FnDefinition {
        name: String,
        args: Vec<String>,
        expr: Expr,
        raw_prefix: String,
    },
    Evaluation {
        expr: Expr,
        raw_prefix: String,
        current_result: Option<String>,
    },
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    pub fn parse(mut self) -> Result<Expr, String> {
        let expr = self.parse_expression(0)?;
        if !self.is_at_end() {
            return Err(format!(
                "Unexpected trailing tokens starting at pos {}",
                self.pos
            ));
        }
        Ok(expr)
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next_token(&mut self) -> Option<Token> {
        if self.pos < self.tokens.len() {
            let tok = self.tokens[self.pos].clone();
            self.pos += 1;
            Some(tok)
        } else {
            None
        }
    }

    fn is_at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn expect(&mut self, token: Token, err_msg: &str) -> Result<(), String> {
        if self.peek() == Some(&token) {
            self.pos += 1;
            Ok(())
        } else {
            Err(err_msg.to_string())
        }
    }

    // Core Pratt Parser loop
    fn parse_expression(&mut self, min_bp: u8) -> Result<Expr, String> {
        let token = self.next_token().ok_or("Unexpected end of expression")?;
        let mut left = self.parse_prefix(token)?;

        while !self.is_at_end() {
            let next_tok = self.peek().cloned().unwrap();
            
            // Handle percentage as a suffix operator (highest precedence)
            if next_tok == Token::Percentage {
                let (left_bp, _) = suffix_binding_power(&next_tok);
                if left_bp < min_bp {
                    break;
                }
                self.next_token(); // consume %
                left = Expr::Percentage(Box::new(left));
                continue;
            }

            // Handle standard infix/binary operators
            if let Some((left_bp, right_bp)) = infix_binding_power(&next_tok) {
                if left_bp < min_bp {
                    break;
                }

                self.next_token(); // consume operator
                left = self.parse_infix(left, next_tok, right_bp)?;
                continue;
            }

            break;
        }

        Ok(left)
    }

    fn parse_prefix(&mut self, token: Token) -> Result<Expr, String> {
        match token {
            Token::Number(val) => {
                // Peek ahead to see if a unit identifier follows immediately
                if let Some(Token::Identifier(unit)) = self.peek() {
                    let unit_str = unit.clone();
                    self.next_token(); // consume identifier
                    Ok(Expr::Quantity(val, unit_str))
                } else {
                    Ok(Expr::Number(val))
                }
            }
            Token::Identifier(ref name) if name == "$" => {
                // Prefix currency notation: $100 -> Quantity(100.0, "$")
                if let Some(&Token::Number(val)) = self.peek() {
                    self.next_token(); // consume number
                    Ok(Expr::Quantity(val, "$".to_string()))
                } else {
                    Ok(Expr::Quantity(1.0, "$".to_string()))
                }
            }
            Token::Identifier(name) => {
                if self.peek() == Some(&Token::LPar) {
                    self.next_token(); // consume '('
                    let mut args = Vec::new();
                    if self.peek() != Some(&Token::RPar) {
                        loop {
                            args.push(self.parse_expression(0)?);
                            if self.peek() == Some(&Token::Comma) {
                                self.next_token(); // consume ','
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Token::RPar, "Expected ')' after function arguments")?;
                    Ok(Expr::FnCall(name, args))
                } else {
                    Ok(Expr::Variable(name))
                }
            }
            Token::LPar => {
                let expr = self.parse_expression(0)?;
                self.expect(Token::RPar, "Expected matching ')'")?;
                Ok(expr)
            }
            Token::Minus => {
                // Unary minus: represented as 0 - expr
                let (_, right_bp) = prefix_binding_power(&Token::Minus);
                let expr = self.parse_expression(right_bp)?;
                Ok(Expr::BinaryOp(Op::Sub, Box::new(Expr::Number(0.0)), Box::new(expr)))
            }
            Token::Plus => {
                // Unary plus: just return the expression
                let (_, right_bp) = prefix_binding_power(&Token::Plus);
                self.parse_expression(right_bp)
            }
            _ => Err(format!("Expected expression, found token {:?}", token)),
        }
    }

    fn parse_infix(&mut self, left: Expr, op_token: Token, right_bp: u8) -> Result<Expr, String> {
        match op_token {
            Token::Plus => {
                let right = self.parse_expression(right_bp)?;
                Ok(Expr::BinaryOp(Op::Add, Box::new(left), Box::new(right)))
            }
            Token::Minus => {
                let right = self.parse_expression(right_bp)?;
                Ok(Expr::BinaryOp(Op::Sub, Box::new(left), Box::new(right)))
            }
            Token::Star => {
                let right = self.parse_expression(right_bp)?;
                Ok(Expr::BinaryOp(Op::Mul, Box::new(left), Box::new(right)))
            }
            Token::Slash => {
                let right = self.parse_expression(right_bp)?;
                Ok(Expr::BinaryOp(Op::Div, Box::new(left), Box::new(right)))
            }
            Token::Caret => {
                let right = self.parse_expression(right_bp)?;
                Ok(Expr::BinaryOp(Op::Pow, Box::new(left), Box::new(right)))
            }
            Token::In => {
                // Explicit conversion: [expr] in [unit]
                // We consume all subsequent tokens that are valid in a unit expression:
                // Identifier, Slash, Star, Caret, Number
                let mut unit_str = String::new();
                let mut consumed_any = false;
                while let Some(tok) = self.peek() {
                    match tok {
                        Token::Identifier(s) => {
                            unit_str.push_str(s);
                            self.next_token();
                            consumed_any = true;
                        }
                        Token::Slash => {
                            unit_str.push('/');
                            self.next_token();
                            consumed_any = true;
                        }
                        Token::Star => {
                            unit_str.push('*');
                            self.next_token();
                            consumed_any = true;
                        }
                        Token::Caret => {
                            unit_str.push('^');
                            self.next_token();
                            consumed_any = true;
                        }
                        Token::Number(val) => {
                            unit_str.push_str(&val.to_string());
                            self.next_token();
                            consumed_any = true;
                        }
                        _ => break,
                    }
                }
                if !consumed_any {
                    return Err("Expected target unit name after conversion keyword".to_string());
                }
                Ok(Expr::Convert(Box::new(left), unit_str))
            }
            _ => Err(format!("Unexpected infix operator {:?}", op_token)),
        }
    }
}

// Pratt Precedence Binding Powers
fn prefix_binding_power(op: &Token) -> ((), u8) {
    match op {
        Token::Plus | Token::Minus => ((), 40),
        _ => panic!("Not a prefix operator"),
    }
}

fn suffix_binding_power(op: &Token) -> (u8, ()) {
    match op {
        Token::Percentage => (50, ()),
        _ => panic!("Not a suffix operator"),
    }
}

fn infix_binding_power(op: &Token) -> Option<(u8, u8)> {
    match op {
        Token::In => Some((5, 6)),
        Token::Plus | Token::Minus => Some((10, 11)),
        Token::Star | Token::Slash => Some((20, 21)),
        Token::Caret => Some((31, 30)), // Right-associative exponentiation
        _ => None,
    }
}

// Parses a full document line (either assignment, evaluation, fn def, or plain markdown)
pub fn parse_line(line_text: &str) -> Line {
    use crate::math::lexer::Lexer;

    let trimmed = line_text.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
        return Line::Text(line_text.to_string());
    }

    // Check for explicit block-level assignment: var = expr
    if let Some(eq_pos) = trimmed.find('=') {
        // Double-check it's not a '=>' arrow
        let is_arrow = eq_pos + 1 < trimmed.len() && trimmed.as_bytes()[eq_pos + 1] == b'>';
        if !is_arrow {
            let left_part = trimmed[..eq_pos].trim();
            let right_part = trimmed[eq_pos + 1..].trim();

            // Check if left_part is a function signature: name(args)
            if left_part.contains('(') && left_part.ends_with(')') {
                if let Some(lpar_pos) = left_part.find('(') {
                    let fn_name = left_part[..lpar_pos].trim();
                    let args_str = &left_part[lpar_pos + 1..left_part.len() - 1];
                    let args: Vec<String> = args_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();

                    if !fn_name.is_empty() && fn_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                        let lexer = Lexer::new(right_part);
                        if let Ok(tokens) = lexer.lex() {
                            let parser = Parser::new(tokens);
                            if let Ok(expr) = parser.parse() {
                                let raw_prefix = line_text[..line_text.find('=').unwrap() + 1].to_string();
                                return Line::FnDefinition {
                                    name: fn_name.to_string(),
                                    args,
                                    expr,
                                    raw_prefix,
                                };
                            }
                        }
                    }
                }
            }

            // Otherwise, check if left_part is a single variable name
            if !left_part.is_empty() && left_part.chars().all(|c| c.is_alphanumeric() || c == '_') {
                let (expr_part, current_result, has_arrow) = if let Some(arrow_pos) = right_part.find("=>") {
                    let expr_part = right_part[..arrow_pos].trim();
                    let res_str = right_part[arrow_pos + 2..].trim();
                    let current_result = if res_str.is_empty() { None } else { Some(res_str.to_string()) };
                    (expr_part, current_result, true)
                } else {
                    (right_part, None, false)
                };

                let lexer = Lexer::new(expr_part);
                if let Ok(tokens) = lexer.lex() {
                    let parser = Parser::new(tokens);
                    if let Ok(expr) = parser.parse() {
                        let eq_idx = line_text.find('=').unwrap();
                        let raw_prefix = if has_arrow {
                            let arrow_idx = line_text.find("=>").unwrap();
                            line_text[..arrow_idx + 2].to_string()
                        } else {
                            line_text[..eq_idx + 1].to_string()
                        };
                        return Line::Assignment {
                            name: left_part.to_string(),
                            expr,
                            raw_prefix,
                            current_result,
                        };
                    }
                }
            }
        }
    }

    // Check for explicit block-level evaluation: expr => [result]
    if let Some(arrow_pos) = trimmed.find("=>") {
        let left_part = trimmed[..arrow_pos].trim();
        let right_part = trimmed[arrow_pos + 2..].trim();
        let current_result = if right_part.is_empty() {
            None
        } else {
            Some(right_part.to_string())
        };

        let lexer = Lexer::new(left_part);
        if let Ok(tokens) = lexer.lex() {
            let parser = Parser::new(tokens);
            if let Ok(expr) = parser.parse() {
                let raw_prefix = line_text[..line_text.find("=>").unwrap() + 2].to_string();
                return Line::Evaluation {
                    expr,
                    raw_prefix,
                    current_result,
                };
            }
        }
    }

    // Default: treated as raw text / markdown
    Line::Text(line_text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::lexer::Lexer;

    fn parse_str(input: &str) -> Expr {
        let tokens = Lexer::new(input).lex().unwrap();
        Parser::new(tokens).parse().unwrap()
    }

    #[test]
    fn test_precedence() {
        assert_eq!(
            parse_str("10 + 20 * 30"),
            Expr::BinaryOp(
                Op::Add,
                Box::new(Expr::Number(10.0)),
                Box::new(Expr::BinaryOp(
                    Op::Mul,
                    Box::new(Expr::Number(20.0)),
                    Box::new(Expr::Number(30.0))
                ))
            )
        );

        assert_eq!(
            parse_str("(10 + 20) * 30"),
            Expr::BinaryOp(
                Op::Mul,
                Box::new(Expr::BinaryOp(
                    Op::Add,
                    Box::new(Expr::Number(10.0)),
                    Box::new(Expr::Number(20.0))
                )),
                Box::new(Expr::Number(30.0))
            )
        );
    }

    #[test]
    fn test_units_parsing() {
        assert_eq!(
            parse_str("10m + 50cm"),
            Expr::BinaryOp(
                Op::Add,
                Box::new(Expr::Quantity(10.0, "m".to_string())),
                Box::new(Expr::Quantity(50.0, "cm".to_string()))
            )
        );
    }

    #[test]
    fn test_standalone_units_parsing() {
        assert_eq!(
            parse_str("m"),
            Expr::Variable("m".to_string())
        );
        assert_eq!(
            parse_str("10 miles / gallon"),
            Expr::BinaryOp(
                Op::Div,
                Box::new(Expr::Quantity(10.0, "miles".to_string())),
                Box::new(Expr::Variable("gallon".to_string()))
            )
        );
        assert_eq!(
            parse_str("cows"),
            Expr::Variable("cows".to_string())
        );
        assert_eq!(
            parse_str("$"),
            Expr::Quantity(1.0, "$".to_string())
        );
    }

    #[test]
    fn test_currency_parsing() {
        assert_eq!(
            parse_str("$100 in EUR"),
            Expr::Convert(
                Box::new(Expr::Quantity(100.0, "$".to_string())),
                "EUR".to_string()
            )
        );
        assert_eq!(
            parse_str("cost in $/week"),
            Expr::Convert(
                Box::new(Expr::Variable("cost".to_string())),
                "$/week".to_string()
            )
        );
    }

    #[test]
    fn test_parse_line() {
        // Variable Assignment
        let l1 = parse_line("price = 100 + 50");
        if let Line::Assignment { name, expr, .. } = l1 {
            assert_eq!(name, "price");
            assert_eq!(
                expr,
                Expr::BinaryOp(
                    Op::Add,
                    Box::new(Expr::Number(100.0)),
                    Box::new(Expr::Number(50.0))
                )
            );
        } else {
            panic!("Expected assignment");
        }

        // Evaluation
        let l2 = parse_line("price * 2 => 300");
        if let Line::Evaluation { expr, current_result, .. } = l2 {
            assert_eq!(current_result, Some("300".to_string()));
            assert_eq!(
                expr,
                Expr::BinaryOp(
                    Op::Mul,
                    Box::new(Expr::Variable("price".to_string())),
                    Box::new(Expr::Number(2.0))
                )
            );
        } else {
            panic!("Expected evaluation");
        }

        // Plain Markdown text
        let l3 = parse_line("# Monthly Report");
        assert_eq!(l3, Line::Text("# Monthly Report".to_string()));
    }
}
