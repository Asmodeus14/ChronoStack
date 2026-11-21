//! Running a validated journey file.
//!
//! Each journey is compiled once into a flat program (`repeat` becomes a start
//! and an end marker), and each user is a [`User`]: a position in that program
//! plus loop counters, its variables and its own random generator.
//! [`User::resume`] runs steps until the user thinks or is done, so the caller
//! decides how time passes:
//!
//! - [`run`] (real time, `chronostack users run`): workers sleep through each
//!   think, capped by `max_think`.
//! - on the virtual clock (scenarios with `users:`), the CLI schedules the next
//!   resume at the think's virtual end.
//!
//! Every random choice (arrivals, `between` thinks, branches) comes from the run
//! seed, so a seed gives the same users making the same choices. A step fails
//! when its request can't be sent, an expectation doesn't hold, or a value to
//! `save` is missing; the user stops there. Target URLs must already be plain
//! `http(s)://` (the CLI resolves `service://` targets first).

use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chronostack_core::duration::parse_duration;
use serde::Serialize;
use serde_json::Value;

use crate::arrival;
use crate::histogram::{Histogram, Summary};
use crate::model::{Branch, Http, Plan, Step, Think, step_label};
use crate::rng::Rng;
use crate::template::{substitute, substitute_json};

/// Largest response body read per request.
const MAX_BODY: u64 = 4 * 1024 * 1024;
/// Distinct error messages kept per step.
const MAX_ERRORS: usize = 5;

#[derive(Debug, Clone)]
pub struct Options {
    pub seed: u64,
    pub run_id: String,
    /// Stop after this much real time: no new users start, and running users
    /// stop before their next step.
    pub duration: Option<Duration>,
    pub request_timeout: Duration,
    /// Record every user's steps in the report (for tests and debugging).
    pub trace: bool,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct UserTotals {
    pub total: u64,
    pub completed: u64,
    /// Ended at a `branch` that didn't go on.
    pub left_at_branch: u64,
    pub failed: u64,
    /// Stopped by `--duration` (or the end of a scenario) before finishing.
    pub stopped: u64,
    /// Never started: the run ended first.
    pub not_started: u64,
    /// Started after their arrival time because all workers were busy.
    pub started_late: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepReport {
    pub journey: String,
    /// Position in the journey, counting steps inside `repeat` (1-based).
    pub index: usize,
    pub name: String,
    pub kind: &'static str,
    pub runs: u64,
    pub failed: u64,
    /// Request latency in microseconds (`http` steps).
    pub latency_us: Option<Summary>,
    pub errors: Vec<String>,
}

/// What one user did, in order (with `trace`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UserTrace {
    pub cohort: String,
    pub user: u32,
    pub events: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub seed: u64,
    pub run_id: String,
    pub elapsed_ms: u128,
    pub stopped_by_duration: bool,
    /// Thinks shortened to `max_think`.
    pub thinks_capped: u64,
    pub users: UserTotals,
    pub steps: Vec<StepReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<Vec<UserTrace>>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.users.failed == 0 && self.steps.iter().all(|s| s.failed == 0)
    }
}

/// How a user's journey ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Done,
    Branch,
    Failed,
    Stopped,
}

/// Where [`User::resume`] stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pause {
    /// The user thinks for this long (ns, as chosen; the caller caps it if it
    /// runs in real time), then resumes.
    Think(i64),
    Finished(Outcome),
}

/// When one user of a cohort starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Arrival {
    pub cohort_index: usize,
    /// 1-based within the cohort (`${user}`).
    pub user: u32,
    pub offset_ns: i64,
}

/// Every user of a plan, in arrival order (ties broken by cohort, then user).
pub fn arrivals(plan: &Plan, seed: u64) -> Vec<Arrival> {
    let mut out = Vec::new();
    for (ci, c) in plan.cohorts.iter().enumerate() {
        let mut rng = Rng::derive(seed, &[ci as u64, u64::MAX]);
        let offsets = arrival::offsets(c.model, c.users, c.arrive_ns, &mut rng);
        for (u, off) in offsets.into_iter().enumerate() {
            out.push(Arrival {
                cohort_index: ci,
                user: u as u32 + 1,
                offset_ns: off,
            });
        }
    }
    out.sort_by_key(|a| (a.offset_ns, a.cohort_index, a.user));
    out
}

