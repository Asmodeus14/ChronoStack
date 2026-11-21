//! `chronostack.yml`: triggers, barriers and advance settings.
//!
//! ```yaml
//! version: 1
//! advance:
//!   max_jump: 1h          # default: 1h if pg_cron is detected, otherwise unlimited
//!   dwell: 1s             # wait after a step/sub-step when no barrier is configured
//!   barrier_timeout: 30s
//! triggers:
//!   - name: expire-trials
//!     schedule: { every: 1d, at: "03:00" }      # UTC; omit `schedule` to run after every --step
//!     exec: { service: worker, command: [node, src/jobs/expire-trials.js] }
//! barriers:
//!   - name: jobs-done
//!     bullmq: { service: cache, queues: [emails] }   # or sidekiq: { service: cache }
//!   - name: queue-drained
//!     redis: { service: cache, command: [LLEN, jobs], equals: "0" }
//! proxies:                                     # HTTP fault proxy (outage windows)
//!   - name: webhooks
//!     upstream: http://receiver:4000
//!     env: { worker: WEBHOOK_URL }             # service: variable pointed at the proxy
//! ```

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chronostack_core::duration::{parse_duration, parse_time_of_day};
use chronostack_core::schedule::Every;
use serde::Deserialize;

pub const FILE_NAMES: [&str; 2] = ["chronostack.yml", "chronostack.yaml"];

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    #[serde(default)]
    pub advance: AdvanceCfg,
    #[serde(default)]
    pub triggers: Vec<Trigger>,
    #[serde(default)]
    pub barriers: Vec<Barrier>,
    #[serde(default)]
    pub proxies: Vec<ProxyCfg>,
    #[serde(default)]
    pub mcp: McpCfg,
    /// Container ports of the copy to publish on 127.0.0.1 (`app:3000`, or
    /// `{service: app, port: 3000, host_port: 8080}` for a fixed host port).
    #[serde(default)]
    pub expose: Vec<ExposeCfg>,
}

/// One `expose:` entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ExposeCfg {
    Short(String),
    Long(ExposeLong),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExposeLong {
    pub service: String,
    pub port: u16,
    pub host_port: Option<u16>,
}

/// Limits for `chronostack mcp` (AI agents).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpCfg {
    /// Largest single `clock_advance` an agent may request, and the largest
    /// total advance of a `scenario_run` (default 400d).
    pub max_advance: Option<String>,
    /// Longest real time one MCP tool call may run before it is stopped
    /// (default 10m).
    pub max_wall: Option<String>,
    /// Most simulated users one `users_run` may start (default 1000).
    pub max_users: Option<u64>,
}

