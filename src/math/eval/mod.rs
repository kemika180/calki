use crate::math::parser::{Expr, Op, Quantity};
use crate::math::units::{are_compatible, combine_units_with_multiplier, convert_quantity};
use std::collections::HashMap;

mod arithmetic;
mod builtins;
mod complex;
mod control;
mod higher_order;

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
        Expr::Factorial(_) => Err("Cannot differentiate factorial".to_string()),
        Expr::DateTime { .. } => Err("Cannot differentiate a date/time".to_string()),
        Expr::BinaryOp(op, left, right) => match op {
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
                Ok(Expr::BinaryOp(
                    Op::Add,
                    Box::new(Expr::BinaryOp(Op::Mul, Box::new(dl), right.clone())),
                    Box::new(Expr::BinaryOp(Op::Mul, left.clone(), Box::new(dr))),
                ))
            }
            Op::Div => {
                let dl = differentiate(left, var)?;
                let dr = differentiate(right, var)?;
                Ok(Expr::BinaryOp(
                    Op::Div,
                    Box::new(Expr::BinaryOp(
                        Op::Sub,
                        Box::new(Expr::BinaryOp(Op::Mul, Box::new(dl), right.clone())),
                        Box::new(Expr::BinaryOp(Op::Mul, left.clone(), Box::new(dr))),
                    )),
                    Box::new(Expr::BinaryOp(
                        Op::Pow,
                        right.clone(),
                        Box::new(Expr::Number(2.0)),
                    )),
                ))
            }
            Op::Pow => {
                let left_has = expr_contains_var(left, var);
                let right_has = expr_contains_var(right, var);
                if left_has && !right_has {
                    let du = differentiate(left, var)?;
                    Ok(Expr::BinaryOp(
                        Op::Mul,
                        Box::new(Expr::BinaryOp(
                            Op::Mul,
                            right.clone(),
                            Box::new(Expr::BinaryOp(
                                Op::Pow,
                                left.clone(),
                                Box::new(Expr::BinaryOp(
                                    Op::Sub,
                                    right.clone(),
                                    Box::new(Expr::Number(1.0)),
                                )),
                            )),
                        )),
                        Box::new(du),
                    ))
                } else if !left_has && right_has {
                    let du = differentiate(right, var)?;
                    Ok(Expr::BinaryOp(
                        Op::Mul,
                        Box::new(Expr::BinaryOp(
                            Op::Mul,
                            Box::new(expr.clone()),
                            Box::new(Expr::FnCall("ln".to_string(), vec![*left.clone()])),
                        )),
                        Box::new(du),
                    ))
                } else if left_has && right_has {
                    let du = differentiate(left, var)?;
                    let dv = differentiate(right, var)?;
                    let term1 = Expr::BinaryOp(
                        Op::Mul,
                        Box::new(dv),
                        Box::new(Expr::FnCall("ln".to_string(), vec![*left.clone()])),
                    );
                    let term2 = Expr::BinaryOp(
                        Op::Div,
                        Box::new(Expr::BinaryOp(Op::Mul, right.clone(), Box::new(du))),
                        left.clone(),
                    );
                    Ok(Expr::BinaryOp(
                        Op::Mul,
                        Box::new(expr.clone()),
                        Box::new(Expr::BinaryOp(Op::Add, Box::new(term1), Box::new(term2))),
                    ))
                } else {
                    Ok(Expr::Number(0.0))
                }
            }
            _ => Err(format!("Cannot differentiate operation {:?}", op)),
        },
        Expr::FnCall(name, args) => {
            if args.len() != 1 {
                return Err("Differentiating multi-argument functions is not supported".to_string());
            }
            let u = &args[0];
            let du = differentiate(u, var)?;
            match name.as_str() {
                "sin" => Ok(Expr::BinaryOp(
                    Op::Mul,
                    Box::new(Expr::FnCall("cos".to_string(), vec![u.clone()])),
                    Box::new(du),
                )),
                "cos" => Ok(Expr::BinaryOp(
                    Op::Mul,
                    Box::new(Expr::BinaryOp(
                        Op::Sub,
                        Box::new(Expr::Number(0.0)),
                        Box::new(Expr::FnCall("sin".to_string(), vec![u.clone()])),
                    )),
                    Box::new(du),
                )),
                "exp" => Ok(Expr::BinaryOp(
                    Op::Mul,
                    Box::new(Expr::FnCall("exp".to_string(), vec![u.clone()])),
                    Box::new(du),
                )),
                "ln" | "log" => Ok(Expr::BinaryOp(Op::Div, Box::new(du), Box::new(u.clone()))),
                _ => Err(format!(
                    "Differentiating function '{}' is not supported",
                    name
                )),
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
                Op::Add => match (&sl, &sr) {
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
                },
                Op::Sub => match (&sl, &sr) {
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
                },
                Op::Mul => match (&sl, &sr) {
                    (Expr::Number(n), _) if *n == 0.0 => Expr::Number(0.0),
                    (_, Expr::Number(n)) if *n == 0.0 => Expr::Number(0.0),
                    (Expr::Number(n), _) if *n == 1.0 => sr,
                    (_, Expr::Number(n)) if *n == 1.0 => sl,
                    (Expr::Number(a), Expr::Number(b)) => Expr::Number(a * b),
                    _ => Expr::BinaryOp(Op::Mul, Box::new(sl), Box::new(sr)),
                },
                Op::Div => match (&sl, &sr) {
                    (Expr::Number(n), _) if *n == 0.0 => Expr::Number(0.0),
                    (_, Expr::Number(n)) if *n == 1.0 => sl,
                    (Expr::Number(a), Expr::Number(b)) if *b != 0.0 => Expr::Number(a / b),
                    _ => Expr::BinaryOp(Op::Div, Box::new(sl), Box::new(sr)),
                },
                Op::Pow => match (&sl, &sr) {
                    (_, Expr::Number(n)) if *n == 0.0 => Expr::Number(1.0),
                    (_, Expr::Number(n)) if *n == 1.0 => sl,
                    (Expr::Number(n), _) if *n == 1.0 => Expr::Number(1.0),
                    (Expr::Number(a), Expr::Number(b)) => Expr::Number(a.powf(*b)),
                    _ => Expr::BinaryOp(Op::Pow, Box::new(sl), Box::new(sr)),
                },
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
        Expr::Factorial(inner) => Expr::Factorial(Box::new(simplify(inner))),
        Expr::FnCall(name, args) => {
            let s_args = args.iter().map(simplify).collect();
            Expr::FnCall(name.clone(), s_args)
        }
        Expr::For {
            var,
            iterable,
            body,
        } => Expr::For {
            var: var.clone(),
            iterable: Box::new(simplify(iterable)),
            body: Box::new(simplify(body)),
        },
        Expr::While { cond, body } => Expr::While {
            cond: Box::new(simplify(cond)),
            body: Box::new(simplify(body)),
        },
        Expr::Block(exprs) => {
            let s_exprs = exprs.iter().map(simplify).collect();
            Expr::Block(s_exprs)
        }
        Expr::IfElse {
            cond,
            then_expr,
            else_expr,
        } => Expr::IfElse {
            cond: Box::new(simplify(cond)),
            then_expr: Box::new(simplify(then_expr)),
            else_expr: Box::new(simplify(else_expr)),
        },
        Expr::LocalAssign(name, val_expr) => {
            Expr::LocalAssign(name.clone(), Box::new(simplify(val_expr)))
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
                format!("{:.4}", val)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            }
        }
        Expr::Quantity(val, unit) => {
            let rounded = if val.fract() == 0.0 {
                format!("{}", *val as i64)
            } else {
                format!("{:.4}", val)
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string()
            };
            format!("{}{}", rounded, unit)
        }
        Expr::Variable(name) => name.clone(),
        Expr::DateTime {
            epoch_secs,
            kind,
            tz_offset_secs,
        } => format_datetime(*epoch_secs, *kind, *tz_offset_secs),
        Expr::Percentage(inner) => format!("{}%", expr_to_string(inner)),
        Expr::Factorial(inner) => format!("{}!", expr_to_string(inner)),
        Expr::BinaryOp(op, left, right) => {
            if *op == Op::Sub
                && let Expr::Number(n) = &**left
                && *n == 0.0
            {
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
        Expr::IfElse {
            cond,
            then_expr,
            else_expr,
        } => {
            format!(
                "if {} {} else {}",
                expr_to_string(cond),
                expr_to_string(then_expr),
                expr_to_string(else_expr)
            )
        }
        Expr::Switch {
            val,
            cases,
            default_case,
        } => {
            let mut cases_strs = Vec::new();
            for (pattern, body) in cases {
                cases_strs.push(format!(
                    "{} => {}",
                    expr_to_string(pattern),
                    expr_to_string(body)
                ));
            }
            if let Some(def) = default_case {
                cases_strs.push(format!("default => {}", expr_to_string(def)));
            }
            format!(
                "switch {} {{\n  {}\n}}",
                expr_to_string(val),
                cases_strs.join("\n  ")
            )
        }
        Expr::For {
            var,
            iterable,
            body,
        } => {
            format!(
                "for {} in {} {}",
                var,
                expr_to_string(iterable),
                expr_to_string(body)
            )
        }
        Expr::While { cond, body } => {
            format!("while {} {}", expr_to_string(cond), expr_to_string(body))
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
                return Err(format!(
                    "Dimension mismatch in vadd: lengths {} and {}",
                    el1.len(),
                    el2.len()
                ));
            }
            let mut result_elements = Vec::new();
            for (x1, x2) in el1.iter().zip(el2.iter()) {
                result_elements.push(quantity_add(x1, x2, ctx)?);
            }
            Ok(Quantity::list(result_elements))
        }
        (None, None) => match (&q1.unit, &q2.unit) {
            (None, None) => Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: q1.value + q2.value,
                unit: None,
            }),
            (Some(u1), Some(u2)) => {
                if !are_compatible(u1, u2) {
                    return Err(format!(
                        "Incompatible units in vadd: cannot add '{}' and '{}'",
                        u1, u2
                    ));
                }
                let right_converted = convert_quantity(q2.value, u2, u1, &ctx.exchange_rates)?;
                Ok(Quantity {
                    display: None,
                    is_bool: false,
                    list: None,
                    value: q1.value + right_converted,
                    unit: Some(u1.clone()),
                })
            }
            _ => Err("Cannot mix dimensionless values with dimensional units in vadd".to_string()),
        },
        _ => Err("Cannot add a list and a scalar".to_string()),
    }
}

