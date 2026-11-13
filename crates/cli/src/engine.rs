//! The advance engine: step → trigger → barrier (ADR-005, ADR-016).
//!
//! 1. Plan every stop of the clock (`core::schedule::plan`).
//! 2. For each stop: move the clock there, run due triggers, then wait on the
//!    barriers (or `dwell` when none are configured).
//! 3. Probe every virtualized service at the end.

use std::time::{Duration, Instant};

use anyhow::Result;
use chronostack_core::cron::Cron;
use chronostack_core::duration::{format_span, format_utc};
use chronostack_core::schedule::{self, Boundary};
use serde::Serialize;
use serde_json::json;

use crate::client::Client;
use crate::config::{Barrier, ExecSpec, HttpSpec, Resolved, Trigger};
use crate::docker::{self, Compose};
use crate::probe;
use crate::session::{ProbeSpec, Session};

pub const EXIT_TRIGGER_FAILED: i32 = 4;
pub const EXIT_BARRIER_TIMEOUT: i32 = 5;

/// Default wait after a sub-step when pg_cron is detected and no barrier is
/// configured: pg_cron's launcher loop blocks at most 1 s (`MaxWait`).
const PG_CRON_DWELL_NS: i64 = 1_200_000_000;
/// Default wait after each `--step` when no barrier is configured.
const STEP_DWELL_NS: i64 = 1_000_000_000;
const PG_CRON_MAX_JUMP_NS: i64 = 3_600_000_000_000;

pub enum Target {
    By(i64),
    To(i64),
}

pub enum MaxJump {
    Auto,
    Disabled,
    Fixed(i64),
}

