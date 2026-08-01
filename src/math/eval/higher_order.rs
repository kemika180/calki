//! Higher-order and symbolic forms that consume *unevaluated* argument
//! expressions: `solve`, `diff`/`der`, `map`, `filter`, `any`, `all`, `zip`,
//! `reduce`. Each takes `&[Expr]` and drives `eval_expr` itself (binding a loop
//! variable, differentiating, etc.), so they run before the eager argument
//! evaluation that the plain builtins rely on.

use crate::math::eval::{
    Context, differentiate, eval_expr, expr_to_string, find_all_variables_in_expr,
    find_variable_in_expr, simplify, solve_equation, solve_symbolic,
};
use crate::math::parser::{Expr, Op, Quantity};

pub(in crate::math::eval) fn solve(args: &[Expr], ctx: &mut Context) -> Result<Quantity, String> {
    if args.len() != 2 && args.len() != 3 {
        return Err("Built-in function 'solve' expects 2 or 3 arguments".to_string());
    }
    let solve_expr = &args[0];
    let var_expr = &args[1];
    let var_name = match var_expr {
        Expr::Variable(v) => v.clone(),
        _ => {
            return Err("Second argument to 'solve' must be a variable name".to_string());
        }
    };
    // Optional third argument: initial guess for the numeric solver.
    let guess = match args.get(2) {
        Some(g) => Some(eval_expr(g, ctx)?.value),
        None => None,
    };

    // When another variable in the equation is still unbound, the numeric solver
    // cannot resolve to a value — instead rearrange symbolically and show the
    // formula (e.g. `solve(x == c + 2, c)` => `x - 2`).
    let has_unbound_other = find_all_variables_in_expr(solve_expr)
        .iter()
        .any(|v| v != &var_name && !ctx.variables.contains_key(v));
    if has_unbound_other {
        return solve_symbolic(solve_expr, &var_name);
    }

    // Fully determined: try symbolic algebraic inversion first, then fall back to
    // numeric Newton-Raphson for equations it can't invert (variable inside a
    // function, or appearing more than once).
    match solve_equation(solve_expr, &var_name, ctx) {
        Ok(q) => Ok(q),
        Err(sym_err) => {
            let contains_var = find_all_variables_in_expr(solve_expr)
                .iter()
                .any(|v| v == &var_name);
            if !contains_var {
                return Err(sym_err);
            }
            solve_numeric(solve_expr, &var_name, guess.unwrap_or(1.0), ctx)
        }
    }
}

/// Numeric root-finder (Newton-Raphson) used when symbolic inversion fails but
/// the equation is fully determined. Operates on the residual `f(x) = left -
/// right` (or `f(x) = expr` for a bare expression), using the symbolic
/// derivative when `differentiate` supports it and a central finite difference
/// otherwise.
fn solve_numeric(
    expr: &Expr,
    var_name: &str,
    guess: f64,
    ctx: &mut Context,
) -> Result<Quantity, String> {
    let residual = match expr {
        Expr::BinaryOp(Op::Eq, left, right) => Expr::BinaryOp(Op::Sub, left.clone(), right.clone()),
        _ => expr.clone(),
    };
    let deriv = differentiate(&residual, var_name)
        .ok()
        .map(|d| simplify(&d));

    const MAX_ITER: usize = 100;
    const TOL: f64 = 1e-10;
    const MIN_SLOPE: f64 = 1e-14;

    let mut x = guess;
    for _ in 0..MAX_ITER {
        let fx = sample(&residual, var_name, x, ctx)?;
        if fx.abs() < TOL {
            return Ok(Quantity::scalar(x, None));
        }
        let dfx = match &deriv {
            Some(d) => sample(d, var_name, x, ctx)?,
            None => {
                let h = 1e-6 * x.abs().max(1.0);
                let f_hi = sample(&residual, var_name, x + h, ctx)?;
                let f_lo = sample(&residual, var_name, x - h, ctx)?;
                (f_hi - f_lo) / (2.0 * h)
            }
        };
        if dfx.abs() < MIN_SLOPE {
            return Err(format!(
                "solve: derivative near zero at x = {}; try a different initial guess",
                x
            ));
        }
        let next = x - fx / dfx;
        if !next.is_finite() {
            return Err("solve: numeric iteration diverged".to_string());
        }
        if (next - x).abs() < TOL {
            return Ok(Quantity::scalar(next, None));
        }
        x = next;
    }
    Err(format!(
        "solve: did not converge within {} iterations; try providing an initial guess",
        MAX_ITER
    ))
}

