//! `chronostack mcp`: a Model Context Protocol server over stdio (ADR-011).
//!
//! Supports both protocol eras:
//! - **2025-11-25 / 2025-06-18 / 2025-03-26** ("legacy"): `initialize` →
//!   `notifications/initialized` → `tools/list` / `tools/call`, plus `ping`;
//! - **2026-07-28**: `server/discover` and per-request `_meta`
//!   (`io.modelcontextprotocol/protocolVersion`, `…/clientCapabilities`).
//!
//! Every tool runs this same binary as a subprocess with `--json`, so agents get
//! exactly the CLI's engine and behaviour, and nothing but MCP messages ever
//! reaches stdout. The tool set is fixed: no arbitrary commands, only the
//! triggers declared in `chronostack.yml`; `clock_advance` and the total advance
//! of `scenario_run` are capped by `mcp.max_advance` (default 400 days); every
//! tool call is stopped after `mcp.max_wall` of real time (default 10 minutes);
//! `users_run` is limited to `mcp.max_users` users (default 1000); scenario and
//! journey files must be inside the project directory. `report_get` and
//! `report_diff` only read `.chronostack/reports/`.

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use anyhow::Result;
use chronostack_core::duration::{format_span, format_utc, parse_duration};
use serde_json::{Map, Value, json};

use crate::client::Client;
use crate::config::{DEFAULT_MCP_MAX_ADVANCE_NS, DEFAULT_MCP_MAX_USERS, DEFAULT_MCP_MAX_WALL_NS};
use crate::fault;
use crate::session::Session;

pub const MODERN_VERSION: &str = "2026-07-28";
pub const LEGACY_VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

const INSTRUCTIONS: &str = "ChronoStack moves virtual wall-clock time for a Docker Compose stack \
(databases, caches, workers) so time-dependent behaviour can be tested. Start the stack with \
`chronostack up` first. Typical flow: env_inspect (what's running, triggers, barriers, exposed ports) → \
clock_status → fault_set (optional outage on a proxy target) → clock_advance (runs declared triggers \
and waits on barriers at each stop) → events_tail / probe to inspect → scenario_validate then \
scenario_run for scripted checks (scenarios can include simulated users on the virtual clock) → \
report_get / report_diff to read and compare runs. users_run runs journeys in real time. Time only \
moves forward.";

type RpcError = (i64, String, Option<Value>);

/// Agent limits from the `mcp:` section of `chronostack.yml`.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Largest `clock_advance`, and largest total advance of a `scenario_run`.
    pub max_advance_ns: i64,
    /// Real time one tool call may take before its subprocess is stopped.
    pub max_wall_ns: i64,
    /// Most simulated users a `users_run` may start.
    pub max_users: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_advance_ns: DEFAULT_MCP_MAX_ADVANCE_NS,
            max_wall_ns: DEFAULT_MCP_MAX_WALL_NS,
            max_users: DEFAULT_MCP_MAX_USERS,
        }
    }
}

fn rpc_err(code: i64, msg: impl Into<String>) -> RpcError {
    (code, msg.into(), None)
}

