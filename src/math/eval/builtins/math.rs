//! General numeric builtins: logarithms, roots, rounding, bitwise `xor`,
//! `mod`, `range`, and the `plot`/`sparkline` renderer. Only `modulo` reconciles
//! units, so it alone takes the context.

use crate::math::eval::complex::{is_complex, make_complex_qty, to_complex_parts};
use crate::math::eval::{Context, check_built_in_args, flatten_quantity};
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

/// Lanczos coefficients (g = 7, n = 9), shared by `gamma` and `lgamma`.
const LANCZOS_G: f64 = 7.0;
const LANCZOS_C: [f64; 9] = [
    0.999_999_999_999_809_9,
    676.520_368_121_885_1,
    -1_259.139_216_722_402_8,
    771.323_428_777_653_1,
    -176.615_029_162_140_6,
    12.507_343_278_686_905,
    -0.138_571_095_265_720_12,
    9.984_369_578_019_572e-6,
    1.505_632_735_149_311_6e-7,
];

/// Lanczos approximation of the gamma function, with the reflection formula for
/// `x < 0.5`. Accurate to ~1e-15 for real arguments. Non-positive integers are
/// poles; callers guard those separately.
fn gamma_lanczos(x: f64) -> f64 {
    if x < 0.5 {
        // Reflection: Γ(x)·Γ(1−x) = π / sin(πx)
        std::f64::consts::PI / ((std::f64::consts::PI * x).sin() * gamma_lanczos(1.0 - x))
    } else {
        let x = x - 1.0;
        let mut a = LANCZOS_C[0];
        let t = x + LANCZOS_G + 0.5;
        for (i, &c) in LANCZOS_C.iter().enumerate().skip(1) {
            a += c / (x + i as f64);
        }
        (2.0 * std::f64::consts::PI).sqrt() * t.powf(x + 0.5) * (-t).exp() * a
    }
}

/// `ln|Γ(x)|` via the same Lanczos series, computed in log space so it stays
/// finite for large `x` (where `gamma` overflows). Reflection for `x < 0.5`.
fn lgamma_lanczos(x: f64) -> f64 {
    if x < 0.5 {
        std::f64::consts::PI.ln()
            - (std::f64::consts::PI * x).sin().abs().ln()
            - lgamma_lanczos(1.0 - x)
    } else {
        let x = x - 1.0;
        let mut a = LANCZOS_C[0];
        let t = x + LANCZOS_G + 0.5;
        for (i, &c) in LANCZOS_C.iter().enumerate().skip(1) {
            a += c / (x + i as f64);
        }
        0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
    }
}

/// Shared validation for single-argument special functions: one real,
/// dimensionless argument. Returns the value.
fn real_dimensionless_arg(name: &str, args: &[Quantity]) -> Result<f64, String> {
    check_built_in_args(name, args, 1)?;
    if args[0].unit.is_some() {
        return Err(format!(
            "Function '{}' expects a dimensionless argument",
            name
        ));
    }
    if is_complex(&args[0]) {
        return Err(format!(
            "Function '{}' does not support complex arguments",
            name
        ));
    }
    Ok(args[0].value)
}

/// Validation for the gamma-family builtins: additionally rejects the poles at
/// non-positive integers.
fn gamma_family_arg(name: &str, args: &[Quantity]) -> Result<f64, String> {
    let x = real_dimensionless_arg(name, args)?;
    if x <= 0.0 && x.fract() == 0.0 {
        return Err(format!("'{}' is undefined at non-positive integers", name));
    }
    Ok(x)
}

/// Error function via Abramowitz & Stegun 7.1.26 (max abs error ~1.5e-7).
pub(in crate::math::eval) fn erf_approx(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    const A1: f64 = 0.254_829_592;
    const A2: f64 = -0.284_496_736;
    const A3: f64 = 1.421_413_741;
    const A4: f64 = -1.453_152_027;
    const A5: f64 = 1.061_405_429;
    let poly = ((((A5 * t + A4) * t + A3) * t + A2) * t + A1) * t;
    sign * (1.0 - poly * (-x * x).exp())
}

pub(in crate::math::eval) fn erf(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    let x = real_dimensionless_arg(name, args)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: erf_approx(x),
        unit: None,
    })
}

pub(in crate::math::eval) fn erfc(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    let x = real_dimensionless_arg(name, args)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: 1.0 - erf_approx(x),
        unit: None,
    })
}

/// Inverse error function on (-1, 1): Winitzki's rational initial guess refined
/// by two Newton steps against [`erf_approx`] (`f(y) = erf(y) − x`).
pub(in crate::math::eval) fn erfinv_approx(x: f64) -> f64 {
    if x <= -1.0 {
        return f64::NEG_INFINITY;
    }
    if x >= 1.0 {
        return f64::INFINITY;
    }
    if x == 0.0 {
        return 0.0;
    }
    const A: f64 = 0.147;
    let ln1mx2 = (1.0 - x * x).ln();
    let t1 = 2.0 / (std::f64::consts::PI * A) + ln1mx2 / 2.0;
    let mut y = x.signum() * ((t1 * t1 - ln1mx2 / A).sqrt() - t1).sqrt();
    for _ in 0..2 {
        let residual = erf_approx(y) - x;
        let deriv = 2.0 / std::f64::consts::PI.sqrt() * (-y * y).exp();
        y -= residual / deriv;
    }
    y
}

pub(in crate::math::eval) fn erfinv(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    let x = real_dimensionless_arg(name, args)?;
    if x <= -1.0 || x >= 1.0 {
        return Err(format!("Function '{}' is defined only on (-1, 1)", name));
    }
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: erfinv_approx(x),
        unit: None,
    })
}

pub(in crate::math::eval) fn gamma(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    let x = gamma_family_arg(name, args)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: gamma_lanczos(x),
        unit: None,
    })
}

pub(in crate::math::eval) fn lgamma(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    let x = gamma_family_arg(name, args)?;
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: lgamma_lanczos(x),
        unit: None,
    })
}

pub(in crate::math::eval) fn beta(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 2)?;
    for a in args {
        if a.unit.is_some() {
            return Err(format!(
                "Function '{}' expects dimensionless arguments",
                name
            ));
        }
        if is_complex(a) {
            return Err(format!(
                "Function '{}' does not support complex arguments",
                name
            ));
        }
    }
    let (a, b) = (args[0].value, args[1].value);
    if a <= 0.0 || b <= 0.0 {
        return Err(format!("Function '{}' requires positive arguments", name));
    }
    // B(a,b) = Γ(a)Γ(b)/Γ(a+b), evaluated in log space to avoid overflow.
    let val = (lgamma_lanczos(a) + lgamma_lanczos(b) - lgamma_lanczos(a + b)).exp();
    Ok(Quantity {
        is_bool: false,
        list: None,
        value: val,
        unit: None,
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
