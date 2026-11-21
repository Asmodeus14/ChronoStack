//! `chronostack` — Stripe Test Clocks for your Docker Compose stack.

mod client;
mod config;
mod detect;
mod docker;
mod engine;
mod expose;
mod fault;
mod init;
mod mcp;
mod probe;
mod queues;
mod report;
mod scenario;
mod session;
mod update;
mod users;
mod vusers;
mod yaml;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chronostack_core::api::ClockState;
use chronostack_core::compose::{self, ClockdSpec, ServicePlan};
use chronostack_core::duration::{format_span, format_utc, parse_duration, parse_utc};
use chronostack_core::paths;
use clap::{Parser, Subcommand};
use serde_json::json;

use crate::client::Client;

/// Controller image used by `up`. The release workflow sets it (at compile
/// time) to the published image pinned by digest. Without that, debug builds
/// (development, e2e tests) use a locally built image, and release builds
/// (`cargo install`) use the published image for this version.
const DEFAULT_CLOCKD_IMAGE: &str = match option_env!("CHRONOSTACK_DEFAULT_CLOCKD_IMAGE") {
    Some(v) => v,
    None if cfg!(debug_assertions) => LOCAL_CLOCKD_IMAGE,
    None => PUBLISHED_CLOCKD_IMAGE,
};
const LOCAL_CLOCKD_IMAGE: &str = "chronostack/clockd:dev";
const PUBLISHED_CLOCKD_IMAGE: &str = concat!(
    "ghcr.io/asmodeus14/chronostack-clockd:",
    env!("CARGO_PKG_VERSION")
);
use crate::docker::Compose;
use crate::session::{ProbeSpec, ServiceEntry, Session};

#[derive(Parser)]
#[command(
    name = "chronostack",
    version,
    about = "Stripe Test Clocks for your Docker Compose stack",
    long_about = "Move the virtual wall-clock time of your whole Docker Compose stack.\n\n\
                  ChronoStack changes what time it IS. It does not make time PASS inside your \
                  application's timers: declare triggers and barriers in chronostack.yml for \
                  scheduled work.\n\n\
                  Guide: https://github.com/Asmodeus14/ChronoStack/blob/main/docs/GUIDE.md"
)]
struct Cli {
    /// Compose file(s), as with `docker compose -f` (used by `up` and `init`).
    #[arg(short = 'f', long = "file", global = true)]
    files: Vec<String>,
    /// Compose project name for the ChronoStack stack (default: `<project>-chronostack`).
    #[arg(short = 'p', long = "project-name", global = true)]
    project_name: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Start your stack with every compatible service on a shared virtual clock.
    Up {
        /// Controller image.
        #[arg(
            long,
            env = "CHRONOSTACK_CLOCKD_IMAGE",
            default_value = DEFAULT_CLOCKD_IMAGE
        )]
        clockd_image: String,
        /// Keep published host ports and container names in the ChronoStack copy.
        /// By default they are removed so the copy can run next to your own stack.
        #[arg(long)]
        keep_ports: bool,
        /// Wait until every virtualized service answers its time probe.
        #[arg(long)]
        wait: bool,
        /// How long `--wait` waits (e.g. 120s, 5m).
        #[arg(long, default_value = "120s", requires = "wait")]
        wait_timeout: String,
        /// Start even if a writable bind mount, external volume or fixed-name volume
        /// is shared with your own stack (jobs run during `advance` can change it).
        #[arg(long)]
        allow_shared_mounts: bool,
    },
    /// Stop the ChronoStack stack (your own stack and files are untouched).
    Down {
        /// Also remove the ChronoStack project's volumes (including the clock).
        #[arg(short = 'v', long)]
        volumes: bool,
    },
    /// Show virtual and real time.
    #[command(alias = "status")]
    Now {
        #[arg(long)]
        json: bool,
    },
    /// Move virtual time forward, running scheduled triggers and waiting on barriers.
    ///
    /// Examples: `advance 25h`, `advance 14d --step 1d`, `advance --to 2026-12-01T03:00:00Z`.
    Advance {
        /// How far to move, e.g. `25h`, `14d`, `1h30m`.
        #[arg(required_unless_present = "to", conflicts_with = "to")]
        duration: Option<String>,
        /// Move to an absolute UTC time instead (RFC 3339, e.g. 2026-12-01T03:00:00Z).
        #[arg(long)]
        to: Option<String>,
        /// Stop every STEP (e.g. `1d`): runs step triggers and barriers each time.
        #[arg(long)]
        step: Option<String>,
        /// Largest single jump: a duration, `none`, or `auto` (1h when pg_cron is detected).
        #[arg(long, default_value = "auto")]
        max_jump: String,
        /// Wait after each step/sub-step when no barrier is configured (e.g. `500ms`).
        #[arg(long)]
        dwell: Option<String>,
        /// Print the plan without moving the clock.
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        json: bool,
        /// Skip verifying that services see the new time.
        #[arg(long)]
        no_probe: bool,
        /// Stop at every occurrence of wildcard pg_cron jobs such as `*/5 * * * *`
        /// (by default pg_cron runs them at most once per clock stop).
        #[arg(long)]
        pg_cron_all: bool,
    },
    /// Validate chronostack.yml and show its triggers and barriers.
    Config,
    /// Stop virtual time.
    Freeze {
        #[arg(long)]
        json: bool,
    },
    /// Let virtual time flow again from where it was frozen.
    Unfreeze {
        #[arg(long)]
        json: bool,
    },
    /// Check that every virtualized service sees the virtual time.
    Probe {
        #[arg(long)]
        json: bool,
    },
    /// Write a starter chronostack.yml for this Compose project.
    ///
    /// Detects Postgres (and pg_cron), Redis, Sidekiq, Celery and Node workers
    /// from the Compose file and suggests triggers and barriers. Starts nothing.
    Init {
        /// Print the file instead of writing it.
        #[arg(long)]
        print: bool,
        /// Overwrite an existing chronostack.yml.
        #[arg(long)]
        force: bool,
    },
    /// Explain which services are virtualized, and why not.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Outage windows on proxy targets, in virtual time.
    Fault {
        #[command(subcommand)]
        cmd: FaultCmd,
    },
    /// Show the controller's event log (clock changes, faults, proxied requests).
    Events {
        /// Only events after this sequence number.
        #[arg(long, default_value_t = 0)]
        since: u64,
        #[arg(long)]
        json: bool,
    },
    /// Serve the Model Context Protocol on stdio, for AI agents (Claude Code, Cursor, …).
    Mcp {
        /// Project directory (where `chronostack up` was run). Default: current directory.
        #[arg(long)]
        project_dir: Option<std::path::PathBuf>,
    },
    /// Update this CLI to the newest release (checksum-verified; no admin rights needed).
    Update {
        /// Only say whether a newer release exists.
        #[arg(long)]
        check: bool,
        /// Install this version instead of the newest (also allows going back).
        #[arg(long)]
        version: Option<String>,
        /// Include pre-releases (automatic when this build is a pre-release).
        #[arg(long)]
        pre: bool,
        /// Reinstall even if already up to date.
        #[arg(long)]
        force: bool,
    },
    /// Simulated users: seeded journeys (HTTP requests, think times, branches).
    Users {
        #[command(subcommand)]
        cmd: UsersCmd,
    },
    /// Run a scenario file and write a report to .chronostack/reports/.
    Run {
        /// Scenario file (YAML).
        file: String,
        /// Print the report JSON instead of progress.
        #[arg(long)]
        json: bool,
        /// Also write a JUnit XML report to this path (for CI test reporters).
        #[arg(long, value_name = "PATH")]
        junit: Option<PathBuf>,
        /// Refuse any advance step that would take virtual time more than this
        /// many nanoseconds past the scenario's start (set by `chronostack mcp`).
        #[arg(long, hide = true)]
        max_advance_ns: Option<i64>,
        /// Check the scenario (and its users file) and show what it would do,
        /// without running it: steps, how far virtual time moves, the users.
        #[arg(long)]
        check: bool,
    },
}