/// `outputSchema` for tools whose `structuredContent` is a JSON object (errors
/// are objects too: `{ "error": … }`).
fn object() -> Value {
    json!({ "type": "object" })
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

fn tools() -> Vec<Value> {
    let none = json!({ "type": "object", "additionalProperties": false });
    let when = "`now`, `+<duration>` (e.g. +2d, +1h30m), `next HH:MM` (UTC) or a UTC time like 2026-12-01T09:00:00Z";
    vec![
        json!({ "name": "clock_status", "title": "Clock status",
            "description": "Current virtual and real time, the offset between them, and whether the clock is frozen.",
            "inputSchema": none, "annotations": { "readOnlyHint": true } }),
        json!({ "name": "clock_advance", "title": "Advance virtual time",
            "description": "Move virtual time forward (never backward). At every stop ChronoStack runs the triggers declared in chronostack.yml (e.g. daily jobs at their virtual time) and waits on the declared barriers, then probes that every service sees the new time. Give exactly one of `duration` or `to`. Returns every stop with trigger and barrier results.",
            "inputSchema": schema(json!({
                "duration": { "type": "string", "description": "How far to move, e.g. 25h, 14d, 1h30m." },
                "to": { "type": "string", "description": format!("Target time: {when}.") },
                "step": { "type": "string", "description": "Optional: also stop every <step> (e.g. 1d) so per-step triggers run." },
                "dry_run": { "type": "boolean", "description": "Only show the planned stops." }
            }), &[]),
            "annotations": { "destructiveHint": false, "idempotentHint": false } }),
        json!({ "name": "clock_freeze", "title": "Freeze virtual time",
            "description": "Stop virtual time where it is (services see a constant wall clock).",
            "inputSchema": none }),
        json!({ "name": "clock_unfreeze", "title": "Unfreeze virtual time",
            "description": "Let virtual time flow again from where it was frozen.",
            "inputSchema": none }),
        json!({ "name": "probe", "title": "Probe services",
            "description": "Check that every virtualized service (Postgres, Redis, app containers) sees the virtual time.",
            "inputSchema": none, "annotations": { "readOnlyHint": true } }),
        json!({ "name": "doctor", "title": "Doctor",
            "description": "Explain which services are virtualized, which are not (e.g. Go binaries) and why.",
            "inputSchema": none, "annotations": { "readOnlyHint": true } }),
        json!({ "name": "fault_list", "title": "List proxy targets and outages",
            "description": "Proxy targets declared in chronostack.yml and the outage windows (virtual time) on them.",
            "inputSchema": none, "annotations": { "readOnlyHint": true } }),
        json!({ "name": "fault_set", "title": "Schedule an outage",
            "description": "Make a proxy target (an HTTP dependency such as a webhook receiver) return an error status during a window of virtual time. Give exactly one of `for` or `until`.",
            "inputSchema": schema(json!({
                "target": { "type": "string", "description": "Proxy target name (see fault_list)." },
                "from": { "type": "string", "description": format!("Start (default now): {when}.") },
                "for": { "type": "string", "description": "Length, e.g. 2h." },
                "until": { "type": "string", "description": format!("End: {when}.") },
                "status": { "type": "integer", "minimum": 400, "maximum": 599, "description": "Status returned during the window (default 503)." }
            }), &["target"]) }),
        json!({ "name": "fault_clear", "title": "Remove outages",
            "description": "Remove one outage window by id, or all of them with all=true.",
            "inputSchema": schema(json!({
                "id": { "type": "integer", "minimum": 1 },
                "all": { "type": "boolean" }
            }), &[]) }),
        json!({ "name": "events_tail", "title": "Event log",
            "description": "Recent controller events: clock changes, outage windows set/cleared, and every request through the fault proxy (forwarded, blocked by a window, or upstream error), with virtual times.",
            "inputSchema": schema(json!({
                "since": { "type": "integer", "minimum": 0, "description": "Only events after this sequence number." },
                "limit": { "type": "integer", "minimum": 1, "maximum": 1000, "description": "Most recent N events (default 50)." }
            }), &[]), "annotations": { "readOnlyHint": true } }),
        json!({ "name": "scenario_run", "title": "Run a scenario",
            "description": "Run a scenario YAML file from the project (steps like advance, fault, assert; optionally simulated users on the virtual clock) and return pass/fail with the failed step and, with users, their outcomes per step. A full report is written to .chronostack/reports/ (see report_get).",
            "inputSchema": schema(json!({
                "file": { "type": "string", "description": "Path inside the project directory, e.g. scenarios/trial.yml." }
            }), &["file"]), "outputSchema": object() }),
        json!({ "name": "env_inspect", "title": "Inspect the environment",
            "description": "One look at what the stack is: every service and whether it sees virtual time (doctor), the triggers with their schedules, the barriers, the fault-proxy targets, and the ports exposed to the host (for service:// URLs). Start here.",
            "inputSchema": none, "outputSchema": object(), "annotations": { "readOnlyHint": true } }),
        json!({ "name": "scenario_validate", "title": "Check a scenario",
            "description": "Check a scenario file (and its users file) without running it: its steps, how far virtual time will move, and the simulated users (cohorts, counts, arrival windows). Says whether it fits mcp.max_advance.",
            "inputSchema": schema(json!({
                "file": { "type": "string", "description": "Path inside the project directory, e.g. scenarios/cohort.yml." }
            }), &["file"]), "outputSchema": object(), "annotations": { "readOnlyHint": true } }),
        json!({ "name": "report_get", "title": "Read a run's report",
            "description": "The report of a scenario run, by run_id or the latest: result, failed step, every step's status and detail, and with simulated users their outcomes, per-step failures and the start of the timeline (who did what at which virtual time).",
            "inputSchema": schema(json!({
                "run_id": { "type": "string", "description": "Run id from scenario_run (default: the latest report)." },
                "timeline": { "type": "integer", "minimum": 0, "maximum": 1000, "description": "How many timeline entries to include (default 50)." }
            }), &[]), "outputSchema": object(), "annotations": { "readOnlyHint": true } }),
        json!({ "name": "report_diff", "title": "Compare two runs",
            "description": "Compare two scenario runs (by run_id; default the latest two): whether the result changed, which steps changed status, user outcome counts, per-step user failures, and whether the simulated users' timeline is the same. Use it to check a fix, or that a run repeats.",
            "inputSchema": schema(json!({
                "a": { "type": "string", "description": "Earlier run id (default: the second-latest report)." },
                "b": { "type": "string", "description": "Later run id (default: the latest report)." }
            }), &[]), "outputSchema": object(), "annotations": { "readOnlyHint": true } }),
        json!({ "name": "users_validate", "title": "Check a journey file",
            "description": "Check a journey file (simulated users) without running it: cohorts, user counts, arrival windows and each journey's steps.",
            "inputSchema": schema(json!({
                "file": { "type": "string", "description": "Path inside the project directory, e.g. journeys/trialists.yml." }
            }), &["file"]), "outputSchema": object(), "annotations": { "readOnlyHint": true } }),
        json!({ "name": "users_run", "title": "Run simulated users (real time)",
            "description": "Run a journey file's simulated users in real time against the stack and return runs, failures and latency per step. Limited to mcp.max_users users; the run stops itself before mcp.max_wall. To run users on the virtual clock (days in seconds), put them in a scenario and use scenario_run.",
            "inputSchema": schema(json!({
                "file": { "type": "string", "description": "Path inside the project directory." },
                "seed": { "type": "integer", "minimum": 0, "description": "Fix every random choice (default: the file's seed or a random one)." },
                "duration": { "type": "string", "description": "Stop after this much real time (e.g. 60s); capped below mcp.max_wall." }
            }), &["file"]), "outputSchema": object() }),
    ]
}

/// Run this binary with `args` (it always ends in `--json`); parse the last JSON line.
fn cli(args: &[String], max_wall: Duration) -> Result<(i32, Value, String)> {
    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.args(args);
    run_bounded(&mut cmd, max_wall)
}

/// Run `cmd`, stopping it after `max_wall`; return (exit code, last JSON line
/// of stdout or null, trimmed stderr).
fn run_bounded(cmd: &mut Command, max_wall: Duration) -> Result<(i32, Value, String)> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    // Drain both pipes on their own threads so a chatty child can't block on a
    // full pipe while we wait for it.
    let drain = |mut r: Box<dyn Read + Send>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = r.read_to_end(&mut buf);
            let _ = tx.send(buf);
        });
        rx
    };
    let out_rx = drain(Box::new(child.stdout.take().expect("piped")));
    let err_rx = drain(Box::new(child.stderr.take().expect("piped")));
    let deadline = Instant::now() + max_wall;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break st;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!(
                "stopped after {} of real time (mcp.max_wall). The clock may have moved part of the way: \
                 check clock_status. Commands it started inside containers may still be running. \
                 Raise mcp.max_wall in chronostack.yml if this call needs longer",
                format_span(i64::try_from(max_wall.as_nanos()).unwrap_or(i64::MAX))
            );
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // The pipes close when the child exits; a grandchild that inherited them
    // could keep them open, so don't wait forever for the rest of the output.
    let grace = Duration::from_secs(5);
    let stdout = out_rx.recv_timeout(grace).unwrap_or_default();
    let stderr = err_rx.recv_timeout(grace).unwrap_or_default();
    let v = String::from_utf8_lossy(&stdout)
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .unwrap_or(Value::Null);
    let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
    Ok((status.code().unwrap_or(1), v, stderr))
}

