//! Shared building blocks for ChronoStack.
//!
//! - [`page`]: the clock page layout shared with the C preload library.
//! - [`api`]: controller (`clockd`) HTTP API types.
//! - [`duration`]: human durations (`14d`, `1h30m`) and timestamp formatting.
//! - [`compose`]: resolved Compose model and override generation.
//! - [`schedule`]: advance planning (steps, scheduled triggers, max-jump sub-steps).

pub mod api;
pub mod compose;
pub mod duration;
pub mod page;
pub mod schedule;

/// Fixed paths inside containers.
pub mod paths {
    /// Mount point of the ChronoStack volume in every container.
    pub const MOUNT: &str = "/chronostack";
    /// Clock page.
    pub const CLOCK: &str = "/chronostack/clock";
    /// Directory holding the preload libraries.
    pub const LIB_DIR: &str = "/chronostack/lib";
    /// Port `clockd` listens on inside its container.
    pub const CLOCKD_PORT: u16 = 7171;
    /// Compose service name of the controller.
    pub const CLOCKD_SERVICE: &str = "chronostack-clockd";
    /// Compose volume name (project-scoped by Compose).
    pub const VOLUME: &str = "chronostack";
}

/// File name of the preload library for an architecture (`uname -m` style) and libc.
pub fn shim_file_name(arch: &str, libc: compose::Libc) -> String {
    format!("libchronoclock-{arch}-{}.so", libc.as_str())
}
