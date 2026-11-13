//! Minimal cron-expression support, used to plan pg_cron jobs.
//!
//! Supports the 5-field Vixie syntax (`*`, `n`, `a-b`, lists, `*/n`, `a-b/n`)
//! and the `@hourly/@daily/@midnight/@weekly/@monthly/@yearly/@annually` macros,
//! evaluated in UTC. Anything else (pg_cron's `30 seconds` interval syntax,
//! `$` last-day-of-month, names like `MON`) is rejected so callers can fall back.
//!
//! Matching follows Vixie cron: if day-of-month or day-of-week is `*`, both
//! must match; otherwise either may match.

const MIN_NS: i64 = 60_000_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    minutes: [bool; 60],
    hours: [bool; 24],
    dom: [bool; 32],
    months: [bool; 13],
    dow: [bool; 7],
    dom_star: bool,
    dow_star: bool,
    /// Minute or hour field starts with `*` (Vixie's "wildcard" jobs, e.g. `*/5 * * * *`).
    /// pg_cron runs these once per clock jump rather than catching up every occurrence.
    pub wildcard: bool,
}

fn field<const N: usize>(spec: &str, lo: usize, hi: usize) -> Result<[bool; N], String> {
    let mut out = [false; N];
    for part in spec.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (
                r,
                s.parse::<usize>()
                    .map_err(|_| format!("bad step in `{part}`"))?,
            ),
            None => (part, 1),
        };
        if step == 0 {
            return Err(format!("zero step in `{part}`"));
        }
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            (
                a.parse().map_err(|_| format!("bad value `{a}`"))?,
                b.parse().map_err(|_| format!("bad value `{b}`"))?,
            )
        } else {
            let v: usize = range
                .parse()
                .map_err(|_| format!("unsupported value `{range}`"))?;
            (v, if part.contains('/') { hi } else { v })
        };
        if a < lo || b > hi || a > b {
            return Err(format!("`{part}` out of range {lo}-{hi}"));
        }
        let mut v = a;
        while v <= b {
            out[v] = true;
            v += step;
        }
    }
    Ok(out)
}

impl Cron {
    pub fn parse(expr: &str) -> Result<Cron, String> {
        let e = expr.trim();
        let e = match e {
            "@yearly" | "@annually" => "0 0 1 1 *",
            "@monthly" => "0 0 1 * *",
            "@weekly" => "0 0 * * 0",
            "@daily" | "@midnight" => "0 0 * * *",
            "@hourly" => "0 * * * *",
            other => other,
        };
        let f: Vec<&str> = e.split_whitespace().collect();
        if f.len() != 5 {
            return Err(format!("`{expr}` is not a 5-field cron expression"));
        }
        let mut dow: [bool; 8] = field(f[4], 0, 7)?;
        if dow[7] {
            dow[0] = true; // 7 = Sunday
        }
        Ok(Cron {
            minutes: field(f[0], 0, 59)?,
            hours: field(f[1], 0, 23)?,
            dom: field(f[2], 1, 31)?,
            months: field(f[3], 1, 12)?,
            dow: [dow[0], dow[1], dow[2], dow[3], dow[4], dow[5], dow[6]],
            dom_star: f[2].starts_with('*'),
            dow_star: f[4].starts_with('*'),
            wildcard: f[0].starts_with('*') || f[1].starts_with('*'),
        })
    }

    fn matches_minute(&self, unix_min: i64) -> bool {
        let secs = unix_min * 60;
        let days = secs.div_euclid(86_400);
        let tod = secs.rem_euclid(86_400);
        let (h, m) = ((tod / 3600) as usize, ((tod % 3600) / 60) as usize);
        if !self.minutes[m] || !self.hours[h] {
            return false;
        }
        let (_, month, day) = crate::duration::civil_from_days_pub(days);
        if !self.months[month as usize] {
            return false;
        }
        let weekday = (days + 4).rem_euclid(7) as usize; // 1970-01-01 was a Thursday
        let dom_ok = self.dom[day as usize];
        let dow_ok = self.dow[weekday];
        if self.dom_star || self.dow_star {
            dom_ok && dow_ok
        } else {
            dom_ok || dow_ok
        }
    }

