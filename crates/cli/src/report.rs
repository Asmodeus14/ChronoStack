//! Scenario reports: `.chronostack/reports/<time>-<scenario>.json` (complete,
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
    let stamp = chronostack_core::duration::format_utc(real_ns)
        .replace(['-', ':'], "")
        .replace('T', "-")
        .trim_end_matches('Z')
        .to_string();
    let base = format!("{stamp}-{}", slug(scenario_name));
    let json_path = dir.join(format!("{base}.json"));
    let md_path = dir.join(format!("{base}.md"));
    fs::write(&json_path, serde_json::to_string_pretty(report)? + "\n")?;
    fs::write(&md_path, markdown)?;
    session::restrict(&json_path);
    session::restrict(&md_path);
    Ok((json_path, md_path))
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

    let events: Vec<&Value> = r["events"].as_array().into_iter().flatten().collect();
    let proxy: Vec<&&Value> = events.iter().filter(|e| e["kind"] == "proxy").collect();
    m += "\n## Requests through the fault proxy\n\n";
    if proxy.is_empty() {
        m += "None.\n";
    } else {
        m += "| Virtual time | Target | Request | Response | Outcome |\n|---|---|---|---|---|\n";
        for e in &proxy {
            let outcome = match str_of(e, "outcome") {
                "fault" => format!("blocked by outage window #{}", e["fault_id"]),
                "forwarded" => "delivered to upstream".to_string(),
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
}
