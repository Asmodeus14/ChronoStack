//! Advance planning: where the engine must stop the virtual clock.
//!
//! Given an advance from `from` to `to`, the engine stops at:
//!
//! - every `--step` end (and always at `to`);
//! - every occurrence of a scheduled trigger in `(from, to]`;
//! - extra sub-steps so no single jump exceeds `max_jump`. pg_cron treats a
//!   jump > 3 h as a clock correction and skips runs (verified from source and
//!   in M0), so `max_jump` defaults to 1 h when pg_cron is present.
//!
//! Pure functions: no I/O, fully unit-tested.

/// A virtual-time recurrence: occurrences at `k * every_ns + at_ns` (Unix ns, UTC).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Every {
    pub every_ns: i64,
    /// Offset inside the period, e.g. 3 h for "every day at 03:00 UTC". `0 <= at_ns < every_ns`.
    pub at_ns: i64,
}

impl Every {
    pub fn new(every_ns: i64, at_ns: i64) -> Result<Every, String> {
        if every_ns <= 0 {
            return Err("`every` must be > 0".into());
        }
        if !(0..every_ns).contains(&at_ns) {
            return Err(
                "`at` must fall inside one `every` period (e.g. at 03:00 needs every >= 1d)".into(),
            );
        }
        Ok(Every { every_ns, at_ns })
    }

    /// First occurrence strictly after `t`.
    pub fn next_after(&self, t: i64) -> i64 {
        let k = (t - self.at_ns).div_euclid(self.every_ns) + 1;
        k * self.every_ns + self.at_ns
    }
}

/// One stop of the virtual clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Boundary {
    pub at_ns: i64,
    /// Indices of scheduled triggers due here.
    pub scheduled: Vec<usize>,
    /// End of a user `--step` (or the final target).
    pub step_end: bool,
    /// Inserted only to respect `max_jump`.
    pub sub_step: bool,
}

/// Hard cap on stops per advance, to refuse plans that would take hours.
pub const MAX_BOUNDARIES: usize = 20_000;