fn s(v: &str) -> String {
    v.to_string()
}

fn arg_str<'a>(a: &'a Map<String, Value>, k: &str) -> Result<Option<&'a str>, String> {
    match a.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(x)) => Ok(Some(x.as_str())),
        Some(_) => Err(format!("`{k}` must be a string")),
    }
}

/// Result of one tool call: (structured result, is_error).
fn call_tool(name: &str, a: &Map<String, Value>, limits: Limits) -> Result<(Value, bool), String> {
    let max_advance_ns = limits.max_advance_ns;
    let max_wall = Duration::from_nanos(u64::try_from(limits.max_wall_ns).unwrap_or(0));
    let known: &[&str] = match name {
        "clock_advance" => &["duration", "to", "step", "dry_run"],
        "fault_set" => &["target", "from", "for", "until", "status"],
        "fault_clear" => &["id", "all"],
        "events_tail" => &["since", "limit"],
        "scenario_run" | "scenario_validate" | "users_validate" => &["file"],
        "report_get" => &["run_id", "timeline"],
        "report_diff" => &["a", "b"],
        "users_run" => &["file", "seed", "duration"],
        _ => &[],
    };
    if let Some(k) = a.keys().find(|k| !known.contains(&k.as_str())) {
        return Err(format!("unknown argument `{k}` for {name}"));
    }
    let run = |args: Vec<String>| -> Result<(Value, bool), String> {
        let (code, v, err) = cli(&args, max_wall).map_err(|e| e.to_string())?;
        if v.is_null() {
            return Ok((
                json!({ "error": if err.is_empty() { format!("exit code {code}") } else { err } }),
                true,
            ));
        }
        let v = if v.is_object() {
            v
        } else {
            json!({ "result": v })
        };
        Ok((v, code != 0))
    };
    match name {
        "clock_status" => run(vec![s("now"), s("--json")]),
        "clock_freeze" => run(vec![s("freeze"), s("--json")]),
        "clock_unfreeze" => run(vec![s("unfreeze"), s("--json")]),
        "probe" => {
            let (v, err) = run(vec![s("probe"), s("--json")])?;
            // Exit 3 (a service sees the wrong time) is a finding, not a tool failure.
            let failed = err && v.get("error").is_some();
            Ok((v, failed))
        }
        "doctor" => run(vec![s("doctor"), s("--json")]),
        "fault_list" => run(vec![s("fault"), s("list"), s("--json")]),
        "clock_advance" => {
            let duration = arg_str(a, "duration")?;
            let to = arg_str(a, "to")?;
            let session = Session::load().map_err(|e| e.to_string())?;
            let client = Client::new(session.base_url(), session.token.clone());
            let now = client.clock().map_err(|e| e.to_string())?.virtual_ns;
            let (target_ns, mut args) = match (duration, to) {
                (Some(d), None) => {
                    let ns = parse_duration(d)?;
                    (now + ns, vec![s("advance"), d.to_string()])
                }
                (None, Some(t)) => {
                    let at = fault::parse_when(t, now).map_err(|e| e.to_string())?;
                    (at, vec![s("advance"), s("--to"), format_utc(at)])
                }
                _ => return Err("give exactly one of `duration` or `to`".into()),
            };
            if target_ns - now > max_advance_ns {
                return Ok((
                    json!({ "error": format!(
                        "refused: advancing {} is more than mcp.max_advance ({}); raise it in chronostack.yml if intended",
                        chronostack_core::duration::format_span(target_ns - now),
                        chronostack_core::duration::format_span(max_advance_ns)) }),
                    true,
                ));
            }
            if let Some(st) = arg_str(a, "step")? {
                args.extend([s("--step"), st.to_string()]);
            }
            if a.get("dry_run").and_then(Value::as_bool) == Some(true) {
                args.push(s("--dry-run"));
            }
            args.push(s("--json"));
            run(args)
        }
        "fault_set" => {
            let target = arg_str(a, "target")?.ok_or("`target` is required")?;
            let mut args = vec![s("fault"), s("set"), target.to_string()];
            if let Some(f) = arg_str(a, "from")? {
                args.extend([s("--from"), f.to_string()]);
            }
            match (arg_str(a, "for")?, arg_str(a, "until")?) {
                (Some(d), None) => args.extend([s("--for"), d.to_string()]),
                (None, Some(u)) => args.extend([s("--until"), u.to_string()]),
                _ => return Err("give exactly one of `for` or `until`".into()),
            }
            if let Some(st) = a.get("status") {
                let st = st
                    .as_u64()
                    .filter(|v| (400..=599).contains(v))
                    .ok_or("`status` must be 400-599")?;
                args.extend([s("--status"), st.to_string()]);
            }
            args.push(s("--json"));
            run(args)
        }
        "fault_clear" => {
            let all = a.get("all").and_then(Value::as_bool) == Some(true);
            let args = match (a.get("id").and_then(Value::as_u64), all) {
                (Some(id), false) => vec![s("fault"), s("clear"), id.to_string()],
                (None, true) => vec![s("fault"), s("clear"), s("--all")],
                _ => return Err("give exactly one of `id` or `all: true`".into()),
            };
            let (code, _, err) = cli(&args, max_wall).map_err(|e| e.to_string())?;
            if code != 0 {
                return Ok((json!({ "error": err }), true));
            }
            let (v, e) = run(vec![s("fault"), s("list"), s("--json")])?;
            Ok((json!({ "remaining": v }), e))
        }
        "events_tail" => {
            let since = a.get("since").and_then(Value::as_u64).unwrap_or(0);
            let limit = a
                .get("limit")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .clamp(1, 1000) as usize;
            let (mut v, err) = run(vec![
                s("events"),
                s("--since"),
                since.to_string(),
                s("--json"),
            ])?;
            if let Some(ev) = v.get_mut("events").and_then(Value::as_array_mut) {
                let skip = ev.len().saturating_sub(limit);
                ev.drain(..skip);
                for e in ev.iter_mut() {
                    if let Some(ns) = e.get("virtual_ns").and_then(Value::as_i64) {
                        e["virtual_time"] = Value::String(format_utc(ns));
                    }
                }
            }
            Ok((v, err))
        }
        "scenario_run" => {
            let file = arg_str(a, "file")?.ok_or("`file` is required")?;
            let path = inside_project(file)?;
            let (v, err) = run(vec![
                s("run"),
                path,
                s("--max-advance-ns"),
                max_advance_ns.to_string(),
                s("--json"),
            ])?;
            if v.get("steps").is_none() {
                return Ok((v, true));
            }
            let steps: Vec<Value> = v["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|st| json!({ "index": st["index"], "name": st["name"], "status": st["status"], "detail": st["detail"], "virtual_after": st["virtual_after"] }))
                .collect();
            let proxy: Vec<Value> = v["events"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|e| e["kind"] == "proxy")
                .map(|e| json!({ "virtual_time": e["virtual_time"], "target": e["target"], "request": format!("{} {}", e["method"].as_str().unwrap_or(""), e["path"].as_str().unwrap_or("")), "status": e["status"], "outcome": e["outcome"] }))
                .collect();
            let mut out = json!({
                "result": v["result"], "run_id": v["run_id"], "failed_step": v["failed_step"], "steps": steps,
                "proxy_requests": proxy, "virtual_start": v["virtual_start"], "virtual_end": v["virtual_end"],
                "report_dir": ".chronostack/reports",
            });
            if v["users"].is_object() {
                out["users"] = users_summary(&v["users"], 0);
            }
            Ok((out, err))
        }
        "env_inspect" => {
            let (doctor, _) = run(vec![s("doctor"), s("--json")])?;
            let cfg = crate::config::find(Path::new("."))
                .map(|p| crate::config::load(&p, &[]))
                .transpose()
                .map_err(|e| format!("{e:#}"))?;
            let mut out = json!({ "doctor": doctor });
            if let Some(r) = cfg {
                out["triggers"] = Value::Array(
                    r.config
                        .triggers
                        .iter()
                        .enumerate()
                        .map(|(i, t)| {
                            let when = r.schedules.iter().find(|(j, _)| *j == i).map(|(_, e)| {
                                format!(
                                    "every {} at +{} (UTC)",
                                    format_span(e.every_ns).trim_start_matches('+'),
                                    format_span(e.at_ns).trim_start_matches('+')
                                )
                            });
                            json!({ "name": t.name, "schedule": when.unwrap_or_else(|| "after every --step".into()),
                                    "kind": if t.exec.is_some() { "exec" } else { "http" } })
                        })
                        .collect(),
                );
                out["barriers"] = json!(
                    r.config
                        .barriers
                        .iter()
                        .map(|b| b.name.clone())
                        .collect::<Vec<_>>()
                );
                out["proxies"] = Value::Array(
                    r.config
                        .proxies
                        .iter()
                        .map(|p| json!({ "name": p.name, "upstream": p.upstream }))
                        .collect(),
                );
            }
            if let Ok(session) = Session::load() {
                out["exposed"] = Value::Array(
                    session
                        .exposed
                        .iter()
                        .map(|e| json!({ "target": format!("service://{}:{}", e.service, e.port), "url": e.url() }))
                        .collect(),
                );
            }
            Ok((out, false))
        }
        "scenario_validate" => {
            let file = arg_str(a, "file")?.ok_or("`file` is required")?;
            let path = inside_project(file)?;
            let (mut v, err) = run(vec![s("run"), path, s("--check"), s("--json")])?;
            if let Some(total) = v.get("total_advance_ns").and_then(Value::as_i64) {
                let fits = total <= max_advance_ns;
                v["within_mcp_max_advance"] = json!(fits);
                if !fits {
                    v["note"] = json!(format!(
                        "scenario_run would refuse this: it moves {} past its start, more than mcp.max_advance ({})",
                        format_span(total).trim_start_matches('+'),
                        format_span(max_advance_ns).trim_start_matches('+')
                    ));
                }
            }
            Ok((v, err))
        }
        "users_validate" => {
            let file = arg_str(a, "file")?.ok_or("`file` is required")?;
            let path = inside_project(file)?;
            run(vec![s("users"), s("validate"), path, s("--json")])
        }
        "users_run" => {
            let file = arg_str(a, "file")?.ok_or("`file` is required")?;
            let path = inside_project(file)?;
            let (v, err) = run(vec![s("users"), s("validate"), path.clone(), s("--json")])?;
            if err {
                return Ok((v, true));
            }
            let users = v["users"].as_u64().unwrap_or(0);
            check_users(users, limits.max_users)?;
            let duration = arg_str(a, "duration")?
                .map(|d| parse_duration(d).map_err(|e| format!("`duration`: {e}")))
                .transpose()?;
            let mut args = vec![s("users"), s("run"), path];
            if let Some(seed) = a.get("seed") {
                let seed = seed
                    .as_u64()
                    .ok_or("`seed` must be a non-negative integer")?;
                args.extend([s("--seed"), seed.to_string()]);
            }
            let cap = users_duration(duration, limits.max_wall_ns);
            args.extend([
                s("--duration"),
                format!("{}ms", cap / 1_000_000),
                s("--json"),
            ]);
            let (mut v, err) = run(args)?;
            if v.get("steps").is_some() {
                v["duration_limit_ms"] = json!(cap / 1_000_000);
            }
            Ok((v, err))
        }
        "report_get" => {
            let n = a
                .get("timeline")
                .and_then(Value::as_u64)
                .unwrap_or(50)
                .min(1000) as usize;
            let reports = list_reports()?;
            let r = pick_report(&reports, arg_str(a, "run_id")?, 0)?;
            Ok((report_summary(&r, n), false))
        }
        "report_diff" => {
            let reports = list_reports()?;
            let b = pick_report(&reports, arg_str(a, "b")?, 0)?;
            let a_run = match arg_str(a, "a")? {
                Some(id) => pick_report(&reports, Some(id), 0)?,
                // Default: the report before `b` (by time).
                None => {
                    let pos = reports
                        .iter()
                        .position(|r| r["run_id"] == b["run_id"])
                        .unwrap_or(0);
                    reports
                        .get(pos + 1)
                        .cloned()
                        .ok_or("only one report so far; run a scenario again to compare")?
                }
            };
            Ok((report_diff(&a_run, &b), false))
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
}

/// Refuse a `users_run` with more users than `mcp.max_users`.
fn check_users(users: u64, max: u64) -> Result<(), String> {
    if users > max {
        return Err(format!(
            "refused: the file has {users} users, more than mcp.max_users ({max}); raise it in chronostack.yml if intended"
        ));
    }
    Ok(())
}

/// Real-time limit for a `users_run`: the requested duration, but at most
/// `mcp.max_wall` minus 30 s (so the run stops itself and still reports).
fn users_duration(requested_ns: Option<i64>, max_wall_ns: i64) -> i64 {
    let cap = (max_wall_ns - 30_000_000_000)
        .max(max_wall_ns / 2)
        .max(1_000_000_000);
    requested_ns.map_or(cap, |d| d.clamp(1_000_000, cap))
}

const REPORT_DIR: &str = ".chronostack/reports";

/// Scenario reports, newest first.
fn list_reports() -> Result<Vec<Value>, String> {
    let mut files: Vec<PathBuf> = std::fs::read_dir(REPORT_DIR)
        .map_err(|_| format!("no reports yet ({REPORT_DIR} is missing): run a scenario first"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect();
    // Names start with the run's real start time (YYYYMMDD-HHMMSS-…).
    files.sort();
    files.reverse();
    let reports: Vec<Value> = files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|t| serde_json::from_str::<Value>(&t).ok())
        .filter(|v| v.get("report_version").is_some())
        .collect();
    if reports.is_empty() {
        return Err("no reports yet: run a scenario first".into());
    }
    Ok(reports)
}

fn pick_report(reports: &[Value], run_id: Option<&str>, nth: usize) -> Result<Value, String> {
    match run_id {
        Some(id) => reports
            .iter()
            .find(|r| r["run_id"] == id)
            .cloned()
            .ok_or_else(|| format!("no report with run id `{id}` in {REPORT_DIR}")),
        None => reports
            .get(nth)
            .cloned()
            .ok_or_else(|| "no such report".into()),
    }
}

/// The users part of a report, with the first `timeline` entries.
fn users_summary(u: &Value, timeline: usize) -> Value {
    let steps: Vec<Value> = u["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| json!({ "journey": s["journey"], "index": s["index"], "name": s["name"], "runs": s["runs"], "failed": s["failed"], "errors": s["errors"] }))
        .collect();
    let tl: Vec<Value> = u["timeline"]
        .as_array()
        .into_iter()
        .flatten()
        .take(timeline)
        .cloned()
        .collect();
    json!({
        "users": u["users"], "seed": u["seed"], "actions": u["actions"], "stops": u["stops"], "steps": steps,
        "timeline_entries": u["timeline"].as_array().map_or(0, Vec::len),
        "timeline": tl,
    })
}

fn report_summary(r: &Value, timeline: usize) -> Value {
    let steps: Vec<Value> = r["steps"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|st| json!({ "index": st["index"], "name": st["name"], "status": st["status"], "detail": st["detail"], "virtual_after": st["virtual_after"] }))
        .collect();
    let mut out = json!({
        "run_id": r["run_id"], "scenario": r["scenario"]["name"], "result": r["result"],
        "failed_step": r["failed_step"], "virtual_start": r["virtual_start"], "virtual_end": r["virtual_end"],
        "real_start": r["real_start"], "real_elapsed_ms": r["real_elapsed_ms"], "steps": steps,
    });
    if r["users"].is_object() {
        out["users"] = users_summary(&r["users"], timeline);
    }
    out
}

/// A users timeline without per-run details (run ids in URLs, ports):
/// (seconds from the scenario's start, cohort, user, step id, kind, outcome).
fn normalized_timeline(r: &Value) -> Vec<(i64, String, u64, String, String, String)> {
    let start = r["virtual_start"]
        .as_str()
        .and_then(|s| chronostack_core::duration::parse_utc(s).ok())
        .unwrap_or(0);
    r["users"]["timeline"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| {
            let at = e["virtual_time"]
                .as_str()
                .and_then(|s| chronostack_core::duration::parse_utc(s).ok())
                .unwrap_or(0);
            let words: Vec<&str> = e["event"]
                .as_str()
                .unwrap_or("")
                .split_whitespace()
                .collect();
            let step = words.first().copied().unwrap_or("").to_string();
            let what = words.get(1).copied().unwrap_or("").to_string();
            let outcome = match what.as_str() {
                "think" | "branch" => words.get(2).copied().unwrap_or("").to_string(),
                _ => match words.iter().position(|w| *w == "->") {
                    Some(i) => words.get(i + 1).copied().unwrap_or("").to_string(),
                    None => words.get(1..3).map(|w| w.join(" ")).unwrap_or_default(),
                },
            };
            (
                (at - start) / 1_000_000_000,
                e["cohort"].as_str().unwrap_or("").to_string(),
                e["user"].as_u64().unwrap_or(0),
                step,
                what,
                outcome,
            )
        })
        .collect()
}

fn report_diff(a: &Value, b: &Value) -> Value {
    let head = |r: &Value| json!({ "run_id": r["run_id"], "scenario": r["scenario"]["name"], "result": r["result"], "real_start": r["real_start"] });
    let steps_of = |r: &Value| -> Vec<(u64, String, String)> {
        r["steps"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|s| {
                (
                    s["index"].as_u64().unwrap_or(0),
                    s["name"].as_str().unwrap_or("").to_string(),
                    s["status"].as_str().unwrap_or("").to_string(),
                )
            })
            .collect()
    };
    let (sa, sb) = (steps_of(a), steps_of(b));
    let mut changed = Vec::new();
    for (i, name, st_b) in &sb {
        let st_a = sa
            .iter()
            .find(|(j, n, _)| j == i && n == name)
            .map(|x| x.2.as_str())
            .unwrap_or("(absent)");
        if st_a != st_b {
            changed.push(json!({ "index": i, "name": name, "a": st_a, "b": st_b }));
        }
    }
    for (i, name, st_a) in &sa {
        if !sb.iter().any(|(j, n, _)| j == i && n == name) {
            changed.push(json!({ "index": i, "name": name, "a": st_a, "b": "(absent)" }));
        }
    }
    let mut out = json!({
        "a": head(a), "b": head(b),
        "same_scenario": a["scenario"]["name"] == b["scenario"]["name"],
        "result_changed": a["result"] != b["result"],
        "step_changes": changed,
    });
    if a["users"].is_object() || b["users"].is_object() {
        let per_step = |r: &Value| -> Vec<(String, u64, String, u64, u64)> {
            r["users"]["steps"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|s| {
                    (
                        s["journey"].as_str().unwrap_or("").to_string(),
                        s["index"].as_u64().unwrap_or(0),
                        s["name"].as_str().unwrap_or("").to_string(),
                        s["runs"].as_u64().unwrap_or(0),
                        s["failed"].as_u64().unwrap_or(0),
                    )
                })
                .collect()
        };
        let (ua, ub) = (per_step(a), per_step(b));
        let mut step_changes = Vec::new();
        for (j, i, n, runs_b, failed_b) in &ub {
            let (runs_a, failed_a) = ua
                .iter()
                .find(|x| &x.0 == j && x.1 == *i)
                .map_or((0, 0), |x| (x.3, x.4));
            if (runs_a, failed_a) != (*runs_b, *failed_b) {
                step_changes.push(json!({ "journey": j, "index": i, "name": n, "runs": [runs_a, runs_b], "failed": [failed_a, failed_b] }));
            }
        }
        let (ta, tb) = (normalized_timeline(a), normalized_timeline(b));
        let first_difference = ta
            .iter()
            .zip(tb.iter())
            .position(|(x, y)| x != y)
            .or((ta.len() != tb.len()).then(|| ta.len().min(tb.len())));
        out["users"] = json!({
            "a": a["users"]["users"], "b": b["users"]["users"],
            "seed": [a["users"]["seed"], b["users"]["seed"]],
            "step_changes": step_changes,
            "timeline_same": ta == tb && !ta.is_empty(),
            "timeline_entries": [ta.len(), tb.len()],
            "first_timeline_difference": first_difference.map(|i| json!({
                "position": i,
                "a": ta.get(i).map(|x| format!("+{}s {} #{} step {} {} {}", x.0, x.1, x.2, x.3, x.4, x.5)),
                "b": tb.get(i).map(|x| format!("+{}s {} #{} step {} {} {}", x.0, x.1, x.2, x.3, x.4, x.5)),
            })),
        });
    }
    out
}

/// Resolve a scenario path and require it to stay inside the project directory.
fn inside_project(file: &str) -> Result<String, String> {
    let base = std::env::current_dir()
        .and_then(|d| d.canonicalize())
        .map_err(|e| e.to_string())?;
    let p: PathBuf = base.join(file);
    let real = p
        .canonicalize()
        .map_err(|_| format!("scenario file `{file}` not found"))?;
    let ext_ok = matches!(
        real.extension().and_then(|e| e.to_str()),
        Some("yml" | "yaml")
    );
    if !real.starts_with(&base) || !ext_ok {
        return Err(format!(
            "`{file}` must be a .yml/.yaml file inside the project directory"
        ));
    }
    Ok(real
        .strip_prefix(&base)
        .unwrap_or(Path::new(file))
        .to_string_lossy()
        .into_owned())
}

/// Per-process protocol state.
struct State {
    /// Legacy version negotiated by `initialize`.
    legacy: Option<String>,
    limits: Limits,
}

fn meta_version(params: &Value) -> Option<&str> {
    params
        .get("_meta")?
        .get("io.modelcontextprotocol/protocolVersion")?
        .as_str()
}

/// Requests other than initialize/discover/ping: either a 2026-07-28 request
/// with `_meta`, or a legacy request after `initialize`.
fn check_era(st: &State, params: &Value) -> Result<bool, RpcError> {
    if let Some(v) = meta_version(params) {
        if v != MODERN_VERSION {
            return Err((
                -32022,
                format!("unsupported protocol version {v}"),
                Some(json!({ "supported": [MODERN_VERSION], "requested": v })),
            ));
        }
        if params["_meta"]
            .get("io.modelcontextprotocol/clientCapabilities")
            .is_none()
        {
            return Err(rpc_err(
                -32602,
                "missing _meta io.modelcontextprotocol/clientCapabilities",
            ));
        }
        return Ok(true);
    }
    if st.legacy.is_some() {
        return Ok(false);
    }
    Err(rpc_err(
        -32602,
        "send `initialize` first, or include `_meta` with io.modelcontextprotocol/protocolVersion 2026-07-28",
    ))
}

fn server_info() -> Value {
    json!({ "name": "chronostack", "title": "ChronoStack", "version": env!("CARGO_PKG_VERSION") })
}

fn handle(st: &mut State, method: &str, params: &Value) -> Result<Value, RpcError> {
    match method {
        "initialize" => {
            let requested = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or("");
            let v = if LEGACY_VERSIONS.contains(&requested) {
                requested
            } else {
                LEGACY_VERSIONS[0]
            };
            st.legacy = Some(v.to_string());
            Ok(json!({
                "protocolVersion": v,
                "capabilities": { "tools": {} },
                "serverInfo": server_info(),
                "instructions": INSTRUCTIONS,
            }))
        }
        "ping" => Ok(json!({})),
        "server/discover" => Ok(json!({
            "resultType": "complete",
            "supportedVersions": [MODERN_VERSION],
            "capabilities": { "tools": {} },
            "_meta": { "io.modelcontextprotocol/serverInfo": server_info() },
            "instructions": INSTRUCTIONS,
            "ttlMs": 3_600_000,
            "cacheScope": "public",
        })),
        "tools/list" => {
            check_era(st, params)?;
            Ok(
                json!({ "tools": tools(), "resultType": "complete", "ttlMs": 300_000, "cacheScope": "public" }),
            )
        }
        "tools/call" => {
            check_era(st, params)?;
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| rpc_err(-32602, "missing tool `name`"))?;
            if !tools().iter().any(|t| t["name"] == name) {
                return Err(rpc_err(-32602, format!("Unknown tool: {name}")));
            }
            let empty = Map::new();
            let args = match params.get("arguments") {
                None | Some(Value::Null) => &empty,
                Some(Value::Object(m)) => m,
                Some(_) => return Err(rpc_err(-32602, "`arguments` must be an object")),
            };
            let (value, is_error) = match call_tool(name, args, st.limits) {
                Ok(r) => r,
                Err(msg) => (json!({ "error": msg }), true),
            };
            Ok(json!({
                "resultType": "complete",
                "content": [{ "type": "text", "text": value.to_string() }],
                "structuredContent": value,
                "isError": is_error,
            }))
        }
        m => Err(rpc_err(-32601, format!("Method not found: {m}"))),
    }
}

/// One JSON-RPC message in, at most one out.
fn handle_line(st: &mut State, line: &str) -> Option<Value> {
    let msg: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(
                json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": format!("Parse error: {e}") } }),
            );
        }
    };
    if msg.is_array() {
        return Some(
            json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32600, "message": "JSON-RPC batches are not supported" } }),
        );
    }
    let id = msg.get("id").cloned();
    let method = msg.get("method").and_then(Value::as_str);
    let (Some(id), Some(method)) = (id, method) else {
        // Notifications (no id) and stray responses get no reply.
        return None;
    };
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    Some(match handle(st, method, &params) {
        Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
        Err((code, message, data)) => {
            let mut e = json!({ "code": code, "message": message });
            if let Some(d) = data {
                e["data"] = d;
            }
            json!({ "jsonrpc": "2.0", "id": id, "error": e })
        }
    })
}