#[derive(Subcommand)]
enum UsersCmd {
    /// Check a journey file without running it.
    Validate {
        /// Journey file (YAML).
        file: String,
        #[arg(long)]
        json: bool,
    },
    /// Run a journey file against your stack, in real time.
    ///
    /// `service://` targets go to ports listed under `expose:` in
    /// chronostack.yml; plain http:// targets work without a session.
    Run {
        /// Journey file (YAML).
        file: String,
        /// Fix every random choice (overrides `seed:` in the file).
        #[arg(long)]
        seed: Option<u64>,
        /// Stop after this much real time (e.g. 60s, 10m).
        #[arg(long)]
        duration: Option<String>,
        /// Print the report JSON instead of the summary.
        #[arg(long)]
        json: bool,
        /// Also write a JUnit XML report to this path.
        #[arg(long, value_name = "PATH")]
        junit: Option<PathBuf>,
        /// Include every user's steps in the JSON report.
        #[arg(long)]
        trace: bool,
    },
}

#[derive(Subcommand)]
enum FaultCmd {
    /// Make a proxy target return an error status during a virtual-time window.
    Set {
        /// Proxy target name (from `proxies:` in chronostack.yml).
        target: String,
        /// Start: `now`, `+<duration>`, `next HH:MM` (UTC) or a UTC time.
        #[arg(long, default_value = "now", allow_hyphen_values = true)]
        from: String,
        /// Length of the window (e.g. 2h).
        #[arg(long = "for", conflicts_with = "until")]
        duration: Option<String>,
        /// End of the window (same forms as --from).
        #[arg(long, allow_hyphen_values = true)]
        until: Option<String>,
        /// Status returned during the window.
        #[arg(long)]
        status: Option<u16>,
        #[arg(long)]
        json: bool,
    },
    /// List proxy targets and outage windows.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Remove one window (by id) or all of them.
    Clear {
        /// Window id (see `fault list`).
        id: Option<u64>,
        #[arg(long, conflicts_with = "id")]
        all: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e:#}");
            std::process::exit(1);
        }
    }
}

