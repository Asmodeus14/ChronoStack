//! `chronostack` — Stripe Test Clocks for your Docker Compose stack.

mod client;
mod detect;
mod docker;
mod probe;
mod session;

use std::fs;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use chronostack_core::api::ClockState;
use chronostack_core::compose::{self, ClockdSpec, ServicePlan};
use chronostack_core::duration::{format_span, format_utc, parse_duration};
use chronostack_core::paths;
use clap::{Parser, Subcommand};
use serde_json::json;

use crate::client::Client;
use crate::docker::Compose;
use crate::session::{ProbeSpec, ServiceEntry, Session};

#[derive(Parser)]
#[command(
    name = "chronostack",
    version,
    about = "Stripe Test Clocks for your Docker Compose stack",
    long_about = "Move the virtual wall-clock time of your whole Docker Compose stack.\n\n\
                  ChronoStack changes what time it IS. It does not make time PASS inside your \
                  application's timers: use triggers/barriers (planned, M2) for scheduled work."
)]
struct Cli {
    /// Compose file(s), as with `docker compose -f` (only used by `up`).
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
            default_value = "chronostack/clockd:dev"
        )]
        clockd_image: String,
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
    /// Move virtual time forward, e.g. `25h`, `14d`, `1h30m`.
    Advance {
        duration: String,
        #[arg(long)]
        json: bool,
        /// Skip verifying that services see the new time.
        #[arg(long)]
        no_probe: bool,
    },
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
    /// Explain which services are virtualized, and why not.
    Doctor {
        #[arg(long)]
        json: bool,
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
        Cmd::Up { ref clockd_image } => up(&cli, clockd_image).map(|_| 0),
        Cmd::Down { volumes } => down(volumes).map(|_| 0),
        Cmd::Now { json } => {
            let mut s = Session::load()?;
            print_clock(&client(&mut s).clock()?, json);
            Ok(0)
        }
        Cmd::Advance {
            duration,
            json,
            no_probe,
        } => {
            let by = parse_duration(&duration).map_err(anyhow::Error::msg)?;
            let mut s = Session::load()?;
            let c = client(&mut s);
            let st = c.advance(by)?;
            if no_probe {
                print_clock(&st, json);
                return Ok(0);
            }
            let probes = probe::probe_all(&s, &c);
            let bad = probes.iter().any(|p| p.status != "ok");
            if json {
                println!("{}", json!({ "clock": st, "probes": probes }));
            } else {
                println!("advanced {}", format_span(by));
                print_clock(&st, false);
                println!();
                probe::print_probes(&probes);
            }
            Ok(if bad { 3 } else { 0 })
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
            Ok(if probes.iter().any(|p| p.status != "ok") {
                3
            } else {
                0
            })
        }
        Cmd::Doctor { json } => doctor(json),
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
    let out = docker::run(&compose.args(&[
        "port",
        paths::CLOCKD_SERVICE,
        &paths::CLOCKD_PORT.to_string(),
    ]))?;
    out.trim()
        .rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .with_context(|| format!("unexpected `docker compose port` output: {out:?}"))
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

fn probe_spec(svc: &compose::Service, image: &str) -> ProbeSpec {
    let env = |k: &str| svc.environment.get(k).cloned().flatten();
    let img = image.to_ascii_lowercase();
    if img.contains("postgres")
        || env("POSTGRES_PASSWORD").is_some()
        || env("POSTGRES_USER").is_some()
    {
        let user = env("POSTGRES_USER").unwrap_or_else(|| "postgres".into());
        let db = env("POSTGRES_DB").unwrap_or_else(|| user.clone());
        ProbeSpec::Postgres { user, db }
    } else if img.contains("redis") || img.contains("valkey") {
        ProbeSpec::Redis
    } else {
        ProbeSpec::Date
    }
}

fn up(cli: &Cli, clockd_image: &str) -> Result<()> {
    if Session::path().exists() {
        bail!(
            "a ChronoStack session already exists in this directory; run `chronostack down` first"
        );
    }
    let files = resolve_files(&cli.files)?;
    let mut cfg_args = vec!["compose".to_string()];
    for f in &files {
        cfg_args.push("-f".into());
        cfg_args.push(f.clone());
    }
    cfg_args.extend(docker::s(&["config", "--format", "json"]));
    let project = compose::parse_config(&docker::run(&cfg_args)?).map_err(anyhow::Error::msg)?;
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
        bail!(
            "controller image `{clockd_image}` not found locally.\n\
             Build it from the repository root with:\n  \
             docker build -f docker/clockd.Dockerfile -t {clockd_image} ."
        );
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

    let token = session::new_token()?;
    let plans: Vec<ServicePlan> = entries.iter().map(|e| e.plan.clone()).collect();
    let doc = compose::generate_override(
        &plans,
        &ClockdSpec {
            image: clockd_image,
            token: &token,
        },
    );
    let override_path = Path::new(session::DIR).join("compose.override.json");
    fs::write(&override_path, serde_json::to_string_pretty(&doc)?)?;
    session::restrict(&override_path);

    let mut all_files = files.clone();
    all_files.push(override_path.to_string_lossy().into_owned());
    let cs = Compose {
        files: all_files.clone(),
        project: cs_project.clone(),
    };
    docker::run_inherit(&cs.args(&["up", "-d"]))?;

    let port = resolve_port(&cs)?;

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
    };
    session.save()?;
    let st = Client::new(session.base_url(), session.token.clone())
        .wait_ready(Duration::from_secs(30))?;

    println!();
    print_plan_table(&session);
    println!();
    print_clock(&st, false);
    println!(
        "\nChronoStack project `{}` is up. Try `chronostack advance 1d`.",
        session.project
    );
    Ok(())
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
    let _ = fs::remove_file(Path::new(session::DIR).join("compose.override.json"));
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
            "probe": pr,
        }));
    }
    let problems = rows.iter().filter(|r| {
        r["virtualized"] == true
            && (r["running"] == false || r["shim_in_pid1"] == false || r["probe"]["status"] != "ok")
    });
    let problem_count = problems.count();

    if json_out {
        println!(
            "{}",
            json!({ "clock": clock.as_ref().ok(), "clockd_reachable": clock.is_ok(), "services": rows, "problems": problem_count })
        );
    } else {
        match &clock {
            Ok(st) => println!(
                "clockd   reachable on 127.0.0.1:{}  virtual {}  ({})",
                s.port,
                format_utc(st.virtual_ns),
                format_span(st.offset_ns)
            ),
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
            .collect();
        if !notes.is_empty() {
            println!("\nnotes:");
            notes.iter().for_each(|n| println!("{n}"));
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
