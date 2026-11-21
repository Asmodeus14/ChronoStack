//! `chronostack users validate|run`: seeded user journeys against your stack.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use chronostack_users::engine::{self, Report};
use chronostack_users::model::{self, Plan};

pub struct RunOptions {
    pub seed: Option<u64>,
    pub duration_ns: Option<i64>,
    pub json: bool,
    pub junit: Option<PathBuf>,
    pub trace: bool,
}

fn load(path: &Path) -> Result<Plan> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    model::parse(&text).map_err(|e| anyhow!("invalid {}: {e}", path.display()))
}

pub fn validate(path: &Path, json: bool) -> Result<i32> {
    let plan = load(path)?;
    let users: u64 = plan.cohorts.iter().map(|c| u64::from(c.users)).sum();
    if json {
        let journeys: BTreeMap<&String, &Vec<String>> = plan.step_names.iter().collect();
        println!(
            "{}",
            serde_json::json!({
                "valid": true,
                "users": users,
                "max_parallel": plan.max_parallel,
                "cohorts": plan.cohorts.iter().map(|c| serde_json::json!({
                    "name": c.name, "users": c.users, "journey": c.journey,
                    "arrive_ns": c.arrive_ns,
                })).collect::<Vec<_>>(),
                "journeys": journeys,
            })
        );
        return Ok(0);
    }
    println!(
        "{} is valid: {} cohort(s), {users} user(s), at most {} at once",
        path.display(),
        plan.cohorts.len(),
        plan.max_parallel
    );
    for c in &plan.cohorts {
        let arrive = if c.arrive_ns > 0 {
            format!(
                ", arriving over {}",
                chronostack_core::duration::format_span(c.arrive_ns).trim_start_matches('+')
            )
        } else {
            String::new()
        };
        println!(
            "  cohort {}: {} user(s), journey {}{arrive}",
            c.name, c.users, c.journey
        );
    }
    for (j, names) in &plan.step_names {
        println!("  journey {j}: {}", names.join(" → "));
    }
    Ok(0)
}

/// Resolve the file's targets to plain URLs; `service://` ones need a session
/// with that port in `expose:`.
pub(crate) fn targets(plan: &Plan) -> Result<BTreeMap<String, String>> {
    let mut exposed = None;
    let mut out = BTreeMap::new();
    for (name, url) in &plan.file.targets {
        let resolved = if crate::expose::parse_service_url(url).is_some() {
            if exposed.is_none() {
                let s = crate::session::Session::load().with_context(|| {
                    format!("target `{name}` is {url}, which needs a running ChronoStack session")
                })?;
                exposed = Some(s.exposed);
            }
            crate::expose::resolve_url(url, exposed.as_deref().unwrap_or_default())
                .with_context(|| format!("target `{name}`"))?
        } else {
            url.clone()
        };
        out.insert(name.clone(), resolved);
    }
    Ok(out)
}

pub fn run(path: &Path, o: &RunOptions) -> Result<i32> {
    let plan = load(path)?;
    let seed = match o.seed.or(plan.file.seed) {
        Some(s) => s,
        None => {
            let mut b = [0u8; 8];
            getrandom::fill(&mut b).map_err(|e| anyhow!("no OS randomness: {e}"))?;
            u64::from_le_bytes(b) >> 11 // stays exact in JSON numbers
        }
    };
    let mut rid = [0u8; 4];
    getrandom::fill(&mut rid).map_err(|e| anyhow!("no OS randomness: {e}"))?;
    let run_id: String = rid.iter().map(|b| format!("{b:02x}")).collect();
    let targets = targets(&plan)?;
    let users: u64 = plan.cohorts.iter().map(|c| u64::from(c.users)).sum();
    if !o.json {
        println!(
            "users: {users} in {} cohort(s), at most {} at once, seed {seed}, run id {run_id}",
            plan.cohorts.len(),
            plan.max_parallel
        );
        for (name, url) in &targets {
            println!("  {name} = {url}");
        }
        println!();
    }
    let opts = engine::Options {
        seed,
        run_id,
        duration: o
            .duration_ns
            .map(|ns| Duration::from_nanos(ns.max(0) as u64)),
        request_timeout: Duration::from_secs(30),
        trace: o.trace,
    };
    let report = engine::run(&plan, &targets, &opts);
    if let Some(p) = &o.junit {
        std::fs::write(p, junit(&report, path))
            .with_context(|| format!("writing {}", p.display()))?;
    }
    if o.json {
        println!("{}", serde_json::to_string(&report)?);
    } else {
        print!("{}", text(&report, plan.max_think_ns));
    }
    Ok(if report.passed() { 0 } else { 6 })
}

