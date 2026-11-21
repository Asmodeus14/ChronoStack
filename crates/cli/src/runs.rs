//! Reading scenario reports back: `chronostack report show|diff` and the MCP
//! `report_get` / `report_diff` tools. Reports are the JSON files `chronostack
//! run` writes to `.chronostack/reports/`.

use std::path::PathBuf;

use serde_json::{Value, json};

pub const REPORT_DIR: &str = ".chronostack/reports";

/// Scenario reports, newest first.
pub fn list_reports() -> Result<Vec<Value>, String> {
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

pub fn pick_report(reports: &[Value], run_id: Option<&str>, nth: usize) -> Result<Value, String> {
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
pub fn users_summary(u: &Value, timeline: usize) -> Value {
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

pub fn report_summary(r: &Value, timeline: usize) -> Value {
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
pub fn normalized_timeline(r: &Value) -> Vec<(i64, String, u64, String, String, String)> {
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

pub fn report_diff(a: &Value, b: &Value) -> Value {
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

/// The two reports to compare: `b` (default the latest) and `a` (default the
/// report before `b`).
pub fn diff_pair(
    reports: &[Value],
    a: Option<&str>,
    b: Option<&str>,
) -> Result<(Value, Value), String> {
    let b = pick_report(reports, b, 0)?;
    let a = match a {
        Some(id) => pick_report(reports, Some(id), 0)?,
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
    Ok((a, b))
}

fn txt(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => "-".into(),
        other => other.to_string(),
    }
}

/// `chronostack report list`.
pub fn list_text(reports: &[Value]) -> String {
    let mut o = format!(
        "{:<10}  {:<20}  {:<7}  SCENARIO\n",
        "RUN ID", "STARTED (UTC)", "RESULT"
    );
    for r in reports {
        o += &format!(
            "{:<10}  {:<20}  {:<7}  {}\n",
            txt(&r["run_id"]),
            txt(&r["real_start"]),
            txt(&r["result"]),
            txt(&r["scenario"]["name"])
        );
    }
    o
}

/// `chronostack report show` (from [`report_summary`]).
pub fn show_text(s: &Value) -> String {
    let mut o = format!(
        "{} — {} (run {}, started {})\nvirtual {} → {}, {:.1}s real time\n\n",
        txt(&s["scenario"]),
        txt(&s["result"]).to_uppercase(),
        txt(&s["run_id"]),
        txt(&s["real_start"]),
        txt(&s["virtual_start"]),
        txt(&s["virtual_end"]),
        s["real_elapsed_ms"].as_f64().unwrap_or(0.0) / 1000.0
    );
    for st in s["steps"].as_array().into_iter().flatten() {
        let mark = match st["status"].as_str() {
            Some("passed") => "✓",
            Some("failed") => "✗",
            _ => "·",
        };
        o += &format!(
            "  {mark} {}. {}  — {}\n",
            txt(&st["index"]),
            txt(&st["name"]),
            txt(&st["detail"])
        );
    }
    if let Some(f) = s["failed_step"].as_object() {
        o += &format!(
            "\nfailed at step {}: {}\n  {}\n",
            txt(&f["index"]),
            txt(&f["name"]),
            txt(&f["detail"])
        );
    }
    if let Some(u) = s.get("users").filter(|u| u.is_object()) {
        let t = &u["users"];
        o += &format!(
            "\nsimulated users (seed {}): {} completed, {} left at a branch, {} failed, {} stopped, {} not started; {} action(s) at {} stop(s)\n",
            txt(&u["seed"]),
            txt(&t["completed"]),
            txt(&t["left_at_branch"]),
            txt(&t["failed"]),
            txt(&t["stopped"]),
            txt(&t["not_started"]),
            txt(&u["actions"]),
            txt(&u["stops"])
        );
        for st in u["steps"].as_array().into_iter().flatten() {
            o += &format!(
                "  {} / {}: {} run(s), {} failed\n",
                txt(&st["journey"]),
                txt(&st["name"]),
                txt(&st["runs"]),
                txt(&st["failed"])
            );
            for e in st["errors"].as_array().into_iter().flatten() {
                o += &format!("      {}\n", txt(e));
            }
        }
        let shown = u["timeline"].as_array().map_or(0, Vec::len);
        if shown > 0 {
            o += &format!(
                "\ntimeline (first {shown} of {}):\n",
                txt(&u["timeline_entries"])
            );
            for e in u["timeline"].as_array().into_iter().flatten() {
                o += &format!(
                    "  {}  {} #{}  {}\n",
                    txt(&e["virtual_time"]),
                    txt(&e["cohort"]),
                    txt(&e["user"]),
                    txt(&e["event"])
                );
            }
        }
    }
    o
}

/// `chronostack report diff` (from [`report_diff`]).
pub fn diff_text(d: &Value) -> String {
    let (a, b) = (&d["a"], &d["b"]);
    let mut o = format!(
        "a: {} {} ({})\nb: {} {} ({})\n\n",
        txt(&a["run_id"]),
        txt(&a["result"]),
        txt(&a["real_start"]),
        txt(&b["run_id"]),
        txt(&b["result"]),
        txt(&b["real_start"])
    );
    if d["same_scenario"] == false {
        o += &format!(
            "note: different scenarios ({} vs {})\n",
            txt(&a["scenario"]),
            txt(&b["scenario"])
        );
    }
    let changes = d["step_changes"].as_array().map_or(&[][..], Vec::as_slice);
    let users_changed = d["users"].is_object()
        && (d["users"]["timeline_same"] == false
            || d["users"]["step_changes"]
                .as_array()
                .is_some_and(|c| !c.is_empty()));
    if d["result_changed"] == false && changes.is_empty() && !users_changed {
        o += "no differences: same result, same step outcomes";
        o += if d["users"].is_object() {
            ", same users timeline\n"
        } else {
            "\n"
        };
        return o;
    }
    if d["result_changed"] == true {
        o += &format!("result: {} → {}\n", txt(&a["result"]), txt(&b["result"]));
    }
    for c in changes {
        o += &format!(
            "step {}. {}: {} → {}\n",
            txt(&c["index"]),
            txt(&c["name"]),
            txt(&c["a"]),
            txt(&c["b"])
        );
    }
    if let Some(u) = d.get("users").filter(|u| u.is_object()) {
        for c in u["step_changes"].as_array().into_iter().flatten() {
            o += &format!(
                "users {} / {}: runs {} → {}, failed {} → {}\n",
                txt(&c["journey"]),
                txt(&c["name"]),
                txt(&c["runs"][0]),
                txt(&c["runs"][1]),
                txt(&c["failed"][0]),
                txt(&c["failed"][1])
            );
        }
        if u["timeline_same"] == true {
            o += "users timeline: the same\n";
        } else if let Some(f) = u["first_timeline_difference"].as_object() {
            o += &format!(
                "users timeline: differs from entry {}\n  a: {}\n  b: {}\n",
                txt(&f["position"]),
                txt(&f["a"]),
                txt(&f["b"])
            );
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
