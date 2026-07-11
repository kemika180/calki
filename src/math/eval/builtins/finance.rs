//! Financial builtins: `pmt`, `fv`, `pv`. Pure over the evaluated arguments.

use crate::math::eval::check_built_in_args;
use crate::math::parser::Quantity;

pub(in crate::math::eval) fn pmt(name: &str, args: &[Quantity]) -> Result<Quantity, String> {
    check_built_in_args(name, args, 3)?;
    let rate = args[0].value;
    let nper = args[1].value;
    let pv = args[2].value;

    if args[0].unit.is_some() {
        return Err(
            "First argument of 'pmt' (rate) must be dimensionless or percentage".to_string(),
        );
    }
    if args[1].unit.is_some() {
        return Err("Second argument of 'pmt' (nper) must be dimensionless".to_string());
    }

    let pmt_val = if rate == 0.0 {
        -pv / nper
    } else {
        -(rate * pv) / (1.0 - (1.0 + rate).powf(-nper))
    };

    Ok(Quantity {
        is_bool: false,
        list: None,
        value: pmt_val,
        unit: args[2].unit.clone(),
    })
}

pub(in crate::math::eval) fn fv(args: &[Quantity]) -> Result<Quantity, String> {
    if args.len() != 3 && args.len() != 4 {
        return Err("Function 'fv' expects 3 or 4 arguments".to_string());
    }
    let rate = args[0].value;
    let nper = args[1].value;
    let pmt = args[2].value;
    let pv = if args.len() == 4 { args[3].value } else { 0.0 };

    if args[0].unit.is_some() {
        return Err(
            "First argument of 'fv' (rate) must be dimensionless or percentage".to_string(),
        );
    }
    if args[1].unit.is_some() {
        return Err("Second argument of 'fv' (nper) must be dimensionless".to_string());
    }

    let fv_val = if rate == 0.0 {
        -pv - pmt * nper
    } else {
        let term = (1.0 + rate).powf(nper);
        -pv * term - pmt * (term - 1.0) / rate
    };

    let target_unit = if args.len() == 4 && args[3].unit.is_some() {
        args[3].unit.clone()
    } else {
        args[2].unit.clone()
    };

    Ok(Quantity {
        is_bool: false,
        list: None,
        value: fv_val,
        unit: target_unit,
    })
}

pub(in crate::math::eval) fn pv(args: &[Quantity]) -> Result<Quantity, String> {
    if args.len() != 3 && args.len() != 4 {
        return Err("Function 'pv' expects 3 or 4 arguments".to_string());
    }
    let rate = args[0].value;
    let nper = args[1].value;
    let pmt = args[2].value;
    let fv = if args.len() == 4 { args[3].value } else { 0.0 };

    if args[0].unit.is_some() {
        return Err(
            "First argument of 'pv' (rate) must be dimensionless or percentage".to_string(),
        );
    }
    if args[1].unit.is_some() {
        return Err("Second argument of 'pv' (nper) must be dimensionless".to_string());
    }

    let pv_val = if rate == 0.0 {
        -fv - pmt * nper
    } else {
        let term = (1.0 + rate).powf(-nper);
        -fv * term - pmt * (1.0 - term) / rate
    };

    let target_unit = if args.len() == 4 && args[3].unit.is_some() {
        args[3].unit.clone()
    } else {
        args[2].unit.clone()
    };

    Ok(Quantity {
        is_bool: false,
        list: None,
        value: pv_val,
        unit: target_unit,
    })
}
