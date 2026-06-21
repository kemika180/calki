pub mod lexer;
pub mod parser;
pub mod eval;
pub mod units;

use crate::math::parser::Line;
use std::collections::HashMap;

// High-level sheet evaluator
// Evaluates the entire document line-by-line, updating `=>` evaluations
// and compiling the active variable registry.
pub fn evaluate_sheet(
    sheet_text: &str,
    exchange_rates: &HashMap<String, f64>,
) -> (String, Vec<(String, String)>) {
    units::clear_custom_units();

    let mut ctx = eval::Context::default();
    ctx.exchange_rates = exchange_rates.clone();

    let mut updated_lines = Vec::new();
    let mut vars_inspector = Vec::new();

    for line_text in sheet_text.lines() {
        let line = parser::parse_line(line_text);

        match line {
            Line::Text(text) => {
                // Scan and evaluate inline math within backticks: `expr =>`
                let evaluated = evaluate_inline_math(&text, &mut ctx);
                updated_lines.push(evaluated);
            }
            Line::Assignment { name, expr, raw_prefix, current_result } => {
                let is_explicit = eval::is_explicit_conversion(&expr, &ctx);
                match eval::eval_and_scale(&expr, &mut ctx) {
                    Ok(qty) => {
                        let formatted = eval::format_quantity(&qty);
                        if let Some(pos) = vars_inspector.iter().position(|(n, _)| n == &name) {
                            vars_inspector[pos] = (name.clone(), formatted.clone());
                        } else {
                            vars_inspector.push((name.clone(), formatted.clone()));
                        }
                        ctx.variables.insert(name.clone(), qty.clone());
                        if is_explicit {
                            ctx.explicit_variables.insert(name.clone());
                        } else {
                            ctx.explicit_variables.remove(&name);
                        }
                        if let Some(ref unit_str) = qty.unit {
                            let _ = units::register_custom_unit(&name, qty.value, unit_str);
                        }
                        if current_result.is_some() || raw_prefix.contains("=>") {
                            updated_lines.push(format!("{} {}", raw_prefix, formatted));
                        } else {
                            updated_lines.push(line_text.to_string());
                        }
                    }
                    Err(err) => {
                        let err_msg = format!("[Error: {}]", err);
                        if let Some(pos) = vars_inspector.iter().position(|(n, _)| n == &name) {
                            vars_inspector[pos] = (name.clone(), err_msg.clone());
                        } else {
                            vars_inspector.push((name.clone(), err_msg.clone()));
                        }
                        if current_result.is_some() || raw_prefix.contains("=>") {
                            updated_lines.push(format!("{} [Error: {}]", raw_prefix, err));
                        } else {
                            updated_lines.push(line_text.to_string());
                        }
                    }
                }
            }
            Line::FnDefinition { name, args, expr, raw_prefix: _ } => {
                ctx.functions.insert(name.clone(), (args.clone(), expr.clone()));
                let formatted_fn = format!("({}) = {}", args.join(", "), eval::expr_to_string(&expr));
                if let Some(pos) = vars_inspector.iter().position(|(n, _)| n == &name) {
                    vars_inspector[pos] = (name.clone(), formatted_fn);
                } else {
                    vars_inspector.push((name.clone(), formatted_fn));
                }
                updated_lines.push(line_text.to_string());
            }
            Line::Evaluation { expr, raw_prefix, .. } => {
                match eval::eval_and_scale(&expr, &mut ctx) {
                    Ok(qty) => {
                        let formatted = eval::format_quantity(&qty);
                        updated_lines.push(format!("{} {}", raw_prefix, formatted));
                    }
                    Err(err) => {
                        updated_lines.push(format!("{} [Error: {}]", raw_prefix, err));
                    }
                }
            }
        }
    }

    // Join with newlines (ensuring trailing newline behavior matches the input)
    let has_trailing_newline = sheet_text.ends_with('\n');
    let mut output = updated_lines.join("\n");
    if has_trailing_newline && !output.is_empty() {
        output.push('\n');
    }

    (output, vars_inspector)
}

