//! Complex-number representation helpers.
//!
//! A complex value is encoded either as a pure-imaginary quantity (unit `"i"`)
//! or as a `"complex"`-unit quantity whose `list` holds `[re, im]`. These three
//! helpers convert between that encoding and raw `(re, im)` pairs.

use crate::math::parser::Quantity;

pub(in crate::math::eval) fn is_complex(qty: &Quantity) -> bool {
    qty.unit.as_deref() == Some("i")
        || (qty.unit.as_deref() == Some("complex") && qty.list.is_some())
}

pub(in crate::math::eval) fn to_complex_parts(qty: &Quantity) -> (f64, f64) {
    if qty.unit.as_deref() == Some("i") {
        (0.0, qty.value)
    } else if qty.unit.as_deref() == Some("complex") {
        if let Some(ref list) = qty.list
            && list.len() >= 2
        {
            return (list[0].value, list[1].value);
        }
        (qty.value, 0.0)
    } else {
        (qty.value, 0.0)
    }
}

pub(in crate::math::eval) fn make_complex_qty(re: f64, im: f64) -> Quantity {
    if im == 0.0 {
        Quantity {
            display: None,
            value: re,
            unit: None,
            list: None,
            is_bool: false,
        }
    } else if re == 0.0 {
        Quantity {
            display: None,
            value: im,
            unit: Some("i".to_string()),
            list: None,
            is_bool: false,
        }
    } else {
        Quantity {
            display: None,
            value: re,
            unit: Some("complex".to_string()),
            list: Some(vec![
                Quantity {
                    display: None,
                    value: re,
                    unit: None,
                    list: None,
                    is_bool: false,
                },
                Quantity {
                    display: None,
                    value: im,
                    unit: Some("i".to_string()),
                    list: None,
                    is_bool: false,
                },
            ]),
            is_bool: false,
        }
    }
}
