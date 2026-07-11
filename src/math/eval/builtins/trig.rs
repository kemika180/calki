//! Trigonometric, hyperbolic, and exponential builtins. All pure over the
//! evaluated arguments; complex arguments are handled via the shared helpers.

use crate::math::eval::check_built_in_args;
use crate::math::eval::complex::{is_complex, make_complex_qty, to_complex_parts};
use crate::math::parser::Quantity;

pub(in crate::math::eval) fn sin(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    if is_complex(&args[0]) {
        let (a, b) = to_complex_parts(&args[0]);
        return Ok(make_complex_qty(a.sin() * b.cosh(), a.cos() * b.sinh()));
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.sin(),
        unit: None,
    })
}

pub(in crate::math::eval) fn cos(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    if is_complex(&args[0]) {
        let (a, b) = to_complex_parts(&args[0]);
        return Ok(make_complex_qty(a.cos() * b.cosh(), -a.sin() * b.sinh()));
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.cos(),
        unit: None,
    })
}

pub(in crate::math::eval) fn tan(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    if is_complex(&args[0]) {
        let (a, b) = to_complex_parts(&args[0]);
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
            (s_im * c_re - s_re * c_im) / denom,
        ));
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.tan(),
        unit: None,
    })
}

pub(in crate::math::eval) fn asin(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    let val = args[0].value;
    if !(-1.0..=1.0).contains(&val) {
        return Err("Argument to 'asin' must be between -1.0 and 1.0".to_string());
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: val.asin(),
        unit: None,
    })
}

pub(in crate::math::eval) fn acos(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    let val = args[0].value;
    if !(-1.0..=1.0).contains(&val) {
        return Err("Argument to 'acos' must be between -1.0 and 1.0".to_string());
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: val.acos(),
        unit: None,
    })
}

pub(in crate::math::eval) fn atan(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.atan(),
        unit: None,
    })
}

pub(in crate::math::eval) fn sinh(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.sinh(),
        unit: None,
    })
}

pub(in crate::math::eval) fn cosh(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.cosh(),
        unit: None,
    })
}

pub(in crate::math::eval) fn tanh(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.tanh(),
        unit: None,
    })
}

pub(in crate::math::eval) fn asinh(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.asinh(),
        unit: None,
    })
}

pub(in crate::math::eval) fn acosh(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    let val = args[0].value;
    if val < 1.0 {
        return Err("Argument to 'acosh' must be greater than or equal to 1.0".to_string());
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: val.acosh(),
        unit: None,
    })
}

pub(in crate::math::eval) fn atanh(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    let val = args[0].value;
    if val <= -1.0 || val >= 1.0 {
        return Err("Argument to 'atanh' must be between -1.0 and 1.0 (exclusive)".to_string());
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: val.atanh(),
        unit: None,
    })
}

pub(in crate::math::eval) fn exp(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    if is_complex(&args[0]) {
        let (a, b) = to_complex_parts(&args[0]);
        let r = a.exp();
        return Ok(make_complex_qty(r * b.cos(), r * b.sin()));
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.exp(),
        unit: None,
    })
}
