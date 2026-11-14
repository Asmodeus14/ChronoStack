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
}

/// Limits for `chronostack mcp` (AI agents).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpCfg {
    /// Largest single `clock_advance` an agent may request (default 400d).
    pub max_advance: Option<String>,
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
    pub timeout: Option<String>,
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
}

pub const DEFAULT_MCP_MAX_ADVANCE_NS: i64 = 400 * 86_400_000_000_000;

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
            },
            max_jump_ns: None,
            dwell_ns: None,
            schedules: vec![],
            trigger_timeouts_ns: vec![],
            barrier_timeouts_ns: vec![],
            mcp_max_advance_ns: DEFAULT_MCP_MAX_ADVANCE_NS,
        }
    }
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
            (None, Some(_)) => {}
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
        ];
        if set.iter().filter(|x| **x).count() != 1 {
            bail!("{what}: set exactly one of `sql`, `redis`, `exec`, `http`");
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
    Ok(Resolved {
        path: None,
        mcp_max_advance_ns,
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
        ];
        for (y, want) in cases {
            let err = format!("{:#}", parse(y).unwrap_err());
            assert!(err.contains(want), "`{y}` gave `{err}`, wanted `{want}`");
        }
    }
}
