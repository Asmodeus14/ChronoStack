//! Probes (does each service see the virtual time?) and `doctor` checks.
//!
//! Probes run through `docker compose exec`, so ChronoStack needs no database
//! credentials or network access to the services.

use anyhow::Result;
use serde::Serialize;

use crate::client::Client;
use crate::docker::{self, Compose};
use crate::session::{ProbeSpec, Session};

/// Tolerance on top of the measured `docker exec` window.
const TOLERANCE_S: f64 = 1.0;

#[derive(Debug, Clone, Serialize)]
pub struct ProbeResult {
    pub service: String,
    pub method: &'static str,
    /// `ok`, `skew`, `unavailable`.
    pub status: &'static str,
    /// Observed minus expected virtual time, seconds.
    pub skew_s: Option<f64>,
    pub detail: Option<String>,
}

fn command(spec: &ProbeSpec) -> (&'static str, Vec<String>) {
    match spec {
        ProbeSpec::Postgres { user, db } => (
            "psql clock_timestamp()",
            vec![
                "psql".into(),
                "-U".into(),
                user.clone(),
                "-d".into(),
                db.clone(),
                "-Atc".into(),
                "select extract(epoch from clock_timestamp())".into(),
            ],
        ),
        ProbeSpec::Redis => ("redis-cli TIME", vec!["redis-cli".into(), "TIME".into()]),
        ProbeSpec::Date => ("date +%s", vec!["date".into(), "+%s".into()]),
    }
}

fn parse_seconds(spec: &ProbeSpec, stdout: &str) -> Option<f64> {
    let mut lines = stdout.lines().map(str::trim).filter(|l| !l.is_empty());
    let first: f64 = lines.next()?.parse().ok()?;
    match spec {
        ProbeSpec::Redis => Some(first + lines.next()?.parse::<f64>().ok()? / 1e6),
        _ => Some(first),
    }
}

pub fn probe_one(
    compose: &Compose,
    service: &str,
    spec: &ProbeSpec,
    client: &Client,
) -> ProbeResult {
    let (method, cmd) = command(spec);
    let unavailable = |detail: String| ProbeResult {
        service: service.into(),
        method,
        status: "unavailable",
        skew_s: None,
        detail: Some(detail),
    };
    let before = match client.clock() {
        Ok(s) => s.virtual_ns as f64 / 1e9,
        Err(e) => return unavailable(e.to_string()),
    };
    let mut args = compose.args(&["exec", "-T", service]);
    args.extend(cmd);
    let out = match docker::output(&args) {
        Ok(o) => o,
        Err(e) => return unavailable(e.to_string()),
    };
    let after = match client.clock() {
        Ok(s) => s.virtual_ns as f64 / 1e9,
        Err(e) => return unavailable(e.to_string()),
    };
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        return unavailable(
            err.lines()
                .last()
                .unwrap_or("probe command failed")
                .trim()
                .to_string(),
        );
    }
    let Some(observed) = parse_seconds(spec, &String::from_utf8_lossy(&out.stdout)) else {
        return unavailable("could not parse probe output".into());
    };
    let ok = observed >= before - TOLERANCE_S && observed <= after + TOLERANCE_S;
    ProbeResult {
        service: service.into(),
        method,
        status: if ok { "ok" } else { "skew" },
        skew_s: Some(observed - (before + after) / 2.0),
        detail: (!ok).then(|| "service does not see the virtual time".to_string()),
    }
}

/// Probe every virtualized service in parallel.
pub fn probe_all(session: &Session, client: &Client) -> Vec<ProbeResult> {
    let compose = session.compose();
    std::thread::scope(|scope| {
        let handles: Vec<_> = session
            .services
            .iter()
            .filter(|e| e.plan.virtualized)
            .map(|e| {
                let compose = &compose;
                scope.spawn(move || probe_one(compose, &e.plan.service, &e.probe, client))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("probe thread panicked"))
            .collect()
    })
}

pub fn print_probes(results: &[ProbeResult]) {
    if results.is_empty() {
        println!("no virtualized services to probe");
        return;
    }
    let w = results
        .iter()
        .map(|r| r.service.len())
        .max()
        .unwrap_or(7)
        .max(7);
    println!("{:<w$}  {:<11}  {:<22}  SKEW", "SERVICE", "STATUS", "PROBE");
    for r in results {
        let skew = r
            .skew_s
            .map(|s| format!("{s:+.2}s"))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<w$}  {:<11}  {:<22}  {}",
            r.service, r.status, r.method, skew
        );
        if let Some(d) = &r.detail {
            println!("{:<w$}  ↳ {d}", "");
        }
    }
}

