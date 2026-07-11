//! Vector and matrix builtins: `vdot`, `vadd`, `vsub`, `transpose`, `matmul`, `len`.
//! `vadd`/`vsub`/`matmul` defer to the shared quantity arithmetic; `vdot` and the
//! reconciliation paths need the context's exchange rates.

use crate::math::eval::{Context, check_built_in_args, matmul_impl, quantity_add, quantity_sub};
use crate::math::parser::Quantity;
use crate::math::units::{are_compatible, combine_units_with_multiplier, convert_quantity};

pub(in crate::math::eval) fn vdot(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let q1 = &args[0];
    let q2 = &args[1];
    let el1 = q1
        .list
        .as_ref()
        .ok_or("vdot expects first argument to be a list/vector")?;
    let el2 = q2
        .list
        .as_ref()
        .ok_or("vdot expects second argument to be a list/vector")?;
    if el1.len() != el2.len() {
        return Err(format!(
            "vdot: vector lengths must match ({} and {})",
            el1.len(),
            el2.len()
        ));
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
                        return Err(format!(
                            "Incompatible units in vdot(): '{}' and '{}'",
                            u1, u2
                        ));
                    }
                    let converted = convert_quantity(prod_val, u2, u1, &ctx.exchange_rates)?;
                    total_val += converted;
                }
                (None, None) => {
                    total_val += prod_val;
                }
                _ => {
                    return Err(
                        "Cannot mix dimensional and dimensionless values in vdot() sum".to_string(),
                    );
                }
            }
        }
    }
    Ok(Quantity::scalar(total_val, target_unit))
}

pub(in crate::math::eval) fn vadd(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    quantity_add(&args[0], &args[1], ctx)
}

pub(in crate::math::eval) fn vsub(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    quantity_sub(&args[0], &args[1], ctx)
}

pub(in crate::math::eval) fn transpose(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    let qty = &args[0];
    let elements = qty
        .list
        .as_ref()
        .ok_or("transpose expects a list or matrix")?;
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
            for row in elements {
                let cell = &row.list.as_ref().unwrap()[col_idx];
                new_row.push(cell.clone());
            }
            transposed_rows.push(Quantity::list(new_row));
        }
        Ok(Quantity::list(transposed_rows))
    } else {
        Err("Invalid matrix for transpose: mix of lists and scalars".to_string())
    }
}

pub(in crate::math::eval) fn matmul(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    matmul_impl(&args[0], &args[1], ctx)
}

pub(in crate::math::eval) fn len(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    let list_qty = &args[0];
    if let Some(ref elements) = list_qty.list {
        Ok(Quantity::scalar(elements.len() as f64, None))
    } else {
        Err("Function 'len' expects a list/vector argument".to_string())
    }
}
