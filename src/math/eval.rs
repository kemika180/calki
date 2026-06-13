use crate::math::parser::{Expr, Op, Quantity};
use crate::math::units::{are_compatible, combine_units_with_multiplier, convert_quantity};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct Context {
    pub variables: HashMap<String, Quantity>,
    pub functions: HashMap<String, (Vec<String>, Expr)>,
    pub exchange_rates: HashMap<String, f64>,
}

impl Default for Context {
    fn default() -> Self {
        let mut variables = HashMap::new();
        variables.insert(
            "pi".to_string(),
            Quantity {
                value: std::f64::consts::PI,
                unit: None,
            },
        );
        variables.insert(
            "e".to_string(),
            Quantity {
                value: std::f64::consts::E,
                unit: None,
            },
        );

        Self {
            variables,
            functions: HashMap::new(),
            exchange_rates: HashMap::new(),
        }
    }
}

pub fn eval_expr(expr: &Expr, ctx: &mut Context) -> Result<Quantity, String> {
    match expr {
        Expr::Number(val) => Ok(Quantity {
            value: *val,
            unit: None,
        }),
        Expr::Quantity(val, unit) => Ok(Quantity {
            value: *val,
            unit: Some(unit.clone()),
        }),
        Expr::Variable(name) => {
            if let Some(val) = ctx.variables.get(name) {
                Ok(val.clone())
            } else {
                Ok(Quantity {
                    value: 1.0,
                    unit: Some(name.clone()),
                })
            }
        }
        Expr::Percentage(inner) => {
            let qty = eval_expr(inner, ctx)?;
            Ok(Quantity {
                value: qty.value * 0.01,
                unit: qty.unit,
            })
        }
        Expr::Convert(inner_expr, target_unit) => {
            let qty = eval_expr(inner_expr, ctx)?;
            let src_unit = qty.unit.ok_or_else(|| {
                format!(
                    "Cannot convert dimensionless value to unit '{}'",
                    target_unit
                )
            })?;
            let converted_val =
                convert_quantity(qty.value, &src_unit, target_unit, &ctx.exchange_rates)?;
            Ok(Quantity {
                value: converted_val,
                unit: Some(target_unit.clone()),
            })
        }
        Expr::FnCall(name, args) => {
            // Evaluate arguments
            let mut arg_vals = Vec::new();
            for arg in args {
                arg_vals.push(eval_expr(arg, ctx)?);
            }

            // Check built-in functions
            match name.as_str() {
                "sin" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity {
                        value: arg_vals[0].value.sin(),
                        unit: None,
                    })
                }
                "cos" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity {
                        value: arg_vals[0].value.cos(),
                        unit: None,
                    })
                }
                "tan" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity {
                        value: arg_vals[0].value.tan(),
                        unit: None,
                    })
                }
                "log" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity {
                        value: arg_vals[0].value.log10(),
                        unit: None,
                    })
                }
                "ln" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity {
                        value: arg_vals[0].value.ln(),
                        unit: None,
                    })
                }
                "sqrt" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if arg_vals[0].value < 0.0 {
                        return Err("Cannot compute square root of a negative number".to_string());
                    }
                    Ok(Quantity {
                        value: arg_vals[0].value.sqrt(),
                        unit: arg_vals[0].unit.clone(), // sqrt(9m^2) would ideally be 3m, but simply forward unit
                    })
                }
                "abs" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity {
                        value: arg_vals[0].value.abs(),
                        unit: arg_vals[0].unit.clone(),
                    })
                }
                "round" => {
                    if arg_vals.len() != 1 && arg_vals.len() != 2 {
                        return Err("Function 'round' expects 1 or 2 arguments".to_string());
                    }
                    let value = arg_vals[0].value;
                    let digits = if arg_vals.len() == 2 {
                        if arg_vals[1].unit.is_some() {
                            return Err("Second argument of 'round' (precision) must be dimensionless".to_string());
                        }
                        arg_vals[1].value as i32
                    } else {
                        0
                    };
                    let factor = 10.0f64.powi(digits);
                    let rounded = (value * factor).round() / factor;
                    Ok(Quantity {
                        value: rounded,
                        unit: arg_vals[0].unit.clone(),
                    })
                }
                "ceil" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity {
                        value: arg_vals[0].value.ceil(),
                        unit: arg_vals[0].unit.clone(),
                    })
                }
                "floor" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity {
                        value: arg_vals[0].value.floor(),
                        unit: arg_vals[0].unit.clone(),
                    })
                }
                "min" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let q1 = &arg_vals[0];
                    let q2 = &arg_vals[1];
                    let (u1, u2) = (&q1.unit, &q2.unit);
                    match (u1, u2) {
                        (Some(unit1), Some(unit2)) => {
                            if !are_compatible(unit1, unit2) {
                                return Err(format!("Incompatible units in min(): '{}' and '{}'", unit1, unit2));
                            }
                            let q2_val_converted = convert_quantity(q2.value, unit2, unit1, &ctx.exchange_rates)?;
                            let min_val = q1.value.min(q2_val_converted);
                            Ok(Quantity {
                                value: min_val,
                                unit: Some(unit1.clone()),
                            })
                        }
                        (None, None) => {
                            Ok(Quantity {
                                value: q1.value.min(q2.value),
                                unit: None,
                            })
                        }
                        _ => {
                            return Err("Cannot compare a quantity with a dimensionless value in min()".to_string());
                        }
                    }
                }
                "max" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let q1 = &arg_vals[0];
                    let q2 = &arg_vals[1];
                    let (u1, u2) = (&q1.unit, &q2.unit);
                    match (u1, u2) {
                        (Some(unit1), Some(unit2)) => {
                            if !are_compatible(unit1, unit2) {
                                return Err(format!("Incompatible units in max(): '{}' and '{}'", unit1, unit2));
                            }
                            let q2_val_converted = convert_quantity(q2.value, unit2, unit1, &ctx.exchange_rates)?;
                            let max_val = q1.value.max(q2_val_converted);
                            Ok(Quantity {
                                value: max_val,
                                unit: Some(unit1.clone()),
                            })
                        }
                        (None, None) => {
                            Ok(Quantity {
                                value: q1.value.max(q2.value),
                                unit: None,
                            })
                        }
                        _ => {
                            return Err("Cannot compare a quantity with a dimensionless value in max()".to_string());
                        }
                    }
                }
                "pmt" => {
                    check_built_in_args(name, &arg_vals, 3)?;
                    let rate = arg_vals[0].value;
                    let nper = arg_vals[1].value;
                    let pv = arg_vals[2].value;
                    
                    if arg_vals[0].unit.is_some() {
                        return Err("First argument of 'pmt' (rate) must be dimensionless or percentage".to_string());
                    }
                    if arg_vals[1].unit.is_some() {
                        return Err("Second argument of 'pmt' (nper) must be dimensionless".to_string());
                    }
                    
                    let pmt_val = if rate == 0.0 {
                        -pv / nper
                    } else {
                        -(rate * pv) / (1.0 - (1.0 + rate).powf(-nper))
                    };
                    
                    Ok(Quantity {
                        value: pmt_val,
                        unit: arg_vals[2].unit.clone(),
                    })
                }
                _ => {
                    // Custom user-defined functions
                    let (params, body) = ctx
                        .functions
                        .get(name)
                        .ok_or_else(|| format!("Undefined function '{}'", name))?
                        .clone();

                    if params.len() != arg_vals.len() {
                        return Err(format!(
                            "Function '{}' expects {} arguments, found {}",
                            name,
                            params.len(),
                            arg_vals.len()
                        ));
                    }

                    // Save current variable scope to prevent leakage
                    let original_variables = ctx.variables.clone();

                    // Bind parameters to argument values
                    for (param_name, arg_qty) in params.iter().zip(arg_vals.into_iter()) {
                        ctx.variables.insert(param_name.clone(), arg_qty);
                    }

                    // Evaluate function body
                    let result = eval_expr(&body, ctx);

                    // Restore scope
                    ctx.variables = original_variables;

                    result
                }
            }
        }
        Expr::BinaryOp(op, left_expr, right_expr) => {
            // Contextual Percentage Check: e.g. 100 - 15%
            let is_right_percentage = matches!(**right_expr, Expr::Percentage(_));

            if (*op == Op::Add || *op == Op::Sub) && is_right_percentage {
                let left_qty = eval_expr(left_expr, ctx)?;
                // Evaluate the percentage as a fraction (e.g. 15% -> 0.15)
                let pct_qty = eval_expr(right_expr, ctx)?;
                
                let delta = left_qty.value * pct_qty.value;
                let final_val = match op {
                    Op::Add => left_qty.value + delta,
                    Op::Sub => left_qty.value - delta,
                    _ => unreachable!(),
                };

                return Ok(Quantity {
                    value: final_val,
                    unit: left_qty.unit,
                });
            }

            // Standard evaluation
            let left_qty = eval_expr(left_expr, ctx)?;
            let right_qty = eval_expr(right_expr, ctx)?;

            match op {
                Op::Add | Op::Sub => {
                    match (&left_qty.unit, &right_qty.unit) {
                        (None, None) => {
                            let value = match op {
                                Op::Add => left_qty.value + right_qty.value,
                                Op::Sub => left_qty.value - right_qty.value,
                                _ => unreachable!(),
                            };
                            Ok(Quantity { value, unit: None })
                        }
                        (Some(u1), Some(u2)) => {
                            if !are_compatible(u1, u2) {
                                return Err(format!(
                                    "Incompatible units: cannot add/subtract '{}' and '{}'",
                                    u1, u2
                                ));
                            }
                            // Convert right unit to left unit
                            let right_converted = convert_quantity(
                                right_qty.value,
                                u2,
                                u1,
                                &ctx.exchange_rates,
                            )?;
                            let value = match op {
                                Op::Add => left_qty.value + right_converted,
                                Op::Sub => left_qty.value - right_converted,
                                _ => unreachable!(),
                            };
                            Ok(Quantity {
                                value,
                                unit: Some(u1.clone()),
                            })
                        }
                        _ => Err("Cannot mix dimensionless values with dimensional units in addition/subtraction".to_string()),
                    }
                }
                Op::Mul => {
                    let (unit, multiplier) = combine_units_with_multiplier(
                        left_qty.unit.as_deref(),
                        right_qty.unit.as_deref(),
                        false,
                        &ctx.exchange_rates,
                    );
                    let value = left_qty.value * right_qty.value * multiplier;
                    Ok(Quantity { value, unit })
                }
                Op::Div => {
                    if right_qty.value == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    let (unit, multiplier) = combine_units_with_multiplier(
                        left_qty.unit.as_deref(),
                        right_qty.unit.as_deref(),
                        true,
                        &ctx.exchange_rates,
                    );
                    let value = (left_qty.value / right_qty.value) * multiplier;
                    Ok(Quantity { value, unit })
                }
                Op::Pow => {
                    if right_qty.unit.is_some() {
                        return Err("Exponent power must be a dimensionless scalar".to_string());
                    }
                    let value = left_qty.value.powf(right_qty.value);
                    Ok(Quantity {
                        value,
                        unit: left_qty.unit, // Simplified: assume unit stays unchanged (e.g. m^2 area needs derived unit support, but for now we forward unit)
                    })
                }
            }
        }
    }
}