fn run(cli: Cli) -> Result<i32> {
    match cli.cmd {
        Cmd::Up {
            ref clockd_image,
            keep_ports,
            wait,
            ref wait_timeout,
            allow_shared_mounts,
        } => {
            let wait = if wait {
                Some(parse_duration(wait_timeout).map_err(anyhow::Error::msg)?)
            } else {
                None
            };
            up(&cli, clockd_image, keep_ports, wait, allow_shared_mounts)
        }
        Cmd::Down { volumes } => down(volumes).map(|_| 0),
        Cmd::Now { json } => {
            let mut s = Session::load()?;
            print_clock(&client(&mut s).clock()?, json);
            Ok(0)
        }
        Cmd::Advance {
            duration,
            to,
            step,
            max_jump,
            dwell,
            dry_run,
            json,
            no_probe,
            pg_cron_all,
        } => {
            let msg = anyhow::Error::msg;
            let target = match (duration, to) {
                (Some(d), None) => engine::Target::By(parse_duration(&d).map_err(msg)?),
                (None, Some(t)) => engine::Target::To(parse_utc(&t).map_err(msg)?),
                _ => bail!("give either a duration or --to"),
            };
            let max_jump = match max_jump.as_str() {
                "auto" => engine::MaxJump::Auto,
                "none" => engine::MaxJump::Disabled,
                v => engine::MaxJump::Fixed(parse_duration(v).map_err(msg)?),
            };
            let opts = engine::Options {
                target,
                step: step
                    .as_deref()
                    .map(parse_duration)
                    .transpose()
                    .map_err(msg)?,
                max_jump,
                dwell: dwell
                    .as_deref()
                    .map(parse_duration)
                    .transpose()
                    .map_err(msg)?,
                dry_run,
                json,
                no_probe,
                pg_cron_all,
            };
            let mut s = Session::load()?;
            let cfg = load_config(&s)?;
            let c = client(&mut s);
            engine::advance(&s, &c, &cfg, &opts)
        }
        Cmd::Config => {
            let services: Vec<String> = Session::load()
                .map(|s| s.services.iter().map(|e| e.plan.service.clone()).collect())
                .unwrap_or_default();
            let Some(path) = config::find(Path::new(".")) else {
                println!("no chronostack.yml here — advances run without triggers or barriers");
                return Ok(0);
            };
            let r = config::load(&path, &services)?;
            println!("{} is valid", path.display());
            for (i, t) in r.config.triggers.iter().enumerate() {
                let when = match r.schedules.iter().find(|(j, _)| *j == i) {
                    Some((_, e)) => format!(
                        "every {} at +{} (UTC)",
                        format_span(e.every_ns).trim_start_matches('+'),
                        format_span(e.at_ns).trim_start_matches('+')
                    ),
                    None => "after every --step".into(),
                };
                println!("  trigger {}: {when}", t.name);
            }
            for b in &r.config.barriers {
                println!("  barrier {}", b.name);
            }
            for e in &r.expose {
                match e.host_port {
                    Some(h) => println!("  expose {}:{} on 127.0.0.1:{h}", e.service, e.port),
                    None => println!("  expose {}:{} on a free port", e.service, e.port),
                }
            }
            Ok(0)
        }
        Cmd::Freeze { json } => {
            let mut s = Session::load()?;
            print_clock(&client(&mut s).freeze()?, json);
            Ok(0)
        }
        Cmd::Unfreeze { json } => {
            let mut s = Session::load()?;
            print_clock(&client(&mut s).unfreeze()?, json);
            Ok(0)
        }
        Cmd::Probe { json } => {
            let mut s = Session::load()?;
            let c = client(&mut s);
            let probes = probe::probe_all(&s, &c);
            if json {
                println!("{}", serde_json::to_string(&probes)?);
            } else {
                probe::print_probes(&probes);
            }
            Ok(if probes.iter().any(|p| !p.passes()) {
                3
            } else {
                0
            })
        }
        Cmd::Init { print, force } => init_cmd(&cli, print, force),
        Cmd::Doctor { json } => doctor(json),
        Cmd::Fault { cmd } => {
            let mut s = Session::load()?;
            let c = client(&mut s);
            match cmd {
                FaultCmd::Set {
                    target,
                    from,
                    duration,
                    until,
                    status,
                    json,
                } => {
                    let f = fault::set(
                        &c,
                        &target,
                        &from,
                        duration.as_deref(),
                        until.as_deref(),
                        status,
                    )?;
                    if json {
                        println!("{}", serde_json::to_string(&f)?);
                    } else {
                        println!(
                            "outage #{}: `{}` returns HTTP {} from {} until {} (virtual time)",
                            f.id,
                            f.target,
                            f.status,
                            format_utc(f.from_ns),
                            format_utc(f.to_ns)
                        );
                    }
                }
                FaultCmd::List { json } => {
                    if json {
                        println!("{}", serde_json::to_string(&c.faults()?)?);
                    } else {
                        fault::print_list(&c)?;
                    }
                }
                FaultCmd::Clear { id, all } => {
                    if id.is_none() && !all {
                        bail!("give a window id (see `chronostack fault list`) or --all");
                    }
                    let removed = c.fault_clear(id)?;
                    println!("removed {} outage window(s)", removed.len());
                }
            }
            Ok(0)
        }
        Cmd::Events { since, json } => {
            let mut s = Session::load()?;
            let e = client(&mut s).events(since)?;
            if json {
                println!("{}", serde_json::to_string(&e)?);
            } else {
                if e.truncated {
                    println!("(older events were dropped; the controller keeps the last 1000)");
                }
                for ev in &e.events {
                    println!("{}", fault::describe(ev));
                }
                if e.events.is_empty() {
                    println!("no events after #{since}");
                }
            }
            Ok(0)
        }
        Cmd::Mcp { project_dir } => {
            if let Some(d) = project_dir {
                std::env::set_current_dir(&d)
                    .with_context(|| format!("cannot enter {}", d.display()))?;
            }
            // The cap comes from chronostack.yml when present; the session is
            // optional here (tools report "run chronostack up" if it's missing).
            let services: Vec<String> = Session::load()
                .map(|s| s.services.iter().map(|e| e.plan.service.clone()).collect())
                .unwrap_or_default();
            let limits = match config::find(Path::new(".")) {
                Some(p) => {
                    let c = config::load(&p, &services)?;
                    mcp::Limits {
                        max_advance_ns: c.mcp_max_advance_ns,
                        max_wall_ns: c.mcp_max_wall_ns,
                        max_users: c.mcp_max_users,
                    }
                }
                None => mcp::Limits::default(),
            };
            mcp::serve(limits)
        }
        Cmd::Update {
            check,
            version,
            pre,
            force,
        } => update::run(&update::Options {
            check,
            version,
            pre,
            force,
        }),
        Cmd::Users { cmd } => match cmd {
            UsersCmd::Validate { file, json } => users::validate(Path::new(&file), json),
            UsersCmd::Run {
                file,
                seed,
                duration,
                json,
                junit,
                trace,
            } => {
                let duration_ns = duration
                    .as_deref()
                    .map(|d| parse_duration(d).map_err(anyhow::Error::msg))
                    .transpose()
                    .context("--duration")?;
                users::run(
                    Path::new(&file),
                    &users::RunOptions {
                        seed,
                        duration_ns,
                        json,
                        junit,
                        trace,
                    },
                )
            }
        },
        Cmd::Run {
            file,
            json,
            junit,
            max_advance_ns,
            check: true,
        } => {
            let _ = (junit, max_advance_ns);
            let session = Session::load().ok();
            let cfg = match &session {
                Some(s) => load_config(s)?,
                None => match config::find(Path::new(".")) {
                    Some(p) => config::load(&p, &[])?,
                    None => config::Resolved::empty(),
                },
            };
            let services: Vec<String> = session
                .as_ref()
                .map(|s| s.services.iter().map(|e| e.plan.service.clone()).collect())
                .unwrap_or_default();
            let path = Path::new(&file);
            let sc = scenario::load(path, &cfg, &services)?;
            let now = match session {
                Some(mut s) => client(&mut s).clock().ok().map(|c| c.virtual_ns),
                None => None,
            };
            let plan = scenario::check(path, &sc, now)?;
            if json {
                println!("{plan}");
            } else {
                print!("{}", scenario::check_text(&plan));
            }
            Ok(0)
        }
        Cmd::Run {
            file,
            json,
            junit,
            max_advance_ns,
            check: false,
        } => {
            let mut s = Session::load()?;
            let cfg = load_config(&s)?;
            let services: Vec<String> = s.services.iter().map(|e| e.plan.service.clone()).collect();
            let path = Path::new(&file);
            let sc = scenario::load(path, &cfg, &services)?;
            let c = client(&mut s);
            let opts = scenario::RunOptions {
                json,
                max_advance_ns,
                junit,
            };
            scenario::run(&s, &c, &cfg, path, &sc, &opts)
        }
    }
}

