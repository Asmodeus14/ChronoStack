//! Running a validated journey file in real time.
//!
//! Users start at their arrival offsets and each runs its journey from start to
//! finish on one of `max_parallel` worker threads, so at most that many users
//! are active at once (a user that arrives while all workers are busy starts
//! late, and is counted). Every random choice (arrivals, `between` thinks,
//! branches) comes from the run seed, so a seed gives the same users making the
//! same choices.
//!
//! A step fails when its request can't be sent, an expectation doesn't hold, or
//! a value to `save` is missing; the user stops there. Think times run in real
//! time, capped by `max_think`. Target URLs must already be plain `http(s)://`
//! (the CLI resolves `service://` targets before calling [`run`]).

use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chronostack_core::duration::parse_duration;
use serde::Serialize;
use serde_json::Value;

use crate::arrival;
use crate::histogram::{Histogram, Summary};
use crate::model::{Plan, Step, Think, step_label};
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
    /// Stopped by `--duration` before finishing.
    pub stopped: u64,
    /// Never started: `--duration` ran out first.
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

/// One user to run.
struct Job {
    cohort_index: usize,
    user: u32,
    offset_ns: i64,
}

#[derive(Default)]
struct StepAcc {
    runs: u64,
    failed: u64,
    latency: Histogram,
    errors: Vec<String>,
}

#[derive(Default)]
struct Acc {
    steps: BTreeMap<(String, usize), StepAcc>,
    users: UserTotals,
    thinks_capped: u64,
    traces: Vec<UserTrace>,
}

impl Acc {
    fn merge(&mut self, o: Acc) {
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
}

/// How a user's journey ended.
enum Outcome {
    Done,
    Branch,
    Failed,
    Stopped,
}

/// Steps under a step, itself included (for numbering steps inside `repeat`).
fn subtree_len(step: &Step) -> usize {
    1 + step
        .repeat
        .as_ref()
        .map_or(0, |r| r.steps.iter().map(subtree_len).sum())
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

struct Ctx<'a> {
    plan: &'a Plan,
    journey: &'a str,
    agent: ureq::Agent,
    vars: BTreeMap<String, String>,
    rng: Rng,
    deadline: Option<Instant>,
    acc: &'a mut Acc,
    trace: Option<Vec<String>>,
}

impl Ctx<'_> {
    fn out_of_time(&self) -> bool {
        self.deadline.is_some_and(|d| Instant::now() >= d)
    }

    fn note(&mut self, e: impl FnOnce() -> String) {
        if let Some(t) = &mut self.trace {
            t.push(e());
        }
    }

    fn record(&mut self, id: usize, ok: bool, latency_us: Option<u64>, err: Option<String>) {
        let s = self
            .acc
            .steps
            .entry((self.journey.to_string(), id))
            .or_default();
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

    /// Run steps starting at step number `first`; `None` means go on.
    fn steps(&mut self, steps: &[Step], first: usize) -> Option<Outcome> {
        let mut id = first;
        for step in steps {
            if self.out_of_time() {
                return Some(Outcome::Stopped);
            }
            if let Some(o) = self.step(step, id) {
                return Some(o);
            }
            id += subtree_len(step);
        }
        None
    }

    fn step(&mut self, step: &Step, id: usize) -> Option<Outcome> {
        if let Some(h) = &step.http {
            let ok = self.http(step, h, id);
            return (!ok).then_some(Outcome::Failed);
        }
        if let Some(t) = &step.think {
            let want = match t {
                Think::Fixed(d) => dur_ns(d),
                Think::Between { between } => {
                    let (lo, hi) = (dur_ns(&between[0]), dur_ns(&between[1]));
                    self.rng.range(lo as u64, hi.max(lo) as u64) as i64
                }
            };
            let ns = want.min(self.plan.max_think_ns);
            if ns < want {
                self.acc.thinks_capped += 1;
            }
            self.note(|| format!("{id} think {ns}ns"));
            self.record(id, true, None, None);
            if !self.sleep(Duration::from_nanos(ns as u64)) {
                return Some(Outcome::Stopped);
            }
            return None;
        }
        if let Some(b) = &step.branch {
            let on = self.rng.chance(b.p);
            self.note(|| format!("{id} branch {}", if on { "on" } else { "off" }));
            self.record(id, true, None, None);
            return (!on).then_some(Outcome::Branch);
        }
        if let Some(r) = &step.repeat {
            self.record(id, true, None, None);
            for _ in 0..r.times {
                if let Some(o) = self.steps(&r.steps, id + 1) {
                    return Some(o);
                }
            }
        }
        None
    }

    /// Sleep, waking early at the deadline; `false` if the deadline came first.
    fn sleep(&self, d: Duration) -> bool {
        let end = Instant::now() + d;
        match self.deadline {
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

    fn http(&mut self, step: &Step, h: &crate::model::Http, id: usize) -> bool {
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
            Ok(Err(e)) => return self.fail(id, None, format!("{method} {url}: {e}")),
            Err(e) => return self.fail(id, None, format!("bad request {url}: {e}")),
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
                            id,
                            Some(latency),
                            format!("{ptr}: expected {want}, got {got}"),
                        );
                    }
                    None => {
                        return self.fail(id, Some(latency), format!("{ptr}: not in the response"));
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
                        id,
                        Some(latency),
                        format!("save {var}: {ptr} not in the response"),
                    );
                }
            }
        }
        self.record(id, true, Some(latency), None);
        true
    }

    fn fail(&mut self, id: usize, latency: Option<u64>, msg: String) -> bool {
        self.note(|| format!("{id} failed: {msg}"));
        self.record(id, false, latency, Some(msg));
        false
    }
}

