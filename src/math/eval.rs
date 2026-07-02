use crate::math::parser::{Expr, Op, Quantity};
use crate::math::units::{are_compatible, combine_units_with_multiplier, convert_quantity};
use std::collections::HashMap;

fn is_complex(qty: &Quantity) -> bool {
    qty.unit.as_deref() == Some("i") || (qty.unit.as_deref() == Some("complex") && qty.list.is_some())
}

fn to_complex_parts(qty: &Quantity) -> (f64, f64) {
    if qty.unit.as_deref() == Some("i") {
        (0.0, qty.value)
    } else if qty.unit.as_deref() == Some("complex") {
        if let Some(ref list) = qty.list
            && list.len() >= 2 {
                return (list[0].value, list[1].value);
            }
        (qty.value, 0.0)
    } else {
        (qty.value, 0.0)
    }
}

fn make_complex_qty(re: f64, im: f64) -> Quantity {
    if im == 0.0 {
        Quantity { value: re, unit: None, list: None, is_bool: false }
    } else if re == 0.0 {
        Quantity { value: im, unit: Some("i".to_string()), list: None, is_bool: false }
    } else {
        Quantity {
            value: re,
            unit: Some("complex".to_string()),
            list: Some(vec![
                Quantity { value: re, unit: None, list: None, is_bool: false },
                Quantity { value: im, unit: Some("i".to_string()), list: None, is_bool: false },
            ]),
            is_bool: false,
        }
    }
}

fn differentiate(expr: &Expr, var: &str) -> Result<Expr, String> {
    match expr {
        Expr::Number(_) => Ok(Expr::Number(0.0)),
        Expr::Quantity(_, _) => Ok(Expr::Number(0.0)),
        Expr::Variable(name) => {
            if name == var {
                Ok(Expr::Number(1.0))
            } else {
                Ok(Expr::Number(0.0))
            }
        }
        Expr::Percentage(inner) => {
            let d_inner = differentiate(inner, var)?;
            Ok(Expr::Percentage(Box::new(d_inner)))
        }
        Expr::BinaryOp(op, left, right) => {
            match op {
                Op::Add => {
                    let dl = differentiate(left, var)?;
                    let dr = differentiate(right, var)?;
                    Ok(Expr::BinaryOp(Op::Add, Box::new(dl), Box::new(dr)))
                }
                Op::Sub => {
                    let dl = differentiate(left, var)?;
                    let dr = differentiate(right, var)?;
                    Ok(Expr::BinaryOp(Op::Sub, Box::new(dl), Box::new(dr)))
                }
                Op::Mul => {
                    let dl = differentiate(left, var)?;
                    let dr = differentiate(right, var)?;
                    Ok(Expr::BinaryOp(Op::Add,
                        Box::new(Expr::BinaryOp(Op::Mul, Box::new(dl), right.clone())),
                        Box::new(Expr::BinaryOp(Op::Mul, left.clone(), Box::new(dr)))
                    ))
                }
                Op::Div => {
                    let dl = differentiate(left, var)?;
                    let dr = differentiate(right, var)?;
                    Ok(Expr::BinaryOp(Op::Div,
                        Box::new(Expr::BinaryOp(Op::Sub,
                            Box::new(Expr::BinaryOp(Op::Mul, Box::new(dl), right.clone())),
                            Box::new(Expr::BinaryOp(Op::Mul, left.clone(), Box::new(dr)))
                        )),
                        Box::new(Expr::BinaryOp(Op::Pow, right.clone(), Box::new(Expr::Number(2.0))))
                    ))
                }
                Op::Pow => {
                    let left_has = expr_contains_var(left, var);
                    let right_has = expr_contains_var(right, var);
                    if left_has && !right_has {
                        let du = differentiate(left, var)?;
                        Ok(Expr::BinaryOp(Op::Mul,
                            Box::new(Expr::BinaryOp(Op::Mul,
                                right.clone(),
                                Box::new(Expr::BinaryOp(Op::Pow,
                                    left.clone(),
                                    Box::new(Expr::BinaryOp(Op::Sub, right.clone(), Box::new(Expr::Number(1.0))))
                                ))
                            )),
                            Box::new(du)
                        ))
                    } else if !left_has && right_has {
                        let du = differentiate(right, var)?;
                        Ok(Expr::BinaryOp(Op::Mul,
                            Box::new(Expr::BinaryOp(Op::Mul,
                                Box::new(expr.clone()),
                                Box::new(Expr::FnCall("ln".to_string(), vec![*left.clone()]))
                            )),
                            Box::new(du)
                        ))
                    } else if left_has && right_has {
                        let du = differentiate(left, var)?;
                        let dv = differentiate(right, var)?;
                        let term1 = Expr::BinaryOp(Op::Mul, Box::new(dv), Box::new(Expr::FnCall("ln".to_string(), vec![*left.clone()])));
                        let term2 = Expr::BinaryOp(Op::Div,
                            Box::new(Expr::BinaryOp(Op::Mul, right.clone(), Box::new(du))),
                            left.clone()
                        );
                        Ok(Expr::BinaryOp(Op::Mul,
                            Box::new(expr.clone()),
                            Box::new(Expr::BinaryOp(Op::Add, Box::new(term1), Box::new(term2)))
                        ))
                    } else {
                        Ok(Expr::Number(0.0))
                    }
                }
                _ => Err(format!("Cannot differentiate operation {:?}", op)),
            }
        }
        Expr::FnCall(name, args) => {
            if args.len() != 1 {
                return Err("Differentiating multi-argument functions is not supported".to_string());
            }
            let u = &args[0];
            let du = differentiate(u, var)?;
            match name.as_str() {
                "sin" => {
                    Ok(Expr::BinaryOp(Op::Mul,
                        Box::new(Expr::FnCall("cos".to_string(), vec![u.clone()])),
                        Box::new(du)
                    ))
                }
                "cos" => {
                    Ok(Expr::BinaryOp(Op::Mul,
                        Box::new(Expr::BinaryOp(Op::Sub,
                            Box::new(Expr::Number(0.0)),
                            Box::new(Expr::FnCall("sin".to_string(), vec![u.clone()]))
                        )),
                        Box::new(du)
                    ))
                }
                "exp" => {
                    Ok(Expr::BinaryOp(Op::Mul,
                        Box::new(Expr::FnCall("exp".to_string(), vec![u.clone()])),
                        Box::new(du)
                    ))
                }
                "ln" | "log" => {
                    Ok(Expr::BinaryOp(Op::Div, Box::new(du), Box::new(u.clone())))
                }
                _ => Err(format!("Differentiating function '{}' is not supported", name)),
            }
        }
        Expr::Convert(inner, unit) => {
            let d_inner = differentiate(inner, var)?;
            Ok(Expr::Convert(Box::new(d_inner), unit.clone()))
        }
        Expr::List(elements) => {
            let mut d_elements = Vec::new();
            for el in elements {
                d_elements.push(differentiate(el, var)?);
            }
            Ok(Expr::List(d_elements))
        }
        _ => Err("Unsupported expression for differentiation".to_string()),
    }
}

fn simplify(expr: &Expr) -> Expr {
    match expr {
        Expr::BinaryOp(op, left, right) => {
            let sl = simplify(left);
            let sr = simplify(right);
            match op {
                Op::Add => {
                    match (&sl, &sr) {
                        (Expr::Number(n), _) if *n == 0.0 => sr,
                        (_, Expr::Number(n)) if *n == 0.0 => sl,
                        (Expr::Number(a), Expr::Number(b)) => Expr::Number(a + b),
                        (left, Expr::BinaryOp(Op::Sub, zero, right)) => {
                            if let Expr::Number(n) = &**zero {
                                if *n == 0.0 {
                                    Expr::BinaryOp(Op::Sub, Box::new(left.clone()), right.clone())
                                } else {
                                    Expr::BinaryOp(Op::Add, Box::new(sl), Box::new(sr))
                                }
                            } else {
                                Expr::BinaryOp(Op::Add, Box::new(sl), Box::new(sr))
                            }
                        }
                        _ => Expr::BinaryOp(Op::Add, Box::new(sl), Box::new(sr)),
                    }
                }
                Op::Sub => {
                    match (&sl, &sr) {
                        (_, Expr::Number(n)) if *n == 0.0 => sl,
                        (Expr::Number(a), Expr::Number(b)) => Expr::Number(a - b),
                        (left, Expr::BinaryOp(Op::Sub, zero, right)) => {
                            if let Expr::Number(n) = &**zero {
                                if *n == 0.0 {
                                    Expr::BinaryOp(Op::Add, Box::new(left.clone()), right.clone())
                                } else {
                                    Expr::BinaryOp(Op::Sub, Box::new(sl), Box::new(sr))
                                }
                            } else {
                                Expr::BinaryOp(Op::Sub, Box::new(sl), Box::new(sr))
                            }
                        }
                        _ => Expr::BinaryOp(Op::Sub, Box::new(sl), Box::new(sr)),
                    }
                }
                Op::Mul => {
                    match (&sl, &sr) {
                        (Expr::Number(n), _) if *n == 0.0 => Expr::Number(0.0),
                        (_, Expr::Number(n)) if *n == 0.0 => Expr::Number(0.0),
                        (Expr::Number(n), _) if *n == 1.0 => sr,
                        (_, Expr::Number(n)) if *n == 1.0 => sl,
                        (Expr::Number(a), Expr::Number(b)) => Expr::Number(a * b),
                        _ => Expr::BinaryOp(Op::Mul, Box::new(sl), Box::new(sr)),
                    }
                }
                Op::Div => {
                    match (&sl, &sr) {
                        (Expr::Number(n), _) if *n == 0.0 => Expr::Number(0.0),
                        (_, Expr::Number(n)) if *n == 1.0 => sl,
                        (Expr::Number(a), Expr::Number(b)) if *b != 0.0 => Expr::Number(a / b),
                        _ => Expr::BinaryOp(Op::Div, Box::new(sl), Box::new(sr)),
                    }
                }
                Op::Pow => {
                    match (&sl, &sr) {
                        (_, Expr::Number(n)) if *n == 0.0 => Expr::Number(1.0),
                        (_, Expr::Number(n)) if *n == 1.0 => sl,
                        (Expr::Number(n), _) if *n == 1.0 => Expr::Number(1.0),
                        (Expr::Number(a), Expr::Number(b)) => Expr::Number(a.powf(*b)),
                        _ => Expr::BinaryOp(Op::Pow, Box::new(sl), Box::new(sr)),
                    }
                }
                _ => Expr::BinaryOp(*op, Box::new(sl), Box::new(sr)),
            }
        }
        Expr::Percentage(inner) => {
            let si = simplify(inner);
            match si {
                Expr::Number(n) => Expr::Number(n * 0.01),
                _ => Expr::Percentage(Box::new(si)),
            }
        }
        Expr::FnCall(name, args) => {
            let s_args = args.iter().map(simplify).collect();
            Expr::FnCall(name.clone(), s_args)
        }
        _ => expr.clone(),
    }
}

fn get_op_precedence(op: &Op) -> u8 {
    match op {
        Op::Or => 1,
        Op::And => 2,
        Op::BitOr => 3,
        Op::BitAnd => 4,
        Op::Eq | Op::Ne | Op::Less | Op::LessEq | Op::Greater | Op::GreaterEq => 5,
        Op::LShift | Op::RShift => 6,
        Op::Add | Op::Sub => 7,
        Op::Mul | Op::Div | Op::Mod => 8,
        Op::Pow => 9,
    }
}

