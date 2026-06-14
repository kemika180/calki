use crate::math::parser::{Expr, Op, Quantity};
use crate::math::units::{are_compatible, combine_units_with_multiplier, convert_quantity};
use std::collections::HashMap;

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
                (Some(u1), Some(u2)) => {
                    if are_compatible(u1, u2) {
                        if let Ok(q2_conv) = convert_quantity(q2.value, u2, u1, exchange_rates) {
                            (q1.value - q2_conv).abs() < 1e-9
                        } else {
                            false
                        }
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

        Self {
            variables,
            functions: HashMap::new(),
            exchange_rates: HashMap::new(),
        }
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
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.sin(),
                        unit: None,
                    })
                }
                "cos" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.cos(),
                        unit: None,
                    })
                }
                "tan" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.tan(),
                        unit: None,
                    })
                }
                "asin" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    let val = arg_vals[0].value;
                    if val < -1.0 || val > 1.0 {
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
                    if val < -1.0 || val > 1.0 {
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
                "exp" => {
                    check_built_in_args(name, &arg_vals, 1)?;
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
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.log10(),
                        unit: None,
                    })
                }
                "ln" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.ln(),
                        unit: None,
                    })
                }
                "sqrt" => {
                    check_built_in_args(name, &arg_vals, 1)?;
                    if arg_vals[0].value < 0.0 {
                        return Err("Cannot compute square root of a negative number".to_string());
                    }
                    Ok(Quantity { is_bool: false, list: None,
                        value: arg_vals[0].value.sqrt(),
                        unit: arg_vals[0].unit.clone(), // sqrt(9m^2) would ideally be 3m, but simply forward unit
                    })
                }
                "abs" => {
                    check_built_in_args(name, &arg_vals, 1)?;
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
                            return Err("Cannot compare a quantity with a dimensionless value in mod()".to_string());
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
                    if right_qty.unit.is_some() {
                        return Err("Exponent power must be a dimensionless scalar".to_string());
                    }
                    let value = left_qty.value.powf(right_qty.value);
                    Ok(Quantity { is_bool: false, list: None,
                        value,
                        unit: left_qty.unit, // Simplified: assume unit stays unchanged (e.g. m^2 area needs derived unit support, but for now we forward unit)
                    })
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
                            return Err("Cannot compare a quantity with a dimensionless value in modulo operator".to_string());
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
    if let Some(ref u) = qty.unit {
        if u.starts_with("sparkline:") {
            return u["sparkline:".len()..].to_string();
        }
    }

    if qty.is_bool {
        return if qty.value != 0.0 { "True".to_string() } else { "False".to_string() };
    }

    if let Some(ref elements) = qty.list {
        let formatted: Vec<String> = elements.iter().map(|el| format_quantity(el)).collect();
        return format!("[{}]", formatted.join(", "));
    }

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
            if u.starts_with('$') {
                let suffix = &u[1..];
                format!("${}{}", rounded, suffix)
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
}
