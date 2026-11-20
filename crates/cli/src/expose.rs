//! `expose:` targets: container ports of the copy published on 127.0.0.1, and
//! `service://<service>:<port>/<path>` URLs that resolve to them.

use anyhow::{Result, anyhow, bail};
use chronostack_core::compose::Expose;
use serde::{Deserialize, Serialize};

/// An exposed port as `up` published it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exposed {
    pub service: String,
    pub port: u16,
    pub host_port: u16,
}

impl Exposed {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.host_port)
    }
}

/// `app:3000` → (`app`, 3000).
pub fn parse_target(s: &str) -> Result<(String, u16)> {
    let (svc, port) = s
        .rsplit_once(':')
        .ok_or_else(|| anyhow!("`{s}`: expected <service>:<port>, e.g. app:3000"))?;
    if svc.trim().is_empty() {
        bail!("`{s}`: missing the service name");
    }
    let port: u16 = port
        .parse()
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| anyhow!("`{s}`: `{port}` is not a port number (1-65535)"))?;
    Ok((svc.to_string(), port))
}

const SCHEME: &str = "service://";

/// For a `service://app:3000/path` URL: (`app`, 3000, `/path`). `None` for any
/// other URL.
pub fn parse_service_url(url: &str) -> Option<Result<(String, u16, String)>> {
    let rest = url.strip_prefix(SCHEME)?;
    let (authority, path) = match rest.find(['/', '?']) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    Some(parse_target(authority).map(|(s, p)| (s, p, path.to_string())))
}

/// Check a URL in chronostack.yml: a `service://` URL must name an exposed port.
pub fn check_url(what: &str, url: &str, expose: &[Expose]) -> Result<()> {
    let Some(parsed) = parse_service_url(url) else {
        return Ok(());
    };
    let (svc, port, _) = parsed.map_err(|e| anyhow!("{what}: {e}"))?;
    if !expose.iter().any(|e| e.service == svc && e.port == port) {
        bail!("{what}: `{url}` needs `{svc}:{port}` in `expose:`");
    }
    Ok(())
}

/// Turn a `service://` URL into `http://127.0.0.1:<host port>/…`; other URLs
/// are returned unchanged.
pub fn resolve_url(url: &str, exposed: &[Exposed]) -> Result<String> {
    let Some(parsed) = parse_service_url(url) else {
        return Ok(url.to_string());
    };
    let (svc, port, path) = parsed?;
    let e = exposed
        .iter()
        .find(|e| e.service == svc && e.port == port)
        .ok_or_else(|| {
            anyhow!(
                "`{url}`: `{svc}:{port}` is not exposed in this session; add it to `expose:` \
                 in chronostack.yml and run `chronostack down` and `up` again"
            )
        })?;
    Ok(format!("{}{path}", e.url()))
}

/// `service://` URLs used outside a session (scenario steps, triggers) resolve
/// through the session in the current directory.
pub fn resolve_with_session(url: &str) -> Result<String> {
    if parse_service_url(url).is_none() {
        return Ok(url.to_string());
    }
    let session = crate::session::Session::load()?;
    resolve_url(url, &session.exposed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exposed() -> Vec<Exposed> {
        vec![Exposed {
            service: "app".into(),
            port: 3000,
            host_port: 49213,
        }]
    }

    #[test]
    fn targets() {
        assert_eq!(parse_target("app:3000").unwrap(), ("app".into(), 3000));
        assert_eq!(parse_target("my-api:80").unwrap(), ("my-api".into(), 80));
        assert!(parse_target("app").is_err());
        assert!(parse_target(":3000").is_err());
        assert!(parse_target("app:0").is_err());
        assert!(parse_target("app:70000").is_err());
        assert!(parse_target("app:http").is_err());
    }

    #[test]
    fn service_urls_resolve_to_the_published_port() {
        let ex = exposed();
        assert_eq!(
            resolve_url("service://app:3000/billing/run?x=1", &ex).unwrap(),
            "http://127.0.0.1:49213/billing/run?x=1"
        );
        assert_eq!(
            resolve_url("service://app:3000", &ex).unwrap(),
            "http://127.0.0.1:49213"
        );
        assert_eq!(
            resolve_url("service://app:3000?q", &ex).unwrap(),
            "http://127.0.0.1:49213?q"
        );
        // Other URLs pass through.
        assert_eq!(
            resolve_url("http://localhost:8080/x", &ex).unwrap(),
            "http://localhost:8080/x"
        );
        let err = resolve_url("service://app:4000/x", &ex)
            .unwrap_err()
            .to_string();
        assert!(err.contains("`app:4000` is not exposed"), "{err}");
    }

    #[test]
    fn config_urls_must_name_an_exposed_port() {
        let expose = [Expose {
            service: "app".into(),
            port: 3000,
            host_port: None,
        }];
        assert!(check_url("t", "service://app:3000/x", &expose).is_ok());
        assert!(check_url("t", "http://anything/x", &expose).is_ok());
        let err = check_url("trigger `x`", "service://web:80/", &expose)
            .unwrap_err()
            .to_string();
        assert!(err.contains("needs `web:80` in `expose:`"), "{err}");
        assert!(check_url("t", "service://app/x", &expose).is_err());
    }
}