/// Bind `var_name = x`, evaluate `expr` to a scalar, then restore the prior
/// binding. The insert/eval/restore pattern used throughout this module.
fn sample(expr: &Expr, var_name: &str, x: f64, ctx: &mut Context) -> Result<f64, String> {
    let prev = ctx
        .variables
        .insert(var_name.to_string(), Quantity::scalar(x, None));
    let result = eval_expr(expr, ctx);
    match prev {
        Some(p) => {
            ctx.variables.insert(var_name.to_string(), p);
        }
        None => {
            ctx.variables.remove(var_name);
        }
    }
    result.map(|q| q.value)
}

pub(in crate::math::eval) fn diff(
    name: &str,
    args: &[Expr],
    ctx: &mut Context,
) -> Result<Quantity, String> {
    if args.len() != 2 {
        return Err(format!("Built-in function '{}' expects 2 arguments", name));
    }
    let diff_expr = &args[0];
    let var_expr = &args[1];
    let var_name = match var_expr {
        Expr::Variable(v) => v.clone(),
        _ => {
            return Err(format!(
                "Second argument to '{}' must be a variable name",
                name
            ));
        }
    };
    let derived_ast = differentiate(diff_expr, &var_name)?;
    let simplified_ast = simplify(&derived_ast);
    if ctx.variables.contains_key(&var_name) {
        eval_expr(&simplified_ast, ctx)
    } else {
        let formula_str = expr_to_string(&simplified_ast);
        Ok(Quantity {
            display: None,
            is_bool: false,
            list: None,
            value: 1.0,
            unit: Some(format!("formula:{}", formula_str)),
        })
    }
}

pub(in crate::math::eval) fn map(args: &[Expr], ctx: &mut Context) -> Result<Quantity, String> {
    if args.len() != 2 {
        return Err("Built-in function 'map' expects 2 arguments".to_string());
    }
    let map_expr = &args[0];
    let list_qty = eval_expr(&args[1], ctx)?;
    let elements = list_qty
        .list
        .as_ref()
        .ok_or("Second argument to 'map' must be a list")?;

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
    Ok(Quantity::list(mapped_elements))
}

pub(in crate::math::eval) fn filter(args: &[Expr], ctx: &mut Context) -> Result<Quantity, String> {
    if args.len() != 2 {
        return Err("Built-in function 'filter' expects 2 arguments".to_string());
    }
    let filter_expr = &args[0];
    let list_qty = eval_expr(&args[1], ctx)?;
    let elements = list_qty
        .list
        .as_ref()
        .ok_or("Second argument to 'filter' must be a list")?;

    let var_name = find_variable_in_expr(filter_expr).unwrap_or_else(|| "x".to_string());

    let mut filtered_elements = Vec::new();
    for el in elements {
        let prev_val = ctx.variables.insert(var_name.clone(), el.clone());
        let res = eval_expr(filter_expr, ctx);
        if let Some(pv) = prev_val {
            ctx.variables.insert(var_name.clone(), pv);
        } else {
            ctx.variables.remove(&var_name);
        }
        let res_qty = res?;
        let keep = if res_qty.is_bool {
            res_qty.value != 0.0
        } else {
            return Err("Filter condition expression must evaluate to a boolean".to_string());
        };
        if keep {
            filtered_elements.push(el.clone());
        }
    }
    Ok(Quantity::list(filtered_elements))
}

