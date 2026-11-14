//! Fault proxy planning (`up`), `chronostack fault …` and `chronostack events`.

use anyhow::{Result, bail};
use chronostack_core::api::{Event, FaultRequest, PROXY_BASE_PORT, ProxyTarget};
use chronostack_core::compose::{self, EnvRewrite, Project};
use chronostack_core::duration::{format_utc, parse_duration, parse_time_of_day, parse_utc};

use crate::client::Client;
use crate::config::ProxyCfg;

const DAY_NS: i64 = 86_400_000_000_000;

/// What `up` needs to wire the proxies in: targets for `clockd`, rewritten
/// environment variables, and the networks `clockd` must join.
#[derive(Debug)]
pub struct ProxyPlan {
    pub targets: Vec<ProxyTarget>,
    pub rewrites: Vec<EnvRewrite>,
    pub networks: Vec<String>,
}

pub fn plan(project: &Project, proxies: &[ProxyCfg]) -> Result<ProxyPlan> {
    let mut targets = Vec::new();
    let mut rewrites = Vec::new();
    let mut networks = Vec::new();
    for (i, px) in proxies.iter().enumerate() {
        let t = ProxyTarget {
            name: px.name.clone(),
            listen_port: PROXY_BASE_PORT + i as u16,
            upstream: px.upstream.trim_end_matches('/').to_string(),
        };
        let base = compose::proxy_base(&t);
        for (svc, var) in &px.env {
            let Some(s) = project.services.iter().find(|s| &s.name == svc) else {
                bail!("proxy `{}`: unknown service `{svc}`", px.name);
            };
            let Some(Some(original)) = s.environment.get(var) else {
                bail!(
                    "proxy `{}`: service `{svc}` has no value for `{var}` in the Compose file; \
                     set it there (e.g. `{var}: {}/…`) so ChronoStack can point it at the proxy",
                    px.name,
                    px.upstream
                );
            };
            let value = compose::rewrite_url(original, &t.upstream, &base)
                .map_err(|e| anyhow::anyhow!("proxy `{}`: {svc}.{var}: {e}", px.name))?;
            networks.extend(s.networks.iter().cloned());
            rewrites.push(EnvRewrite {
                service: svc.clone(),
                var: var.clone(),
                original: original.clone(),
                value,
            });
        }
        targets.push(t);
    }
    // Only declare networks when the project uses non-default ones; otherwise
    // `clockd` is already on the project's default network.
    if networks.iter().all(|n| n == "default") {
        networks.clear();
    }
    Ok(ProxyPlan {
        targets,
        rewrites,
        networks,
    })
}

/// A point in virtual time: `now`, `+<duration>` (from now), `next HH:MM`
/// (next UTC time of day, strictly after now) or an RFC 3339 UTC timestamp.
pub fn parse_when(s: &str, now: i64) -> Result<i64> {
    let s = s.trim();
    if s == "now" {
        return Ok(now);
    }
    if let Some(d) = s.strip_prefix('+') {
        return parse_duration(d)
            .map(|d| now + d)
            .map_err(anyhow::Error::msg);
    }
    if let Some(t) = s.strip_prefix("next ") {
        return Ok(next_time_of_day(
            now,
            parse_time_of_day(t.trim()).map_err(anyhow::Error::msg)?,
        ));
    }
    parse_utc(s).map_err(|e| {
        anyhow::anyhow!("`{s}`: expected `now`, `+<duration>`, `next HH:MM` or a UTC time ({e})")
    })
}

/// First instant strictly after `now` whose UTC time of day is `tod_ns`.
pub fn next_time_of_day(now: i64, tod_ns: i64) -> i64 {
    let day = now.div_euclid(DAY_NS) * DAY_NS;
    let t = day + tod_ns;
    if t > now { t } else { t + DAY_NS }
}

/// Resolve a window from `from` plus either `for` (a duration) or `until`.
pub fn window(now: i64, from: &str, dur: Option<&str>, until: Option<&str>) -> Result<(i64, i64)> {
    let start = parse_when(from, now)?;
    let end = match (dur, until) {
        (Some(d), None) => start + parse_duration(d).map_err(anyhow::Error::msg)?,
        (None, Some(u)) => parse_when(u, now)?,
        _ => bail!("give exactly one of `for` (a duration) or `until` (a time)"),
    };
    if end <= start {
        bail!("the outage window must end after it starts");
    }
    Ok((start, end))
}

pub fn set(
    client: &Client,
    target: &str,
    from: &str,
    dur: Option<&str>,
    until: Option<&str>,
    status: Option<u16>,
) -> Result<chronostack_core::api::Fault> {
    let now = client.clock()?.virtual_ns;
    let (from_ns, to_ns) = window(now, from, dur, until)?;
    client.fault_add(&FaultRequest {
        target: target.into(),
        from_ns,
        to_ns,
        status,
    })
}