pub struct Options {
    pub target: Target,
    pub step: Option<i64>,
    pub max_jump: MaxJump,
    pub dwell: Option<i64>,
    pub dry_run: bool,
    pub json: bool,
    pub no_probe: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub name: String,
    pub ok: bool,
    pub elapsed_ms: u128,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StopResult {
    pub at: String,
    pub at_ns: i64,
    pub step_end: bool,
    pub sub_step: bool,
    pub pinned: bool,
    pub triggers: Vec<ActionResult>,
    pub barriers: Vec<ActionResult>,
    pub dwell_ms: u128,
}

/// Postgres services with pg_cron in `shared_preload_libraries`.
fn detect_pg_cron(session: &Session) -> Vec<String> {
    let compose = session.compose();
    session
        .services
        .iter()
        .filter(|e| e.plan.virtualized)
        .filter_map(|e| match &e.probe {
            ProbeSpec::Postgres { user, db } => {
                let args = compose.args(&[
                    "exec",
                    "-T",
                    &e.plan.service,
                    "psql",
                    "-U",
                    user,
                    "-d",
                    db,
                    "-Atc",
                    "show shared_preload_libraries",
                ]);
                let out = docker::output(&args).ok()?;
                String::from_utf8_lossy(&out.stdout)
                    .contains("pg_cron")
                    .then(|| e.plan.service.clone())
            }
            _ => None,
        })
        .collect()
}

/// How pg_cron jobs will be handled during an advance.
struct PgCronPlan {
    /// `schedule-aware`, `hourly` or `off`.
    mode: &'static str,
    reason: Option<String>,
    /// Stops at each fixed-time job's minute (+1 s).
    pinned: Vec<i64>,
}

/// pg_cron evaluates schedules in `cron.timezone`; only UTC is planned exactly.
fn utc_zone(tz: &str) -> bool {
    matches!(
        tz.trim().to_ascii_uppercase().as_str(),
        "GMT" | "UTC" | "ETC/UTC" | "ETC/GMT" | "Z" | "ZULU" | "UCT"
    )
}

fn psql_one(compose: &Compose, service: &str, user: &str, db: &str, sql: &str) -> Option<String> {
    let args = compose.args(&[
        "exec", "-T", service, "psql", "-U", user, "-d", db, "-Atc", sql,
    ]);
    let out = docker::output(&args).ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Read pg_cron's active schedules and decide how to step (see M2 results):
/// after a jump of more than 3 h pg_cron still runs jobs matching the *current*
/// minute, so stopping exactly on each fixed-time job's minute is enough.
/// Wildcard jobs (`*/5 * * * *`) run once per stop either way.
fn pg_cron_plan(session: &Session, services: &[String], from: i64, to: i64) -> PgCronPlan {
    let hourly = |reason: String| PgCronPlan {
        mode: "hourly",
        reason: Some(reason),
        pinned: vec![],
    };
    let compose = session.compose();
    let mut pinned = Vec::new();
    for svc in services {
        let Some(ProbeSpec::Postgres { user, db }) = session
            .services
            .iter()
            .find(|e| &e.plan.service == svc)
            .map(|e| &e.probe)
        else {
            return hourly(format!("{svc}: not a Postgres probe target"));
        };
        let Some(tz) = psql_one(&compose, svc, user, db, "show cron.timezone") else {
            return hourly(format!("{svc}: cannot read cron.timezone"));
        };
        if !utc_zone(&tz) {
            return hourly(format!(
                "{svc}: cron.timezone is {tz}, only UTC schedules are planned exactly"
            ));
        }
        let cron_db = psql_one(&compose, svc, user, db, "show cron.database_name")
            .unwrap_or_else(|| "postgres".into());
        let Some(rows) = psql_one(
            &compose,
            svc,
            user,
            &cron_db,
            "select schedule from cron.job where active",
        ) else {
            return hourly(format!("{svc}: cannot read cron.job in database {cron_db}"));
        };
        for schedule in rows.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let cron = match Cron::parse(schedule) {
                Ok(c) => c,
                Err(e) => {
                    return hourly(format!("{svc}: schedule `{schedule}` not supported ({e})"));
                }
            };
            if cron.wildcard {
                continue;
            }
            match cron.occurrences(from, to, 5_000) {
                Ok(occ) => pinned.extend(occ.into_iter().map(|t| t + 1_000_000_000)),
                Err(e) => return hourly(format!("{svc}: schedule `{schedule}`: {e}")),
            }
        }
    }
    pinned.sort_unstable();
    pinned.dedup();
    PgCronPlan {
        mode: "schedule-aware",
        reason: None,
        pinned,
    }
}

fn tail(s: &str, n: usize) -> String {
    let t = s.trim();
    let start = t
        .char_indices()
        .rev()
        .nth(n.saturating_sub(1))
        .map(|(i, _)| i)
        .unwrap_or(0);
    t[start..].to_string()
}

fn run_exec(compose: &Compose, spec: &ExecSpec, timeout: Duration) -> (bool, Option<String>) {
    let mut args = compose.args(&["exec", "-T", &spec.service]);
    args.extend(spec.command.iter().cloned());
    match docker::output_timeout(&args, timeout) {
        Ok(t) if t.timed_out => (false, Some(format!("timed out after {timeout:?}"))),
        Ok(t) if t.code == Some(0) => (true, Some(tail(&t.stdout, 300)).filter(|s| !s.is_empty())),
        Ok(t) => (
            false,
            Some(format!(
                "exit {:?}: {}",
                t.code,
                tail(
                    if t.stderr.trim().is_empty() {
                        &t.stdout
                    } else {
                        &t.stderr
                    },
                    400
                )
            )),
        ),
        Err(e) => (false, Some(e.to_string())),
    }
}

fn run_http(spec: &HttpSpec, timeout: Duration) -> (bool, Option<String>) {
    let agent = ureq::AgentBuilder::new().timeout(timeout).build();
    let resp = agent.request(&spec.method, &spec.url).call();
    let code = match resp {
        Ok(r) => r.status(),
        Err(ureq::Error::Status(c, _)) => c,
        Err(e) => return (false, Some(e.to_string())),
    };
    let ok = match spec.status {
        Some(want) => code == want,
        None => (200..300).contains(&code),
    };
    (ok, Some(format!("HTTP {code}")))
}

fn run_trigger(compose: &Compose, t: &Trigger, timeout_ns: i64) -> ActionResult {
    let start = Instant::now();
    let timeout = Duration::from_nanos(timeout_ns as u64);
    let (ok, detail) = match (&t.exec, &t.http) {
        (Some(e), _) => run_exec(compose, e, timeout),
        (_, Some(h)) => run_http(h, timeout),
        _ => (false, Some("no action".into())),
    };
    ActionResult {
        name: t.name.clone(),
        ok,
        elapsed_ms: start.elapsed().as_millis(),
        detail,
    }
}

/// Evaluate a barrier once.
fn barrier_once(session: &Session, compose: &Compose, b: &Barrier) -> (bool, Option<String>) {
    let quick = Duration::from_secs(20);
    if let Some(s) = &b.sql {
        let (u, d) = match session
            .services
            .iter()
            .find(|e| e.plan.service == s.service)
            .map(|e| &e.probe)
        {
            Some(ProbeSpec::Postgres { user, db }) => (user.clone(), db.clone()),
            _ => ("postgres".to_string(), "postgres".to_string()),
        };
        let user = s.user.clone().unwrap_or(u);
        let db = s.database.clone().unwrap_or(d);
        let args = compose.args(&[
            "exec", "-T", &s.service, "psql", "-U", &user, "-d", &db, "-Atc", &s.query,
        ]);
        return match docker::output_timeout(&args, quick) {
            Ok(t) => {
                let v = t.stdout.trim().to_ascii_lowercase();
                let ok = t.code == Some(0) && matches!(v.as_str(), "t" | "true" | "1");
                (
                    ok,
                    Some(if t.code == Some(0) {
                        format!("= {v}")
                    } else {
                        tail(&t.stderr, 200)
                    }),
                )
            }
            Err(e) => (false, Some(e.to_string())),
        };
    }
    if let Some(r) = &b.redis {
        let mut args = compose.args(&["exec", "-T", &r.service, "redis-cli"]);
        args.extend(r.command.iter().cloned());
        return match docker::output_timeout(&args, quick) {
            Ok(t) => {
                let v = t.stdout.trim().to_string();
                (t.code == Some(0) && v == r.equals, Some(format!("= {v}")))
            }
            Err(e) => (false, Some(e.to_string())),
        };
    }
    if let Some(e) = &b.exec {
        return run_exec(compose, e, quick);
    }
    if let Some(h) = &b.http {
        return run_http(h, quick);
    }
    (false, Some("no condition".into()))
}

fn wait_barrier(
    session: &Session,
    compose: &Compose,
    b: &Barrier,
    timeout_ns: i64,
) -> ActionResult {
    let start = Instant::now();
    let deadline = Duration::from_nanos(timeout_ns as u64);
    loop {
        let (ok, detail) = barrier_once(session, compose, b);
        if ok {
            return ActionResult {
                name: b.name.clone(),
                ok,
                elapsed_ms: start.elapsed().as_millis(),
                detail: None,
            };
        }
        if start.elapsed() >= deadline {
            return ActionResult {
                name: b.name.clone(),
                ok: false,
                elapsed_ms: start.elapsed().as_millis(),
                detail: Some(format!(
                    "not satisfied after {deadline:?} (last: {})",
                    detail.unwrap_or_default()
                )),
            };
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

fn print_plan(
    stops: &[Boundary],
    cfg: &Resolved,
    from: i64,
    max_jump: Option<i64>,
    pg_cron: &[String],
) {
    let subs = stops.iter().filter(|b| b.sub_step).count();
    println!(
        "plan: {} stop(s) from {} to {}{}",
        stops.len(),
        format_utc(from),
        format_utc(stops.last().map(|b| b.at_ns).unwrap_or(from)),
        match max_jump {
            Some(mj) => format!(
                ", max jump {}{} ({subs} sub-steps)",
                format_span(mj).trim_start_matches('+'),
                if pg_cron.is_empty() {
                    String::new()
                } else {
                    format!(" — pg_cron detected in {}", pg_cron.join(", "))
                }
            ),
            None => String::new(),
        }
    );
    for b in stops.iter().filter(|b| !b.sub_step) {
        let names: Vec<&str> = b
            .scheduled
            .iter()
            .map(|i| cfg.config.triggers[*i].name.as_str())
            .collect();
        println!(
            "  {}  {}{}",
            format_utc(b.at_ns),
            if b.step_end { "step end" } else { "        " },
            if names.is_empty() {
                String::new()
            } else {
                format!("  triggers: {}", names.join(", "))
            }
        );
    }
}

pub fn advance(session: &Session, client: &Client, cfg: &Resolved, opts: &Options) -> Result<i32> {
    let start_state = client.clock()?;
    let from = start_state.virtual_ns;
    let to = match opts.target {
        Target::By(d) => from + d,
        Target::To(t) => t,
    };

    let pg_cron = match (&opts.max_jump, cfg.max_jump_ns) {
        (MaxJump::Auto, None) => detect_pg_cron(session),
        _ => vec![],
    };
    let pg = if pg_cron.is_empty() {
        PgCronPlan {
            mode: "off",
            reason: None,
            pinned: vec![],
        }
    } else {
        pg_cron_plan(session, &pg_cron, from, to)
    };
    let max_jump = match opts.max_jump {
        MaxJump::Disabled => None,
        MaxJump::Fixed(v) => Some(v),
        MaxJump::Auto => cfg
            .max_jump_ns
            .or((pg.mode == "hourly").then_some(PG_CRON_MAX_JUMP_NS)),
    };
    let stops = schedule::plan(from, to, opts.step, max_jump, &cfg.schedules, &pg.pinned)
        .map_err(anyhow::Error::msg)?;

    let barriers = &cfg.config.barriers;
    let dwell_ns = opts
        .dwell
        .or(cfg.dwell_ns)
        .unwrap_or(if !barriers.is_empty() {
            0
        } else if !pg_cron.is_empty() {
            PG_CRON_DWELL_NS
        } else if opts.step.is_some() {
            STEP_DWELL_NS
        } else {
            0
        });

    if opts.dry_run {
        if opts.json {
            let list: Vec<_> = stops
                .iter()
                .map(|b| {
                    json!({
                        "at": format_utc(b.at_ns), "at_ns": b.at_ns, "step_end": b.step_end, "sub_step": b.sub_step, "pinned": b.pinned,
                        "triggers": b.scheduled.iter().map(|i| &cfg.config.triggers[*i].name).collect::<Vec<_>>(),
                    })
                })
                .collect();
            println!(
                "{}",
                json!({ "from_ns": from, "to_ns": to, "max_jump_ns": max_jump, "dwell_ns": dwell_ns, "pg_cron": pg_cron, "pg_cron_mode": pg.mode, "pg_cron_reason": pg.reason, "stops": list })
            );
        } else {
            print_plan(&stops, cfg, from, max_jump, &pg_cron);
            if pg.mode != "off" {
                println!(
                    "pg_cron: {}{}",
                    pg.mode,
                    pg.reason
                        .as_deref()
                        .map(|r| format!(" ({r})"))
                        .unwrap_or_default()
                );
            }
        }
        return Ok(0);
    }

    if !opts.json {
        print_plan_header(&stops, from, max_jump, &pg_cron, dwell_ns, cfg);
        if pg.mode != "off" {
            println!(
                "pg_cron: {} — {} job stop(s){}",
                pg.mode,
                stops.iter().filter(|b| b.pinned).count(),
                pg.reason
                    .as_deref()
                    .map(|r| format!(" ({r})"))
                    .unwrap_or_default()
            );
        }
    }
    let compose = session.compose();
    let step_triggers: Vec<usize> = cfg
        .config
        .triggers
        .iter()
        .enumerate()
        .filter(|(_, t)| t.schedule.is_none())
        .map(|(i, _)| i)
        .collect();
    let run_start = Instant::now();
    let mut results: Vec<StopResult> = Vec::with_capacity(stops.len());
    let mut code = 0;

    'stops: for b in &stops {
        let current = client.clock()?.virtual_ns;
        if b.at_ns > current {
            client.advance_to(b.at_ns)?;
        }
        let mut due: Vec<usize> = b.scheduled.clone();
        if b.step_end {
            due.extend(step_triggers.iter().copied());
        }
        let mut r = StopResult {
            at: format_utc(b.at_ns),
            at_ns: b.at_ns,
            step_end: b.step_end,
            sub_step: b.sub_step,
            pinned: b.pinned,
            triggers: vec![],
            barriers: vec![],
            dwell_ms: 0,
        };
        for i in &due {
            let t = &cfg.config.triggers[*i];
            let a = run_trigger(&compose, t, cfg.trigger_timeouts_ns[*i]);
            let failed = !a.ok;
            r.triggers.push(a);
            if failed {
                code = EXIT_TRIGGER_FAILED;
                print_stop(&r, opts.json);
                results.push(r);
                break 'stops;
            }
        }
        if !barriers.is_empty() {
            for (i, bar) in barriers.iter().enumerate() {
                let a = wait_barrier(session, &compose, bar, cfg.barrier_timeouts_ns[i]);
                let failed = !a.ok;
                r.barriers.push(a);
                if failed {
                    code = EXIT_BARRIER_TIMEOUT;
                    print_stop(&r, opts.json);
                    results.push(r);
                    break 'stops;
                }
            }
        } else if dwell_ns > 0 && (b.step_end || b.sub_step || b.pinned) {
            std::thread::sleep(Duration::from_nanos(dwell_ns as u64));
            r.dwell_ms = (dwell_ns / 1_000_000) as u128;
        }
        print_stop(&r, opts.json);
        results.push(r);
    }

    let end_state = client.clock()?;
    let probes = if opts.no_probe || code != 0 {
        vec![]
    } else {
        probe::probe_all(session, client)
    };
    if code == 0 && probes.iter().any(|p| p.status != "ok") {
        code = 3;
    }
    let trigger_runs: usize = results.iter().map(|r| r.triggers.len()).sum();
    if opts.json {
        println!(
            "{}",
            json!({
                "ok": code == 0, "exit_code": code, "from_ns": from, "to_ns": to,
                "elapsed_ms": run_start.elapsed().as_millis(), "max_jump_ns": max_jump, "pg_cron": pg_cron, "pg_cron_mode": pg.mode,
                "trigger_runs": trigger_runs, "stops": results, "clock": end_state, "probes": probes,
            })
        );
    } else {
        println!(
            "\n{} — {} stop(s), {} trigger run(s), {:.1}s real time",
            if code == 0 { "done" } else { "STOPPED" },
            results.len(),
            trigger_runs,
            run_start.elapsed().as_secs_f64()
        );
        println!(
            "virtual  {}  ({} vs real)",
            format_utc(end_state.virtual_ns),
            format_span(end_state.offset_ns)
        );
        if !probes.is_empty() {
            println!();
            probe::print_probes(&probes);
        }
    }
    Ok(code)
}

fn print_plan_header(
    stops: &[Boundary],
    from: i64,
    max_jump: Option<i64>,
    pg_cron: &[String],
    dwell_ns: i64,
    cfg: &Resolved,
) {
    let subs = stops.iter().filter(|b| b.sub_step).count();
    let mut extras = Vec::new();
    if let Some(mj) = max_jump {
        extras.push(format!(
            "max jump {} ({subs} sub-steps)",
            format_span(mj).trim_start_matches('+')
        ));
    }
    if !pg_cron.is_empty() {
        extras.push(format!("pg_cron detected in {}", pg_cron.join(", ")));
    }
    if dwell_ns > 0 {
        extras.push(format!("dwell {}ms", dwell_ns / 1_000_000));
    }
    if let Some(p) = &cfg.path {
        extras.push(format!("config {}", p.display()));
    }
    println!(
        "advancing {} → {} in {} stop(s){}",
        format_utc(from),
        format_utc(stops.last().map(|b| b.at_ns).unwrap_or(from)),
        stops.len(),
        if extras.is_empty() {
            String::new()
        } else {
            format!(" — {}", extras.join(", "))
        }
    );
}

fn print_stop(r: &StopResult, json: bool) {
    if json || (r.sub_step && r.triggers.is_empty() && r.barriers.iter().all(|b| b.ok)) {
        return;
    }
    let fmt = |a: &ActionResult| {
        format!(
            "{} {} {:.2}s{}",
            a.name,
            if a.ok { "ok" } else { "FAILED" },
            a.elapsed_ms as f64 / 1000.0,
            if a.ok {
                String::new()
            } else {
                format!(" — {}", a.detail.clone().unwrap_or_default())
            }
        )
    };
    let mut parts = Vec::new();
    if r.pinned {
        parts.push("pg_cron job(s) due".to_string());
    }
    if !r.triggers.is_empty() {
        parts.push(format!(
            "trigger {}",
            r.triggers.iter().map(fmt).collect::<Vec<_>>().join(", ")
        ));
    }
    if !r.barriers.is_empty() {
        parts.push(format!(
            "barrier {}",
            r.barriers.iter().map(fmt).collect::<Vec<_>>().join(", ")
        ));
    }
    println!(
        "  {}  {}",
        r.at,
        if parts.is_empty() {
            "step".to_string()
        } else {
            parts.join("; ")
        }
    );
}
