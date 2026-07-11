//! General numeric builtins: logarithms, roots, rounding, bitwise `xor`,
//! `mod`, `range`, and the `plot`/`sparkline` renderer. Only `modulo` reconciles
//! units, so it alone takes the context.

use crate::math::eval::{
    Context, check_built_in_args, flatten_quantity, is_complex, make_complex_qty, to_complex_parts,
};
use crate::math::parser::Quantity;
use crate::math::units::{are_compatible, convert_quantity};

pub(in crate::math::eval) fn log(args: &[Quantity]) -> Result<Quantity, String> {
    if args.len() != 1 && args.len() != 2 {
        return Err("Function 'log' expects 1 or 2 arguments".to_string());
    }
    if args.len() == 2 {
        if args[1].unit.is_some() || is_complex(&args[1]) {
            return Err(
                "Second argument to 'log' (base) must be a real dimensionless number".to_string(),
            );
        }
        let base = args[1].value;
        if base <= 0.0 || base == 1.0 {
            return Err("Logarithm base must be positive and not equal to 1".to_string());
        }
        if is_complex(&args[0]) {
            let (a, b) = to_complex_parts(&args[0]);
            let r = (a * a + b * b).sqrt();
            let theta = b.atan2(a);
            let ln_z = make_complex_qty(r.ln(), theta);
            let (ln_re, ln_im) = to_complex_parts(&ln_z);
            let ln_base = base.ln();
            return Ok(make_complex_qty(ln_re / ln_base, ln_im / ln_base));
        }
        if args[0].value < 0.0 {
            let ln_re = (-args[0].value).ln();
            let ln_im = std::f64::consts::PI;
            let ln_base = base.ln();
            return Ok(make_complex_qty(ln_re / ln_base, ln_im / ln_base));
        }
        Ok(Quantity {
            is_bool: false,
            list: None,
            value: args[0].value.log(base),
            unit: None,
        })
    } else {
        if is_complex(&args[0]) {
            let (a, b) = to_complex_parts(&args[0]);
            let r = (a * a + b * b).sqrt();
            let theta = b.atan2(a);
            let ln_z = make_complex_qty(r.ln(), theta);
            let (ln_re, ln_im) = to_complex_parts(&ln_z);
            let ln_10 = 10.0f64.ln();
            return Ok(make_complex_qty(ln_re / ln_10, ln_im / ln_10));
        }
        if args[0].value < 0.0 {
            let ln_re = (-args[0].value).ln();
            let ln_im = std::f64::consts::PI;
            let ln_10 = 10.0f64.ln();
            return Ok(make_complex_qty(ln_re / ln_10, ln_im / ln_10));
        }
        Ok(Quantity {
            is_bool: false,
            list: None,
            value: args[0].value.log10(),
            unit: None,
        })
    }
}

pub(in crate::math::eval) fn ln(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    if is_complex(&args[0]) {
        let (a, b) = to_complex_parts(&args[0]);
        let r = (a * a + b * b).sqrt();
        let theta = b.atan2(a);
        return Ok(make_complex_qty(r.ln(), theta));
    }
    if args[0].value < 0.0 {
        return Ok(make_complex_qty(
            (-args[0].value).ln(),
            std::f64::consts::PI,
        ));
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.ln(),
        unit: None,
    })
}

pub(in crate::math::eval) fn log2(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    if is_complex(&args[0]) {
        let (a, b) = to_complex_parts(&args[0]);
        let r = (a * a + b * b).sqrt();
        let theta = b.atan2(a);
        let ln_z = make_complex_qty(r.ln(), theta);
        let (ln_re, ln_im) = to_complex_parts(&ln_z);
        let ln_2 = 2.0f64.ln();
        return Ok(make_complex_qty(ln_re / ln_2, ln_im / ln_2));
    }
    if args[0].value < 0.0 {
        let ln_re = (-args[0].value).ln();
        let ln_im = std::f64::consts::PI;
        let ln_2 = 2.0f64.ln();
        return Ok(make_complex_qty(ln_re / ln_2, ln_im / ln_2));
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.log2(),
        unit: None,
    })
}

pub(in crate::math::eval) fn sqrt(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    if is_complex(&args[0]) {
        let (a, b) = to_complex_parts(&args[0]);
        let r = (a * a + b * b).sqrt();
        let theta = b.atan2(a);
        let r_sqrt = r.sqrt();
        let half_theta = theta / 2.0;
        return Ok(make_complex_qty(
            r_sqrt * half_theta.cos(),
            r_sqrt * half_theta.sin(),
        ));
    }
    if args[0].value < 0.0 {
        let val = (-args[0].value).sqrt();
        return Ok(make_complex_qty(0.0, val));
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.sqrt(),
        unit: args[0].unit.clone(),
    })
}

