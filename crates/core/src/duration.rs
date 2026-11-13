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

/// Parse an RFC 3339 UTC timestamp: `2026-11-14T03:00:00Z` (seconds optional,
/// `Z` or `+00:00` required) into Unix nanoseconds.
pub fn parse_utc(input: &str) -> Result<i64, String> {
    let bad = || format!("invalid timestamp `{input}`: expected e.g. 2026-11-14T03:00:00Z (UTC)");
    let s = input.trim();
    let s = s
        .strip_suffix('Z')
        .or_else(|| s.strip_suffix("+00:00"))
        .ok_or_else(bad)?;
    let (date, time) = s.split_once('T').ok_or_else(bad)?;
    let d: Vec<&str> = date.split('-').collect();
    if d.len() != 3 {
        return Err(bad());
    }
    let (y, m, day): (i64, u32, u32) = (
        d[0].parse().map_err(|_| bad())?,
        d[1].parse().map_err(|_| bad())?,
        d[2].parse().map_err(|_| bad())?,
    );
    if !(1..=12).contains(&m) || day == 0 || day > days_in_month(y, m) {
        return Err(bad());
    }
    let tod = parse_time_of_day(time).map_err(|_| bad())?;
    Ok(days_from_civil(y, m, day) * 86_400 * NS + tod)
}

/// Parse `HH:MM` or `HH:MM:SS` (UTC time of day) into nanoseconds after midnight.
pub fn parse_time_of_day(input: &str) -> Result<i64, String> {
    let bad = || format!("invalid time of day `{input}`: expected HH:MM or HH:MM:SS (UTC)");
    let parts: Vec<&str> = input.trim().split(':').collect();
    if !(2..=3).contains(&parts.len()) || parts.iter().any(|p| p.len() != 2) {
        return Err(bad());
    }
    let nums: Result<Vec<i64>, _> = parts.iter().map(|p| p.parse::<i64>()).collect();
    let nums = nums.map_err(|_| bad())?;
    let (h, m, sec) = (nums[0], nums[1], *nums.get(2).unwrap_or(&0));
    if h > 23 || m > 59 || sec > 59 || h < 0 || m < 0 || sec < 0 {
        return Err(bad());
    }
    Ok((h * 3600 + m * 60 + sec) * NS)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// Howard Hinnant's `days_from_civil`.
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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

    #[test]
    fn parses_utc_round_trip() {
        for ts in [
            "1970-01-01T00:00:00Z",
            "2000-02-29T12:34:56Z",
            "2026-11-14T03:00:00Z",
            "2099-12-31T23:59:59Z",
        ] {
            assert_eq!(format_utc(parse_utc(ts).unwrap()), ts);
        }
        assert_eq!(
            parse_utc("2026-11-14T03:00Z").unwrap(),
            parse_utc("2026-11-14T03:00:00+00:00").unwrap()
        );
        for bad in [
            "2026-11-14",
            "2026-11-14T03:00:00",
            "2026-02-30T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-11-14T24:00:00Z",
            "x",
        ] {
            assert!(parse_utc(bad).is_err(), "accepted {bad}");
        }
    }

    #[test]
    fn parses_time_of_day() {
        assert_eq!(parse_time_of_day("03:00").unwrap(), 3 * 3600 * NS);
        assert_eq!(
            parse_time_of_day("23:59:30").unwrap(),
            (23 * 3600 + 59 * 60 + 30) * NS
        );
        for bad in ["3:00", "24:00", "03:60", "03", "03:00:00:00"] {
            assert!(parse_time_of_day(bad).is_err(), "accepted {bad}");
        }
    }
}