/// Variables every user starts with: the targets (without a trailing `/`) and
/// `run_id`.
pub fn base_vars(targets: &BTreeMap<String, String>, run_id: &str) -> BTreeMap<String, String> {
    targets
        .iter()
        .map(|(k, v)| (k.clone(), v.trim_end_matches('/').to_string()))
        .chain([("run_id".to_string(), run_id.to_string())])
        .collect()
}

/// One instruction of a compiled journey.
#[derive(Debug, Clone)]
enum Op {
    Http {
        id: usize,
        step: Box<Step>,
    },
    Think {
        id: usize,
        think: Think,
    },
    Branch {
        id: usize,
        branch: Branch,
    },
    /// Enter a `repeat`: run the following ops `times` times, up to its `End`.
    Repeat {
        id: usize,
        times: u32,
    },
    End,
}

/// A journey as a flat program. Step ids number steps in file order, counting
/// steps inside `repeat` (1-based), as in the report.
#[derive(Debug, Clone)]
pub struct Program {
    ops: Vec<Op>,
}

impl Program {
    pub fn compile(steps: &[Step]) -> Program {
        let mut ops = Vec::new();
        let mut id = 1;
        compile_into(steps, &mut id, &mut ops);
        Program { ops }
    }
}

fn compile_into(steps: &[Step], id: &mut usize, ops: &mut Vec<Op>) {
    for s in steps {
        let this = *id;
        *id += 1;
        if s.http.is_some() {
            ops.push(Op::Http {
                id: this,
                step: Box::new(s.clone()),
            });
        } else if let Some(t) = &s.think {
            ops.push(Op::Think {
                id: this,
                think: t.clone(),
            });
        } else if let Some(b) = &s.branch {
            ops.push(Op::Branch {
                id: this,
                branch: b.clone(),
            });
        } else if let Some(r) = &s.repeat {
            ops.push(Op::Repeat {
                id: this,
                times: r.times,
            });
            compile_into(&r.steps, id, ops);
            ops.push(Op::End);
        }
    }
}

/// The compiled program of every journey.
pub fn programs(plan: &Plan) -> BTreeMap<String, Program> {
    plan.file
        .journeys
        .iter()
        .map(|(name, steps)| (name.clone(), Program::compile(steps)))
        .collect()
}

#[derive(Default)]
struct StepAcc {
    runs: u64,
    failed: u64,
    latency: Histogram,
    errors: Vec<String>,
}

/// Per-step counts and user outcomes, collected by one or more workers and
/// merged.
#[derive(Default)]
pub struct Stats {
    steps: BTreeMap<(String, usize), StepAcc>,
    pub users: UserTotals,
    pub thinks_capped: u64,
    traces: Vec<UserTrace>,
}

impl Stats {
    pub fn merge(&mut self, o: Stats) {
        for (k, s) in o.steps {
            let e = self.steps.entry(k).or_default();
            e.runs += s.runs;
            e.failed += s.failed;
            e.latency.merge(&s.latency);
            for m in s.errors {
                if e.errors.len() < MAX_ERRORS && !e.errors.contains(&m) {
                    e.errors.push(m);
                }
            }
        }
        let (u, v) = (&mut self.users, o.users);
        u.total += v.total;
        u.completed += v.completed;
        u.left_at_branch += v.left_at_branch;
        u.failed += v.failed;
        u.stopped += v.stopped;
        u.not_started += v.not_started;
        u.started_late += v.started_late;
        self.thinks_capped += o.thinks_capped;
        self.traces.extend(o.traces);
    }

    pub fn record_outcome(&mut self, o: Outcome) {
        let u = &mut self.users;
        match o {
            Outcome::Done => u.completed += 1,
            Outcome::Branch => u.left_at_branch += 1,
            Outcome::Failed => u.failed += 1,
            Outcome::Stopped => u.stopped += 1,
        }
    }

    pub fn add_trace(&mut self, t: UserTrace) {
        self.traces.push(t);
    }

