use std::collections::HashMap;
use std::cell::RefCell;

thread_local! {
    pub static CUSTOM_UNIT_PROFILES: RefCell<HashMap<String, HashMap<Dimension, i32>>> = RefCell::new(HashMap::new());
    pub static CUSTOM_UNIT_FACTORS: RefCell<HashMap<String, f64>> = RefCell::new(HashMap::new());
}

pub fn register_custom_unit(name: &str, value: f64, unit_str: &str) -> Result<(), String> {
    let map = parse_unit(unit_str);
    let profile = get_dimension_profile(&map)?;

    let mut factor = value;
    for (u, exp) in &map {
        let u_factor = get_linear_factor(u, &HashMap::new())?;
        factor *= u_factor.powi(*exp);
    }

    CUSTOM_UNIT_PROFILES.with(|profiles| {
        profiles.borrow_mut().insert(name.to_string(), profile);
    });
    CUSTOM_UNIT_FACTORS.with(|factors| {
        factors.borrow_mut().insert(name.to_string(), factor);
    });

    Ok(())
}

pub fn clear_custom_units() {
    CUSTOM_UNIT_PROFILES.with(|p| p.borrow_mut().clear());
    CUSTOM_UNIT_FACTORS.with(|f| f.borrow_mut().clear());
}

