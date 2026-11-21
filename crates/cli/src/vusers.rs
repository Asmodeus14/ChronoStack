//! Simulated users on the virtual clock: a scenario's `users:` journeys.
//!
//! Users arrive and think in *virtual* time, counted from the scenario's start.
//! During each `advance` step the clock also stops wherever a user is due:
//! at each such stop the due users act (in parallel, up to `max_parallel`),
//! then the barriers in chronostack.yml must hold again so their side effects
//! settle, then time moves on. Due times are rounded up to `resolution`
//! (default 1m) so many users share a stop.
//!
//! The *issue* timeline (which user does which step at which virtual time)
//! depends only on the seed: due times are computed from the stop's virtual
//! time, never from how long requests took.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, anyhow, bail};
use chronostack_core::duration::{format_utc, parse_duration};
use chronostack_users::engine::{self, Arrival, Pause, Program, Stats, User};
use chronostack_users::model::{self, Plan};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::Resolved;
use crate::report::redact;
use crate::session::Session;

pub const DEFAULT_RESOLUTION_NS: i64 = 60_000_000_000;
const MIN_RESOLUTION_NS: i64 = 1_000_000_000;
/// Most user stops in one `advance` step.
pub const MAX_USER_STOPS: usize = 20_000;
/// Most timeline entries kept in a report.
const MAX_TIMELINE: usize = 20_000;

/// `users:` in a scenario: a journey file, or `{ file, resolution }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum UsersRef {
    File(String),
    Full(UsersFull),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsersFull {
    pub file: String,
    /// Users due within one `resolution` of each other act at the same stop.
    pub resolution: Option<String>,
}

impl UsersRef {
    pub fn file(&self) -> &str {
        match self {
            UsersRef::File(f) => f,
            UsersRef::Full(f) => &f.file,
        }
    }

    pub fn resolution_ns(&self) -> Result<i64> {
        let ns = match self {
            UsersRef::Full(UsersFull {
                resolution: Some(r),
                ..
            }) => parse_duration(r).map_err(|e| anyhow!("users.resolution: {e}"))?,
            _ => DEFAULT_RESOLUTION_NS,
        };
        if ns < MIN_RESOLUTION_NS {
            bail!("users.resolution must be at least 1s");
        }
        Ok(ns)
    }
}

/// The journey file of a scenario, resolved relative to the scenario's folder.
pub fn journey_path(scenario: &Path, r: &UsersRef) -> std::path::PathBuf {
    let p = Path::new(r.file());
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        scenario.parent().unwrap_or(Path::new(".")).join(p)
    }
}

pub fn load_plan(path: &Path) -> Result<Plan> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    model::parse(&text).map_err(|e| anyhow!("invalid {}: {e}", path.display()))
}

/// `(due virtual ns, cohort index, user)`: the order users act in.
type Key = (i64, usize, u32);
/// A user after acting, and where it paused.
type Acted = (Key, Box<User>, Pause);

/// A user waiting for its next action.
enum Slot {
    Arrive(Arrival),
    Resume(Box<User>),
}

pub struct VirtualUsers {
    plan: Plan,
    programs: BTreeMap<String, Program>,
    vars: BTreeMap<String, String>,
    pub seed: u64,
    run_id: String,
    pub resolution_ns: i64,
    start_ns: i64,
    /// `(due virtual ns, cohort index, user)` → what happens then. The key
    /// order is the issue order.
    pending: BTreeMap<Key, Slot>,
    stats: Stats,
    timeline: Vec<Value>,
    timeline_truncated: bool,
    pub actions: u64,
    pub stops: u64,
    real_ms: u128,
}

impl VirtualUsers {
    pub fn new(
        plan: Plan,
        targets: &BTreeMap<String, String>,
        seed: u64,
        run_id: &str,
        resolution_ns: i64,
        start_ns: i64,
    ) -> VirtualUsers {
        let programs = engine::programs(&plan);
        let mut pending = BTreeMap::new();
        for a in engine::arrivals(&plan, seed) {
            let due = round_up(start_ns, start_ns + a.offset_ns, resolution_ns);
            pending.insert((due, a.cohort_index, a.user), Slot::Arrive(a));
        }
        let mut stats = Stats::default();
        stats.users.total = pending.len() as u64;
        VirtualUsers {
            vars: engine::base_vars(targets, run_id),
            plan,
            programs,
            seed,
            run_id: run_id.to_string(),
            resolution_ns,
            start_ns,
            pending,
            stats,
            timeline: Vec::new(),
            timeline_truncated: false,
            actions: 0,
            stops: 0,
            real_ms: 0,
        }
    }

    /// When the next user is due, if any.
    pub fn next_due(&self) -> Option<i64> {
        self.pending.keys().next().map(|k| k.0)
    }

