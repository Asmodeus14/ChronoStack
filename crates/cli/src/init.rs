//! `chronostack init`: write a starter `chronostack.yml` from the Compose project.
//!
//! Detection is deliberately conservative. Only what can be read reliably from
//! the Compose model becomes active configuration (a Sidekiq barrier on the
//! Redis service). Everything else is a commented suggestion or a note, so the
//! generated file never asserts something that isn't true of the stack.

use anyhow::{Context, Result};
use chronostack_core::compose::{self, Project, Service, StoreKind};

use crate::config;

const SCHEMA_URL: &str = "https://raw.githubusercontent.com/Asmodeus14/ChronoStack/main/docs/schema/chronostack.schema.json";
const GUIDE_URL: &str = "https://github.com/Asmodeus14/ChronoStack/blob/main/docs/GUIDE.md";

/// The generated file plus what to tell the user about it.
#[derive(Debug)]
pub struct Generated {
    pub yaml: String,
    /// One line per detected service.
    pub detected: Vec<String>,
    /// Things the user should change outside `chronostack.yml`.
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Postgres { pg_cron: bool },
    Redis,
    Sidekiq,
    CeleryBeat,
    CeleryWorker,
    NodeWorker,
    App,
}

struct Svc<'a> {
    svc: &'a Service,
    role: Role,
    /// `celery beat` / `worker -B` also runs a worker (embedded beat).
    embedded_beat: bool,
}

fn words(s: &Service) -> Vec<&str> {
    s.entrypoint
        .iter()
        .chain(&s.command)
        .flat_map(|w| w.split_whitespace())
        .collect()
}

fn has_word(ws: &[&str], needle: &str) -> bool {
    ws.iter().any(|w| {
        w.to_ascii_lowercase()
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .any(|part| part == needle)
    })
}

fn classify<'a>(project: &Project, svc: &'a Service) -> Svc<'a> {
    let ws = words(svc);
    let image = svc.image.clone().unwrap_or_default().to_ascii_lowercase();
    let line = ws.join(" ").to_ascii_lowercase();
    let mut embedded_beat = false;
    // A Postgres *server*: matched by image, or a command that starts `postgres`.
    // App containers that only set POSTGRES_* to connect are not servers.
    let pg_image = ["postgres", "pgvector", "postgis", "timescale"]
        .iter()
        .any(|n| image.contains(n));
    let role = if pg_image || ws.first() == Some(&"postgres") {
        Role::Postgres {
            pg_cron: line.contains("pg_cron")
                || image.contains("pg_cron")
                || image.contains("pgcron"),
        }
    } else if compose::store_kind(svc, &compose::image_name(&project.name, svc)) == StoreKind::Redis
        && !has_word(&ws, "sidekiq")
    {
        Role::Redis
    } else if has_word(&ws, "sidekiq") {
        Role::Sidekiq
    } else if has_word(&ws, "celery") {
        let beat_flag = ws.iter().any(|w| *w == "-B" || *w == "--beat");
        if has_word(&ws, "beat") && !has_word(&ws, "worker") {
            Role::CeleryBeat
        } else {
            embedded_beat = beat_flag;
            Role::CeleryWorker
        }
    } else if (image.contains("node")
        || ["node", "npm", "npx", "yarn", "pnpm", "bun", "tsx"]
            .iter()
            .any(|w| has_word(&ws, w)))
        && ["worker", "queue", "queues", "jobs", "bull", "bullmq"]
            .iter()
            .any(|w| has_word(&ws, w) || svc.name.to_ascii_lowercase().contains(w))
    {
        Role::NodeWorker
    } else {
        Role::App
    };
    Svc {
        svc,
        role,
        embedded_beat,
    }
}

/// `--max-interval` value in seconds, if given on the command line.
fn max_interval(ws: &[&str]) -> Option<f64> {
    ws.iter().enumerate().find_map(|(i, w)| {
        if let Some(v) = w.strip_prefix("--max-interval=") {
            v.parse().ok()
        } else if *w == "--max-interval" {
            ws.get(i + 1)?.parse().ok()
        } else {
            None
        }
    })
}