pub(crate) fn expr_to_string(expr: &Expr) -> String {
    match expr {
        Expr::Number(val) => {
            if val.fract() == 0.0 {
                format!("{}", *val as i64)
            } else {
                format!("{:.4}", val).trim_end_matches('0').trim_end_matches('.').to_string()
            }
        }
        Expr::Quantity(val, unit) => {
            let rounded = if val.fract() == 0.0 {
                format!("{}", *val as i64)
            } else {
                format!("{:.4}", val).trim_end_matches('0').trim_end_matches('.').to_string()
            };
            format!("{}{}", rounded, unit)
        }
        Expr::Variable(name) => name.clone(),
        Expr::Percentage(inner) => format!("{}%", expr_to_string(inner)),
        Expr::BinaryOp(op, left, right) => {
            if *op == Op::Sub
                && let Expr::Number(n) = &**left
                    && *n == 0.0 {
                        let right_precedence = match &**right {
                            Expr::BinaryOp(right_op, _, _) => get_op_precedence(right_op),
                            _ => 100,
                        };
                        let right_str = if right_precedence < 7 {
                            format!("({})", expr_to_string(right))
                        } else {
                            expr_to_string(right)
                        };
                        return format!("-{}", right_str);
                    }

            let op_str = match op {
                Op::Add => " + ",
                Op::Sub => " - ",
                Op::Mul => " * ",
                Op::Div => " / ",
                Op::Pow => "^",
                Op::Mod => " % ",
                Op::BitAnd => " & ",
                Op::BitOr => " | ",
                Op::LShift => " << ",
                Op::RShift => " >> ",
                Op::Eq => " == ",
                Op::Ne => " != ",
                Op::Less => " < ",
                Op::LessEq => " <= ",
                Op::Greater => " > ",
                Op::GreaterEq => " >= ",
                Op::And => " and ",
                Op::Or => " or ",
            };

            let parent_prec = get_op_precedence(op);

            let left_str = match &**left {
                Expr::BinaryOp(left_op, _, _) => {
                    if get_op_precedence(left_op) < parent_prec {
                        format!("({})", expr_to_string(left))
                    } else {
                        expr_to_string(left)
                    }
                }
                _ => expr_to_string(left),
            };

            let right_str = match &**right {
                Expr::BinaryOp(right_op, _, _) => {
                    let is_pow = *op == Op::Pow;
                    let right_prec = get_op_precedence(right_op);
                    if right_prec < parent_prec || (right_prec == parent_prec && !is_pow) {
                        format!("({})", expr_to_string(right))
                    } else {
                        expr_to_string(right)
                    }
                }
                _ => expr_to_string(right),
            };

            format!("{}{}{}", left_str, op_str, right_str)
        }
        Expr::FnCall(name, args) => {
            let args_str: Vec<String> = args.iter().map(expr_to_string).collect();
            format!("{}({})", name, args_str.join(", "))
        }
        Expr::Convert(inner, unit) => {
            format!("{} in {}", expr_to_string(inner), unit)
        }
        Expr::List(elements) => {
            let els: Vec<String> = elements.iter().map(expr_to_string).collect();
            format!("[{}]", els.join(", "))
        }
        Expr::Not(inner) => format!("not {}", expr_to_string(inner)),
        Expr::BitNot(inner) => format!("~{}", expr_to_string(inner)),
        Expr::Block(exprs) => {
            let els: Vec<String> = exprs.iter().map(expr_to_string).collect();
            format!("{{\n  {}\n}}", els.join("\n  "))
        }
        Expr::LocalAssign(name, val_expr) => {
            format!("{} = {}", name, expr_to_string(val_expr))
        }
        Expr::IfElse { cond, then_expr, else_expr } => {
            format!("if {} {} else {}", expr_to_string(cond), expr_to_string(then_expr), expr_to_string(else_expr))
        }
        Expr::Switch { val, cases, default_case } => {
            let mut cases_strs = Vec::new();
            for (pattern, body) in cases {
                cases_strs.push(format!("{} => {}", expr_to_string(pattern), expr_to_string(body)));
            }
            if let Some(def) = default_case {
                cases_strs.push(format!("default => {}", expr_to_string(def)));
            }
            format!("switch {} {{\n  {}\n}}", expr_to_string(val), cases_strs.join("\n  "))
        }
        Expr::StringLiteral(val) => {
            format!("\"{}\"", val)
        }
    }
}

fn flatten_quantity(qty: &Quantity, target: &mut Vec<Quantity>) {
    if let Some(ref elements) = qty.list {
        for el in elements {
            flatten_quantity(el, target);
        }
    } else {
        target.push(qty.clone());
    }
}

fn quantity_add(q1: &Quantity, q2: &Quantity, ctx: &Context) -> Result<Quantity, String> {
    match (&q1.list, &q2.list) {
        (Some(el1), Some(el2)) => {
            if el1.len() != el2.len() {
                return Err(format!("Dimension mismatch in vadd: lengths {} and {}", el1.len(), el2.len()));
            }
            let mut result_elements = Vec::new();
            for (x1, x2) in el1.iter().zip(el2.iter()) {
                result_elements.push(quantity_add(x1, x2, ctx)?);
            }
            Ok(Quantity::list(result_elements))
        }
        (None, None) => {
            match (&q1.unit, &q2.unit) {
                (None, None) => {
                    Ok(Quantity { is_bool: false, list: None, value: q1.value + q2.value, unit: None })
                }
                (Some(u1), Some(u2)) => {
                    if !are_compatible(u1, u2) {
                        return Err(format!("Incompatible units in vadd: cannot add '{}' and '{}'", u1, u2));
                    }
                    let right_converted = convert_quantity(q2.value, u2, u1, &ctx.exchange_rates)?;
                    Ok(Quantity { is_bool: false,
                        list: None,
                        value: q1.value + right_converted,
                        unit: Some(u1.clone()),
                    })
                }
                _ => Err("Cannot mix dimensionless values with dimensional units in vadd".to_string()),
            }
        }
        _ => Err("Cannot add a list and a scalar".to_string()),
    }
}

fn quantity_sub(q1: &Quantity, q2: &Quantity, ctx: &Context) -> Result<Quantity, String> {
    match (&q1.list, &q2.list) {
        (Some(el1), Some(el2)) => {
            if el1.len() != el2.len() {
                return Err(format!("Dimension mismatch in vsub: lengths {} and {}", el1.len(), el2.len()));
            }
            let mut result_elements = Vec::new();
            for (x1, x2) in el1.iter().zip(el2.iter()) {
                result_elements.push(quantity_sub(x1, x2, ctx)?);
            }
            Ok(Quantity::list(result_elements))
        }
        (None, None) => {
            match (&q1.unit, &q2.unit) {
                (None, None) => {
                    Ok(Quantity { is_bool: false, list: None, value: q1.value - q2.value, unit: None })
                }
                (Some(u1), Some(u2)) => {
                    if !are_compatible(u1, u2) {
                        return Err(format!("Incompatible units in vsub: cannot subtract '{}' and '{}'", u1, u2));
                    }
                    let right_converted = convert_quantity(q2.value, u2, u1, &ctx.exchange_rates)?;
                    Ok(Quantity { is_bool: false,
                        list: None,
                        value: q1.value - right_converted,
                        unit: Some(u1.clone()),
                    })
                }
                _ => Err("Cannot mix dimensionless values with dimensional units in vsub".to_string()),
            }
        }
        _ => Err("Cannot subtract a list and a scalar".to_string()),
    }
}

fn quantity_mul(left_qty: &Quantity, right_qty: &Quantity, ctx: &Context) -> Result<Quantity, String> {
    let (unit, multiplier) = combine_units_with_multiplier(
        left_qty.unit.as_deref(),
        right_qty.unit.as_deref(),
        false,
        &ctx.exchange_rates,
    );
    let value = left_qty.value * right_qty.value * multiplier;
    Ok(Quantity { is_bool: false, list: None, value, unit })
}

fn quantity_div(left_qty: &Quantity, right_qty: &Quantity, ctx: &Context) -> Result<Quantity, String> {
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
    Ok(Quantity { is_bool: false, list: None, value, unit })
}

fn quantity_pow(left_qty: &Quantity, right_qty: &Quantity) -> Result<Quantity, String> {
    if right_qty.unit.is_some() {
        return Err("Exponent power must be a dimensionless scalar".to_string());
    }
    let value = left_qty.value.powf(right_qty.value);
    Ok(Quantity { is_bool: false, list: None,
        value,
        unit: left_qty.unit.clone(),
    })
}

fn expr_contains_var(expr: &Expr, var_name: &str) -> bool {
    match expr {
        Expr::Variable(name) => name == var_name,
        Expr::Percentage(inner) | Expr::Not(inner) | Expr::BitNot(inner) | Expr::Convert(inner, _) => {
            expr_contains_var(inner, var_name)
        }
        Expr::BinaryOp(_, left, right) => {
            expr_contains_var(left, var_name) || expr_contains_var(right, var_name)
        }
        Expr::FnCall(_, args) | Expr::List(args) => {
            args.iter().any(|arg| expr_contains_var(arg, var_name))
        }
        Expr::Number(_) | Expr::Quantity(_, _) | Expr::StringLiteral(_) => false,
        Expr::Block(exprs) => {
            exprs.iter().any(|e| expr_contains_var(e, var_name))
        }
        Expr::LocalAssign(name, val_expr) => {
            name == var_name || expr_contains_var(val_expr, var_name)
        }
        Expr::IfElse { cond, then_expr, else_expr } => {
            expr_contains_var(cond, var_name) || expr_contains_var(then_expr, var_name) || expr_contains_var(else_expr, var_name)
        }
        Expr::Switch { val, cases, default_case } => {
            expr_contains_var(val, var_name) || 
            cases.iter().any(|(pat, body)| expr_contains_var(pat, var_name) || expr_contains_var(body, var_name)) ||
            default_case.as_ref().map_or(false, |def| expr_contains_var(def, var_name))
        }
    }
}

fn solve_equation(expr: &Expr, var_name: &str, ctx: &mut Context) -> Result<Quantity, String> {
    match expr {
        Expr::BinaryOp(Op::Eq, left, right) => {
            let left_has = expr_contains_var(left, var_name);
            let right_has = expr_contains_var(right, var_name);
            if left_has && !right_has {
                let target_val = eval_expr(right, ctx)?;
                solve_rec(left, target_val, var_name, ctx)
            } else if right_has && !left_has {
                let target_val = eval_expr(left, ctx)?;
                solve_rec(right, target_val, var_name, ctx)
            } else if !left_has && !right_has {
                Err("Equation does not contain the variable to solve for".to_string())
            } else {
                Err("Variable appears on both sides of the equation, which is not supported by the simple solver".to_string())
            }
        }
        _ => {
            // Solve expr == 0
            let target_val = Quantity { is_bool: false, list: None, value: 0.0, unit: None };
            solve_rec(expr, target_val, var_name, ctx)
        }
    }
}

fn solve_rec(expr: &Expr, target_val: Quantity, var_name: &str, ctx: &mut Context) -> Result<Quantity, String> {
    match expr {
        Expr::Variable(name) if name == var_name => {
            Ok(target_val)
        }
        Expr::BinaryOp(op, left, right) => {
            let left_has = expr_contains_var(left, var_name);
            let right_has = expr_contains_var(right, var_name);
            if left_has && !right_has {
                let r_val = eval_expr(right, ctx)?;
                let next_target = match op {
                    Op::Add => {
                        quantity_sub(&target_val, &r_val, ctx)?
                    }
                    Op::Sub => {
                        quantity_add(&target_val, &r_val, ctx)?
                    }
                    Op::Mul => {
                        quantity_div(&target_val, &r_val, ctx)?
                    }
                    Op::Div => {
                        quantity_mul(&target_val, &r_val, ctx)?
                    }
                    Op::Pow => {
                        let one_over_r = Quantity {
                            is_bool: false,
                            list: None,
                            value: 1.0 / r_val.value,
                            unit: None,
                        };
                        quantity_pow(&target_val, &one_over_r)?
                    }
                    _ => return Err(format!("Unsupported operator '{:?}' in equation solving", op)),
                };
                solve_rec(left, next_target, var_name, ctx)
            } else if right_has && !left_has {
                let l_val = eval_expr(left, ctx)?;
                let next_target = match op {
                    Op::Add => {
                        quantity_sub(&target_val, &l_val, ctx)?
                    }
                    Op::Sub => {
                        quantity_sub(&l_val, &target_val, ctx)?
                    }
                    Op::Mul => {
                        quantity_div(&target_val, &l_val, ctx)?
                    }
                    Op::Div => {
                        quantity_div(&l_val, &target_val, ctx)?
                    }
                    _ => return Err(format!("Unsupported operator '{:?}' in equation solving", op)),
                };
                solve_rec(right, next_target, var_name, ctx)
            } else if !left_has && !right_has {
                Err("Sub-expression does not contain the variable".to_string())
            } else {
                Err("Variable appears on both sides of a sub-expression".to_string())
            }
        }
        _ => Err("Equation is too complex or non-algebraic".to_string()),
    }
}