fn check_built_in_args(name: &str, args: &[Quantity], expected: usize) -> Result<(), String> {
    if args.len() != expected {
        return Err(format!(
            "Built-in function '{}' expects {} arguments, found {}",
            name,
            expected,
            args.len()
        ));
    }
    Ok(())
}

// Formats a Quantity nicely for buffer output
pub fn format_quantity(qty: &Quantity) -> String {
    let rounded = if qty.value.fract() == 0.0 {
        format!("{}", qty.value as i64)
    } else {
        // limit to 4 decimal places
        format!("{:.4}", qty.value)
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    };

    match &qty.unit {
        Some(u) => {
            if u == "$" {
                format!("${}", rounded) // prefix format
            } else {
                let starts_with_word = u.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false);
                if starts_with_word {
                    format!("{} {}", rounded, u) // postfix format with space for words
                } else {
                    format!("{}{}", rounded, u) // postfix format without space for symbols
                }
            }
        }
        None => rounded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::parser::{parse_line, Line};

    #[test]
    fn test_eval_basic() {
        let mut ctx = Context::default();
        let e1 = parse_line("x = 10");
        if let Line::Assignment { name, expr, .. } = e1 {
            let val = eval_expr(&expr, &mut ctx).unwrap();
            ctx.variables.insert(name, val);
        }

        let e2 = parse_line("x * 5 =>");
        if let Line::Evaluation { expr, .. } = e2 {
            let res = eval_expr(&expr, &mut ctx).unwrap();
            assert_eq!(res.value, 50.0);
        }
    }

    #[test]
    fn test_percentage_subtraction() {
        let mut ctx = Context::default();
        let e = parse_line("100 - 15% =>");
        if let Line::Evaluation { expr, .. } = e {
            let res = eval_expr(&expr, &mut ctx).unwrap();
            assert_eq!(res.value, 85.0);
        }
    }

    #[test]
    fn test_function_evaluation() {
        let mut ctx = Context::default();
        let def = parse_line("f(x) = x^2 + 10");
        if let Line::FnDefinition { name, args, expr, .. } = def {
            ctx.functions.insert(name, (args, expr));
        }

        let eval = parse_line("f(5) =>");
        if let Line::Evaluation { expr, .. } = eval {
            let res = eval_expr(&expr, &mut ctx).unwrap();
            assert_eq!(res.value, 35.0);
        }
    }

    #[test]
    fn test_new_functions() {
        let mut ctx = Context::default();
        
        // round
        let r1 = eval_expr(&parse_line("round(3.14159, 2) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(r1.value, 3.14);
        let r2 = eval_expr(&parse_line("round(3.8) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(r2.value, 4.0);

        // ceil and floor
        let c = eval_expr(&parse_line("ceil(4.1) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(c.value, 5.0);
        let f = eval_expr(&parse_line("floor(4.9) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(f.value, 4.0);

        // min and max
        let mn = eval_expr(&parse_line("min(10, 20) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(mn.value, 10.0);
        let mx = eval_expr(&parse_line("max(10, 20) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(mx.value, 20.0);

        // pmt
        let p = eval_expr(&parse_line("pmt(0.05 / 12, 60, -20000) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((p.value - 377.424).abs() < 1e-2);
    }
}

// Helper trait to easily unwrap Line to Expr in tests
#[cfg(test)]
trait LineExt {
    fn unwrap_expr(self) -> Expr;
}
#[cfg(test)]
impl LineExt for crate::math::parser::Line {
    fn unwrap_expr(self) -> Expr {
        match self {
            crate::math::parser::Line::Evaluation { expr, .. } => expr,
            crate::math::parser::Line::Assignment { expr, .. } => expr,
            _ => panic!("Not an expression line"),
        }
    }
}
