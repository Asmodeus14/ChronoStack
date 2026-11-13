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