pub fn serve(limits: Limits) -> Result<i32> {
    eprintln!(
        "chronostack mcp: serving on stdio (protocol {MODERN_VERSION}, {}) in {}",
        LEGACY_VERSIONS.join(", "),
        std::env::current_dir()?.display()
    );
    let mut st = State {
        legacy: None,
        limits,
    };
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = handle_line(&mut st, &line) {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn st() -> State {
        State {
            legacy: None,
            limits: Limits {
                max_advance_ns: 1,
                max_wall_ns: DEFAULT_MCP_MAX_WALL_NS,
                max_users: DEFAULT_MCP_MAX_USERS,
            },
        }
    }

    fn call(st: &mut State, v: Value) -> Value {
        handle_line(st, &v.to_string()).unwrap_or(Value::Null)
    }

    #[test]
    fn legacy_handshake() {
        let mut s = st();
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list"}),
        );
        assert_eq!(r["error"]["code"], -32602, "tools/list before initialize");
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
        );
        assert_eq!(r["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(r["result"]["capabilities"]["tools"], json!({}));
        assert_eq!(r["result"]["serverInfo"]["name"], "chronostack");
        assert!(
            handle_line(
                &mut s,
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            )
            .is_none()
        );
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/list"}),
        );
        let names: Vec<&str> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names.len(), 17);
        for n in [
            "clock_status",
            "clock_advance",
            "clock_freeze",
            "clock_unfreeze",
            "probe",
            "doctor",
            "fault_set",
            "fault_clear",
            "scenario_run",
            "events_tail",
            "env_inspect",
            "scenario_validate",
            "report_get",
            "report_diff",
            "users_validate",
            "users_run",
        ] {
            assert!(names.contains(&n), "{n}");
        }
        for t in r["result"]["tools"].as_array().unwrap() {
            assert_eq!(t["inputSchema"]["type"], "object", "{}", t["name"]);
        }
        assert_eq!(
            call(&mut s, json!({"jsonrpc":"2.0","id":4,"method":"ping"}))["result"],
            json!({})
        );
    }

    #[test]
    fn version_negotiation_and_errors() {
        let mut s = st();
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1999-01-01"}}),
        );
        assert_eq!(
            r["result"]["protocolVersion"], LEGACY_VERSIONS[0],
            "unsupported → latest legacy"
        );
        assert_eq!(
            call(&mut s, json!({"jsonrpc":"2.0","id":2,"method":"nope"}))["error"]["code"],
            -32601
        );
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"rm_rf"}}),
        );
        assert_eq!(r["error"]["code"], -32602);
        assert!(
            r["error"]["message"]
                .as_str()
                .unwrap()
                .contains("Unknown tool")
        );
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"clock_status","arguments":{"x":1}}}),
        );
        assert_eq!(
            r["result"]["isError"], true,
            "bad arguments are tool errors"
        );
        assert_eq!(
            handle_line(&mut s, "{nope").unwrap()["error"]["code"],
            -32700
        );
        assert_eq!(call(&mut s, json!([1]))["error"]["code"], -32600);
    }

    #[test]
    fn modern_era() {
        let mut s = st();
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":"d","method":"server/discover","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}}}}),
        );
        assert_eq!(r["result"]["supportedVersions"], json!(["2026-07-28"]));
        assert_eq!(r["result"]["resultType"], "complete");
        assert_eq!(r["result"]["cacheScope"], "public");
        let meta = json!({"io.modelcontextprotocol/protocolVersion":"2026-07-28","io.modelcontextprotocol/clientCapabilities":{}});
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta": meta}}),
        );
        assert_eq!(r["result"]["ttlMs"], 300_000);
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":2,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2030-01-01","io.modelcontextprotocol/clientCapabilities":{}}}}),
        );
        assert_eq!(r["error"]["code"], -32022);
        assert_eq!(r["error"]["data"]["requested"], "2030-01-01");
        let r = call(
            &mut s,
            json!({"jsonrpc":"2.0","id":3,"method":"tools/list","params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28"}}}),
        );
        assert_eq!(r["error"]["code"], -32602, "clientCapabilities is required");
    }

    #[test]
    fn scenario_paths_stay_in_the_project() {
        assert!(inside_project("../../etc/passwd").is_err());
        assert!(inside_project("Cargo.toml").is_err(), "not a .yml file");
        assert!(inside_project("does-not-exist.yml").is_err());
    }

    #[test]
    fn user_limits() {
        assert!(check_users(1000, 1000).is_ok());
        let e = check_users(1001, 1000).unwrap_err();
        assert!(e.contains("mcp.max_users (1000)"), "{e}");
        let wall = 600_000_000_000; // 10 min
        assert_eq!(
            users_duration(None, wall),
            570_000_000_000,
            "30 s before max_wall"
        );
        assert_eq!(users_duration(Some(60_000_000_000), wall), 60_000_000_000);
        assert_eq!(
            users_duration(Some(3_600_000_000_000), wall),
            570_000_000_000
        );
        // A short max_wall still leaves the run half of it.
        assert_eq!(users_duration(None, 20_000_000_000), 10_000_000_000);
    }

    fn sample_report(
        run_id: &str,
        result: &str,
        step3: &str,
        timeline: Value,
        failed: u64,
    ) -> Value {
        json!({
            "report_version": 1, "run_id": run_id, "result": result,
            "scenario": { "name": "cohort" },
            "virtual_start": "2026-12-01T00:00:00Z", "real_start": "2026-11-21T10:00:00Z",
            "steps": [
                { "index": 1, "name": "advance", "status": "passed" },
                { "index": 2, "name": "assert a", "status": "passed" },
                { "index": 3, "name": "assert b", "status": step3 },
            ],
            "users": {
                "seed": 7, "actions": 4, "stops": 3,
                "users": { "total": 2, "completed": 2 - failed, "failed": failed },
                "steps": [{ "journey": "j", "index": 1, "name": "sign up", "runs": 2, "failed": failed, "errors": [] }],
                "timeline": timeline,
            }
        })
    }

    fn tl(urls: &[&str]) -> Value {
        Value::Array(urls.iter().enumerate().map(|(i, u)| json!({
            "virtual_time": format!("2026-12-01T00:0{i}:00Z"), "cohort": "c", "user": i + 1, "event": u,
        })).collect())
    }

    #[test]
    fn report_diff_finds_what_changed() {
        let same = tl(&[
            "1 POST http://127.0.0.1:5000/signup?e=u1-aa -> 201",
            "2 think 3600ns",
        ]);
        let other_ports = tl(&[
            "1 POST http://127.0.0.1:6000/signup?e=u1-bb -> 201",
            "2 think 3600ns",
        ]);
        let d = report_diff(
            &sample_report("aa", "passed", "passed", same.clone(), 0),
            &sample_report("bb", "passed", "passed", other_ports, 0),
        );
        assert_eq!(d["result_changed"], false);
        assert_eq!(d["step_changes"], json!([]));
        assert_eq!(
            d["users"]["timeline_same"], true,
            "ports and run ids are ignored: {d}"
        );

        let worse = tl(&[
            "1 POST http://127.0.0.1:5000/signup?e=u1-cc -> 500",
            "2 think 3600ns",
        ]);
        let d = report_diff(
            &sample_report("aa", "passed", "passed", same, 0),
            &sample_report("cc", "failed", "failed", worse, 1),
        );
        assert_eq!(d["result_changed"], true);
        assert_eq!(
            d["step_changes"],
            json!([{ "index": 3, "name": "assert b", "a": "passed", "b": "failed" }])
        );
        assert_eq!(d["users"]["timeline_same"], false);
        assert_eq!(d["users"]["first_timeline_difference"]["position"], 0);
        assert_eq!(d["users"]["step_changes"][0]["failed"], json!([0, 1]));
    }

    #[test]
    fn report_summary_trims_the_timeline() {
        let r = sample_report(
            "aa",
            "passed",
            "passed",
            tl(&["1 GET x -> 200", "2 think 1ns", "3 branch on"]),
            0,
        );
        let s = report_summary(&r, 2);
        assert_eq!(s["run_id"], "aa");
        assert_eq!(s["steps"].as_array().unwrap().len(), 3);
        assert_eq!(s["users"]["timeline_entries"], 3);
        assert_eq!(s["users"]["timeline"].as_array().unwrap().len(), 2);
    }

    fn shell(script: &str) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("powershell");
            c.args(["-NoProfile", "-NonInteractive", "-Command", script]);
            c
        } else {
            let mut c = Command::new("sh");
            c.args(["-c", script]);
            c
        }
    }

    #[test]
    fn bounded_run_returns_the_last_json_line() {
        let script = if cfg!(windows) {
            r#"Write-Output 'progress'; Write-Output '{"ok":true}'; [Console]::Error.WriteLine('note'); exit 4"#
        } else {
            r#"echo progress; echo '{"ok":true}'; echo note >&2; exit 4"#
        };
        let (code, v, err) = run_bounded(&mut shell(script), Duration::from_secs(60)).unwrap();
        assert_eq!(code, 4);
        assert_eq!(v, json!({ "ok": true }));
        assert_eq!(err, "note");
    }

    #[test]
    fn bounded_run_stops_a_slow_command() {
        let script = if cfg!(windows) {
            "Start-Sleep -Seconds 30"
        } else {
            "sleep 30"
        };
        let t = Instant::now();
        let e = run_bounded(&mut shell(script), Duration::from_millis(500)).unwrap_err();
        assert!(e.to_string().contains("mcp.max_wall"), "{e}");
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "took {:?}",
            t.elapsed()
        );
    }
}