/// Build the file. `sources` are the Compose files it was generated from.
pub fn generate(project: &Project, sources: &[String]) -> Generated {
    let svcs: Vec<Svc> = project
        .services
        .iter()
        .map(|s| classify(project, s))
        .collect();
    let redis = svcs
        .iter()
        .find(|s| s.role == Role::Redis)
        .map(|s| s.svc.name.as_str());
    let redis_or_placeholder = redis.unwrap_or("<redis-service>");
    let mut detected = Vec::new();
    let mut warnings = Vec::new();
    let mut active_barriers = Vec::new();
    let mut suggested_barriers = Vec::new();

    for s in &svcs {
        let name = &s.svc.name;
        let ws = words(s.svc);
        match s.role {
            Role::Postgres { pg_cron } => detected.push(if pg_cron {
                format!("{name}: Postgres with pg_cron (handled automatically: the clock stops on each job's minute)")
            } else {
                format!("{name}: Postgres (now() follows the virtual clock; nothing to add)")
            }),
            Role::Redis => detected.push(format!("{name}: Redis (TTLs follow the virtual clock)")),
            Role::Sidekiq => {
                detected.push(format!("{name}: Sidekiq worker"));
                if redis.is_some() {
                    active_barriers.push(format!(
                        "  # Wait until Sidekiq has no queued, running or due scheduled jobs.\n  \
                         # If jobs are enqueued by sidekiq-cron or Clockwork, add `settle: 35s`\n  \
                         # so their next poll runs before the check.\n  \
                         - name: {name}-jobs-done\n    sidekiq: {{ service: {redis_or_placeholder} }}\n"
                    ));
                } else {
                    suggested_barriers.push(format!(
                        "  # Sidekiq ({name}): point this at the Redis service it uses.\n  \
                         # - name: {name}-jobs-done\n  \
                         #   sidekiq: {{ service: <redis-service> }}\n"
                    ));
                }
            }
            Role::CeleryBeat | Role::CeleryWorker => {
                let beat = s.role == Role::CeleryBeat || s.embedded_beat;
                detected.push(format!(
                    "{name}: Celery {}",
                    match (s.role, beat) {
                        (Role::CeleryBeat, _) => "beat",
                        (_, true) => "worker with embedded beat (-B)",
                        _ => "worker",
                    }
                ));
                if beat {
                    let line = ws.join(" ").to_ascii_lowercase();
                    if line.contains("databasescheduler") || line.contains("django_celery_beat") {
                        warnings.push(format!(
                            "{name}: django-celery-beat's DatabaseScheduler may not send crontab tasks after a clock jump; \
                             declare triggers for them (see issue #9)"
                        ));
                    }
                    match max_interval(&ws) {
                        Some(v) if v <= 1.0 => {}
                        _ => warnings.push(format!(
                            "{name}: Celery beat only notices a jump at its next check (up to 5 min by default). \
                             Add `--max-interval 1` to its command in your compose file, \
                             unless beat_max_loop_interval <= 1 is already set in your Celery config"
                        )),
                    }
                }
                if s.role == Role::CeleryWorker {
                    suggested_barriers.push(format!(
                        "  # Celery ({name}) with a Redis broker: wait until nothing is queued.\n  \
                         # Running tasks aren't counted; see the guide, section 5, for more.\n  \
                         # - name: {name}-queue-empty\n  \
                         #   redis: {{ service: {redis_or_placeholder}, command: [LLEN, celery], equals: \"0\" }}\n"
                    ));
                }
            }
            Role::NodeWorker => {
                detected.push(format!("{name}: Node worker (possibly BullMQ)"));
                suggested_barriers.push(format!(
                    "  # If {name} runs BullMQ (or Bull): list its queue names to wait until\n  \
                     # they have no waiting, active or due delayed jobs.\n  \
                     # - name: {name}-jobs-done\n  \
                     #   bullmq: {{ service: {redis_or_placeholder}, queues: [<queue-name>] }}\n"
                ));
            }
            Role::App => detected.push(format!("{name}: app service")),
        }
    }

    // The service most likely to run scheduled jobs, for the trigger example.
    let trigger_service = svcs
        .iter()
        .rev() // max_by_key keeps the last maximum: prefer the first in file order
        .filter(|s| matches!(s.role, Role::App | Role::NodeWorker))
        .max_by_key(|s| {
            let n = s.svc.name.to_ascii_lowercase();
            ["worker", "cron", "scheduler", "jobs"]
                .iter()
                .any(|w| n.contains(w))
        })
        .map(|s| s.svc.name.as_str());

    let mut y = String::new();
    y.push_str(&format!("# yaml-language-server: $schema={SCHEMA_URL}\n"));
    y.push_str(&format!(
        "# Generated by `chronostack init` from {}. Review it, then run:\n",
        sources.join(", ")
    ));
    y.push_str("#   chronostack up --wait && chronostack doctor\n");
    y.push_str(&format!("# Guide: {GUIDE_URL}\n"));
    y.push_str("version: 1\n\n");

    y.push_str("# Detected services:\n");
    for d in &detected {
        y.push_str(&format!("#   {d}\n"));
    }
    y.push('\n');

    if let Some(svc) = trigger_service {
        y.push_str(
            "# In-process schedulers (node-cron, APScheduler, setTimeout loops, Spring\n\
             # @Scheduled) don't fire when the clock jumps. Declare a trigger to run the\n\
             # job's code at its virtual time instead:\n",
        );
        y.push_str("# triggers:\n");
        y.push_str("#   - name: nightly-job\n");
        y.push_str("#     schedule: { every: 1d, at: \"03:00\" }   # UTC\n");
        y.push_str(&format!(
            "#     exec: {{ service: {svc}, command: [<your>, <job>, <command>] }}\n\n"
        ));
    }

    if !active_barriers.is_empty() {
        y.push_str("# Barriers: before the clock moves on, wait until the work is done.\n");
        y.push_str("barriers:\n");
        for b in &active_barriers {
            y.push_str(b);
        }
        for b in &suggested_barriers {
            y.push_str(b);
        }
    } else if !suggested_barriers.is_empty() {
        y.push_str("# Barriers: before the clock moves on, wait until the work is done.\n");
        y.push_str("# barriers:\n");
        for b in &suggested_barriers {
            y.push_str(b);
        }
    }

    Generated {
        yaml: y,
        detected,
        warnings,
    }
}