/// Is the shim mapped into PID 1 of the service? `None` = could not tell.
///
/// `/proc/1/maps` is only readable by PID 1's own user (or with
/// CAP_SYS_PTRACE). Postgres and Redis drop to non-root users, so the check
/// runs as PID 1's uid.
pub fn shim_loaded(compose: &Compose, service: &str) -> Option<bool> {
    let uid_out =
        docker::output(&compose.args(&["exec", "-T", service, "stat", "-c", "%u", "/proc/1"]))
            .ok()?;
    let uid = String::from_utf8_lossy(&uid_out.stdout).trim().to_string();
    let mut args = compose.args(&["exec", "-T"]);
    if uid_out.status.success() && !uid.is_empty() && uid.chars().all(|c| c.is_ascii_digit()) {
        args.push("--user".into());
        args.push(uid);
    }
    args.extend(docker::s(&[
        service,
        "grep",
        "-q",
        "libchronoclock",
        "/proc/1/maps",
    ]));
    match docker::output(&args).ok()?.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

/// Processes in a service whose clock reads bypass the shim.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProcessReport {
    pub processes_checked: usize,
    /// Go binaries: the Go runtime reads the clock via the vDSO directly, so
    /// time is NOT virtualized even when the shim is loaded (found with Gitea
    /// in M1 external-project testing: PID 1 was a C supervisor, the server Go).
    pub go_processes: Vec<String>,
}

/// Phase 1 (any user): list processes with their uid and name. `/proc/<pid>/status`
/// and `cmdline` are world-readable.
const PROCESS_LIST: &str = r#"
for p in $(ls -d /proc/[0-9]* 2>/dev/null); do
  pid=${p#/proc/}
  [ "$pid" = "$$" ] && continue
  uid=$(awk '/^Uid:/ {print $2}' "$p/status" 2>/dev/null)
  name=$(tr '\0' '\n' < "$p/cmdline" 2>/dev/null | head -n 1)
  [ -z "$uid" ] || [ -z "$name" ] && continue
  echo "p|$pid|$uid|$name"
done
"#;

/// Phase 2 (run as the processes' own uid): inspect the *real* executable via
/// `/proc/<pid>/exe`. The command line can lie: Gitea's wrapper uses
/// `exec -a "$0"`. `.gopclntab` is a section name present in Go binaries
/// (stripped or not) and absent from C/C++ runtimes (checked: redis, postgres, node).
const GO_CHECK: &str = r#"
for pid; do
  if grep -qF .gopclntab "/proc/$pid/exe" 2>/dev/null; then echo "go|$pid"; else echo "ok|$pid"; fi
done
"#;

/// Scan every process of a running service. `None` = could not tell (no shell).
pub fn process_report(compose: &Compose, service: &str) -> Option<ProcessReport> {
    let out = docker::output(&compose.args(&[
        "exec",
        "-T",
        "--user",
        "0",
        service,
        "sh",
        "-c",
        PROCESS_LIST,
    ]))
    .ok()?;
    if !out.status.success() {
        return None;
    }
    // uid -> [(pid, name)]
    let mut by_uid: std::collections::BTreeMap<String, Vec<(String, String)>> = Default::default();
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        let mut parts = line.splitn(4, '|');
        if parts.next() != Some("p") {
            continue;
        }
        let (Some(pid), Some(uid), Some(name)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        if !uid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        by_uid
            .entry(uid.into())
            .or_default()
            .push((pid.into(), name.into()));
    }
    let mut report = ProcessReport::default();
    for (uid, procs) in &by_uid {
        let mut args = compose.args(&[
            "exec", "-T", "--user", uid, service, "sh", "-c", GO_CHECK, "sh",
        ]);
        args.extend(procs.iter().map(|(pid, _)| pid.clone()));
        let Ok(o) = docker::output(&args) else {
            continue;
        };
        for line in String::from_utf8_lossy(&o.stdout).lines() {
            let Some((kind, pid)) = line.split_once('|') else {
                continue;
            };
            report.processes_checked += 1;
            if kind == "go" {
                if let Some((_, name)) = procs.iter().find(|(p, _)| p == pid) {
                    let name = name.rsplit('/').next().unwrap_or(name).to_string();
                    if !report.go_processes.contains(&name) {
                        report.go_processes.push(name);
                    }
                }
            }
        }
    }
    Some(report)
}

/// Services Compose reports as running.
pub fn running_services(compose: &Compose) -> Result<Vec<String>> {
    let out = docker::run(&compose.args(&["ps", "--format", "json"]))?;
    let mut running = Vec::new();
    let mut push = |v: &serde_json::Value| {
        if v["State"].as_str() == Some("running") {
            if let Some(s) = v["Service"].as_str() {
                running.push(s.to_string());
            }
        }
    };
    let trimmed = out.trim();
    if trimmed.starts_with('[') {
        if let Ok(serde_json::Value::Array(items)) = serde_json::from_str(trimmed) {
            items.iter().for_each(&mut push);
        }
    } else {
        for line in trimmed.lines() {
            if let Ok(v) = serde_json::from_str(line) {
                push(&v);
            }
        }
    }
    Ok(running)
}