    fn record(
        &mut self,
        journey: &str,
        id: usize,
        ok: bool,
        latency_us: Option<u64>,
        err: Option<String>,
    ) {
        let s = self.steps.entry((journey.to_string(), id)).or_default();
        s.runs += 1;
        if !ok {
            s.failed += 1;
        }
        if let Some(l) = latency_us {
            s.latency.record(l);
        }
        if let Some(m) = err {
            if s.errors.len() < MAX_ERRORS && !s.errors.contains(&m) {
                s.errors.push(m);
            }
        }
    }

    /// One row per step of each journey, in file order, including steps no user
    /// reached; traces sorted by `(cohort, user)`.
    pub fn report(
        mut self,
        plan: &Plan,
        seed: u64,
        run_id: &str,
        elapsed_ms: u128,
        stopped: bool,
        trace: bool,
    ) -> Report {
        let mut steps = Vec::new();
        for (journey, step_list) in &plan.file.journeys {
            let mut id = 1;
            let mut flat = Vec::new();
            flatten(step_list, &mut id, &mut flat);
            for (index, name, kind) in flat {
                let a = self
                    .steps
                    .remove(&(journey.clone(), index))
                    .unwrap_or_default();
                steps.push(StepReport {
                    journey: journey.clone(),
                    index,
                    name,
                    kind,
                    runs: a.runs,
                    failed: a.failed,
                    latency_us: a.latency.summary(),
                    errors: a.errors,
                });
            }
        }
        let trace = trace.then(|| {
            let mut t = self.traces;
            t.sort_by(|a, b| (a.cohort.as_str(), a.user).cmp(&(b.cohort.as_str(), b.user)));
            t
        });
        Report {
            seed,
            run_id: run_id.to_string(),
            elapsed_ms,
            stopped_by_duration: stopped,
            thinks_capped: self.thinks_capped,
            users: self.users,
            steps,
            trace,
        }
    }
}