/// Parse and validate generated text exactly as `chronostack.yml` is loaded.
pub fn check(yaml: &str, project: &Project) -> Result<()> {
    let names: Vec<String> = project.services.iter().map(|s| s.name.clone()).collect();
    let cfg: config::Config =
        serde_norway::from_str(yaml).context("generated chronostack.yml does not parse")?;
    config::validate(cfg, &names).context("generated chronostack.yml is not valid")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(json: &str) -> Project {
        compose::parse_config(json).unwrap()
    }

    fn generated(json: &str) -> (Project, Generated) {
        let p = project(json);
        let g = generate(&p, &["compose.yml".into()]);
        check(&g.yaml, &p).unwrap_or_else(|e| panic!("{e:#}\n---\n{}", g.yaml));
        (p, g)
    }

    #[test]
    fn sidekiq_gets_an_active_barrier() {
        let (_, g) = generated(
            r#"{"name":"x","services":{
                "redis":{"image":"redis:7"},
                "worker":{"image":"ruby:3.3","command":["bash","-c","bundle exec sidekiq -c 5"]}}}"#,
        );
        assert!(g.yaml.contains("barriers:\n"));
        assert!(g.yaml.contains("sidekiq: { service: redis }"));
        assert!(g.detected.iter().any(|d| d.contains("worker: Sidekiq")));
    }

    #[test]
    fn sidekiq_without_redis_is_only_suggested() {
        let (_, g) = generated(
            r#"{"name":"x","services":{"worker":{"image":"ruby:3.3","command":"sidekiq"}}}"#,
        );
        assert!(!g.yaml.contains("\nbarriers:"));
        assert!(g.yaml.contains("#   sidekiq: { service: <redis-service> }"));
    }

    #[test]
    fn celery_beat_without_max_interval_warns() {
        let (_, g) = generated(
            r#"{"name":"x","services":{
                "redis":{"image":"redis:7"},
                "beat":{"image":"app","command":["celery","-A","app","beat"]},
                "worker":{"image":"app","command":["celery","-A","app","worker"]}}}"#,
        );
        assert_eq!(g.warnings.len(), 1);
        assert!(g.warnings[0].starts_with("beat:") && g.warnings[0].contains("--max-interval 1"));
        assert!(
            g.yaml
                .contains("#   redis: { service: redis, command: [LLEN, celery]")
        );
    }

    #[test]
    fn celery_beat_with_max_interval_is_quiet() {
        let (_, g) = generated(
            r#"{"name":"x","services":{
                "beat":{"image":"app","command":["celery","-A","app","beat","--max-interval","1"]}}}"#,
        );
        assert!(g.warnings.is_empty());
        let (_, g) = generated(
            r#"{"name":"x","services":{
                "beat":{"image":"app","command":"celery -A app beat --max-interval=0.5"}}}"#,
        );
        assert!(g.warnings.is_empty());
    }

    #[test]
    fn embedded_beat_and_database_scheduler() {
        let (_, g) = generated(
            r#"{"name":"x","services":{
                "celery":{"image":"app","command":["celery","-A","t","worker","-B"]},
                "beat":{"image":"app","command":["celery","-A","t","beat","--scheduler","django_celery_beat.schedulers:DatabaseScheduler","--max-interval","1"]}}}"#,
        );
        assert!(g.detected.iter().any(|d| d.contains("embedded beat")));
        assert!(
            g.warnings
                .iter()
                .any(|w| w.starts_with("celery:") && w.contains("--max-interval"))
        );
        assert!(
            g.warnings
                .iter()
                .any(|w| w.starts_with("beat:") && w.contains("#9"))
        );
    }

    #[test]
    fn node_worker_gets_a_commented_bullmq_barrier() {
        let (_, g) = generated(
            r#"{"name":"x","services":{
                "cache":{"image":"valkey/valkey:8"},
                "api":{"image":"node:22","command":["node","api.js"]},
                "worker":{"image":"node:22","command":["node","worker.js"]}}}"#,
        );
        assert!(g.yaml.contains("# barriers:"));
        assert!(
            g.yaml
                .contains("#   bullmq: { service: cache, queues: [<queue-name>] }")
        );
        assert!(g.yaml.contains("exec: { service: worker,"));
        assert!(g.detected.iter().any(|d| d == "api: app service"));
    }

    #[test]
    fn pg_cron_and_app_with_postgres_env() {
        let (_, g) = generated(
            r#"{"name":"x","services":{
                "db":{"build":".","command":["postgres","-c","shared_preload_libraries=pg_cron"]},
                "api":{"image":"myapp","environment":{"POSTGRES_PASSWORD":"pw"}}}}"#,
        );
        assert!(
            g.detected
                .iter()
                .any(|d| d.starts_with("db: Postgres with pg_cron"))
        );
        assert!(g.detected.iter().any(|d| d == "api: app service"));
    }

    #[test]
    fn nothing_to_detect_still_valid() {
        let (_, g) = generated(r#"{"name":"x","services":{"db":{"image":"postgres:16"}}}"#);
        assert!(g.warnings.is_empty());
        assert!(!g.yaml.contains("triggers"));
        assert!(g.yaml.contains("version: 1"));
    }
}