    /// Let every user due at or before `now_ns` act. Returns how many acted.
    pub fn run_due(&mut self, now_ns: i64) -> usize {
        let mut due = Vec::new();
        while let Some(entry) = self.pending.first_entry() {
            if entry.key().0 > now_ns {
                break;
            }
            let (k, slot) = entry.remove_entry();
            due.push((k, slot));
        }
        if due.is_empty() {
            return 0;
        }
        let started = Instant::now();
        self.stops += 1;
        self.actions += due.len() as u64;
        let stamp = format_utc(now_ns);
        let users: Vec<(Key, Box<User>)> = due
            .into_iter()
            .map(|(k, slot)| {
                let user = match slot {
                    Slot::Resume(u) => u,
                    Slot::Arrive(a) => Box::new(User::new(
                        &self.plan,
                        &self.programs,
                        a,
                        self.seed,
                        &self.vars,
                        Duration::from_secs(30),
                        true,
                    )),
                };
                (k, user)
            })
            .collect();
        // Bounded parallelism: a shared queue, `max_parallel` workers.
        let workers = self.plan.max_parallel.min(users.len()).max(1);
        let queue = std::sync::Mutex::new(users.into_iter());
        let results: Vec<(Stats, Vec<Acted>)> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..workers)
                .map(|_| {
                    let queue = &queue;
                    let stamp = &stamp;
                    s.spawn(move || {
                        let mut stats = Stats::default();
                        let mut out = Vec::new();
                        loop {
                            let next = queue.lock().expect("queue lock").next();
                            let Some((k, mut user)) = next else { break };
                            user.note(|| format!("@ {stamp}"));
                            let pause = user.resume(&mut stats, None);
                            out.push((k, user, pause));
                        }
                        (stats, out)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("user worker panicked"))
                .collect()
        });
        let mut done: Vec<Acted> = Vec::new();
        for (stats, out) in results {
            self.stats.merge(stats);
            done.extend(out);
        }
        let acted = done.len();
        // Reschedule in issue order, from the stop's virtual time.
        done.sort_by_key(|(k, _, _)| *k);
        for ((_, ci, u), user, pause) in done {
            match pause {
                Pause::Think(ns) => {
                    let due =
                        round_up(self.start_ns, now_ns.saturating_add(ns), self.resolution_ns)
                            .max(now_ns + self.resolution_ns);
                    self.pending.insert((due, ci, u), Slot::Resume(user));
                }
                Pause::Finished(o) => {
                    self.stats.record_outcome(o);
                    self.keep_trace(*user);
                }
            }
        }
        self.real_ms += started.elapsed().as_millis();
        acted
    }

    fn keep_trace(&mut self, user: User) {
        let (cohort, n) = (user.cohort.clone(), user.user);
        let Some(t) = user.into_trace() else { return };
        let mut at = String::new();
        for e in t.events {
            if let Some(stamp) = e.strip_prefix("@ ") {
                at = stamp.to_string();
                continue;
            }
            if self.timeline.len() >= MAX_TIMELINE {
                self.timeline_truncated = true;
                return;
            }
            self.timeline.push(json!({
                "virtual_time": at, "cohort": cohort, "user": n, "event": redact(&e),
            }));
        }
    }

    /// Wait for every barrier in chronostack.yml after users acted, so their
    /// work settles before time moves. `Err` is the first one that didn't
    /// hold: `(name, detail)`.
    pub fn settle(session: &Session, cfg: &Resolved) -> Result<(), (String, String)> {
        let compose = session.compose();
        for (i, b) in cfg.config.barriers.iter().enumerate() {
            let a = crate::engine::wait_barrier(session, &compose, b, cfg.barrier_timeouts_ns[i]);
            if !a.ok {
                return Err((a.name, a.detail.unwrap_or_default()));
            }
        }
        Ok(())
    }

    /// The users part of the report: per-step table, outcomes, timeline. Users
    /// still waiting count as not started (never arrived) or stopped
    /// (mid-journey when the scenario ended).
    pub fn finish(mut self) -> (engine::Report, Value) {
        for (_, slot) in std::mem::take(&mut self.pending) {
            match slot {
                Slot::Arrive(_) => self.stats.users.not_started += 1,
                Slot::Resume(mut u) => {
                    let o = u.stop();
                    self.stats.record_outcome(o);
                    self.keep_trace(*u);
                }
            }
        }
        let mut timeline = std::mem::take(&mut self.timeline);
        timeline.sort_by(|a, b| {
            let key = |v: &Value| {
                (
                    v["virtual_time"].as_str().unwrap_or("").to_string(),
                    v["cohort"].as_str().unwrap_or("").to_string(),
                    v["user"].as_u64().unwrap_or(0),
                )
            };
            key(a).cmp(&key(b))
        });
        let report = self.stats.report(
            &self.plan,
            self.seed,
            &self.run_id,
            self.real_ms,
            false,
            false,
        );
        let mut v = serde_json::to_value(&report).unwrap_or_default();
        for s in v["steps"].as_array_mut().into_iter().flatten() {
            if let Some(errs) = s["errors"].as_array_mut() {
                for e in errs.iter_mut() {
                    *e = Value::String(redact(e.as_str().unwrap_or("")));
                }
            }
        }
        v["resolution_s"] = json!(self.resolution_ns / 1_000_000_000);
        v["actions"] = json!(self.actions);
        v["stops"] = json!(self.stops);
        v["timeline"] = Value::Array(timeline);
        v["timeline_truncated"] = json!(self.timeline_truncated);
        (report, v)
    }
}

