//! Resolved Compose model (from `docker compose config --format json`) and
//! generation of the ChronoStack override file.
//!
//! The override is built here as JSON and written as YAML by the CLI
//! (`crates/cli/src/yaml.rs`), so it can carry Compose's `!reset` tag; Compose
//! reads it as an extra `-f` file. User files are never modified.

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
    /// `command` from the Compose file, split into words (empty when not set).
    pub command: Vec<String>,
    /// `entrypoint` from the Compose file, split into words (empty when not set).
    pub entrypoint: Vec<String>,
}

/// A Compose `command`/`entrypoint` value: a list, or a string split on whitespace.
fn words(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => s.split_whitespace().map(str::to_string).collect(),
        _ => vec![],
    }
}

/// The resolved Compose project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub services: Vec<Service>,
    /// Mounts the ChronoStack copy would share with the user's own stack.
    pub shared_mounts: Vec<SharedMount>,
}

/// Why a mount is shared between the user's stack and the ChronoStack copy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SharedKind {
    /// A host file or directory.
    Bind,
    /// A volume declared `external: true`.
    ExternalVolume,
    /// A volume with a fixed `name:`, which Compose does not prefix with the project.
    NamedVolume,
}

impl SharedKind {
    pub fn describe(self) -> &'static str {
        match self {
            SharedKind::Bind => "bind mount",
            SharedKind::ExternalVolume => "external volume",
            SharedKind::NamedVolume => "volume with a fixed name",
        }
    }
}

/// A mount the copy would share with the user's own stack. Ordinary named
/// volumes are not shared: Compose names them `<project>_<key>`, and the copy
/// runs under its own project name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedMount {
    pub service: String,
    pub kind: SharedKind,
    /// Host path (bind mount) or Docker volume name.
    pub source: String,
    /// Path inside the container.
    pub target: String,
    pub read_only: bool,
}

/// Find the shared mounts in a resolved model (`docker compose config --format
/// json`, loaded under the user's own project name).
fn shared_mounts(v: &Value, project: &str) -> Vec<SharedMount> {
    let top = v.get("volumes").and_then(Value::as_object);
    let mut out = Vec::new();
    let Some(services) = v.get("services").and_then(Value::as_object) else {
        return out;
    };
    for (svc, s) in services {
        for m in s
            .get("volumes")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(source) = m.get("source").and_then(Value::as_str) else {
                continue; // anonymous volume or tmpfs: private to the container
            };
            let kind = match m.get("type").and_then(Value::as_str) {
                Some("bind") => SharedKind::Bind,
                Some("volume") => {
                    let decl = top.and_then(|t| t.get(source));
                    let name = decl.and_then(|d| d.get("name")).and_then(Value::as_str);
                    if decl
                        .and_then(|d| d.get("external"))
                        .and_then(Value::as_bool)
                        == Some(true)
                    {
                        SharedKind::ExternalVolume
                    } else if name.is_some_and(|n| n != format!("{project}_{source}")) {
                        SharedKind::NamedVolume
                    } else {
                        continue;
                    }
                }
                _ => continue,
            };
            let name = match kind {
                SharedKind::Bind => source,
                _ => top
                    .and_then(|t| t.get(source))
                    .and_then(|d| d.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or(source),
            };
            out.push(SharedMount {
                service: svc.clone(),
                kind,
                source: name.to_string(),
                target: m
                    .get("target")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                read_only: m.get("read_only").and_then(Value::as_bool) == Some(true),
            });
        }
    }
    out
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
                command: words(s.get("command")),
                entrypoint: words(s.get("entrypoint")),
            });
        }
    }
    if services.iter().any(|s| s.name == paths::CLOCKD_SERVICE) {
        return Err(format!(
            "the project already defines a service named `{}`",
            paths::CLOCKD_SERVICE
        ));
    }
    let shared_mounts = shared_mounts(&v, &name);
    Ok(Project {
        name,
        services,
        shared_mounts,
    })
}

/// What kind of datastore a service is, by image name and environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreKind {
    Postgres,
    Redis,
    Other,
}

