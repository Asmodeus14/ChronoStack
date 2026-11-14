//! Resolved Compose model (from `docker compose config --format json`) and
//! generation of the ChronoStack override file.
//!
//! The override is written as JSON, which is valid YAML, so Compose accepts it
//! as an extra `-f` file. User files are never modified.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::api::ProxyTarget;
use crate::paths;

/// C library family of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Libc {
    Gnu,
    Musl,
}

impl Libc {
    pub fn as_str(self) -> &'static str {
        match self {
            Libc::Gnu => "gnu",
            Libc::Musl => "musl",
        }
    }
}

/// A service from the resolved Compose model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub name: String,
    pub image: Option<String>,
    pub has_build: bool,
    pub platform: Option<String>,
    /// `None` value = variable passed through from the host environment.
    pub environment: BTreeMap<String, Option<String>>,
    /// Host ports this service publishes (e.g. `80`, `127.0.0.1:3000`).
    pub published_ports: Vec<String>,
    pub container_name: Option<String>,
    /// Networks the service joins (`default` when none are declared).
    pub networks: Vec<String>,
}

/// The resolved Compose project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub services: Vec<Service>,
}

/// Parse the output of `docker compose config --format json`.
pub fn parse_config(json_text: &str) -> Result<Project, String> {
    let v: Value =
        serde_json::from_str(json_text).map_err(|e| format!("invalid compose JSON: {e}"))?;
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .ok_or("compose config has no project `name`")?
        .to_string();
    let mut services = Vec::new();
    if let Some(map) = v.get("services").and_then(Value::as_object) {
        for (svc_name, s) in map {
            let mut environment = BTreeMap::new();
            match s.get("environment") {
                Some(Value::Object(env)) => {
                    for (k, val) in env {
                        let val = match val {
                            Value::Null => None,
                            Value::String(s) => Some(s.clone()),
                            other => Some(other.to_string()),
                        };
                        environment.insert(k.clone(), val);
                    }
                }
                Some(Value::Array(list)) => {
                    for item in list.iter().filter_map(Value::as_str) {
                        match item.split_once('=') {
                            Some((k, val)) => {
                                environment.insert(k.to_string(), Some(val.to_string()))
                            }
                            None => environment.insert(item.to_string(), None),
                        };
                    }
                }
                _ => {}
            }
            services.push(Service {
                name: svc_name.clone(),
                image: s.get("image").and_then(Value::as_str).map(str::to_string),
                has_build: s.get("build").is_some(),
                platform: s
                    .get("platform")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                environment,
                published_ports: s
                    .get("ports")
                    .and_then(Value::as_array)
                    .map(|ports| {
                        ports
                            .iter()
                            .filter_map(|p| {
                                let published = match p.get("published")? {
                                    Value::String(v) if !v.is_empty() => v.clone(),
                                    Value::Number(n) => n.to_string(),
                                    _ => return None,
                                };
                                Some(match p.get("host_ip").and_then(Value::as_str) {
                                    Some(ip) if !ip.is_empty() => format!("{ip}:{published}"),
                                    _ => published,
                                })
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                container_name: s
                    .get("container_name")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                networks: match s.get("networks") {
                    Some(Value::Object(m)) => m.keys().cloned().collect(),
                    Some(Value::Array(a)) => a
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_string)
                        .collect(),
                    _ if s.get("network_mode").is_some() => vec![],
                    _ => vec!["default".to_string()],
                },
            });
        }
    }
    if services.iter().any(|s| s.name == paths::CLOCKD_SERVICE) {
        return Err(format!(
            "the project already defines a service named `{}`",
            paths::CLOCKD_SERVICE
        ));
    }
    Ok(Project { name, services })
}

/// Image name Compose uses for a service: explicit `image`, or `<project>-<service>` for builds.
pub fn image_name(project: &str, svc: &Service) -> String {
    svc.image
        .clone()
        .unwrap_or_else(|| format!("{project}-{}", svc.name))
}

/// Map Docker architecture names to `uname -m` names. `None` = unsupported.
pub fn normalize_arch(docker_arch: &str) -> Option<&'static str> {
    match docker_arch {
        "amd64" | "x86_64" => Some("x86_64"),
        "arm64" | "aarch64" | "arm64/v8" => Some("aarch64"),
        _ => None,
    }
}

/// What image inspection found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageInfo {
    pub id: String,
    /// `uname -m` style, or the raw Docker value if unsupported.
    pub arch: String,
    /// `None` = no dynamic loader found (static/scratch/distroless-static image).
    pub libc: Option<Libc>,
    /// `LD_PRELOAD` baked into the image's `ENV`, if any.
    pub image_ld_preload: Option<String>,
}

/// Per-service decision, also persisted in the session file for `doctor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServicePlan {
    pub service: String,
    pub image: String,
    pub arch: Option<String>,
    pub libc: Option<Libc>,
    pub virtualized: bool,
    /// Why the service is not virtualized.
    pub reason: Option<String>,
    /// Full `LD_PRELOAD` value set by the override.
    pub ld_preload: Option<String>,
}