/// Round `t` up to the grid `start + k * step`.
fn round_up(start: i64, t: i64, step: i64) -> i64 {
    let off = t.saturating_sub(start).max(0);
    let k = (off + step - 1) / step;
    start.saturating_add(k.saturating_mul(step))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000_000_000;

    #[test]
    fn rounding_to_the_grid() {
        assert_eq!(round_up(1_000, 1_000, MIN), 1_000);
        assert_eq!(round_up(1_000, 1_001, MIN), 1_000 + MIN);
        assert_eq!(round_up(1_000, 1_000 + MIN, MIN), 1_000 + MIN);
        assert_eq!(round_up(1_000, 500, MIN), 1_000, "never before the start");
    }

    #[test]
    fn users_ref_forms() {
        let r: UsersRef = serde_norway::from_str("journeys.yml").unwrap();
        assert_eq!(
            (r.file(), r.resolution_ns().unwrap()),
            ("journeys.yml", DEFAULT_RESOLUTION_NS)
        );
        let r: UsersRef = serde_norway::from_str("{ file: j.yml, resolution: 5s }").unwrap();
        assert_eq!(
            (r.file(), r.resolution_ns().unwrap()),
            ("j.yml", 5_000_000_000)
        );
        let r: UsersRef = serde_norway::from_str("{ file: j.yml, resolution: 100ms }").unwrap();
        assert!(r.resolution_ns().is_err());
        assert_eq!(
            journey_path(
                Path::new("scenarios/a.yml"),
                &UsersRef::File("j.yml".into())
            ),
            Path::new("scenarios").join("j.yml")
        );
    }

    /// Thinks only (no HTTP): the issue timeline follows the seed and the grid.
    fn plan(text: &str) -> Plan {
        model::parse(text).unwrap()
    }

    #[test]
    fn users_act_at_their_virtual_times() {
        let p = plan(
            "version: 1\ncohorts: [{ name: c, users: 3, arrive: { over: 3h }, journey: j }]\n\
             journeys:\n  j:\n    - think: 2d\n    - think: 90s\n",
        );
        let start = 1_000 * MIN;
        let mut v = VirtualUsers::new(p, &BTreeMap::new(), 7, "r", MIN, start);
        // Arrivals at +0, +1h, +2h.
        assert_eq!(v.next_due(), Some(start));
        let mut stops = Vec::new();
        while let Some(t) = v.next_due() {
            v.run_due(t);
            stops.push((t - start) / MIN);
        }
        // Each user: arrive, +2d, then +90s rounded up to the 1-minute grid (+2m).
        let d2 = 2 * 24 * 60;
        assert_eq!(
            stops,
            [0, 60, 120, d2, d2 + 2, d2 + 60, d2 + 62, d2 + 120, d2 + 122]
        );
        let (r, j) = v.finish();
        assert_eq!(r.users.completed, 3);
        assert_eq!(j["actions"], 9);
        assert_eq!(j["stops"], 9);
        let first = &j["timeline"][0];
        assert_eq!(first["cohort"], "c");
        assert_eq!(first["user"], 1);
        assert!(first["event"].as_str().unwrap().contains("think"));
    }

    #[test]
    fn unfinished_users_are_reported_at_the_end() {
        let p = plan(
            "version: 1\ncohorts: [{ name: c, users: 2, arrive: { over: 10d }, journey: j }]\n\
             journeys:\n  j:\n    - think: 3d\n",
        );
        let mut v = VirtualUsers::new(p, &BTreeMap::new(), 1, "r", MIN, 0);
        v.run_due(0); // user 1 arrives and starts a 3-day think; user 2 arrives at +5d
        let (r, _) = v.finish();
        assert_eq!((r.users.stopped, r.users.not_started), (1, 1));
    }

    #[test]
    fn the_same_seed_gives_the_same_timeline() {
        let text = "version: 1\ncohorts: [{ name: c, users: 6, arrive: { over: 1d, model: poisson }, journey: j }]\n\
                    journeys:\n  j:\n    - think: { between: [1h, 9h] }\n    - branch: { p: 0.5 }\n    - think: { between: [1h, 2d] }\n";
        let run = |seed| {
            let mut v = VirtualUsers::new(plan(text), &BTreeMap::new(), seed, "r", MIN, 0);
            while let Some(t) = v.next_due() {
                v.run_due(t);
            }
            v.finish().1["timeline"].clone()
        };
        assert_eq!(run(3), run(3));
        assert_ne!(run(3), run(4));
    }
}