fn matmul_impl(q1: &Quantity, q2: &Quantity, ctx: &Context) -> Result<Quantity, String> {
    let el1 = q1.list.as_ref().ok_or("matmul expects first argument to be a list/matrix")?;
    let el2 = q2.list.as_ref().ok_or("matmul expects second argument to be a list/matrix")?;
    if el1.is_empty() || el2.is_empty() {
        return Err("Empty list/matrix is not allowed for matmul".to_string());
    }

    let q1_all_scalars = el1.iter().all(|el| el.list.is_none());
    let q2_all_scalars = el2.iter().all(|el| el.list.is_none());

    // Convert/interpret inputs
    let (matrix_a, treat_a_as_1d) = if q1_all_scalars {
        (vec![el1.clone()], true)
    } else {
        let mut mat = Vec::new();
        let first_len = el1[0].list.as_ref().map(|l| l.len()).unwrap_or(0);
        for row in el1 {
            let row_el = row.list.as_ref().ok_or("matmul expects a 2D matrix or 1D vector")?;
            if row_el.len() != first_len {
                return Err("Matrix rows must all have the same length".to_string());
            }
            mat.push(row_el.clone());
        }
        (mat, false)
    };

    let (matrix_b, treat_b_as_1d) = if q2_all_scalars {
        // Treat 1D list as a column vector (N x 1)
        let mut mat = Vec::new();
        for el in el2 {
            mat.push(vec![el.clone()]);
        }
        (mat, true)
    } else {
        let mut mat = Vec::new();
        let first_len = el2[0].list.as_ref().map(|l| l.len()).unwrap_or(0);
        for row in el2 {
            let row_el = row.list.as_ref().ok_or("matmul expects a 2D matrix or 1D vector")?;
            if row_el.len() != first_len {
                return Err("Matrix rows must all have the same length".to_string());
            }
            mat.push(row_el.clone());
        }
        (mat, false)
    };

    let rows_a = matrix_a.len();
    let cols_a = matrix_a[0].len();
    let rows_b = matrix_b.len();
    let cols_b = matrix_b[0].len();

    if cols_a != rows_b {
        return Err(format!(
            "Dimension mismatch in matmul: cannot multiply matrix of shape {}x{} by {}x{}",
            rows_a, cols_a, rows_b, cols_b
        ));
    }

    let mut result_matrix = vec![vec![Quantity::scalar(0.0, None); cols_b]; rows_a];

    for i in 0..rows_a {
        for j in 0..cols_b {
            let mut sum_val = 0.0;
            let mut sum_unit: Option<String> = None;
            for k in 0..cols_a {
                let q_a = &matrix_a[i][k];
                let q_b = &matrix_b[k][j];
                let (unit, multiplier) = combine_units_with_multiplier(
                    q_a.unit.as_deref(),
                    q_b.unit.as_deref(),
                    false,
                    &ctx.exchange_rates,
                );
                let term_val = q_a.value * q_b.value * multiplier;
                if k == 0 {
                    sum_val = term_val;
                    sum_unit = unit;
                } else {
                    match (&sum_unit, &unit) {
                        (Some(u1), Some(u2)) => {
                            if !are_compatible(u1, u2) {
                                return Err(format!(
                                    "Incompatible units in matmul cell summation: '{}' and '{}'",
                                    u1, u2
                                ));
                            }
                            let converted = convert_quantity(term_val, u2, u1, &ctx.exchange_rates)?;
                            sum_val += converted;
                        }
                        (None, None) => {
                            sum_val += term_val;
                        }
                        _ => {
                            return Err("Cannot mix dimensional and dimensionless values in matmul cell summation".to_string());
                        }
                    }
                }
            }
            result_matrix[i][j] = Quantity::scalar(sum_val, sum_unit);
        }
    }

    // Now format the result based on input dimensions
    if treat_a_as_1d && treat_b_as_1d {
        // 1D dot 1D -> scalar (this would be result_matrix[0][0])
        Ok(result_matrix[0][0].clone())
    } else if treat_a_as_1d {
        // 1D dot 2D -> 1D vector (result is 1 x cols_b, we return it as a list of length cols_b)
        Ok(Quantity::list(result_matrix[0].clone()))
    } else if treat_b_as_1d {
        // 2D dot 1D -> 1D vector (result is rows_a x 1, we return it as a list of length rows_a)
        let flat_res: Vec<Quantity> = result_matrix.into_iter().map(|row| row[0].clone()).collect();
        Ok(Quantity::list(flat_res))
    } else {
        // 2D dot 2D -> 2D matrix
        let row_quantities: Vec<Quantity> = result_matrix.into_iter().map(Quantity::list).collect();
        Ok(Quantity::list(row_quantities))
    }
}

fn eval_eq_logic(q1: &Quantity, q2: &Quantity, exchange_rates: &HashMap<String, f64>) -> bool {
    match (&q1.list, &q2.list) {
        (Some(l1), Some(l2)) => {
            if l1.len() != l2.len() {
                return false;
            }
            for (el1, el2) in l1.iter().zip(l2.iter()) {
                if !eval_eq_logic(el1, el2, exchange_rates) {
                    return false;
                }
            }
            true
        }
        (None, None) => {
            match (&q1.unit, &q2.unit) {
                (Some(u1), Some(u2))
                    if are_compatible(u1, u2) => {
                        if let Ok(q2_conv) = convert_quantity(q2.value, u2, u1, exchange_rates) {
                            (q1.value - q2_conv).abs() < 1e-9
                        } else {
                            false
                        }
                    }
                (None, None) => (q1.value - q2.value).abs() < 1e-9,
                _ => false,
            }
        }
        _ => false,
    }
}

fn eval_ne_logic(q1: &Quantity, q2: &Quantity, exchange_rates: &HashMap<String, f64>) -> bool {
    !eval_eq_logic(q1, q2, exchange_rates)
}

fn eval_lt_logic(q1: &Quantity, q2: &Quantity, exchange_rates: &HashMap<String, f64>) -> Result<bool, String> {
    if q1.list.is_some() || q2.list.is_some() {
        return Err("Cannot perform ordering comparison (<) on lists".to_string());
    }
    match (&q1.unit, &q2.unit) {
        (Some(u1), Some(u2)) => {
            if !are_compatible(u1, u2) {
                return Err(format!("Incompatible units: '{}' and '{}'", u1, u2));
            }
            let q2_conv = convert_quantity(q2.value, u2, u1, exchange_rates)?;
            Ok(q1.value < q2_conv)
        }
        (None, None) => Ok(q1.value < q2.value),
        _ => Err("Cannot compare a quantity with a dimensionless value".to_string()),
    }
}

fn eval_lte_logic(q1: &Quantity, q2: &Quantity, exchange_rates: &HashMap<String, f64>) -> Result<bool, String> {
    if q1.list.is_some() || q2.list.is_some() {
        return Err("Cannot perform ordering comparison (<=) on lists".to_string());
    }
    match (&q1.unit, &q2.unit) {
        (Some(u1), Some(u2)) => {
            if !are_compatible(u1, u2) {
                return Err(format!("Incompatible units: '{}' and '{}'", u1, u2));
            }
            let q2_conv = convert_quantity(q2.value, u2, u1, exchange_rates)?;
            Ok(q1.value <= q2_conv)
        }
        (None, None) => Ok(q1.value <= q2.value),
        _ => Err("Cannot compare a quantity with a dimensionless value".to_string()),
    }
}

fn eval_gt_logic(q1: &Quantity, q2: &Quantity, exchange_rates: &HashMap<String, f64>) -> Result<bool, String> {
    if q1.list.is_some() || q2.list.is_some() {
        return Err("Cannot perform ordering comparison (>) on lists".to_string());
    }
    match (&q1.unit, &q2.unit) {
        (Some(u1), Some(u2)) => {
            if !are_compatible(u1, u2) {
                return Err(format!("Incompatible units: '{}' and '{}'", u1, u2));
            }
            let q2_conv = convert_quantity(q2.value, u2, u1, exchange_rates)?;
            Ok(q1.value > q2_conv)
        }
        (None, None) => Ok(q1.value > q2.value),
        _ => Err("Cannot compare a quantity with a dimensionless value".to_string()),
    }
}

fn eval_gte_logic(q1: &Quantity, q2: &Quantity, exchange_rates: &HashMap<String, f64>) -> Result<bool, String> {
    if q1.list.is_some() || q2.list.is_some() {
        return Err("Cannot perform ordering comparison (>=) on lists".to_string());
    }
    match (&q1.unit, &q2.unit) {
        (Some(u1), Some(u2)) => {
            if !are_compatible(u1, u2) {
                return Err(format!("Incompatible units: '{}' and '{}'", u1, u2));
            }
            let q2_conv = convert_quantity(q2.value, u2, u1, exchange_rates)?;
            Ok(q1.value >= q2_conv)
        }
        (None, None) => Ok(q1.value >= q2.value),
        _ => Err("Cannot compare a quantity with a dimensionless value".to_string()),
    }
}

fn eval_and_logic(q1: &Quantity, q2: &Quantity) -> Result<bool, String> {
    if q1.list.is_some() || q2.list.is_some() {
        return Err("Logical AND cannot be applied to lists".to_string());
    }
    Ok(q1.value != 0.0 && q2.value != 0.0)
}

fn eval_or_logic(q1: &Quantity, q2: &Quantity) -> Result<bool, String> {
    if q1.list.is_some() || q2.list.is_some() {
        return Err("Logical OR cannot be applied to lists".to_string());
    }
    Ok(q1.value != 0.0 || q2.value != 0.0)
}

#[derive(Clone, Debug)]
pub struct Context {
    pub variables: HashMap<String, Quantity>,
    pub functions: HashMap<String, (Vec<String>, Expr)>,
    pub exchange_rates: HashMap<String, f64>,
    pub explicit_variables: std::collections::HashSet<String>,
}

impl Default for Context {
    fn default() -> Self {
        let mut variables = HashMap::new();
        variables.insert(
            "pi".to_string(),
            Quantity { is_bool: false, list: None,
                value: std::f64::consts::PI,
                unit: None,
            },
        );
        variables.insert(
            "e".to_string(),
            Quantity { is_bool: false, list: None,
                value: std::f64::consts::E,
                unit: None,
            },
        );
        variables.insert(
            "inf".to_string(),
            Quantity { is_bool: false, list: None,
                value: std::f64::INFINITY,
                unit: None,
            },
        );

        // Common physical and mathematical constants
        let constants = vec![
            ("c", 299792458.0, Some("m/s")),
            ("g", 9.80665, Some("m/s^2")),
            ("G", 6.6743e-11, Some("m^3/kg/s^2")),
            ("h", 6.62607015e-34, Some("kg*m^2/s")),
            ("hbar", 1.054571817e-34, Some("kg*m^2/s")),
            ("kb", 1.380649e-23, Some("kg*m^2/s^2/K")),
            ("NA", 6.02214076e23, None),
            ("R", 8.314462618, Some("kg*m^2/s^2/K")),
            ("me", 9.1093837015e-31, Some("kg")),
            ("mp", 1.67262192369e-27, Some("kg")),
        ];

        for &(name, value, unit) in &constants {
            variables.insert(
                name.to_string(),
                Quantity {
                    is_bool: false,
                    list: None,
                    value,
                    unit: unit.map(|u| u.to_string()),
                },
            );
            if let Some(unit_str) = unit {
                let _ = crate::math::units::register_custom_unit(name, value, unit_str);
            }
        }

        Self {
            variables,
            functions: HashMap::new(),
            exchange_rates: HashMap::new(),
            explicit_variables: std::collections::HashSet::new(),
        }
    }
}

pub fn is_explicit_conversion(expr: &Expr, ctx: &Context) -> bool {
    match expr {
        Expr::Convert(..) => true,
        Expr::Variable(name) => ctx.explicit_variables.contains(name),
        _ => false,
    }
}

pub fn eval_and_scale(expr: &Expr, ctx: &mut Context) -> Result<Quantity, String> {
    let qty = eval_expr(expr, ctx)?;
    if is_explicit_conversion(expr, ctx) {
        Ok(qty)
    } else {
        Ok(crate::math::units::auto_scale_quantity(qty, &ctx.exchange_rates))
    }
}

