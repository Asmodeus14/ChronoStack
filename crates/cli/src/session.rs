//! `.chronostack/` state: session file (token, port, plans) and caches.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chronostack_core::compose::ServicePlan;
use serde::{Deserialize, Serialize};

use crate::docker::Compose;

pub const DIR: &str = ".chronostack";

/// How `probe` reads a service's clock.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ProbeSpec {
    Postgres { user: String, db: String },
    Redis,
    Date,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceEntry {
    #[serde(flatten)]
    pub plan: ServicePlan,
    pub probe: ProbeSpec,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub version: u32,
    /// Compose project ChronoStack runs (`<base>-chronostack` by default).
    pub project: String,
    /// The user's own project name.
    pub base_project: String,
    /// User compose files followed by the override.
    pub compose_files: Vec<String>,
    pub clockd_image: String,
    pub engine_arch: String,
    pub port: u16,
    pub token: String,
    pub services: Vec<ServiceEntry>,
}

impl Session {
    pub fn path() -> PathBuf {
        Path::new(DIR).join("session.json")
    }

    pub fn load() -> Result<Session> {
        let p = Self::path();
        let text = fs::read_to_string(&p).with_context(|| {
            format!(
                "no ChronoStack session here ({} missing) — run `chronostack up` first",
                p.display()
            )
        })?;
        let s: Session = serde_json::from_str(&text).context("corrupt session file")?;
        if s.version != 1 {
            bail!(
                "session file version {} is not supported by this CLI",
                s.version
            );
        }
        Ok(s)
    }

    pub fn save(&self) -> Result<()> {
        ensure_dir()?;
        let p = Self::path();
        fs::write(&p, serde_json::to_string_pretty(self)?)?;
        restrict(&p);
        Ok(())
    }

    pub fn compose(&self) -> Compose {
        Compose {
            files: self.compose_files.clone(),
            project: self.project.clone(),
        }
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
}

/// Create `.chronostack/` with a `.gitignore` so the token is never committed.
pub fn ensure_dir() -> Result<()> {
    fs::create_dir_all(DIR)?;
    let gi = Path::new(DIR).join(".gitignore");
    if !gi.exists() {
        fs::write(
            gi,
            "# ChronoStack runtime state (contains a session token)\n*\n",
        )?;
    }
    Ok(())
}

/// Owner-only permissions where the platform supports them.
pub fn restrict(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    let _ = path; // Windows: files under the user's profile/project inherit user ACLs.
}

/// 256-bit random token, hex encoded.
pub fn new_token() -> Result<String> {
    let mut b = [0u8; 32];
    getrandom::getrandom(&mut b).map_err(|e| anyhow::anyhow!("no OS randomness: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}