/// Load `chronostack.yml` from the current directory, if present.
fn load_config(s: &Session) -> Result<config::Resolved> {
    let services: Vec<String> = s.services.iter().map(|e| e.plan.service.clone()).collect();
    match config::find(Path::new(".")) {
        Some(p) => config::load(&p, &services),
        None => Ok(config::Resolved::empty()),
    }
}

/// Connect to clockd. Docker may assign a new random host port when the
/// controller container restarts (found by the M1 e2e test), so if the stored
/// port does not answer, re-resolve it once and persist the new one.
fn client(s: &mut Session) -> Client {
    let c = Client::new(s.base_url(), s.token.clone());
    if c.clock().is_ok() {
        return c;
    }
    if let Ok(port) = resolve_port(&s.compose()) {
        if port != s.port {
            s.port = port;
            let _ = s.save();
        }
    }
    Client::new(s.base_url(), s.token.clone())
}

/// Host port Compose published for the controller.
fn resolve_port(compose: &Compose) -> Result<u16> {
    published_port(compose, paths::CLOCKD_SERVICE, paths::CLOCKD_PORT)
}

/// Host port Compose published for a service's container port.
fn published_port(compose: &Compose, service: &str, port: u16) -> Result<u16> {
    let out = docker::run(&compose.args(&["port", service, &port.to_string()]))?;
    // One line per binding (`127.0.0.1:49213`); the first is enough.
    out.lines()
        .next()
        .and_then(|l| l.trim().rsplit(':').next())
        .and_then(|p| p.parse().ok())
        .with_context(|| {
            format!("unexpected `docker compose port {service} {port}` output: {out:?}")
        })
}

fn print_clock(st: &ClockState, json: bool) {
    if json {
        println!("{}", serde_json::to_string(st).expect("serializable"));
        return;
    }
    println!(
        "virtual  {}  ({} vs real)",
        format_utc(st.virtual_ns),
        format_span(st.offset_ns)
    );
    println!("real     {}", format_utc(st.real_ns));
    println!(
        "frozen   {}    generation {}",
        if st.frozen { "yes" } else { "no" },
        st.generation
    );
}

/// Compose's default file names, mirroring its own lookup (including the
/// automatically merged `*.override.*` file).
fn resolve_files(given: &[String]) -> Result<Vec<String>> {
    if !given.is_empty() {
        return Ok(given.to_vec());
    }
    let base = [
        "compose.yaml",
        "compose.yml",
        "docker-compose.yaml",
        "docker-compose.yml",
    ]
    .into_iter()
    .find(|f| Path::new(f).exists())
    .context("no compose file found here (compose.yaml / docker-compose.yml); pass one with -f")?;
    let mut files = vec![base.to_string()];
    let stem = base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base);
    for ext in ["yaml", "yml"] {
        let o = format!("{stem}.override.{ext}");
        if Path::new(&o).exists() {
            files.push(o);
            break;
        }
    }
    Ok(files)
}

