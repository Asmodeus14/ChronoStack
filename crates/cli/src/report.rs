//! Scenario reports: `.chronostack/reports/<time>-<scenario>-<run id>.json` (complete,
//! machine-readable) and `.md` (for people). Nothing is sent anywhere.
//!
//! Text that comes from your services (command output, request paths) is
//! truncated and passed through [`redact`] before it is written.

use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

use anyhow::Result;
use regex::Regex;
use serde_json::Value;

use crate::session;

/// Longest captured output kept per field.
pub const MAX_TEXT: usize = 4096;

fn patterns() -> &'static [(Regex, &'static str)] {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        vec![
            // user:password@ in URLs
            (
                Regex::new(r"(?i)\b([a-z][a-z0-9+.\-]*://[^\s:/@]+):[^\s@/]+@").unwrap(),
                "$1:[REDACTED]@",
            ),
            // Authorization: Bearer xxx / Basic xxx
            (
                Regex::new(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=\-]{8,}").unwrap(),
                "$1 [REDACTED]",
            ),
            // key=value, key: value, "key": "value"
            (
                Regex::new(
                    r#"(?i)\b([a-z0-9_\-]*?(?:password|passwd|pwd|secret|token|api[_\-]?key|access[_\-]?key|private[_\-]?key|auth))("?\s*[=:]\s*"?)([^\s"',;&}]+)"#,
                )
                .unwrap(),
                "$1$2[REDACTED]",
            ),
            // Bare tokens: 32+ hex characters (session ids, API keys, hashes)
            // or 40+ characters of base64/base64url without spaces.
            (
                Regex::new(r"\b[A-Fa-f0-9]{32,}\b|\b[A-Za-z0-9_\-+/]{40,}={0,2}").unwrap(),
                "[REDACTED]",
            ),
        ]
    })
}

/// Mask likely secrets and cap the length.
pub fn redact(s: &str) -> String {
    let mut out = s.to_string();
    for (re, rep) in patterns() {
        out = re.replace_all(&out, *rep).into_owned();
    }
    if out.len() > MAX_TEXT {
        let mut cut = MAX_TEXT;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push_str(" …[truncated]");
    }
    out
}

fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "scenario".into()
    } else {
        s.chars().take(40).collect()
    }
}

/// Write both files; returns `(json_path, md_path)`.
pub fn write(
    report: &Value,
    markdown: &str,
    scenario_name: &str,
    real_ns: i64,
) -> Result<(PathBuf, PathBuf)> {
    let dir = PathBuf::from(session::DIR).join("reports");
    fs::create_dir_all(&dir)?;
    let run_id = report["run_id"].as_str().unwrap_or("run");
    let base = report_base(real_ns, scenario_name, run_id);
    let json_path = dir.join(format!("{base}.json"));
    let md_path = dir.join(format!("{base}.md"));
    fs::write(&json_path, serde_json::to_string_pretty(report)? + "\n")?;
    fs::write(&md_path, markdown)?;
    session::restrict(&json_path);
    session::restrict(&md_path);
    Ok((json_path, md_path))
}

/// `YYYYMMDD-HHMMSS.mmm-<scenario>-<run id>`: unique for every run (two runs of a
/// scenario can start within the same second), and file names sort by start time.
fn report_base(real_ns: i64, scenario_name: &str, run_id: &str) -> String {
    let stamp = chronostack_core::duration::format_utc(real_ns)
        .replace(['-', ':'], "")
        .replace('T', "-")
        .trim_end_matches('Z')
        .to_string();
    let ms = real_ns.rem_euclid(1_000_000_000) / 1_000_000;
    format!("{stamp}.{ms:03}-{}-{run_id}", slug(scenario_name))
}

/// Escape text for XML 1.0 attributes and content, dropping characters XML
/// can't carry (control characters other than tab, newline and carriage return).
pub(crate) fn xml(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            '\t' | '\n' | '\r' => o.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => {}
            c => o.push(c),
        }
    }
    o
}

fn secs(ms: &Value) -> String {
    format!("{:.3}", ms.as_u64().unwrap_or(0) as f64 / 1000.0)
}