pub(in crate::math::eval) fn abs(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    if is_complex(&args[0]) {
        let (a, b) = to_complex_parts(&args[0]);
        return Ok(Quantity {
            is_bool: false,
            list: None,
            value: (a * a + b * b).sqrt(),
            unit: None,
        });
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.abs(),
        unit: args[0].unit.clone(),
    })
}

pub(in crate::math::eval) fn round(args: &[Quantity]) -> Result<Quantity, String> {
    if args.len() != 1 && args.len() != 2 {
        return Err("Function 'round' expects 1 or 2 arguments".to_string());
    }
    let value = args[0].value;
    let digits = if args.len() == 2 {
        if args[1].unit.is_some() {
            return Err("Second argument of 'round' (precision) must be dimensionless".to_string());
        }
        args[1].value as i32
    } else {
        0
    };
    let factor = 10.0f64.powi(digits);
    let rounded = (value * factor).round() / factor;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: rounded,
        unit: args[0].unit.clone(),
    })
}

pub(in crate::math::eval) fn xor(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let val = (args[0].value as i64) ^ (args[1].value as i64);
    let unit = args[0].unit.clone().or(args[1].unit.clone());
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: val as f64,
        unit,
    })
}

pub(in crate::math::eval) fn ceil(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.ceil(),
        unit: args[0].unit.clone(),
    })
}

pub(in crate::math::eval) fn floor(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 1)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: args[0].value.floor(),
        unit: args[0].unit.clone(),
    })
}

pub(in crate::math::eval) fn plot(args: &[Quantity]) -> Result<Quantity, String> {
    if args.is_empty() {
        return Err("Function 'plot' expects at least 1 argument".to_string());
    }
    let mut flat_args = Vec::new();
    for arg in args {
        flatten_quantity(arg, &mut flat_args);
    }
    if flat_args.is_empty() {
        return Err("Function 'plot' expects at least 1 argument or non-empty list".to_string());
    }

    let min_val = flat_args
        .iter()
        .map(|q| q.value)
        .fold(f64::INFINITY, f64::min);
    let max_val = flat_args
        .iter()
        .map(|q| q.value)
        .fold(f64::NEG_INFINITY, f64::max);

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

pub(in crate::math::eval) fn modulo(
    name: &str,
    args: &[Quantity],
    ctx: &Context,
) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    let q1 = &args[0];
    let q2 = &args[1];
    match (&q1.unit, &q2.unit) {
        (Some(u1), Some(u2)) => {
            if !are_compatible(u1, u2) {
                return Err(format!(
                    "Incompatible units in mod(): '{}' and '{}'",
                    u1, u2
                ));
            }
            let converted = convert_quantity(q2.value, u2, u1, &ctx.exchange_rates)?;
            let rem = q1.value % converted;
            Ok(Quantity {
                is_bool: false,
                list: None,
                value: rem,
                unit: Some(u1.clone()),
            })
        }
        (None, None) => Ok(Quantity {
            is_bool: false,
            list: None,
            value: q1.value % q2.value,
            unit: None,
        }),
        _ => Err("Cannot compare a quantity with a dimensionless value in mod()".to_string()),
    }
}

pub(in crate::math::eval) fn range(args: &[Quantity]) -> Result<Quantity, String> {
    if args.is_empty() || args.len() > 3 {
        return Err("Built-in function 'range' expects 1, 2, or 3 arguments".to_string());
    }
    let start;
    let end;
    let step;

    if args.len() == 1 {
        start = 0.0;
        end = args[0].value;
        step = 1.0;
    } else if args.len() == 2 {
        start = args[0].value;
        end = args[1].value;
        step = 1.0;
    } else {
        start = args[0].value;
        end = args[1].value;
        step = args[2].value;
    }

    if step == 0.0 {
        return Err("Step value for 'range' cannot be zero".to_string());
    }

    let mut elements = Vec::new();
    let mut current = start;
    let max_range_size = 5000;

    if step > 0.0 {
        while current < end {
            if elements.len() >= max_range_size {
                return Err(format!(
                    "Range size exceeded safety limit of {}",
                    max_range_size
                ));
            }
            elements.push(Quantity::scalar(current, None));
            current += step;
        }
    } else {
        while current > end {
            if elements.len() >= max_range_size {
                return Err(format!(
                    "Range size exceeded safety limit of {}",
                    max_range_size
                ));
            }
            elements.push(Quantity::scalar(current, None));
            current += step;
        }
    }

    Ok(Quantity::list(elements))
}