pub fn eval_expr(expr: &Expr, ctx: &mut Context) -> Result<Quantity, String> {
    match expr {
        Expr::Number(val) => Ok(Quantity { is_bool: false, list: None,
            value: *val,
            unit: None,
        }),
        Expr::Quantity(val, unit) => Ok(Quantity { is_bool: false, list: None,
            value: *val,
            unit: Some(unit.clone()),
        }),
        Expr::Variable(name) => {
            if let Some(val) = ctx.variables.get(name) {
                Ok(val.clone())
            } else {
                Ok(Quantity { is_bool: false, list: None,
                    value: 1.0,
                    unit: Some(name.clone()),
                })
            }
        }
        Expr::Percentage(inner) => {
            let qty = eval_expr(inner, ctx)?;
            Ok(Quantity { is_bool: false, list: None,
                value: qty.value * 0.01,
                unit: qty.unit,
            })
        }
        Expr::Block(exprs) => {
            let original_variables = ctx.variables.clone();
            let mut last_val = Quantity::scalar(0.0, None);
            for expr in exprs {
                last_val = eval_expr(expr, ctx)?;
            }
            ctx.variables = original_variables;
            Ok(last_val)
        }
        Expr::LocalAssign(name, val_expr) => {
            let qty = eval_expr(val_expr, ctx)?;
            ctx.variables.insert(name.clone(), qty.clone());
            Ok(qty)
        }
        Expr::IfElse { cond, then_expr, else_expr } => {
            let cond_qty = eval_expr(cond, ctx)?;
            let is_true = if cond_qty.is_bool {
                cond_qty.value != 0.0
            } else {
                return Err("Condition in if-else must be a boolean".to_string());
            };
            if is_true {
                eval_expr(then_expr, ctx)
            } else {
                eval_expr(else_expr, ctx)
            }
        }
        Expr::Switch { val, cases, default_case } => {
            let switch_val = eval_expr(val, ctx)?;
            let mut matched = false;
            let mut result = Quantity::scalar(0.0, None);
            
            for (pattern_expr, res_expr) in cases {
                let pattern_val = eval_expr(pattern_expr, ctx)?;
                if eval_eq_logic(&switch_val, &pattern_val, &ctx.exchange_rates) {
                    result = eval_expr(res_expr, ctx)?;
                    matched = true;
                    break;
                }
            }
            
            if !matched {
                if let Some(def_expr) = default_case {
                    result = eval_expr(def_expr, ctx)?;
                } else {
                    return Err("No case matched in switch statement and no default case provided".to_string());
                }
            }
            Ok(result)
        }
        Expr::StringLiteral(val) => {
            Ok(Quantity {
                value: 0.0,
                unit: Some(val.clone()),
                list: None,
                is_bool: false,
            })
        }
        Expr::Convert(inner_expr, target_unit) => {
            let qty = eval_expr(inner_expr, ctx)?;
            if target_unit == "hex" || target_unit == "HEX" || target_unit == "bin" || target_unit == "BIN" {
                return Ok(Quantity {
                    is_bool: qty.is_bool,
                    list: qty.list,
                    value: qty.value,
                    unit: Some(target_unit.to_lowercase()),
                });
            }
            let src_unit = qty.unit.ok_or_else(|| {
                format!(
                    "Cannot convert dimensionless value to unit '{}'",
                    target_unit
                )
            })?;
            let converted_val =
                convert_quantity(qty.value, &src_unit, target_unit, &ctx.exchange_rates)?;
            Ok(Quantity { is_bool: false, list: None,
                value: converted_val,
                unit: Some(target_unit.clone()),
            })
        }
        Expr::List(elements) => {
            let mut el_vals = Vec::new();
            for el in elements {
                el_vals.push(eval_expr(el, ctx)?);
            }
            Ok(Quantity::list(el_vals))
        }
        Expr::Not(inner) => {
            let qty = eval_expr(inner, ctx)?;
            if qty.list.is_some() {
                return Err("Logical NOT cannot be applied to a list".to_string());
            }
            Ok(Quantity::boolean(qty.value == 0.0))
        }
        Expr::BitNot(inner) => {
            let qty = eval_expr(inner, ctx)?;
            if qty.list.is_some() {
                return Err("Bitwise NOT cannot be applied to a list".to_string());
            }
            let val = !(qty.value as i64);
            Ok(Quantity { is_bool: false, list: None, value: val as f64, unit: qty.unit })
        }
        Expr::FnCall(name, args) => {
            if name == "solve" {
                if args.len() != 2 {
                    return Err("Built-in function 'solve' expects 2 arguments".to_string());
                }
                let solve_expr = &args[0];
                let var_expr = &args[1];
                let var_name = match var_expr {
                    Expr::Variable(v) => v.clone(),
                    _ => return Err("Second argument to 'solve' must be a variable name".to_string()),
                };
                return solve_equation(solve_expr, &var_name, ctx);
            }
            if name == "diff" || name == "der" {
                if args.len() != 2 {
                    return Err(format!("Built-in function '{}' expects 2 arguments", name));
                }
                let diff_expr = &args[0];
                let var_expr = &args[1];
                let var_name = match var_expr {
                    Expr::Variable(v) => v.clone(),
                    _ => return Err(format!("Second argument to '{}' must be a variable name", name)),
                };
                let derived_ast = differentiate(diff_expr, &var_name)?;
                let simplified_ast = simplify(&derived_ast);
                if ctx.variables.contains_key(&var_name) {
                    return eval_expr(&simplified_ast, ctx);
                } else {
                    let formula_str = expr_to_string(&simplified_ast);
                    return Ok(Quantity {
                        is_bool: false,
                        list: None,
                        value: 1.0,
                        unit: Some(format!("formula:{}", formula_str)),
                    });
                }
            }

            if name == "map" {
                if args.len() != 2 {
                    return Err("Built-in function 'map' expects 2 arguments".to_string());
                }
                let map_expr = &args[0];
                let list_qty = eval_expr(&args[1], ctx)?;
                let elements = list_qty.list.as_ref().ok_or("Second argument to 'map' must be a list")?;
                
                let var_name = find_variable_in_expr(map_expr).unwrap_or_else(|| "x".to_string());
                
                let mut mapped_elements = Vec::new();
                for el in elements {
                    let prev_val = ctx.variables.insert(var_name.clone(), el.clone());
                    let res = eval_expr(map_expr, ctx);
                    if let Some(pv) = prev_val {
                        ctx.variables.insert(var_name.clone(), pv);
                    } else {
                        ctx.variables.remove(&var_name);
                    }
                    mapped_elements.push(res?);
                }
                return Ok(Quantity::list(mapped_elements));
            }
            if name == "reduce" {
                if args.len() != 2 {
                    return Err("Built-in function 'reduce' expects 2 arguments".to_string());
                }
                let reduce_expr = &args[0];
                let list_qty = eval_expr(&args[1], ctx)?;
                let elements = list_qty.list.as_ref().ok_or("Second argument to 'reduce' must be a list")?;
                if elements.is_empty() {
                    return Err("Cannot reduce an empty list".to_string());
                }
                
                let vars = find_all_variables_in_expr(reduce_expr);
                let (acc_var, el_var) = if vars.len() >= 2 {
                    (vars[0].clone(), vars[1].clone())
                } else if vars.len() == 1 {
                    if vars[0] == "y" {
                        ("x".to_string(), "y".to_string())
                    } else {
                        (vars[0].clone(), "y".to_string())
                    }
                } else {
                    ("x".to_string(), "y".to_string())
                };
                
                let mut acc = elements[0].clone();
                for el in &elements[1..] {
                    let prev_acc = ctx.variables.insert(acc_var.clone(), acc.clone());
                    let prev_el = ctx.variables.insert(el_var.clone(), el.clone());
                    
                    let res = eval_expr(reduce_expr, ctx);
                    
                    if let Some(pa) = prev_acc {
                        ctx.variables.insert(acc_var.clone(), pa);
                    } else {
                        ctx.variables.remove(&acc_var);
                    }
                    if let Some(pe) = prev_el {
                        ctx.variables.insert(el_var.clone(), pe);
                    } else {
                        ctx.variables.remove(&el_var);
                    }
                    
                    acc = res?;
                }
                return Ok(acc);
            }

            // Evaluate arguments
            let mut arg_vals = Vec::new();
            for arg in args {
                arg_vals.push(eval_expr(arg, ctx)?);
            }

            // Check built-in functions
            match name.as_str() {
                "sin" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if is_complex(&arg_vals[0]) {
                        let (a, b) = to_complex_parts(&arg_vals[0]);
                        return Ok(make_complex_qty(a.sin() * b.cosh(), a.cos() * b.sinh()));
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.sin(),
                        unit: None,
                    })
                }
                "cos" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if is_complex(&arg_vals[0]) {
                        let (a, b) = to_complex_parts(&arg_vals[0]);
                        return Ok(make_complex_qty(a.cos() * b.cosh(), -a.sin() * b.sinh()));
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.cos(),
                        unit: None,
                    })
                }
                "tan" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if is_complex(&arg_vals[0]) {
                        let (a, b) = to_complex_parts(&arg_vals[0]);
                        let sz = make_complex_qty(a.sin() * b.cosh(), a.cos() * b.sinh());
                        let cz = make_complex_qty(a.cos() * b.cosh(), -a.sin() * b.sinh());
                        let (s_re, s_im) = to_complex_parts(&sz);
                        let (c_re, c_im) = to_complex_parts(&cz);
                        let denom = c_re * c_re + c_im * c_im;
                        if denom == 0.0 {
                            return Err("Division by zero in complex tan".to_string());
                        }
                        return Ok(make_complex_qty(
                            (s_re * c_re + s_im * c_im) / denom,
                            (s_im * c_re - s_re * c_im) / denom
                        ));
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.tan(),
                        unit: None,
                    })
                }
                "asin" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    let val = arg_vals[0].value;
                    if !(-1.0..=1.0).contains(&val) {
                        return Err("Argument to 'asin' must be between -1.0 and 1.0".to_string());
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: val.asin(),
                        unit: None,
                    })
                }
                "acos" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    let val = arg_vals[0].value;
                    if !(-1.0..=1.0).contains(&val) {
                        return Err("Argument to 'acos' must be between -1.0 and 1.0".to_string());
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: val.acos(),
                        unit: None,
                    })
                }
                "atan" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.atan(),
                        unit: None,
                    })
                }
                "sinh" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.sinh(),
                        unit: None,
                    })
                }
                "cosh" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.cosh(),
                        unit: None,
                    })
                }
                "tanh" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.tanh(),
                        unit: None,
                    })
                }
                "asinh" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.asinh(),
                        unit: None,
                    })
                }
                "acosh" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    let val = arg_vals[0].value;
                    if val < 1.0 {
                        return Err("Argument to 'acosh' must be greater than or equal to 1.0".to_string());
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: val.acosh(),
                        unit: None,
                    })
                }
                "atanh" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    let val = arg_vals[0].value;
                    if val <= -1.0 || val >= 1.0 {
                        return Err("Argument to 'atanh' must be between -1.0 and 1.0 (exclusive)".to_string());
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: val.atanh(),
                        unit: None,
                    })
                }
                "exp" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if is_complex(&arg_vals[0]) {
                        let (a, b) = to_complex_parts(&arg_vals[0]);
                        let r = a.exp();
                        return Ok(make_complex_qty(r * b.cos(), r * b.sin()));
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.exp(),
                        unit: None,
                    })
                }
                "sum" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'sum' expects at least 1 argument".to_string());
                    }
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    if flat_args.is_empty() {
                        return Err("Function 'sum' expects at least 1 argument or non-empty list".to_string());
                    }
                    let mut total = flat_args[0].value;
                    let target_unit = &flat_args[0].unit;
                    for q in &flat_args[1..] {
                        match (target_unit, &q.unit) {
                            (Some(u1), Some(u2)) => {
                                if !are_compatible(u1, u2) {
                                    return Err(format!("Incompatible units in sum(): '{}' and '{}'", u1, u2));
                                }
                                let converted = convert_quantity(q.value, u2, u1, &ctx.exchange_rates)?;
                                total += converted;
                            }
                            (None, None) => {
                                total += q.value;
                            }
                            _ => {
                                return Err("Cannot mix dimensional and dimensionless values in sum()".to_string());
                            }
                        }
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: total,
                        unit: target_unit.clone(),
                    })
                }
                "prod" | "product" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'prod' expects at least 1 argument".to_string());
                    }
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    if flat_args.is_empty() {
                        return Err("Function 'prod' expects at least 1 argument or non-empty list".to_string());
                    }
                    let mut total_val = 1.0;
                    let mut current_unit: Option<String> = None;
                    for q in flat_args {
                        total_val *= q.value;
                        let (new_unit, multiplier) = combine_units_with_multiplier(
                            current_unit.as_deref(),
                            q.unit.as_deref(),
                            false,
                            &ctx.exchange_rates,
                        );
                        total_val *= multiplier;
                        current_unit = new_unit;
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: total_val,
                        unit: current_unit,
                    })
                }
                "mean" | "average" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'mean' expects at least 1 argument".to_string());
                    }
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    if flat_args.is_empty() {
                        return Err("Function 'mean' expects at least 1 argument or non-empty list".to_string());
                    }
                    let mut total = flat_args[0].value;
                    let target_unit = &flat_args[0].unit;
                    for q in &flat_args[1..] {
                        match (target_unit, &q.unit) {
                            (Some(u1), Some(u2)) => {
                                if !are_compatible(u1, u2) {
                                    return Err(format!("Incompatible units in mean(): '{}' and '{}'", u1, u2));
                                }
                                let converted = convert_quantity(q.value, u2, u1, &ctx.exchange_rates)?;
                                total += converted;
                            }
                            (None, None) => {
                                total += q.value;
                            }
                            _ => {
                                return Err("Cannot mix dimensional and dimensionless values in mean()".to_string());
                            }
                        }
                    }
                    let mean_val = total / (flat_args.len() as f64);
                    Ok(Quantity { is_bool: false, list: None,
                        value: mean_val,
                        unit: target_unit.clone(),
                    })
                }
                "median" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'median' expects at least 1 argument".to_string());
                    }
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    if flat_args.is_empty() {
                        return Err("Function 'median' expects at least 1 argument or non-empty list".to_string());
                    }
                    let mut vals = Vec::new();
                    let target_unit = &flat_args[0].unit;
                    vals.push(flat_args[0].value);
                    for q in &flat_args[1..] {
                        match (target_unit, &q.unit) {
                            (Some(u1), Some(u2)) => {
                                if !are_compatible(u1, u2) {
                                    return Err(format!("Incompatible units in median(): '{}' and '{}'", u1, u2));
                                }
                                let converted = convert_quantity(q.value, u2, u1, &ctx.exchange_rates)?;
                                vals.push(converted);
                            }
                            (None, None) => {
                                vals.push(q.value);
                            }
                            _ => {
                                return Err("Cannot mix dimensional and dimensionless values in median()".to_string());
                            }
                        }
                    }
                    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    let len = vals.len();
                    let median_val = if len % 2 == 0 {
                        (vals[len / 2 - 1] + vals[len / 2]) / 2.0
                    } else {
                        vals[len / 2]
                    };
                    Ok(Quantity { is_bool: false, list: None,
                        value: median_val,
                        unit: target_unit.clone(),
                    })
                }
                "stddev" | "stdev" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'stddev' expects at least 1 argument".to_string());
                    }
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    if flat_args.is_empty() {
                        return Err("Function 'stddev' expects at least 1 argument or non-empty list".to_string());
                    }
                    let mut vals = Vec::new();
                    let target_unit = &flat_args[0].unit;
                    vals.push(flat_args[0].value);
                    for q in &flat_args[1..] {
                        match (target_unit, &q.unit) {
                            (Some(u1), Some(u2)) => {
                                if !are_compatible(u1, u2) {
                                    return Err(format!("Incompatible units in stddev(): '{}' and '{}'", u1, u2));
                                }
                                let converted = convert_quantity(q.value, u2, u1, &ctx.exchange_rates)?;
                                vals.push(converted);
                            }
                            (None, None) => {
                                vals.push(q.value);
                            }
                            _ => {
                                return Err("Cannot mix dimensional and dimensionless values in stddev()".to_string());
                            }
                        }
                    }
                    let len = vals.len();
                    if len == 1 {
                        return Ok(Quantity { is_bool: false, list: None,
                            value: 0.0,
                            unit: target_unit.clone(),
                        });
                    }
                    let sum: f64 = vals.iter().sum();
                    let mean = sum / (len as f64);
                    let variance_sum: f64 = vals.iter().map(|&x| {
                        let diff = x - mean;
                        diff * diff
                    }).sum();
                    let stddev_val = (variance_sum / ((len - 1) as f64)).sqrt();
                    Ok(Quantity { is_bool: false, list: None,
                        value: stddev_val,
                        unit: target_unit.clone(),
                    })
                }
                "var" | "variance" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'variance' expects at least 1 argument".to_string());
                    }
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    if flat_args.is_empty() {
                        return Err("Function 'variance' expects at least 1 argument or non-empty list".to_string());
                    }
                    let mut vals = Vec::new();
                    let target_unit = &flat_args[0].unit;
                    vals.push(flat_args[0].value);
                    for q in &flat_args[1..] {
                        match (target_unit, &q.unit) {
                            (Some(u1), Some(u2)) => {
                                if !are_compatible(u1, u2) {
                                    return Err(format!("Incompatible units in variance(): '{}' and '{}'", u1, u2));
                                }
                                let converted = convert_quantity(q.value, u2, u1, &ctx.exchange_rates)?;
                                vals.push(converted);
                            }
                            (None, None) => {
                                vals.push(q.value);
                            }
                            _ => {
                                return Err("Cannot mix dimensional and dimensionless values in variance()".to_string());
                            }
                        }
                    }
                    let len = vals.len();
                    if len == 1 {
                        return Ok(Quantity { is_bool: false, list: None,
                            value: 0.0,
                            unit: target_unit.clone(),
                        });
                    }
                    let sum: f64 = vals.iter().sum();
                    let mean = sum / (len as f64);
                    let variance_sum: f64 = vals.iter().map(|&x| {
                        let diff = x - mean;
                        diff * diff
                    }).sum();
                    let variance_val = variance_sum / ((len - 1) as f64);
                    Ok(Quantity { is_bool: false, list: None,
                        value: variance_val,
                        unit: target_unit.clone(),
                    })
                }
                "len" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    let list_qty = &arg_vals[0];
                    if let Some(ref elements) = list_qty.list {
                        Ok(Quantity::scalar(elements.len() as f64, None))
                    } else {
                        Err("Function 'len' expects a list/vector argument".to_string())
                    }
                }
                "count" => {
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    Ok(Quantity::scalar(flat_args.len() as f64, None))
                }
                "vdot" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let q1 = &arg_vals[0];
                    let q2 = &arg_vals[1];
                    let el1 = q1.list.as_ref().ok_or("vdot expects first argument to be a list/vector")?;
                    let el2 = q2.list.as_ref().ok_or("vdot expects second argument to be a list/vector")?;
                    if el1.len() != el2.len() {
                        return Err(format!("vdot: vector lengths must match ({} and {})", el1.len(), el2.len()));
                    }
                    let mut total_val = 0.0;
                    let mut target_unit: Option<String> = None;
                    for (q1, q2) in el1.iter().zip(el2.iter()) {
                        if q1.list.is_some() || q2.list.is_some() {
                            return Err("vdot expects flat vectors (lists of scalars)".to_string());
                        }
                        let (unit, multiplier) = combine_units_with_multiplier(
                            q1.unit.as_deref(),
                            q2.unit.as_deref(),
                            false,
                            &ctx.exchange_rates,
                        );
                        let prod_val = q1.value * q2.value * multiplier;
                        if total_val == 0.0 && target_unit.is_none() {
                            total_val = prod_val;
                            target_unit = unit;
                        } else {
                            match (&target_unit, &unit) {
                                (Some(u1), Some(u2)) => {
                                    if !are_compatible(u1, u2) {
                                        return Err(format!("Incompatible units in vdot(): '{}' and '{}'", u1, u2));
                                    }
                                    let converted = convert_quantity(prod_val, u2, u1, &ctx.exchange_rates)?;
                                    total_val += converted;
                                }
                                (None, None) => {
                                    total_val += prod_val;
                                }
                                _ => {
                                    return Err("Cannot mix dimensional and dimensionless values in vdot() sum".to_string());
                                }
                            }
                        }
                    }
                    Ok(Quantity::scalar(total_val, target_unit))
                }
                "vadd" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    quantity_add(&arg_vals[0], &arg_vals[1], ctx)
                }
                "vsub" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    quantity_sub(&arg_vals[0], &arg_vals[1], ctx)
                }
                "transpose" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    let qty = &arg_vals[0];
                    let elements = qty.list.as_ref().ok_or("transpose expects a list or matrix")?;
                    if elements.is_empty() {
                        return Ok(qty.clone());
                    }
                    let all_lists = elements.iter().all(|el| el.list.is_some());
                    let all_scalars = elements.iter().all(|el| el.list.is_none());
                    if all_scalars {
                        // 1D list of scalars -> 2D list of shape N x 1
                        let mut new_rows = Vec::new();
                        for el in elements {
                            new_rows.push(Quantity::list(vec![el.clone()]));
                        }
                        Ok(Quantity::list(new_rows))
                    } else if all_lists {
                        // 2D list -> 2D list
                        let num_rows = elements.len();
                        let first_row_len = elements[0].list.as_ref().unwrap().len();
                        for row in elements {
                            let row_el = row.list.as_ref().unwrap();
                            if row_el.len() != first_row_len {
                                return Err("Matrix rows must all have the same length".to_string());
                            }
                        }
                        let mut transposed_rows = Vec::new();
                        for col_idx in 0..first_row_len {
                            let mut new_row = Vec::new();
                            for row_idx in 0..num_rows {
                                let cell = &elements[row_idx].list.as_ref().unwrap()[col_idx];
                                new_row.push(cell.clone());
                            }
                            transposed_rows.push(Quantity::list(new_row));
                        }
                        Ok(Quantity::list(transposed_rows))
                    } else {
                        Err("Invalid matrix for transpose: mix of lists and scalars".to_string())
                    }
                }
                "matmul" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    matmul_impl(&arg_vals[0], &arg_vals[1], ctx)
                }
                "if" => {
                    check_built_in_args(name, &arg_vals, 3)?;
                    let cond = arg_vals[0].value;
                    if cond != 0.0 {
                        Ok(arg_vals[1].clone())
                    } else {
                        Ok(arg_vals[2].clone())
                    }
                }
                "and" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'and' expects at least 1 argument".to_string());
                    }
                    let all_true = arg_vals.iter().all(|q| q.value != 0.0);
                    Ok(Quantity::boolean(all_true))
                }
                "or" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'or' expects at least 1 argument".to_string());
                    }
                    let any_true = arg_vals.iter().any(|q| q.value != 0.0);
                    Ok(Quantity::boolean(any_true))
                }
                "not" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if arg_vals[0].list.is_some() {
                        return Err("Logical NOT cannot be applied to a list".to_string());
                    }
                    Ok(Quantity::boolean(arg_vals[0].value == 0.0))
                }
                "eq" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let res = eval_eq_logic(&arg_vals[0], &arg_vals[1], &ctx.exchange_rates);
                    Ok(Quantity::boolean(res))
                }
                "ne" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let res = eval_ne_logic(&arg_vals[0], &arg_vals[1], &ctx.exchange_rates);
                    Ok(Quantity::boolean(res))
                }
                "lt" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let res = eval_lt_logic(&arg_vals[0], &arg_vals[1], &ctx.exchange_rates)?;
                    Ok(Quantity::boolean(res))
                }
                "lte" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let res = eval_lte_logic(&arg_vals[0], &arg_vals[1], &ctx.exchange_rates)?;
                    Ok(Quantity::boolean(res))
                }
                "gt" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let res = eval_gt_logic(&arg_vals[0], &arg_vals[1], &ctx.exchange_rates)?;
                    Ok(Quantity::boolean(res))
                }
                "gte" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let res = eval_gte_logic(&arg_vals[0], &arg_vals[1], &ctx.exchange_rates)?;
                    Ok(Quantity::boolean(res))
                }
                "log" => {
                    if arg_vals.len() != 1 && arg_vals.len() != 2 {
                        return Err("Function 'log' expects 1 or 2 arguments".to_string());
                    }
                    if arg_vals.len() == 2 {
                        if arg_vals[1].unit.is_some() || is_complex(&arg_vals[1]) {
                            return Err("Second argument to 'log' (base) must be a real dimensionless number".to_string());
                        }
                        let base = arg_vals[1].value;
                        if base <= 0.0 || base == 1.0 {
                            return Err("Logarithm base must be positive and not equal to 1".to_string());
                        }
                        if is_complex(&arg_vals[0]) {
                            let (a, b) = to_complex_parts(&arg_vals[0]);
                            let r = (a * a + b * b).sqrt();
                            let theta = b.atan2(a);
                            let ln_z = make_complex_qty(r.ln(), theta);
                            let (ln_re, ln_im) = to_complex_parts(&ln_z);
                            let ln_base = base.ln();
                            return Ok(make_complex_qty(ln_re / ln_base, ln_im / ln_base));
                        }
                        if arg_vals[0].value < 0.0 {
                            let ln_re = (-arg_vals[0].value).ln();
                            let ln_im = std::f64::consts::PI;
                            let ln_base = base.ln();
                            return Ok(make_complex_qty(ln_re / ln_base, ln_im / ln_base));
                        }
                        Ok(Quantity {
                            is_bool: false,
                            list: None,
                            value: arg_vals[0].value.log(base),
                            unit: None,
                        })
                    } else {
                        if is_complex(&arg_vals[0]) {
                            let (a, b) = to_complex_parts(&arg_vals[0]);
                            let r = (a * a + b * b).sqrt();
                            let theta = b.atan2(a);
                            let ln_z = make_complex_qty(r.ln(), theta);
                            let (ln_re, ln_im) = to_complex_parts(&ln_z);
                            let ln_10 = 10.0f64.ln();
                            return Ok(make_complex_qty(ln_re / ln_10, ln_im / ln_10));
                        }
                        if arg_vals[0].value < 0.0 {
                            let ln_re = (-arg_vals[0].value).ln();
                            let ln_im = std::f64::consts::PI;
                            let ln_10 = 10.0f64.ln();
                            return Ok(make_complex_qty(ln_re / ln_10, ln_im / ln_10));
                        }
                        Ok(Quantity { is_bool: false, list: None,
                            value: arg_vals[0].value.log10(),
                            unit: None,
                        })
                    }
                }
                "ln" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if is_complex(&arg_vals[0]) {
                        let (a, b) = to_complex_parts(&arg_vals[0]);
                        let r = (a * a + b * b).sqrt();
                        let theta = b.atan2(a);
                        return Ok(make_complex_qty(r.ln(), theta));
                    }
                    if arg_vals[0].value < 0.0 {
                        return Ok(make_complex_qty((-arg_vals[0].value).ln(), std::f64::consts::PI));
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.ln(),
                        unit: None,
                    })
                }
                "log2" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if is_complex(&arg_vals[0]) {
                        let (a, b) = to_complex_parts(&arg_vals[0]);
                        let r = (a * a + b * b).sqrt();
                        let theta = b.atan2(a);
                        let ln_z = make_complex_qty(r.ln(), theta);
                        let (ln_re, ln_im) = to_complex_parts(&ln_z);
                        let ln_2 = 2.0f64.ln();
                        return Ok(make_complex_qty(ln_re / ln_2, ln_im / ln_2));
                    }
                    if arg_vals[0].value < 0.0 {
                        let ln_re = (-arg_vals[0].value).ln();
                        let ln_im = std::f64::consts::PI;
                        let ln_2 = 2.0f64.ln();
                        return Ok(make_complex_qty(ln_re / ln_2, ln_im / ln_2));
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.log2(),
                        unit: None,
                    })
                }
                "sqrt" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if is_complex(&arg_vals[0]) {
                        let (a, b) = to_complex_parts(&arg_vals[0]);
                        let r = (a * a + b * b).sqrt();
                        let theta = b.atan2(a);
                        let r_sqrt = r.sqrt();
                        let half_theta = theta / 2.0;
                        return Ok(make_complex_qty(r_sqrt * half_theta.cos(), r_sqrt * half_theta.sin()));
                    }
                    if arg_vals[0].value < 0.0 {
                        let val = (-arg_vals[0].value).sqrt();
                        return Ok(make_complex_qty(0.0, val));
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.sqrt(),
                        unit: arg_vals[0].unit.clone(),
                    })
                }
                "abs" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if is_complex(&arg_vals[0]) {
                        let (a, b) = to_complex_parts(&arg_vals[0]);
                        return Ok(Quantity { is_bool: false, list: None,
                            value: (a * a + b * b).sqrt(),
                            unit: None,
                        });
                    }
                    Ok(Quantity { is_bool: false, list: None,
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
                    Ok(Quantity { is_bool: false, list: None,
                        value: rounded,
                        unit: arg_vals[0].unit.clone(),
                    })
                }
                "xor" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let val = (arg_vals[0].value as i64) ^ (arg_vals[1].value as i64);
                    let unit = arg_vals[0].unit.clone().or(arg_vals[1].unit.clone());
                    Ok(Quantity { is_bool: false, list: None, value: val as f64, unit })
                }
                "ceil" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.ceil(),
                        unit: arg_vals[0].unit.clone(),
                    })
                }
                "floor" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.floor(),
                        unit: arg_vals[0].unit.clone(),
                    })
                }
                "plot" | "sparkline" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'plot' expects at least 1 argument".to_string());
                    }
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    if flat_args.is_empty() {
                        return Err("Function 'plot' expects at least 1 argument or non-empty list".to_string());
                    }

                    let min_val = flat_args.iter().map(|q| q.value).fold(f64::INFINITY, f64::min);
                    let max_val = flat_args.iter().map(|q| q.value).fold(f64::NEG_INFINITY, f64::max);

                    let blocks = [' ', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
                    let mut sparkline = String::new();

                    if max_val == min_val {
                        for _ in 0..flat_args.len() {
                            sparkline.push('▄');
                        }
                    } else {
                        let range = max_val - min_val;
                        for q in &flat_args {
                            let norm = (q.value - min_val) / range;
                            let idx = (norm * 7.0).round() as usize;
                            sparkline.push(blocks[idx]);
                        }
                    }

                    Ok(Quantity {
                        is_bool: false,
                        list: None,
                        value: 0.0,
                        unit: Some(format!("sparkline:{}", sparkline)),
                    })
                }
                "mod" => {
                    check_built_in_args(name, &arg_vals, 2)?;
                    let q1 = &arg_vals[0];
                    let q2 = &arg_vals[1];
                    match (&q1.unit, &q2.unit) {
                        (Some(u1), Some(u2)) => {
                            if !are_compatible(u1, u2) {
                                return Err(format!("Incompatible units in mod(): '{}' and '{}'", u1, u2));
                            }
                            let converted = convert_quantity(q2.value, u2, u1, &ctx.exchange_rates)?;
                            let rem = q1.value % converted;
                            Ok(Quantity { is_bool: false, list: None,
                                value: rem,
                                unit: Some(u1.clone()),
                            })
                        }
                        (None, None) => {
                            Ok(Quantity { is_bool: false, list: None,
                                value: q1.value % q2.value,
                                unit: None,
                            })
                        }
                        _ => {
                            Err("Cannot compare a quantity with a dimensionless value in mod()".to_string())
                        }
                    }
                }
                "min" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'min' expects at least 1 argument".to_string());
                    }
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    if flat_args.is_empty() {
                        return Err("Function 'min' expects at least 1 argument or non-empty list".to_string());
                    }
                    let mut min_val = flat_args[0].value;
                    let target_unit = &flat_args[0].unit;
                    for q in &flat_args[1..] {
                        match (target_unit, &q.unit) {
                            (Some(u1), Some(u2)) => {
                                if !are_compatible(u1, u2) {
                                    return Err(format!("Incompatible units in min(): '{}' and '{}'", u1, u2));
                                }
                                let converted = convert_quantity(q.value, u2, u1, &ctx.exchange_rates)?;
                                min_val = min_val.min(converted);
                            }
                            (None, None) => {
                                min_val = min_val.min(q.value);
                            }
                            _ => {
                                return Err("Cannot compare a quantity with a dimensionless value in min()".to_string());
                            }
                        }
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: min_val,
                        unit: target_unit.clone(),
                    })
                }
                "max" => {
                    if arg_vals.is_empty() {
                        return Err("Function 'max' expects at least 1 argument".to_string());
                    }
                    let mut flat_args = Vec::new();
                    for arg in &arg_vals {
                        flatten_quantity(arg, &mut flat_args);
                    }
                    if flat_args.is_empty() {
                        return Err("Function 'max' expects at least 1 argument or non-empty list".to_string());
                    }
                    let mut max_val = flat_args[0].value;
                    let target_unit = &flat_args[0].unit;
                    for q in &flat_args[1..] {
                        match (target_unit, &q.unit) {
                            (Some(u1), Some(u2)) => {
                                if !are_compatible(u1, u2) {
                                    return Err(format!("Incompatible units in max(): '{}' and '{}'", u1, u2));
                                }
                                let converted = convert_quantity(q.value, u2, u1, &ctx.exchange_rates)?;
                                max_val = max_val.max(converted);
                            }
                            (None, None) => {
                                max_val = max_val.max(q.value);
                            }
                            _ => {
                                return Err("Cannot compare a quantity with a dimensionless value in max()".to_string());
                            }
                        }
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: max_val,
                        unit: target_unit.clone(),
                    })
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
                    
                    Ok(Quantity { is_bool: false, list: None,
                        value: pmt_val,
                        unit: arg_vals[2].unit.clone(),
                    })
                }
                "fv" => {
                    if arg_vals.len() != 3 && arg_vals.len() != 4 {
                        return Err("Function 'fv' expects 3 or 4 arguments".to_string());
                    }
                    let rate = arg_vals[0].value;
                    let nper = arg_vals[1].value;
                    let pmt = arg_vals[2].value;
                    let pv = if arg_vals.len() == 4 { arg_vals[3].value } else { 0.0 };

                    if arg_vals[0].unit.is_some() {
                        return Err("First argument of 'fv' (rate) must be dimensionless or percentage".to_string());
                    }
                    if arg_vals[1].unit.is_some() {
                        return Err("Second argument of 'fv' (nper) must be dimensionless".to_string());
                    }

                    let fv_val = if rate == 0.0 {
                        -pv - pmt * nper
                    } else {
                        let term = (1.0 + rate).powf(nper);
                        -pv * term - pmt * (term - 1.0) / rate
                    };

                    let target_unit = if arg_vals.len() == 4 && arg_vals[3].unit.is_some() {
                        arg_vals[3].unit.clone()
                    } else {
                        arg_vals[2].unit.clone()
                    };

                    Ok(Quantity { is_bool: false, list: None,
                        value: fv_val,
                        unit: target_unit,
                    })
                }
                "pv" => {
                    if arg_vals.len() != 3 && arg_vals.len() != 4 {
                        return Err("Function 'pv' expects 3 or 4 arguments".to_string());
                    }
                    let rate = arg_vals[0].value;
                    let nper = arg_vals[1].value;
                    let pmt = arg_vals[2].value;
                    let fv = if arg_vals.len() == 4 { arg_vals[3].value } else { 0.0 };

                    if arg_vals[0].unit.is_some() {
                        return Err("First argument of 'pv' (rate) must be dimensionless or percentage".to_string());
                    }
                    if arg_vals[1].unit.is_some() {
                        return Err("Second argument of 'pv' (nper) must be dimensionless".to_string());
                    }

                    let pv_val = if rate == 0.0 {
                        -fv - pmt * nper
                    } else {
                        let term = (1.0 + rate).powf(-nper);
                        -fv * term - pmt * (1.0 - term) / rate
                    };

                    let target_unit = if arg_vals.len() == 4 && arg_vals[3].unit.is_some() {
                        arg_vals[3].unit.clone()
                    } else {
                        arg_vals[2].unit.clone()
                    };

                    Ok(Quantity { is_bool: false, list: None,
                        value: pv_val,
                        unit: target_unit,
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
                    for (param_name, arg_qty) in params.iter().zip(arg_vals) {
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

                return Ok(Quantity { is_bool: false, list: None,
                    value: final_val,
                    unit: left_qty.unit,
                });
            }

            // Standard evaluation
            let left_qty = eval_expr(left_expr, ctx)?;
            let right_qty = eval_expr(right_expr, ctx)?;

            match op {
                Op::Add | Op::Sub => {
                    if is_complex(&left_qty) || is_complex(&right_qty) {
                        let (a, b) = to_complex_parts(&left_qty);
                        let (c, d) = to_complex_parts(&right_qty);
                        return match op {
                            Op::Add => Ok(make_complex_qty(a + c, b + d)),
                            Op::Sub => Ok(make_complex_qty(a - c, b - d)),
                            _ => unreachable!(),
                        };
                    }
                    match (&left_qty.unit, &right_qty.unit) {
                        (None, None) => {
                            let value = match op {
                                Op::Add => left_qty.value + right_qty.value,
                                Op::Sub => left_qty.value - right_qty.value,
                                _ => unreachable!(),
                            };
                            Ok(Quantity { is_bool: false, list: None, value, unit: None })
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
                            Ok(Quantity { is_bool: false, list: None,
                                value,
                                unit: Some(u1.clone()),
                            })
                        }
                        _ => Err("Cannot mix dimensionless values with dimensional units in addition/subtraction".to_string()),
                    }
                }
                Op::Mul => {
                    if is_complex(&left_qty) || is_complex(&right_qty) {
                        let (a, b) = to_complex_parts(&left_qty);
                        let (c, d) = to_complex_parts(&right_qty);
                        return Ok(make_complex_qty(a * c - b * d, a * d + b * c));
                    }
                    let (unit, multiplier) = combine_units_with_multiplier(
                        left_qty.unit.as_deref(),
                        right_qty.unit.as_deref(),
                        false,
                        &ctx.exchange_rates,
                    );
                    let value = left_qty.value * right_qty.value * multiplier;
                    Ok(Quantity { is_bool: false, list: None, value, unit })
                }
                Op::Div => {
                    if is_complex(&left_qty) || is_complex(&right_qty) {
                        let (a, b) = to_complex_parts(&left_qty);
                        let (c, d) = to_complex_parts(&right_qty);
                        let denom = c * c + d * d;
                        if denom == 0.0 {
                            return Err("Division by zero in complex division".to_string());
                        }
                        return Ok(make_complex_qty((a * c + b * d) / denom, (b * c - a * d) / denom));
                    }
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
                    Ok(Quantity { is_bool: false, list: None, value, unit })
                }
                Op::Pow => {
                    if is_complex(&left_qty) || is_complex(&right_qty) {
                        let (a, b) = to_complex_parts(&left_qty);
                        let (c, d) = to_complex_parts(&right_qty);
                        if d != 0.0 {
                            return Err("Complex exponent is not supported".to_string());
                        }
                        let n = c; // real exponent
                        let r = (a * a + b * b).sqrt();
                        let theta = b.atan2(a);
                        let r_n = r.powf(n);
                        let n_theta = n * theta;
                        return Ok(make_complex_qty(r_n * n_theta.cos(), r_n * n_theta.sin()));
                    }
                    if right_qty.unit.is_some() {
                        return Err("Exponent power must be a dimensionless scalar".to_string());
                    }
                    let value = left_qty.value.powf(right_qty.value);
                    let unit = if let Some(ref u) = left_qty.unit {
                        let power = right_qty.value;
                        if power == 0.0 {
                            None
                        } else {
                            let mut map = crate::math::units::parse_unit(u);
                            for exp in map.values_mut() {
                                *exp = (*exp as f64 * power).round() as i32;
                            }
                            map.retain(|_, &mut exp| exp != 0);
                            crate::math::units::format_unit_map(&map)
                        }
                    } else {
                        None
                    };
                    Ok(Quantity { is_bool: false, list: None, value, unit })
                }
                Op::Mod => {
                    let u1 = &left_qty.unit;
                    let u2 = &right_qty.unit;
                    match (u1, u2) {
                        (Some(unit1), Some(unit2)) => {
                            if !are_compatible(unit1, unit2) {
                                return Err(format!("Incompatible units in modulo operator: '{}' and '{}'", unit1, unit2));
                            }
                            let right_converted = convert_quantity(right_qty.value, unit2, unit1, &ctx.exchange_rates)?;
                            let rem = left_qty.value % right_converted;
                            Ok(Quantity { is_bool: false, list: None,
                                value: rem,
                                unit: Some(unit1.clone()),
                            })
                        }
                        (None, None) => {
                            Ok(Quantity { is_bool: false, list: None,
                                value: left_qty.value % right_qty.value,
                                unit: None,
                            })
                        }
                        _ => {
                            Err("Cannot compare a quantity with a dimensionless value in modulo operator".to_string())
                        }
                    }
                }
                Op::Less => {
                    let res = eval_lt_logic(&left_qty, &right_qty, &ctx.exchange_rates)?;
                    Ok(Quantity::boolean(res))
                }
                Op::LessEq => {
                    let res = eval_lte_logic(&left_qty, &right_qty, &ctx.exchange_rates)?;
                    Ok(Quantity::boolean(res))
                }
                Op::Greater => {
                    let res = eval_gt_logic(&left_qty, &right_qty, &ctx.exchange_rates)?;
                    Ok(Quantity::boolean(res))
                }
                Op::GreaterEq => {
                    let res = eval_gte_logic(&left_qty, &right_qty, &ctx.exchange_rates)?;
                    Ok(Quantity::boolean(res))
                }
                Op::Eq => {
                    let res = eval_eq_logic(&left_qty, &right_qty, &ctx.exchange_rates);
                    Ok(Quantity::boolean(res))
                }
                Op::Ne => {
                    let res = eval_ne_logic(&left_qty, &right_qty, &ctx.exchange_rates);
                    Ok(Quantity::boolean(res))
                }
                Op::And => {
                    let res = eval_and_logic(&left_qty, &right_qty)?;
                    Ok(Quantity::boolean(res))
                }
                Op::Or => {
                    let res = eval_or_logic(&left_qty, &right_qty)?;
                    Ok(Quantity::boolean(res))
                }
                Op::BitAnd => {
                    let val = (left_qty.value as i64) & (right_qty.value as i64);
                    let unit = left_qty.unit.or(right_qty.unit);
                    Ok(Quantity { is_bool: false, list: None, value: val as f64, unit })
                }
                Op::BitOr => {
                    let val = (left_qty.value as i64) | (right_qty.value as i64);
                    let unit = left_qty.unit.or(right_qty.unit);
                    Ok(Quantity { is_bool: false, list: None, value: val as f64, unit })
                }
                Op::LShift => {
                    let val = (left_qty.value as i64) << (right_qty.value as i64);
                    let unit = left_qty.unit;
                    Ok(Quantity { is_bool: false, list: None, value: val as f64, unit })
                }
                Op::RShift => {
                    let val = (left_qty.value as i64) >> (right_qty.value as i64);
                    let unit = left_qty.unit;
                    Ok(Quantity { is_bool: false, list: None, value: val as f64, unit })
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

fn format_float(val: f64) -> String {
    if val.fract() == 0.0 {
        format!("{}", val as i64)
    } else {
        let abs_val = val.abs();
        let formatted = if abs_val < 1e-4 && abs_val > 0.0 {
            if abs_val < 1e-9 {
                format!("{:e}", val)
            } else {
                format!("{:.10}", val)
            }
        } else {
            format!("{:.4}", val)
        };
        formatted
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

// Formats a Quantity nicely for buffer output
pub fn format_quantity(qty: &Quantity) -> String {
    if let Some(ref u) = qty.unit {
        if u.starts_with("sparkline:") {
            return u["sparkline:".len()..].to_string();
        }
        if u.starts_with("formula:") {
            return u["formula:".len()..].to_string();
        }
        if u == "complex"
            && let Some(ref list) = qty.list
            && list.len() >= 2 {
                let re = list[0].value;
                let im = list[1].value;
                let re_str = format_float(re);
                let im_str = format_float(im.abs());
                if im < 0.0 {
                    return format!("{} - {}i", re_str, im_str);
                } else {
                    return format!("{} + {}i", re_str, im_str);
                }
            }
    }

    if qty.is_bool {
        return if qty.value != 0.0 { "True".to_string() } else { "False".to_string() };
    }

    if let Some(ref elements) = qty.list {
        let formatted: Vec<String> = elements.iter().map(format_quantity).collect();
        return format!("[{}]", formatted.join(", "));
    }

    let rounded = if let Some(ref u) = qty.unit {
        if u == "hex" {
            format!("0x{:X}", qty.value as i64)
        } else if u == "bin" {
            format!("0b{:b}", qty.value as i64)
        } else {
            format_float(qty.value)
        }
    } else {
        format_float(qty.value)
    };

    match &qty.unit {
        Some(u) => {
            if u == "hex" || u == "bin" {
                rounded
            } else {
                let adjusted_u = crate::math::units::adjust_unit_plurality(u, qty.value);
                if let Some(suffix) = adjusted_u.strip_prefix('$') {
                    format!("${}{}", rounded, suffix)
                } else {
                    let starts_with_word = adjusted_u.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false);
                    if starts_with_word && adjusted_u != "i" {
                        format!("{} {}", rounded, adjusted_u) // postfix format with space for words
                    } else {
                        format!("{}{}", rounded, adjusted_u) // postfix format without space for symbols
                    }
                }
            }
        }
        None => rounded,
    }
}

fn find_variable_in_expr(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Variable(name) => Some(name.clone()),
        Expr::Percentage(inner) => find_variable_in_expr(inner),
        Expr::BinaryOp(_, left, right) => {
            find_variable_in_expr(left).or_else(|| find_variable_in_expr(right))
        }
        Expr::FnCall(_, args) => {
            for arg in args {
                if let Some(v) = find_variable_in_expr(arg) {
                    return Some(v);
                }
            }
            None
        }
        Expr::Convert(inner, _) => find_variable_in_expr(inner),
        Expr::List(elements) => {
            for el in elements {
                if let Some(v) = find_variable_in_expr(el) {
                    return Some(v);
                }
            }
            None
        }
        Expr::Not(inner) => find_variable_in_expr(inner),
        Expr::BitNot(inner) => find_variable_in_expr(inner),
        _ => None,
    }
}

fn find_all_variables_in_expr(expr: &Expr) -> Vec<String> {
    let mut vars = Vec::new();
    find_all_variables_in_expr_helper(expr, &mut vars);
    vars
}

fn find_all_variables_in_expr_helper(expr: &Expr, vars: &mut Vec<String>) {
    match expr {
        Expr::Variable(name)
            if !vars.contains(name) => {
                vars.push(name.clone());
            }
        Expr::Percentage(inner) => find_all_variables_in_expr_helper(inner, vars),
        Expr::BinaryOp(_, left, right) => {
            find_all_variables_in_expr_helper(left, vars);
            find_all_variables_in_expr_helper(right, vars);
        }
        Expr::FnCall(_, args) => {
            for arg in args {
                find_all_variables_in_expr_helper(arg, vars);
            }
        }
        Expr::Convert(inner, _) => find_all_variables_in_expr_helper(inner, vars),
        Expr::List(elements) => {
            for el in elements {
                find_all_variables_in_expr_helper(el, vars);
            }
        }
        Expr::Not(inner) => find_all_variables_in_expr_helper(inner, vars),
        Expr::BitNot(inner) => find_all_variables_in_expr_helper(inner, vars),
        _ => {}
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
        println!("DEBUG unwrap_expr self: {:?}", self);
        match self {
            crate::math::parser::Line::Evaluation { expr, .. } => expr,
            crate::math::parser::Line::Assignment { expr, .. } => expr,
            _ => panic!("Not an expression line"),
        }
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
        let r1 = eval_expr(&parse_line("round(2.71828, 2) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(r1.value, 2.72);
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

        // mod function and % infix operator
        let md1 = eval_expr(&parse_line("mod(10, 3) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(md1.value, 1.0);
        let md2 = eval_expr(&parse_line("10 % 3 =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(md2.value, 1.0);
        let md3 = eval_expr(&parse_line("10% =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(md3.value, 0.1);

        // pmt
        let p = eval_expr(&parse_line("pmt(0.05 / 12, 60, -20000) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((p.value - 377.424).abs() < 1e-2);

        // asin, acos, atan
        let as1 = eval_expr(&parse_line("asin(0.5) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((as1.value - std::f64::consts::FRAC_PI_6).abs() < 1e-6); // ~ pi/6
        let ac1 = eval_expr(&parse_line("acos(0.5) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((ac1.value - std::f64::consts::FRAC_PI_3).abs() < 1e-6); // ~ pi/3
        let at1 = eval_expr(&parse_line("atan(1.0) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((at1.value - std::f64::consts::FRAC_PI_4).abs() < 1e-6); // ~ pi/4

        // sinh, cosh, tanh
        let sh1 = eval_expr(&parse_line("sinh(1.0) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((sh1.value - 1.17520119).abs() < 1e-6);
        let ch1 = eval_expr(&parse_line("cosh(1.0) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((ch1.value - 1.54308063).abs() < 1e-6);
        let th1 = eval_expr(&parse_line("tanh(1.0) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((th1.value - 0.76159415).abs() < 1e-6);

        // exp
        let ex1 = eval_expr(&parse_line("exp(1.0) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((ex1.value - std::f64::consts::E).abs() < 1e-9);

        // fv and pv
        let fv1 = eval_expr(&parse_line("fv(0.05 / 12, 60, -377.424, 20000) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!(fv1.value.abs() < 10.0);

        let pv1 = eval_expr(&parse_line("pv(0.05 / 12, 60, -377.424, 0) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((pv1.value - 20000.0).abs() < 10.0);

        // sum, mean, median, stddev, variance
        let s_val = eval_expr(&parse_line("sum(10m, 200cm, 3m) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(s_val.value, 15.0); // 10m + 2m + 3m = 15m
        assert_eq!(s_val.unit, Some("m".to_string()));

        let avg_val = eval_expr(&parse_line("average(10m, 200cm, 3m) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(avg_val.value, 5.0); // 15m / 3 = 5m

        let med_val = eval_expr(&parse_line("median(10m, 200cm, 6m) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(med_val.value, 6.0); // sorted: 2m, 6m, 10m. Median is 6m

        let sd_val = eval_expr(&parse_line("stddev(2, 4, 4, 4, 5, 5, 7, 9) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((sd_val.value - 2.1380899).abs() < 1e-6);

        let var_val = eval_expr(&parse_line("variance(2, 4, 4, 4, 5, 5, 7, 9) =>").unwrap_expr(), &mut ctx).unwrap();
        assert!((var_val.value - 4.5714285).abs() < 1e-6);

        // Logic and Comparisons
        let if_val = eval_expr(&parse_line("if(eq(5m, 500cm), 10m, 20m) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(if_val.value, 10.0);
        assert_eq!(if_val.unit, Some("m".to_string()));

        let and_val = eval_expr(&parse_line("and(1, 0, 1) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(and_val.value, 0.0);
        assert!(and_val.is_bool);
        assert_eq!(format_quantity(&and_val), "False");

        let or_val = eval_expr(&parse_line("or(0, 0, 1) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(or_val.value, 1.0);
        assert!(or_val.is_bool);
        assert_eq!(format_quantity(&or_val), "True");

        let not_val = eval_expr(&parse_line("not(0) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(not_val.value, 1.0);
        assert!(not_val.is_bool);
        assert_eq!(format_quantity(&not_val), "True");

        let lt_val = eval_expr(&parse_line("lt(2m, 300cm) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(lt_val.value, 1.0);
        assert!(lt_val.is_bool);
        assert_eq!(format_quantity(&lt_val), "True");

        let gt_val = eval_expr(&parse_line("gt(2m, 300cm) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(gt_val.value, 0.0);
        assert!(gt_val.is_bool);
        assert_eq!(format_quantity(&gt_val), "False");

        let gte_val = eval_expr(&parse_line("gte(300cm, 3m) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(gte_val.value, 1.0);
        assert!(gte_val.is_bool);
        assert_eq!(format_quantity(&gte_val), "True");

        // Operator tests
        let op_lt = eval_expr(&parse_line("2m < 300cm =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(op_lt.value, 1.0);
        assert!(op_lt.is_bool);
        assert_eq!(format_quantity(&op_lt), "True");

        let op_gt = eval_expr(&parse_line("2m > 300cm =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(op_gt.value, 0.0);
        assert!(op_gt.is_bool);
        assert_eq!(format_quantity(&op_gt), "False");

        let op_lte = eval_expr(&parse_line("3m <= 300cm =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(op_lte.value, 1.0);
        assert!(op_lte.is_bool);
        assert_eq!(format_quantity(&op_lte), "True");

        let op_gte = eval_expr(&parse_line("3m >= 300cm =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(op_gte.value, 1.0);
        assert!(op_gte.is_bool);
        assert_eq!(format_quantity(&op_gte), "True");

        let op_eq = eval_expr(&parse_line("3m == 300cm =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(op_eq.value, 1.0);
        assert!(op_eq.is_bool);
        assert_eq!(format_quantity(&op_eq), "True");

        let op_ne = eval_expr(&parse_line("3m != 300cm =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(op_ne.value, 0.0);
        assert!(op_ne.is_bool);
        assert_eq!(format_quantity(&op_ne), "False");

        let op_and = eval_expr(&parse_line("1 == 1 and 2 == 2 =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(op_and.value, 1.0);
        assert!(op_and.is_bool);
        assert_eq!(format_quantity(&op_and), "True");

        let op_or = eval_expr(&parse_line("1 == 2 or 2 == 2 =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(op_or.value, 1.0);
        assert!(op_or.is_bool);
        assert_eq!(format_quantity(&op_or), "True");

        let op_not = eval_expr(&parse_line("not 1 == 2 =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(op_not.value, 1.0);
        assert!(op_not.is_bool);
        assert_eq!(format_quantity(&op_not), "True");

        // Mathematical equivalence test
        let math_equiv = eval_expr(&parse_line("(1 < 2) + 5 =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(math_equiv.value, 6.0);
        assert!(!math_equiv.is_bool);
        assert_eq!(format_quantity(&math_equiv), "6");
    }

    #[test]
    fn test_lists_and_vectors() {
        let mut ctx = Context::default();

        // 1. Basic list evaluation and formatting
        let list_expr = parse_line("[1, 2, 3] =>").unwrap_expr();
        let list_qty = eval_expr(&list_expr, &mut ctx).unwrap();
        assert!(list_qty.list.is_some());
        assert_eq!(format_quantity(&list_qty), "[1, 2, 3]");

        // 2. Multi-dimensional lists
        let matrix_expr = parse_line("[[1, 2], [3, 4]] =>").unwrap_expr();
        let matrix_qty = eval_expr(&matrix_expr, &mut ctx).unwrap();
        assert_eq!(format_quantity(&matrix_qty), "[[1, 2], [3, 4]]");

        // Let's test stats functions with lists
        let sum_list = eval_expr(&parse_line("sum([1, 2, 3]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(sum_list.value, 6.0);

        let sum_mixed = eval_expr(&parse_line("sum([1, 2], 3, [4, 5]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(sum_mixed.value, 15.0);

        let sum_matrix = eval_expr(&parse_line("sum([[1, 2], [3, 4]]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(sum_matrix.value, 10.0);

        let mean_list = eval_expr(&parse_line("mean([2, 4, 6]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(mean_list.value, 4.0);

        let min_list = eval_expr(&parse_line("min([3, 1, 5]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(min_list.value, 1.0);

        let max_list = eval_expr(&parse_line("max([3, 1, 5]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(max_list.value, 5.0);

        let min_mixed = eval_expr(&parse_line("min([10, 20], 5, [15, 30]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(min_mixed.value, 5.0);

        let max_mixed = eval_expr(&parse_line("max([10, 20], 5, [15, 30]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(max_mixed.value, 30.0);

        let count_list = eval_expr(&parse_line("count([1, 2, 3, 4, 5]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(count_list.value, 5.0);

        let count_mixed = eval_expr(&parse_line("count([1, 2], 3, [4, 5]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(count_mixed.value, 5.0);

        // 3. Vector/matrix utilities
        let length = eval_expr(&parse_line("len([10, 20, 30, 40]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(length.value, 4.0);

        let vdot_val = eval_expr(&parse_line("vdot([1, 2], [3, 4]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(vdot_val.value, 11.0); // 1*3 + 2*4 = 11

        let vadd_val = eval_expr(&parse_line("vadd([1, 2], [3, 4]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(format_quantity(&vadd_val), "[4, 6]");

        let vsub_val = eval_expr(&parse_line("vsub([5, 10], [1, 2]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(format_quantity(&vsub_val), "[4, 8]");

        let trans_val = eval_expr(&parse_line("transpose([1, 2]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(format_quantity(&trans_val), "[[1], [2]]");

        let matmul_val1 = eval_expr(&parse_line("matmul([[1, 2], [3, 4]], [[5], [6]]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(format_quantity(&matmul_val1), "[[17], [39]]");

        let matmul_val2 = eval_expr(&parse_line("matmul([[1, 2], [3, 4]], [5, 6]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(format_quantity(&matmul_val2), "[17, 39]");

        // Let's test plot/sparkline
        let plot_qty1 = eval_expr(&parse_line("plot([1, 3, 2, 5, 4]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(format_quantity(&plot_qty1), " ▅▃█▆");

        let plot_qty2 = eval_expr(&parse_line("plot(10, 10, 10) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(format_quantity(&plot_qty2), "▄▄▄");
    }

    #[test]
    fn test_equation_solver_and_custom_units() {
        let mut ctx = Context::default();

        // 1. Test basic equation solving
        let expr1 = parse_line("solve(2 * x + 5 == 15, x) =>").unwrap_expr();
        let res1 = eval_expr(&expr1, &mut ctx).unwrap();
        assert_eq!(res1.value, 5.0);

        // 2. Test equation solving with units
        let expr2 = parse_line("solve(3 * y - 10m == 20m, y) =>").unwrap_expr();
        let res2 = eval_expr(&expr2, &mut ctx).unwrap();
        assert_eq!(res2.value, 10.0);
        assert_eq!(res2.unit, Some("m".to_string()));

        // 3. Test custom units via evaluate_sheet
        let rates = HashMap::new();
        let sheet = r#"
widget = 15cm
res = 2 widget + 10cm
res =>
res_cm = 2 widget in cm
res_cm =>
"#;
        let (output, _) = crate::math::evaluate_sheet(sheet, &rates);
        assert!(output.contains("res => 2.6667 widget"), "Actual output: {}", output);
        assert!(output.contains("res_cm => 30 cm"), "Actual output: {}", output);

        // 4. Test complex custom unit: J = 1 kg * m^2 / s^2
        let sheet_complex = r#"
J = 1 kg * m^2 / s^2
res_j = 2 J + 5 kg * m^2 / s^2
res_j =>
"#;
        let (output_complex, _) = crate::math::evaluate_sheet(sheet_complex, &rates);
        assert!(output_complex.contains("res_j => 7 J"), "Actual output: {}", output_complex);
    }

    #[test]
    fn test_hex_and_bin_support() {
        let mut ctx = Context::default();

        // 1. Basic parsing
        let expr1 = parse_line("0xA9 =>").unwrap_expr();
        let res1 = eval_expr(&expr1, &mut ctx).unwrap();
        assert_eq!(res1.value, 169.0);

        let expr2 = parse_line("0b1010 =>").unwrap_expr();
        let res2 = eval_expr(&expr2, &mut ctx).unwrap();
        assert_eq!(res2.value, 10.0);

        // 2. Formatting in hex / bin
        let expr3 = parse_line("0xA9 + 5 in hex =>").unwrap_expr();
        let res3 = eval_expr(&expr3, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res3), "0xAE");

        let expr4 = parse_line("0b1010 & 0b0011 in bin =>").unwrap_expr();
        let res4 = eval_expr(&expr4, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res4), "0b10");

        // 3. Bitwise OR and XOR
        let expr5 = parse_line("0b1010 | 0b0011 in bin =>").unwrap_expr();
        let res5 = eval_expr(&expr5, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res5), "0b1011");

        let expr6 = parse_line("xor(0b1010, 0b0011) in bin =>").unwrap_expr();
        let res6 = eval_expr(&expr6, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res6), "0b1001");

        // 4. Bitwise Shift
        let expr7 = parse_line("0b1010 << 1 in bin =>").unwrap_expr();
        let res7 = eval_expr(&expr7, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res7), "0b10100");
    }

    #[test]
    fn test_symbolic_differentiation() {
        let mut ctx = Context::default();

        // 1. Symbolic formula
        let expr1 = parse_line("diff(x^2 + 5 * x - 3, x) =>").unwrap_expr();
        let res1 = eval_expr(&expr1, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res1), "2 * x + 5");

        let expr2 = parse_line("der(sin(y) + cos(y), y) =>").unwrap_expr();
        let res2 = eval_expr(&expr2, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res2), "cos(y) - sin(y)");

        // 2. Evaluation with variable defined
        let sheet = r#"
        x = 10
        res = diff(x^2 + 5 * x, x)
        res =>
        "#;
        let rates = HashMap::new();
        let (output, _) = crate::math::evaluate_sheet(sheet, &rates);
        assert!(output.contains("res => 25"), "Actual output: {}", output);
    }

    #[test]
    fn test_complex_numbers_support() {
        let mut ctx = Context::default();

        // 1. imaginary literals
        let expr1 = parse_line("3i =>").unwrap_expr();
        let res1 = eval_expr(&expr1, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res1), "3i");

        // 2. complex addition and subtraction
        let expr2 = parse_line("(2 + 3i) + (4 - 5i) =>").unwrap_expr();
        let res2 = eval_expr(&expr2, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res2), "6 - 2i");

        // 3. complex multiplication
        let expr3 = parse_line("(2 + 3i) * (4 + 5i) =>").unwrap_expr();
        let res3 = eval_expr(&expr3, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res3), "-7 + 22i");

        // 4. complex division
        let expr4 = parse_line("(2 + 3i) / (1 + 2i) =>").unwrap_expr();
        let res4 = eval_expr(&expr4, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res4), "1.6 - 0.2i");

        // 5. negative square roots
        let expr5 = parse_line("sqrt(-4) =>").unwrap_expr();
        let res5 = eval_expr(&expr5, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res5), "2i");

        // 6. complex modulus / absolute value
        let expr6 = parse_line("abs(3 + 4i) =>").unwrap_expr();
        let res6 = eval_expr(&expr6, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res6), "5");
    }

    #[test]
    fn test_calca_extension_capabilities() {
        let mut ctx = Context::default();

        // 1. Inverse Hyperbolic Functions
        let expr_asinh = parse_line("asinh(0.5) =>").unwrap_expr();
        let res_asinh = eval_expr(&expr_asinh, &mut ctx).unwrap();
        assert!((res_asinh.value - 0.481211825).abs() < 1e-6);

        let expr_acosh = parse_line("acosh(2.0) =>").unwrap_expr();
        let res_acosh = eval_expr(&expr_acosh, &mut ctx).unwrap();
        assert!((res_acosh.value - 1.316957896).abs() < 1e-6);

        let expr_atanh = parse_line("atanh(0.5) =>").unwrap_expr();
        let res_atanh = eval_expr(&expr_atanh, &mut ctx).unwrap();
        assert!((res_atanh.value - 0.549306144).abs() < 1e-6);

        // 2. Extended Logarithms
        let expr_log = parse_line("log(100) =>").unwrap_expr();
        let res_log = eval_expr(&expr_log, &mut ctx).unwrap();
        assert_eq!(res_log.value, 2.0);

        let expr_log_base = parse_line("log(8, 2) =>").unwrap_expr();
        let res_log_base = eval_expr(&expr_log_base, &mut ctx).unwrap();
        assert_eq!(res_log_base.value, 3.0);

        let expr_log2 = parse_line("log2(16) =>").unwrap_expr();
        let res_log2 = eval_expr(&expr_log2, &mut ctx).unwrap();
        assert_eq!(res_log2.value, 4.0);

        let expr_log_neg = parse_line("ln(-1) =>").unwrap_expr();
        let res_log_neg = eval_expr(&expr_log_neg, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res_log_neg), "3.1416i");

        // 3. List Product
        let expr_prod = parse_line("prod([2, 3, 4]) =>").unwrap_expr();
        let res_prod = eval_expr(&expr_prod, &mut ctx).unwrap();
        assert_eq!(res_prod.value, 24.0);

        let expr_prod_units = parse_line("prod(2m, 3m) =>").unwrap_expr();
        let res_prod_units = eval_expr(&expr_prod_units, &mut ctx).unwrap();
        assert_eq!(res_prod_units.value, 6.0);
        assert_eq!(res_prod_units.unit, Some("m^2".to_string()));

        // 4. Functional Map & Reduce
        let expr_map = parse_line("map(x^2, [1, 2, 3]) =>").unwrap_expr();
        let res_map = eval_expr(&expr_map, &mut ctx).unwrap();
        assert_eq!(format_quantity(&res_map), "[1, 4, 9]");

        let expr_reduce = parse_line("reduce(x + y, [10, 20, 30]) =>").unwrap_expr();
        let res_reduce = eval_expr(&expr_reduce, &mut ctx).unwrap();
        assert_eq!(res_reduce.value, 60.0);

        let expr_reduce_custom = parse_line("reduce(a * b, [2, 3, 4]) =>").unwrap_expr();
        let res_reduce_custom = eval_expr(&expr_reduce_custom, &mut ctx).unwrap();
        assert_eq!(res_reduce_custom.value, 24.0);
    }

    #[test]
    fn test_common_constants() {
        let mut ctx = Context::default();

        // 1. Test c (speed of light)
        let expr_c = parse_line("c =>").unwrap_expr();
        let res_c = eval_expr(&expr_c, &mut ctx).unwrap();
        assert_eq!(res_c.value, 299792458.0);
        assert_eq!(res_c.unit, Some("m/s".to_string()));

        // 2. Test g (acceleration of gravity)
        let expr_g = parse_line("g =>").unwrap_expr();
        let res_g = eval_expr(&expr_g, &mut ctx).unwrap();
        assert_eq!(res_g.value, 9.80665);
        assert_eq!(res_g.unit, Some("m/s^2".to_string()));

        // 3. Test unit conversion using constant unit (e.g. converting speed to c)
        let expr_conv = parse_line("599584916 m/s in c =>").unwrap_expr();
        let res_conv = eval_expr(&expr_conv, &mut ctx).unwrap();
        assert_eq!(res_conv.value, 2.0);
        assert_eq!(res_conv.unit, Some("c".to_string()));

        // 4. Test hbar
        let expr_hbar = parse_line("hbar =>").unwrap_expr();
        let res_hbar = eval_expr(&expr_hbar, &mut ctx).unwrap();
        assert_eq!(res_hbar.value, 1.054571817e-34);

        // 5. Test inf
        let expr_inf = parse_line("inf =>").unwrap_expr();
        let res_inf = eval_expr(&expr_inf, &mut ctx).unwrap();
        assert!(res_inf.value.is_infinite());
    }

    #[test]
    fn test_format_quantity_plurality() {
        let q1 = Quantity::scalar(1.0, Some("days".to_string()));
        assert_eq!(format_quantity(&q1), "1 day");

        let q2 = Quantity::scalar(5.0, Some("days".to_string()));
        assert_eq!(format_quantity(&q2), "5 days");

        let q3 = Quantity::scalar(12.0, Some("month/year".to_string()));
        assert_eq!(format_quantity(&q3), "12 months/year");

        let q4 = Quantity::scalar(1.0, Some("month/year".to_string()));
        assert_eq!(format_quantity(&q4), "1 month/year");

        let q5 = Quantity::scalar(1.0, Some("miles/hour".to_string()));
        assert_eq!(format_quantity(&q5), "1 mile/hour");

        let q6 = Quantity::scalar(55.0, Some("miles/hour".to_string()));
        assert_eq!(format_quantity(&q6), "55 miles/hour");
    }
}
