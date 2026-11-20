//! The journey file format and its validation.
//!
//! ```yaml
//! version: 1
//! seed: 42
//! targets:
//!   api: service://app:3000
//! cohorts:
//!   - name: trialists
//!     users: 50
//!     arrive: { over: 10m, model: uniform }
//!     journey: trial_to_paid
//! journeys:
//!   trial_to_paid:
//!     - name: sign up
//!       http: { method: POST, url: "${api}/signup", json: { email: "u${user}@test" } }
//!       expect: { status: 201 }
//!       save: { token: /token }
//!     - think: { between: [2s, 5s] }
//!     - branch: { p: 0.3 }
//!     - http: { url: "${api}/me", headers: { Authorization: "Bearer ${token}" } }
//! ```

use std::collections::{BTreeMap, BTreeSet};

use chronostack_core::duration::parse_duration;
use serde::Deserialize;
use serde_json::Value;

use crate::arrival;
use crate::template;

/// Variables every user has.
pub const BUILTIN_VARS: [&str; 3] = ["user", "cohort", "run_id"];

pub const MAX_USERS_PER_COHORT: u32 = 100_000;
pub const MAX_PARALLEL: usize = 256;
pub const DEFAULT_PARALLEL: usize = 32;
pub const MAX_REPEAT: u32 = 10_000;
pub const MAX_NESTING: usize = 8;
/// Longest real think time when journeys run in real time (standalone).
pub const DEFAULT_MAX_THINK_NS: i64 = 30_000_000_000;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub version: u32,
    /// Fixes every random choice of the run. A random seed is picked (and
    /// reported) when it's absent.
    pub seed: Option<u64>,
    /// Most users running a step at the same time (default 32, at most 256).
    pub max_parallel: Option<usize>,
    /// Cap on each real-time think (default 30s).
    pub max_think: Option<String>,
    /// Named base URLs (`http://…`, `https://…` or `service://app:3000`),
    /// used as `${name}`.
    #[serde(default)]
    pub targets: BTreeMap<String, String>,
    pub cohorts: Vec<Cohort>,
    pub journeys: BTreeMap<String, Vec<Step>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cohort {
    pub name: String,
    pub users: u32,
    /// How the users' start times are spread; all start at once without it.
    pub arrive: Option<Arrive>,
    pub journey: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Arrive {
    pub over: String,
    #[serde(default)]
    pub model: arrival::Model,
}

/// One step: exactly one of `http`, `think`, `branch` or `repeat`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub name: Option<String>,
    pub http: Option<Http>,
    pub think: Option<Think>,
    pub branch: Option<Branch>,
    pub repeat: Option<Repeat>,
    /// Only with `http`.
    pub expect: Option<Expect>,
    /// Only with `http`: `variable: /json/pointer` into the response body.
    pub save: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Http {
    #[serde(default = "default_get")]
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    /// JSON body (sent as `application/json`).
    pub json: Option<Value>,
    /// Raw body; set `Content-Type` in `headers`.
    pub body: Option<String>,
}

fn default_get() -> String {
    "GET".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Think {
    Fixed(String),
    Between { between: [String; 2] },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Branch {
    /// Probability the user goes on; otherwise their journey ends here.
    pub p: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repeat {
    pub times: u32,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Expect {
    /// One status or a list; default any 2xx.
    pub status: Option<StatusExpect>,
    /// `/json/pointer: value` pairs the response body must contain.
    pub json: Option<BTreeMap<String, Value>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StatusExpect {
    One(u16),
    Any(Vec<u16>),
}

impl StatusExpect {
    pub fn accepts(&self, status: u16) -> bool {
        match self {
            StatusExpect::One(s) => *s == status,
            StatusExpect::Any(list) => list.contains(&status),
        }
    }
}

/// A validated file, with durations in nanoseconds.
#[derive(Debug, Clone)]
pub struct Plan {
    pub file: File,
    pub max_parallel: usize,
    pub max_think_ns: i64,
    pub cohorts: Vec<CohortPlan>,
    /// Steps of each journey, flattened to names for reports, in file order.
    pub step_names: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct CohortPlan {
    pub name: String,
    pub users: u32,
    pub journey: String,
    pub arrive_ns: i64,
    pub model: arrival::Model,
}

/// Parse and validate a journey file.
pub fn parse(text: &str) -> Result<Plan, String> {
    let file: File = serde_norway::from_str(text).map_err(|e| e.to_string())?;
    validate(file)
}

fn dur(what: &str, v: &str) -> Result<i64, String> {
    let ns = parse_duration(v).map_err(|e| format!("{what}: {e}"))?;
    if ns < 0 {
        return Err(format!("{what}: must not be negative"));
    }
    Ok(ns)
}

fn is_name(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(f) if f.is_ascii_alphabetic() || f == '_')
        && c.all(|x| x.is_ascii_alphanumeric() || x == '_')
}

pub fn validate(file: File) -> Result<Plan, String> {
    if file.version != 1 {
        return Err(format!(
            "unsupported `version: {}` (expected 1)",
            file.version
        ));
    }
    let max_parallel = file.max_parallel.unwrap_or(DEFAULT_PARALLEL);
    if !(1..=MAX_PARALLEL).contains(&max_parallel) {
        return Err(format!("max_parallel must be 1-{MAX_PARALLEL}"));
    }
    let max_think_ns = match &file.max_think {
        Some(v) => dur("max_think", v)?,
        None => DEFAULT_MAX_THINK_NS,
    };
    for (name, url) in &file.targets {
        if !is_name(name) || BUILTIN_VARS.contains(&name.as_str()) {
            return Err(format!(
                "target `{name}`: use a name of letters, digits and `_` that isn't {}",
                BUILTIN_VARS.join(", ")
            ));
        }
        let ok = ["http://", "https://", "service://"]
            .iter()
            .any(|p| url.starts_with(p));
        if !ok {
            return Err(format!(
                "target `{name}`: `{url}` must start with http://, https:// or service://"
            ));
        }
    }
    if file.cohorts.is_empty() {
        return Err("at least one cohort is needed".into());
    }
    let mut cohort_names = BTreeSet::new();
    let mut cohorts = Vec::new();
    for c in &file.cohorts {
        let what = format!("cohort `{}`", c.name);
        if c.name.trim().is_empty() || !cohort_names.insert(c.name.clone()) {
            return Err(format!(
                "cohorts need unique, non-empty names (`{}`)",
                c.name
            ));
        }
        if !(1..=MAX_USERS_PER_COHORT).contains(&c.users) {
            return Err(format!("{what}: users must be 1-{MAX_USERS_PER_COHORT}"));
        }
        if !file.journeys.contains_key(&c.journey) {
            return Err(format!("{what}: unknown journey `{}`", c.journey));
        }
        let (arrive_ns, model) = match &c.arrive {
            Some(a) => (dur(&format!("{what}: arrive.over"), &a.over)?, a.model),
            None => (0, arrival::Model::Uniform),
        };
        cohorts.push(CohortPlan {
            name: c.name.clone(),
            users: c.users,
            journey: c.journey.clone(),
            arrive_ns,
            model,
        });
    }
    let mut step_names = BTreeMap::new();
    for (jname, steps) in &file.journeys {
        if steps.is_empty() {
            return Err(format!("journey `{jname}` has no steps"));
        }
        let mut known: BTreeSet<String> = BUILTIN_VARS.iter().map(|s| s.to_string()).collect();
        known.extend(file.targets.keys().cloned());
        let mut names = Vec::new();
        check_steps(
            &format!("journey `{jname}`"),
            steps,
            &mut known,
            &mut names,
            0,
        )?;
        step_names.insert(jname.clone(), names);
    }
    Ok(Plan {
        file,
        max_parallel,
        max_think_ns,
        cohorts,
        step_names,
    })
}

/// A step's display name: its `name:`, or a short description.
pub fn step_label(step: &Step) -> String {
    if let Some(n) = &step.name {
        return n.clone();
    }
    if let Some(h) = &step.http {
        return format!("{} {}", h.method.to_ascii_uppercase(), h.url);
    }
    if let Some(t) = &step.think {
        return match t {
            Think::Fixed(d) => format!("think {d}"),
            Think::Between { between } => format!("think {}-{}", between[0], between[1]),
        };
    }
    if let Some(b) = &step.branch {
        return format!("branch p={}", b.p);
    }
    if let Some(r) = &step.repeat {
        return format!("repeat {}x", r.times);
    }
    "step".into()
}

fn check_steps(
    path: &str,
    steps: &[Step],
    known: &mut BTreeSet<String>,
    names: &mut Vec<String>,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_NESTING {
        return Err(format!(
            "{path}: `repeat` nested more than {MAX_NESTING} deep"
        ));
    }
    for (i, step) in steps.iter().enumerate() {
        let what = format!("{path}, step {} ({})", i + 1, step_label(step));
        let kinds = [
            step.http.is_some(),
            step.think.is_some(),
            step.branch.is_some(),
            step.repeat.is_some(),
        ];
        if kinds.iter().filter(|k| **k).count() != 1 {
            return Err(format!(
                "{what}: set exactly one of `http`, `think`, `branch` or `repeat`"
            ));
        }
        if step.http.is_none() && (step.expect.is_some() || step.save.is_some()) {
            return Err(format!("{what}: `expect` and `save` go with `http`"));
        }
        let used = |vals: &mut Vec<String>| -> Result<(), String> {
            for v in vals.iter() {
                if !known.contains(v) {
                    return Err(format!(
                        "{what}: `${{{v}}}` isn't defined here (targets, {}, or a value saved by an earlier step)",
                        BUILTIN_VARS.join(", ")
                    ));
                }
            }
            Ok(())
        };
        if let Some(h) = &step.http {
            let method = h.method.to_ascii_uppercase();
            if !matches!(
                method.as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE" | "HEAD" | "OPTIONS"
            ) {
                return Err(format!("{what}: unsupported method `{}`", h.method));
            }
            if h.json.is_some() && h.body.is_some() {
                return Err(format!("{what}: set `json` or `body`, not both"));
            }
            let mut refs = template::refs(&h.url);
            for (k, v) in &h.headers {
                refs.extend(template::refs(k));
                refs.extend(template::refs(v));
            }
            if let Some(j) = &h.json {
                template::refs_json(j, &mut refs);
            }
            if let Some(b) = &h.body {
                refs.extend(template::refs(b));
            }
            used(&mut refs)?;
            if let Some(e) = &step.expect {
                if let Some(StatusExpect::Any(list)) = &e.status {
                    if list.is_empty() {
                        return Err(format!("{what}: expect.status list is empty"));
                    }
                }
                for ptr in e.json.iter().flat_map(|m| m.keys()) {
                    if !ptr.is_empty() && !ptr.starts_with('/') {
                        return Err(format!(
                            "{what}: expect.json key `{ptr}` must be a JSON pointer like /plan"
                        ));
                    }
                }
            }
            for (var, ptr) in step.save.iter().flatten() {
                if !is_name(var) || BUILTIN_VARS.contains(&var.as_str()) {
                    return Err(format!("{what}: can't save into `{var}`"));
                }
                if !ptr.is_empty() && !ptr.starts_with('/') {
                    return Err(format!(
                        "{what}: save.{var}: `{ptr}` must be a JSON pointer like /token"
                    ));
                }
            }
            // Saved values are known to later steps.
            known.extend(step.save.iter().flatten().map(|(k, _)| k.clone()));
        }
        if let Some(t) = &step.think {
            match t {
                Think::Fixed(d) => {
                    dur(&format!("{what}: think"), d)?;
                }
                Think::Between { between } => {
                    let lo = dur(&format!("{what}: think.between"), &between[0])?;
                    let hi = dur(&format!("{what}: think.between"), &between[1])?;
                    if lo > hi {
                        return Err(format!("{what}: think.between goes from low to high"));
                    }
                }
            }
        }
        if let Some(b) = &step.branch {
            if !(0.0..=1.0).contains(&b.p) {
                return Err(format!("{what}: branch.p must be between 0 and 1"));
            }
        }
        names.push(step_label(step));
        if let Some(r) = &step.repeat {
            if !(1..=MAX_REPEAT).contains(&r.times) {
                return Err(format!("{what}: repeat.times must be 1-{MAX_REPEAT}"));
            }
            if r.steps.is_empty() {
                return Err(format!("{what}: repeat.steps is empty"));
            }
            check_steps(&what, &r.steps, known, names, depth + 1)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = r#"
version: 1
seed: 42
max_parallel: 8
targets:
  api: service://app:3000
cohorts:
  - name: trialists
    users: 50
    arrive: { over: 10m, model: poisson }
    journey: trial_to_paid
  - name: browsers
    users: 5
    journey: browse
journeys:
  trial_to_paid:
    - name: sign up
      http: { method: POST, url: "${api}/signup", json: { email: "u${user}+${run_id}@test" } }
      expect: { status: 201, json: { /plan: trial } }
      save: { token: /token }
    - think: { between: [2s, 5s] }
    - branch: { p: 0.3 }
    - name: add card
      http:
        method: post
        url: "${api}/billing/card"
        headers: { Authorization: "Bearer ${token}" }
      expect: { status: [200, 204] }
  browse:
    - repeat:
        times: 3
        steps:
          - http: { url: "${api}/products" }
          - think: 1s
"#;

    fn with(steps: &str) -> Result<Plan, String> {
        parse(&format!(
            "version: 1\ntargets: {{ api: 'http://localhost:1' }}\ncohorts: [{{ name: c, users: 1, journey: j }}]\njourneys:\n  j:\n{steps}"
        ))
    }

    #[test]
    fn example_parses() {
        let p = parse(EXAMPLE).unwrap();
        assert_eq!(p.max_parallel, 8);
        assert_eq!(p.max_think_ns, DEFAULT_MAX_THINK_NS);
        assert_eq!(p.file.seed, Some(42));
        let c = &p.cohorts[0];
        assert_eq!(
            (c.users, c.arrive_ns, c.model),
            (50, 600_000_000_000, arrival::Model::Poisson)
        );
        assert_eq!(p.cohorts[1].arrive_ns, 0);
        assert_eq!(
            p.step_names["trial_to_paid"],
            ["sign up", "think 2s-5s", "branch p=0.3", "add card"]
        );
        assert_eq!(
            p.step_names["browse"],
            ["repeat 3x", "GET ${api}/products", "think 1s"]
        );
    }

    #[test]
    fn status_expectations() {
        assert!(StatusExpect::One(201).accepts(201));
        assert!(!StatusExpect::One(201).accepts(200));
        assert!(StatusExpect::Any(vec![200, 204]).accepts(204));
    }

    #[test]
    fn variables_must_be_defined_before_use() {
        // token is saved by step 1, so step 2 may use it…
        assert!(with("    - http: { url: '${api}/a' }\n      save: { token: /t }\n    - http: { url: '${api}/b?t=${token}' }\n").is_ok());
        // …but not before it's saved.
        let err = with("    - http: { url: '${api}/b?t=${token}' }\n    - http: { url: '${api}/a' }\n      save: { token: /t }\n").unwrap_err();
        assert!(err.contains("`${token}` isn't defined here"), "{err}");
        // Built-ins are always there; unknown names are not.
        assert!(with("    - http: { url: '${api}/${user}/${cohort}/${run_id}' }\n").is_ok());
        assert!(with("    - http: { url: '${nope}/x' }\n").is_err());
        // Variables in headers and JSON bodies are checked too.
        assert!(with("    - http: { url: '${api}', headers: { X: '${nope}' } }\n").is_err());
        assert!(with("    - http: { url: '${api}', json: { a: ['${nope}'] } }\n").is_err());
    }

    #[test]
    fn rejects_bad_files() {
        let bad = [
            (
                "    - think: 1s\n      http: { url: 'http://x' }\n",
                "exactly one of",
            ),
            (
                "    - think: 1s\n      expect: { status: 200 }\n",
                "go with `http`",
            ),
            (
                "    - http: { url: 'http://x', method: FETCH }\n",
                "unsupported method",
            ),
            (
                "    - http: { url: 'http://x', json: {}, body: x }\n",
                "not both",
            ),
            (
                "    - http: { url: 'http://x' }\n      expect: { status: [] }\n",
                "list is empty",
            ),
            (
                "    - http: { url: 'http://x' }\n      expect: { json: { plan: pro } }\n",
                "JSON pointer",
            ),
            (
                "    - http: { url: 'http://x' }\n      save: { user: /id }\n",
                "can't save into",
            ),
            (
                "    - http: { url: 'http://x' }\n      save: { t: token }\n",
                "JSON pointer",
            ),
            ("    - think: soon\n", "think"),
            ("    - think: { between: [5s, 2s] }\n", "low to high"),
            ("    - branch: { p: 1.5 }\n", "between 0 and 1"),
            (
                "    - repeat: { times: 0, steps: [{ think: 1s }] }\n",
                "repeat.times",
            ),
            ("    - repeat: { times: 2, steps: [] }\n", "empty"),
            (
                "    - http: { url: 'http://x', extra: 1 }\n",
                "unknown field",
            ),
        ];
        for (steps, needle) in bad {
            let err = with(steps).unwrap_err();
            assert!(err.contains(needle), "{steps:?}: {err}");
        }
    }

    #[test]
    fn rejects_bad_top_level() {
        let base =
            "cohorts: [{ name: c, users: 1, journey: j }]\njourneys: { j: [{ think: 1s }] }\n";
        assert!(parse(&format!("version: 1\n{base}")).is_ok());
        let bad = [
            (format!("version: 2\n{base}"), "version"),
            (format!("version: 1\nmax_parallel: 0\n{base}"), "max_parallel"),
            (format!("version: 1\nmax_parallel: 999\n{base}"), "max_parallel"),
            (format!("version: 1\ntargets: {{ api: 'ftp://x' }}\n{base}"), "must start with"),
            (format!("version: 1\ntargets: {{ user: 'http://x' }}\n{base}"), "target `user`"),
            ("version: 1\ncohorts: []\njourneys: {}\n".to_string(), "at least one cohort"),
            ("version: 1\ncohorts: [{ name: c, users: 1, journey: nope }]\njourneys: { j: [{ think: 1s }] }\n".to_string(), "unknown journey"),
            ("version: 1\ncohorts: [{ name: c, users: 0, journey: j }]\njourneys: { j: [{ think: 1s }] }\n".to_string(), "users must be"),
            ("version: 1\ncohorts: [{ name: c, users: 1, journey: j }, { name: c, users: 1, journey: j }]\njourneys: { j: [{ think: 1s }] }\n".to_string(), "unique"),
            ("version: 1\ncohorts: [{ name: c, users: 1, journey: j }]\njourneys: { j: [] }\n".to_string(), "no steps"),
            ("version: 1\ncohorts: [{ name: c, users: 1, journey: j, arrive: { over: 1m, model: burst } }]\njourneys: { j: [{ think: 1s }] }\n".to_string(), "burst"),
        ];
        for (text, needle) in bad {
            let err = parse(&text).unwrap_err();
            assert!(err.contains(needle), "{text:?}: {err}");
        }
    }

    #[test]
    fn repeat_nesting_is_limited() {
        let mut steps = "{ think: 1s }".to_string();
        for _ in 0..=MAX_NESTING + 1 {
            steps = format!("{{ repeat: {{ times: 1, steps: [{steps}] }} }}");
        }
        let err = with(&format!("    - {steps}\n")).unwrap_err();
        assert!(err.contains("nested"), "{err}");
    }
}
