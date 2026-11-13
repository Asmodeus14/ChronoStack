//! Clock page layout v2 — the Rust mirror of `shim/chronoclock.h`.
//!
//! ```text
//! virtual_ns = frozen ? frozen_virtual_ns : CLOCK_MONOTONIC_ns + offset_ns
//! ```
//!
//! Single writer (`clockd`), many readers (the C preload library). Writer
//! protocol (ADR-017):
//!
//! 1. `seq += 1` (odd) with a relaxed store, then a **`SeqCst` fence** — a clock
//!    read inside the write window must not happen before the odd `seq` is
//!    globally visible (M0 stress test CC-2 caught exactly this).
//! 2. Write the data fields (relaxed).
//! 3. `generation += 1`, then `seq += 1` (even) with a release store.

use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64};

/// `"CHRONOST"` as little-endian bytes.
pub const MAGIC: u64 = 0x5453_4F4E_4F52_4843;
/// Layout version understood by this build.
pub const LAYOUT_VERSION: u32 = 2;
/// Size of the backing file (one page).
pub const FILE_SIZE: usize = 4096;

/// The shared clock page. Field order, sizes and alignment must match
/// `struct chrono_page` in `shim/chronoclock.h` (checked by tests here and by
/// `_Static_assert`s there).
#[repr(C, align(64))]
#[derive(Debug, Default)]
pub struct ClockPage {
    pub magic: AtomicU64,
    pub layout_version: AtomicU32,
    pub page_size: AtomicU32,
    pub seq: AtomicU64,
    /// Offset relative to `CLOCK_MONOTONIC`.
    pub offset_ns: AtomicI64,
    pub frozen_virtual_ns: AtomicI64,
    pub frozen: AtomicU32,
    pub flags: AtomicU32,
    pub generation: AtomicU64,
    pub session_id: AtomicU64,
}

/// Field offsets shared with the C header's `_Static_assert`s.
pub const OFFSETS: [(&str, usize); 10] = [
    ("magic", 0),
    ("layout_version", 8),
    ("page_size", 12),
    ("seq", 16),
    ("offset_ns", 24),
    ("frozen_virtual_ns", 32),
    ("frozen", 40),
    ("flags", 44),
    ("generation", 48),
    ("session_id", 56),
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    #[test]
    fn layout_matches_c_header() {
        assert_eq!(size_of::<ClockPage>(), 64);
        assert_eq!(align_of::<ClockPage>(), 64);
        let actual = [
            ("magic", offset_of!(ClockPage, magic)),
            ("layout_version", offset_of!(ClockPage, layout_version)),
            ("page_size", offset_of!(ClockPage, page_size)),
            ("seq", offset_of!(ClockPage, seq)),
            ("offset_ns", offset_of!(ClockPage, offset_ns)),
            (
                "frozen_virtual_ns",
                offset_of!(ClockPage, frozen_virtual_ns),
            ),
            ("frozen", offset_of!(ClockPage, frozen)),
            ("flags", offset_of!(ClockPage, flags)),
            ("generation", offset_of!(ClockPage, generation)),
            ("session_id", offset_of!(ClockPage, session_id)),
        ];
        assert_eq!(actual, OFFSETS);
    }

    #[test]
    fn c_header_asserts_the_same_offsets() {
        let header = include_str!("../../../shim/chronoclock.h");
        assert!(header.contains("sizeof(struct chrono_page) == 64"));
        for (name, off) in OFFSETS {
            let needle = format!("offsetof(struct chrono_page, {name}) == {off},");
            assert!(header.contains(&needle), "C header lacks `{needle}`");
        }
        assert!(header.contains(&format!("#define CHRONO_LAYOUT_VERSION {LAYOUT_VERSION}u")));
        assert!(header.contains("#define CHRONO_MAGIC 0x54534F4E4F524843ULL"));
        assert_eq!(MAGIC.to_le_bytes(), *b"CHRONOST");
    }
}