fn quantity_sub(q1: &Quantity, q2: &Quantity, ctx: &Context) -> Result<Quantity, String> {
    match (&q1.list, &q2.list) {
        (Some(el1), Some(el2)) => {
            if el1.len() != el2.len() {
                return Err(format!(
                    "Dimension mismatch in vsub: lengths {} and {}",
                    el1.len(),
                    el2.len()
                ));
            }
            let mut result_elements = Vec::new();
            for (x1, x2) in el1.iter().zip(el2.iter()) {
                result_elements.push(quantity_sub(x1, x2, ctx)?);
            }
            Ok(Quantity::list(result_elements))
        }
        (None, None) => match (&q1.unit, &q2.unit) {
            (None, None) => Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: q1.value - q2.value,
                unit: None,
            }),
            (Some(u1), Some(u2)) => {
                if !are_compatible(u1, u2) {
                    return Err(format!(
                        "Incompatible units in vsub: cannot subtract '{}' and '{}'",
                        u1, u2
                    ));
                }
                let right_converted = convert_quantity(q2.value, u2, u1, &ctx.exchange_rates)?;
                Ok(Quantity {
                    display: None,
                    is_bool: false,
                    list: None,
                    value: q1.value - right_converted,
                    unit: Some(u1.clone()),
                })
            }
            _ => Err("Cannot mix dimensionless values with dimensional units in vsub".to_string()),
        },
        _ => Err("Cannot subtract a list and a scalar".to_string()),
    }
}

fn quantity_mul(
    left_qty: &Quantity,
    right_qty: &Quantity,
    ctx: &Context,
) -> Result<Quantity, String> {
    let (unit, multiplier) = combine_units_with_multiplier(
        left_qty.unit.as_deref(),
        right_qty.unit.as_deref(),
        false,
        &ctx.exchange_rates,
    );
    let value = left_qty.value * right_qty.value * multiplier;
    Ok(Quantity {
        display: None,
        is_bool: false,
        list: None,
        value,
        unit,
    })
}

fn quantity_div(
    left_qty: &Quantity,
    right_qty: &Quantity,
    ctx: &Context,
) -> Result<Quantity, String> {
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
    Ok(Quantity {
        display: None,
        is_bool: false,
        list: None,
        value,
        unit,
    })
}

fn quantity_pow(left_qty: &Quantity, right_qty: &Quantity) -> Result<Quantity, String> {
    if right_qty.unit.is_some() {
        return Err("Exponent power must be a dimensionless scalar".to_string());
    }
    let value = left_qty.value.powf(right_qty.value);
    Ok(Quantity {
        display: None,
        is_bool: false,
        list: None,
        value,
        unit: left_qty.unit.clone(),
    })
}

fn expr_contains_var(expr: &Expr, var_name: &str) -> bool {
    match expr {
        Expr::Variable(name) => name == var_name,
        Expr::Percentage(inner)
        | Expr::Factorial(inner)
        | Expr::Not(inner)
        | Expr::BitNot(inner)
        | Expr::Convert(inner, _) => expr_contains_var(inner, var_name),
        Expr::BinaryOp(_, left, right) => {
            expr_contains_var(left, var_name) || expr_contains_var(right, var_name)
        }
        Expr::FnCall(_, args) | Expr::List(args) => {
            args.iter().any(|arg| expr_contains_var(arg, var_name))
        }
        Expr::Number(_) | Expr::Quantity(_, _) | Expr::StringLiteral(_) | Expr::DateTime { .. } => {
            false
        }
        Expr::Block(exprs) => exprs.iter().any(|e| expr_contains_var(e, var_name)),
        Expr::LocalAssign(name, val_expr) => {
            name == var_name || expr_contains_var(val_expr, var_name)
        }
        Expr::IfElse {
            cond,
            then_expr,
            else_expr,
        } => {
            expr_contains_var(cond, var_name)
                || expr_contains_var(then_expr, var_name)
                || expr_contains_var(else_expr, var_name)
        }
        Expr::Switch {
            val,
            cases,
            default_case,
        } => {
            expr_contains_var(val, var_name)
                || cases.iter().any(|(pat, body)| {
                    expr_contains_var(pat, var_name) || expr_contains_var(body, var_name)
                })
                || default_case
                    .as_ref()
                    .is_some_and(|def| expr_contains_var(def, var_name))
        }
        Expr::For {
            var,
            iterable,
            body,
        } => {
            var == var_name
                || expr_contains_var(iterable, var_name)
                || expr_contains_var(body, var_name)
        }
        Expr::While { cond, body } => {
            expr_contains_var(cond, var_name) || expr_contains_var(body, var_name)
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
            let target_val = Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: 0.0,
                unit: None,
            };
            solve_rec(expr, target_val, var_name, ctx)
        }
    }
}