/// Decide whether and how to virtualize one service.
pub fn plan_service(
    project: &str,
    svc: &Service,
    info: Option<&ImageInfo>,
    engine_arch: &str,
) -> ServicePlan {
    let image = image_name(project, svc);
    let skip = |arch: Option<String>, libc, reason: String| ServicePlan {
        service: svc.name.clone(),
        image: image.clone(),
        arch,
        libc,
        virtualized: false,
        reason: Some(reason),
        ld_preload: None,
    };
    let Some(info) = info else {
        return skip(None, None, "image could not be inspected".into());
    };
    let Some(arch) = normalize_arch(&info.arch) else {
        return skip(
            Some(info.arch.clone()),
            info.libc,
            format!("unsupported architecture `{}`", info.arch),
        );
    };
    if arch != engine_arch {
        return skip(
            Some(arch.into()),
            info.libc,
            format!(
                "image is {arch} but the Docker engine is {engine_arch} (emulated images are not supported yet)"
            ),
        );
    }
    let Some(libc) = info.libc else {
        return skip(
            Some(arch.into()),
            None,
            "no dynamic loader in image (static binary, e.g. Go or scratch): LD_PRELOAD cannot apply".into(),
        );
    };
    let ours = format!("{}/{}", paths::LIB_DIR, crate::shim_file_name(arch, libc));
    let existing = match svc.environment.get("LD_PRELOAD") {
        Some(Some(v)) => Some(v.clone()),
        _ => info.image_ld_preload.clone(),
    };
    let ld_preload = merge_ld_preload(&ours, existing.as_deref());
    ServicePlan {
        service: svc.name.clone(),
        image,
        arch: Some(arch.into()),
        libc: Some(libc),
        virtualized: true,
        reason: None,
        ld_preload: Some(ld_preload),
    }
}

/// Put our library first and keep any existing preloads (e.g. jemalloc).
pub fn merge_ld_preload(ours: &str, existing: Option<&str>) -> String {
    let mut parts = vec![ours.to_string()];
    for p in existing
        .unwrap_or("")
        .split([':', ' '])
        .filter(|p| !p.is_empty())
    {
        if !p.contains("libchronoclock-") {
            parts.push(p.to_string());
        }
    }
    parts.join(":")
}

/// Settings for the controller service.
pub struct ClockdSpec<'a> {
    pub image: &'a str,
    pub token: &'a str,
    /// Fault-proxy targets served by `clockd` (empty = no proxies).
    pub proxies: &'a [ProxyTarget],
    /// Networks `clockd` must join so services can reach the proxies.
    pub networks: &'a [String],
    /// Environment variables pointed at a proxy.
    pub env_rewrites: &'a [EnvRewrite],
}

