//! Binary-operator evaluation over two already-evaluated operands.
//!
//! `eval_binary_op` is the `match op { … }` dispatch lifted out of `eval_expr`'s
//! `BinaryOp` arm; the operand evaluation and the `x - 15%` percentage special
//! case remain in the caller. Operands are taken by value because the bitwise
//! and shift branches move their units out.

use crate::math::eval::complex::{is_complex, make_complex_qty, to_complex_parts};
use crate::math::eval::{
    Context, eval_and_logic, eval_eq_logic, eval_gt_logic, eval_gte_logic, eval_lt_logic,
    eval_lte_logic, eval_ne_logic, eval_or_logic, matmul_impl, scale_list,
};
use crate::math::parser::{Op, Quantity};
use crate::math::units::{are_compatible, combine_units_with_multiplier, convert_quantity};

pub(in crate::math::eval) fn eval_binary_op(
    op: &Op,
    left_qty: Quantity,
    right_qty: Quantity,
    ctx: &Context,
) -> Result<Quantity, String> {
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
                    Ok(Quantity { display: None,
                        is_bool: false,
                        list: None,
                        value,
                        unit: None,
                    })
                }
                (Some(u1), Some(u2)) => {
                    if !are_compatible(u1, u2) {
                        return Err(format!(
                            "Incompatible units: cannot add/subtract '{}' and '{}'",
                            u1, u2
                        ));
                    }
                    // Convert right unit to left unit
                    let right_converted =
                        convert_quantity(right_qty.value, u2, u1, &ctx.exchange_rates)?;
                    let value = match op {
                        Op::Add => left_qty.value + right_converted,
                        Op::Sub => left_qty.value - right_converted,
                        _ => unreachable!(),
                    };
                    Ok(Quantity { display: None,
                        is_bool: false,
                        list: None,
                        value,
                        unit: Some(u1.clone()),
                    })
                }
                _ => Err(
                    "Cannot mix dimensionless values with dimensional units in addition/subtraction"
                        .to_string(),
                ),
            }
        }
        Op::Mul => {
            if is_complex(&left_qty) || is_complex(&right_qty) {
                let (a, b) = to_complex_parts(&left_qty);
                let (c, d) = to_complex_parts(&right_qty);
                return Ok(make_complex_qty(a * c - b * d, a * d + b * c));
            }
            // Matrix / vector multiplication: list*list => matmul, scalar*list => scale.
            match (left_qty.list.is_some(), right_qty.list.is_some()) {
                (true, true) => return matmul_impl(&left_qty, &right_qty, ctx),
                (true, false) => return scale_list(&left_qty, &right_qty, ctx),
                (false, true) => return scale_list(&right_qty, &left_qty, ctx),
                (false, false) => {}
            }
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
        Op::Div => {
            if is_complex(&left_qty) || is_complex(&right_qty) {
                let (a, b) = to_complex_parts(&left_qty);
                let (c, d) = to_complex_parts(&right_qty);
                let denom = c * c + d * d;
                if denom == 0.0 {
                    return Err("Division by zero in complex division".to_string());
                }
                return Ok(make_complex_qty(
                    (a * c + b * d) / denom,
                    (b * c - a * d) / denom,
                ));
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
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value,
                unit,
            })
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
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value,
                unit,
            })
        }
        Op::Mod => {
            let u1 = &left_qty.unit;
            let u2 = &right_qty.unit;
            match (u1, u2) {
                (Some(unit1), Some(unit2)) => {
                    if !are_compatible(unit1, unit2) {
                        return Err(format!(
                            "Incompatible units in modulo operator: '{}' and '{}'",
                            unit1, unit2
                        ));
                    }
                    let right_converted =
                        convert_quantity(right_qty.value, unit2, unit1, &ctx.exchange_rates)?;
                    let rem = left_qty.value % right_converted;
                    Ok(Quantity {
                        display: None,
                        is_bool: false,
                        list: None,
                        value: rem,
                        unit: Some(unit1.clone()),
                    })
                }
                (None, None) => Ok(Quantity {
                    display: None,
                    is_bool: false,
                    list: None,
                    value: left_qty.value % right_qty.value,
                    unit: None,
                }),
                _ => Err(
                    "Cannot compare a quantity with a dimensionless value in modulo operator"
                        .to_string(),
                ),
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
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: val as f64,
                unit,
            })
        }
        Op::BitOr => {
            let val = (left_qty.value as i64) | (right_qty.value as i64);
            let unit = left_qty.unit.or(right_qty.unit);
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: val as f64,
                unit,
            })
        }
        Op::LShift => {
            let val = (left_qty.value as i64) << (right_qty.value as i64);
            let unit = left_qty.unit;
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: val as f64,
                unit,
            })
        }
        Op::RShift => {
            let val = (left_qty.value as i64) >> (right_qty.value as i64);
            let unit = left_qty.unit;
            Ok(Quantity {
                display: None,
                is_bool: false,
                list: None,
                value: val as f64,
                unit,
            })
        }
    }
}
