//! Linear scenarios: `chronostack run scenarios/trial.yml`.
//!
//! ```yaml
//! version: 1
//! name: Trial expires and the webhook survives an outage
//! steps:
//!   - advance: { to: next 00:00 }                # now, +<dur>, next HH:MM or a UTC time
//!   - name: sign up
//!     exec: { service: api, command: [node, scripts/call.js, signup, a@example.com] }
//!   - fault: { target: webhooks, from: +14d2h, for: 4h }
//!   - advance: 14d
//!   - advance: { by: 12h, step: 1h }
//!   - name: user was downgraded
//!     assert: { sql: { service: db, query: "select plan from users", equals: free } }
//!   - wait_for: { redis: { service: cache, command: [GET, k], equals: "1" }, timeout: 10s }
//! ```
//!
//! Steps run in order and stop at the first failure. Every run writes a report
//! (see [`crate::report`]). Exit code 0 = passed, 6 = a step failed.
//!
//! **Variables.** `${run_id}` is a random id for each run (use it to make
//! names unique, so a scenario can run again on the same stack). An `exec`
//! step with `save: name` stores the last line of its output as `${name}` for
//! later steps. References are checked before the run; reports show the
//! `${…}` form, not the values.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use chronostack_core::duration::{format_span, format_utc, parse_duration};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::client::Client;
use crate::config::{Barrier, ExecSpec, HttpSpec, RedisSpec, Resolved, SqlSpec};
use crate::engine::{self, MaxJump, Options, Target};
use crate::fault;
use crate::probe;
use crate::report::{self, redact};
use crate::session::Session;

pub const EXIT_SCENARIO_FAILED: i32 = 6;
const DEFAULT_WAIT_NS: i64 = 30_000_000_000;
const DEFAULT_EXEC_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub version: u32,
    pub name: String,
    pub description: Option<String>,
    /// Simulated users that act on the virtual clock during `advance` steps.
    pub users: Option<crate::vusers::UsersRef>,
    pub steps: Vec<Step>,
    /// Steps as written (with `${…}` unresolved), re-read at run time after
    /// substituting variables.
    #[serde(skip)]
    pub raw_steps: Vec<Value>,
}

/// Built-in variables.
const BUILTIN_VARS: [&str; 1] = ["run_id"];

fn var_regex() -> &'static regex::Regex {
    static R: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    R.get_or_init(|| regex::Regex::new(r"\$\{([A-Za-z_][A-Za-z0-9_]*)\}").expect("valid regex"))
}

/// Every `${name}` referenced in a JSON tree.
fn var_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::String(s) => {
            out.extend(var_regex().captures_iter(s).map(|c| c[1].to_string()));
        }
        Value::Array(a) => a.iter().for_each(|x| var_refs(x, out)),
        Value::Object(m) => m.values().for_each(|x| var_refs(x, out)),
        _ => {}
    }
}