/// One environment variable of one service, rewritten to go through a proxy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvRewrite {
    pub service: String,
    pub var: String,
    pub original: String,
    pub value: String,
}

/// Base URL of a proxy target as seen from other services.
pub fn proxy_base(t: &ProxyTarget) -> String {
    format!("http://{}:{}", paths::CLOCKD_SERVICE, t.listen_port)
}

/// Point `original` (a URL that must start with `upstream`) at the proxy,
/// keeping the rest of the URL: `http://receiver:4000/hooks?x=1` with upstream
/// `http://receiver:4000` becomes `http://chronostack-clockd:7200/hooks?x=1`.
pub fn rewrite_url(original: &str, upstream: &str, proxy: &str) -> Result<String, String> {
    let up = upstream.trim_end_matches('/');
    let head = original.get(..up.len()).unwrap_or("");
    let rest = original.get(up.len()..).unwrap_or("");
    if !head.eq_ignore_ascii_case(up)
        || !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('?'))
    {
        return Err(format!(
            "`{original}` does not start with the proxy's upstream `{upstream}`"
        ));
    }
    Ok(format!("{proxy}{rest}"))
}

/// Validate a proxy upstream: `http[s]://host[:port]`, no path, query or
/// credentials. The app always talks plain HTTP to the proxy inside the Compose
/// network; the proxy uses TLS to an `https://` upstream.
pub fn check_upstream(upstream: &str) -> Result<(), String> {
    let rest = upstream
        .strip_prefix("http://")
        .or_else(|| upstream.strip_prefix("https://"))
        .ok_or_else(|| format!("upstream `{upstream}` must start with http:// or https://"))?;
    let rest = rest.trim_end_matches('/');
    if rest.is_empty() || rest.contains(['/', '?', '#', '@']) {
        return Err(format!(
            "upstream `{upstream}` must be http[s]://host[:port] with no path, query or credentials"
        ));
    }
    Ok(())
}

/// Marker the CLI turns into Compose's `!reset` YAML tag (JSON cannot carry tags).
pub const RESET_MARKER: &str = "__chronostack_reset__";

/// Fields removed in the ChronoStack copy so it can run next to the user's own
/// stack: fixed host ports and `container_name` would collide (R13).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Isolation {
    pub service: String,
    pub dropped_ports: Vec<String>,
    pub dropped_container_name: Option<String>,
}

/// Services that need isolation.
pub fn isolation_for(project: &Project) -> Vec<Isolation> {
    project
        .services
        .iter()
        .filter(|s| !s.published_ports.is_empty() || s.container_name.is_some())
        .map(|s| Isolation {
            service: s.name.clone(),
            dropped_ports: s.published_ports.clone(),
            dropped_container_name: s.container_name.clone(),
        })
        .collect()
}

