//! Control-flow constructs: `Block`, `For`, `While`, `LocalAssign`, `IfElse`,
//! `Switch`. Each drives `eval_expr` over sub-expressions and manages loop/block
//! variable scoping so locals don't leak into the surrounding context.

use crate::math::eval::{Context, eval_eq_logic, eval_expr, expr_to_string};
use crate::math::parser::{Expr, Quantity};

pub(in crate::math::eval) fn eval_block(
    exprs: &[Expr],
    ctx: &mut Context,
) -> Result<Quantity, String> {
    let original_keys: std::collections::HashSet<String> = ctx.variables.keys().cloned().collect();
    let mut last_val = Quantity::scalar(0.0, None);
    let mut result = Ok(());
    for expr in exprs {
        match eval_expr(expr, ctx) {
            Ok(val) => last_val = val,
            Err(e) => {
                result = Err(e);
                break;
            }
        }
    }
    ctx.variables.retain(|k, _| original_keys.contains(k));
    result.map(|_| last_val)
}

pub(in crate::math::eval) fn eval_for(
    var: &str,
    iterable: &Expr,
    body: &Expr,
    ctx: &mut Context,
) -> Result<Quantity, String> {
    let iterable_val = eval_expr(iterable, ctx)?;
    let elements = match iterable_val.list {
        Some(el) => el,
        None => {
            return Err(format!(
                "Cannot iterate over non-list value: {}",
                expr_to_string(iterable)
            ));
        }
    };

    let max_iterations = 2000;
    if elements.len() > max_iterations {
        return Err(format!(
            "Loop exceeds maximum iteration limit of {}",
            max_iterations
        ));
    }

    let mut last_val = Quantity::scalar(0.0, None);
    let original_loop_var = ctx.variables.get(var).cloned();

    let mut loop_err = None;
    for element in elements {
        ctx.variables.insert(var.to_string(), element);
        match eval_expr(body, ctx) {
            Ok(val) => last_val = val,
            Err(e) => {
                loop_err = Some(e);
                break;
            }
        }
    }

    if let Some(pv) = original_loop_var {
        ctx.variables.insert(var.to_string(), pv);
    } else {
        ctx.variables.remove(var);
    }

    if let Some(e) = loop_err {
        Err(e)
    } else {
        Ok(last_val)
    }
}

pub(in crate::math::eval) fn eval_while(
    cond: &Expr,
    body: &Expr,
    ctx: &mut Context,
) -> Result<Quantity, String> {
    let mut last_val = Quantity::scalar(0.0, None);
    let mut iterations = 0;
    let max_iterations = 2000;

    loop {
        let cond_val = eval_expr(cond, ctx)?;
        let is_true = if cond_val.is_bool {
            cond_val.value != 0.0
        } else {
            return Err("Condition in while loop must be a boolean".to_string());
        };

        if !is_true {
            break;
        }

        iterations += 1;
        if iterations > max_iterations {
            return Err(format!(
                "While loop exceeded maximum iteration limit of {}",
                max_iterations
            ));
        }

        last_val = eval_expr(body, ctx)?;
    }

    Ok(last_val)
}

pub(in crate::math::eval) fn eval_local_assign(
    name: &str,
    val_expr: &Expr,
    ctx: &mut Context,
) -> Result<Quantity, String> {
    let qty = eval_expr(val_expr, ctx)?;
    ctx.variables.insert(name.to_string(), qty.clone());
    Ok(qty)
}

pub(in crate::math::eval) fn eval_if_else(
    cond: &Expr,
    then_expr: &Expr,
    else_expr: &Expr,
    ctx: &mut Context,
) -> Result<Quantity, String> {
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

pub(in crate::math::eval) fn eval_switch(
    val: &Expr,
    cases: &[(Expr, Expr)],
    default_case: Option<&Expr>,
    ctx: &mut Context,
) -> Result<Quantity, String> {
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
            return Err(
                "No case matched in switch statement and no default case provided".to_string(),
            );
        }
    }
    Ok(result)
}
