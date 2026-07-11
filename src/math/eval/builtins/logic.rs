//! Logical and comparison builtins. Comparisons reconcile units through the
//! shared `eval_*_logic` helpers, which need the context's exchange rates.

use crate::math::eval::{
    Context, check_built_in_args, eval_eq_logic, eval_gt_logic, eval_gte_logic, eval_lt_logic,
    eval_lte_logic, eval_ne_logic,
};
use crate::math::parser::Quantity;

pub(in crate::math::eval) fn if_(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 3)?;
    let cond = args[0].value;
    if cond != 0.0 {
        Ok(args[1].clone())
    } else {
        Ok(args[2].clone())
    }
}

pub(in crate::math::eval) fn and(args: &[Quantity]) -> Result<Quantity, String> {
    if args.is_empty() {
        return Err("Function 'and' expects at least 1 argument".to_string());
    }
    let all_true = args.iter().all(|q| q.value != 0.0);
    Ok(Quantity::boolean(all_true))
}

pub(in crate::math::eval) fn or(args: &[Quantity]) -> Result<Quantity, String> {
    if args.is_empty() {
        return Err("Function 'or' expects at least 1 argument".to_string());
    }
    let any_true = args.iter().any(|q| q.value != 0.0);
    Ok(Quantity::boolean(any_true))
}

pub(in crate::math::eval) fn not(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    if args[0].list.is_some() {
        return Err("Logical NOT cannot be applied to a list".to_string());
    }
    Ok(Quantity::boolean(args[0].value == 0.0))
}

pub(in crate::math::eval) fn eq(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let res = eval_eq_logic(&args[0], &args[1], &ctx.exchange_rates);
    Ok(Quantity::boolean(res))
}

pub(in crate::math::eval) fn ne(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let res = eval_ne_logic(&args[0], &args[1], &ctx.exchange_rates);
    Ok(Quantity::boolean(res))
}

pub(in crate::math::eval) fn lt(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let res = eval_lt_logic(&args[0], &args[1], &ctx.exchange_rates)?;
    Ok(Quantity::boolean(res))
}

pub(in crate::math::eval) fn lte(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let res = eval_lte_logic(&args[0], &args[1], &ctx.exchange_rates)?;
    Ok(Quantity::boolean(res))
}

pub(in crate::math::eval) fn gt(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let res = eval_gt_logic(&args[0], &args[1], &ctx.exchange_rates)?;
    Ok(Quantity::boolean(res))
}

pub(in crate::math::eval) fn gte(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let res = eval_gte_logic(&args[0], &args[1], &ctx.exchange_rates)?;
    Ok(Quantity::boolean(res))
}
