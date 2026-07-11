//! Statistical aggregation builtins over flattened argument lists.
//!
//! `sum`/`mean`/`median`/`stddev`/`variance`/`min`/`max` all share the same
//! "flatten, then reconcile every element to the first element's unit" prelude;
//! [`reconcile`] centralizes it. Error strings are preserved exactly — `min`/`max`
//! historically phrase the dimensional-mismatch case as "Cannot compare …" while
//! the others say "Cannot mix …", so that distinction is threaded through `compare`.

use crate::math::eval::{Context, flatten_quantity};
use crate::math::parser::Quantity;
use crate::math::units::{are_compatible, combine_units_with_multiplier, convert_quantity};

/// Flatten `args`, then convert every element to the first element's unit.
/// Returns the reconciled scalar values and the shared target unit.
///
/// `display` is the function name used in error messages; `compare` selects the
/// dimensional-mismatch wording (`true` → "Cannot compare …", used by min/max).
fn reconcile(
    args: &[Quantity],
    display: &str,
    ctx: &Context,
    compare: bool,
) -> Result<(Vec<f64>, Option<String>), String> {
    if args.is_empty() {
        return Err(format!(
            "Function '{}' expects at least 1 argument",
            display
        ));
    }
    let mut flat_args = Vec::new();
    for arg in args {
        flatten_quantity(arg, &mut flat_args);
    }
    if flat_args.is_empty() {
        return Err(format!(
            "Function '{}' expects at least 1 argument or non-empty list",
            display
        ));
    }
    let target_unit = flat_args[0].unit.clone();
    let mut vals = Vec::with_capacity(flat_args.len());
    vals.push(flat_args[0].value);
    for q in &flat_args[1..] {
        match (&target_unit, &q.unit) {
            (Some(u1), Some(u2)) => {
                if !are_compatible(u1, u2) {
                    return Err(format!(
                        "Incompatible units in {}(): '{}' and '{}'",
                        display, u1, u2
                    ));
                }
                vals.push(convert_quantity(q.value, u2, u1, &ctx.exchange_rates)?);
            }
            (None, None) => {
                vals.push(q.value);
            }
            _ => {
                return Err(if compare {
                    format!(
                        "Cannot compare a quantity with a dimensionless value in {}()",
                        display
                    )
                } else {
                    format!(
                        "Cannot mix dimensional and dimensionless values in {}()",
                        display
                    )
                });
            }
        }
    }
    Ok((vals, target_unit))
}

pub(in crate::math::eval) fn sum(args: &[Quantity], ctx: &Context) -> Result<Quantity, String> {
    let (vals, target_unit) = reconcile(args, "sum", ctx, false)?;
    let total: f64 = vals.iter().sum();
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: total,
        unit: target_unit,
    })
}

pub(in crate::math::eval) fn prod(args: &[Quantity], ctx: &Context) -> Result<Quantity, String> {
    if args.is_empty() {
        return Err("Function 'prod' expects at least 1 argument".to_string());
    }
    let mut flat_args = Vec::new();
    for arg in args {
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
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: total_val,
        unit: current_unit,
    })
}

pub(in crate::math::eval) fn mean(args: &[Quantity], ctx: &Context) -> Result<Quantity, String> {
    let (vals, target_unit) = reconcile(args, "mean", ctx, false)?;
    let total: f64 = vals.iter().sum();
    let mean_val = total / (vals.len() as f64);
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: mean_val,
        unit: target_unit,
    })
}

pub(in crate::math::eval) fn median(args: &[Quantity], ctx: &Context) -> Result<Quantity, String> {
    let (mut vals, target_unit) = reconcile(args, "median", ctx, false)?;
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let len = vals.len();
    let median_val = if len % 2 == 0 {
        (vals[len / 2 - 1] + vals[len / 2]) / 2.0
    } else {
        vals[len / 2]
    };
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: median_val,
        unit: target_unit,
    })
}

pub(in crate::math::eval) fn stddev(args: &[Quantity], ctx: &Context) -> Result<Quantity, String> {
    let (vals, target_unit) = reconcile(args, "stddev", ctx, false)?;
    let len = vals.len();
    if len == 1 {
        return Ok(Quantity {
            is_bool: false,
            list: None,
            value: 0.0,
            unit: target_unit,
        });
    }
    let sum: f64 = vals.iter().sum();
    let mean = sum / (len as f64);
    let variance_sum: f64 = vals
        .iter()
        .map(|&x| {
            let diff = x - mean;
            diff * diff
        })
        .sum();
    let stddev_val = (variance_sum / ((len - 1) as f64)).sqrt();
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: stddev_val,
        unit: target_unit,
    })
}

pub(in crate::math::eval) fn variance(
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    let (vals, target_unit) = reconcile(args, "variance", ctx, false)?;
    let len = vals.len();
    if len == 1 {
        return Ok(Quantity {
            is_bool: false,
            list: None,
            value: 0.0,
            unit: target_unit,
        });
    }
    let sum: f64 = vals.iter().sum();
    let mean = sum / (len as f64);
    let variance_sum: f64 = vals
        .iter()
        .map(|&x| {
            let diff = x - mean;
            diff * diff
        })
        .sum();
    let variance_val = variance_sum / ((len - 1) as f64);
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: variance_val,
        unit: target_unit,
    })
}

pub(in crate::math::eval) fn count(args: &[Quantity]) -> Result<Quantity, String> {
    let mut flat_args = Vec::new();
    for arg in args {
        flatten_quantity(arg, &mut flat_args);
    }
    Ok(Quantity::scalar(flat_args.len() as f64, None))
}

pub(in crate::math::eval) fn min(args: &[Quantity], ctx: &Context) -> Result<Quantity, String> {
    let (vals, target_unit) = reconcile(args, "min", ctx, true)?;
    let min_val = vals[1..].iter().copied().fold(vals[0], f64::min);
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: min_val,
        unit: target_unit,
    })
}

pub(in crate::math::eval) fn max(args: &[Quantity], ctx: &Context) -> Result<Quantity, String> {
    let (vals, target_unit) = reconcile(args, "max", ctx, true)?;
    let max_val = vals[1..].iter().copied().fold(vals[0], f64::max);
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: max_val,
        unit: target_unit,
    })
}