fn evaluate_inline_math(text: &str, ctx: &mut eval::Context) -> String {
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
                    let expr_part = inner[..arrow_pos].trim();
                    let lexer = lexer::Lexer::new(expr_part);
                    if let Ok(tokens) = lexer.lex() {
                        let parser = parser::Parser::new(tokens);
                        if let Ok(expr) = parser.parse() {
                            match eval::eval_and_scale(&expr, ctx) {
                                Ok(qty) => {
                                    let formatted = eval::format_quantity(&qty);
                                    result.push_str(&format!("`{} => {}`", expr_part, formatted));
                                }
                                Err(err) => {
                                    result.push_str(&format!("`{} => [Error: {}]`", expr_part, err));
                                }
                            }
                        } else {
                            result.push_str(&format!("`{}`", inner)); // parse fail
                        }
                    } else {
                        result.push_str(&format!("`{}`", inner)); // lex fail
                    }
                } else {
                    result.push_str(&format!("`{}`", inner)); // no arrow
                }
            } else {
                result.push('`');
                result.push_str(&inner);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_sheet() {
        let sheet = r#"# Grocery Math
price = 6
quantity = 3
tax = 8.5%

total = price * quantity * (1 + tax)
total => 19.53

We bought items for `price * quantity =>` total.
"#;
        let rates = HashMap::new();
        let (output, vars) = evaluate_sheet(sheet, &rates);

        assert!(output.contains("total => 19.53"));
        assert!(output.contains("We bought items for `price * quantity => 18` total."));
        assert_eq!(vars.len(), 4);
        assert_eq!(vars[0], ("price".to_string(), "6".to_string()));

        // Test unit cancellation commute cost scenario
        let unit_sheet = r#"
mileage = 27 miles / 1 gallon
commute = 88 miles / 1 day
gas_cost = $4.09 / 1 gallon

cost = commute / mileage * gas_cost
cost =>
"#;
        let (unit_output, _) = evaluate_sheet(unit_sheet, &rates);
        assert!(unit_output.contains("cost => $13.3304/day"), "Actual output: {}", unit_output);

        // Test unit cancellation with standalone units (implied 1)
        let unit_sheet_2 = r#"
mileage = 27 miles / gallon
commute = 88 miles / day
gas_cost = $4.09 / gallon

cost = commute / mileage * gas_cost
cost =>
cost in $/week =>
cost * 5 days =>
"#;
        let (unit_output_2, _) = evaluate_sheet(unit_sheet_2, &rates);
        assert!(unit_output_2.contains("cost => $13.3304/day"), "Actual output: {}", unit_output_2);
        assert!(unit_output_2.contains("cost in $/week => $93.3126/week"), "Actual output: {}", unit_output_2);
        assert!(unit_output_2.contains("cost * 5 days => $66.6519"), "Actual output: {}", unit_output_2);

        // Test payment reproduction
        let payment_sheet = r#"
cost = $37000
apr = 1%
sales_tax = 4%
years = 6

months = years * 12

monthly = apr / 12 / 100
principal = cost * (1 + sales_tax)

payment = (monthly * principal) / (1 - (1 + monthly) ^ (-1 * months))
payment =>
"#;
        let (payment_output, _) = evaluate_sheet(payment_sheet, &rates);
        assert!(payment_output.contains("payment => $534.607"), "Actual output:\n{}", payment_output);

        // Test assignment with trailing arrow
        let assignment_arrow_sheet = r#"
a = 10 + 20 =>
b = a * 2 => 60
"#;
        let (arrow_output, _) = evaluate_sheet(assignment_arrow_sheet, &rates);
        assert!(arrow_output.contains("a = 10 + 20 => 30"), "Actual output:\n{}", arrow_output);
        assert!(arrow_output.contains("b = a * 2 => 60"), "Actual output:\n{}", arrow_output);

        // Test de-duplication of variables in inspector
        let dup_sheet = r#"
x = 5
y = 10
x = 15
"#;
        let (_, vars) = evaluate_sheet(dup_sheet, &rates);
        assert_eq!(vars.len(), 2);
        assert_eq!(vars[0], ("x".to_string(), "15".to_string()));
        assert_eq!(vars[1], ("y".to_string(), "10".to_string()));

        // Test energy units and auto-scaling
        let energy_sheet = r#"
e1 = 0.000001 J =>
e2 = 0.000001 J to J =>
e3 = 1500 J =>
e4 = 1500 J to J =>
e1 =>
e2 =>
e3 =>
e4 =>
"#;
        let (energy_output, _) = evaluate_sheet(energy_sheet, &rates);
        assert!(energy_output.contains("e1 = 0.000001 J => 1 uJ"), "Actual output:\n{}", energy_output);
        assert!(energy_output.contains("e2 = 0.000001 J to J => 0.000001 J"), "Actual output:\n{}", energy_output);
        assert!(energy_output.contains("e3 = 1500 J => 1.5 kJ"), "Actual output:\n{}", energy_output);
        assert!(energy_output.contains("e4 = 1500 J to J => 1500 J"), "Actual output:\n{}", energy_output);
        assert!(energy_output.contains("e1 => 1 uJ"), "Actual output:\n{}", energy_output);
        assert!(energy_output.contains("e2 => 0.000001 J"), "Actual output:\n{}", energy_output);

        // Test custom unit scaling/prefixing
        let custom_scale_sheet = r#"
A = 10 m
B = 1 MA to m =>
B to A =>
"#;
        let (custom_scale_output, _) = evaluate_sheet(custom_scale_sheet, &rates);
        assert!(custom_scale_output.contains("B = 1 MA to m => 10000000 m"), "Actual output:\n{}", custom_scale_output);
        assert!(custom_scale_output.contains("B to A => 1000000 A"), "Actual output:\n{}", custom_scale_output);
    }
}
