//! Human durations and UTC timestamp formatting (no external date crates).

const NS: i64 = 1_000_000_000;

/// Parse a duration such as `90s`, `25h`, `14d`, `1h30m`, `2w`, `500ms`.
///
/// Units: `w`, `d`, `h`, `m`, `s`, `ms`. A bare number means seconds. Must be > 0.
pub fn parse_duration(input: &str) -> Result<i64, String> {
    let s = input.trim();
    if s.is_empty() {
        return Err("empty duration".into());
    }
    if let Ok(n) = s.parse::<u64>() {
        return checked_positive(i64::try_from(n).ok().and_then(|n| n.checked_mul(NS)), input);
    }
    let mut total: i64 = 0;
    let mut rest = s;
    while !rest.is_empty() {
        let digits = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if digits == 0 {
            return Err(format!(
                "invalid duration `{input}`: expected a number at `{rest}`"
            ));
        }
        let n: i64 = rest[..digits]
            .parse()
            .map_err(|_| format!("invalid number in `{input}`"))?;
        rest = &rest[digits..];
        let unit_len = rest
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or(rest.len());
        let unit = &rest[..unit_len];
        rest = &rest[unit_len..];
        let mult = match unit {
            "w" => 7 * 86_400 * NS,
            "d" => 86_400 * NS,
            "h" => 3_600 * NS,
            "m" => 60 * NS,
            "s" => NS,
            "ms" => 1_000_000,
            "" => {
                return Err(format!(
                    "invalid duration `{input}`: missing unit after {n}"
                ));
            }
            other => {
                return Err(format!(
                    "invalid duration `{input}`: unknown unit `{other}` (use w, d, h, m, s, ms)"
                ));
            }
        };
        total = n
            .checked_mul(mult)
            .and_then(|v| total.checked_add(v))
            .ok_or_else(|| format!("duration `{input}` is too large"))?;
    }
    checked_positive(Some(total), input)
}

fn checked_positive(v: Option<i64>, input: &str) -> Result<i64, String> {
    match v {
        Some(v) if v > 0 => Ok(v),
        Some(_) => Err(format!("duration `{input}` must be greater than zero")),
        None => Err(format!("duration `{input}` is too large")),
    }
}

/// Format a signed nanosecond span compactly: `+14d 2h 5m 3s`, `-0s`.
pub fn format_span(ns: i64) -> String {
    let sign = if ns < 0 { '-' } else { '+' };
    let mut secs = (ns.unsigned_abs() / NS as u64) as i64;
    let d = secs / 86_400;
    secs %= 86_400;
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let mut parts = Vec::new();
    if d > 0 {
        parts.push(format!("{d}d"));
    }
    if h > 0 {
        parts.push(format!("{h}h"));
    }
    if m > 0 {
        parts.push(format!("{m}m"));
    }
    if s > 0 || parts.is_empty() {
        parts.push(format!("{s}s"));
    }
    format!("{sign}{}", parts.join(" "))
}

/// Format Unix nanoseconds as RFC 3339 UTC (`2026-11-13T10:00:00Z`).
pub fn format_utc(unix_ns: i64) -> String {
    let secs = unix_ns.div_euclid(NS);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (y, mo, d) = civil_from_days(days);
    format!(
        "{y:04}-{mo:02}-{d:02}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// Howard Hinnant's `civil_from_days`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_units() {
        assert_eq!(parse_duration("90").unwrap(), 90 * NS);
        assert_eq!(parse_duration("90s").unwrap(), 90 * NS);
        assert_eq!(parse_duration("25h").unwrap(), 25 * 3600 * NS);
        assert_eq!(parse_duration("14d").unwrap(), 14 * 86_400 * NS);
        assert_eq!(parse_duration("1h30m").unwrap(), 5400 * NS);
        assert_eq!(parse_duration("2w").unwrap(), 14 * 86_400 * NS);
        assert_eq!(parse_duration("500ms").unwrap(), 500_000_000);
        assert_eq!(parse_duration(" 1d12h ").unwrap(), 36 * 3600 * NS);
    }

    #[test]
    fn rejects_bad_input() {
        for bad in [
            "",
            "0",
            "0s",
            "d",
            "1x",
            "1h-",
            "-1h",
            "1.5h",
            "99999999999999999999d",
        ] {
            assert!(parse_duration(bad).is_err(), "accepted `{bad}`");
        }
    }

    #[test]
    fn formats_spans() {
        assert_eq!(format_span(0), "+0s");
        assert_eq!(format_span(14 * 86_400 * NS + 3_700 * NS), "+14d 1h 1m 40s");
        assert_eq!(format_span(-90 * NS), "-1m 30s");
    }

    #[test]
    fn formats_utc() {
        assert_eq!(format_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_utc(951_782_400 * NS), "2000-02-29T00:00:00Z");
        assert_eq!(format_utc(1_794_563_696 * NS), "2026-11-13T09:54:56Z");
        assert_eq!(format_utc(-1), "1969-12-31T23:59:59Z");
    }
}