fn ms(us: u64) -> String {
    if us >= 10_000 {
        format!("{:.0}ms", us as f64 / 1000.0)
    } else {
        format!("{:.1}ms", us as f64 / 1000.0)
    }
}

/// The text summary.
pub fn text(r: &Report, max_think_ns: i64) -> String {
    let mut o = table(r);
    let u = &r.users;
    notes(&mut o, r, max_think_ns, u);
    o
}

/// The per-step table, user outcomes and errors.
pub fn table(r: &Report) -> String {
    let mut o = String::new();
    let w = r
        .steps
        .iter()
        .map(|s| s.journey.len() + s.name.len() + 3)
        .max()
        .unwrap_or(4)
        .max(4);
    let _ = writeln!(
        o,
        "{:<w$}  {:>6}  {:>6}  {:>8}  {:>8}  {:>8}  {:>8}",
        "STEP", "RUNS", "FAILED", "P50", "P90", "P99", "MAX"
    );
    for s in &r.steps {
        let label = format!("{} / {}", s.journey, s.name);
        let lat = |f: fn(&chronostack_users::histogram::Summary) -> u64| {
            s.latency_us.as_ref().map_or("-".to_string(), |l| ms(f(l)))
        };
        let _ = writeln!(
            o,
            "{label:<w$}  {:>6}  {:>6}  {:>8}  {:>8}  {:>8}  {:>8}",
            s.runs,
            s.failed,
            lat(|l| l.p50),
            lat(|l| l.p90),
            lat(|l| l.p99),
            lat(|l| l.max),
        );
    }
    let u = &r.users;
    let _ = writeln!(
        o,
        "\nusers: {} completed, {} left at a branch, {} failed{}{}",
        u.completed,
        u.left_at_branch,
        u.failed,
        if u.stopped > 0 {
            format!(", {} stopped by --duration", u.stopped)
        } else {
            String::new()
        },
        if u.not_started > 0 {
            format!(", {} not started", u.not_started)
        } else {
            String::new()
        },
    );
    let failing: Vec<_> = r.steps.iter().filter(|s| !s.errors.is_empty()).collect();
    if !failing.is_empty() {
        let _ = writeln!(o, "\nerrors:");
        for s in failing {
            for e in &s.errors {
                let _ = writeln!(o, "  {} / {}: {e}", s.journey, s.name);
            }
        }
    }
    o
}

fn notes(o: &mut String, r: &Report, max_think_ns: i64, u: &chronostack_users::engine::UserTotals) {
    if r.thinks_capped > 0 {
        let _ = writeln!(
            o,
            "\nnote: {} think(s) were shortened to max_think ({}); users run in real time here",
            r.thinks_capped,
            chronostack_core::duration::format_span(max_think_ns).trim_start_matches('+')
        );
    }
    if u.started_late > 0 {
        let _ = writeln!(
            o,
            "note: {} user(s) started late because max_parallel users were already active",
            u.started_late
        );
    }
    let _ = writeln!(
        o,
        "\n{} in {:.1}s (seed {}, run id {})",
        if r.passed() { "PASSED" } else { "FAILED" },
        r.elapsed_ms as f64 / 1000.0,
        r.seed,
        r.run_id
    );
}