pub(in crate::math::eval) fn any(args: &[Expr], ctx: &mut Context) -> Result<Quantity, String> {
    if args.len() != 2 {
        return Err("Built-in function 'any' expects 2 arguments".to_string());
    }
    let any_expr = &args[0];
    let list_qty = eval_expr(&args[1], ctx)?;
    let elements = list_qty
        .list
        .as_ref()
        .ok_or("Second argument to 'any' must be a list")?;

    let var_name = find_variable_in_expr(any_expr).unwrap_or_else(|| "x".to_string());

    for el in elements {
        let prev_val = ctx.variables.insert(var_name.clone(), el.clone());
        let res = eval_expr(any_expr, ctx);
        if let Some(pv) = prev_val {
            ctx.variables.insert(var_name.clone(), pv);
        } else {
            ctx.variables.remove(&var_name);
        }
        let res_qty = res?;
        let is_true = if res_qty.is_bool {
            res_qty.value != 0.0
        } else {
            return Err("Condition expression in 'any' must evaluate to a boolean".to_string());
        };
        if is_true {
            return Ok(Quantity::boolean(true));
        }
    }
    Ok(Quantity::boolean(false))
}

pub(in crate::math::eval) fn all(args: &[Expr], ctx: &mut Context) -> Result<Quantity, String> {
    if args.len() != 2 {
        return Err("Built-in function 'all' expects 2 arguments".to_string());
    }
    let all_expr = &args[0];
    let list_qty = eval_expr(&args[1], ctx)?;
    let elements = list_qty
        .list
        .as_ref()
        .ok_or("Second argument to 'all' must be a list")?;

    let var_name = find_variable_in_expr(all_expr).unwrap_or_else(|| "x".to_string());

    for el in elements {
        let prev_val = ctx.variables.insert(var_name.clone(), el.clone());
        let res = eval_expr(all_expr, ctx);
        if let Some(pv) = prev_val {
            ctx.variables.insert(var_name.clone(), pv);
        } else {
            ctx.variables.remove(&var_name);
        }
        let res_qty = res?;
        let is_true = if res_qty.is_bool {
            res_qty.value != 0.0
        } else {
            return Err("Condition expression in 'all' must evaluate to a boolean".to_string());
        };
        if !is_true {
            return Ok(Quantity::boolean(false));
        }
    }
    Ok(Quantity::boolean(true))
}

pub(in crate::math::eval) fn zip(args: &[Expr], ctx: &mut Context) -> Result<Quantity, String> {
    if args.len() != 2 {
        return Err("Built-in function 'zip' expects 2 arguments".to_string());
    }
    let list1_qty = eval_expr(&args[0], ctx)?;
    let list2_qty = eval_expr(&args[1], ctx)?;
    let el1 = list1_qty
        .list
        .as_ref()
        .ok_or("First argument to 'zip' must be a list")?;
    let el2 = list2_qty
        .list
        .as_ref()
        .ok_or("Second argument to 'zip' must be a list")?;

    let min_len = std::cmp::min(el1.len(), el2.len());
    let mut zipped = Vec::new();
    for i in 0..min_len {
        let pair = vec![el1[i].clone(), el2[i].clone()];
        zipped.push(Quantity::list(pair));
    }
    Ok(Quantity::list(zipped))
}

pub(in crate::math::eval) fn reduce(args: &[Expr], ctx: &mut Context) -> Result<Quantity, String> {
    if args.len() != 2 {
        return Err("Built-in function 'reduce' expects 2 arguments".to_string());
    }
    let reduce_expr = &args[0];
    let list_qty = eval_expr(&args[1], ctx)?;
    let elements = list_qty
        .list
        .as_ref()
        .ok_or("Second argument to 'reduce' must be a list")?;
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
    Ok(acc)
}
