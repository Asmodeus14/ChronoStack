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
/// note that a process started by `docker exec` inside the container may keep
/// running.
pub fn output_timeout(args: &[String], timeout: std::time::Duration) -> Result<Timed> {
    use std::io::Read;
    let start = std::time::Instant::now();
    let mut child = Command::new("docker")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to run `docker`")?;
    let mut so = child.stdout.take().expect("piped");
    let mut se = child.stderr.take().expect("piped");
    let t_out = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = so.read_to_string(&mut s);
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = String::new();
        let _ = se.read_to_string(&mut s);
        s
    });
    let mut timed_out = false;
    let status = loop {
        if let Some(st) = child.try_wait()? {
            break Some(st);
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            timed_out = true;
            break None;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    };
    Ok(Timed {
        code: status.and_then(|s| s.code()),
        stdout: t_out.join().unwrap_or_default(),
        stderr: t_err.join().unwrap_or_default(),
        timed_out,
    })
}