/// Plan an advance. `schedules` are `(trigger index, recurrence)`.
pub fn plan(
    from: i64,
    to: i64,
    step: Option<i64>,
    max_jump: Option<i64>,
    schedules: &[(usize, Every)],
) -> Result<Vec<Boundary>, String> {
    if to <= from {
        return Err(
            "time only moves forward: target must be after the current virtual time".into(),
        );
    }
    let mut points: std::collections::BTreeMap<i64, Boundary> = Default::default();
    let mut add = |at: i64, f: &dyn Fn(&mut Boundary)| -> Result<(), String> {
        let b = points.entry(at).or_insert_with(|| Boundary {
            at_ns: at,
            scheduled: vec![],
            step_end: false,
            sub_step: false,
        });
        f(b);
        Ok(())
    };

    if let Some(step) = step {
        if step <= 0 {
            return Err("--step must be > 0".into());
        }
        if (to - from) / step > MAX_BOUNDARIES as i64 {
            return Err(format!(
                "--step would create more than {MAX_BOUNDARIES} steps"
            ));
        }
        let mut t = from + step;
        while t < to {
            add(t, &|b| b.step_end = true)?;
            t += step;
        }
    }
    add(to, &|b| b.step_end = true)?;

    for (idx, every) in schedules {
        let mut t = every.next_after(from);
        let mut n = 0usize;
        while t <= to {
            n += 1;
            if n > MAX_BOUNDARIES {
                return Err(format!(
                    "trigger #{idx} would run more than {MAX_BOUNDARIES} times in this advance"
                ));
            }
            let i = *idx;
            add(t, &move |b| b.scheduled.push(i))?;
            t += every.every_ns;
        }
    }

    let mut out: Vec<Boundary> = Vec::with_capacity(points.len());
    let mut prev = from;
    for (_, b) in points {
        if let Some(mj) = max_jump {
            if mj <= 0 {
                return Err("max_jump must be > 0".into());
            }
            while b.at_ns - prev > mj {
                prev += mj;
                out.push(Boundary {
                    at_ns: prev,
                    scheduled: vec![],
                    step_end: false,
                    sub_step: true,
                });
                if out.len() > MAX_BOUNDARIES {
                    return Err(format!(
                        "max_jump would create more than {MAX_BOUNDARIES} stops; raise max_jump or advance less at once"
                    ));
                }
            }
        }
        prev = b.at_ns;
        out.push(b);
        if out.len() > MAX_BOUNDARIES {
            return Err(format!("plan exceeds {MAX_BOUNDARIES} stops"));
        }
    }
    for b in &mut out {
        b.scheduled.sort_unstable();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3_600_000_000_000;
    const D: i64 = 24 * H;
    // 2026-11-13T00:00:00Z
    const DAY0: i64 = 1_794_528_000 * 1_000_000_000;

    #[test]
    fn next_occurrence() {
        let daily3 = Every::new(D, 3 * H).unwrap();
        assert_eq!(daily3.next_after(DAY0), DAY0 + 3 * H);
        assert_eq!(
            daily3.next_after(DAY0 + 3 * H),
            DAY0 + D + 3 * H,
            "strictly after"
        );
        assert_eq!(daily3.next_after(DAY0 + 5 * H), DAY0 + D + 3 * H);
        assert!(Every::new(H, 3 * H).is_err());
        assert!(Every::new(0, 0).is_err());
    }

    #[test]
    fn single_jump_without_options() {
        let p = plan(DAY0, DAY0 + 14 * D, None, None, &[]).unwrap();
        assert_eq!(p.len(), 1);
        assert!(p[0].step_end && !p[0].sub_step);
    }

    #[test]
    fn daily_trigger_runs_once_per_day_even_without_steps() {
        let s = [(0, Every::new(D, 3 * H).unwrap())];
        let p = plan(DAY0 + H, DAY0 + H + 14 * D, None, None, &s).unwrap();
        let runs: Vec<_> = p
            .iter()
            .filter(|b| b.scheduled == vec![0])
            .map(|b| b.at_ns)
            .collect();
        assert_eq!(runs.len(), 14);
        assert_eq!(runs[0], DAY0 + 3 * H);
        assert_eq!(runs[13], DAY0 + 13 * D + 3 * H);
        assert!(p.last().unwrap().step_end);
    }

    #[test]
    fn steps_and_triggers_merge() {
        let s = [(0, Every::new(D, 0).unwrap())];
        // from midnight: step ends and daily midnight occurrences coincide.
        let p = plan(DAY0, DAY0 + 3 * D, Some(D), None, &s).unwrap();
        assert_eq!(p.len(), 3);
        assert!(p.iter().all(|b| b.step_end && b.scheduled == vec![0]));
    }

    #[test]
    fn max_jump_inserts_sub_steps() {
        let p = plan(DAY0, DAY0 + 3 * D, None, Some(H), &[]).unwrap();
        assert_eq!(p.len(), 72);
        assert_eq!(p.iter().filter(|b| b.sub_step).count(), 71);
        assert!(p.windows(2).all(|w| w[1].at_ns - w[0].at_ns <= H));
    }

    #[test]
    fn max_jump_respected_around_triggers() {
        let s = [(0, Every::new(D, 3 * H + 30 * 60_000_000_000).unwrap())];
        let p = plan(DAY0, DAY0 + D, None, Some(H), &s).unwrap();
        let mut prev = DAY0;
        for b in &p {
            assert!(b.at_ns - prev <= H);
            prev = b.at_ns;
        }
        assert_eq!(p.iter().filter(|b| !b.scheduled.is_empty()).count(), 1);
    }

    #[test]
    fn rejects_backwards_and_huge_plans() {
        assert!(plan(DAY0, DAY0, None, None, &[]).is_err());
        assert!(plan(DAY0, DAY0 + 365 * D, None, Some(60_000_000_000), &[]).is_err());
        assert!(plan(DAY0, DAY0 + D, Some(0), None, &[]).is_err());
    }
}
