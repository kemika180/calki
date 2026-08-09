//! Shared date/time helpers: resolving civil (wall-clock) times to epoch
//! seconds, and resolving timezone identifiers (IANA names, common
//! abbreviations, and fixed UTC offsets) to a [`jiff::tz::TimeZone`].

use jiff::tz::{Offset, TimeZone};

/// Resolve a civil date/time in the given zone to `(epoch_seconds_utc,
/// utc_offset_seconds)`. The offset is the zone's offset at that instant (so
/// DST is applied), captured for round-trip rendering.
pub fn civil_to_epoch_in_zone(
    year: i16,
    month: i8,
    day: i8,
    hour: i8,
    minute: i8,
    second: i8,
    tz: &TimeZone,
) -> Result<(f64, i32), String> {
    let dt = jiff::civil::DateTime::new(year, month, day, hour, minute, second, 0)
        .map_err(|e| format!("Invalid date/time: {e}"))?;
    let zoned = dt
        .to_zoned(tz.clone())
        .map_err(|e| format!("Invalid date/time: {e}"))?;
    Ok((
        zoned.timestamp().as_second() as f64,
        zoned.offset().seconds(),
    ))
}

/// Today's civil date `(year, month, day)` in the given zone. Used to anchor a
/// standalone time literal (e.g. `9am PST`) to the current day.
pub fn today_in_zone(tz: &TimeZone) -> (i16, i8, i8) {
    let z = jiff::Timestamp::now().to_zoned(tz.clone());
    (z.year(), z.month(), z.day())
}

/// Resolve a timezone identifier to a [`TimeZone`]. Accepts, in order:
/// a fixed UTC offset (`UTC`, `GMT`, `Z`, `UTC+2`, `GMT-5`, `+3`, `-8`), a
/// common abbreviation (`PST`, `EST`, `CET`, …), or an IANA name
/// (`America/New_York`, `Europe/London`). Abbreviations map to an IANA zone, so
/// DST is applied by date (best-effort — abbreviations are inherently lossy).
pub fn resolve_timezone(name: &str) -> Result<TimeZone, String> {
    let trimmed = name.trim();
    if let Some(tz) = parse_fixed_offset(trimmed) {
        return Ok(tz);
    }
    if let Some(iana) = abbrev_to_iana(trimmed) {
        return TimeZone::get(iana).map_err(|e| format!("Unknown timezone '{}': {}", name, e));
    }
    TimeZone::get(trimmed).map_err(|_| format!("Unknown timezone '{}'", name))
}

/// Parse a fixed UTC offset: `UTC`/`GMT`/`Z` (zero), or an optional `UTC`/`GMT`
/// prefix followed by a signed whole-hour offset (`UTC+2`, `GMT-5`, `+3`, `-8`).
/// Minute-granularity offsets (`+05:30`) are not accepted here — use the IANA
/// name (e.g. `Asia/Kolkata`).
fn parse_fixed_offset(s: &str) -> Option<TimeZone> {
    if s.eq_ignore_ascii_case("UTC") || s.eq_ignore_ascii_case("GMT") || s == "Z" {
        return Some(TimeZone::UTC);
    }
    let rest = {
        let lower = s.to_ascii_lowercase();
        if let Some(stripped) = lower
            .strip_prefix("utc")
            .or_else(|| lower.strip_prefix("gmt"))
        {
            // Re-slice the original by the stripped length to preserve case-free digits.
            &s[s.len() - stripped.len()..]
        } else {
            s
        }
    };
    let (sign, digits) = match rest.strip_prefix('+') {
        Some(d) => (1, d),
        None => (-1, rest.strip_prefix('-')?),
    };
    let hours: i32 = digits.trim().parse().ok()?;
    if hours > 23 {
        return None;
    }
    Offset::from_seconds(sign * hours * 3600)
        .ok()
        .map(TimeZone::fixed)
}

/// Map a common timezone abbreviation to a representative IANA zone. Returns
/// `None` for unknown abbreviations (the caller then tries an IANA lookup).
fn abbrev_to_iana(abbr: &str) -> Option<&'static str> {
    let key = abbr.to_ascii_uppercase();
    Some(match key.as_str() {
        "EST" | "EDT" | "ET" => "America/New_York",
        "CST" | "CDT" | "CT" => "America/Chicago",
        "MST" | "MDT" | "MT" => "America/Denver",
        "PST" | "PDT" | "PT" => "America/Los_Angeles",
        "AKST" | "AKDT" => "America/Anchorage",
        "HST" => "Pacific/Honolulu",
        "BST" | "GB" => "Europe/London",
        "CET" | "CEST" => "Europe/Paris",
        "EET" | "EEST" => "Europe/Athens",
        "IST" => "Asia/Kolkata",
        "JST" => "Asia/Tokyo",
        "KST" => "Asia/Seoul",
        "SGT" => "Asia/Singapore",
        "HKT" => "Asia/Hong_Kong",
        "AEST" | "AEDT" => "Australia/Sydney",
        "NZST" | "NZDT" => "Pacific/Auckland",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_offsets() {
        for name in ["UTC", "GMT", "Z", "utc", "gmt"] {
            assert_eq!(
                resolve_timezone(name)
                    .unwrap()
                    .to_offset(jiff::Timestamp::UNIX_EPOCH),
                Offset::UTC
            );
        }
        // UTC+2 / GMT-5 / bare signed hours.
        let plus2 = resolve_timezone("UTC+2").unwrap();
        assert_eq!(
            plus2.to_offset(jiff::Timestamp::UNIX_EPOCH),
            Offset::from_seconds(2 * 3600).unwrap()
        );
        let minus5 = resolve_timezone("GMT-5").unwrap();
        assert_eq!(
            minus5.to_offset(jiff::Timestamp::UNIX_EPOCH),
            Offset::from_seconds(-5 * 3600).unwrap()
        );
        let plus3 = resolve_timezone("+3").unwrap();
        assert_eq!(
            plus3.to_offset(jiff::Timestamp::UNIX_EPOCH),
            Offset::from_seconds(3 * 3600).unwrap()
        );
    }

    #[test]
    fn test_named_and_abbrev_zones() {
        // Abbreviations resolve to IANA zones.
        assert!(resolve_timezone("PST").is_ok());
        assert!(resolve_timezone("est").is_ok());
        assert!(resolve_timezone("JST").is_ok());
        // Full IANA names.
        assert!(resolve_timezone("America/New_York").is_ok());
        assert!(resolve_timezone("Europe/London").is_ok());
        // Nonsense errors.
        assert!(resolve_timezone("Nowhere/Nowhere").is_err());
        assert!(resolve_timezone("XYZ").is_err());
    }

    #[test]
    fn test_civil_epoch_in_zone_is_dst_aware() {
        // 2026-07-01 12:00 in New York is EDT (UTC-4) in summer.
        let tz = resolve_timezone("America/New_York").unwrap();
        let (_, off) = civil_to_epoch_in_zone(2026, 7, 1, 12, 0, 0, &tz).unwrap();
        assert_eq!(off, -4 * 3600);
        // 2026-01-01 12:00 in New York is EST (UTC-5) in winter.
        let (_, off_winter) = civil_to_epoch_in_zone(2026, 1, 1, 12, 0, 0, &tz).unwrap();
        assert_eq!(off_winter, -5 * 3600);
    }
}
