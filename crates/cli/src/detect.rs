//! Image inspection: architecture, libc family and baked-in `LD_PRELOAD`.
//!
//! libc detection never runs the image: it creates a stopped container and
//! checks for dynamic-loader paths with `docker cp`, which also works for
//! shell-less images (distroless, scratch). Results are cached per image id.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chronostack_core::compose::{ImageInfo, Libc, normalize_arch};
use serde_json::Value;

use crate::docker;

/// Loader paths, checked in order. musl first: Alpine images have no glibc loader.
const LOADERS: &[(&str, Libc)] = &[
    ("/lib/ld-musl-x86_64.so.1", Libc::Musl),
    ("/lib/ld-musl-aarch64.so.1", Libc::Musl),
    ("/lib64/ld-linux-x86-64.so.2", Libc::Gnu),
    ("/usr/lib64/ld-linux-x86-64.so.2", Libc::Gnu),
    ("/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2", Libc::Gnu),
    ("/usr/lib/x86_64-linux-gnu/ld-linux-x86-64.so.2", Libc::Gnu),
    ("/lib/ld-linux-aarch64.so.1", Libc::Gnu),
    ("/usr/lib/ld-linux-aarch64.so.1", Libc::Gnu),
    ("/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1", Libc::Gnu),
    (
        "/usr/lib/aarch64-linux-gnu/ld-linux-aarch64.so.1",
        Libc::Gnu,
    ),
];

/// `uname -m` style architecture of the Docker engine.
pub fn engine_arch() -> Result<String> {
    let raw = docker::run(&docker::s(&["version", "--format", "{{.Server.Arch}}"]))?;
    let raw = raw.trim();
    normalize_arch(raw)
        .map(str::to_string)
        .ok_or_else(|| anyhow!("unsupported Docker engine architecture `{raw}`"))
}

/// Pull the image if it is not present locally.
pub fn ensure_image(image: &str, platform: Option<&str>) -> Result<()> {
    if docker::output(&docker::s(&["image", "inspect", image]))?
        .status
        .success()
    {
        return Ok(());
    }
    eprintln!("pulling {image} …");
    let mut args = docker::s(&["pull", "--quiet"]);
    if let Some(p) = platform {
        args.push("--platform".into());
        args.push(p.into());
    }
    args.push(image.into());
    docker::run(&args).map(|_| ())
}

type Cache = BTreeMap<String, Option<Libc>>;

fn load_cache(path: &Path) -> Cache {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Inspect an image (must exist locally).
pub fn inspect(image: &str, cache_path: &Path) -> Result<ImageInfo> {
    let raw = docker::run(&docker::s(&[
        "image",
        "inspect",
        "--format",
        "{{json .}}",
        image,
    ]))?;
    let v: Value =
        serde_json::from_str(raw.trim()).context("parsing `docker image inspect` output")?;
    let id = v["Id"].as_str().unwrap_or_default().to_string();
    let arch = v["Architecture"].as_str().unwrap_or_default().to_string();
    let image_ld_preload = v["Config"]["Env"].as_array().and_then(|env| {
        env.iter()
            .filter_map(Value::as_str)
            .find_map(|e| e.strip_prefix("LD_PRELOAD=").map(str::to_string))
    });

    let mut cache = load_cache(cache_path);
    let libc = match cache.get(&id) {
        Some(l) => *l,
        None => {
            let l = probe_libc(image)?;
            cache.insert(id.clone(), l);
            if let Ok(text) = serde_json::to_string_pretty(&cache) {
                let _ = fs::write(cache_path, text);
            }
            l
        }
    };
    Ok(ImageInfo {
        id,
        arch,
        libc,
        image_ld_preload,
    })
}

fn probe_libc(image: &str) -> Result<Option<Libc>> {
    // --entrypoint is not validated at create time, so this works for any image.
    let cid = docker::run(&docker::s(&[
        "create",
        "--entrypoint",
        "/chronostack-probe",
        image,
    ]))?
    .trim()
    .to_string();
    let mut found = None;
    for (path, libc) in LOADERS {
        let out = docker::output(&docker::s(&["cp", &format!("{cid}:{path}"), "-"]))?;
        if out.status.success() {
            found = Some(*libc);
            break;
        }
    }
    let _ = docker::output(&docker::s(&["rm", "-f", &cid]));
    Ok(found)
}