/// Run a plan. `targets` are the resolved base URLs (`name` → `http(s)://…`).
pub fn run(plan: &Plan, targets: &BTreeMap<String, String>, opts: &Options) -> Report {
    let started = Instant::now();
    let deadline = opts.duration.map(|d| started + d);

    // Every user, in arrival order (ties broken by cohort, then user).
    let mut jobs = Vec::new();
    for (ci, c) in plan.cohorts.iter().enumerate() {
        let mut rng = Rng::derive(opts.seed, &[ci as u64, u64::MAX]);
        let offsets = arrival::offsets(c.model, c.users, c.arrive_ns, &mut rng);
        for (u, off) in offsets.into_iter().enumerate() {
            jobs.push(Job {
                cohort_index: ci,
                user: u as u32 + 1,
                offset_ns: off,
            });
        }
    }
    jobs.sort_by_key(|j| (j.offset_ns, j.cohort_index, j.user));
    let total = jobs.len() as u64;
    let workers = plan.max_parallel.min(jobs.len()).max(1);
    let queue = Arc::new(Mutex::new(VecDeque::from(jobs)));

    let base_vars: BTreeMap<String, String> = targets
        .iter()
        .map(|(k, v)| (k.clone(), v.trim_end_matches('/').to_string()))
        .chain([("run_id".to_string(), opts.run_id.clone())])
        .collect();

    let mut acc = std::thread::scope(|s| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let queue = Arc::clone(&queue);
                let base_vars = &base_vars;
                s.spawn(move || {
                    let mut acc = Acc::default();
                    loop {
                        let Some(job) = queue.lock().expect("queue lock").pop_front() else {
                            break;
                        };
                        let start_at = started + Duration::from_nanos(job.offset_ns as u64);
                        if deadline.is_some_and(|d| start_at >= d) {
                            acc.users.not_started += 1;
                            continue;
                        }
                        let now = Instant::now();
                        if start_at > now {
                            std::thread::sleep(start_at - now);
                        } else if now - start_at > Duration::from_millis(100) {
                            acc.users.started_late += 1;
                        }
                        run_user(plan, &job, base_vars, opts, deadline, &mut acc);
                    }
                    acc
                })
            })
            .collect();
        let mut all = Acc::default();
        for h in handles {
            all.merge(h.join().expect("worker panicked"));
        }
        all
    });
    acc.users.total = total;

    // One row per step of each journey, in file order, including steps no user reached.
    let mut steps = Vec::new();
    for (journey, step_list) in &plan.file.journeys {
        let mut id = 1;
        let mut flat = Vec::new();
        flatten(step_list, &mut id, &mut flat);
        for (index, name, kind) in flat {
            let a = acc.steps.remove(&(journey.clone(), index));
            let a = a.unwrap_or_default();
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
    let trace = opts.trace.then(|| {
        let mut t = acc.traces;
        t.sort_by(|a, b| (a.cohort.as_str(), a.user).cmp(&(b.cohort.as_str(), b.user)));
        t
    });
    Report {
        seed: opts.seed,
        run_id: opts.run_id.clone(),
        elapsed_ms: started.elapsed().as_millis(),
        stopped_by_duration: deadline.is_some_and(|d| Instant::now() >= d),
        thinks_capped: acc.thinks_capped,
        users: acc.users,
        steps,
        trace,
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

fn run_user(
    plan: &Plan,
    job: &Job,
    base_vars: &BTreeMap<String, String>,
    opts: &Options,
    deadline: Option<Instant>,
    acc: &mut Acc,
) {
    let cohort = &plan.cohorts[job.cohort_index];
    let mut vars = base_vars.clone();
    vars.insert("user".into(), job.user.to_string());
    vars.insert("cohort".into(), cohort.name.clone());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(opts.request_timeout))
        .http_status_as_error(false)
        .build()
        .into();
    let steps = &plan.file.journeys[&cohort.journey];
    let mut ctx = Ctx {
        plan,
        journey: &cohort.journey,
        agent,
        vars,
        rng: Rng::derive(opts.seed, &[job.cohort_index as u64, u64::from(job.user)]),
        deadline,
        acc,
        trace: opts.trace.then(Vec::new),
    };
    let outcome = ctx.steps(steps, 1).unwrap_or(Outcome::Done);
    let events = ctx.trace.take();
    let u = &mut acc.users;
    match outcome {
        Outcome::Done => u.completed += 1,
        Outcome::Branch => u.left_at_branch += 1,
        Outcome::Failed => u.failed += 1,
        Outcome::Stopped => u.stopped += 1,
    }
    if let Some(events) = events {
        acc.traces.push(UserTrace {
            cohort: cohort.name.clone(),
            user: job.user,
            events,
        });
    }
}