/// Replace `${name}` in every string of a JSON tree.
fn substitute(v: &Value, vars: &std::collections::BTreeMap<String, String>) -> Value {
    match v {
        Value::String(s) => Value::String(
            var_regex()
                .replace_all(s, |c: &regex::Captures<'_>| {
                    vars.get(&c[1]).cloned().unwrap_or_else(|| c[0].to_string())
                })
                .into_owned(),
        ),
        Value::Array(a) => Value::Array(a.iter().map(|x| substitute(x, vars)).collect()),
        Value::Object(m) => Value::Object(
            m.iter()
                .map(|(k, x)| (k.clone(), substitute(x, vars)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Parse scenario YAML (typed steps for checking, raw steps for running).
pub fn parse(text: &str) -> Result<Scenario> {
    let mut sc: Scenario = serde_norway::from_str(text)?;
    let raw: Value = serde_norway::from_str(text)?;
    sc.raw_steps = raw
        .get("steps")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    Ok(sc)
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    /// Shown in output and reports; generated from the action when omitted.
    pub name: Option<String>,
    pub advance: Option<AdvanceStep>,
    /// Run a trigger declared in chronostack.yml, now.
    pub trigger: Option<String>,
    pub exec: Option<ExecSpec>,
    pub http: Option<HttpSpec>,
    pub fault: Option<FaultStep>,
    /// Proxy target name, a fault id (`#3`), or `all`.
    pub fault_clear: Option<String>,
    /// Checked once.
    #[serde(rename = "assert")]
    pub check: Option<Check>,
    /// Polled until true or `timeout` (default 30 s real time).
    pub wait_for: Option<Check>,
    /// Real time to wait (lets services react).
    pub sleep: Option<String>,
    /// `exec` only: store the last line of the output as `${name}`.
    pub save: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AdvanceStep {
    By(String),
    Full(AdvanceFull),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvanceFull {
    pub by: Option<String>,
    /// `+<dur>`, `next HH:MM` or a UTC time.
    pub to: Option<String>,
    pub step: Option<String>,
    pub max_jump: Option<String>,
    pub dwell: Option<String>,
    /// Probe services afterwards (default true).
    pub probe: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaultStep {
    pub target: String,
    /// `now` (default), `+<dur>`, `next HH:MM` or a UTC time.
    #[serde(default = "now")]
    pub from: String,
    #[serde(rename = "for")]
    pub duration: Option<String>,
    pub until: Option<String>,
    pub status: Option<u16>,
}

fn now() -> String {
    "now".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub sql: Option<SqlSpec>,
    pub redis: Option<RedisSpec>,
    pub exec: Option<ExecSpec>,
    pub http: Option<HttpSpec>,
    pub timeout: Option<String>,
}

impl Check {
    fn barrier(&self, name: &str) -> Barrier {
        Barrier {
            name: name.into(),
            sql: self.sql.clone(),
            redis: self.redis.clone(),
            exec: self.exec.clone(),
            http: self.http.clone(),
            bullmq: None,
            sidekiq: None,
            celery: None,
            timeout: self.timeout.clone(),
            settle: None,
        }
    }

    fn describe(&self) -> String {
        if let Some(s) = &self.sql {
            match &s.equals {
                Some(e) => format!("sql on {}: `{}` = `{e}`", s.service, s.query),
                None => format!("sql on {}: `{}`", s.service, s.query),
            }
        } else if let Some(r) = &self.redis {
            format!(
                "redis on {}: {} = `{}`",
                r.service,
                r.command.join(" "),
                r.equals
            )
        } else if let Some(e) = &self.exec {
            format!("exec on {}: {} exits 0", e.service, e.command.join(" "))
        } else if let Some(h) = &self.http {
            format!("http {} {}", h.method, h.url)
        } else {
            "?".into()
        }
    }

    fn count(&self) -> usize {
        [
            self.sql.is_some(),
            self.redis.is_some(),
            self.exec.is_some(),
            self.http.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count()
    }
}

impl Step {
    fn kind(&self) -> &'static str {
        if self.advance.is_some() {
            "advance"
        } else if self.trigger.is_some() {
            "trigger"
        } else if self.exec.is_some() {
            "exec"
        } else if self.http.is_some() {
            "http"
        } else if self.fault.is_some() {
            "fault"
        } else if self.fault_clear.is_some() {
            "fault_clear"
        } else if self.check.is_some() {
            "assert"
        } else if self.wait_for.is_some() {
            "wait_for"
        } else {
            "sleep"
        }
    }

    fn actions(&self) -> usize {
        [
            self.advance.is_some(),
            self.trigger.is_some(),
            self.exec.is_some(),
            self.http.is_some(),
            self.fault.is_some(),
            self.fault_clear.is_some(),
            self.check.is_some(),
            self.wait_for.is_some(),
            self.sleep.is_some(),
        ]
        .iter()
        .filter(|x| **x)
        .count()
    }

    pub fn title(&self) -> String {
        if let Some(n) = &self.name {
            return n.clone();
        }
        match self.kind() {
            "advance" => match self.advance.as_ref().expect("kind") {
                AdvanceStep::By(d) => format!("advance {d}"),
                AdvanceStep::Full(f) => {
                    let mut s = match (&f.by, &f.to) {
                        (Some(b), _) => format!("advance {b}"),
                        (_, Some(t)) => format!("advance to {t}"),
                        _ => "advance".into(),
                    };
                    if let Some(st) = &f.step {
                        s += &format!(" --step {st}");
                    }
                    s
                }
            },
            "trigger" => format!("trigger {}", self.trigger.as_deref().unwrap_or("")),
            "exec" => {
                let e = self.exec.as_ref().expect("kind");
                format!("exec on {}: {}", e.service, e.command.join(" "))
            }
            "http" => {
                let h = self.http.as_ref().expect("kind");
                format!("http {} {}", h.method, h.url)
            }
            "fault" => {
                let f = self.fault.as_ref().expect("kind");
                format!(
                    "outage on {} from {} {}",
                    f.target,
                    f.from,
                    match (&f.duration, &f.until) {
                        (Some(d), _) => format!("for {d}"),
                        (_, Some(u)) => format!("until {u}"),
                        _ => String::new(),
                    }
                )
            }
            "fault_clear" => format!("clear outage {}", self.fault_clear.as_deref().unwrap_or("")),
            "assert" => format!("assert {}", self.check.as_ref().expect("kind").describe()),
            "wait_for" => format!(
                "wait for {}",
                self.wait_for.as_ref().expect("kind").describe()
            ),
            _ => format!("sleep {}", self.sleep.as_deref().unwrap_or("")),
        }
    }
}

fn dur(what: &str, v: &str) -> Result<i64> {
    parse_duration(v).map_err(|e| anyhow::anyhow!("{what}: {e}"))
}

/// Parse and check a scenario against the config before anything runs.
pub fn load(path: &Path, cfg: &Resolved, services: &[String]) -> Result<Scenario> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let sc = parse(&text).with_context(|| format!("invalid scenario {}", path.display()))?;
    validate(&sc, cfg, services).with_context(|| format!("invalid scenario {}", path.display()))?;
    if let Some(u) = &sc.users {
        u.resolution_ns()
            .with_context(|| format!("invalid scenario {}", path.display()))?;
        crate::vusers::load_plan(&crate::vusers::journey_path(path, u))
            .with_context(|| format!("invalid scenario {}: users", path.display()))?;
    }
    Ok(sc)
}

pub fn validate(sc: &Scenario, cfg: &Resolved, services: &[String]) -> Result<()> {
    if sc.version != 1 {
        bail!("unsupported `version: {}` (expected 1)", sc.version);
    }
    if sc.steps.is_empty() {
        bail!("`steps` is empty");
    }
    let svc = |what: &str, s: &str| -> Result<()> {
        if !services.is_empty() && !services.iter().any(|x| x == s) {
            bail!(
                "{what}: unknown service `{s}` (known: {})",
                services.join(", ")
            );
        }
        Ok(())
    };
    let proxies: Vec<&str> = cfg.config.proxies.iter().map(|p| p.name.as_str()).collect();
    let mut defined: Vec<String> = BUILTIN_VARS.iter().map(|s| s.to_string()).collect();
    for (i, st) in sc.steps.iter().enumerate() {
        let what = format!(
            "step {} ({})",
            i + 1,
            st.name.as_deref().unwrap_or(st.kind())
        );
        if let Some(raw) = sc.raw_steps.get(i) {
            let mut refs = Vec::new();
            var_refs(raw, &mut refs);
            if let Some(missing) = refs.iter().find(|r| !defined.contains(r)) {
                bail!(
                    "{what}: `${{{missing}}}` is not defined (built-in: {}; an earlier exec step can `save:` it)",
                    BUILTIN_VARS.join(", ")
                );
            }
        }
        if let Some(name) = &st.save {
            if st.exec.is_none() {
                bail!("{what}: `save` works only on exec steps");
            }
            let ok = !name.is_empty()
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && !name.starts_with(|c: char| c.is_ascii_digit());
            if !ok || BUILTIN_VARS.contains(&name.as_str()) {
                bail!("{what}: `save: {name}` must be a new name of letters, digits and `_`");
            }
            defined.push(name.clone());
        }
        if st.actions() != 1 {
            bail!(
                "{what}: set exactly one of advance, trigger, exec, http, fault, fault_clear, assert, wait_for, sleep"
            );
        }
        if let Some(a) = &st.advance {
            match a {
                AdvanceStep::By(d) => {
                    dur(&what, d)?;
                }
                AdvanceStep::Full(f) => {
                    if f.by.is_some() == f.to.is_some() {
                        bail!("{what}: advance needs exactly one of `by` or `to`");
                    }
                    if let Some(b) = &f.by {
                        dur(&what, b)?;
                    }
                    // A target with a saved variable (`to: "${ends_at}"`) is
                    // checked at run time, once the variable has a value.
                    if let Some(t) = f.to.as_ref().filter(|t| !var_regex().is_match(t)) {
                        fault::parse_when(t, 0).with_context(|| what.clone())?;
                    }
                    for v in [&f.step, &f.dwell].into_iter().flatten() {
                        dur(&what, v)?;
                    }
                    if let Some(m) = &f.max_jump {
                        if m != "auto" && m != "none" {
                            dur(&what, m)?;
                        }
                    }
                }
            }
        }
        if let Some(t) = &st.trigger {
            if !cfg.config.triggers.iter().any(|x| &x.name == t) {
                bail!("{what}: no trigger named `{t}` in chronostack.yml");
            }
        }
        if let Some(e) = &st.exec {
            svc(&what, &e.service)?;
            if e.command.is_empty() {
                bail!("{what}: exec.command is empty");
            }
        }
        if let Some(f) = &st.fault {
            if !proxies.contains(&f.target.as_str()) {
                bail!(
                    "{what}: no proxy named `{}` in chronostack.yml (declared: {})",
                    f.target,
                    if proxies.is_empty() {
                        "none".to_string()
                    } else {
                        proxies.join(", ")
                    }
                );
            }
            // Check the forms only; the window itself is resolved against the
            // virtual time when the step runs.
            if f.duration.is_some() == f.until.is_some() {
                bail!("{what}: give exactly one of `for` (a duration) or `until` (a time)");
            }
            fault::parse_when(&f.from, 0).with_context(|| format!("{what}: from"))?;
            if let Some(d) = &f.duration {
                dur(&format!("{what}: for"), d)?;
            }
            if let Some(u) = &f.until {
                fault::parse_when(u, 0).with_context(|| format!("{what}: until"))?;
            }
        }
        if let Some(c) = &st.fault_clear {
            let c = c.trim();
            if c != "all" && !c.starts_with('#') && !proxies.contains(&c) {
                bail!("{what}: fault_clear takes a proxy name, `#<id>` or `all`");
            }
        }
        for c in [&st.check, &st.wait_for].into_iter().flatten() {
            if c.count() != 1 {
                bail!("{what}: set exactly one of sql, redis, exec, http");
            }
            for s in [
                c.sql.as_ref().map(|x| &x.service),
                c.redis.as_ref().map(|x| &x.service),
                c.exec.as_ref().map(|x| &x.service),
            ]
            .into_iter()
            .flatten()
            {
                svc(&what, s)?;
            }
            if let Some(t) = &c.timeout {
                dur(&what, t)?;
            }
        }
        if st.check.as_ref().is_some_and(|c| c.timeout.is_some()) {
            bail!("{what}: `timeout` belongs to wait_for; assert checks once");
        }
        if let Some(s) = &st.sleep {
            let d = dur(&what, s)?;
            if d > 600_000_000_000 {
                bail!("{what}: sleep is real time; at most 10m");
            }
        }
    }
    Ok(())
}

fn real_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or(0)
}

/// Compact form of an advance outcome for the report: counts plus the stops
/// where something ran or failed.
fn advance_summary(v: &Value) -> Value {
    let stops = v["stops"].as_array().cloned().unwrap_or_default();
    let interesting: Vec<Value> = stops
        .iter()
        .filter(|s| {
            s["triggers"].as_array().is_some_and(|a| !a.is_empty())
                || s["barriers"]
                    .as_array()
                    .is_some_and(|a| a.iter().any(|b| b["ok"] == false))
        })
        .take(200)
        .map(|s| {
            let mut s = s.clone();
            for k in ["triggers", "barriers"] {
                if let Some(a) = s[k].as_array_mut() {
                    for x in a {
                        if let Some(d) = x["detail"].as_str() {
                            x["detail"] = Value::String(redact(d));
                        }
                    }
                }
            }
            s
        })
        .collect();
    json!({
        "exit_code": v["exit_code"],
        "stops": stops.len(),
        "trigger_runs": v["trigger_runs"],
        "pg_cron_mode": v["pg_cron_mode"],
        "elapsed_ms": v["elapsed_ms"],
        "stops_with_activity": interesting,
        "probes": v["probes"],
    })
}

fn advance_failure(v: &Value) -> String {
    let code = v["exit_code"].as_i64().unwrap_or(1);
    for s in v["stops"].as_array().into_iter().flatten() {
        for k in ["triggers", "barriers"] {
            for a in s[k].as_array().into_iter().flatten() {
                if a["ok"] == false {
                    return format!(
                        "{} `{}` failed at virtual {}: {}",
                        &k[..k.len() - 1],
                        a["name"].as_str().unwrap_or("?"),
                        s["at"].as_str().unwrap_or("?"),
                        redact(a["detail"].as_str().unwrap_or(""))
                    );
                }
            }
        }
    }
    if code == 3 {
        let bad: Vec<String> = v["probes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|p| !crate::probe::status_passes(p["status"].as_str().unwrap_or("")))
            .map(|p| {
                format!(
                    "{} ({})",
                    p["service"].as_str().unwrap_or("?"),
                    p["status"].as_str().unwrap_or("?")
                )
            })
            .collect();
        return format!("services do not see the virtual time: {}", bad.join(", "));
    }
    format!("advance stopped with exit code {code}")
}

struct StepOutcome {
    ok: bool,
    detail: String,
    advance: Option<Value>,
    /// Unredacted output of an `exec` step (for `save:`).
    output: Option<String>,
}

/// Refuse an `advance` step that would end more than `cap_ns` past `start_ns`
/// (the scenario's starting virtual time). `chronostack mcp` sets the cap from
/// `mcp.max_advance`, so a scenario can't move further than `clock_advance` may.
fn check_horizon(st: &Step, now_ns: i64, start_ns: i64, cap_ns: i64) -> Result<()> {
    let Some(a) = &st.advance else {
        return Ok(());
    };
    let by = |d: &str| -> Result<i64> {
        Ok(now_ns.saturating_add(parse_duration(d).map_err(anyhow::Error::msg)?))
    };
    let target = match a {
        AdvanceStep::By(d) => by(d)?,
        AdvanceStep::Full(f) => match (&f.by, &f.to) {
            (Some(b), _) => by(b)?,
            (_, Some(t)) => fault::parse_when(t, now_ns)?,
            _ => return Ok(()),
        },
    };
    let span = target.saturating_sub(start_ns);
    if span > cap_ns {
        bail!(
            "refused: this step would move virtual time {} past the scenario start, more than mcp.max_advance ({}); raise it in chronostack.yml if intended",
            format_span(span),
            format_span(cap_ns)
        );
    }
    Ok(())
}

fn run_step(
    st: &Step,
    session: &Session,
    client: &Client,
    cfg: &Resolved,
    users: &mut Option<crate::vusers::VirtualUsers>,
) -> Result<StepOutcome> {
    let compose = session.compose();
    let done = |ok: bool, detail: String| StepOutcome {
        ok,
        detail,
        advance: None,
        output: None,
    };
    match st.kind() {
        "advance" => {
            let (target, f) = match st.advance.as_ref().expect("kind") {
                AdvanceStep::By(d) => (
                    Target::By(parse_duration(d).map_err(anyhow::Error::msg)?),
                    AdvanceFull::default(),
                ),
                AdvanceStep::Full(f) => {
                    let t = match (&f.by, &f.to) {
                        (Some(b), _) => Target::By(parse_duration(b).map_err(anyhow::Error::msg)?),
                        (_, Some(t)) => {
                            Target::To(fault::parse_when(t, client.clock()?.virtual_ns)?)
                        }
                        _ => unreachable!("validated"),
                    };
                    (t, f.clone())
                }
            };
            let opts = Options {
                target,
                step: f
                    .step
                    .as_deref()
                    .map(parse_duration)
                    .transpose()
                    .map_err(anyhow::Error::msg)?,
                max_jump: match f.max_jump.as_deref() {
                    None | Some("auto") => MaxJump::Auto,
                    Some("none") => MaxJump::Disabled,
                    Some(v) => MaxJump::Fixed(parse_duration(v).map_err(anyhow::Error::msg)?),
                },
                dwell: f
                    .dwell
                    .as_deref()
                    .map(parse_duration)
                    .transpose()
                    .map_err(anyhow::Error::msg)?,
                dry_run: false,
                json: false,
                no_probe: f.probe == Some(false),
                pg_cron_all: false,
            };
            let out = match users.as_mut() {
                Some(u) => advance_with_users(session, client, cfg, opts, u)?,
                None => engine::advance_full(session, client, cfg, &opts)?,
            };
            let ok = out.code == 0;
            let detail = if ok {
                format!(
                    "{} stop(s), {} trigger run(s)",
                    out.summary["stops"]
                        .as_array()
                        .map(|a| a.len())
                        .unwrap_or(0),
                    out.summary["trigger_runs"]
                ) + &out.summary["user_actions"]
                    .as_u64()
                    .map(|n| {
                        format!(
                            ", {n} user action(s) at {} stop(s)",
                            out.summary["user_stops"]
                        )
                    })
                    .unwrap_or_default()
            } else {
                advance_failure(&out.summary)
            };
            Ok(StepOutcome {
                ok,
                detail,
                advance: Some(advance_summary(&out.summary)),
                output: None,
            })
        }
        "trigger" => {
            let name = st.trigger.as_deref().expect("kind");
            let i = cfg
                .config
                .triggers
                .iter()
                .position(|t| t.name == name)
                .expect("validated");
            let a = engine::run_trigger(
                &compose,
                &cfg.config.triggers[i],
                cfg.trigger_timeouts_ns[i],
            );
            Ok(done(a.ok, redact(a.detail.as_deref().unwrap_or(""))))
        }
        "exec" => {
            let (ok, d) = engine::run_exec(
                &compose,
                st.exec.as_ref().expect("kind"),
                DEFAULT_EXEC_TIMEOUT,
            );
            let raw = d.unwrap_or_default();
            Ok(StepOutcome {
                ok,
                detail: redact(&raw),
                advance: None,
                output: Some(raw),
            })
        }
        "http" => {
            let (ok, d) =
                engine::run_http(st.http.as_ref().expect("kind"), Duration::from_secs(30));
            Ok(done(ok, redact(d.as_deref().unwrap_or(""))))
        }
        "fault" => {
            let f = st.fault.as_ref().expect("kind");
            let x = fault::set(
                client,
                &f.target,
                &f.from,
                f.duration.as_deref(),
                f.until.as_deref(),
                f.status,
            )?;
            Ok(done(
                true,
                format!(
                    "#{}: {} returns HTTP {} from {} until {} (virtual)",
                    x.id,
                    x.target,
                    x.status,
                    format_utc(x.from_ns),
                    format_utc(x.to_ns)
                ),
            ))
        }
        "fault_clear" => {
            let c = st.fault_clear.as_deref().expect("kind").trim();
            let removed = if c == "all" {
                client.fault_clear(None)?
            } else if let Some(id) = c.strip_prefix('#') {
                client.fault_clear(Some(id.parse().context("fault id")?))?
            } else {
                let ids: Vec<u64> = client
                    .faults()?
                    .faults
                    .iter()
                    .filter(|f| f.target == c)
                    .map(|f| f.id)
                    .collect();
                let mut all = Vec::new();
                for id in ids {
                    all.extend(client.fault_clear(Some(id))?);
                }
                all
            };
            Ok(done(true, format!("{} window(s) removed", removed.len())))
        }
        "assert" => {
            let b = st.check.as_ref().expect("kind").barrier(&st.title());
            let (ok, d) = engine::barrier_once(session, &compose, &b);
            let got = redact(d.as_deref().unwrap_or(""));
            let detail = match (&b.sql, &b.redis, ok) {
                (Some(s), _, false) => format!(
                    "expected {}, got {}",
                    s.equals
                        .as_deref()
                        .map(|e| format!("`{e}`"))
                        .unwrap_or_else(|| "true".into()),
                    got.trim_start_matches("= ")
                ),
                (_, Some(r), false) => format!(
                    "expected `{}`, got `{}`",
                    r.equals,
                    got.trim_start_matches("= ")
                ),
                _ => got,
            };
            Ok(done(ok, detail))
        }
        "wait_for" => {
            let c = st.wait_for.as_ref().expect("kind");
            let timeout = c
                .timeout
                .as_deref()
                .map(parse_duration)
                .transpose()
                .map_err(anyhow::Error::msg)?
                .unwrap_or(DEFAULT_WAIT_NS);
            let a = engine::wait_barrier(session, &compose, &c.barrier(&st.title()), timeout);
            Ok(done(
                a.ok,
                if a.ok {
                    format!("after {:.1}s", a.elapsed_ms as f64 / 1000.0)
                } else {
                    redact(a.detail.as_deref().unwrap_or(""))
                },
            ))
        }
        _ => {
            let d =
                parse_duration(st.sleep.as_deref().unwrap_or("0s")).map_err(anyhow::Error::msg)?;
            std::thread::sleep(Duration::from_nanos(d.max(0) as u64));
            Ok(done(true, String::new()))
        }
    }
}

/// An `advance` with simulated users: move to each time a user is due, let the
/// due users act, wait for the barriers, and go on to the step's target. Stops
/// in between skip probes; the last one probes as configured.
fn advance_with_users(
    session: &Session,
    client: &Client,
    cfg: &Resolved,
    opts: Options,
    users: &mut crate::vusers::VirtualUsers,
) -> Result<engine::Outcome> {
    let now = client.clock()?.virtual_ns;
    let target = match opts.target {
        Target::By(d) => now + d,
        Target::To(t) => t,
    };
    let (actions0, stops0) = (users.actions, users.stops);
    let mut all_stops: Vec<Value> = Vec::new();
    let mut trigger_runs = 0u64;
    let mut sub = 0usize;
    let fail = |summary: Value, code: i32| engine::Outcome { code, summary };
    loop {
        let now = client.clock()?.virtual_ns;
        // Users due now (e.g. arrivals at the scenario's start) act first.
        if users.run_due(now) > 0 {
            if let Err((name, detail)) = crate::vusers::VirtualUsers::settle(session, cfg) {
                let summary = json!({
                    "exit_code": engine::EXIT_BARRIER_TIMEOUT,
                    "stops": [{ "at": format_utc(now), "barriers": [
                        { "name": format!("{name} (after users acted)"), "ok": false, "detail": detail }
                    ] }],
                    "trigger_runs": trigger_runs,
                });
                return Ok(fail(summary, engine::EXIT_BARRIER_TIMEOUT));
            }
        }
        let Some(next) = users.next_due().filter(|t| *t < target) else {
            break;
        };
        sub += 1;
        if sub > crate::vusers::MAX_USER_STOPS {
            bail!(
                "more than {} user stops in one advance; raise users.resolution",
                crate::vusers::MAX_USER_STOPS
            );
        }
        let step_opts = Options {
            target: Target::To(next),
            no_probe: true,
            ..opts.clone()
        };
        let out = engine::advance_full(session, client, cfg, &step_opts)?;
        trigger_runs += out.summary["trigger_runs"].as_u64().unwrap_or(0);
        all_stops.extend(out.summary["stops"].as_array().cloned().unwrap_or_default());
        if out.code != 0 {
            return Ok(out);
        }
    }
    let mut out = if client.clock()?.virtual_ns < target {
        engine::advance_full(
            session,
            client,
            cfg,
            &Options {
                target: Target::To(target),
                ..opts
            },
        )?
    } else {
        engine::Outcome {
            code: 0,
            summary: json!({ "stops": [], "trigger_runs": 0 }),
        }
    };
    if out.code == 0 {
        all_stops.extend(out.summary["stops"].as_array().cloned().unwrap_or_default());
        out.summary["stops"] = Value::Array(all_stops);
        out.summary["trigger_runs"] =
            json!(trigger_runs + out.summary["trigger_runs"].as_u64().unwrap_or(0));
    }
    out.summary["user_actions"] = json!(users.actions - actions0);
    out.summary["user_stops"] = json!(users.stops - stops0);
    Ok(out)
}

/// What a scenario would do, without running it (`chronostack run --check`).
/// `now` is the current virtual time when a session is running; without one,
/// `advance: { to: … }` targets can't be resolved and are reported as such.
pub fn check(path: &Path, sc: &Scenario, now: Option<i64>) -> Result<Value> {
    let mut t = now.unwrap_or(0);
    let mut unresolved = 0;
    let mut steps = Vec::new();
    for (i, st) in sc.steps.iter().enumerate() {
        let mut e = json!({ "index": i + 1, "name": st.title(), "kind": st.kind() });
        if let Some(a) = &st.advance {
            let by = |d: &str| parse_duration(d).map_err(anyhow::Error::msg);
            let target = match a {
                AdvanceStep::By(d) => Some(t + by(d)?),
                AdvanceStep::Full(f) => match (&f.by, &f.to) {
                    (Some(b), _) => Some(t + by(b)?),
                    (_, Some(to)) => {
                        // Relative forms work without a clock; `next HH:MM` and
                        // absolute times need the session's virtual time.
                        if now.is_some() || to.starts_with('+') {
                            Some(fault::parse_when(to, t)?)
                        } else {
                            None
                        }
                    }
                    _ => None,
                },
            };
            match target {
                Some(x) if x > t => {
                    e["advance_ns"] = json!(x - t);
                    e["advance"] = json!(format_span(x - t).trim_start_matches('+'));
                    t = x;
                }
                Some(_) => e["advance_ns"] = json!(0),
                None => {
                    unresolved += 1;
                    e["advance"] = json!("depends on the current virtual time");
                }
            }
        }
        steps.push(e);
    }
    let start = now.unwrap_or(0);
    let mut out = json!({
        "scenario": sc.name,
        "path": path.to_string_lossy().replace('\\', "/"),
        "valid": true,
        "steps": steps,
        "total_advance_ns": t - start,
        "total_advance": format_span(t - start).trim_start_matches('+'),
        "unresolved_advances": unresolved,
    });
    if let Some(n) = now {
        out["virtual_start"] = json!(format_utc(n));
        out["virtual_end"] = json!(format_utc(t));
    }
    if let Some(u) = &sc.users {
        let jp = crate::vusers::journey_path(path, u);
        let plan = crate::vusers::load_plan(&jp)?;
        let total: u64 = plan.cohorts.iter().map(|c| u64::from(c.users)).sum();
        out["users"] = json!({
            "file": jp.to_string_lossy().replace('\\', "/"),
            "total": total,
            "resolution": format_span(u.resolution_ns()?).trim_start_matches('+'),
            "seed": plan.file.seed,
            "targets": plan.file.targets,
            "cohorts": plan.cohorts.iter().map(|c| json!({
                "name": c.name, "users": c.users, "journey": c.journey,
                "arrive_over": format_span(c.arrive_ns).trim_start_matches('+'),
            })).collect::<Vec<_>>(),
            "journeys": plan.step_names,
        });
    }
    Ok(out)
}

/// `chronostack run --check` as text.
pub fn check_text(v: &Value) -> String {
    let mut o = format!(
        "{} is valid: {} step(s), virtual time moves {}{}\n",
        v["path"].as_str().unwrap_or(""),
        v["steps"].as_array().map_or(0, Vec::len),
        v["total_advance"].as_str().unwrap_or("?"),
        match v["unresolved_advances"].as_u64() {
            Some(0) | None => String::new(),
            Some(n) => format!(
                " plus {n} advance(s) that depend on the current time (start the stack to see them)"
            ),
        }
    );
    for s in v["steps"].as_array().into_iter().flatten() {
        let adv = s["advance"]
            .as_str()
            .map(|a| format!("  ({a})"))
            .unwrap_or_default();
        o += &format!(
            "  {}. {} [{}]{adv}\n",
            s["index"],
            s["name"].as_str().unwrap_or(""),
            s["kind"].as_str().unwrap_or("")
        );
    }
    if let Some(u) = v.get("users").filter(|u| u.is_object()) {
        o += &format!(
            "  users: {} from {}, on a {} grid\n",
            u["total"],
            u["file"].as_str().unwrap_or(""),
            u["resolution"].as_str().unwrap_or("")
        );
        for c in u["cohorts"].as_array().into_iter().flatten() {
            o += &format!(
                "    cohort {}: {} user(s), journey {}, arriving over {}\n",
                c["name"].as_str().unwrap_or(""),
                c["users"],
                c["journey"].as_str().unwrap_or(""),
                c["arrive_over"].as_str().unwrap_or("")
            );
        }
    }
    o
}

/// How `chronostack run` reports and what it may do.
#[derive(Debug, Default)]
pub struct RunOptions {
    /// Print the report JSON instead of progress.
    pub json: bool,
    /// Refuse advance steps that end more than this past the start (MCP cap).
    pub max_advance_ns: Option<i64>,
    /// Also write a JUnit XML file here.
    pub junit: Option<PathBuf>,
}

/// Run a scenario; returns the exit code (0 passed, 6 failed).
pub fn run(
    session: &Session,
    client: &Client,
    cfg: &Resolved,
    path: &Path,
    sc: &Scenario,
    opts: &RunOptions,
) -> Result<i32> {
    let json_out = opts.json;
    let max_advance_ns = opts.max_advance_ns;
    let real_start = real_now();
    let t0 = Instant::now();
    let start = client.clock()?;
    let first_seq = client.events(u64::MAX).map(|e| e.last_seq).unwrap_or(0);
    let total = sc.steps.len();
    let mut rid = [0u8; 4];
    getrandom::fill(&mut rid).map_err(|e| anyhow::anyhow!("no OS randomness: {e}"))?;
    let run_id: String = rid.iter().map(|b| format!("{b:02x}")).collect();
    let mut vars = std::collections::BTreeMap::from([("run_id".to_string(), run_id.clone())]);
    let mut users = match &sc.users {
        Some(u) => {
            let plan = crate::vusers::load_plan(&crate::vusers::journey_path(path, u))?;
            let targets = crate::users::targets(&plan)?;
            let seed = match plan.file.seed {
                Some(s) => s,
                None => {
                    let mut b = [0u8; 8];
                    getrandom::fill(&mut b)
                        .map_err(|e| anyhow::anyhow!("no OS randomness: {e}"))?;
                    u64::from_le_bytes(b) >> 11
                }
            };
            let users_total: u64 = plan.cohorts.iter().map(|c| u64::from(c.users)).sum();
            if !json_out {
                println!(
                    "users: {users_total} from {} (seed {seed}, actions on a {} grid)",
                    u.file(),
                    format_span(u.resolution_ns()?).trim_start_matches('+')
                );
            }
            Some(crate::vusers::VirtualUsers::new(
                plan,
                &targets,
                seed,
                &run_id,
                u.resolution_ns()?,
                start.virtual_ns,
            ))
        }
        None => None,
    };
    if !json_out {
        println!("scenario: {} ({total} steps, run id {run_id})\n", sc.name);
    }
    let mut steps: Vec<Value> = Vec::new();
    let mut clock_log = vec![
        json!({ "after_step": 0, "virtual": format_utc(start.virtual_ns), "generation": start.generation, "frozen": start.frozen }),
    ];
    let mut failed: Option<Value> = None;
    for (i, st) in sc.steps.iter().enumerate() {
        let idx = i + 1;
        let title = st.title();
        if failed.is_some() {
            steps.push(
                json!({ "index": idx, "name": title, "kind": st.kind(), "status": "skipped" }),
            );
            continue;
        }
        if !json_out {
            println!("── step {idx}/{total}: {title}");
        }
        let before = client.clock()?;
        let s0 = Instant::now();
        // Run the step as written, with variables substituted. The title above
        // keeps the `${…}` form so saved values (tokens) don't reach the report.
        let resolved: Result<Step> = match sc.raw_steps.get(i) {
            Some(raw) => serde_json::from_value(substitute(raw, &vars))
                .context("step after substituting variables"),
            None => Ok(st.clone()),
        };
        let mut out = resolved
            .and_then(|step| {
                if let Some(cap) = max_advance_ns {
                    check_horizon(&step, before.virtual_ns, start.virtual_ns, cap)?;
                }
                run_step(&step, session, client, cfg, &mut users)
            })
            .unwrap_or_else(|e| StepOutcome {
                ok: false,
                detail: redact(&format!("{e:#}")),
                advance: None,
                output: None,
            });
        if let (true, Some(name)) = (out.ok, &st.save) {
            let value = out
                .output
                .as_deref()
                .unwrap_or("")
                .lines()
                .rev()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("")
                .trim()
                .to_string();
            if value.is_empty() {
                out.ok = false;
                out.detail = format!("`save: {name}`: the command printed nothing");
            } else {
                vars.insert(name.clone(), value);
                out.detail = format!("{} (saved as ${{{name}}})", out.detail);
            }
        }
        let after = client.clock()?;
        clock_log.push(json!({ "after_step": idx, "virtual": format_utc(after.virtual_ns), "generation": after.generation, "frozen": after.frozen }));
        if !json_out {
            println!(
                "   {} {}{}\n",
                if out.ok { "✓" } else { "✗ FAILED" },
                out.detail,
                if st.kind() == "advance" {
                    String::new()
                } else {
                    format!(" ({:.1}s)", s0.elapsed().as_secs_f64())
                }
            );
        }
        let mut entry = json!({
            "index": idx, "name": title, "kind": st.kind(),
            "status": if out.ok { "passed" } else { "failed" },
            "virtual_before": format_utc(before.virtual_ns), "virtual_after": format_utc(after.virtual_ns),
            "elapsed_ms": s0.elapsed().as_millis(), "detail": out.detail,
        });
        if let Some(a) = out.advance {
            entry["advance"] = a;
        }
        if !out.ok {
            failed = Some(
                json!({ "index": idx, "name": title, "virtual_time": format_utc(after.virtual_ns), "detail": out.detail }),
            );
        }
        steps.push(entry);
    }

    let end = client.clock()?;
    let users_report = users.take().map(|u| u.finish());
    if let (None, Some((r, _))) = (&failed, &users_report) {
        if !r.passed() {
            let first = r
                .steps
                .iter()
                .find_map(|s| {
                    s.errors
                        .first()
                        .map(|e| format!("{} / {}: {}", s.journey, s.name, redact(e)))
                })
                .unwrap_or_default();
            failed = Some(json!({
                "index": Value::Null, "name": "simulated users",
                "virtual_time": format_utc(end.virtual_ns),
                "detail": format!("{} user(s) failed; first error: {first}", r.users.failed),
            }));
        }
    }
    let probes = probe::probe_all(session, client);
    let ev = client.events(first_seq).ok();
    let events: Vec<Value> = ev
        .as_ref()
        .map(|e| {
            e.events
                .iter()
                .map(|x| {
                    let mut v = serde_json::to_value(x).unwrap_or_default();
                    v["virtual_time"] = Value::String(format_utc(x.virtual_ns));
                    if let Some(p) = x.path.as_deref() {
                        v["path"] = Value::String(redact(p));
                    }
                    v
                })
                .collect()
        })
        .unwrap_or_default();
    let faults: Vec<Value> = client
        .faults()
        .map(|l| {
            l.faults
                .iter()
                .map(|f| json!({ "id": f.id, "target": f.target, "from": format_utc(f.from_ns), "until": format_utc(f.to_ns), "status": f.status }))
                .collect()
        })
        .unwrap_or_default();
    let passed = failed.is_none();
    let report = json!({
        "report_version": 1,
        "chronostack_version": env!("CARGO_PKG_VERSION"),
        "scenario": { "name": sc.name, "description": sc.description, "path": path.to_string_lossy().replace('\\', "/") },
        "run_id": run_id,
        "result": if passed { "passed" } else { "failed" },
        "failed_step": failed,
        "project": session.project,
        "real_start": format_utc(real_start),
        "real_elapsed_ms": t0.elapsed().as_millis(),
        "virtual_start": format_utc(start.virtual_ns),
        "virtual_end": format_utc(end.virtual_ns),
        "steps": steps,
        "clock_log": clock_log,
        "events": events,
        "events_truncated": ev.as_ref().is_some_and(|e| e.truncated),
        "faults": faults,
        "probes": probes,
    });
    let mut report = report;
    if let Some((r, v)) = &users_report {
        report["users"] = v.clone();
        if !json_out {
            println!(
                "simulated users ({} action(s) at {} stop(s)):\n",
                v["actions"], v["stops"]
            );
            print!("{}", crate::users::table(r));
            println!();
        }
    }
    let md = report::markdown(&report);
    let (jp, mp) = report::write(&report, &md, &sc.name, real_start)?;
    if let Some(j) = &opts.junit {
        report::write_junit(&report, j)
            .with_context(|| format!("writing JUnit report {}", j.display()))?;
    }
    if json_out {
        println!("{report}");
    } else {
        println!(
            "{} — {}/{} steps passed, {:.1}s real time, virtual {} → {}",
            if passed { "PASSED" } else { "FAILED" },
            steps.iter().filter(|s| s["status"] == "passed").count(),
            total,
            t0.elapsed().as_secs_f64(),
            format_utc(start.virtual_ns),
            format_utc(end.virtual_ns)
        );
        if let Some(f) = report["failed_step"].as_object() {
            println!(
                "failed at step {}: {}\n  {}",
                f["index"],
                f["name"].as_str().unwrap_or(""),
                f["detail"].as_str().unwrap_or("")
            );
        }
        println!("report: {}\n        {}", mp.display(), jp.display());
        if let Some(j) = &opts.junit {
            println!("        {}", j.display());
        }
    }
    Ok(if passed { 0 } else { EXIT_SCENARIO_FAILED })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config;

    fn cfg() -> Resolved {
        let c: config::Config = serde_norway::from_str(
            "version: 1\ntriggers: [{name: expire, exec: {service: worker, command: [x]}}]\nproxies: [{name: hooks, upstream: 'http://r:1'}]\n",
        )
        .unwrap();
        config::validate(c, &[]).unwrap()
    }

    fn parse(y: &str) -> Result<Scenario> {
        let sc = super::parse(y)?;
        validate(&sc, &cfg(), &["db".into(), "worker".into(), "cache".into()])?;
        Ok(sc)
    }

    #[test]
    fn full_scenario() {
        let sc = parse(
            r#"
version: 1
name: trial
steps:
  - advance: { to: next 00:00 }
  - name: sign up
    exec: { service: worker, command: [node, x.js] }
  - fault: { target: hooks, from: +14d2h, for: 4h }
  - advance: 14d
  - advance: { by: 12h, step: 1h }
  - trigger: expire
  - assert: { sql: { service: db, query: "select plan from users", equals: free } }
  - wait_for: { redis: { service: cache, command: [GET, k], equals: "1" }, timeout: 10s }
  - http: { url: "http://localhost:1/x", status: 204 }
  - fault_clear: all
  - sleep: 1s
"#,
        )
        .unwrap();
        let titles: Vec<String> = sc.steps.iter().map(Step::title).collect();
        assert_eq!(titles[0], "advance to next 00:00");
        assert_eq!(titles[1], "sign up");
        assert_eq!(titles[2], "outage on hooks from +14d2h for 4h");
        assert_eq!(titles[3], "advance 14d");
        assert_eq!(titles[4], "advance 12h --step 1h");
        assert_eq!(
            titles[6],
            "assert sql on db: `select plan from users` = `free`"
        );
        assert_eq!(titles[7], "wait for redis on cache: GET k = `1`");
    }

    #[test]
    fn variables() {
        let sc = parse(
            "version: 1\nname: x\nsteps:\n  - exec: { service: db, command: [psql, '${run_id}'] }\n    save: user_id\n  - assert: { sql: { service: db, query: \"select '${user_id}'\", equals: '${user_id}' } }\n",
        )
        .unwrap();
        let vars = std::collections::BTreeMap::from([
            ("run_id".to_string(), "ab12cd34".to_string()),
            ("user_id".to_string(), "7".to_string()),
        ]);
        let s0: Step = serde_json::from_value(substitute(&sc.raw_steps[0], &vars)).unwrap();
        assert_eq!(s0.exec.unwrap().command, vec!["psql", "ab12cd34"]);
        let s1: Step = serde_json::from_value(substitute(&sc.raw_steps[1], &vars)).unwrap();
        let sql = s1.check.unwrap().sql.unwrap();
        assert_eq!(
            (sql.query.as_str(), sql.equals.as_deref()),
            ("select '7'", Some("7"))
        );
        assert!(
            sc.steps[1].title().contains("${user_id}"),
            "titles keep the ${{…}} form"
        );
    }

    #[test]
    fn advance_to_a_saved_variable() {
        let y = "version: 1
name: x
steps:
  - exec: { service: db, command: [x] }
    save: ends_at
  - advance: { to: \"${ends_at}\" }
";
        let sc = parse(y).expect("a saved variable is accepted as an advance target");
        let vars = std::collections::BTreeMap::from([(
            "ends_at".to_string(),
            "2026-12-14T17:10:00Z".to_string(),
        )]);
        let st: Step = serde_json::from_value(substitute(&sc.raw_steps[1], &vars)).unwrap();
        let Some(AdvanceStep::Full(f)) = st.advance else {
            panic!("expected a full advance step")
        };
        assert!(fault::parse_when(f.to.as_deref().unwrap(), 0).is_ok());
        // Still rejected: a variable nothing defines, and a malformed literal.
        for (y, want) in [
            (
                "version: 1
name: x
steps: [{advance: {to: '${nope}'}}]",
                "`${nope}` is not defined",
            ),
            (
                "version: 1
name: x
steps: [{advance: {to: 'tomorrow'}}]",
                "expected `now`",
            ),
        ] {
            let err = format!("{:#}", parse(y).unwrap_err());
            assert!(err.contains(want), "`{y}` gave `{err}`, wanted `{want}`");
        }
    }

    #[test]
    fn rejects_mistakes() {
        let cases = [
            ("version: 1\nname: x\nsteps: []", "empty"),
            (
                "version: 1\nname: x\nsteps: [{advance: 1d, trigger: expire}]",
                "exactly one",
            ),
            (
                "version: 1\nname: x\nsteps: [{trigger: nope}]",
                "no trigger named",
            ),
            (
                "version: 1\nname: x\nsteps: [{fault: {target: nope, for: 1h}}]",
                "no proxy named",
            ),
            (
                "version: 1\nname: x\nsteps: [{fault: {target: hooks}}]",
                "exactly one of `for`",
            ),
            (
                "version: 1\nname: x\nsteps: [{assert: {sql: {service: nope, query: x}}}]",
                "unknown service",
            ),
            (
                "version: 1\nname: x\nsteps: [{assert: {sql: {service: db, query: x}, timeout: 1s}}]",
                "belongs to wait_for",
            ),
            (
                "version: 1\nname: x\nsteps: [{advance: {by: 1d, to: '+1d'}}]",
                "exactly one of `by`",
            ),
            ("version: 1\nname: x\nsteps: [{sleep: 1h}]", "at most 10m"),
            ("version: 1\nname: x\nsteps: [{advance: banana}]", "step 1"),
            (
                "version: 1\nname: x\nsteps: [{exec: {service: db, command: ['${nope}']}}]",
                "`${nope}` is not defined",
            ),
            (
                "version: 1\nname: x\nsteps: [{exec: {service: db, command: ['${later}']}}, {exec: {service: db, command: [x]}, save: later}]",
                "`${later}` is not defined",
            ),
            (
                "version: 1\nname: x\nsteps: [{sleep: 1s, save: x}]",
                "only on exec",
            ),
            (
                "version: 1\nname: x\nsteps: [{exec: {service: db, command: [x]}, save: run_id}]",
                "new name",
            ),
        ];
        for (y, want) in cases {
            let err = format!("{:#}", parse(y).unwrap_err());
            assert!(err.contains(want), "`{y}` gave `{err}`, wanted `{want}`");
        }
    }

    #[test]
    fn horizon_cap_counts_from_the_scenario_start() {
        const D: i64 = 86_400_000_000_000;
        let step = |y: &str| -> Step { serde_norway::from_str(y).unwrap() };
        let start = 1_000 * D;
        // 10d from the start: allowed under a 30d cap.
        assert!(check_horizon(&step("advance: 10d"), start, start, 30 * D).is_ok());
        // Already 25d in, another 10d ends 35d past the start: refused.
        let e = check_horizon(&step("advance: 10d"), start + 25 * D, start, 30 * D).unwrap_err();
        assert!(e.to_string().contains("mcp.max_advance"), "{e}");
        // `by` and `to` forms.
        assert!(check_horizon(&step("advance: {by: 100y}"), start, start, 30 * D).is_err());
        assert!(check_horizon(&step("advance: {to: +29d}"), start, start, 30 * D).is_ok());
        assert!(check_horizon(&step("advance: {to: +31d}"), start, start, 30 * D).is_err());
        // Exactly at the cap is allowed.
        assert!(check_horizon(&step("advance: 30d"), start, start, 30 * D).is_ok());
        // Non-advance steps are not affected.
        assert!(check_horizon(&step("sleep: 1s"), start, start, 0).is_ok());
    }
}
