//! Vector and matrix builtins: `vdot`, `vadd`, `vsub`, `transpose`, `matmul`, `det`, `inv`,
//! `linsolve`, `len`.
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

/// Extract a square, dimensionless `n x n` matrix of raw values from a Quantity,
/// validating shape and rejecting units. Shared by `det` and `inv`.
fn to_square_matrix(qty: &Quantity, name: &str) -> Result<Vec<Vec<f64>>, String> {
    let rows = qty
        .list
        .as_ref()
        .ok_or_else(|| format!("{}() expects a matrix", name))?;
    if rows.is_empty() {
        return Err(format!("{}() expects a non-empty matrix", name));
    }
    let n = rows.len();
    let mut mat = Vec::with_capacity(n);
    for row in rows {
        let cells = row
            .list
            .as_ref()
            .ok_or_else(|| format!("{}() expects a 2D matrix", name))?;
        if cells.len() != n {
            return Err(format!(
                "{}() expects a square matrix (found a {}x{} shape)",
                name,
                n,
                cells.len()
            ));
        }
        let mut r = Vec::with_capacity(n);
        for cell in cells {
            if cell.list.is_some() {
                return Err(format!("{}() expects a 2D matrix of numbers", name));
            }
            if cell.unit.is_some() {
                return Err(format!("{}() requires a dimensionless matrix", name));
            }
            r.push(cell.value);
        }
        mat.push(r);
    }
    Ok(mat)
}

/// Determinant of a square matrix via Gaussian elimination with partial pivoting.
fn determinant(mut m: Vec<Vec<f64>>) -> f64 {
    let n = m.len();
    let mut det = 1.0;
    for i in 0..n {
        let mut pivot = i;
        for r in (i + 1)..n {
            if m[r][i].abs() > m[pivot][i].abs() {
                pivot = r;
            }
        }
        if m[pivot][i] == 0.0 {
            return 0.0;
        }
        if pivot != i {
            m.swap(i, pivot);
            det = -det;
        }
        // The pivot row is not modified while eliminating rows below it.
        let pivot_row = m[i].clone();
        det *= pivot_row[i];
        for row in m.iter_mut().skip(i + 1) {
            let factor = row[i] / pivot_row[i];
            for (c, &pv) in pivot_row.iter().enumerate().skip(i) {
                row[c] -= factor * pv;
            }
        }
    }
    det
}

pub(in crate::math::eval) fn det(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    let m = to_square_matrix(&args[0], "det")?;
    Ok(Quantity::scalar(determinant(m), None))
}

/// Inverse of a square matrix via Gauss-Jordan elimination on `[A | I]`.
fn invert(m: &[Vec<f64>]) -> Result<Vec<Vec<f64>>, String> {
    let n = m.len();
    // Build the augmented matrix [A | I].
    let mut aug: Vec<Vec<f64>> = m
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let mut r = row.clone();
            r.extend((0..n).map(|j| if i == j { 1.0 } else { 0.0 }));
            r
        })
        .collect();
    for i in 0..n {
        let mut pivot = i;
        for r in (i + 1)..n {
            if aug[r][i].abs() > aug[pivot][i].abs() {
                pivot = r;
            }
        }
        if aug[pivot][i].abs() < 1e-12 {
            return Err("inv() matrix is singular (not invertible)".to_string());
        }
        aug.swap(i, pivot);
        let piv = aug[i][i];
        for x in aug[i].iter_mut() {
            *x /= piv;
        }
        let pivot_row = aug[i].clone();
        for (r, row) in aug.iter_mut().enumerate() {
            if r != i {
                let factor = row[i];
                for (c, &pv) in pivot_row.iter().enumerate() {
                    row[c] -= factor * pv;
                }
            }
        }
    }
    // The right half of the reduced augmented matrix is the inverse.
    Ok(aug.iter().map(|row| row[n..].to_vec()).collect())
}

pub(in crate::math::eval) fn inv(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    let m = to_square_matrix(&args[0], "inv")?;
    let inverted = invert(&m)?;
    let rows = inverted
        .into_iter()
        .map(|r| Quantity::list(r.into_iter().map(|v| Quantity::scalar(v, None)).collect()))
        .collect();
    Ok(Quantity::list(rows))
}

/// Extract a flat, dimensionless length-`n` vector of raw values. The `b`-side
/// counterpart to [`to_square_matrix`], shared by `linsolve`.
fn to_vector(qty: &Quantity, name: &str, n: usize) -> Result<Vec<f64>, String> {
    let elems = qty
        .list
        .as_ref()
        .ok_or_else(|| format!("{}() expects a vector as its second argument", name))?;
    if elems.len() != n {
        return Err(format!(
            "{}() vector length {} does not match the {}x{} matrix",
            name,
            elems.len(),
            n,
            n
        ));
    }
    let mut v = Vec::with_capacity(n);
    for cell in elems {
        if cell.list.is_some() {
            return Err(format!("{}() expects a flat vector of numbers", name));
        }
        if cell.unit.is_some() {
            return Err(format!("{}() requires a dimensionless system", name));
        }
        v.push(cell.value);
    }
    Ok(v)
}

/// Solve the linear system `A x = b` by Gaussian elimination with partial
/// pivoting followed by back-substitution. `a` is `n x n`; `b` has length `n`.
fn solve_linear(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Result<Vec<f64>, String> {
    let n = a.len();
    for i in 0..n {
        let mut pivot = i;
        for r in (i + 1)..n {
            if a[r][i].abs() > a[pivot][i].abs() {
                pivot = r;
            }
        }
        if a[pivot][i].abs() < 1e-12 {
            return Err("linsolve() matrix is singular (no unique solution)".to_string());
        }
        if pivot != i {
            a.swap(i, pivot);
            b.swap(i, pivot);
        }
        // Eliminate column i from every row below the pivot.
        let pivot_row = a[i].clone();
        let pivot_b = b[i];
        for r in (i + 1)..n {
            let factor = a[r][i] / pivot_row[i];
            for (c, &pv) in pivot_row.iter().enumerate().skip(i) {
                a[r][c] -= factor * pv;
            }
            b[r] -= factor * pivot_b;
        }
    }
    // Back-substitution: solve for each unknown from the bottom row up.
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut sum = b[i];
        for (c, xc) in x.iter().enumerate().skip(i + 1) {
            sum -= a[i][c] * xc;
        }
        x[i] = sum / a[i][i];
    }
    Ok(x)
}

pub(in crate::math::eval) fn linsolve(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let a = to_square_matrix(&args[0], "linsolve")?;
    let b = to_vector(&args[1], "linsolve", a.len())?;
    let x = solve_linear(a, b)?;
    Ok(Quantity::list(
        x.into_iter().map(|v| Quantity::scalar(v, None)).collect(),
    ))
}