    /// Minute starts in `(from_ns, to_ns]`, at most `limit` (error beyond).
    pub fn occurrences(&self, from_ns: i64, to_ns: i64, limit: usize) -> Result<Vec<i64>, String> {
        let mut out = Vec::new();
        let mut m = from_ns.div_euclid(MIN_NS) + 1;
        let last = to_ns.div_euclid(MIN_NS);
        while m <= last {
            if self.matches_minute(m) {
                out.push(m * MIN_NS);
                if out.len() > limit {
                    return Err(format!("more than {limit} occurrences"));
                }
            }
            m += 1;
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duration::{format_utc, parse_utc};

    fn occ(expr: &str, from: &str, to: &str) -> Vec<String> {
        Cron::parse(expr)
            .unwrap()
            .occurrences(parse_utc(from).unwrap(), parse_utc(to).unwrap(), 10_000)
            .unwrap()
            .into_iter()
            .map(format_utc)
            .collect()
    }

    #[test]
    fn daily_at_three() {
        let o = occ("0 3 * * *", "2026-11-15T02:30:00Z", "2026-11-18T02:30:00Z");
        assert_eq!(
            o,
            [
                "2026-11-15T03:00:00Z",
                "2026-11-16T03:00:00Z",
                "2026-11-17T03:00:00Z"
            ]
        );
        assert!(!Cron::parse("0 3 * * *").unwrap().wildcard);
    }

    #[test]
    fn macros_steps_lists_ranges() {
        assert_eq!(
            occ("@daily", "2026-11-15T00:00:00Z", "2026-11-17T00:00:00Z").len(),
            2
        );
        assert_eq!(
            occ(
                "*/15 9-10 * * *",
                "2026-11-15T00:00:00Z",
                "2026-11-16T00:00:00Z"
            )
            .len(),
            8
        );
        assert_eq!(
            occ(
                "0 9,17 * * *",
                "2026-11-15T00:00:00Z",
                "2026-11-16T00:00:00Z"
            ),
            ["2026-11-15T09:00:00Z", "2026-11-15T17:00:00Z"]
        );
        assert!(Cron::parse("*/5 * * * *").unwrap().wildcard);
        assert!(
            Cron::parse("0 * * * *").unwrap().wildcard,
            "hour field `*` is a wildcard job in Vixie cron"
        );
    }

    #[test]
    fn day_of_month_and_week() {
        // 2026-11-16 is a Monday.
        assert_eq!(
            occ("0 0 * * 1", "2026-11-14T00:00:00Z", "2026-11-23T00:00:00Z"),
            ["2026-11-16T00:00:00Z", "2026-11-23T00:00:00Z"]
        );
        assert_eq!(
            occ("0 0 1 * *", "2026-11-14T00:00:00Z", "2027-01-02T00:00:00Z"),
            ["2026-12-01T00:00:00Z", "2027-01-01T00:00:00Z"]
        );
        // dom and dow both restricted -> OR (Vixie): the 20th, or any Monday.
        let o = occ("0 0 20 * 1", "2026-11-14T00:00:00Z", "2026-11-24T00:00:00Z");
        assert_eq!(
            o,
            [
                "2026-11-16T00:00:00Z",
                "2026-11-20T00:00:00Z",
                "2026-11-23T00:00:00Z"
            ]
        );
        assert_eq!(
            occ("0 0 * * 7", "2026-11-14T00:00:00Z", "2026-11-16T00:00:00Z"),
            ["2026-11-15T00:00:00Z"],
            "7 = Sunday"
        );
    }

    #[test]
    fn rejects_unsupported() {
        for bad in [
            "30 seconds",
            "0 3 * *",
            "0 3 $ * *",
            "0 3 * * MON",
            "61 * * * *",
            "*/0 * * * *",
            "5-1 * * * *",
        ] {
            assert!(Cron::parse(bad).is_err(), "accepted `{bad}`");
        }
    }
}