/// Build the override document. `isolate` = services whose host ports and
/// container names are reset (empty with `--keep-ports`).
pub fn generate_override(
    plans: &[ServicePlan],
    clockd: &ClockdSpec<'_>,
    isolate: &[Isolation],
) -> Value {
    let mut services = Map::new();
    let mut clockd_env = json!({ "CHRONOSTACK_TOKEN": clockd.token });
    if !clockd.proxies.is_empty() {
        clockd_env["CHRONOSTACK_PROXIES"] =
            Value::String(serde_json::to_string(clockd.proxies).expect("serializable"));
    }
    services.insert(
        paths::CLOCKD_SERVICE.into(),
        json!({
            "image": clockd.image,
            "environment": clockd_env,
            "volumes": [format!("{}:{}", paths::VOLUME, paths::MOUNT)],
            "ports": [format!("127.0.0.1::{}", paths::CLOCKD_PORT)],
            "healthcheck": {
                "test": ["CMD", "/usr/local/bin/clockd", "healthcheck"],
                "interval": "1s",
                "timeout": "3s",
                "retries": 30,
            },
            "labels": { "dev.chronostack.role": "clockd" },
        }),
    );
    for p in plans.iter().filter(|p| p.virtualized) {
        services.insert(
            p.service.clone(),
            json!({
                "volumes": [format!("{}:{}:ro", paths::VOLUME, paths::MOUNT)],
                "environment": {
                    "LD_PRELOAD": p.ld_preload,
                    "CHRONOSTACK_CLOCK_PATH": paths::CLOCK,
                },
                "depends_on": { paths::CLOCKD_SERVICE: { "condition": "service_healthy" } },
            }),
        );
    }
    if !clockd.networks.is_empty() {
        let mut nets: Vec<&String> = clockd.networks.iter().collect();
        nets.sort();
        nets.dedup();
        services[paths::CLOCKD_SERVICE]["networks"] = json!(nets);
    }
    for rw in clockd.env_rewrites {
        let entry = services
            .entry(rw.service.clone())
            .or_insert_with(|| json!({}));
        if entry.get("environment").is_none() {
            entry["environment"] = json!({});
        }
        entry["environment"][&rw.var] = Value::String(rw.value.clone());
        if entry.get("depends_on").is_none() {
            entry["depends_on"] =
                json!({ paths::CLOCKD_SERVICE: { "condition": "service_healthy" } });
        }
    }
    for iso in isolate {
        let entry = services
            .entry(iso.service.clone())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("service entries are objects");
        if !iso.dropped_ports.is_empty() {
            entry.insert("ports".into(), json!({ RESET_MARKER: [] }));
        }
        if iso.dropped_container_name.is_some() {
            entry.insert("container_name".into(), json!({ RESET_MARKER: null }));
        }
    }
    json!({ "services": services, "volumes": { paths::VOLUME: {} } })
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{
      "name": "shop",
      "services": {
        "db":   {"image": "postgres:16", "environment": {"POSTGRES_PASSWORD": "pw"}},
        "api":  {"build": {"context": "."}, "environment": {"LD_PRELOAD": "/usr/lib/libjemalloc.so.2", "HOME": null}},
        "whoami": {"image": "traefik/whoami"}
      }
    }"#;

    fn info(arch: &str, libc: Option<Libc>) -> ImageInfo {
        ImageInfo {
            id: "sha256:x".into(),
            arch: arch.into(),
            libc,
            image_ld_preload: None,
        }
    }

    #[test]
    fn parses_resolved_config() {
        let p = parse_config(CONFIG).unwrap();
        assert_eq!(p.name, "shop");
        let api = p.services.iter().find(|s| s.name == "api").unwrap();
        assert!(api.has_build && api.image.is_none());
        assert_eq!(api.environment.get("HOME"), Some(&None));
        assert_eq!(image_name(&p.name, api), "shop-api");
    }

    #[test]
    fn list_form_environment() {
        let p = parse_config(
            r#"{"name":"x","services":{"a":{"image":"i","environment":["A=1","B"]}}}"#,
        )
        .unwrap();
        assert_eq!(p.services[0].environment.get("A"), Some(&Some("1".into())));
        assert_eq!(p.services[0].environment.get("B"), Some(&None));
    }

    #[test]
    fn rejects_reserved_service_name() {
        assert!(
            parse_config(r#"{"name":"x","services":{"chronostack-clockd":{"image":"i"}}}"#)
                .is_err()
        );
    }

    #[test]
    fn plans_each_kind_of_service() {
        let p = parse_config(CONFIG).unwrap();
        let get = |n: &str| p.services.iter().find(|s| s.name == n).unwrap();

        let db = plan_service(
            "shop",
            get("db"),
            Some(&info("amd64", Some(Libc::Gnu))),
            "x86_64",
        );
        assert!(db.virtualized);
        assert_eq!(
            db.ld_preload.as_deref(),
            Some("/chronostack/lib/libchronoclock-x86_64-gnu.so")
        );

        let api = plan_service(
            "shop",
            get("api"),
            Some(&info("amd64", Some(Libc::Musl))),
            "x86_64",
        );
        assert_eq!(
            api.ld_preload.as_deref(),
            Some("/chronostack/lib/libchronoclock-x86_64-musl.so:/usr/lib/libjemalloc.so.2")
        );

        let go = plan_service("shop", get("whoami"), Some(&info("amd64", None)), "x86_64");
        assert!(!go.virtualized);
        assert!(go.reason.unwrap().contains("static"));

        let emulated = plan_service(
            "shop",
            get("db"),
            Some(&info("amd64", Some(Libc::Gnu))),
            "aarch64",
        );
        assert!(!emulated.virtualized);

        let missing = plan_service("shop", get("db"), None, "x86_64");
        assert!(!missing.virtualized);
    }

    #[test]
    fn image_level_ld_preload_is_preserved() {
        let svc = Service {
            name: "a".into(),
            image: Some("i".into()),
            has_build: false,
            platform: None,
            environment: BTreeMap::new(),
            published_ports: vec![],
            container_name: None,
            networks: vec!["default".into()],
        };
        let mut i = info("arm64", Some(Libc::Gnu));
        i.image_ld_preload = Some("/lib/a.so /lib/b.so".into());
        let plan = plan_service("p", &svc, Some(&i), "aarch64");
        assert_eq!(
            plan.ld_preload.as_deref(),
            Some("/chronostack/lib/libchronoclock-aarch64-gnu.so:/lib/a.so:/lib/b.so")
        );
    }

    #[test]
    fn merge_is_idempotent() {
        let ours = "/chronostack/lib/libchronoclock-x86_64-gnu.so";
        assert_eq!(merge_ld_preload(ours, Some(ours)), ours);
        assert_eq!(merge_ld_preload(ours, None), ours);
    }

    #[test]
    fn override_document() {
        let plans = vec![
            plan_service(
                "p",
                &parse_config(CONFIG).unwrap().services[1],
                Some(&info("amd64", Some(Libc::Gnu))),
                "x86_64",
            ),
            ServicePlan {
                service: "static".into(),
                image: "x".into(),
                arch: None,
                libc: None,
                virtualized: false,
                reason: Some("static".into()),
                ld_preload: None,
            },
        ];
        let doc = generate_override(
            &plans,
            &ClockdSpec {
                image: "chronostack/clockd:dev",
                token: "t0k",
                proxies: &[],
                networks: &[],
                env_rewrites: &[],
            },
            &[],
        );
        let svcs = doc["services"].as_object().unwrap();
        assert_eq!(
            svcs.len(),
            2,
            "clockd + one virtualized service; static service untouched"
        );
        assert_eq!(
            doc["services"]["chronostack-clockd"]["ports"][0],
            "127.0.0.1::7171"
        );
        assert_eq!(
            doc["services"]["chronostack-clockd"]["environment"]["CHRONOSTACK_TOKEN"],
            "t0k"
        );
        let db = &doc["services"]["db"];
        assert_eq!(db["volumes"][0], "chronostack:/chronostack:ro");
        assert_eq!(
            db["environment"]["CHRONOSTACK_CLOCK_PATH"],
            "/chronostack/clock"
        );
        assert_eq!(
            db["depends_on"]["chronostack-clockd"]["condition"],
            "service_healthy"
        );
        assert!(doc["volumes"]["chronostack"].is_object());
    }

    #[test]
    fn isolation_resets_ports_and_container_name() {
        let p = parse_config(
            r#"{"name":"x","services":{
              "web":{"image":"nginx","container_name":"fixed",
                     "ports":[{"mode":"ingress","target":80,"published":"8080","protocol":"tcp"},
                              {"target":443,"published":"443","host_ip":"127.0.0.1"}]},
              "db":{"image":"postgres","expose":["5432"]},
              "anon":{"image":"x","ports":[{"target":9000}]}}}"#,
        )
        .unwrap();
        let iso = isolation_for(&p);
        assert_eq!(
            iso.len(),
            1,
            "db (expose only) and anon (random host port) are untouched"
        );
        assert_eq!(iso[0].dropped_ports, vec!["8080", "127.0.0.1:443"]);
        assert_eq!(iso[0].dropped_container_name.as_deref(), Some("fixed"));
        let doc = generate_override(
            &[],
            &ClockdSpec {
                image: "i",
                token: "t",
                proxies: &[],
                networks: &[],
                env_rewrites: &[],
            },
            &iso,
        );
        assert_eq!(doc["services"]["web"]["ports"][RESET_MARKER], json!([]));
        assert!(doc["services"]["web"]["container_name"][RESET_MARKER].is_null());
        assert!(doc["services"].get("db").is_none());
    }

    #[test]
    fn proxy_urls_and_override() {
        let t = ProxyTarget {
            name: "hooks".into(),
            listen_port: 7200,
            upstream: "http://receiver:4000".into(),
        };
        let base = proxy_base(&t);
        assert_eq!(base, "http://chronostack-clockd:7200");
        assert_eq!(
            rewrite_url("http://receiver:4000/hooks?a=1", &t.upstream, &base).unwrap(),
            "http://chronostack-clockd:7200/hooks?a=1"
        );
        assert_eq!(
            rewrite_url("HTTP://Receiver:4000", "http://receiver:4000/", &base).unwrap(),
            "http://chronostack-clockd:7200"
        );
        assert!(rewrite_url("http://receiver:40001/x", &t.upstream, &base).is_err());
        assert!(rewrite_url("http://other:4000/x", &t.upstream, &base).is_err());
        assert!(check_upstream("http://receiver:4000").is_ok());
        assert!(check_upstream("https://api.partner.example").is_ok());
        assert!(check_upstream("ftp://receiver").is_err());
        assert_eq!(
            rewrite_url(
                "https://api.partner.example/v1/hooks",
                "https://api.partner.example",
                &base
            )
            .unwrap(),
            "http://chronostack-clockd:7200/v1/hooks",
            "the app talks plain HTTP to the proxy; the proxy uses TLS upstream"
        );
        assert!(check_upstream("http://receiver:4000/hooks").is_err());
        assert!(check_upstream("http://u:p@receiver").is_err());

        let p = parse_config(
            r#"{"name":"x","services":{
                "worker":{"image":"w","networks":{"back":null}},
                "host":{"image":"h","network_mode":"host"},
                "plain":{"image":"p"}}}"#,
        )
        .unwrap();
        let nets = |n: &str| {
            p.services
                .iter()
                .find(|s| s.name == n)
                .unwrap()
                .networks
                .clone()
        };
        assert_eq!(nets("worker"), vec!["back"]);
        assert!(nets("host").is_empty());
        assert_eq!(nets("plain"), vec!["default"]);

        let rw = EnvRewrite {
            service: "worker".into(),
            var: "WEBHOOK_URL".into(),
            original: "http://receiver:4000/hooks".into(),
            value: "http://chronostack-clockd:7200/hooks".into(),
        };
        let doc = generate_override(
            &[],
            &ClockdSpec {
                image: "i",
                token: "t",
                proxies: std::slice::from_ref(&t),
                networks: &["back".into(), "default".into(), "back".into()],
                env_rewrites: std::slice::from_ref(&rw),
            },
            &[],
        );
        let c = &doc["services"]["chronostack-clockd"];
        let proxies: Vec<ProxyTarget> =
            serde_json::from_str(c["environment"]["CHRONOSTACK_PROXIES"].as_str().unwrap())
                .unwrap();
        assert_eq!(proxies, vec![t]);
        assert_eq!(c["networks"], json!(["back", "default"]));
        let w = &doc["services"]["worker"];
        assert_eq!(
            w["environment"]["WEBHOOK_URL"],
            "http://chronostack-clockd:7200/hooks"
        );
        assert_eq!(
            w["depends_on"]["chronostack-clockd"]["condition"],
            "service_healthy"
        );
    }
}