/// Resolve the Compose files and load the project as Compose itself sees it.
fn load_project(given: &[String]) -> Result<(Vec<String>, compose::Project)> {
    let files = resolve_files(given)?;
    let mut cfg_args = vec!["compose".to_string()];
    for f in &files {
        cfg_args.push("-f".into());
        cfg_args.push(f.clone());
    }
    cfg_args.extend(docker::s(&["config", "--format", "json"]));
    let project = compose::parse_config(&docker::run(&cfg_args)?).map_err(anyhow::Error::msg)?;
    Ok((files, project))
}

fn init_cmd(cli: &Cli, print: bool, force: bool) -> Result<i32> {
    if !print && !force {
        if let Some(existing) = config::find(Path::new(".")) {
            bail!(
                "{} already exists; use --force to overwrite it, or --print to see the suggestion",
                existing.file_name().unwrap_or_default().to_string_lossy()
            );
        }
    }
    let (files, project) = load_project(&cli.files)?;
    let g = init::generate(&project, &files);
    init::check(&g.yaml, &project)?;
    if print {
        print!("{}", g.yaml);
    } else {
        fs::write(config::FILE_NAMES[0], &g.yaml)
            .with_context(|| format!("writing {}", config::FILE_NAMES[0]))?;
    }
    // With --print the YAML is on stdout, so the summary goes to stderr.
    let mut out = String::new();
    if !print {
        out.push_str(&format!(
            "wrote {}
",
            config::FILE_NAMES[0]
        ));
    }
    out.push_str(
        "detected:
",
    );
    for d in &g.detected {
        out.push_str(&format!(
            "  {d}
"
        ));
    }
    if !g.warnings.is_empty() {
        out.push_str(
            "to change in your compose setup:
",
        );
        for w in &g.warnings {
            out.push_str(&format!(
                "  ! {w}
"
            ));
        }
    }
    out.push_str("next: fill in the commented suggestions you need, then run\n");
    out.push_str("  chronostack up --wait && chronostack doctor && chronostack advance 1d\n");
    if print {
        eprint!("{out}");
    } else {
        print!("{out}");
    }
    Ok(0)
}

fn probe_spec(svc: &compose::Service, image: &str) -> ProbeSpec {
    let env = |k: &str| svc.environment.get(k).cloned().flatten();
    // App containers often set POSTGRES_* too (to connect); probe_one falls
    // back to `date` when psql has no server to talk to.
    match compose::store_kind(svc, image) {
        compose::StoreKind::Postgres => {
            let user = env("POSTGRES_USER").unwrap_or_else(|| "postgres".into());
            let db = env("POSTGRES_DB").unwrap_or_else(|| user.clone());
            ProbeSpec::Postgres { user, db }
        }
        compose::StoreKind::Redis => ProbeSpec::Redis,
        compose::StoreKind::Other => ProbeSpec::Date,
    }
}

const OVERRIDE_FILE: &str = "compose.override.yml";

/// Compose's `!reset` tag needs Docker Compose >= 2.24.
fn compose_supports_reset() -> bool {
    let Ok(v) = docker::run(&docker::s(&["compose", "version", "--short"])) else {
        return false;
    };
    let nums: Vec<u64> = v
        .trim()
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .take(2)
        .filter_map(|x| x.parse().ok())
        .collect();
    matches!(nums.as_slice(), [major, minor] if (*major, *minor) >= (2, 24))
}

/// How a shared mount's source is shown: bind paths under the current
/// directory as `./…`, everything else as Compose resolved it.
fn mount_source(m: &compose::SharedMount) -> String {
    if m.kind == compose::SharedKind::Bind {
        if let Ok(cwd) = std::env::current_dir() {
            if let Ok(rest) = Path::new(&m.source).strip_prefix(&cwd) {
                return format!("./{}", rest.display());
            }
        }
    }
    m.source.clone()
}

fn describe_mount(m: &compose::SharedMount) -> String {
    format!(
        "{}: {} is shared with your own stack ({}{}, mounted at {})",
        m.service,
        mount_source(m),
        m.kind.describe(),
        if m.read_only { ", read-only" } else { "" },
        m.target
    )
}

/// Refuse to start when the copy would share writable storage with the
/// user's own stack, unless `--allow-shared-mounts` is given. Read-only
/// mounts can't be changed, so they only get a note.
fn check_shared_mounts(mounts: &[compose::SharedMount], allow: bool) -> Result<()> {
    let (read_only, writable): (Vec<_>, Vec<_>) = mounts.iter().partition(|m| m.read_only);
    for m in &read_only {
        eprintln!("note: {}", describe_mount(m));
    }
    if writable.is_empty() {
        return Ok(());
    }
    let list: Vec<String> = writable
        .iter()
        .map(|m| format!("  {}", describe_mount(m)))
        .collect();
    let list = list.join("\n");
    if allow {
        eprintln!(
            "warning: starting with storage shared with your own stack (--allow-shared-mounts):\n\
             {list}\n\
             Jobs that run during `advance` can change it."
        );
        return Ok(());
    }
    bail!(
        "the ChronoStack copy would share storage with your own stack:\n\
         {list}\n\
         Jobs that run during `advance` can change these files and volumes, \
         so your real data could be changed or deleted.\n\
         Use a named volume (Compose gives the copy its own), mount it read-only (`:ro`), \
         or pass --allow-shared-mounts if sharing is intended."
    )
}