pub fn print_list(client: &Client) -> Result<()> {
    let l = client.faults()?;
    if l.targets.is_empty() {
        println!("no proxy targets (declare `proxies:` in chronostack.yml, then `chronostack up`)");
        return Ok(());
    }
    println!("PROXY TARGETS");
    for t in &l.targets {
        println!(
            "  {:<16} {} → {}",
            t.name,
            compose::proxy_base(t),
            t.upstream
        );
    }
    if l.faults.is_empty() {
        println!("\nno outage windows");
        return Ok(());
    }
    let now = client.clock()?.virtual_ns;
    println!("\nOUTAGE WINDOWS (virtual time)");
    for f in &l.faults {
        let state = if now < f.from_ns {
            "scheduled"
        } else if now < f.to_ns {
            "ACTIVE"
        } else {
            "over"
        };
        println!(
            "  #{:<3} {:<16} {} → {}  HTTP {}  {state}",
            f.id,
            f.target,
            format_utc(f.from_ns),
            format_utc(f.to_ns),
            f.status
        );
    }
    Ok(())
}

/// One line per event, for `chronostack events` and reports.
pub fn describe(e: &Event) -> String {
    let what = match e.kind.as_str() {
        "proxy" => format!(
            "{} {} {} → {} ({}{})",
            e.target.as_deref().unwrap_or("?"),
            e.method.as_deref().unwrap_or("?"),
            e.path.as_deref().unwrap_or("?"),
            e.status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "-".into()),
            e.outcome.as_deref().unwrap_or("?"),
            match (e.outcome.as_deref(), e.fault_id) {
                (Some("fault"), Some(id)) => format!(" #{id}"),
                _ => String::new(),
            }
        ),
        "fault_set" => format!(
            "fault #{} set: {}",
            e.fault_id.unwrap_or(0),
            e.message.as_deref().unwrap_or("")
        ),
        "fault_clear" => format!(
            "fault #{} cleared ({})",
            e.fault_id.unwrap_or(0),
            e.target.as_deref().unwrap_or("")
        ),
        k => k.to_string(),
    };
    format!("#{:<5} {}  {what}", e.seq, format_utc(e.virtual_ns))
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: i64 = 3_600_000_000_000;

    #[test]
    fn when_expressions() {
        let now = parse_utc("2026-11-15T10:30:00Z").unwrap();
        assert_eq!(parse_when("now", now).unwrap(), now);
        assert_eq!(parse_when("+2h", now).unwrap(), now + 2 * H);
        assert_eq!(
            format_utc(parse_when("next 09:00", now).unwrap()),
            "2026-11-16T09:00:00Z"
        );
        assert_eq!(
            format_utc(parse_when("next 11:00", now).unwrap()),
            "2026-11-15T11:00:00Z"
        );
        assert_eq!(
            format_utc(parse_when("next 10:30", now).unwrap()),
            "2026-11-16T10:30:00Z",
            "strictly after now"
        );
        assert_eq!(
            parse_when("2026-12-01T00:00:00Z", now).unwrap(),
            parse_utc("2026-12-01T00:00:00Z").unwrap()
        );
        assert!(parse_when("tomorrow", now).is_err());
        assert_eq!(
            window(now, "+1h", Some("2h"), None).unwrap(),
            (now + H, now + 3 * H)
        );
        assert_eq!(
            window(now, "now", None, Some("+1h")).unwrap(),
            (now, now + H)
        );
        assert!(window(now, "now", None, None).is_err());
        assert!(window(now, "+2h", None, Some("+1h")).is_err());
    }

    #[test]
    fn proxy_plan_rewrites_env_and_collects_networks() {
        let project = compose::parse_config(
            r#"{"name":"x","services":{
              "worker":{"image":"w","environment":{"WEBHOOK_URL":"http://receiver:4000/hooks"},"networks":{"back":null}},
              "api":{"image":"a","environment":{"OTHER":"http://elsewhere/x"}}}}"#,
        )
        .unwrap();
        let cfg = |env: &[(&str, &str)]| ProxyCfg {
            name: "hooks".into(),
            upstream: "http://receiver:4000/".into(),
            env: env
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        };
        let p = plan(&project, &[cfg(&[("worker", "WEBHOOK_URL")])]).unwrap();
        assert_eq!(p.targets[0].listen_port, PROXY_BASE_PORT);
        assert_eq!(p.targets[0].upstream, "http://receiver:4000");
        assert_eq!(p.rewrites[0].value, "http://chronostack-clockd:7200/hooks");
        assert_eq!(p.networks, vec!["back"]);
        let e = plan(&project, &[cfg(&[("api", "WEBHOOK_URL")])]).unwrap_err();
        assert!(e.to_string().contains("no value"), "{e}");
        let e = plan(&project, &[cfg(&[("api", "OTHER")])]).unwrap_err();
        assert!(e.to_string().contains("does not start with"), "{e}");
    }
}