pub fn is_custom_unit(name: &str) -> bool {
    CUSTOM_UNIT_FACTORS.with(|f| {
        f.borrow().contains_key(name)
    })
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Dimension {
    Length,
    Time,
    Mass,
    Area,
    Volume,
    Speed,
    Data,
    Temperature,
    Currency,
    Energy,
    Power,
    Force,
    Frequency,
    Pressure,
}

#[derive(Clone)]
pub enum Conversion {
    Linear(f64), // multiply by factor to get base unit
    Temperature(TempUnit),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TempUnit {
    C,
    K,
    F,
}

pub fn get_exact_unit_info(name: &str) -> Option<(Dimension, Conversion)> {
    match name {
        // Length (Base: m)
        "m" | "meter" | "meters" => Some((Dimension::Length, Conversion::Linear(1.0))),
        "cm" | "centimeter" | "centimeters" => Some((Dimension::Length, Conversion::Linear(0.01))),
        "mm" | "millimeter" | "millimeters" => Some((Dimension::Length, Conversion::Linear(0.001))),
        "km" | "kilometer" | "kilometers" => Some((Dimension::Length, Conversion::Linear(1000.0))),
        "inch" | "inches" => Some((Dimension::Length, Conversion::Linear(0.0254))),
        "ft" | "feet" | "foot" => Some((Dimension::Length, Conversion::Linear(0.3048))),
        "yard" | "yards" | "yd" => Some((Dimension::Length, Conversion::Linear(0.9144))),
        "mile" | "miles" | "mi" => Some((Dimension::Length, Conversion::Linear(1609.344))),

        // Time (Base: sec)
        "sec" | "s" | "second" | "seconds" => Some((Dimension::Time, Conversion::Linear(1.0))),
        "ms" | "millisecond" | "milliseconds" => Some((Dimension::Time, Conversion::Linear(0.001))),
        "min" | "mins" | "minute" | "minutes" => Some((Dimension::Time, Conversion::Linear(60.0))),
        "hour" | "hours" | "hr" | "hrs" | "h" => Some((Dimension::Time, Conversion::Linear(3600.0))),
        "day" | "days" => Some((Dimension::Time, Conversion::Linear(86400.0))),
        "week" | "weeks" => Some((Dimension::Time, Conversion::Linear(604800.0))),
        "month" | "months" => Some((Dimension::Time, Conversion::Linear(2628000.0))),
        "year" | "years" | "yr" | "yrs" => Some((Dimension::Time, Conversion::Linear(31536000.0))),

        // Mass (Base: kg)
        "kg" | "kilogram" | "kilograms" => Some((Dimension::Mass, Conversion::Linear(1.0))),
        "g" | "gram" | "grams" => Some((Dimension::Mass, Conversion::Linear(0.001))),
        "mg" | "milligram" | "milligrams" => Some((Dimension::Mass, Conversion::Linear(0.000001))),
        "ton" | "tons" => Some((Dimension::Mass, Conversion::Linear(1000.0))),
        "lb" | "lbs" | "pound" | "pounds" => Some((Dimension::Mass, Conversion::Linear(0.45359237))),
        "oz" | "ounce" | "ounces" => Some((Dimension::Mass, Conversion::Linear(0.028349523))),

        // Area (Base: m^2)
        "m^2" | "m2" => Some((Dimension::Area, Conversion::Linear(1.0))),
        "cm^2" | "cm2" => Some((Dimension::Area, Conversion::Linear(0.0001))),
        "km^2" | "km2" => Some((Dimension::Area, Conversion::Linear(1000000.0))),
        "hectare" | "hectares" | "ha" => Some((Dimension::Area, Conversion::Linear(10000.0))),
        "acre" | "acres" => Some((Dimension::Area, Conversion::Linear(4046.8564))),

        // Volume (Base: m^3)
        "m^3" | "m3" => Some((Dimension::Volume, Conversion::Linear(1.0))),
        "L" | "l" | "liter" | "liters" => Some((Dimension::Volume, Conversion::Linear(0.001))),
        "mL" | "ml" | "milliliter" | "milliliters" => Some((Dimension::Volume, Conversion::Linear(0.000001))),
        "tsp" | "teaspoon" | "teaspoons" => Some((Dimension::Volume, Conversion::Linear(0.00000492892159))),
        "tbsp" | "tablespoon" | "tablespoons" => Some((Dimension::Volume, Conversion::Linear(0.0000147867648))),
        "cup" | "cups" => Some((Dimension::Volume, Conversion::Linear(0.00024))),
        "pint" | "pints" | "pt" => Some((Dimension::Volume, Conversion::Linear(0.000473176473))),
        "quart" | "quarts" | "qt" => Some((Dimension::Volume, Conversion::Linear(0.000946352946))),
        "gallon" | "gallons" | "gal" => Some((Dimension::Volume, Conversion::Linear(0.00378541178))),

        // Speed (Base: m/s)
        "m/s" => Some((Dimension::Speed, Conversion::Linear(1.0))),
        "km/h" | "kmh" => Some((Dimension::Speed, Conversion::Linear(0.277777778))),
        "mph" => Some((Dimension::Speed, Conversion::Linear(0.44704))),
        "knot" | "knots" | "kt" | "kts" => Some((Dimension::Speed, Conversion::Linear(0.514444444))),

        // Data / Storage (Base: B)
        "B" | "byte" | "bytes" => Some((Dimension::Data, Conversion::Linear(1.0))),
        "KB" | "kb" | "kilobyte" | "kilobytes" => Some((Dimension::Data, Conversion::Linear(1000.0))),
        "MB" | "mb" | "megabyte" | "megabytes" => Some((Dimension::Data, Conversion::Linear(1000000.0))),
        "GB" | "gb" | "gigabyte" | "gigabytes" => Some((Dimension::Data, Conversion::Linear(1000000000.0))),
        "TB" | "tb" | "terabyte" | "terabytes" => Some((Dimension::Data, Conversion::Linear(1000000000000.0))),
        "KiB" | "kib" => Some((Dimension::Data, Conversion::Linear(1024.0))),
        "MiB" | "mib" => Some((Dimension::Data, Conversion::Linear(1048576.0))),
        "GiB" | "gib" => Some((Dimension::Data, Conversion::Linear(1073741824.0))),
        "TiB" | "tib" => Some((Dimension::Data, Conversion::Linear(1099511627776.0))),

        // Temperature (Base: C)
        "C" | "celsius" => Some((Dimension::Temperature, Conversion::Temperature(TempUnit::C))),
        "K" | "kelvin" => Some((Dimension::Temperature, Conversion::Temperature(TempUnit::K))),
        "F" | "fahrenheit" => Some((Dimension::Temperature, Conversion::Temperature(TempUnit::F))),

        // Energy (Base: J)
        "J" | "joule" | "joules" => Some((Dimension::Energy, Conversion::Linear(1.0))),
        "eV" | "electronvolt" | "electronvolts" => Some((Dimension::Energy, Conversion::Linear(1.602176634e-19))),
        "cal" | "calorie" | "calories" => Some((Dimension::Energy, Conversion::Linear(4.184))),
        "kcal" | "kilocalorie" | "kilocalories" => Some((Dimension::Energy, Conversion::Linear(4184.0))),
        "Wh" | "watt-hour" | "watt-hours" => Some((Dimension::Energy, Conversion::Linear(3600.0))),
        "kWh" | "kilowatt-hour" => Some((Dimension::Energy, Conversion::Linear(3600000.0))),
        "MWh" | "megawatt-hour" => Some((Dimension::Energy, Conversion::Linear(3600000000.0))),

        // Power (Base: W)
        "W" | "watt" | "watts" => Some((Dimension::Power, Conversion::Linear(1.0))),
        "kW" | "kilowatt" => Some((Dimension::Power, Conversion::Linear(1000.0))),
        "MW" | "megawatt" => Some((Dimension::Power, Conversion::Linear(1000000.0))),

        // Force (Base: N)
        "N" | "newton" | "newtons" => Some((Dimension::Force, Conversion::Linear(1.0))),

        // Frequency (Base: Hz)
        "Hz" | "hertz" => Some((Dimension::Frequency, Conversion::Linear(1.0))),

        // Pressure (Base: Pa)
        "Pa" | "pascal" | "pascals" => Some((Dimension::Pressure, Conversion::Linear(1.0))),
        "psi" => Some((Dimension::Pressure, Conversion::Linear(6894.757293104))),
        "bar" | "bars" => Some((Dimension::Pressure, Conversion::Linear(100000.0))),
        "atm" | "atmosphere" | "atmospheres" => Some((Dimension::Pressure, Conversion::Linear(101325.0))),

        // Currency (Base: USD)
        "USD" | "$" | "EUR" | "GBP" | "CAD" | "AUD" | "JPY" | "CNY" => Some((Dimension::Currency, Conversion::Linear(1.0))),

        _ => None,
    }
}

const SHORT_PREFIXES: &[(&str, f64)] = &[
    ("p", 1e-12),
    ("n", 1e-9),
    ("u", 1e-6),
    ("μ", 1e-6),
    ("m", 1e-3),
    ("c", 1e-2),
    ("d", 1e-1),
    ("k", 1e3),
    ("M", 1e6),
    ("G", 1e9),
    ("T", 1e12),
];

const LONG_PREFIXES: &[(&str, f64)] = &[
    ("pico", 1e-12),
    ("nano", 1e-9),
    ("micro", 1e-6),
    ("centi", 1e-2),
    ("deci", 1e-1),
    ("kilo", 1e3),
    ("mega", 1e6),
    ("giga", 1e9),
    ("tera", 1e12),
];

fn is_short_base(base: &str) -> bool {
    matches!(base, "m" | "g" | "s" | "sec" | "l" | "L" | "W" | "Wh" | "wh" | "J" | "eV" | "cal" | "N" | "Hz" | "Pa")
}

fn is_long_base(base: &str) -> bool {
    matches!(base, "meter" | "meters" | "gram" | "grams" | "second" | "seconds" | "liter" | "liters" | "watt" | "watts" | "watt-hour" | "watt-hours" | "joule" | "joules" | "electronvolt" | "electronvolts" | "calorie" | "calories" | "newton" | "newtons" | "hertz" | "pascal" | "pascals" | "bar" | "bars" | "atmosphere" | "atmospheres")
}

pub fn get_unit_info(name: &str) -> Option<(Dimension, Conversion)> {
    let custom_opt = CUSTOM_UNIT_PROFILES.with(|profiles| {
        profiles.borrow().get(name).cloned()
    });
    if let Some(profile) = custom_opt
        && profile.len() == 1 {
            let (&dim, &exp) = profile.iter().next().unwrap();
            if exp == 1 {
                let factor = CUSTOM_UNIT_FACTORS.with(|factors| {
                    factors.borrow().get(name).cloned().unwrap_or(1.0)
                });
                return Some((dim, Conversion::Linear(factor)));
            }
        }

    if let Some(info) = get_exact_unit_info(name) {
        return Some(info);
    }

    // Try matching long prefixes
    for &(prefix, multiplier) in LONG_PREFIXES {
        if name.starts_with(prefix) && name.len() > prefix.len() {
            let suffix = &name[prefix.len()..];
            if (is_long_base(suffix) || is_custom_unit(suffix))
                && let Some((dim, Conversion::Linear(base_factor))) = get_unit_info(suffix) {
                    return Some((dim, Conversion::Linear(base_factor * multiplier)));
                }
        }
    }

    // Try matching short prefixes
    for &(prefix, multiplier) in SHORT_PREFIXES {
        if name.starts_with(prefix) && name.len() > prefix.len() {
            let suffix = &name[prefix.len()..];
            if (is_short_base(suffix) || is_custom_unit(suffix))
                && let Some((dim, Conversion::Linear(base_factor))) = get_unit_info(suffix) {
                    return Some((dim, Conversion::Linear(base_factor * multiplier)));
                }
        }
    }

    None
}

pub fn get_dimension_profile(map: &HashMap<String, i32>) -> Result<HashMap<Dimension, i32>, String> {
    let mut profile = HashMap::new();
    for (unit, exp) in map {
        let custom_opt = CUSTOM_UNIT_PROFILES.with(|profiles| {
            profiles.borrow().get(unit).cloned()
        });
        if let Some(custom_profile) = custom_opt {
            for (dim, d_exp) in custom_profile {
                *profile.entry(dim).or_insert(0) += d_exp * exp;
            }
        } else {
            let (dim, _) = get_unit_info(unit)
                .ok_or_else(|| format!("Unknown unit '{}'", unit))?;
            match dim {
                Dimension::Area => {
                    *profile.entry(Dimension::Length).or_insert(0) += 2 * exp;
                }
                Dimension::Volume => {
                    *profile.entry(Dimension::Length).or_insert(0) += 3 * exp;
                }
                Dimension::Speed => {
                    *profile.entry(Dimension::Length).or_insert(0) += exp;
                    *profile.entry(Dimension::Time).or_insert(0) -= exp;
                }
                Dimension::Energy => {
                    *profile.entry(Dimension::Mass).or_insert(0) += exp;
                    *profile.entry(Dimension::Length).or_insert(0) += 2 * exp;
                    *profile.entry(Dimension::Time).or_insert(0) -= 2 * exp;
                }
                Dimension::Power => {
                    *profile.entry(Dimension::Mass).or_insert(0) += exp;
                    *profile.entry(Dimension::Length).or_insert(0) += 2 * exp;
                    *profile.entry(Dimension::Time).or_insert(0) -= 3 * exp;
                }
                Dimension::Force => {
                    *profile.entry(Dimension::Mass).or_insert(0) += exp;
                    *profile.entry(Dimension::Length).or_insert(0) += exp;
                    *profile.entry(Dimension::Time).or_insert(0) -= 2 * exp;
                }
                Dimension::Frequency => {
                    *profile.entry(Dimension::Time).or_insert(0) -= exp;
                }
                Dimension::Pressure => {
                    *profile.entry(Dimension::Mass).or_insert(0) += exp;
                    *profile.entry(Dimension::Length).or_insert(0) -= exp;
                    *profile.entry(Dimension::Time).or_insert(0) -= 2 * exp;
                }
                _ => {
                    *profile.entry(dim).or_insert(0) += exp;
                }
            }
        }
    }
    profile.retain(|_, &mut v| v != 0);
    Ok(profile)
}

fn get_linear_factor(unit: &str, rates: &HashMap<String, f64>) -> Result<f64, String> {
    let custom_factor = CUSTOM_UNIT_FACTORS.with(|factors| {
        factors.borrow().get(unit).cloned()
    });
    if let Some(factor) = custom_factor {
        return Ok(factor);
    }

    let (dim, conv) = get_unit_info(unit)
        .ok_or_else(|| format!("Unknown unit '{}'", unit))?;
    match dim {
        Dimension::Currency => {
            if unit == "USD" || unit == "$" {
                Ok(1.0)
            } else {
                let rate = rates.get(unit).ok_or_else(|| {
                    format!("Exchange rate not loaded for currency '{}'", unit)
                })?;
                Ok(1.0 / rate)
            }
        }
        Dimension::Temperature => {
            match conv {
                Conversion::Temperature(TempUnit::C) => Ok(1.0),
                Conversion::Temperature(TempUnit::K) => Ok(1.0),
                Conversion::Temperature(TempUnit::F) => Ok(1.0 / 1.8),
                _ => Err("Invalid temperature conversion".to_string()),
            }
        }
        _ => {
            match conv {
                Conversion::Linear(factor) => Ok(factor),
                _ => Err("Invalid linear conversion".to_string()),
            }
        }
    }
}

pub fn convert_quantity(
    val: f64,
    from_unit: &str,
    to_unit: &str,
    rates: &HashMap<String, f64>,
) -> Result<f64, String> {
    // Check if both are simple units and both are temperature units
    if let (Some((Dimension::Temperature, Conversion::Temperature(from_t))),
            Some((Dimension::Temperature, Conversion::Temperature(to_t)))) =
        (get_unit_info(from_unit), get_unit_info(to_unit))
    {
        let c_val = match from_t {
            TempUnit::C => val,
            TempUnit::K => val - 273.15,
            TempUnit::F => (val - 32.0) / 1.8,
        };
        let target_val = match to_t {
            TempUnit::C => c_val,
            TempUnit::K => c_val + 273.15,
            TempUnit::F => c_val * 1.8 + 32.0,
        };
        return Ok(target_val);
    }

    // Otherwise, parse as compound units
    let from_map = parse_unit(from_unit);
    let to_map = parse_unit(to_unit);

    // Compute dimension profiles to check compatibility
    let from_profile = get_dimension_profile(&from_map)?;
    let to_profile = get_dimension_profile(&to_map)?;

    if from_profile != to_profile {
        return Err(format!(
            "Cannot convert from unit '{}' to incompatible unit '{}'",
            from_unit, to_unit
        ));
    }

    // Calculate conversion factor
    let mut from_factor = 1.0;
    for (unit, exp) in &from_map {
        let u_factor = get_linear_factor(unit, rates)?;
        from_factor *= u_factor.powi(*exp);
    }

    let mut to_factor = 1.0;
    for (unit, exp) in &to_map {
        let u_factor = get_linear_factor(unit, rates)?;
        to_factor *= u_factor.powi(*exp);
    }

    Ok(val * from_factor / to_factor)
}

fn parse_unit_term(term: &str) -> (String, i32) {
    let term = term.trim();
    if term.is_empty() {
        return ("".to_string(), 0);
    }
    if let Some(pos) = term.find('^') {
        let name = term[..pos].trim().to_string();
        let exp_str = term[pos + 1..].trim();
        let exp = exp_str.parse::<i32>().unwrap_or(1);
        (name, exp)
    } else {
        let mut name_end = term.len();
        let chars: Vec<char> = term.chars().collect();
        while name_end > 0 && (chars[name_end - 1].is_ascii_digit() || chars[name_end - 1] == '-') {
            name_end -= 1;
        }
        if name_end > 0 && name_end < term.len() {
            let name = term[..name_end].trim().to_string();
            let exp_str = &term[name_end..];
            if let Ok(exp) = exp_str.parse::<i32>() {
                (name, exp)
            } else {
                (term.to_string(), 1)
            }
        } else {
            (term.to_string(), 1)
        }
    }
}

// Helper: check if two units have the same dimension
pub fn are_compatible(unit1: &str, unit2: &str) -> bool {
    let map1 = parse_unit(unit1);
    let map2 = parse_unit(unit2);
    if let (Ok(p1), Ok(p2)) = (get_dimension_profile(&map1), get_dimension_profile(&map2)) {
        p1 == p2
    } else {
        false
    }
}

pub fn parse_unit(s: &str) -> HashMap<String, i32> {
    let mut exponents: HashMap<String, i32> = HashMap::new();
    if s.is_empty() {
        return exponents;
    }

    let mut current_token = String::new();
    let mut current_is_denom = false;
    
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '/' || c == '*' {
            if !current_token.trim().is_empty() {
                let (unit_name, term_exp) = parse_unit_term(&current_token);
                if !unit_name.is_empty() && unit_name != "1" {
                    let total_exp = if current_is_denom { -term_exp } else { term_exp };
                    *exponents.entry(unit_name).or_insert(0) += total_exp;
                }
                current_token.clear();
            }
            current_is_denom = c == '/';
        } else {
            current_token.push(c);
        }
        i += 1;
    }
    
    if !current_token.trim().is_empty() {
        let (unit_name, term_exp) = parse_unit_term(&current_token);
        if !unit_name.is_empty() && unit_name != "1" {
            let total_exp = if current_is_denom { -term_exp } else { term_exp };
            *exponents.entry(unit_name).or_insert(0) += total_exp;
        }
    }
    
    exponents.retain(|_, &mut exp| exp != 0);
    exponents
}

pub fn format_unit_map(exponents: &HashMap<String, i32>) -> Option<String> {
    let mut numerators = Vec::new();
    let mut denominators = Vec::new();

    let mut keys: Vec<&String> = exponents.keys().collect();
    keys.sort();

    for key in keys {
        let exp = exponents[key];
        if exp > 0 {
            if exp == 1 {
                numerators.push(key.clone());
            } else {
                numerators.push(format!("{}^{}", key, exp));
            }
        } else if exp < 0 {
            let abs_exp = exp.abs();
            if abs_exp == 1 {
                denominators.push(key.clone());
            } else {
                denominators.push(format!("{}^{}", key, abs_exp));
            }
        }
    }

    if numerators.is_empty() && denominators.is_empty() {
        None
    } else if denominators.is_empty() {
        Some(numerators.join("*"))
    } else if numerators.is_empty() {
        Some(format!("1/{}", denominators.join("/")))
    } else {
        Some(format!("{}/{}", numerators.join("*"), denominators.join("/")))
    }
}

pub fn combine_units_with_multiplier(
    u1: Option<&str>,
    u2: Option<&str>,
    is_division: bool,
    rates: &HashMap<String, f64>,
) -> (Option<String>, f64) {
    match (u1, u2) {
        (Some(a), Some(b)) => {
            let map1 = parse_unit(a);
            let map2 = parse_unit(b);

            // Compute combined exponents
            let mut combined = map1.clone();
            if is_division {
                for (unit, exp) in map2 {
                    *combined.entry(unit).or_insert(0) -= exp;
                }
            } else {
                for (unit, exp) in map2 {
                    *combined.entry(unit).or_insert(0) += exp;
                }
            }

            // Group by DimensionKey
            #[derive(Clone, PartialEq, Eq, Hash)]
            enum DimensionKey {
                Known(Dimension),
                Unknown(String),
            }

            let get_unit_dimension = |unit: &str| -> Option<Dimension> {
                get_unit_info(unit).map(|(dim, _)| dim)
            };

            let mut grouped: HashMap<DimensionKey, Vec<(String, i32)>> = HashMap::new();
            for (unit, exp) in combined {
                if exp == 0 {
                    continue;
                }
                let key = if let Some(dim) = get_unit_dimension(&unit) {
                    DimensionKey::Known(dim)
                } else {
                    DimensionKey::Unknown(unit.clone())
                };
                grouped.entry(key).or_default().push((unit, exp));
            }

            let mut final_map = HashMap::new();
            let mut multiplier = 1.0;

            for (_key, mut units_list) in grouped {
                let total_exp: i32 = units_list.iter().map(|(_, exp)| exp).sum();
                if total_exp == 0 {
                    // Cancel out completely, adjust multiplier
                    for (unit, exp) in units_list {
                        if let Ok(u_factor) = get_linear_factor(&unit, rates) {
                            multiplier *= u_factor.powi(exp);
                        }
                    }
                } else {
                    // Choose one unit to keep. Sort alphabetically for determinism.
                    units_list.sort_by(|a, b| a.0.cmp(&b.0));
                    // Choose the first one
                    let chosen_unit = units_list[0].0.clone();
                    
                    // Adjust multiplier for all units in list
                    for (unit, exp) in &units_list {
                        if let Ok(u_factor) = get_linear_factor(unit, rates) {
                            multiplier *= u_factor.powi(*exp);
                        }
                    }
                    if let Ok(chosen_factor) = get_linear_factor(&chosen_unit, rates) {
                        multiplier *= chosen_factor.powi(-total_exp);
                    }

                    final_map.insert(chosen_unit, total_exp);
                }
            }

            (format_unit_map(&final_map), multiplier)
        }
        (Some(a), None) => {
            let map1 = parse_unit(a);
            (format_unit_map(&map1), 1.0)
        }
        (None, Some(b)) => {
            let mut map2 = parse_unit(b);
            if is_division {
                for exp in map2.values_mut() {
                    *exp = -*exp;
                }
            }
            (format_unit_map(&map2), 1.0)
        }
        (None, None) => (None, 1.0),
    }
}

// Helper: multiply or divide units to create derived ones with automatic cancellation & simplification
#[cfg(test)]
pub fn combine_units(u1: Option<&str>, u2: Option<&str>, is_division: bool) -> Option<String> {
    let dummy_rates = HashMap::new();
    let (unit, _) = combine_units_with_multiplier(u1, u2, is_division, &dummy_rates);
    unit
}

fn get_singular_plural(unit: &str) -> Option<(&'static str, &'static str)> {
    let pairs = [
        ("second", "seconds"),
        ("sec", "secs"),
        ("minute", "minutes"),
        ("min", "mins"),
        ("hour", "hours"),
        ("hr", "hrs"),
        ("day", "days"),
        ("week", "weeks"),
        ("month", "months"),
        ("year", "years"),
        ("yr", "yrs"),
        ("meter", "meters"),
        ("centimeter", "centimeters"),
        ("millimeter", "millimeters"),
        ("kilometer", "kilometers"),
        ("inch", "inches"),
        ("foot", "feet"),
        ("yard", "yards"),
        ("mile", "miles"),
        ("liter", "liters"),
        ("gallon", "gallons"),
        ("pound", "pounds"),
        ("lb", "lbs"),
        ("ounce", "ounces"),
        ("cup", "cups"),
        ("pint", "pints"),
        ("quart", "quarts"),
        ("ton", "tons"),
        ("gram", "grams"),
        ("kilogram", "kilograms"),
        ("watt", "watts"),
        ("watt-hour", "watt-hours"),
        ("joule", "joules"),
        ("electronvolt", "electronvolts"),
        ("calorie", "calories"),
        ("newton", "newtons"),
        ("hertz", "hertz"),
        ("pascal", "pascals"),
        ("bar", "bars"),
        ("atmosphere", "atmospheres"),
    ];
    for &(s, p) in &pairs {
        if unit == s || unit == p {
            return Some((s, p));
        }
    }
    None
}

fn adjust_token_plurality(token: &str, is_denominator: bool, value: f64) -> String {
    // Separate exponent suffix (e.g. "^2" or "2" at the end of "miles^2" or "miles2")
    let base_len = token.trim_end_matches(|c: char| c.is_ascii_digit() || c == '^').len();
    let (base, suffix) = token.split_at(base_len);

    let is_singular = is_denominator || (value.abs() - 1.0).abs() < 1e-9;

    // 1. Try direct match
    if let Some((s, p)) = get_singular_plural(base) {
        let adjusted_base = if is_singular { s } else { p };
        return format!("{}{}", adjusted_base, suffix);
    }

    // 2. Try matching long prefixes
    for &(prefix, _) in LONG_PREFIXES {
        if base.starts_with(prefix) && base.len() > prefix.len() {
            let suffix_part = &base[prefix.len()..];
            if let Some((s, p)) = get_singular_plural(suffix_part) {
                let adjusted_suffix = if is_singular { s } else { p };
                return format!("{}{}{}", prefix, adjusted_suffix, suffix);
            }
        }
    }

    // 3. Try matching short prefixes
    for &(prefix, _) in SHORT_PREFIXES {
        if base.starts_with(prefix) && base.len() > prefix.len() {
            let suffix_part = &base[prefix.len()..];
            if let Some((s, p)) = get_singular_plural(suffix_part) {
                let adjusted_suffix = if is_singular { s } else { p };
                return format!("{}{}{}", prefix, adjusted_suffix, suffix);
            }
        }
    }

    token.to_string()
}

pub fn adjust_unit_plurality(unit: &str, value: f64) -> String {
    let parts: Vec<&str> = unit.split('/').collect();
    if parts.is_empty() {
        return String::new();
    }

    // Process numerator (first part)
    let numerator_tokens: Vec<String> = parts[0]
        .split('*')
        .map(|token| adjust_token_plurality(token, false, value))
        .collect();
    let numerator = numerator_tokens.join("*");

    if parts.len() > 1 {
        // Process denominators (all parts after numerator)
        let denominator_parts: Vec<String> = parts[1..]
            .iter()
            .map(|den_part| {
                let den_tokens: Vec<String> = den_part
                    .split('*')
                    .map(|token| adjust_token_plurality(token, true, value))
                    .collect();
                den_tokens.join("*")
            })
            .collect();
        format!("{}/{}", numerator, denominator_parts.join("/"))
    } else {
        numerator
    }
}

const AUTO_SHORT_PREFIXES: &[(&str, f64)] = &[
    ("T", 1e12),
    ("G", 1e9),
    ("M", 1e6),
    ("k", 1e3),
    ("", 1.0),
    ("d", 1e-1),
    ("c", 1e-2),
    ("m", 1e-3),
    ("u", 1e-6),
    ("n", 1e-9),
    ("p", 1e-12),
];

const AUTO_LONG_PREFIXES: &[(&str, f64)] = &[
    ("tera", 1e12),
    ("giga", 1e9),
    ("mega", 1e6),
    ("kilo", 1e3),
    ("", 1.0),
    ("deci", 1e-1),
    ("centi", 1e-2),
    ("micro", 1e-6),
    ("nano", 1e-9),
    ("pico", 1e-12),
];

pub fn get_base_unit(name: &str) -> (&str, bool) {
    // Try matching long prefixes first
    for &(prefix, _) in LONG_PREFIXES {
        if name.starts_with(prefix) && name.len() > prefix.len() {
            let suffix = &name[prefix.len()..];
            if is_long_base(suffix) && get_exact_unit_info(suffix).is_some() {
                return (suffix, true);
            }
        }
    }
    // Try matching short prefixes
    for &(prefix, _) in SHORT_PREFIXES {
        if name.starts_with(prefix) && name.len() > prefix.len() {
            let suffix = &name[prefix.len()..];
            if is_short_base(suffix) && get_exact_unit_info(suffix).is_some() {
                return (suffix, false);
            }
        }
    }
    // No prefix, check if it's long or short base
    let is_long = is_long_base(name);
    (name, is_long)
}

pub fn auto_scale_quantity(mut qty: crate::math::parser::Quantity, _rates: &HashMap<String, f64>) -> crate::math::parser::Quantity {
    let Some(ref u) = qty.unit else {
        return qty;
    };
    // If it's a compound unit (contains *, /, ^), don't auto-scale it
    if u.contains('*') || u.contains('/') || u.contains('^') {
        return qty;
    }
    // Get unit info to ensure it is a linear conversion
    let Some((dim, Conversion::Linear(u_factor))) = get_unit_info(u) else {
        return qty;
    };
    
    // We don't want to auto-scale certain dimensions or units if they are not metric-based
    let (base_unit, is_long) = get_base_unit(u);
    if !is_short_base(base_unit) && !is_long_base(base_unit) {
        return qty;
    }

    let Some((_, Conversion::Linear(base_unit_factor))) = get_unit_info(base_unit) else {
        return qty;
    };

    let base_unit_val = qty.value * (u_factor / base_unit_factor);
    
    // Find the best prefix
    let prefixes = if is_long { AUTO_LONG_PREFIXES } else { AUTO_SHORT_PREFIXES };
    let is_length_or_volume = matches!(
        base_unit,
        "m" | "meter" | "meters" | "l" | "L" | "liter" | "liters"
    );

    let mut best_prefix = "";
    let mut best_multiplier = 1.0;
    let mut found = false;

    for &(prefix, multiplier) in prefixes {
        // Skip deci/centi if not length or volume
        if (prefix == "d" || prefix == "c" || prefix == "deci" || prefix == "centi") && !is_length_or_volume {
            continue;
        }
        // For Time dimension, skip prefixes with multiplier > 1.0 (like k, M, G, T)
        if dim == Dimension::Time && multiplier > 1.0 {
            continue;
        }

        let scaled_abs = (base_unit_val / multiplier).abs();
        if scaled_abs >= 1.0 && scaled_abs < 1000.0 {
            best_prefix = prefix;
            best_multiplier = multiplier;
            found = true;
            break;
        }
    }

    if !found {
        let mut min_prefix = "";
        let mut min_mult = f64::MAX;
        let mut max_prefix = "";
        let mut max_mult = f64::MIN;

        for &(prefix, multiplier) in prefixes {
            if (prefix == "d" || prefix == "c" || prefix == "deci" || prefix == "centi") && !is_length_or_volume {
                continue;
            }
            if dim == Dimension::Time && multiplier > 1.0 {
                continue;
            }
            if multiplier < min_mult {
                min_mult = multiplier;
                min_prefix = prefix;
            }
            if multiplier > max_mult {
                max_mult = multiplier;
                max_prefix = prefix;
            }
        }

        if base_unit_val.abs() > 0.0 {
            if base_unit_val.abs() < min_mult {
                best_prefix = min_prefix;
                best_multiplier = min_mult;
            } else if base_unit_val.abs() >= max_mult {
                best_prefix = max_prefix;
                best_multiplier = max_mult;
            }
        }
    }

    qty.value = base_unit_val / best_multiplier;
    qty.unit = Some(format!("{}{}", best_prefix, base_unit));
    qty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_length_conversion() {
        let rates = HashMap::new();
        let val = convert_quantity(5.0, "km", "m", &rates).unwrap();
        assert_eq!(val, 5000.0);

        let val2 = convert_quantity(1.0, "inch", "mm", &rates).unwrap();
        assert_eq!(val2, 25.4);
    }

    #[test]
    fn test_temperature_conversion() {
        let rates = HashMap::new();
        let f = convert_quantity(20.0, "C", "F", &rates).unwrap();
        assert_eq!(f, 68.0);

        let c = convert_quantity(100.0, "F", "C", &rates).unwrap();
        assert_eq!((c * 100.0).round() / 100.0, 37.78);
    }

    #[test]
    fn test_time_conversion() {
        let rates = HashMap::new();
        let months = convert_quantity(1.0, "year", "month", &rates).unwrap();
        assert_eq!(months, 12.0);

        let years = convert_quantity(24.0, "months", "years", &rates).unwrap();
        assert_eq!(years, 2.0);
    }

    #[test]
    fn test_currency_conversion() {
        let mut rates = HashMap::new();
        rates.insert("EUR".to_string(), 0.92);
        let eur_val = convert_quantity(100.0, "USD", "EUR", &rates).unwrap();
        assert_eq!(eur_val, 92.0);

        let usd_val = convert_quantity(92.0, "EUR", "USD", &rates).unwrap();
        assert_eq!(usd_val, 100.0);
    }

    #[test]
    fn test_unit_combination_and_cancellation() {
        // Multiplication: miles/day * $/gallon = miles*$/day/gallon
        let u = combine_units(Some("miles/day"), Some("$/gallon"), false);
        assert_eq!(u, Some("$*miles/day/gallon".to_string())); // sorted alphabetically

        // Division/cancellation: miles/day / (miles/gallon) = gallon/day
        let u2 = combine_units(Some("miles/day"), Some("miles/gallon"), true);
        assert_eq!(u2, Some("gallon/day".to_string()));

        // More complex cancellation: gallon/day * ($/gallon) = $/day
        let u3 = combine_units(Some("gallon/day"), Some("$/gallon"), false);
        assert_eq!(u3, Some("$/day".to_string()));

        // Division by same unit: m/s / (m/s) = None (dimensionless)
        let u4 = combine_units(Some("m/s"), Some("m/s"), true);
        assert_eq!(u4, None);
    }

    #[test]
    fn test_compound_unit_conversions() {
        let mut rates = HashMap::new();
        rates.insert("EUR".to_string(), 0.92);
        
        // $/day to $/week (1 week = 7 days)
        // 10 $/day should be 70 $/week
        let val1 = convert_quantity(10.0, "$/day", "$/week", &rates).unwrap();
        assert!((val1 - 70.0).abs() < 1e-9);
        
        // km/h to m/s
        // 90 km/h should be 25 m/s
        let val2 = convert_quantity(90.0, "km/h", "m/s", &rates).unwrap();
        assert_eq!(val2, 25.0);

        // Incompatible compound units should fail
        let err = convert_quantity(1.0, "$/day", "m/s", &rates);
        assert!(err.is_err());

        // Speed units compatibility (mph, km/h, m/s)
        let val3 = convert_quantity(65.0, "mph", "km/h", &rates).unwrap();
        assert!((val3 - 104.60736).abs() < 1e-4);
        let val4 = convert_quantity(65.0, "mph", "m/s", &rates).unwrap();
        assert!((val4 - 29.0576).abs() < 1e-4);

        // miles/mph to hours
        let val5 = convert_quantity(320.0 / 65.0, "miles/mph", "hours", &rates).unwrap();
        assert!((val5 - (320.0 / 65.0)).abs() < 1e-9);
    }

    #[test]
    fn test_metric_prefixes() {
        let rates = HashMap::new();
        
        // test nanometers (nm) to millimeters (mm)
        // 1000000 nm should be 1 mm
        let val1 = convert_quantity(1000000.0, "nm", "mm", &rates).unwrap();
        assert!((val1 - 1.0).abs() < 1e-9);

        // test kilometers (km) to meters (m) using long prefix
        let val2 = convert_quantity(2.5, "kilometers", "meters", &rates).unwrap();
        assert!((val2 - 2500.0).abs() < 1e-9);

        // test picoseconds (ps) to seconds (second)
        let val3 = convert_quantity(1e12, "ps", "second", &rates).unwrap();
        assert!((val3 - 1.0).abs() < 1e-9);

        // test milliwatts (mW) to watts (W)
        let val4 = convert_quantity(500.0, "mW", "W", &rates).unwrap();
        assert!((val4 - 0.5).abs() < 1e-9);

        // test gigawatt-hours (GWh) to watt-hours (Wh)
        let val5 = convert_quantity(1.5, "GWh", "Wh", &rates).unwrap();
        assert!((val5 - 1.5e9).abs() < 1e-9);

        // test that non-metric unit prefixing fails
        assert!(get_unit_info("kinches").is_none());
        assert!(get_unit_info("mhours").is_none());
        assert!(get_unit_info("kmiles").is_none());
    }

    #[test]
    fn test_complex_custom_units() {
        let rates = HashMap::new();

        // 1. Verify parsing of complex units
        let map_asterisk = parse_unit("kg*m^2*s^-2");
        let mut expected = HashMap::new();
        expected.insert("kg".to_string(), 1);
        expected.insert("m".to_string(), 2);
        expected.insert("s".to_string(), -2);
        assert_eq!(map_asterisk, expected);

        let map_slash = parse_unit("kg*m^2/s^2");
        assert_eq!(map_slash, expected);

        // 2. Verify formatting of complex units
        assert_eq!(format_unit_map(&map_asterisk), Some("kg*m^2/s^2".to_string()));

        // 3. Verify registration of custom complex unit J
        register_custom_unit("J", 1.0, "kg*m^2*s^-2").unwrap();

        // 4. Verify compatibility
        assert!(are_compatible("J", "kg*m^2/s^2"));
        assert!(are_compatible("J", "kg*m^2*s^-2"));

        // 5. Verify conversion
        let val1 = convert_quantity(5.0, "kg*m^2/s^2", "J", &rates).unwrap();
        assert_eq!(val1, 5.0);

        let val2 = convert_quantity(3600.0, "J", "Wh", &rates).unwrap();
        assert!((val2 - 1.0).abs() < 1e-9);

        let val3 = convert_quantity(1.0, "Wh", "J", &rates).unwrap();
        assert_eq!(val3, 3600.0);
    }

    #[test]
    fn test_unit_plurality() {
        assert_eq!(adjust_unit_plurality("days", 1.0), "day");
        assert_eq!(adjust_unit_plurality("days", 5.0), "days");
        assert_eq!(adjust_unit_plurality("day", 5.0), "days");
        assert_eq!(adjust_unit_plurality("day", 1.0), "day");
        assert_eq!(adjust_unit_plurality("miles/hour", 1.0), "mile/hour");
        assert_eq!(adjust_unit_plurality("miles/hour", 5.0), "miles/hour");
        assert_eq!(adjust_unit_plurality("miles/hours", 5.0), "miles/hour"); // denominator is always singular
        assert_eq!(adjust_unit_plurality("month/years", 12.0), "months/year");
        assert_eq!(adjust_unit_plurality("1/years", 2.0), "1/year");
        assert_eq!(adjust_unit_plurality("kilometers", 1.0), "kilometer");
        assert_eq!(adjust_unit_plurality("kilometer", 5.0), "kilometers");
    }

    #[test]
    fn test_energy_units_and_scaling() {
        let rates = HashMap::new();

        // 1. Verify exact unit info and basic conversions
        let val_ev = convert_quantity(1.0, "eV", "J", &rates).unwrap();
        assert!((val_ev - 1.602176634e-19).abs() < 1e-30);

        let val_cal = convert_quantity(1.0, "cal", "J", &rates).unwrap();
        assert_eq!(val_cal, 4.184);

        let val_kcal = convert_quantity(1.0, "kcal", "cal", &rates).unwrap();
        assert_eq!(val_kcal, 1000.0);

        // 2. Verify prefix parsing for new units (e.g. mJ, uJ, nJ, pJ, kJ, MJ, GJ, TJ)
        let mj_val = convert_quantity(1.0, "mJ", "J", &rates).unwrap();
        assert_eq!(mj_val, 0.001);

        let uj_val = convert_quantity(1.0, "uJ", "J", &rates).unwrap();
        assert_eq!(uj_val, 1e-6);

        // 3. Verify auto-scaling of values
        let q1 = crate::math::parser::Quantity::scalar(0.000001, Some("J".to_string()));
        let q1_scaled = auto_scale_quantity(q1, &rates);
        assert_eq!(q1_scaled.value, 1.0);
        assert_eq!(q1_scaled.unit, Some("uJ".to_string()));

        let q2 = crate::math::parser::Quantity::scalar(1500.0, Some("J".to_string()));
        let q2_scaled = auto_scale_quantity(q2, &rates);
        assert_eq!(q2_scaled.value, 1.5);
        assert_eq!(q2_scaled.unit, Some("kJ".to_string()));

        // check time units (ks shouldn't be matched for scaling up)
        let q_time = crate::math::parser::Quantity::scalar(3600.0, Some("s".to_string()));
        let q_time_scaled = auto_scale_quantity(q_time, &rates);
        assert_eq!(q_time_scaled.value, 3600.0);
        assert_eq!(q_time_scaled.unit, Some("s".to_string()));

        // check time scaling down (milli seconds)
        let q_time_down = crate::math::parser::Quantity::scalar(0.005, Some("s".to_string()));
        let q_time_down_scaled = auto_scale_quantity(q_time_down, &rates);
        assert_eq!(q_time_down_scaled.value, 5.0);
        assert_eq!(q_time_down_scaled.unit, Some("ms".to_string()));

        // check custom unit prefix matching (e.g. mA, kA for custom unit A = 10 m)
        register_custom_unit("A", 10.0, "m").unwrap();
        let ma_info = get_unit_info("mA").unwrap();
        assert_eq!(ma_info.0, Dimension::Length);
        if let Conversion::Linear(factor) = ma_info.1 {
            assert!((factor - 0.01).abs() < 1e-9);
        } else {
            panic!("Expected linear conversion");
        }

        let ka_info = get_unit_info("kA").unwrap();
        assert_eq!(ka_info.0, Dimension::Length);
        if let Conversion::Linear(factor) = ka_info.1 {
            assert_eq!(factor, 10000.0);
        } else {
            panic!("Expected linear conversion");
        }

        // 4. Verify Force unit conversions and J = N*m
        let force_val = convert_quantity(10.0, "N*m", "J", &rates).unwrap();
        assert_eq!(force_val, 10.0);

        let mn_val = convert_quantity(1.0, "mN", "N", &rates).unwrap();
        assert_eq!(mn_val, 0.001);

        // 5. Verify Frequency unit conversions and scaling
        let ghz_val = convert_quantity(1.0, "GHz", "Hz", &rates).unwrap();
        assert_eq!(ghz_val, 1e9);

        let q_hz = crate::math::parser::Quantity::scalar(4500000000.0, Some("Hz".to_string()));
        let q_hz_scaled = auto_scale_quantity(q_hz, &rates);
        assert_eq!(q_hz_scaled.value, 4.5);
        assert_eq!(q_hz_scaled.unit, Some("GHz".to_string()));

        // 6. Verify Pressure unit conversions and scaling
        let psi_to_pa = convert_quantity(1.0, "psi", "Pa", &rates).unwrap();
        assert_eq!(psi_to_pa, 6894.757293104);

        let bar_to_atm = convert_quantity(1.0, "bar", "atm", &rates).unwrap();
        assert!((bar_to_atm - (100000.0 / 101325.0)).abs() < 1e-9);

        let q_pa = crate::math::parser::Quantity::scalar(150000.0, Some("Pa".to_string()));
        let q_pa_scaled = auto_scale_quantity(q_pa, &rates);
        assert_eq!(q_pa_scaled.value, 150.0);
        assert_eq!(q_pa_scaled.unit, Some("kPa".to_string()));
    }
}