fn flatten(steps: &[Step], id: &mut usize, out: &mut Vec<(usize, String, &'static str)>) {
    for s in steps {
        let kind = if s.http.is_some() {
            "http"
        } else if s.think.is_some() {
            "think"
        } else if s.branch.is_some() {
            "branch"
        } else {
            "repeat"
        };
        out.push((*id, step_label(s), kind));
        *id += 1;
        if let Some(r) = &s.repeat {
            flatten(&r.steps, id, out);
        }
    }
}

fn dur_ns(v: &str) -> i64 {
    parse_duration(v).unwrap_or(0).max(0)
}

/// A JSON value as a template variable: strings without quotes.
fn as_var(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// One simulated user.
pub struct User {
    pub cohort: String,
    pub cohort_index: usize,
    pub user: u32,
    journey: String,
    program: Program,
    pc: usize,
    /// `(op index of the Repeat, iterations left)`.
    loops: Vec<(usize, u32)>,
    vars: BTreeMap<String, String>,
    rng: Rng,
    agent: ureq::Agent,
    trace: Option<Vec<String>>,
    finished: Option<Outcome>,
}

impl User {
    pub fn new(
        plan: &Plan,
        programs: &BTreeMap<String, Program>,
        arrival: Arrival,
        seed: u64,
        base_vars: &BTreeMap<String, String>,
        request_timeout: Duration,
        trace: bool,
    ) -> User {
        let cohort = &plan.cohorts[arrival.cohort_index];
        let mut vars = base_vars.clone();
        vars.insert("user".into(), arrival.user.to_string());
        vars.insert("cohort".into(), cohort.name.clone());
        User {
            cohort: cohort.name.clone(),
            cohort_index: arrival.cohort_index,
            user: arrival.user,
            journey: cohort.journey.clone(),
            program: programs[&cohort.journey].clone(),
            pc: 0,
            loops: Vec::new(),
            vars,
            rng: Rng::derive(
                seed,
                &[arrival.cohort_index as u64, u64::from(arrival.user)],
            ),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(request_timeout))
                .http_status_as_error(false)
                .build()
                .into(),
            trace: trace.then(Vec::new),
            finished: None,
        }
    }

    pub fn finished(&self) -> Option<Outcome> {
        self.finished
    }

    /// Add an event to the user's trace (when tracing).
    pub fn note(&mut self, e: impl FnOnce() -> String) {
        if let Some(t) = &mut self.trace {
            t.push(e());
        }
    }

    /// Stop a user that hasn't finished (`--duration`, end of a scenario).
    pub fn stop(&mut self) -> Outcome {
        *self.finished.get_or_insert(Outcome::Stopped)
    }

    pub fn into_trace(self) -> Option<UserTrace> {
        let (cohort, user) = (self.cohort, self.user);
        self.trace.map(|events| UserTrace {
            cohort,
            user,
            events,
        })
    }

    /// Run steps until the user thinks or finishes. `deadline` (real time) is
    /// checked before each step.
    pub fn resume(&mut self, stats: &mut Stats, deadline: Option<Instant>) -> Pause {
        if let Some(o) = self.finished {
            return Pause::Finished(o);
        }
        loop {
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return self.finish(Outcome::Stopped);
            }
            let Some(op) = self.program.ops.get(self.pc).cloned() else {
                return self.finish(Outcome::Done);
            };
            self.pc += 1;
            match op {
                Op::Http { id, step } => {
                    let h = step.http.as_ref().expect("http op");
                    if !self.http(&step, h, id, stats) {
                        return self.finish(Outcome::Failed);
                    }
                }
                Op::Think { id, think } => {
                    let ns = match think {
                        Think::Fixed(d) => dur_ns(&d),
                        Think::Between { between } => {
                            let (lo, hi) = (dur_ns(&between[0]), dur_ns(&between[1]));
                            self.rng.range(lo as u64, hi.max(lo) as u64) as i64
                        }
                    };
                    self.note(|| format!("{id} think {ns}ns"));
                    stats.record(&self.journey, id, true, None, None);
                    return Pause::Think(ns);
                }
                Op::Branch { id, branch } => {
                    let on = self.rng.chance(branch.p);
                    self.note(|| format!("{id} branch {}", if on { "on" } else { "off" }));
                    stats.record(&self.journey, id, true, None, None);
                    if !on {
                        return self.finish(Outcome::Branch);
                    }
                }
                Op::Repeat { id, times } => {
                    stats.record(&self.journey, id, true, None, None);
                    self.loops.push((self.pc - 1, times));
                }
                Op::End => {
                    let top = self.loops.last_mut().expect("End inside a repeat");
                    top.1 -= 1;
                    if top.1 > 0 {
                        self.pc = top.0 + 1;
                    } else {
                        self.loops.pop();
                    }
                }
            }
        }
    }

    fn finish(&mut self, o: Outcome) -> Pause {
        self.finished = Some(o);
        Pause::Finished(o)
    }

    fn http(&mut self, step: &Step, h: &Http, id: usize, stats: &mut Stats) -> bool {
        let method = h.method.to_ascii_uppercase();
        let url = substitute(&h.url, &self.vars);
        let mut builder = ureq::http::Request::builder()
            .method(method.as_str())
            .uri(&url);
        for (k, v) in &h.headers {
            builder = builder.header(substitute(k, &self.vars), substitute(v, &self.vars));
        }
        let body: Option<Vec<u8>> = match (&h.json, &h.body) {
            (Some(j), _) => {
                let has_ct = h
                    .headers
                    .keys()
                    .any(|k| k.eq_ignore_ascii_case("content-type"));
                if !has_ct {
                    builder = builder.header("Content-Type", "application/json");
                }
                Some(substitute_json(j, &self.vars).to_string().into_bytes())
            }
            (None, Some(b)) => Some(substitute(b, &self.vars).into_bytes()),
            (None, None) => None,
        };
        let no_body = matches!(method.as_str(), "GET" | "HEAD" | "DELETE" | "OPTIONS");
        let started = Instant::now();
        let sent = match (&body, no_body) {
            (Some(b), _) => builder.body(b.as_slice()).map(|r| self.agent.run(r)),
            (None, true) => builder.body(()).map(|r| self.agent.run(r)),
            (None, false) => builder.body(&[][..]).map(|r| self.agent.run(r)),
        };
        let mut resp = match sent {
            Ok(Ok(r)) => r,
            Ok(Err(e)) => return self.fail(stats, id, None, format!("{method} {url}: {e}")),
            Err(e) => return self.fail(stats, id, None, format!("bad request {url}: {e}")),
        };
        let mut raw = Vec::new();
        let _ = resp
            .body_mut()
            .as_reader()
            .take(MAX_BODY)
            .read_to_end(&mut raw);
        let latency = started.elapsed().as_micros() as u64;
        let status = resp.status().as_u16();
        self.note(|| format!("{id} {method} {url} -> {status}"));

        let expect = step.expect.as_ref();
        let status_ok = match expect.and_then(|e| e.status.as_ref()) {
            Some(s) => s.accepts(status),
            None => (200..300).contains(&status),
        };
        if !status_ok {
            return self.fail(
                stats,
                id,
                Some(latency),
                format!("{method} {url}: unexpected HTTP {status}"),
            );
        }
        let wants_json = expect.and_then(|e| e.json.as_ref()).is_some() || step.save.is_some();
        let json: Value = if wants_json {
            match serde_json::from_slice(&raw) {
                Ok(v) => v,
                Err(e) => {
                    return self.fail(
                        stats,
                        id,
                        Some(latency),
                        format!("{method} {url}: response is not JSON ({e})"),
                    );
                }
            }
        } else {
            Value::Null
        };
        if let Some(checks) = expect.and_then(|e| e.json.as_ref()) {
            for (ptr, want) in checks {
                let want = substitute_json(want, &self.vars);
                match json.pointer(ptr) {
                    Some(got) if *got == want => {}
                    Some(got) => {
                        return self.fail(
                            stats,
                            id,
                            Some(latency),
                            format!("{ptr}: expected {want}, got {got}"),
                        );
                    }
                    None => {
                        return self.fail(
                            stats,
                            id,
                            Some(latency),
                            format!("{ptr}: not in the response"),
                        );
                    }
                }
            }
        }
        for (var, ptr) in step.save.iter().flatten() {
            match json.pointer(ptr) {
                Some(v) => {
                    self.vars.insert(var.clone(), as_var(v));
                }
                None => {
                    return self.fail(
                        stats,
                        id,
                        Some(latency),
                        format!("save {var}: {ptr} not in the response"),
                    );
                }
            }
        }
        stats.record(&self.journey, id, true, Some(latency), None);
        true
    }

    fn fail(&mut self, stats: &mut Stats, id: usize, latency: Option<u64>, msg: String) -> bool {
        self.note(|| format!("{id} failed: {msg}"));
        stats.record(&self.journey, id, false, latency, Some(msg));
        false
    }
}

