//! `chronostack update`: replace this binary with a newer release from GitHub.
//!
//! Same steps as the install scripts: pick the release, download the archive for
//! this platform and the release's `SHA256SUMS`, verify the checksum, then swap
//! the running executable (works on Windows too, where a running .exe can't be
//! overwritten directly). No admin rights: it only writes where the binary is.

use std::io::Read;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use semver::Version;
use sha2::{Digest, Sha256};

const REPO: &str = "Asmodeus14/ChronoStack";
const MAX_DOWNLOAD: u64 = 200 * 1024 * 1024;

/// Release archive suffix for this build, matching the release workflow.
pub fn target() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => Some("linux-x64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("macos", "aarch64") => Some("darwin-arm64"),
        ("macos", "x86_64") => Some("darwin-x64"),
        ("windows", "x86_64") => Some("win32-x64"),
        _ => None,
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .user_agent(concat!("chronostack-cli/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn get(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>> {
    let mut resp = agent
        .get(url)
        .call()
        .with_context(|| format!("downloading {url}"))?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(MAX_DOWNLOAD)
        .read_to_end(&mut buf)?;
    Ok(buf)
}

/// Newest published release (pre-releases only if `pre`), as a version.
fn newest(agent: &ureq::Agent, pre: bool) -> Result<Version> {
    let body = get(
        agent,
        &format!("https://api.github.com/repos/{REPO}/releases?per_page=30"),
    )?;
    let releases: Vec<serde_json::Value> =
        serde_json::from_slice(&body).context("unexpected GitHub API response")?;
    releases
        .iter()
        .filter(|r| r["draft"] != true)
        .filter_map(|r| r["tag_name"].as_str())
        .filter_map(|t| Version::parse(t.trim_start_matches('v')).ok())
        .filter(|v| pre || v.pre.is_empty())
        .max()
        .ok_or_else(|| anyhow!("no published release found on github.com/{REPO}"))
}

/// The checksum listed for `file` in a SHA256SUMS text (`<hex>  <file>` or `<hex> *<file>`).
fn expected_sha(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (h, f) = (it.next()?, it.next()?);
        (f.trim_start_matches('*') == file).then(|| h.to_ascii_lowercase())
    })
}

fn extract(archive: &[u8], zip: bool, name: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    if zip {
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(archive))?;
        z.by_name(name)?.read_to_end(&mut out)?;
    } else {
        let mut t = tar::Archive::new(flate2::read::GzDecoder::new(archive));
        for entry in t.entries()? {
            let mut e = entry?;
            if e.path()?.to_string_lossy().trim_start_matches("./") == name {
                e.read_to_end(&mut out)?;
                return Ok(out);
            }
        }
        bail!("`{name}` not found in the archive");
    }
    Ok(out)
}

pub struct Options {
    pub check: bool,
    pub version: Option<String>,
    pub pre: bool,
    pub force: bool,
}

pub fn run(o: &Options) -> Result<i32> {
    let current = Version::parse(env!("CARGO_PKG_VERSION"))?;
    let target = target().ok_or_else(|| {
        anyhow!(
            "no release builds for {}/{}",
            std::env::consts::OS,
            std::env::consts::ARCH
        )
    })?;
    let agent = agent();
    // Pre-release builds follow pre-releases; stable builds only stable ones.
    let pre = o.pre || !current.pre.is_empty();
    let wanted = match &o.version {
        Some(v) => Version::parse(v.trim_start_matches('v')).context("--version")?,
        None => newest(&agent, pre)?,
    };
    if o.check {
        if wanted > current {
            println!("update available: {current} → {wanted} (run `chronostack update`)");
        } else {
            println!("chronostack {current} is up to date");
        }
        return Ok(0);
    }
    if wanted <= current && !o.force && o.version.is_none() {
        println!("chronostack {current} is up to date");
        return Ok(0);
    }

    let base = std::env::var("CHRONOSTACK_DOWNLOAD_BASE")
        .unwrap_or_else(|_| format!("https://github.com/{REPO}/releases/download/v{wanted}"));
    let zip = target.starts_with("win32");
    let file = format!(
        "chronostack-{wanted}-{target}.{}",
        if zip { "zip" } else { "tar.gz" }
    );
    eprintln!("downloading {file} …");
    let archive = get(&agent, &format!("{base}/{file}"))?;
    let sums = String::from_utf8(get(&agent, &format!("{base}/SHA256SUMS"))?)?;
    let want = expected_sha(&sums, &file)
        .ok_or_else(|| anyhow!("{file} is not listed in the release's SHA256SUMS"))?;
    let got: String = Sha256::digest(&archive)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if got != want {
        bail!("checksum mismatch for {file} (expected {want}, got {got}); nothing was changed");
    }
    eprintln!("checksum ok");

    let bin = extract(
        &archive,
        zip,
        if zip {
            "chronostack.exe"
        } else {
            "chronostack"
        },
    )?;
    let exe = std::env::current_exe()?;
    let tmp = exe.with_file_name(format!(".chronostack-update-{}", std::process::id()));
    std::fs::write(&tmp, &bin)
        .with_context(|| format!("cannot write next to {}", exe.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755))?;
    }
    let swapped = self_replace::self_replace(&tmp);
    let _ = std::fs::remove_file(&tmp);
    swapped.with_context(|| format!("cannot replace {}", exe.display()))?;
    println!(
        "updated chronostack {current} → {wanted} ({})",
        exe.display()
    );
    println!(
        "the controller image follows the CLI: run `chronostack down` and `chronostack up` in running projects"
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checksum_lines() {
        let sums =
            "aa11  chronostack-1.0.0-linux-x64.tar.gz\nBB22 *chronostack-1.0.0-win32-x64.zip\n";
        assert_eq!(
            expected_sha(sums, "chronostack-1.0.0-linux-x64.tar.gz").as_deref(),
            Some("aa11")
        );
        assert_eq!(
            expected_sha(sums, "chronostack-1.0.0-win32-x64.zip").as_deref(),
            Some("bb22")
        );
        assert!(expected_sha(sums, "other").is_none());
    }

    #[test]
    fn extracts_from_both_archive_kinds() {
        // tar.gz with a top-level `chronostack`
        let mut tar_bytes = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut tar_bytes, flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            let mut h = tar::Header::new_gnu();
            h.set_size(3);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, "chronostack", &b"bin"[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        assert_eq!(extract(&tar_bytes, false, "chronostack").unwrap(), b"bin");
        // zip with chronostack.exe
        let mut zip_bytes = std::io::Cursor::new(Vec::new());
        {
            let mut z = zip::ZipWriter::new(&mut zip_bytes);
            z.start_file("chronostack.exe", zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut z, b"exe").unwrap();
            z.finish().unwrap();
        }
        assert_eq!(
            extract(zip_bytes.get_ref(), true, "chronostack.exe").unwrap(),
            b"exe"
        );
    }

    #[test]
    fn known_targets() {
        assert!(target().is_some(), "release builds exist for this platform");
    }
}