fn up(
    cli: &Cli,
    clockd_image: &str,
    keep_ports: bool,
    wait_ns: Option<i64>,
    allow_shared_mounts: bool,
) -> Result<i32> {
    if Session::path().exists() {
        bail!(
            "a ChronoStack session already exists in this directory; run `chronostack down` first"
        );
    }
    let (files, project) = load_project(&cli.files)?;
    check_shared_mounts(&project.shared_mounts, allow_shared_mounts)?;
    let cs_project = cli
        .project_name
        .clone()
        .unwrap_or_else(|| format!("{}-chronostack", project.name));

    session::ensure_dir()?;
    let engine_arch = detect::engine_arch()?;
    if !docker::output(&docker::s(&["image", "inspect", clockd_image]))?
        .status
        .success()
    {
        if clockd_image == LOCAL_CLOCKD_IMAGE {
            bail!(
                "controller image `{clockd_image}` not found locally.\n\
                 Build it from the repository root with:\n  \
                 docker build -f docker/clockd.Dockerfile -t {clockd_image} .\n\
                 (release builds of the CLI pull a published image instead)"
            );
        }
        eprintln!("pulling controller image {clockd_image} …");
        if docker::run_inherit(&docker::s(&["pull", clockd_image])).is_err() {
            bail!("could not pull the controller image `{clockd_image}`");
        }
    }

    let user_compose = Compose {
        files: files.clone(),
        project: cs_project.clone(),
    };
    if project.services.iter().any(|s| s.has_build) {
        eprintln!("building images …");
        docker::run_inherit(&user_compose.args(&["build"]))?;
    }

    let cache = Path::new(session::DIR).join("image-cache.json");
    let mut entries = Vec::new();
    for svc in &project.services {
        let image = compose::image_name(&cs_project, svc);
        let info = (|| {
            if !svc.has_build {
                detect::ensure_image(&image, svc.platform.as_deref())?;
            }
            detect::inspect(&image, &cache)
        })();
        let plan: ServicePlan = match &info {
            Ok(i) => compose::plan_service(&cs_project, svc, Some(i), &engine_arch),
            Err(e) => {
                eprintln!("warning: {}: {e:#}", svc.name);
                compose::plan_service(&cs_project, svc, None, &engine_arch)
            }
        };
        entries.push(ServiceEntry {
            probe: probe_spec(svc, &image),
            plan,
        });
    }

    let names: Vec<String> = project.services.iter().map(|s| s.name.clone()).collect();
    let (proxies, expose) = match config::find(Path::new(".")) {
        Some(p) => {
            let r = config::load(&p, &names)?;
            (r.config.proxies, r.expose)
        }
        None => (vec![], vec![]),
    };
    let proxy_plan = fault::plan(&project, &proxies)?;

    let token = session::new_token()?;
    let plans: Vec<ServicePlan> = entries.iter().map(|e| e.plan.clone()).collect();
    let mut isolate = if keep_ports {
        vec![]
    } else {
        compose::isolation_for(&project)
    };
    if !isolate.is_empty() && !compose_supports_reset() {
        eprintln!(
            "warning: Docker Compose < 2.24 cannot remove published ports (`!reset`); \
             the ChronoStack copy keeps them and cannot run next to your own stack"
        );
        isolate.clear();
    }
    let doc = compose::generate_override(
        &plans,
        &ClockdSpec {
            image: clockd_image,
            token: &token,
            proxies: &proxy_plan.targets,
            networks: &proxy_plan.networks,
            env_rewrites: &proxy_plan.rewrites,
        },
        &isolate,
        &expose,
    );
    let override_path = Path::new(session::DIR).join(OVERRIDE_FILE);
    fs::write(&override_path, yaml::to_compose_yaml(&doc)?)?;
    session::restrict(&override_path);

    let mut all_files = files.clone();
    all_files.push(override_path.to_string_lossy().into_owned());
    let cs = Compose {
        files: all_files.clone(),
        project: cs_project.clone(),
    };
    docker::run_inherit(&cs.args(&["up", "-d"]))?;

    let port = resolve_port(&cs)?;
    let exposed = expose
        .iter()
        .map(|e| {
            Ok(expose::Exposed {
                service: e.service.clone(),
                port: e.port,
                host_port: published_port(&cs, &e.service, e.port)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let session = Session {
        version: 1,
        project: cs_project,
        base_project: project.name,
        compose_files: all_files,
        clockd_image: clockd_image.into(),
        engine_arch,
        port,
        token,
        services: entries,
        shared_mounts: project.shared_mounts.clone(),
        exposed,
    };
    session.save()?;
    let st = Client::new(session.base_url(), session.token.clone())
        .wait_ready(Duration::from_secs(30))?;

    println!();
    print_plan_table(&session);
    if !isolate.is_empty() {
        println!(
            "
isolated from your own stack (use `up --keep-ports` to keep these):"
        );
        for i in &isolate {
            let mut what = Vec::new();
            if !i.dropped_ports.is_empty() {
                what.push(format!(
                    "host ports {} not published",
                    i.dropped_ports.join(", ")
                ));
            }
            if let Some(n) = &i.dropped_container_name {
                what.push(format!("container_name `{n}` not used"));
            }
            println!("  {}: {}", i.service, what.join("; "));
        }
    }
    if !session.exposed.is_empty() {
        println!(
            "\nreachable from your machine (`service://<service>:<port>` in chronostack.yml):"
        );
        for e in &session.exposed {
            println!("  {}:{}  ->  {}", e.service, e.port, e.url());
        }
    }
    if !proxy_plan.rewrites.is_empty() {
        println!("\nrouted through the fault proxy (`chronostack fault set <target> …`):");
        for rw in &proxy_plan.rewrites {
            println!(
                "  {}: {} = {}  (was {})",
                rw.service, rw.var, rw.value, rw.original
            );
        }
    }
    println!();
    print_clock(&st, false);
    if let Some(ns) = wait_ns {
        let c = Client::new(session.base_url(), session.token.clone());
        let code = wait_for_probes(&session, &c, Duration::from_nanos(ns.max(0) as u64));
        if code != 0 {
            return Ok(code);
        }
    }
    println!(
        "\nChronoStack project `{}` is up. Try `chronostack advance 1d`.",
        session.project
    );
    Ok(0)
}

/// `up --wait`: poll the time probes until no service is `unavailable`.
/// Returns 0 when all are `ok`, 3 on skew or timeout.
fn wait_for_probes(session: &Session, c: &Client, timeout: Duration) -> i32 {
    let start = std::time::Instant::now();
    let mut last_pending = String::new();
    loop {
        let probes = probe::probe_all(session, c);
        let pending: Vec<&str> = probes
            .iter()
            .filter(|p| p.status == "unavailable")
            .map(|p| p.service.as_str())
            .collect();
        if pending.is_empty() {
            println!();
            probe::print_probes(&probes);
            return if probes.iter().all(|p| p.passes()) {
                0
            } else {
                3
            };
        }
        if start.elapsed() >= timeout {
            println!();
            probe::print_probes(&probes);
            eprintln!(
                "\nservices still not answering after {:.0}s: {}",
                timeout.as_secs_f64(),
                pending.join(", ")
            );
            return 3;
        }
        let now = pending.join(", ");
        if now != last_pending {
            println!("waiting for {now} ...");
            last_pending = now;
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn print_plan_table(s: &Session) {
    let w = s
        .services
        .iter()
        .map(|e| e.plan.service.len())
        .max()
        .unwrap_or(7)
        .max(7);
    println!("{:<w$}  {:<14}  VIRTUALIZED", "SERVICE", "PLATFORM");
    for e in &s.services {
        let p = &e.plan;
        let platform = match (&p.arch, p.libc) {
            (Some(a), Some(l)) => format!("{a}/{}", l.as_str()),
            (Some(a), None) => format!("{a}/static"),
            _ => "unknown".into(),
        };
        let v = if p.virtualized {
            "yes".to_string()
        } else {
            format!("no — {}", p.reason.as_deref().unwrap_or("unknown"))
        };
        println!("{:<w$}  {:<14}  {v}", p.service, platform);
    }
}

fn down(volumes: bool) -> Result<()> {
    let s = Session::load()?;
    let mut args = vec!["down"];
    if volumes {
        args.push("-v");
    }
    docker::run_inherit(&s.compose().args(&args))?;
    let _ = fs::remove_file(Path::new(session::DIR).join(OVERRIDE_FILE));
    fs::remove_file(Session::path())?;
    println!(
        "ChronoStack project `{}` is down.{}",
        s.project,
        if volumes { " Volumes removed." } else { "" }
    );
    Ok(())
}

fn doctor(json_out: bool) -> Result<i32> {
    let mut s = Session::load()?;
    let compose = s.compose();
    let c = client(&mut s);
    let clock = c.clock();
    let running = probe::running_services(&compose).unwrap_or_default();
    let probes = probe::probe_all(&s, &c);

    let mut rows = Vec::new();
    for e in &s.services {
        let p = &e.plan;
        let is_running = running.contains(&p.service);
        let shim = if p.virtualized && is_running {
            probe::shim_loaded(&compose, &p.service)
        } else {
            None
        };
        let procs = if p.virtualized && is_running {
            probe::process_report(&compose, &p.service)
        } else {
            None
        };
        let pr = probes.iter().find(|r| r.service == p.service);
        rows.push(json!({
            "service": p.service,
            "image": p.image,
            "arch": p.arch,
            "libc": p.libc,
            "virtualized": p.virtualized,
            "reason": p.reason,
            "running": is_running,
            "shim_in_pid1": shim,
            "processes": procs,
            "probe": pr,
        }));
    }
    let problems = rows.iter().filter(|r| {
        let completed = r["probe"]["status"] == "completed";
        r["virtualized"] == true
            && !completed
            && (r["running"] == false
                || r["shim_in_pid1"] == false
                || !probe::status_passes(r["probe"]["status"].as_str().unwrap_or(""))
                || r["processes"]["go_processes"]
                    .as_array()
                    .is_some_and(|g| !g.is_empty()))
    });
    let problem_count = problems.count();
    let clockd_version = c.version().ok();

    if json_out {
        println!(
            "{}",
            json!({ "clock": clock.as_ref().ok(), "clockd_reachable": clock.is_ok(), "cli_version": env!("CARGO_PKG_VERSION"), "clockd_version": clockd_version, "services": rows, "shared_mounts": s.shared_mounts, "exposed": s.exposed, "problems": problem_count })
        );
    } else {
        match &clock {
            Ok(st) => {
                println!(
                    "clockd   reachable on 127.0.0.1:{}  virtual {}  ({})  version {}",
                    s.port,
                    format_utc(st.virtual_ns),
                    format_span(st.offset_ns),
                    clockd_version.as_deref().unwrap_or("unknown")
                );
                if clockd_version.as_deref() != Some(env!("CARGO_PKG_VERSION")) {
                    println!(
                        "warning: the CLI is {} but the controller is {}; run `chronostack down` and `up` with matching versions",
                        env!("CARGO_PKG_VERSION"),
                        clockd_version
                            .as_deref()
                            .unwrap_or("older than 0.1.0-alpha.1")
                    );
                }
            }
            Err(e) => println!("clockd   NOT reachable: {e:#}"),
        }
        println!();
        let w = rows
            .iter()
            .filter_map(|r| r["service"].as_str())
            .map(str::len)
            .max()
            .unwrap_or(7)
            .max(7);
        println!(
            "{:<w$}  {:<14}  {:<11}  {:<8}  {:<12}  PROBE",
            "SERVICE", "PLATFORM", "VIRTUALIZED", "RUNNING", "SHIM IN PID1"
        );
        for r in &rows {
            let platform = match (r["arch"].as_str(), r["libc"].as_str()) {
                (Some(a), Some(l)) => format!("{a}/{l}"),
                (Some(a), None) => format!("{a}/static"),
                _ => "unknown".into(),
            };
            let shim = match r["shim_in_pid1"].as_bool() {
                Some(true) => "yes",
                Some(false) => "NO",
                None => "-",
            };
            let probe = match r["probe"]["status"].as_str() {
                Some("ok") => format!("ok ({:+.2}s)", r["probe"]["skew_s"].as_f64().unwrap_or(0.0)),
                Some(other) => other.to_string(),
                None => "-".into(),
            };
            println!(
                "{:<w$}  {:<14}  {:<11}  {:<8}  {:<12}  {probe}",
                r["service"].as_str().unwrap_or(""),
                platform,
                if r["virtualized"] == true {
                    "yes"
                } else {
                    "no"
                },
                if r["running"] == true { "yes" } else { "no" },
                shim,
            );
        }
        let notes: Vec<_> = rows
            .iter()
            .filter_map(|r| r["reason"].as_str().map(|why| format!("  {}: not virtualized — {why}", r["service"].as_str().unwrap_or(""))))
            .chain(rows.iter().filter(|r| r["shim_in_pid1"] == false).map(|r| {
                format!(
                    "  {}: shim not loaded in PID 1 — the main process may be statically linked (Go?) or setuid",
                    r["service"].as_str().unwrap_or("")
                )
            }))
            .chain(rows.iter().filter_map(|r| {
                let go = r["processes"]["go_processes"].as_array()?;
                (!go.is_empty()).then(|| {
                    let names: Vec<_> = go.iter().filter_map(|v| v.as_str()).collect();
                    format!(
                        "  {}: Go process(es) {} read the clock directly (vDSO) and will NOT see virtual time, even though other processes do",
                        r["service"].as_str().unwrap_or(""),
                        names.join(", ")
                    )
                })
            }))
            .collect();
        if !notes.is_empty() {
            println!("\nnotes:");
            notes.iter().for_each(|n| println!("{n}"));
        }
        if !s.exposed.is_empty() {
            println!("\nreachable from your machine:");
            for e in &s.exposed {
                println!("  {}:{}  ->  {}", e.service, e.port, e.url());
            }
        }
        if !s.shared_mounts.is_empty() {
            println!("\nshared with your own stack (not copied):");
            for m in &s.shared_mounts {
                println!("  {}", describe_mount(m));
            }
            if s.shared_mounts.iter().any(|m| !m.read_only) {
                println!("  jobs that run during `advance` can change the writable ones");
            }
        }
        println!(
            "\n{}",
            if problem_count == 0 {
                "all virtualized services look healthy"
            } else {
                "some virtualized services need attention (see above)"
            }
        );
    }
    Ok(if clock.is_err() || problem_count > 0 {
        3
    } else {
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chronostack_core::compose::{SharedKind, SharedMount};

    fn mount(kind: SharedKind, source: &str, read_only: bool) -> SharedMount {
        SharedMount {
            service: "db".into(),
            kind,
            source: source.into(),
            target: "/data".into(),
            read_only,
        }
    }

    #[test]
    fn writable_shared_mounts_stop_up_unless_allowed() {
        let mounts = [
            mount(SharedKind::Bind, "/srv/app/data", false),
            mount(SharedKind::ExternalVolume, "shared-db", false),
        ];
        let err = check_shared_mounts(&mounts, false).unwrap_err().to_string();
        assert!(err.contains("db: /srv/app/data is shared with your own stack (bind mount"));
        assert!(err.contains("db: shared-db is shared with your own stack (external volume"));
        assert!(err.contains("--allow-shared-mounts"));
        assert!(check_shared_mounts(&mounts, true).is_ok());
    }

    #[test]
    fn read_only_and_unshared_mounts_are_allowed() {
        assert!(check_shared_mounts(&[], false).is_ok());
        let ro = [mount(SharedKind::Bind, "/srv/app/conf", true)];
        assert!(check_shared_mounts(&ro, false).is_ok());
        assert!(describe_mount(&ro[0]).contains("(bind mount, read-only, mounted at /data)"));
    }
}
