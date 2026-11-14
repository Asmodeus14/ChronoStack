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
//! triggers declared in `chronostack.yml`; `clock_advance` is capped by
//! `mcp.max_advance` (default 400 days); scenario files must be inside the
//! project directory.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Result;
use chronostack_core::duration::{format_utc, parse_duration};
use serde_json::{Map, Value, json};

use crate::client::Client;
use crate::fault;
use crate::session::Session;

pub const MODERN_VERSION: &str = "2026-07-28";
pub const LEGACY_VERSIONS: [&str; 3] = ["2025-11-25", "2025-06-18", "2025-03-26"];

const INSTRUCTIONS: &str = "ChronoStack moves virtual wall-clock time for a Docker Compose stack \
(databases, caches, workers) so time-dependent behaviour can be tested. Start the stack with \
`chronostack up` first. Typical flow: clock_status → fault_set (optional outage on a proxy target) → \
clock_advance (runs declared triggers and waits on barriers at each stop) → events_tail / probe to \
inspect → scenario_run for scripted checks. Time only moves forward.";

type RpcError = (i64, String, Option<Value>);

fn rpc_err(code: i64, msg: impl Into<String>) -> RpcError {
    (code, msg.into(), None)
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
            "description": "Run a scenario YAML file from the project (steps like advance, fault, assert) and return pass/fail with the failed step. A full report is written to .chronostack/reports/.",
            "inputSchema": schema(json!({
                "file": { "type": "string", "description": "Path inside the project directory, e.g. scenarios/trial.yml." }
            }), &["file"]) }),
    ]
}

/// Run this binary with `args` (it always ends in `--json`); parse the last JSON line.
fn cli(args: &[String]) -> Result<(i32, Value, String)> {
    let exe = std::env::current_exe()?;
    let out = Command::new(exe).args(args).stdin(Stdio::null()).output()?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
    let v = stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .unwrap_or(Value::Null);
    Ok((out.status.code().unwrap_or(1), v, stderr))
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
fn call_tool(
    name: &str,
    a: &Map<String, Value>,
    max_advance_ns: i64,
) -> Result<(Value, bool), String> {
    let known: &[&str] = match name {
        "clock_advance" => &["duration", "to", "step", "dry_run"],
        "fault_set" => &["target", "from", "for", "until", "status"],
        "fault_clear" => &["id", "all"],
        "events_tail" => &["since", "limit"],
        "scenario_run" => &["file"],
        _ => &[],
    };
    if let Some(k) = a.keys().find(|k| !known.contains(&k.as_str())) {
        return Err(format!("unknown argument `{k}` for {name}"));
    }
    let run = |args: Vec<String>| -> Result<(Value, bool), String> {
        let (code, v, err) = cli(&args).map_err(|e| e.to_string())?;
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
            let (code, _, err) = cli(&args).map_err(|e| e.to_string())?;
            let (v, e) = run(vec![s("fault"), s("list"), s("--json")])?;
            if code != 0 {
                return Ok((json!({ "error": err }), true));
            }
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
            let (v, err) = run(vec![s("run"), path, s("--json")])?;
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
            Ok((
                json!({
                    "result": v["result"], "failed_step": v["failed_step"], "steps": steps,
                    "proxy_requests": proxy, "virtual_start": v["virtual_start"], "virtual_end": v["virtual_end"],
                    "report_dir": ".chronostack/reports",
                }),
                err,
            ))
        }
        _ => Err(format!("Unknown tool: {name}")),
    }
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
    max_advance_ns: i64,
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
            let (value, is_error) = match call_tool(name, args, st.max_advance_ns) {
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

pub fn serve(max_advance_ns: i64) -> Result<i32> {
    eprintln!(
        "chronostack mcp: serving on stdio (protocol {MODERN_VERSION}, {}) in {}",
        LEGACY_VERSIONS.join(", "),
        std::env::current_dir()?.display()
    );
    let mut st = State {
        legacy: None,
        max_advance_ns,
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
            max_advance_ns: 1,
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
        assert_eq!(names.len(), 11);
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
}