/// Classify a service. App containers often set `POSTGRES_*` too (to
/// connect), so callers must tolerate an app being classified as Postgres.
pub fn store_kind(svc: &Service, image: &str) -> StoreKind {
    let img = image.to_ascii_lowercase();
    let env = |k: &str| matches!(svc.environment.get(k), Some(Some(_)));
    if ["postgres", "pgvector", "postgis", "timescale"]
        .iter()
        .any(|name| img.contains(name))
        || env("POSTGRES_PASSWORD")
        || env("POSTGRES_USER")
    {
        StoreKind::Postgres
    } else if img.contains("redis") || img.contains("valkey") {
        StoreKind::Redis
    } else {
        StoreKind::Other
    }
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
/// Marker the CLI turns into Compose's `!override` YAML tag: the value replaces
/// the user's instead of being merged into it.
pub const OVERRIDE_MARKER: &str = "__chronostack_override__";

/// A container port of the copy published on the host's loopback interface
/// (`expose:` in chronostack.yml), so host-side tools can reach it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expose {
    pub service: String,
    /// Port inside the container.
    pub port: u16,
    /// Fixed host port, or `None` for one Docker picks.
    pub host_port: Option<u16>,
}

impl Expose {
    /// Compose short syntax, bound to 127.0.0.1 only.
    pub fn compose_port(&self) -> String {
        match self.host_port {
            Some(h) => format!("127.0.0.1:{h}:{}", self.port),
            None => format!("127.0.0.1::{}", self.port),
        }
    }
}

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
/// container names are reset (empty with `--keep-ports`). `expose` = ports to
/// publish on 127.0.0.1; for an isolated service they replace its own ports.
pub fn generate_override(
    plans: &[ServicePlan],
    clockd: &ClockdSpec<'_>,
    isolate: &[Isolation],
    expose: &[Expose],
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
    let exposed_ports = |service: &str| -> Vec<String> {
        expose
            .iter()
            .filter(|e| e.service == service)
            .map(Expose::compose_port)
            .collect()
    };
    for iso in isolate {
        let ports = exposed_ports(&iso.service);
        let entry = services
            .entry(iso.service.clone())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("service entries are objects");
        if !iso.dropped_ports.is_empty() {
            let value = if ports.is_empty() {
                json!({ RESET_MARKER: [] })
            } else {
                json!({ OVERRIDE_MARKER: ports })
            };
            entry.insert("ports".into(), value);
        }
        if iso.dropped_container_name.is_some() {
            entry.insert("container_name".into(), json!({ RESET_MARKER: null }));
        }
    }
    // Exposed services whose own ports weren't replaced above: add to them
    // (Compose merges `ports` lists from several files).
    let mut seen = std::collections::BTreeSet::new();
    for e in expose {
        if !seen.insert(e.service.as_str()) {
            continue;
        }
        let replaced = isolate
            .iter()
            .any(|i| i.service == e.service && !i.dropped_ports.is_empty());
        if replaced {
            continue;
        }
        let entry = services
            .entry(e.service.clone())
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .expect("service entries are objects");
        entry.insert("ports".into(), json!(exposed_ports(&e.service)));
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

    /// Shape taken from `docker compose config --format json` (Compose 2.x).
    const MOUNTS: &str = r#"{
      "name": "mnt",
      "services": {
        "db": {"image": "postgres:16", "volumes": [
          {"type": "bind", "source": "/home/me/app/data", "target": "/var/lib/postgresql/data", "bind": {}},
          {"type": "bind", "source": "/home/me/app/conf", "target": "/etc/conf", "read_only": true, "bind": {}},
          {"type": "volume", "source": "shared-db", "target": "/backup", "volume": {}},
          {"type": "volume", "source": "fixed", "target": "/fixed", "volume": {}},
          {"type": "volume", "source": "db-data", "target": "/normal", "volume": {}},
          {"type": "tmpfs", "target": "/tmp/x"},
          {"type": "volume", "target": "/just-anon", "volume": {}}
        ]},
        "web": {"image": "nginx"}
      },
      "volumes": {
        "db-data": {"name": "mnt_db-data"},
        "fixed": {"name": "my-fixed-vol"},
        "shared-db": {"name": "shared-db", "external": true}
      }
    }"#;

    #[test]
    fn finds_mounts_shared_with_the_users_stack() {
        let p = parse_config(MOUNTS).unwrap();
        let got: Vec<_> = p
            .shared_mounts
            .iter()
            .map(|m| (m.kind, m.source.as_str(), m.target.as_str(), m.read_only))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    SharedKind::Bind,
                    "/home/me/app/data",
                    "/var/lib/postgresql/data",
                    false
                ),
                (SharedKind::Bind, "/home/me/app/conf", "/etc/conf", true),
                (SharedKind::ExternalVolume, "shared-db", "/backup", false),
                (SharedKind::NamedVolume, "my-fixed-vol", "/fixed", false),
            ]
        );
        assert!(p.shared_mounts.iter().all(|m| m.service == "db"));
    }

    #[test]
    fn ordinary_volumes_are_not_shared() {
        assert!(parse_config(CONFIG).unwrap().shared_mounts.is_empty());
        // A volume whose name is the project default is copied per project.
        let only_default = MOUNTS
            .replace(r#""name": "my-fixed-vol""#, r#""name": "mnt_fixed""#)
            .replace(r#", "external": true"#, "")
            .replace(r#""name": "shared-db""#, r#""name": "mnt_shared-db""#)
            .replace(r#""type": "bind""#, r#""type": "tmpfs""#);
        assert!(
            parse_config(&only_default)
                .unwrap()
                .shared_mounts
                .is_empty()
        );
    }

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
    fn command_and_entrypoint_in_both_forms() {
        let p = parse_config(
            r#"{"name":"x","services":{
                "a":{"image":"i","command":["celery","-A","app","beat"],"entrypoint":"/bin/sh -c"},
                "b":{"image":"i","command":"bundle exec  sidekiq"},
                "c":{"image":"i"}}}"#,
        )
        .unwrap();
        assert_eq!(p.services[0].command, ["celery", "-A", "app", "beat"]);
        assert_eq!(p.services[0].entrypoint, ["/bin/sh", "-c"]);
        assert_eq!(p.services[1].command, ["bundle", "exec", "sidekiq"]);
        assert!(p.services[2].command.is_empty() && p.services[2].entrypoint.is_empty());
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
            command: vec![],
            entrypoint: vec![],
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
            &[],
        );
        assert_eq!(doc["services"]["web"]["ports"][RESET_MARKER], json!([]));
        assert!(doc["services"]["web"]["container_name"][RESET_MARKER].is_null());
        assert!(doc["services"].get("db").is_none());
    }

    #[test]
    fn exposed_ports_are_published_on_loopback() {
        let iso = vec![Isolation {
            service: "web".into(),
            dropped_ports: vec!["8080".into()],
            dropped_container_name: None,
        }];
        let expose = vec![
            Expose {
                service: "web".into(),
                port: 80,
                host_port: None,
            },
            Expose {
                service: "api".into(),
                port: 3000,
                host_port: Some(13000),
            },
            Expose {
                service: "api".into(),
                port: 9229,
                host_port: None,
            },
        ];
        let spec = ClockdSpec {
            image: "i",
            token: "t",
            proxies: &[],
            networks: &[],
            env_rewrites: &[],
        };
        let doc = generate_override(&[], &spec, &iso, &expose);
        // web's own host ports are replaced by the exposed one…
        assert_eq!(
            doc["services"]["web"]["ports"][OVERRIDE_MARKER],
            json!(["127.0.0.1::80"])
        );
        // …api had none dropped, so its exposed ports are added.
        assert_eq!(
            doc["services"]["api"]["ports"],
            json!(["127.0.0.1:13000:3000", "127.0.0.1::9229"])
        );
        // Without `expose`, isolation still resets the ports.
        let doc = generate_override(&[], &spec, &iso, &[]);
        assert_eq!(doc["services"]["web"]["ports"][RESET_MARKER], json!([]));
        assert!(doc["services"].get("api").is_none());
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