/// An HTTP egress target routed through the `clockd` fault proxy.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProxyCfg {
    pub name: String,
    /// `http://host:port` the requests really go to (inside the Compose network).
    pub upstream: String,
    /// `service: VARIABLE` pairs: each variable (a URL starting with `upstream`)
    /// is rewritten to point at the proxy in the ChronoStack copy.
    #[serde(default)]
    pub env: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdvanceCfg {
    pub max_jump: Option<String>,
    pub dwell: Option<String>,
    pub barrier_timeout: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleCfg {
    pub every: String,
    pub at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecSpec {
    pub service: String,
    pub command: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HttpSpec {
    pub url: String,
    #[serde(default = "default_post")]
    pub method: String,
    /// Expected status; default: any 2xx.
    pub status: Option<u16>,
    /// Request body (sent as `application/json` unless `content_type` is set).
    pub body: Option<String>,
    pub content_type: Option<String>,
}

fn default_post() -> String {
    "POST".into()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SqlSpec {
    pub service: String,
    /// Without `equals`: must return a single boolean-ish value (`t`, `true`, `1` = satisfied).
    pub query: String,
    /// Expected value of the (single-cell) result, compared as text.
    pub equals: Option<String>,
    pub user: Option<String>,
    pub database: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedisSpec {
    pub service: String,
    pub command: Vec<String>,
    pub equals: String,
}

/// Satisfied when these BullMQ (or Bull) queues have nothing waiting, running,
/// prioritized, or delayed and already due.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BullmqSpec {
    /// The Redis service the queues live in.
    pub service: String,
    pub queues: Vec<String>,
    /// The queues' key prefix (BullMQ's `prefix` option).
    #[serde(default = "default_bullmq_prefix")]
    pub prefix: String,
}

fn default_bullmq_prefix() -> String {
    "bull".into()
}

/// Satisfied when Sidekiq has nothing queued, no scheduled or retry job that is
/// already due, and no job running.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SidekiqSpec {
    /// The Redis service Sidekiq uses.
    pub service: String,
    /// A `redis-namespace` prefix, if the app uses one (keys look like `<namespace>:queue:default`).
    pub namespace: Option<String>,
}

/// A Celery barrier (Redis broker): the queues are empty and, with `worker`,
/// the worker has nothing running, prefetched, or scheduled and already due.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CelerySpec {
    /// The Redis service Celery uses as its broker.
    pub service: String,
    /// Queue names (default `[celery]`).
    #[serde(default = "default_celery_queues")]
    pub queues: Vec<String>,
    /// Also ask a worker (Celery's `inspect`) what it is running. Without it,
    /// a task that has already been taken off the queue isn't seen.
    pub worker: Option<CeleryWorker>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CeleryWorker {
    /// The service running `celery worker`.
    pub service: String,
    /// The app, as given to `celery -A` (e.g. `proj` or `proj.celery:app`).
    pub app: String,
    /// Restart this worker after every clock jump. Celery holds ETA/countdown
    /// tasks on a real-time timer, so a jump past their time doesn't run them;
    /// a restarted worker gets them back from the broker and runs the due ones.
    #[serde(default)]
    pub restart_on_jump: bool,
}

fn default_celery_queues() -> Vec<String> {
    vec!["celery".into()]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    pub name: String,
    pub schedule: Option<ScheduleCfg>,
    pub exec: Option<ExecSpec>,
    pub http: Option<HttpSpec>,
    pub timeout: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Barrier {
    pub name: String,
    pub sql: Option<SqlSpec>,
    pub redis: Option<RedisSpec>,
    pub exec: Option<ExecSpec>,
    pub http: Option<HttpSpec>,
    #[serde(default)]
    pub bullmq: Option<BullmqSpec>,
    #[serde(default)]
    pub sidekiq: Option<SidekiqSpec>,
    #[serde(default)]
    pub celery: Option<CelerySpec>,
    pub timeout: Option<String>,
    /// Wait this long at each stop before the first check, for schedulers that
    /// only enqueue work when they next poll (sidekiq-cron, Clockwork, …).
    pub settle: Option<String>,
}

/// Validated, ready-to-use settings.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub path: Option<PathBuf>,
    pub config: Config,
    pub max_jump_ns: Option<i64>,
    pub dwell_ns: Option<i64>,
    /// `(trigger index, recurrence)` for scheduled triggers.
    pub schedules: Vec<(usize, Every)>,
    pub trigger_timeouts_ns: Vec<i64>,
    pub barrier_timeouts_ns: Vec<i64>,
    /// `mcp.max_advance` (default 400 days).
    pub mcp_max_advance_ns: i64,
    /// `mcp.max_wall` (default 10 minutes).
    pub mcp_max_wall_ns: i64,
    /// `mcp.max_users` (default 1000).
    pub mcp_max_users: u64,
    /// `expose:` entries, validated.
    pub expose: Vec<chronostack_core::compose::Expose>,
}

pub const DEFAULT_MCP_MAX_ADVANCE_NS: i64 = 400 * 86_400_000_000_000;
pub const DEFAULT_MCP_MAX_WALL_NS: i64 = 600_000_000_000;
pub const DEFAULT_MCP_MAX_USERS: u64 = 1000;

const DEFAULT_TIMEOUT_NS: i64 = 60_000_000_000;
const DEFAULT_BARRIER_TIMEOUT_NS: i64 = 30_000_000_000;

fn dur(field: &str, v: &str) -> Result<i64> {
    parse_duration(v).map_err(|e| anyhow::anyhow!("{field}: {e}"))
}

impl Resolved {
    pub fn empty() -> Resolved {
        Resolved {
            path: None,
            config: Config {
                version: 1,
                advance: AdvanceCfg::default(),
                triggers: vec![],
                barriers: vec![],
                proxies: vec![],
                mcp: McpCfg::default(),
                expose: vec![],
            },
            max_jump_ns: None,
            dwell_ns: None,
            schedules: vec![],
            trigger_timeouts_ns: vec![],
            barrier_timeouts_ns: vec![],
            mcp_max_advance_ns: DEFAULT_MCP_MAX_ADVANCE_NS,
            mcp_max_wall_ns: DEFAULT_MCP_MAX_WALL_NS,
            mcp_max_users: DEFAULT_MCP_MAX_USERS,
            expose: vec![],
        }
    }
}

/// Validate `expose:`: known services, real ports, no duplicates.
fn check_expose(
    entries: &[ExposeCfg],
    check_service: &dyn Fn(&str, &str) -> Result<()>,
) -> Result<Vec<chronostack_core::compose::Expose>> {
    let mut out: Vec<chronostack_core::compose::Expose> = Vec::new();
    for entry in entries {
        let e = match entry {
            ExposeCfg::Short(s) => {
                let (service, port) =
                    crate::expose::parse_target(s).map_err(|e| anyhow::anyhow!("expose: {e}"))?;
                chronostack_core::compose::Expose {
                    service,
                    port,
                    host_port: None,
                }
            }
            ExposeCfg::Long(l) => {
                if l.port == 0 || l.host_port == Some(0) {
                    bail!("expose `{}`: ports must be 1-65535", l.service);
                }
                chronostack_core::compose::Expose {
                    service: l.service.clone(),
                    port: l.port,
                    host_port: l.host_port,
                }
            }
        };
        let what = format!("expose `{}:{}`", e.service, e.port);
        check_service(&what, &e.service)?;
        if out
            .iter()
            .any(|x| x.service == e.service && x.port == e.port)
        {
            bail!("{what}: listed twice");
        }
        if let Some(h) = e.host_port {
            if out.iter().any(|x| x.host_port == Some(h)) {
                bail!("{what}: host port {h} is used by another entry");
            }
        }
        out.push(e);
    }
    Ok(out)
}

/// Find `chronostack.yml` in `dir`.
pub fn find(dir: &Path) -> Option<PathBuf> {
    FILE_NAMES.iter().map(|n| dir.join(n)).find(|p| p.exists())
}

/// Load and validate; `services` = names known to the session (for references).
pub fn load(path: &Path, services: &[String]) -> Result<Resolved> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let config: Config =
        serde_norway::from_str(&text).with_context(|| format!("invalid {}", path.display()))?;
    let mut r =
        validate(config, services).with_context(|| format!("invalid {}", path.display()))?;
    r.path = Some(path.to_path_buf());
    Ok(r)
}

pub fn validate(config: Config, services: &[String]) -> Result<Resolved> {
    if config.version != 1 {
        bail!("unsupported `version: {}` (expected 1)", config.version);
    }
    let check_service = |what: &str, s: &str| -> Result<()> {
        if !services.is_empty() && !services.iter().any(|x| x == s) {
            bail!(
                "{what}: unknown service `{s}` (known: {})",
                services.join(", ")
            );
        }
        Ok(())
    };
    let expose = check_expose(&config.expose, &check_service)?;
    let mut names = std::collections::HashSet::new();
    let mut schedules = Vec::new();
    let mut trigger_timeouts_ns = Vec::new();
    for (i, t) in config.triggers.iter().enumerate() {
        let what = format!("trigger `{}`", t.name);
        if t.name.trim().is_empty() || !names.insert(format!("t:{}", t.name)) {
            bail!("triggers need unique, non-empty names (`{}`)", t.name);
        }
        match (&t.exec, &t.http) {
            (Some(e), None) => {
                check_service(&what, &e.service)?;
                if e.command.is_empty() {
                    bail!("{what}: exec.command is empty");
                }
            }
            (None, Some(h)) => crate::expose::check_url(&what, &h.url, &expose)?,
            _ => bail!("{what}: set exactly one of `exec` or `http`"),
        }
        if let Some(s) = &t.schedule {
            let every = dur(&format!("{what}: schedule.every"), &s.every)?;
            let at = match &s.at {
                Some(a) => {
                    parse_time_of_day(a).map_err(|e| anyhow::anyhow!("{what}: schedule.at: {e}"))?
                }
                None => 0,
            };
            schedules.push((
                i,
                Every::new(every, at).map_err(|e| anyhow::anyhow!("{what}: {e}"))?,
            ));
        }
        trigger_timeouts_ns.push(match &t.timeout {
            Some(v) => dur(&format!("{what}: timeout"), v)?,
            None => DEFAULT_TIMEOUT_NS,
        });
    }
    let barrier_timeout_ns = match &config.advance.barrier_timeout {
        Some(v) => dur("advance.barrier_timeout", v)?,
        None => DEFAULT_BARRIER_TIMEOUT_NS,
    };
    let mut barrier_timeouts_ns = Vec::new();
    for b in &config.barriers {
        let what = format!("barrier `{}`", b.name);
        if b.name.trim().is_empty() || !names.insert(format!("b:{}", b.name)) {
            bail!("barriers need unique, non-empty names (`{}`)", b.name);
        }
        let set = [
            b.sql.is_some(),
            b.redis.is_some(),
            b.exec.is_some(),
            b.http.is_some(),
            b.bullmq.is_some(),
            b.sidekiq.is_some(),
            b.celery.is_some(),
        ];
        if set.iter().filter(|x| **x).count() != 1 {
            bail!(
                "{what}: set exactly one of `sql`, `redis`, `exec`, `http`, `bullmq`, `sidekiq`, `celery`"
            );
        }
        if let Some(s) = &b.sql {
            check_service(&what, &s.service)?;
        }
        if let Some(s) = &b.redis {
            check_service(&what, &s.service)?;
        }
        if let Some(s) = &b.exec {
            check_service(&what, &s.service)?;
        }
        if let Some(h) = &b.http {
            crate::expose::check_url(&what, &h.url, &expose)?;
        }
        if let Some(q) = &b.bullmq {
            check_service(&what, &q.service)?;
            if q.queues.is_empty() || q.queues.iter().any(|n| n.trim().is_empty()) {
                bail!("{what}: `bullmq.queues` needs at least one non-empty queue name");
            }
            if q.prefix.trim().is_empty() {
                bail!("{what}: `bullmq.prefix` can't be empty");
            }
        }
        if let Some(s) = &b.sidekiq {
            check_service(&what, &s.service)?;
        }
        if let Some(c) = &b.celery {
            check_service(&what, &c.service)?;
            if c.queues.is_empty() || c.queues.iter().any(|q| q.trim().is_empty()) {
                bail!("{what}: `celery.queues` needs at least one non-empty queue name");
            }
            if let Some(w) = &c.worker {
                check_service(&format!("{what}: celery.worker"), &w.service)?;
                if w.app.trim().is_empty() {
                    bail!(
                        "{what}: `celery.worker.app` can't be empty (the name given to `celery -A`)"
                    );
                }
            }
        }
        if let Some(v) = &b.settle {
            dur(&format!("{what}: settle"), v)?;
        }
        barrier_timeouts_ns.push(match &b.timeout {
            Some(v) => dur(&format!("{what}: timeout"), v)?,
            None => barrier_timeout_ns,
        });
    }
    let mut proxy_names = std::collections::HashSet::new();
    if config.proxies.len() > chronostack_core::api::MAX_PROXY_TARGETS {
        bail!(
            "at most {} proxies",
            chronostack_core::api::MAX_PROXY_TARGETS
        );
    }
    for px in &config.proxies {
        let what = format!("proxy `{}`", px.name);
        let valid_name = !px.name.is_empty()
            && px
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        if !valid_name || !proxy_names.insert(px.name.clone()) {
            bail!(
                "proxies need unique names made of letters, digits, `-` and `_` (`{}`)",
                px.name
            );
        }
        chronostack_core::compose::check_upstream(&px.upstream)
            .map_err(|e| anyhow::anyhow!("{what}: {e}"))?;
        for (svc, var) in &px.env {
            check_service(&format!("{what}: env"), svc)?;
            if var.trim().is_empty() {
                bail!("{what}: env.{svc}: empty variable name");
            }
        }
    }
    let max_jump_ns = config
        .advance
        .max_jump
        .as_deref()
        .map(|v| dur("advance.max_jump", v))
        .transpose()?;
    let dwell_ns = config
        .advance
        .dwell
        .as_deref()
        .map(|v| dur("advance.dwell", v))
        .transpose()?;
    let mcp_max_advance_ns = match &config.mcp.max_advance {
        Some(v) => dur("mcp.max_advance", v)?,
        None => DEFAULT_MCP_MAX_ADVANCE_NS,
    };
    let mcp_max_wall_ns = match &config.mcp.max_wall {
        Some(v) => dur("mcp.max_wall", v)?,
        None => DEFAULT_MCP_MAX_WALL_NS,
    };
    if mcp_max_wall_ns <= 0 {
        bail!("mcp.max_wall must be positive");
    }
    let mcp_max_users = config.mcp.max_users.unwrap_or(DEFAULT_MCP_MAX_USERS);
    if mcp_max_users == 0 {
        bail!("mcp.max_users must be at least 1");
    }
    Ok(Resolved {
        path: None,
        expose,
        mcp_max_users,
        mcp_max_advance_ns,
        mcp_max_wall_ns,
        config,
        max_jump_ns,
        dwell_ns,
        schedules,
        trigger_timeouts_ns,
        barrier_timeouts_ns,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(y: &str) -> Result<Resolved> {
        let c: Config = serde_norway::from_str(y)?;
        validate(c, &["worker".into(), "db".into(), "cache".into()])
    }

    #[test]
    fn mcp_limits() {
        let r = parse("version: 1\n").unwrap();
        assert_eq!(r.mcp_max_advance_ns, DEFAULT_MCP_MAX_ADVANCE_NS);
        assert_eq!(r.mcp_max_wall_ns, DEFAULT_MCP_MAX_WALL_NS);
        let r = parse("version: 1\nmcp: { max_advance: 30d, max_wall: 90s }\n").unwrap();
        assert_eq!(r.mcp_max_advance_ns, 30 * 86_400_000_000_000);
        assert_eq!(r.mcp_max_wall_ns, 90_000_000_000);
        assert!(parse("version: 1\nmcp: { max_wall: 0s }\n").is_err());
        assert!(parse("version: 1\nmcp: { max_wall: soon }\n").is_err());
    }

    #[test]
    fn expose_entries() {
        let r = parse(
            "version: 1\nexpose:\n  - worker:3000\n  - { service: db, port: 5432, host_port: 15432 }\n",
        )
        .unwrap();
        let got: Vec<_> = r
            .expose
            .iter()
            .map(|e| (e.service.as_str(), e.port, e.host_port))
            .collect();
        assert_eq!(got, vec![("worker", 3000, None), ("db", 5432, Some(15432))]);
        for bad in [
            "expose: [nope:3000]",
            "expose: [worker]",
            "expose: [worker:0]",
            "expose: [worker:3000, worker:3000]",
            "expose: [{service: worker, port: 1, host_port: 9000}, {service: db, port: 2, host_port: 9000}]",
            "expose: [{service: worker, port: 1, extra: x}]",
        ] {
            assert!(parse(&format!("version: 1\n{bad}\n")).is_err(), "{bad}");
        }
    }

    #[test]
    fn celery_barriers() {
        let r = parse(
            "version: 1\nbarriers:\n  - name: c\n    celery: { service: cache, worker: { service: worker, app: proj } }\n",
        )
        .unwrap();
        let c = r.config.barriers[0].celery.as_ref().unwrap();
        assert_eq!(c.queues, ["celery"], "default queue");
        assert_eq!(c.worker.as_ref().unwrap().app, "proj");
        assert!(
            !c.worker.as_ref().unwrap().restart_on_jump,
            "off by default"
        );
        let r = parse(
            "version: 1\nbarriers:\n  - name: c\n    celery: { service: cache, worker: { service: worker, app: proj, restart_on_jump: true } }\n",
        )
        .unwrap();
        assert!(
            r.config.barriers[0]
                .celery
                .as_ref()
                .unwrap()
                .worker
                .as_ref()
                .unwrap()
                .restart_on_jump
        );
        for bad in [
            "celery: { service: nope }",
            "celery: { service: cache, queues: [] }",
            "celery: { service: cache, worker: { service: nope, app: proj } }",
            "celery: { service: cache, worker: { service: worker, app: ' ' } }",
            "celery: { service: cache }\n    sql: { service: db, query: 'select 1' }",
        ] {
            assert!(
                parse(&format!("version: 1\nbarriers:\n  - name: c\n    {bad}\n")).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn service_urls_must_be_exposed() {
        let trig = "triggers:\n  - name: tick\n    http: { url: \"service://worker:3000/tick\" }\n";
        let err = parse(&format!("version: 1\n{trig}"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs `worker:3000` in `expose:`"), "{err}");
        assert!(parse(&format!("version: 1\nexpose: [worker:3000]\n{trig}")).is_ok());
        let barrier = "barriers:\n  - name: ready\n    http: { url: \"service://db:80/\" }\n";
        assert!(parse(&format!("version: 1\nexpose: [worker:3000]\n{barrier}")).is_err());
    }

    #[test]
    fn full_example() {
        let r = parse(
            r#"
version: 1
advance: { max_jump: 1h, dwell: 500ms, barrier_timeout: 10s }
triggers:
  - name: expire-trials
    schedule: { every: 1d, at: "03:00" }
    exec: { service: worker, command: [node, jobs/expire.js] }
  - name: tick
    http: { url: "http://localhost:8080/tick" }
barriers:
  - name: idle
    sql: { service: db, query: "select count(*) = 0 from jobs" }
  - name: drained
    redis: { service: cache, command: [LLEN, q], equals: "0" }
"#,
        )
        .unwrap();
        assert_eq!(r.schedules.len(), 1);
        assert_eq!(r.schedules[0].1.at_ns, 3 * 3_600_000_000_000);
        assert_eq!(r.max_jump_ns, Some(3_600_000_000_000));
        assert_eq!(r.config.triggers[1].http.as_ref().unwrap().method, "POST");
        assert_eq!(r.barrier_timeouts_ns, vec![10_000_000_000; 2]);
    }

    #[test]
    fn queue_barriers() {
        let r = parse(
            r#"
version: 1
barriers:
  - name: jobs
    bullmq: { service: cache, queues: [emails, cron-queue] }
  - name: custom-prefix
    bullmq: { service: cache, queues: [background-jobs], prefix: "{app:jobs}" }
  - name: sidekiq
    sidekiq: { service: cache }
    settle: 3s
  - name: namespaced
    sidekiq: { service: cache, namespace: myapp }
"#,
        )
        .unwrap();
        let b = &r.config.barriers;
        assert_eq!(b[0].bullmq.as_ref().unwrap().prefix, "bull");
        assert_eq!(b[1].bullmq.as_ref().unwrap().prefix, "{app:jobs}");
        assert_eq!(b[2].settle.as_deref(), Some("3s"));
        assert_eq!(
            b[3].sidekiq.as_ref().unwrap().namespace.as_deref(),
            Some("myapp")
        );
    }

    #[test]
    fn proxies() {
        let r = parse(
            "version: 1\nproxies:\n  - name: hooks\n    upstream: http://receiver:4000\n    env: { worker: WEBHOOK_URL }\n",
        )
        .unwrap();
        assert_eq!(r.config.proxies[0].env["worker"], "WEBHOOK_URL");
        for (y, want) in [
            (
                "version: 1\nproxies: [{name: 'a b', upstream: 'http://r'}]",
                "unique names",
            ),
            (
                "version: 1\nproxies: [{name: a, upstream: 'http://r'}, {name: a, upstream: 'http://r'}]",
                "unique names",
            ),
            (
                "version: 1\nproxies: [{name: a, upstream: 'ftp://r'}]",
                "http://",
            ),
            (
                "version: 1\nproxies: [{name: a, upstream: 'http://r/x'}]",
                "no path",
            ),
            (
                "version: 1\nproxies: [{name: a, upstream: 'http://r', env: {nope: X}}]",
                "unknown service",
            ),
        ] {
            let err = format!("{:#}", parse(y).unwrap_err());
            assert!(err.contains(want), "`{y}` gave `{err}`, wanted `{want}`");
        }
    }

    #[test]
    fn rejects_mistakes() {
        let cases = [
            ("version: 2", "unsupported"),
            ("version: 1\nunknown: 1", "unknown field"),
            ("version: 1\ntriggers: [{name: a}]", "exactly one"),
            (
                "version: 1\ntriggers: [{name: a, exec: {service: nope, command: [x]}}]",
                "unknown service",
            ),
            (
                "version: 1\ntriggers: [{name: a, exec: {service: worker, command: []}}]",
                "empty",
            ),
            (
                "version: 1\ntriggers: [{name: a, schedule: {every: 1h, at: '03:00'}, exec: {service: worker, command: [x]}}]",
                "inside one",
            ),
            (
                "version: 1\ntriggers: [{name: a, exec: {service: worker, command: [x]}}, {name: a, exec: {service: worker, command: [x]}}]",
                "unique",
            ),
            (
                "version: 1\nbarriers: [{name: b, sql: {service: db, query: x}, redis: {service: cache, command: [x], equals: '1'}}]",
                "exactly one",
            ),
            ("version: 1\nadvance: {max_jump: 0s}", "greater than zero"),
            (
                "version: 1\nbarriers: [{name: b, bullmq: {service: cache, queues: []}}]",
                "at least one",
            ),
            (
                "version: 1\nbarriers: [{name: b, bullmq: {service: nope, queues: [q]}}]",
                "unknown service",
            ),
            (
                "version: 1\nbarriers: [{name: b, sidekiq: {service: cache}, settle: soon}]",
                "settle",
            ),
            (
                "version: 1\nbarriers: [{name: b, sidekiq: {service: cache}, bullmq: {service: cache, queues: [q]}}]",
                "exactly one",
            ),
        ];
        for (y, want) in cases {
            let err = format!("{:#}", parse(y).unwrap_err());
            assert!(err.contains(want), "`{y}` gave `{err}`, wanted `{want}`");
        }
    }
}