fn solve_rec(
    expr: &Expr,
    target_val: Quantity,
    var_name: &str,
    ctx: &mut Context,
) -> Result<Quantity, String> {
    match expr {
        Expr::Variable(name) if name == var_name => Ok(target_val),
        Expr::BinaryOp(op, left, right) => {
            let left_has = expr_contains_var(left, var_name);
            let right_has = expr_contains_var(right, var_name);
            if left_has && !right_has {
                let r_val = eval_expr(right, ctx)?;
                let next_target = match op {
                    Op::Add => quantity_sub(&target_val, &r_val, ctx)?,
                    Op::Sub => quantity_add(&target_val, &r_val, ctx)?,
                    Op::Mul => quantity_div(&target_val, &r_val, ctx)?,
                    Op::Div => quantity_mul(&target_val, &r_val, ctx)?,
                    Op::Pow => {
                        let one_over_r = Quantity {
                            display: None,
                            is_bool: false,
                            list: None,
                            value: 1.0 / r_val.value,
                            unit: None,
                        };
                        quantity_pow(&target_val, &one_over_r)?
                    }
                    _ => {
                        return Err(format!(
                            "Unsupported operator '{:?}' in equation solving",
                            op
                        ));
                    }
                };
                solve_rec(left, next_target, var_name, ctx)
            } else if right_has && !left_has {
                let l_val = eval_expr(left, ctx)?;
                let next_target = match op {
                    Op::Add => quantity_sub(&target_val, &l_val, ctx)?,
                    Op::Sub => quantity_sub(&l_val, &target_val, ctx)?,
                    Op::Mul => quantity_div(&target_val, &l_val, ctx)?,
                    Op::Div => quantity_div(&l_val, &target_val, ctx)?,
                    _ => {
                        return Err(format!(
                            "Unsupported operator '{:?}' in equation solving",
                            op
                        ));
                    }
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

/// Symbolic counterpart to [`solve_equation`]. Instead of evaluating the
/// non-target side to a number and applying inverse *quantity* ops, it applies
/// the inverse ops as AST nodes, producing a formula for the target variable in
/// terms of the still-free variables. Used when the equation cannot be resolved
/// to a concrete value because other variables are unbound — so the user sees
/// the rearrangement (`c = x - 2`) rather than a failure.
fn solve_symbolic(expr: &Expr, var_name: &str) -> Result<Quantity, String> {
    let formula = match expr {
        Expr::BinaryOp(Op::Eq, left, right) => {
            let left_has = expr_contains_var(left, var_name);
            let right_has = expr_contains_var(right, var_name);
            if left_has && !right_has {
                solve_rec_symbolic(left, (**right).clone(), var_name)?
            } else if right_has && !left_has {
                solve_rec_symbolic(right, (**left).clone(), var_name)?
            } else if !left_has && !right_has {
                return Err("Equation does not contain the variable to solve for".to_string());
            } else {
                return Err("Variable appears on both sides of the equation, which is not supported by the simple solver".to_string());
            }
        }
        // A bare expression is rearranged as `expr == 0`.
        _ => solve_rec_symbolic(expr, Expr::Number(0.0), var_name)?,
    };
    let simplified = simplify(&formula);
    Ok(Quantity {
        display: None,
        is_bool: false,
        list: None,
        value: 1.0,
        unit: Some(format!("formula:{}", expr_to_string(&simplified))),
    })
}

/// Structural mirror of [`solve_rec`] that peels operators off `expr`, applying
/// each inverse op to `target` as a new AST node instead of a numeric value.
fn solve_rec_symbolic(expr: &Expr, target: Expr, var_name: &str) -> Result<Expr, String> {
    match expr {
        Expr::Variable(name) if name == var_name => Ok(target),
        Expr::BinaryOp(op, left, right) => {
            let left_has = expr_contains_var(left, var_name);
            let right_has = expr_contains_var(right, var_name);
            if left_has && !right_has {
                let next_target = match op {
                    Op::Add => Expr::BinaryOp(Op::Sub, Box::new(target), right.clone()),
                    Op::Sub => Expr::BinaryOp(Op::Add, Box::new(target), right.clone()),
                    Op::Mul => Expr::BinaryOp(Op::Div, Box::new(target), right.clone()),
                    Op::Div => Expr::BinaryOp(Op::Mul, Box::new(target), right.clone()),
                    Op::Pow => Expr::BinaryOp(
                        Op::Pow,
                        Box::new(target),
                        Box::new(Expr::BinaryOp(
                            Op::Div,
                            Box::new(Expr::Number(1.0)),
                            right.clone(),
                        )),
                    ),
                    _ => {
                        return Err(format!(
                            "Unsupported operator '{:?}' in equation solving",
                            op
                        ));
                    }
                };
                solve_rec_symbolic(left, next_target, var_name)
            } else if right_has && !left_has {
                let next_target = match op {
                    Op::Add => Expr::BinaryOp(Op::Sub, Box::new(target), left.clone()),
                    Op::Sub => Expr::BinaryOp(Op::Sub, left.clone(), Box::new(target)),
                    Op::Mul => Expr::BinaryOp(Op::Div, Box::new(target), left.clone()),
                    Op::Div => Expr::BinaryOp(Op::Div, left.clone(), Box::new(target)),
                    _ => {
                        return Err(format!(
                            "Unsupported operator '{:?}' in equation solving",
                            op
                        ));
                    }
                };
                solve_rec_symbolic(right, next_target, var_name)
            } else if !left_has && !right_has {
                Err("Sub-expression does not contain the variable".to_string())
            } else {
                Err("Variable appears on both sides of a sub-expression".to_string())
            }
        }
        _ => Err("Equation is too complex or non-algebraic".to_string()),
    }
}

/// Multiply every (possibly nested) element of `list_qty` by the scalar `scalar`,
/// combining units element-wise. Used to wire `scalar * matrix` / `matrix * scalar`.
fn scale_list(list_qty: &Quantity, scalar: &Quantity, ctx: &Context) -> Result<Quantity, String> {
    let elements = list_qty
        .list
        .as_ref()
        .ok_or("scale_list expects a list/matrix")?;
    let mut out = Vec::with_capacity(elements.len());
    for el in elements {
        if el.list.is_some() {
            out.push(scale_list(el, scalar, ctx)?);
        } else {
            let (unit, multiplier) = combine_units_with_multiplier(
                el.unit.as_deref(),
                scalar.unit.as_deref(),
                false,
                &ctx.exchange_rates,
            );
            out.push(Quantity::scalar(el.value * scalar.value * multiplier, unit));
        }
    }
    Ok(Quantity::list(out))
}

fn matmul_impl(q1: &Quantity, q2: &Quantity, ctx: &Context) -> Result<Quantity, String> {
    let el1 = q1
        .list
        .as_ref()
        .ok_or("matmul expects first argument to be a list/matrix")?;
    let el2 = q2
        .list
        .as_ref()
        .ok_or("matmul expects second argument to be a list/matrix")?;
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
            let row_el = row
                .list
                .as_ref()
                .ok_or("matmul expects a 2D matrix or 1D vector")?;
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
            let row_el = row
                .list
                .as_ref()
                .ok_or("matmul expects a 2D matrix or 1D vector")?;
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
                            let converted =
                                convert_quantity(term_val, u2, u1, &ctx.exchange_rates)?;
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
        let flat_res: Vec<Quantity> = result_matrix
            .into_iter()
            .map(|row| row[0].clone())
            .collect();
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
        (None, None) => match (&q1.unit, &q2.unit) {
            (Some(u1), Some(u2)) if are_compatible(u1, u2) => {
                if let Ok(q2_conv) = convert_quantity(q2.value, u2, u1, exchange_rates) {
                    (q1.value - q2_conv).abs() < 1e-9
                } else {
                    false
                }
            }
            (None, None) => (q1.value - q2.value).abs() < 1e-9,
            _ => false,
        },
        _ => false,
    }
}

fn eval_ne_logic(q1: &Quantity, q2: &Quantity, exchange_rates: &HashMap<String, f64>) -> bool {
    !eval_eq_logic(q1, q2, exchange_rates)
}

fn eval_lt_logic(
    q1: &Quantity,
    q2: &Quantity,
    exchange_rates: &HashMap<String, f64>,
) -> Result<bool, String> {
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

fn eval_lte_logic(
    q1: &Quantity,
    q2: &Quantity,
    exchange_rates: &HashMap<String, f64>,
) -> Result<bool, String> {
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

fn eval_gt_logic(
    q1: &Quantity,
    q2: &Quantity,
    exchange_rates: &HashMap<String, f64>,
) -> Result<bool, String> {
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

fn eval_gte_logic(
    q1: &Quantity,
    q2: &Quantity,
    exchange_rates: &HashMap<String, f64>,
) -> Result<bool, String> {
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
            Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: std::f64::consts::PI,
                unit: None,
            },
        );
        variables.insert(
            "e".to_string(),
            Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: std::f64::consts::E,
                unit: None,
            },
        );
        variables.insert(
            "inf".to_string(),
            Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: f64::INFINITY,
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
                    display: None,
                    is_bool: false,
                    list: None,
                    value,
                    unit: unit.map(|u| u.to_string()),
                },
            );
            // Register the constant's unit only if the name isn't already a built-in
            // unit abbreviation. Otherwise a constant like `h` (Planck) or `g` (grav.
            // accel.) shadows the built-in `h`=hour / `g`=gram, which breaks compound
            // unit resolution in the convert path (e.g. `X to km/h` splits to `km` `h`
            // and resolves `h` to Planck's constant). The value stays available as a
            // variable above.
            if let Some(unit_str) = unit
                && crate::math::units::get_exact_unit_info(name).is_none()
            {
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
        Ok(crate::math::units::auto_scale_quantity(
            qty,
            &ctx.exchange_rates,
        ))
    }
}

pub fn eval_expr(expr: &Expr, ctx: &mut Context) -> Result<Quantity, String> {
    match expr {
        Expr::Number(val) => Ok(Quantity {
            display: None,
            is_bool: false,
            list: None,
            value: *val,
            unit: None,
        }),
        Expr::Quantity(val, unit) => Ok(Quantity {
            display: None,
            is_bool: false,
            list: None,
            value: *val,
            unit: Some(unit.clone()),
        }),
        Expr::DateTime {
            epoch_secs,
            kind,
            tz_offset_secs,
        } => Ok(Quantity {
            display: Some(crate::math::parser::DisplayFormat::DateTime {
                kind: *kind,
                tz_offset_secs: *tz_offset_secs,
            }),
            is_bool: false,
            list: None,
            value: *epoch_secs,
            unit: None,
        }),
        Expr::Variable(name) => {
            if let Some(val) = ctx.variables.get(name) {
                Ok(val.clone())
            } else {
                Ok(Quantity {
                    display: None,
                    is_bool: false,
                    list: None,
                    value: 1.0,
                    unit: Some(name.clone()),
                })
            }
        }
        Expr::Percentage(inner) => {
            let qty = eval_expr(inner, ctx)?;
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: qty.value * 0.01,
                unit: qty.unit,
            })
        }
        Expr::Factorial(inner) => {
            let qty = eval_expr(inner, ctx)?;
            if qty.unit.is_some() || qty.list.is_some() {
                return Err("Factorial expects a dimensionless number".to_string());
            }
            let n = qty.value;
            if n < 0.0 && n.fract() == 0.0 {
                return Err("Factorial is undefined for negative integers".to_string());
            }
            // n! = Γ(n+1); reuse the gamma builtin so non-integer args work too.
            let result = builtins::math::gamma("!", &[Quantity::scalar(n + 1.0, None)])?;
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: result.value,
                unit: None,
            })
        }
        Expr::Block(exprs) => control::eval_block(exprs, ctx),
        Expr::For {
            var,
            iterable,
            body,
        } => control::eval_for(var, iterable, body, ctx),
        Expr::While { cond, body } => control::eval_while(cond, body, ctx),
        Expr::LocalAssign(name, val_expr) => control::eval_local_assign(name, val_expr, ctx),
        Expr::IfElse {
            cond,
            then_expr,
            else_expr,
        } => control::eval_if_else(cond, then_expr, else_expr, ctx),
        Expr::Switch {
            val,
            cases,
            default_case,
        } => control::eval_switch(val, cases, default_case.as_deref(), ctx),
        Expr::StringLiteral(val) => Ok(Quantity {
            display: None,
            value: 0.0,
            unit: Some(val.clone()),
            list: None,
            is_bool: false,
        }),
        Expr::Convert(inner_expr, target_unit) => {
            let qty = eval_expr(inner_expr, ctx)?;
            if target_unit == "hex"
                || target_unit == "HEX"
                || target_unit == "bin"
                || target_unit == "BIN"
            {
                return Ok(Quantity {
                    display: None,
                    is_bool: qty.is_bool,
                    list: qty.list,
                    value: qty.value,
                    unit: Some(target_unit.to_lowercase()),
                });
            }
            if target_unit == "%" || target_unit == "percent" {
                // Display-only: keep the value untouched (so further math is
                // unaffected) and tag it to render ×100 with a `%` suffix.
                if qty.unit.is_some() {
                    return Err(format!(
                        "Cannot render '{}' as a percentage",
                        qty.unit.as_deref().unwrap_or("")
                    ));
                }
                if qty.list.is_some() {
                    return Err("Cannot render a list as a percentage".to_string());
                }
                return Ok(Quantity {
                    is_bool: false,
                    list: None,
                    value: qty.value,
                    unit: None,
                    display: Some(crate::math::parser::DisplayFormat::Percent),
                });
            }
            // Date/time zone conversion: `<datetime> in <zone>`. Keeps the
            // instant (epoch value) and re-renders it in the target zone.
            if let Some(crate::math::parser::DisplayFormat::DateTime { kind, .. }) = &qty.display {
                let kind = *kind;
                let tz = crate::math::datetime::resolve_timezone(target_unit)?;
                let ts = jiff::Timestamp::from_second(qty.value as i64)
                    .map_err(|e| format!("Invalid date/time: {e}"))?;
                let zoned = ts.to_zoned(tz);
                // A bare date stays a date only if it lands exactly on midnight
                // in the new zone; otherwise it gains a time component.
                let new_kind = if matches!(kind, crate::math::parser::DateTimeKind::Date)
                    && zoned.hour() == 0
                    && zoned.minute() == 0
                    && zoned.second() == 0
                {
                    crate::math::parser::DateTimeKind::Date
                } else {
                    crate::math::parser::DateTimeKind::DateTime
                };
                return Ok(Quantity {
                    display: Some(crate::math::parser::DisplayFormat::DateTime {
                        kind: new_kind,
                        tz_offset_secs: zoned.offset().seconds(),
                    }),
                    is_bool: false,
                    list: None,
                    value: qty.value,
                    unit: None,
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
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
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
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: val as f64,
                unit: qty.unit,
            })
        }
        Expr::FnCall(name, args) => {
            match name.as_str() {
                "solve" => return higher_order::solve(args, ctx),
                "diff" | "der" => return higher_order::diff(name, args, ctx),
                "map" => return higher_order::map(args, ctx),
                "filter" => return higher_order::filter(args, ctx),
                "any" => return higher_order::any(args, ctx),
                "all" => return higher_order::all(args, ctx),
                "zip" => return higher_order::zip(args, ctx),
                "reduce" => return higher_order::reduce(args, ctx),
                _ => {}
            }

            // Evaluate arguments
            let mut arg_vals = Vec::new();
            for arg in args {
                arg_vals.push(eval_expr(arg, ctx)?);
            }

            // Check built-in functions
            match name.as_str() {
                "sin" => builtins::trig::sin(name, &arg_vals),
                "cos" => builtins::trig::cos(name, &arg_vals),
                "tan" => builtins::trig::tan(name, &arg_vals),
                "asin" => builtins::trig::asin(name, &arg_vals),
                "acos" => builtins::trig::acos(name, &arg_vals),
                "atan" => builtins::trig::atan(name, &arg_vals),
                "sinh" => builtins::trig::sinh(name, &arg_vals),
                "cosh" => builtins::trig::cosh(name, &arg_vals),
                "tanh" => builtins::trig::tanh(name, &arg_vals),
                "asinh" => builtins::trig::asinh(name, &arg_vals),
                "acosh" => builtins::trig::acosh(name, &arg_vals),
                "atanh" => builtins::trig::atanh(name, &arg_vals),
                "exp" => builtins::trig::exp(name, &arg_vals),
                "sum" => builtins::stats::sum(&arg_vals, ctx),
                "prod" | "product" => builtins::stats::prod(&arg_vals, ctx),
                "mean" | "average" => builtins::stats::mean(&arg_vals, ctx),
                "median" => builtins::stats::median(&arg_vals, ctx),
                "stddev" | "stdev" => builtins::stats::stddev(&arg_vals, ctx),
                "var" | "variance" => builtins::stats::variance(&arg_vals, ctx),
                "len" => builtins::vector::len(name, &arg_vals),
                "count" => builtins::stats::count(&arg_vals),
                "vdot" => builtins::vector::vdot(name, &arg_vals, ctx),
                "vadd" => builtins::vector::vadd(name, &arg_vals, ctx),
                "vsub" => builtins::vector::vsub(name, &arg_vals, ctx),
                "transpose" => builtins::vector::transpose(name, &arg_vals),
                "matmul" => builtins::vector::matmul(name, &arg_vals, ctx),
                "det" => builtins::vector::det(name, &arg_vals),
                "inv" => builtins::vector::inv(name, &arg_vals),
                "linsolve" => builtins::vector::linsolve(name, &arg_vals),
                "if" => builtins::logic::if_(name, &arg_vals),
                "and" => builtins::logic::and(&arg_vals),
                "or" => builtins::logic::or(&arg_vals),
                "not" => builtins::logic::not(name, &arg_vals),
                "eq" => builtins::logic::eq(name, &arg_vals, ctx),
                "ne" => builtins::logic::ne(name, &arg_vals, ctx),
                "lt" => builtins::logic::lt(name, &arg_vals, ctx),
                "lte" => builtins::logic::lte(name, &arg_vals, ctx),
                "gt" => builtins::logic::gt(name, &arg_vals, ctx),
                "gte" => builtins::logic::gte(name, &arg_vals, ctx),
                "log" => builtins::math::log(&arg_vals),
                "ln" => builtins::math::ln(name, &arg_vals),
                "log2" => builtins::math::log2(name, &arg_vals),
                "sqrt" => builtins::math::sqrt(name, &arg_vals),
                "gamma" => builtins::math::gamma(name, &arg_vals),
                "lgamma" => builtins::math::lgamma(name, &arg_vals),
                "beta" => builtins::math::beta(name, &arg_vals),
                "erf" => builtins::math::erf(name, &arg_vals),
                "erfc" => builtins::math::erfc(name, &arg_vals),
                "normpdf" => builtins::stats::normpdf(name, &arg_vals),
                "normcdf" => builtins::stats::normcdf(name, &arg_vals),
                "erfinv" => builtins::math::erfinv(name, &arg_vals),
                "norminv" => builtins::stats::norminv(name, &arg_vals),
                "abs" => builtins::math::abs(name, &arg_vals),
                "round" => builtins::math::round(&arg_vals),
                "xor" => builtins::math::xor(name, &arg_vals),
                "ceil" => builtins::math::ceil(name, &arg_vals),
                "floor" => builtins::math::floor(name, &arg_vals),
                "plot" | "sparkline" => builtins::math::plot(&arg_vals),
                "mod" => builtins::math::modulo(name, &arg_vals, ctx),
                "min" => builtins::stats::min(&arg_vals, ctx),
                "max" => builtins::stats::max(&arg_vals, ctx),
                "pmt" => builtins::finance::pmt(name, &arg_vals),
                "fv" => builtins::finance::fv(&arg_vals),
                "pv" => builtins::finance::pv(&arg_vals),
                "range" => builtins::math::range(&arg_vals),
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

                return Ok(Quantity {
                    display: None,
                    is_bool: false,
                    list: None,
                    value: final_val,
                    unit: left_qty.unit,
                });
            }

            // Standard evaluation
            let left_qty = eval_expr(left_expr, ctx)?;
            let right_qty = eval_expr(right_expr, ctx)?;

            arithmetic::eval_binary_op(op, left_qty, right_qty, ctx)
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
        if abs_val < 1e-9 && abs_val > 0.0 {
            // Scientific notation, 4 significant figures (e.g. `2.152e-17`).
            // Trim trailing zeros from the mantissa ONLY — trimming the whole
            // string would corrupt the exponent (`9.87e-10` -> `9.87e-1`).
            let s = format!("{:.3e}", val);
            return match s.split_once('e') {
                Some((mantissa, exp)) => {
                    let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
                    format!("{}e{}", mantissa, exp)
                }
                None => s,
            };
        }
        let formatted = if abs_val < 1e-4 && abs_val > 0.0 {
            format!("{:.10}", val)
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
/// Render an epoch-seconds (UTC) `value` as an ISO 8601 date/time string in the
/// zone given by `tz_offset_secs`. Falls back to the raw number on any error so
/// formatting is always infallible.
fn format_datetime(
    epoch_secs: f64,
    kind: crate::math::parser::DateTimeKind,
    tz_offset_secs: i32,
) -> String {
    use crate::math::parser::DateTimeKind;
    let (Ok(ts), Ok(offset)) = (
        jiff::Timestamp::from_second(epoch_secs as i64),
        jiff::tz::Offset::from_seconds(tz_offset_secs),
    ) else {
        return format_float(epoch_secs);
    };
    let zoned = ts.to_zoned(jiff::tz::TimeZone::fixed(offset));
    match kind {
        DateTimeKind::Date => zoned.strftime("%Y-%m-%d").to_string(),
        DateTimeKind::DateTime => zoned.strftime("%Y-%m-%dT%H:%M").to_string(),
    }
}

pub fn format_quantity(qty: &Quantity) -> String {
    if qty.display == Some(crate::math::parser::DisplayFormat::Percent) {
        return format!("{}%", format_float(qty.value * 100.0));
    }
    if let Some(crate::math::parser::DisplayFormat::DateTime {
        kind,
        tz_offset_secs,
    }) = qty.display
    {
        return format_datetime(qty.value, kind, tz_offset_secs);
    }
    if let Some(ref u) = qty.unit {
        if let Some(rest) = u.strip_prefix("sparkline:") {
            return rest.to_string();
        }
        if let Some(rest) = u.strip_prefix("formula:") {
            return rest.to_string();
        }
        if u == "complex"
            && let Some(ref list) = qty.list
            && list.len() >= 2
        {
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
        return if qty.value != 0.0 {
            "True".to_string()
        } else {
            "False".to_string()
        };
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
                    let starts_with_word = adjusted_u
                        .chars()
                        .next()
                        .map(|c| c.is_alphabetic())
                        .unwrap_or(false);
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
        Expr::Factorial(inner) => find_variable_in_expr(inner),
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
        Expr::Block(exprs) => {
            for ex in exprs {
                if let Some(v) = find_variable_in_expr(ex) {
                    return Some(v);
                }
            }
            None
        }
        Expr::IfElse {
            cond,
            then_expr,
            else_expr,
        } => find_variable_in_expr(cond)
            .or_else(|| find_variable_in_expr(then_expr))
            .or_else(|| find_variable_in_expr(else_expr)),
        Expr::Switch {
            val,
            cases,
            default_case,
        } => find_variable_in_expr(val)
            .or_else(|| {
                for (p, b) in cases {
                    if let Some(v) = find_variable_in_expr(p).or_else(|| find_variable_in_expr(b)) {
                        return Some(v);
                    }
                }
                None
            })
            .or_else(|| {
                default_case
                    .as_ref()
                    .and_then(|def| find_variable_in_expr(def))
            }),
        Expr::LocalAssign(_, val_expr) => find_variable_in_expr(val_expr),
        Expr::For {
            var: _,
            iterable,
            body,
        } => find_variable_in_expr(iterable).or_else(|| find_variable_in_expr(body)),
        Expr::While { cond, body } => {
            find_variable_in_expr(cond).or_else(|| find_variable_in_expr(body))
        }
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
        Expr::Variable(name) if !vars.contains(name) => {
            vars.push(name.clone());
        }
        Expr::Percentage(inner) => find_all_variables_in_expr_helper(inner, vars),
        Expr::Factorial(inner) => find_all_variables_in_expr_helper(inner, vars),
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
        Expr::Block(exprs) => {
            for ex in exprs {
                find_all_variables_in_expr_helper(ex, vars);
            }
        }
        Expr::IfElse {
            cond,
            then_expr,
            else_expr,
        } => {
            find_all_variables_in_expr_helper(cond, vars);
            find_all_variables_in_expr_helper(then_expr, vars);
            find_all_variables_in_expr_helper(else_expr, vars);
        }
        Expr::Switch {
            val,
            cases,
            default_case,
        } => {
            find_all_variables_in_expr_helper(val, vars);
            for (p, b) in cases {
                find_all_variables_in_expr_helper(p, vars);
                find_all_variables_in_expr_helper(b, vars);
            }
            if let Some(def) = default_case {
                find_all_variables_in_expr_helper(def, vars);
            }
        }
        Expr::LocalAssign(name, val_expr) => {
            if !vars.contains(name) {
                vars.push(name.clone());
            }
            find_all_variables_in_expr_helper(val_expr, vars);
        }
        Expr::For {
            var: _,
            iterable,
            body,
        } => {
            find_all_variables_in_expr_helper(iterable, vars);
            find_all_variables_in_expr_helper(body, vars);
        }
        Expr::While { cond, body } => {
            find_all_variables_in_expr_helper(cond, vars);
            find_all_variables_in_expr_helper(body, vars);
        }
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
    use crate::math::parser::{Line, parse_line};

    #[test]
    fn test_gamma() {
        let ev = |s: &str| -> f64 {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx)
                .unwrap()
                .value
        };
        // Γ(n) = (n-1)! for positive integers
        assert!((ev("gamma(5) =>") - 24.0).abs() < 1e-9);
        assert!((ev("gamma(1) =>") - 1.0).abs() < 1e-9);
        // Γ(1/2) = sqrt(pi)
        assert!((ev("gamma(0.5) =>") - std::f64::consts::PI.sqrt()).abs() < 1e-9);
        // reflection: Γ(-0.5) = -2·sqrt(pi)
        assert!((ev("gamma(-0.5) =>") - (-2.0 * std::f64::consts::PI.sqrt())).abs() < 1e-9);
        // poles at non-positive integers error; units rejected
        let mut ctx = Context::default();
        assert!(eval_expr(&parse_line("gamma(0) =>").unwrap_expr(), &mut ctx).is_err());
        assert!(eval_expr(&parse_line("gamma(-3) =>").unwrap_expr(), &mut ctx).is_err());
        assert!(eval_expr(&parse_line("gamma(5 m) =>").unwrap_expr(), &mut ctx).is_err());
    }

    #[test]
    fn test_format_float_small_scientific() {
        // 4 significant figures, trailing mantissa zeros trimmed, exponent intact
        // (the trim must not turn e-10 into e-1).
        assert_eq!(format_float(2.151973671249889e-17), "2.152e-17");
        assert_eq!(format_float(9.865876450377006e-10), "9.866e-10");
        assert_eq!(format_float(5e-10), "5e-10");
        assert_eq!(format_float(1.5e-12), "1.5e-12");
        assert_eq!(format_float(2e-20), "2e-20");
        // normcdf(-6) renders with the correct exponent end-to-end
        let mut ctx = Context::default();
        let s = format_quantity(
            &eval_expr(&parse_line("normcdf(-6) =>").unwrap_expr(), &mut ctx).unwrap(),
        );
        assert_eq!(s, "9.866e-10");
        // larger-magnitude values keep their trailing-zero trim
        assert_eq!(format_float(1.5e-6), "0.0000015");
        assert_eq!(format_float(2.5), "2.5");
    }

    #[test]
    fn test_percent_display() {
        let fmt = |s: &str| -> String {
            let mut ctx = Context::default();
            format_quantity(&eval_expr(&parse_line(s).unwrap_expr(), &mut ctx).unwrap())
        };
        assert_eq!(fmt("1/2 in % =>"), "50%");
        assert_eq!(fmt("1 in % =>"), "100%");
        assert_eq!(fmt("0.075 to % =>"), "7.5%");
        assert_eq!(fmt("0.333 in percent =>"), "33.3%");
        // value is preserved for further math (not scaled by the display tag):
        // (1/2 in %) * 2 = 1, rendered plainly once the tag is dropped by arithmetic.
        assert_eq!(fmt("(1/2 in %) * 2 =>"), "1");
        // a dimensioned value cannot be rendered as a percentage
        let mut ctx = Context::default();
        assert!(eval_expr(&parse_line("5 m in % =>").unwrap_expr(), &mut ctx).is_err());
        // `%` is a display target only as the whole target; `km%` is just an
        // unknown unit, so it errors rather than inventing a value.
        assert!(eval_expr(&parse_line("5000 m in km% =>").unwrap_expr(), &mut ctx).is_err());
    }

    #[test]
    fn test_gamma_factorial_edge_cases() {
        let mut ctx = Context::default();
        let err = |s: &str| {
            let mut c = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut c).is_err()
        };
        // overflow surfaces as an error, not a silent inf
        assert!(err("gamma(172) =>"));
        assert!(err("171! =>"));
        // still fine just below the overflow threshold
        assert!(
            eval_expr(&parse_line("gamma(170) =>").unwrap_expr(), &mut ctx)
                .unwrap()
                .value
                .is_finite()
        );
        // list arguments are rejected (consistent with normpdf/normcdf), not
        // silently reduced to the first element
        assert!(err("gamma([2, 3, 4]) =>"));
        assert!(err("erf([0.1, 0.2]) =>"));
        assert!(err("beta([1, 2], 3) =>"));
    }

    #[test]
    fn test_erfinv_norminv() {
        let ev = |s: &str| -> f64 {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx)
                .unwrap()
                .value
        };
        assert!(ev("erfinv(0) =>").abs() < 1e-9);
        // round-trip against erf (both use the same A&S kernel)
        assert!((ev("erfinv(erf(0.6)) =>") - 0.6).abs() < 1e-4);
        assert!((ev("erf(erfinv(0.4)) =>") - 0.4).abs() < 1e-6);
        // probit: norminv(0.975) ≈ 1.959964 (the 95% two-sided z)
        assert!((ev("norminv(0.975) =>") - 1.959_963_98).abs() < 1e-3);
        assert!(ev("norminv(0.5) =>").abs() < 1e-9);
        assert!((ev("norminv(0.5, 10, 2) =>") - 10.0).abs() < 1e-9);
        // norminv inverts normcdf
        assert!((ev("norminv(normcdf(1.2)) =>") - 1.2).abs() < 1e-4);
        // domain errors
        let mut ctx = Context::default();
        assert!(eval_expr(&parse_line("erfinv(1) =>").unwrap_expr(), &mut ctx).is_err());
        assert!(eval_expr(&parse_line("norminv(0) =>").unwrap_expr(), &mut ctx).is_err());
        assert!(eval_expr(&parse_line("norminv(1) =>").unwrap_expr(), &mut ctx).is_err());
    }

    #[test]
    fn test_normal_distribution() {
        let ev = |s: &str| -> f64 {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx)
                .unwrap()
                .value
        };
        // standard normal
        assert!((ev("normcdf(0) =>") - 0.5).abs() < 1e-9);
        assert!((ev("normpdf(0) =>") - 1.0 / (2.0 * std::f64::consts::PI).sqrt()).abs() < 1e-9);
        // 68-95-99.7: P(|Z|<1.96) ≈ 0.95, so normcdf(1.96) ≈ 0.975
        assert!((ev("normcdf(1.96) =>") - 0.975).abs() < 1e-3);
        // symmetry of the pdf and cdf about mu
        assert!((ev("normpdf(-1.5) =>") - ev("normpdf(1.5) =>")).abs() < 1e-12);
        assert!((ev("normcdf(-1.0) =>") + ev("normcdf(1.0) =>") - 1.0).abs() < 1e-6);
        // parameterized: normcdf(mu, mu, sigma) = 0.5 for any sigma
        assert!((ev("normcdf(10, 10, 3) =>") - 0.5).abs() < 1e-9);
        // left tail is accurate now, not ~75x off: normcdf(-6) ≈ 9.866e-10
        assert!((ev("normcdf(-6) =>") - 9.865_876_e-10).abs() < 1e-14);
        // sigma <= 0 and units error
        let mut ctx = Context::default();
        assert!(eval_expr(&parse_line("normpdf(1, 0, 0) =>").unwrap_expr(), &mut ctx).is_err());
        assert!(eval_expr(&parse_line("normcdf(1 m) =>").unwrap_expr(), &mut ctx).is_err());
    }

    #[test]
    fn test_erf() {
        let ev = |s: &str| -> f64 {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx)
                .unwrap()
                .value
        };
        assert!(ev("erf(0) =>").abs() < 1e-9);
        // erf(1) ≈ 0.8427007929
        assert!((ev("erf(1) =>") - 0.842_700_792_9).abs() < 1e-6);
        // odd: erf(-x) = -erf(x)
        assert!((ev("erf(-0.7) =>") + ev("erf(0.7) =>")).abs() < 1e-9);
        // high accuracy across the range (incomplete-gamma kernel, ~1e-14)
        assert!((ev("erf(2) =>") - 0.995_322_265_018_952_7).abs() < 1e-12);
        // complement: erf(x) + erfc(x) = 1
        assert!((ev("erf(1.3) =>") + ev("erfc(1.3) =>") - 1.0).abs() < 1e-12);
        // erfc tail survives instead of collapsing: erfc(5) ≈ 1.5375e-12,
        // erfc(6) ≈ 2.1519e-17 (the old 1 - erf kernel returned 0 here)
        assert!((ev("erfc(5) =>") - 1.537_459_794_428_e-12).abs() < 1e-16);
        assert!(ev("erfc(6) =>") > 0.0 && ev("erfc(6) =>") < 1e-16);
        let mut ctx = Context::default();
        assert!(eval_expr(&parse_line("erf(1 m) =>").unwrap_expr(), &mut ctx).is_err());
    }

    #[test]
    fn test_beta() {
        let ev = |s: &str| -> f64 {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx)
                .unwrap()
                .value
        };
        // B(1,1) = 1; B(2,3) = 1/12; B(a,b) symmetric
        assert!((ev("beta(1, 1) =>") - 1.0).abs() < 1e-9);
        assert!((ev("beta(2, 3) =>") - 1.0 / 12.0).abs() < 1e-9);
        assert!((ev("beta(2.5, 4.5) =>") - ev("beta(4.5, 2.5) =>")).abs() < 1e-12);
        // non-positive args and units error
        let mut ctx = Context::default();
        assert!(eval_expr(&parse_line("beta(0, 1) =>").unwrap_expr(), &mut ctx).is_err());
        assert!(eval_expr(&parse_line("beta(2, 3 m) =>").unwrap_expr(), &mut ctx).is_err());
    }

    #[test]
    fn test_lgamma() {
        let ev = |s: &str| -> f64 {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx)
                .unwrap()
                .value
        };
        // lgamma(x) = ln(gamma(x)); gamma(5)=24
        assert!((ev("lgamma(5) =>") - 24.0f64.ln()).abs() < 1e-9);
        assert!(ev("lgamma(1) =>").abs() < 1e-9); // ln(1) = 0
        // stable for large args where gamma overflows (Γ(171) is > f64::MAX):
        // recurrence lgamma(x+1) − lgamma(x) = ln(x)
        assert!((ev("lgamma(171) =>") - ev("lgamma(170) =>") - 170.0f64.ln()).abs() < 1e-6);
        assert!(ev("lgamma(171) =>").is_finite());
        let mut ctx = Context::default();
        assert!(eval_expr(&parse_line("lgamma(0) =>").unwrap_expr(), &mut ctx).is_err());
    }

    #[test]
    fn test_factorial() {
        let ev = |s: &str| -> f64 {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx)
                .unwrap()
                .value
        };
        assert!((ev("5! =>") - 120.0).abs() < 1e-9);
        assert!((ev("0! =>") - 1.0).abs() < 1e-9);
        assert!((ev("1! =>") - 1.0).abs() < 1e-9);
        // non-integer via gamma: 0.5! = Γ(1.5) = sqrt(pi)/2
        assert!((ev("0.5! =>") - std::f64::consts::PI.sqrt() / 2.0).abs() < 1e-9);
        // binds tighter than *: 3 * 2! = 6, not (3*2)! = 720
        assert!((ev("3 * 2! =>") - 6.0).abs() < 1e-9);
        // negative integers and units error
        let mut ctx = Context::default();
        assert!(eval_expr(&parse_line("(-3)! =>").unwrap_expr(), &mut ctx).is_err());
        assert!(eval_expr(&parse_line("(5 m)! =>").unwrap_expr(), &mut ctx).is_err());
    }

    #[test]
    fn test_matrix_star_operator() {
        let ev = |s: &str| -> String {
            let mut ctx = Context::default();
            format_quantity(&eval_expr(&parse_line(s).unwrap_expr(), &mut ctx).unwrap())
        };
        // matrix * matrix => matmul
        assert_eq!(
            ev("[[1, 2], [3, 4]] * [[5, 6], [7, 8]] =>"),
            "[[19, 22], [43, 50]]"
        );
        // scalar * matrix and matrix * scalar => element-wise scale
        assert_eq!(ev("2 * [[1, 2], [3, 4]] =>"), "[[2, 4], [6, 8]]");
        assert_eq!(ev("[[1, 2], [3, 4]] * 3 =>"), "[[3, 6], [9, 12]]");
        // vector * scalar
        assert_eq!(ev("[1, 2, 3] * 10 =>"), "[10, 20, 30]");
        // dimension mismatch is an error, not a silent scalar
        let mut ctx = Context::default();
        assert!(
            eval_expr(
                &parse_line("[[1, 2]] * [[1, 2]] =>").unwrap_expr(),
                &mut ctx
            )
            .is_err()
        );
    }

    #[test]
    fn test_matrix_determinant() {
        let ev = |s: &str| -> f64 {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx)
                .unwrap()
                .value
        };
        assert!((ev("det([[1, 2], [3, 4]]) =>") - (-2.0)).abs() < 1e-9);
        assert!((ev("det([[2, 0], [0, 3]]) =>") - 6.0).abs() < 1e-9);
        assert!((ev("det([[6, 1, 1], [4, -2, 5], [2, 8, 7]]) =>") - (-306.0)).abs() < 1e-9);
        assert!(ev("det([[1, 2], [2, 4]]) =>").abs() < 1e-9); // singular => 0
        // non-square is an error
        let mut ctx = Context::default();
        assert!(
            eval_expr(
                &parse_line("det([[1, 2, 3], [4, 5, 6]]) =>").unwrap_expr(),
                &mut ctx
            )
            .is_err()
        );
    }

    #[test]
    fn test_matrix_inverse() {
        let eval_q = |s: &str| -> Quantity {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx).unwrap()
        };
        // Flatten a matrix Quantity to row-major values.
        let flat = |q: &Quantity| -> Vec<f64> {
            q.list
                .as_ref()
                .unwrap()
                .iter()
                .flat_map(|row| row.list.as_ref().unwrap().iter().map(|c| c.value))
                .collect()
        };
        // inv([[1,2],[3,4]]) = [[-2, 1], [1.5, -0.5]]
        let inv_a = flat(&eval_q("inv([[1, 2], [3, 4]]) =>"));
        for (got, exp) in inv_a.iter().zip([-2.0, 1.0, 1.5, -0.5].iter()) {
            assert!((got - exp).abs() < 1e-9, "inv got {got}, expected {exp}");
        }
        // Round-trip A * inv(A) ~= identity (exercises the wired matrix `*`).
        let prod = flat(&eval_q("[[1, 2], [3, 4]] * inv([[1, 2], [3, 4]]) =>"));
        for (got, exp) in prod.iter().zip([1.0, 0.0, 0.0, 1.0].iter()) {
            assert!(
                (got - exp).abs() < 1e-9,
                "round-trip got {got}, expected {exp}"
            );
        }
        // Singular and non-square are errors.
        let mut ctx = Context::default();
        assert!(
            eval_expr(
                &parse_line("inv([[1, 2], [2, 4]]) =>").unwrap_expr(),
                &mut ctx
            )
            .is_err()
        );
        assert!(
            eval_expr(
                &parse_line("inv([[1, 2, 3], [4, 5, 6]]) =>").unwrap_expr(),
                &mut ctx
            )
            .is_err()
        );
    }

    #[test]
    fn test_linear_system_solver() {
        let eval_q = |s: &str| -> Quantity {
            let mut ctx = Context::default();
            eval_expr(&parse_line(s).unwrap_expr(), &mut ctx).unwrap()
        };
        let vals = |q: &Quantity| -> Vec<f64> {
            q.list.as_ref().unwrap().iter().map(|c| c.value).collect()
        };
        // 2x1 + x2 = 3 ; x1 + 3x2 = 5  ->  x = [0.8, 1.4]
        let x = vals(&eval_q("linsolve([[2, 1], [1, 3]], [3, 5]) =>"));
        for (got, exp) in x.iter().zip([0.8, 1.4].iter()) {
            assert!((got - exp).abs() < 1e-9, "2x2 got {got}, expected {exp}");
        }
        // Classic 3x3 with solution [2, 3, -1].
        let x3 = vals(&eval_q(
            "linsolve([[2, 1, -1], [-3, -1, 2], [-2, 1, 2]], [8, -11, -3]) =>",
        ));
        for (got, exp) in x3.iter().zip([2.0, 3.0, -1.0].iter()) {
            assert!((got - exp).abs() < 1e-9, "3x3 got {got}, expected {exp}");
        }
        // Singular matrix -> error (no unique solution).
        let mut ctx = Context::default();
        assert!(
            eval_expr(
                &parse_line("linsolve([[1, 2], [2, 4]], [1, 2]) =>").unwrap_expr(),
                &mut ctx
            )
            .is_err()
        );
        // Right-hand side length mismatch -> error.
        assert!(
            eval_expr(
                &parse_line("linsolve([[1, 2], [3, 4]], [1, 2, 3]) =>").unwrap_expr(),
                &mut ctx
            )
            .is_err()
        );
    }

    #[test]
    fn test_scope_guard_no_leak_on_error() {
        // A block that errors mid-body must not leak the locals it defined
        // before the error (the RAII ScopeGuard prunes them on the early return).
        let block = Expr::Block(vec![
            Expr::LocalAssign("leaky".to_string(), Box::new(Expr::Number(42.0))),
            Expr::BinaryOp(
                Op::Div,
                Box::new(Expr::Number(1.0)),
                Box::new(Expr::Number(0.0)),
            ),
        ]);
        let mut ctx = Context::default();
        let res = eval_expr(&block, &mut ctx);
        assert!(res.is_err(), "expected block to error on 1/0");
        assert!(
            !ctx.variables.contains_key("leaky"),
            "block-local leaked into context on the error path"
        );

        // A for-loop whose body errors mid-iteration must likewise leave neither
        // the loop variable nor the body-locals behind.
        let for_expr = Expr::For {
            var: "i".to_string(),
            iterable: Box::new(Expr::List(vec![Expr::Number(1.0), Expr::Number(2.0)])),
            body: Box::new(Expr::Block(vec![
                Expr::LocalAssign(
                    "temp".to_string(),
                    Box::new(Expr::Variable("i".to_string())),
                ),
                Expr::BinaryOp(
                    Op::Div,
                    Box::new(Expr::Number(1.0)),
                    Box::new(Expr::Number(0.0)),
                ),
            ])),
        };
        let mut ctx = Context::default();
        let res = eval_expr(&for_expr, &mut ctx);
        assert!(res.is_err(), "expected loop body to error on 1/0");
        assert!(
            !ctx.variables.contains_key("temp"),
            "loop body-local leaked into context on the error path"
        );
        assert!(
            !ctx.variables.contains_key("i"),
            "loop variable leaked into context on the error path"
        );
    }

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
    fn test_datetime_tz_conversion() {
        use crate::math::parser::{DateTimeKind, Expr};
        let mut ctx = Context::default();
        let convert = |ctx: &mut Context, epoch: f64, zone: &str| {
            let e = Expr::Convert(
                Box::new(Expr::DateTime {
                    epoch_secs: epoch,
                    kind: DateTimeKind::DateTime,
                    tz_offset_secs: 0,
                }),
                zone.to_string(),
            );
            eval_expr(&e, ctx).map(|q| format_quantity(&q))
        };

        // Epoch 0 = 1970-01-01T00:00:00Z. Fixed-offset targets are tz-independent.
        assert_eq!(convert(&mut ctx, 0.0, "UTC").unwrap(), "1970-01-01T00:00");
        assert_eq!(convert(&mut ctx, 0.0, "UTC+2").unwrap(), "1970-01-01T02:00");
        assert_eq!(convert(&mut ctx, 0.0, "GMT-5").unwrap(), "1969-12-31T19:00");

        // Named zone, DST-aware: 2026-07-01T16:00Z → New York EDT (−4) = 12:00.
        let (summer, _) = crate::math::datetime::civil_to_epoch_in_zone(
            2026,
            7,
            1,
            16,
            0,
            0,
            &jiff::tz::TimeZone::UTC,
        )
        .unwrap();
        assert_eq!(
            convert(&mut ctx, summer, "America/New_York").unwrap(),
            "2026-07-01T12:00"
        );
        assert_eq!(
            convert(&mut ctx, summer, "PST").unwrap(),
            "2026-07-01T09:00"
        );

        // Unknown zone errors.
        assert!(convert(&mut ctx, 0.0, "Nowhere").is_err());

        // Parser folds a UTC offset into the conversion target...
        let toks = crate::math::lexer::Lexer::new("2026-08-01T09:00 in UTC+2")
            .lex()
            .unwrap();
        match crate::math::parser::Parser::new(toks).parse().unwrap() {
            Expr::Convert(_, t) => assert_eq!(t, "UTC+2"),
            other => panic!("expected Convert, got {:?}", other),
        }
        // ...but leaves trailing arithmetic on unit conversions alone.
        let toks2 = crate::math::lexer::Lexer::new("1 m in km + 5 m")
            .lex()
            .unwrap();
        match crate::math::parser::Parser::new(toks2).parse().unwrap() {
            Expr::BinaryOp(Op::Add, _, _) => {}
            other => panic!("expected (… in km) + 5 m, got {:?}", other),
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
        if let Line::FnDefinition {
            name, args, expr, ..
        } = def
        {
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
        let p = eval_expr(
            &parse_line("pmt(0.05 / 12, 60, -20000) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
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
        let fv1 = eval_expr(
            &parse_line("fv(0.05 / 12, 60, -377.424, 20000) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert!(fv1.value.abs() < 10.0);

        let pv1 = eval_expr(
            &parse_line("pv(0.05 / 12, 60, -377.424, 0) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert!((pv1.value - 20000.0).abs() < 10.0);

        // sum, mean, median, stddev, variance
        let s_val = eval_expr(
            &parse_line("sum(10m, 200cm, 3m) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(s_val.value, 15.0); // 10m + 2m + 3m = 15m
        assert_eq!(s_val.unit, Some("m".to_string()));

        let avg_val = eval_expr(
            &parse_line("average(10m, 200cm, 3m) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(avg_val.value, 5.0); // 15m / 3 = 5m

        let med_val = eval_expr(
            &parse_line("median(10m, 200cm, 6m) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(med_val.value, 6.0); // sorted: 2m, 6m, 10m. Median is 6m

        let sd_val = eval_expr(
            &parse_line("stddev(2, 4, 4, 4, 5, 5, 7, 9) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert!((sd_val.value - 2.1380899).abs() < 1e-6);

        let var_val = eval_expr(
            &parse_line("variance(2, 4, 4, 4, 5, 5, 7, 9) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert!((var_val.value - 4.5714285).abs() < 1e-6);

        // Logic and Comparisons
        let if_val = eval_expr(
            &parse_line("if(eq(5m, 500cm), 10m, 20m) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
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

        let op_and =
            eval_expr(&parse_line("1 == 1 and 2 == 2 =>").unwrap_expr(), &mut ctx).unwrap();
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

        let sum_mixed = eval_expr(
            &parse_line("sum([1, 2], 3, [4, 5]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(sum_mixed.value, 15.0);

        let sum_matrix = eval_expr(
            &parse_line("sum([[1, 2], [3, 4]]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(sum_matrix.value, 10.0);

        let mean_list =
            eval_expr(&parse_line("mean([2, 4, 6]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(mean_list.value, 4.0);

        let min_list = eval_expr(&parse_line("min([3, 1, 5]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(min_list.value, 1.0);

        let max_list = eval_expr(&parse_line("max([3, 1, 5]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(max_list.value, 5.0);

        let min_mixed = eval_expr(
            &parse_line("min([10, 20], 5, [15, 30]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(min_mixed.value, 5.0);

        let max_mixed = eval_expr(
            &parse_line("max([10, 20], 5, [15, 30]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(max_mixed.value, 30.0);

        let count_list = eval_expr(
            &parse_line("count([1, 2, 3, 4, 5]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(count_list.value, 5.0);

        let count_mixed = eval_expr(
            &parse_line("count([1, 2], 3, [4, 5]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(count_mixed.value, 5.0);

        // 3. Vector/matrix utilities
        let length = eval_expr(
            &parse_line("len([10, 20, 30, 40]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(length.value, 4.0);

        let vdot_val = eval_expr(
            &parse_line("vdot([1, 2], [3, 4]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(vdot_val.value, 11.0); // 1*3 + 2*4 = 11

        let vadd_val = eval_expr(
            &parse_line("vadd([1, 2], [3, 4]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(format_quantity(&vadd_val), "[4, 6]");

        let vsub_val = eval_expr(
            &parse_line("vsub([5, 10], [1, 2]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(format_quantity(&vsub_val), "[4, 8]");

        let trans_val =
            eval_expr(&parse_line("transpose([1, 2]) =>").unwrap_expr(), &mut ctx).unwrap();
        assert_eq!(format_quantity(&trans_val), "[[1], [2]]");

        let matmul_val1 = eval_expr(
            &parse_line("matmul([[1, 2], [3, 4]], [[5], [6]]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(format_quantity(&matmul_val1), "[[17], [39]]");

        let matmul_val2 = eval_expr(
            &parse_line("matmul([[1, 2], [3, 4]], [5, 6]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(format_quantity(&matmul_val2), "[17, 39]");

        // Let's test plot/sparkline
        let plot_qty1 = eval_expr(
            &parse_line("plot([1, 3, 2, 5, 4]) =>").unwrap_expr(),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(format_quantity(&plot_qty1), " ▅▃█▆");

        let plot_qty2 =
            eval_expr(&parse_line("plot(10, 10, 10) =>").unwrap_expr(), &mut ctx).unwrap();
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
        assert!(
            output.contains("res => 2.6667 widget"),
            "Actual output: {}",
            output
        );
        assert!(
            output.contains("res_cm => 30 cm"),
            "Actual output: {}",
            output
        );

        // 4. Test complex custom unit: J = 1 kg * m^2 / s^2
        let sheet_complex = r#"
J = 1 kg * m^2 / s^2
res_j = 2 J + 5 kg * m^2 / s^2
res_j =>
"#;
        let (output_complex, _) = crate::math::evaluate_sheet(sheet_complex, &rates);
        assert!(
            output_complex.contains("res_j => 7 J"),
            "Actual output: {}",
            output_complex
        );
    }

    #[test]
    fn test_solve_symbolic_rearrange() {
        let rates = HashMap::new();

        // Other variable unbound -> rearrange to a formula instead of failing.
        let s1 = "sol = solve(x == c + 2, c)\nsol =>\n";
        let out1 = crate::math::evaluate_sheet(s1, &rates).0;
        assert!(out1.contains("sol => x - 2"), "Actual: {}", out1);

        // Multi-variable rearrange (solve for x in y = m*x + b).
        let s2 = "sol = solve(y == m * x + b, x)\nsol =>\n";
        let out2 = crate::math::evaluate_sheet(s2, &rates).0;
        assert!(out2.contains("sol => (y - b) / m"), "Actual: {}", out2);

        // All others bound -> numeric result preserved (no regression).
        let s3 = "x = 5\nsol = solve(x == c + 2, c)\nsol =>\n";
        let out3 = crate::math::evaluate_sheet(s3, &rates).0;
        assert!(out3.contains("sol => 3"), "Actual: {}", out3);
    }

    #[test]
    fn test_solve_newton_raphson() {
        let mut ctx = Context::default();

        // Variable on both sides -> symbolic fails, Newton finds the fixed point.
        let e = parse_line("solve(cos(x) == x, x) =>").unwrap_expr();
        let r = eval_expr(&e, &mut ctx).unwrap();
        assert!(
            (r.value - 0.739085).abs() < 1e-4,
            "cos(x)==x root: {}",
            r.value
        );

        // Variable inside a function, with an explicit initial guess (3-arg form).
        let e = parse_line("solve(sin(x) == 0.5, x, 0.3) =>").unwrap_expr();
        let r = eval_expr(&e, &mut ctx).unwrap();
        assert!(
            (r.value - std::f64::consts::FRAC_PI_6).abs() < 1e-4,
            "sin(x)==0.5 root: {}",
            r.value
        );

        // sqrt is not symbolically differentiable -> exercises the finite-difference path.
        let e = parse_line("solve(sqrt(x) == 3, x, 4) =>").unwrap_expr();
        let r = eval_expr(&e, &mut ctx).unwrap();
        assert!((r.value - 9.0).abs() < 1e-4, "sqrt(x)==3 root: {}", r.value);

        // Existing symbolic/numeric paths still win when applicable (no Newton).
        let e = parse_line("solve(2 * x + 5 == 15, x) =>").unwrap_expr();
        let r = eval_expr(&e, &mut ctx).unwrap();
        assert!((r.value - 5.0).abs() < 1e-9, "linear solve: {}", r.value);

        // No real root -> reports non-convergence rather than a bogus value.
        let e = parse_line("solve(exp(x) == -1, x) =>").unwrap_expr();
        assert!(
            eval_expr(&e, &mut ctx).is_err(),
            "exp(x)==-1 should not converge"
        );
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

    #[test]
    fn test_loops_and_ranges() {
        use crate::math::parser::parse_line;
        let mut ctx = Context::default();

        // 1. Test range(5)
        let expr_r1 = parse_line("range(5) =>").unwrap_expr();
        let res_r1 = eval_expr(&expr_r1, &mut ctx).unwrap();
        let list_elements = res_r1.list.unwrap();
        assert_eq!(list_elements.len(), 5);
        assert_eq!(list_elements[0].value, 0.0);
        assert_eq!(list_elements[4].value, 4.0);

        // 2. Test range(2, 6)
        let expr_r2 = parse_line("range(2, 6) =>").unwrap_expr();
        let res_r2 = eval_expr(&expr_r2, &mut ctx).unwrap();
        let list_elements2 = res_r2.list.unwrap();
        assert_eq!(list_elements2.len(), 4);
        assert_eq!(list_elements2[0].value, 2.0);
        assert_eq!(list_elements2[3].value, 5.0);

        // 3. Test range(1, 10, 2)
        let expr_r3 = parse_line("range(1, 10, 2) =>").unwrap_expr();
        let res_r3 = eval_expr(&expr_r3, &mut ctx).unwrap();
        let list_elements3 = res_r3.list.unwrap();
        assert_eq!(list_elements3.len(), 5);
        assert_eq!(list_elements3[0].value, 1.0);
        assert_eq!(list_elements3[4].value, 9.0);

        // 4. Test for loop: summing values
        let expr_assign = Expr::LocalAssign("sum".to_string(), Box::new(Expr::Number(0.0)));
        eval_expr(&expr_assign, &mut ctx).unwrap();

        let expr_for = parse_line("for x in range(1, 6) { sum = sum + x; sum } =>").unwrap_expr();
        let res_for = eval_expr(&expr_for, &mut ctx).unwrap();
        assert_eq!(res_for.value, 15.0);

        let expr_sum = parse_line("sum =>").unwrap_expr();
        let res_sum = eval_expr(&expr_sum, &mut ctx).unwrap();
        assert_eq!(res_sum.value, 15.0);

        // 5. Test while loop
        let expr_assign_w = Expr::LocalAssign("count".to_string(), Box::new(Expr::Number(0.0)));
        eval_expr(&expr_assign_w, &mut ctx).unwrap();

        let expr_while =
            parse_line("while count < 3 { count = count + 1; count } =>").unwrap_expr();
        let res_while = eval_expr(&expr_while, &mut ctx).unwrap();
        assert_eq!(res_while.value, 3.0);

        let expr_count = parse_line("count =>").unwrap_expr();
        let res_count = eval_expr(&expr_count, &mut ctx).unwrap();
        assert_eq!(res_count.value, 3.0);

        // 6. Test filter
        let expr_filter = parse_line("filter(x > 2, range(5)) =>").unwrap_expr();
        let res_filter = eval_expr(&expr_filter, &mut ctx).unwrap();
        let filter_elements = res_filter.list.unwrap();
        assert_eq!(filter_elements.len(), 2);
        assert_eq!(filter_elements[0].value, 3.0);
        assert_eq!(filter_elements[1].value, 4.0);

        // 7. Test any/all
        let expr_any_t = parse_line("any(x > 3, range(5)) =>").unwrap_expr();
        let res_any_t = eval_expr(&expr_any_t, &mut ctx).unwrap();
        assert_eq!(res_any_t.value, 1.0); // true

        let expr_any_f = parse_line("any(x > 5, range(5)) =>").unwrap_expr();
        let res_any_f = eval_expr(&expr_any_f, &mut ctx).unwrap();
        assert_eq!(res_any_f.value, 0.0); // false

        let expr_all_t = parse_line("all(x >= 0, range(5)) =>").unwrap_expr();
        let res_all_t = eval_expr(&expr_all_t, &mut ctx).unwrap();
        assert_eq!(res_all_t.value, 1.0); // true

        let expr_all_f = parse_line("all(x > 2, range(5)) =>").unwrap_expr();
        let res_all_f = eval_expr(&expr_all_f, &mut ctx).unwrap();
        assert_eq!(res_all_f.value, 0.0); // false

        // 8. Test zip
        let expr_zip = parse_line("zip(range(2), range(10, 12)) =>").unwrap_expr();
        let res_zip = eval_expr(&expr_zip, &mut ctx).unwrap();
        let zip_elements = res_zip.list.unwrap();
        assert_eq!(zip_elements.len(), 2);
        assert_eq!(zip_elements[0].list.as_ref().unwrap()[0].value, 0.0);
        assert_eq!(zip_elements[0].list.as_ref().unwrap()[1].value, 10.0);
        assert_eq!(zip_elements[1].list.as_ref().unwrap()[0].value, 1.0);
        assert_eq!(zip_elements[1].list.as_ref().unwrap()[1].value, 11.0);

        // 9. Test multi-line block contained in {} with semicolon separation
        let expr_multiline = parse_line("{\n  a = 10;\n  b = 20;\n  a + b\n} =>").unwrap_expr();
        let res_multiline = eval_expr(&expr_multiline, &mut ctx).unwrap();
        assert_eq!(res_multiline.value, 30.0);

        // 10. Test error scope isolation for Blocks
        let expr_err_block =
            parse_line("{\n  block_local_err = 42;\n  non_existent_func(1)\n} =>").unwrap_expr();
        let res_err_block = eval_expr(&expr_err_block, &mut ctx);
        assert!(res_err_block.is_err());
        assert!(!ctx.variables.contains_key("block_local_err"));

        // 11. Test loop variable restoration on For loop error
        ctx.variables
            .insert("loop_var".to_string(), Quantity::scalar(999.0, None));
        let expr_err_loop =
            parse_line("for loop_var in range(3) {\n  non_existent_func(1)\n} =>").unwrap_expr();
        let res_err_loop = eval_expr(&expr_err_loop, &mut ctx);
        assert!(res_err_loop.is_err());
        assert_eq!(ctx.variables.get("loop_var").unwrap().value, 999.0);
    }
}
