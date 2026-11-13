//! Thin wrappers around the `docker` CLI.

use std::process::{Command, Output, Stdio};

use anyhow::{Context, Result, bail};

/// Run `docker <args>` and capture output.
pub fn output(args: &[String]) -> Result<Output> {
    Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .context("failed to run `docker` — is Docker installed and on PATH?")
}

/// Run `docker <args>`; return stdout or fail with stderr.
pub fn run(args: &[String]) -> Result<String> {
    let out = output(args)?;
    if !out.status.success() {
        bail!(
            "`docker {}` failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Run `docker <args>` with output streamed to the user's terminal.
pub fn run_inherit(args: &[String]) -> Result<()> {
    let status = Command::new("docker")
        .args(args)
        .status()
        .context("failed to run `docker`")?;
    if !status.success() {
        bail!("`docker {}` exited with {status}", args.join(" "));
    }
    Ok(())
}

pub fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// `docker compose -f … -p …` prefix for a project.
#[derive(Debug, Clone)]
pub struct Compose {
    pub files: Vec<String>,
    pub project: String,
}

impl Compose {
    pub fn args(&self, rest: &[&str]) -> Vec<String> {
        let mut a = vec!["compose".to_string()];
        for f in &self.files {
            a.push("-f".into());
            a.push(f.clone());
        }
        a.push("-p".into());
        a.push(self.project.clone());
        a.extend(rest.iter().map(|x| x.to_string()));
        a
    }
}

/// Result of a command run with a deadline.
#[derive(Debug, Clone)]
pub struct Timed {
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

/// Run `docker <args>` with a timeout. On timeout the docker CLI is killed;
/// a process started by `docker exec` inside the container keeps running
/// unless the caller stops it (see `engine::run_exec`).
pub fn output_timeout(args: &[String], timeout: std::time::Duration) -> Result<Timed> {
    use std::io::Read;
    use std::sync::mpsc;
    use std::time::Duration;
    let start = std::time::Instant::now();
    let mut cmd = Command::new("docker");
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Own process group, so a timeout also kills `docker compose`'s plugin
    // process (which otherwise keeps the output pipes open).
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut cmd, 0);
    let mut child = cmd.spawn().context("failed to run `docker`")?;
    let reader = |mut r: Box<dyn Read + Send>| {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let mut b = Vec::new();
            let _ = r.read_to_end(&mut b);
            let _ = tx.send(String::from_utf8_lossy(&b).into_owned());
        });
        rx
    };
    let r_out = reader(Box::new(child.stdout.take().expect("piped")));
    let r_err = reader(Box::new(child.stderr.take().expect("piped")));
    let mut timed_out = false;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if start.elapsed() > timeout {
            kill_tree(child.id());
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // After a timeout a surviving grandchild may still hold the pipes: don't wait forever.
    let grace = if timed_out {
        Duration::from_secs(2)
    } else {
        Duration::from_secs(30)
    };
    Ok(Timed {
        code: status.and_then(|s| s.code()),
        stdout: r_out.recv_timeout(grace).unwrap_or_default(),
        stderr: r_err.recv_timeout(grace).unwrap_or_default(),
        timed_out,
    })
}

/// Kill a process and its descendants (the `docker` CLI and its compose plugin).
fn kill_tree(pid: u32) {
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    #[cfg(unix)]
    let _ = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