/// JUnit XML for CI test reporters: one `<testsuite>` per scenario and one
/// `<testcase>` per step (failed → `<failure>`, not run → `<skipped>`). Built
/// only from the JSON report, so it always agrees with the other two files.
pub fn junit(r: &Value) -> String {
    let sc = &r["scenario"];
    let name = str_of(sc, "name");
    let class = format!("chronostack.{}", slug(name));
    let steps: &[Value] = r["steps"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    // Simulated users: one test case per journey step.
    let user_steps: &[Value] = r["users"]["steps"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let user_failed = user_steps
        .iter()
        .filter(|s| s["failed"].as_u64().unwrap_or(0) > 0)
        .count();
    let count = |s: &str| {
        steps.iter().filter(|x| x["status"] == s).count()
            + if s == "failed" { user_failed } else { 0 }
    };
    let total = steps.len() + user_steps.len();
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x += &format!(
        "<testsuites name=\"chronostack\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"0\" time=\"{}\">\n",
        total,
        count("failed"),
        count("skipped"),
        secs(&r["real_elapsed_ms"])
    );
    x += &format!(
        "  <testsuite name=\"{}\" tests=\"{}\" failures=\"{}\" skipped=\"{}\" errors=\"0\" time=\"{}\" timestamp=\"{}\">\n",
        xml(name),
        total,
        count("failed"),
        count("skipped"),
        secs(&r["real_elapsed_ms"]),
        xml(str_of(r, "real_start").trim_end_matches('Z'))
    );
    x += "    <properties>\n";
    for k in [
        "run_id",
        "virtual_start",
        "virtual_end",
        "chronostack_version",
        "project",
    ] {
        x += &format!(
            "      <property name=\"{k}\" value=\"{}\"/>\n",
            xml(str_of(r, k))
        );
    }
    x += &format!(
        "      <property name=\"scenario_path\" value=\"{}\"/>\n",
        xml(str_of(sc, "path"))
    );
    x += "    </properties>\n";
    for st in steps {
        let case = format!(
            "{:02}. {}",
            st["index"].as_u64().unwrap_or(0),
            str_of(st, "name")
        );
        let open = format!(
            "    <testcase name=\"{}\" classname=\"{}\" time=\"{}\"",
            xml(&case),
            xml(&class),
            secs(&st["elapsed_ms"])
        );
        let detail = str_of(st, "detail");
        let virt = format!(
            "virtual {} -> {}",
            str_of(st, "virtual_before"),
            str_of(st, "virtual_after")
        );
        match str_of(st, "status") {
            "skipped" => {
                x += &format!(
                    "{open}>\n      <skipped message=\"not run: an earlier step failed\"/>\n    </testcase>\n"
                );
            }
            "failed" => {
                let first = detail.lines().next().unwrap_or("step failed");
                x += &format!(
                    "{open}>\n      <failure message=\"{}\" type=\"{}\">{}\n{}</failure>\n    </testcase>\n",
                    xml(first),
                    xml(str_of(st, "kind")),
                    xml(&virt),
                    xml(detail)
                );
            }
            _ => {
                x += &format!(
                    "{open}>\n      <system-out>{}\n{}</system-out>\n    </testcase>\n",
                    xml(&virt),
                    xml(detail)
                );
            }
        }
    }
    for s in user_steps {
        let case = format!(
            "users / {} / {}. {}",
            str_of(s, "journey"),
            s["index"].as_u64().unwrap_or(0),
            str_of(s, "name")
        );
        let open = format!(
            "    <testcase name=\"{}\" classname=\"{}\" time=\"0\"",
            xml(&case),
            xml(&format!("{class}.users"))
        );
        let failed = s["failed"].as_u64().unwrap_or(0);
        if failed > 0 {
            let errs: Vec<&str> = s["errors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            x += &format!(
                "{open}>\n      <failure message=\"{}\" type=\"users\">{}</failure>\n    </testcase>\n",
                xml(&format!("{failed} of {} run(s) failed", s["runs"])),
                xml(&errs.join("\n"))
            );
        } else {
            x += &format!(
                "{open}>\n      <system-out>{} run(s)</system-out>\n    </testcase>\n",
                s["runs"]
            );
        }
    }
    x += "  </testsuite>\n</testsuites>\n";
    x
}

/// Write the JUnit file to `path`, creating parent directories.
pub fn write_junit(report: &Value, path: &std::path::Path) -> Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    fs::write(path, junit(report))?;
    Ok(())
}

fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ").replace('\r', "")
}

fn str_of<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