/// Sleep, waking early at the deadline; `false` if the deadline came first.
fn sleep_until(d: Duration, deadline: Option<Instant>) -> bool {
    let end = Instant::now() + d;
    match deadline {
        Some(dl) if dl < end => {
            std::thread::sleep(dl.saturating_duration_since(Instant::now()));
            false
        }
        _ => {
            std::thread::sleep(end.saturating_duration_since(Instant::now()));
            true
        }
    }
}

/// Run a plan in real time (`chronostack users run`). `targets` are the
/// resolved base URLs (`name` → `http(s)://…`).
pub fn run(plan: &Plan, targets: &BTreeMap<String, String>, opts: &Options) -> Report {
    let started = Instant::now();
    let deadline = opts.duration.map(|d| started + d);
    let jobs = arrivals(plan, opts.seed);
    let total = jobs.len() as u64;
    let workers = plan.max_parallel.min(jobs.len()).max(1);
    let queue = Arc::new(Mutex::new(VecDeque::from(jobs)));
    let vars = base_vars(targets, &opts.run_id);
    let progs = programs(plan);

    let mut stats = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let queue = Arc::clone(&queue);
                let (vars, progs) = (&vars, &progs);
                s.spawn(move || {
                    let mut stats = Stats::default();
                    loop {
                        let Some(job) = queue.lock().expect("queue lock").pop_front() else {
                            break;
                        };
                        let start_at = started + Duration::from_nanos(job.offset_ns as u64);
                        if deadline.is_some_and(|d| start_at >= d) {
                            stats.users.not_started += 1;
                            continue;
                        }
                        let now = Instant::now();
                        if start_at > now {
                            std::thread::sleep(start_at - now);
                        } else if now - start_at > Duration::from_millis(100) {
                            stats.users.started_late += 1;
                        }
                        let mut user = User::new(
                            plan,
                            progs,
                            job,
                            opts.seed,
                            vars,
                            opts.request_timeout,
                            opts.trace,
                        );
                        let outcome = loop {
                            match user.resume(&mut stats, deadline) {
                                Pause::Finished(o) => break o,
                                Pause::Think(ns) => {
                                    let capped = ns.min(plan.max_think_ns);
                                    if capped < ns {
                                        stats.thinks_capped += 1;
                                    }
                                    if !sleep_until(Duration::from_nanos(capped as u64), deadline) {
                                        break user.stop();
                                    }
                                }
                            }
                        };
                        stats.record_outcome(outcome);
                        if let Some(t) = user.into_trace() {
                            stats.add_trace(t);
                        }
                    }
                    stats
                })
            })
            .collect();
        let mut all = Stats::default();
        for h in handles {
            all.merge(h.join().expect("worker panicked"));
        }
        all
    });
    stats.users.total = total;
    let stopped = deadline.is_some_and(|d| Instant::now() >= d);
    stats.report(
        plan,
        opts.seed,
        &opts.run_id,
        started.elapsed().as_millis(),
        stopped,
        opts.trace,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::parse;

    fn plan(steps: &str) -> Plan {
        parse(&format!(
            "version: 1\ncohorts: [{{ name: c, users: 1, journey: j }}]\njourneys:\n  j:\n{steps}"
        ))
        .unwrap()
    }

    /// Run a user with no HTTP steps to completion, collecting the thinks.
    fn thinks(p: &Plan, seed: u64) -> (Vec<i64>, Outcome) {
        let progs = programs(p);
        let arrival = arrivals(p, seed)[0];
        let mut u = User::new(
            p,
            &progs,
            arrival,
            seed,
            &BTreeMap::new(),
            Duration::from_secs(1),
            false,
        );
        let mut stats = Stats::default();
        let mut out = Vec::new();
        loop {
            match u.resume(&mut stats, None) {
                Pause::Think(ns) => out.push(ns),
                Pause::Finished(o) => return (out, o),
            }
        }
    }

    #[test]
    fn repeat_runs_its_steps_the_given_number_of_times() {
        let p = plan(
            "    - think: 1s\n    - repeat:\n        times: 3\n        steps:\n          - think: 2s\n          - repeat: { times: 2, steps: [{ think: 3s }] }\n    - think: 4s\n",
        );
        let (t, o) = thinks(&p, 1);
        let s = 1_000_000_000;
        assert_eq!(
            t,
            [
                s,
                2 * s,
                3 * s,
                3 * s,
                2 * s,
                3 * s,
                3 * s,
                2 * s,
                3 * s,
                3 * s,
                4 * s
            ]
        );
        assert_eq!(o, Outcome::Done);
    }

    #[test]
    fn thinks_are_seeded_and_resume_after_each() {
        let p = plan("    - think: { between: [1s, 9s] }\n    - think: { between: [1s, 9s] }\n");
        let (a, _) = thinks(&p, 42);
        assert_eq!(a, thinks(&p, 42).0);
        assert_ne!(a, thinks(&p, 43).0);
        assert!(
            a.iter()
                .all(|t| (1_000_000_000..=9_000_000_000).contains(t))
        );
    }

    #[test]
    fn a_closed_branch_finishes_the_user() {
        let p = plan("    - branch: { p: 0.0 }\n    - think: 1s\n");
        assert_eq!(thinks(&p, 1), (vec![], Outcome::Branch));
        let p = plan("    - branch: { p: 1.0 }\n    - think: 1s\n");
        assert_eq!(thinks(&p, 1), (vec![1_000_000_000], Outcome::Done));
    }

    #[test]
    fn a_finished_user_stays_finished() {
        let p = plan("    - think: 1s\n");
        let progs = programs(&p);
        let a = arrivals(&p, 1)[0];
        let mut u = User::new(
            &p,
            &progs,
            a,
            1,
            &BTreeMap::new(),
            Duration::from_secs(1),
            false,
        );
        let mut stats = Stats::default();
        assert_eq!(u.resume(&mut stats, None), Pause::Think(1_000_000_000));
        assert_eq!(u.stop(), Outcome::Stopped);
        assert_eq!(
            u.resume(&mut stats, None),
            Pause::Finished(Outcome::Stopped)
        );
        assert_eq!(u.finished(), Some(Outcome::Stopped));
    }
}