/// JUnit XML: one test case per journey step; a step fails if any run of it failed.
pub fn junit(r: &Report, file: &Path) -> String {
    use crate::report::xml;
    let failures = r.steps.iter().filter(|s| s.failed > 0).count();
    let skipped = r.steps.iter().filter(|s| s.runs == 0).count();
    let secs = format!("{:.3}", r.elapsed_ms as f64 / 1000.0);
    let suite = xml(&format!("users {}", file.display()));
    let mut o = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    let _ = writeln!(
        o,
        "<testsuites name=\"{suite}\" tests=\"{}\" failures=\"{failures}\" skipped=\"{skipped}\" time=\"{secs}\">",
        r.steps.len()
    );
    let _ = writeln!(
        o,
        "  <testsuite name=\"{suite}\" tests=\"{}\" failures=\"{failures}\" skipped=\"{skipped}\" time=\"{secs}\">",
        r.steps.len()
    );
    let _ = writeln!(o, "    <properties>");
    for (k, v) in [
        ("seed", r.seed.to_string()),
        ("run_id", r.run_id.clone()),
        ("users", r.users.total.to_string()),
    ] {
        let _ = writeln!(o, "      <property name=\"{k}\" value=\"{}\"/>", xml(&v));
    }
    let _ = writeln!(o, "    </properties>");
    for s in &r.steps {
        let name = xml(&format!("{} / {}. {}", s.journey, s.index, s.name));
        let time = s
            .latency_us
            .as_ref()
            .map_or("0.000".into(), |l| format!("{:.3}", l.mean / 1_000_000.0));
        let _ = write!(
            o,
            "    <testcase classname=\"{}\" name=\"{name}\" time=\"{time}\"",
            xml(&s.journey)
        );
        if s.failed > 0 {
            let msg = xml(&format!("{} of {} run(s) failed", s.failed, s.runs));
            let _ = writeln!(o, ">");
            let _ = writeln!(
                o,
                "      <failure message=\"{msg}\">{}</failure>",
                xml(&s.errors.join("\n"))
            );
            let _ = writeln!(o, "    </testcase>");
        } else if s.runs == 0 {
            let _ = writeln!(
                o,
                ">\n      <skipped message=\"no user reached this step\"/>\n    </testcase>"
            );
        } else {
            let _ = writeln!(o, "/>");
        }
    }
    let _ = writeln!(o, "  </testsuite>\n</testsuites>");
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronostack_users::engine::{StepReport, UserTotals};

    fn report(failed: u64) -> Report {
        Report {
            seed: 42,
            run_id: "ab12".into(),
            elapsed_ms: 1500,
            stopped_by_duration: false,
            thinks_capped: 1,
            users: UserTotals {
                total: 3,
                completed: 3 - failed,
                failed,
                ..Default::default()
            },
            steps: vec![
                StepReport {
                    journey: "trial".into(),
                    index: 1,
                    name: "sign <up>".into(),
                    kind: "http",
                    runs: 3,
                    failed,
                    latency_us: None,
                    errors: if failed > 0 {
                        vec!["unexpected HTTP 500".into()]
                    } else {
                        vec![]
                    },
                },
                StepReport {
                    journey: "trial".into(),
                    index: 2,
                    name: "me".into(),
                    kind: "http",
                    runs: 0,
                    failed: 0,
                    latency_us: None,
                    errors: vec![],
                },
            ],
            trace: None,
        }
    }

    #[test]
    fn junit_has_a_case_per_step() {
        let x = junit(&report(1), Path::new("j.yml"));
        assert!(x.contains(r#"tests="2" failures="1" skipped="1""#), "{x}");
        assert!(x.contains(r#"name="trial / 1. sign &lt;up&gt;""#), "{x}");
        assert!(
            x.contains(r#"<failure message="1 of 3 run(s) failed">unexpected HTTP 500</failure>"#),
            "{x}"
        );
        assert!(
            x.contains(r#"<skipped message="no user reached this step"/>"#),
            "{x}"
        );
        assert!(x.contains(r#"<property name="seed" value="42"/>"#), "{x}");
        assert!(junit(&report(0), Path::new("j.yml")).contains(r#"failures="0""#));
    }

    #[test]
    fn text_summary() {
        let t = text(&report(1), 30_000_000_000);
        assert!(t.contains("trial / sign <up>"), "{t}");
        assert!(
            t.contains("users: 2 completed, 0 left at a branch, 1 failed"),
            "{t}"
        );
        assert!(t.contains("unexpected HTTP 500"), "{t}");
        assert!(t.contains("shortened to max_think (30s)"), "{t}");
        assert!(t.contains("FAILED in 1.5s (seed 42, run id ab12)"), "{t}");
        assert!(text(&report(0), 1).contains("PASSED"));
    }
}