/// Human-readable report. Built only from the JSON report, so both always agree.
pub fn markdown(r: &Value) -> String {
    let mut m = String::new();
    let sc = &r["scenario"];
    let passed = r["result"] == "passed";
    m += &format!("# Scenario report: {}\n\n", str_of(sc, "name"));
    if let Some(d) = sc.get("description").and_then(Value::as_str) {
        m += &format!("{}\n\n", d.trim());
    }
    m += &format!(
        "**Result: {}**  \n",
        if passed { "PASSED ✅" } else { "FAILED ❌" }
    );
    m += &format!(
        "Virtual time: {} → {}  \nReal time: {:.1} s (started {})  \nScenario file: `{}` · ChronoStack project `{}`\n\n",
        str_of(r, "virtual_start"),
        str_of(r, "virtual_end"),
        r["real_elapsed_ms"].as_f64().unwrap_or(0.0) / 1000.0,
        str_of(r, "real_start"),
        str_of(sc, "path"),
        str_of(r, "project"),
    );

    if let Some(f) = r.get("failed_step").filter(|f| !f.is_null()) {
        m += "## What failed\n\n";
        m += &format!(
            "Step {} of {}, **{}**, at virtual time {}:\n\n```\n{}\n```\n\n",
            f["index"],
            r["steps"].as_array().map(|a| a.len()).unwrap_or(0),
            str_of(f, "name"),
            str_of(f, "virtual_time"),
            str_of(f, "detail")
        );
        m += "The steps after it were not run. The proxy traffic and outage windows below show what the services did up to that point.\n\n";
    }

    m += "## Steps\n\n| # | Step | Virtual time (after) | Result | Details |\n|---|---|---|---|---|\n";
    for s in r["steps"].as_array().into_iter().flatten() {
        let status = match str_of(s, "status") {
            "passed" => "✅ passed",
            "failed" => "❌ failed",
            other => other,
        };
        m += &format!(
            "| {} | {} | {} | {} | {} |\n",
            s["index"],
            cell(str_of(s, "name")),
            str_of(s, "virtual_after"),
            status,
            cell(str_of(s, "detail"))
        );
    }

    if let Some(u) = r.get("users").filter(|u| u.is_object()) {
        let t = &u["users"];
        m += &format!(
            "\n## Simulated users\n\nSeed {}, actions on a {} s grid: {} action(s) at {} stop(s). {} completed, {} left at a branch, {} failed, {} stopped when the scenario ended, {} not started.\n\n",
            u["seed"],
            u["resolution_s"],
            u["actions"],
            u["stops"],
            t["completed"],
            t["left_at_branch"],
            t["failed"],
            t["stopped"],
            t["not_started"]
        );
        m += "| Journey / step | Runs | Failed | p50 | p99 | Errors |\n|---|---|---|---|---|---|\n";
        for s in u["steps"].as_array().into_iter().flatten() {
            let ms = |k: &str| {
                s["latency_us"][k]
                    .as_u64()
                    .map_or("-".to_string(), |v| format!("{:.1} ms", v as f64 / 1000.0))
            };
            let errs: Vec<&str> = s["errors"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            m += &format!(
                "| {} / {} | {} | {} | {} | {} | {} |\n",
                cell(str_of(s, "journey")),
                cell(str_of(s, "name")),
                s["runs"],
                s["failed"],
                ms("p50"),
                ms("p99"),
                cell(&errs.join("; "))
            );
        }
        m += "\nThe full issue timeline (who did what at which virtual time) is in the JSON report under `users.timeline`.\n";
    }

    let events: Vec<&Value> = r["events"].as_array().into_iter().flatten().collect();
    let proxy: Vec<&&Value> = events.iter().filter(|e| e["kind"] == "proxy").collect();
    m += "\n## Requests through the fault proxy\n\n";
    if proxy.is_empty() {
        m += "None.\n";
    } else {
        m += "| Virtual time | Target | Request | Response | Outcome |\n|---|---|---|---|---|\n";
        for e in &proxy {
            let delayed = match e["delay_ms"].as_u64() {
                Some(d) => format!(
                    " after a {} s delay (window #{})",
                    d as f64 / 1000.0,
                    e["fault_id"]
                ),
                None => String::new(),
            };
            let outcome = match str_of(e, "outcome") {
                "fault" => format!(
                    "blocked by outage window #{}{}",
                    e["fault_id"],
                    if delayed.is_empty() {
                        String::new()
                    } else {
                        format!(
                            ", after {} s",
                            e["delay_ms"].as_u64().unwrap_or(0) as f64 / 1000.0
                        )
                    }
                ),
                "forwarded" => format!("delivered to upstream{delayed}"),
                "upstream_error" => format!("upstream unreachable: {}", cell(str_of(e, "message"))),
                o => o.to_string(),
            };
            m += &format!(
                "| {} | {} | {} {} | {} | {} |\n",
                str_of(e, "virtual_time"),
                str_of(e, "target"),
                str_of(e, "method"),
                cell(str_of(e, "path")),
                e["status"],
                outcome
            );
        }
    }
    if r["events_truncated"] == true {
        m += "\n*Older events were dropped from the controller's buffer (it keeps the last 1000).*\n";
    }

    m += "\n## Outage windows\n\n";
    let faults: Vec<&Value> = r["faults"].as_array().into_iter().flatten().collect();
    if faults.is_empty() {
        m += "None.\n";
    } else {
        m += "| # | Target | From (virtual) | Until (virtual) | Returns |\n|---|---|---|---|---|\n";
        for f in faults {
            m += &format!(
                "| {} | {} | {} | {} | HTTP {} |\n",
                f["id"],
                str_of(f, "target"),
                str_of(f, "from"),
                str_of(f, "until"),
                f["status"]
            );
        }
    }

    m += "\n## Services at the end\n\n";
    let probes: Vec<&Value> = r["probes"].as_array().into_iter().flatten().collect();
    if probes.is_empty() {
        m += "Not probed.\n";
    } else {
        m += "| Service | Sees virtual time? | Detail |\n|---|---|---|\n";
        for p in probes {
            m += &format!(
                "| {} | {} | {} |\n",
                str_of(p, "service"),
                str_of(p, "status"),
                cell(str_of(p, "detail"))
            );
        }
    }
    m += &format!(
        "\n## Reproduce\n\n```sh\nchronostack run {}\n```\n\nThe JSON version of this report has every clock stop, trigger output and event.\n",
        str_of(sc, "path")
    );
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_names_are_unique_and_sort_by_time() {
        // 2026-11-21T07:49:37.512Z and .901Z: two runs in the same second.
        let t = 1_795_247_377_512_000_000;
        let a = report_base(t, "Users over three days", "aaaa1111");
        let b = report_base(t + 389_000_000, "Users over three days", "bbbb2222");
        assert_ne!(a, b, "same scenario, same second: still two files");
        assert!(a < b, "{a} sorts before {b}");
        assert!(a.ends_with("-aaaa1111"), "{a}");
        assert!(a.contains(".512-"), "{a}");
        // A later second still sorts after, whatever the milliseconds.
        assert!(b < report_base(t + 1_000_000_000 - 512_000_000, "a", "z"));
    }

    #[test]
    fn redacts_common_secrets() {
        let cases = [
            (
                "postgres://app:s3cret@db:5432/app",
                "postgres://app:[REDACTED]@db:5432/app",
            ),
            (
                "Authorization: Bearer abcdefghijklmnop",
                "Authorization: Bearer [REDACTED]",
            ),
            ("PGPASSWORD=hunter2 next", "PGPASSWORD=[REDACTED] next"),
            (
                r#"{"api_key": "xyz123", "n": 1}"#,
                r#"{"api_key": "[REDACTED]", "n": 1}"#,
            ),
            ("token: abc.def", "token: [REDACTED]"),
            (
                "ACCESS_KEY=AKIA1234 and plain text",
                "ACCESS_KEY=[REDACTED] and plain text",
            ),
            ("expired 1 trial(s)", "expired 1 trial(s)"),
            ("17af00b632dd389ba6f8834479377cb3", "[REDACTED]"),
            (
                "token abc eyJhbGciOiJIUzI1NiJ9eyJzdWIiOiIxMjM0NTY3ODkwIn0x",
                "token abc [REDACTED]",
            ),
            (
                "user 550e8400-e29b-41d4-a716-446655440000",
                "user 550e8400-e29b-41d4-a716-446655440000",
            ),
        ];
        for (input, want) in cases {
            assert_eq!(redact(input), want, "input: {input}");
        }
        let long = "log line ".repeat(MAX_TEXT / 9 + 10);
        assert!(redact(&long).ends_with("…[truncated]"));
    }

    #[test]
    fn slugs() {
        assert_eq!(
            slug("Trial expires & webhook survives!"),
            "trial-expires-webhook-survives"
        );
        assert_eq!(slug("***"), "scenario");
    }

    #[test]
    fn markdown_explains_a_failure() {
        let r = serde_json::json!({
            "scenario": {"name": "trial", "path": "scenarios/trial.yml"},
            "result": "failed",
            "virtual_start": "2026-11-15T00:00:00Z", "virtual_end": "2026-11-29T12:00:00Z",
            "real_elapsed_ms": 12000, "real_start": "x", "project": "p",
            "failed_step": {"index": 2, "name": "receiver got it once", "virtual_time": "2026-11-29T12:00:00Z",
                            "detail": "expected `1`, got `(empty)`"},
            "steps": [
                {"index": 1, "name": "advance 14d", "virtual_after": "a", "status": "passed", "detail": "14 stops"},
                {"index": 2, "name": "receiver got it once", "virtual_after": "b", "status": "failed", "detail": "expected `1`"}
            ],
            "events": [{"kind": "proxy", "virtual_time": "t", "target": "webhooks", "method": "POST",
                        "path": "/hooks", "status": 503, "outcome": "fault", "fault_id": 1}],
            "faults": [{"id": 1, "target": "webhooks", "from": "f", "until": "u", "status": 503}],
            "probes": []
        });
        let m = markdown(&r);
        assert!(m.contains("FAILED"));
        assert!(m.contains("## What failed"));
        assert!(m.contains("Step 2 of 2, **receiver got it once**"));
        assert!(m.contains("blocked by outage window #1"));
        assert!(m.contains("chronostack run scenarios/trial.yml"));
    }

    fn sample() -> Value {
        serde_json::json!({
            "scenario": { "name": "Trial <expiry> & renewal", "path": "scenarios/trial.yml" },
            "run_id": "ab12cd34", "chronostack_version": "0.1.0", "project": "demo-chronostack",
            "real_start": "2026-11-18T10:00:00Z", "real_elapsed_ms": 12345,
            "virtual_start": "2026-11-18T10:00:00Z", "virtual_end": "2026-12-03T10:00:00Z",
            "result": "failed",
            "steps": [
                { "index": 1, "name": "advance 15d", "kind": "advance", "status": "passed", "elapsed_ms": 9000,
                  "virtual_before": "2026-11-18T10:00:00Z", "virtual_after": "2026-12-03T10:00:00Z", "detail": "15 stop(s)" },
                { "index": 2, "name": "trial expired", "kind": "assert", "status": "failed", "elapsed_ms": 250,
                  "virtual_before": "2026-12-03T10:00:00Z", "virtual_after": "2026-12-03T10:00:00Z",
                  "detail": "expected \"expired\", got \"active\"\nquery: select status from trials\u{1}" },
                { "index": 3, "name": "webhook sent", "kind": "assert", "status": "skipped" }
            ]
        })
    }

    #[test]
    fn junit_counts_and_cases() {
        let x = junit(&sample());
        assert!(x.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"));
        assert!(x.contains(
            "<testsuite name=\"Trial &lt;expiry&gt; &amp; renewal\" tests=\"3\" failures=\"1\" skipped=\"1\" errors=\"0\" time=\"12.345\" timestamp=\"2026-11-18T10:00:00\">"
        ));
        assert!(x.contains(
            "name=\"01. advance 15d\" classname=\"chronostack.trial-expiry-renewal\" time=\"9.000\""
        ));
        assert!(x.contains("<failure message=\"expected &quot;expired&quot;, got &quot;active&quot;\" type=\"assert\">"));
        assert!(x.contains("<skipped message="));
        assert!(x.contains("<property name=\"run_id\" value=\"ab12cd34\"/>"));
        assert!(!x.contains('\u{1}'), "control characters are dropped");
        assert_eq!(x.matches("<testcase ").count(), 3);
        assert_eq!(x.matches("</testcase>").count(), 3);
    }

    #[test]
    fn junit_for_a_passing_run() {
        let mut r = sample();
        r["steps"] = serde_json::json!([r["steps"][0].clone()]);
        let x = junit(&r);
        assert!(x.contains("tests=\"1\" failures=\"0\" skipped=\"0\""));
        assert!(x.contains("<system-out>virtual 2026-11-18T10:00:00Z -&gt; 2026-12-03T10:00:00Z\n15 stop(s)</system-out>"));
    }
}
