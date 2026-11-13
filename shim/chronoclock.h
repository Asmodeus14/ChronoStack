/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/*
 * ChronoStack clock page, layout v2.
 *
 * Single writer (clockd), many readers (libchronoclock in every target
 * process). Readers use a seqlock: an odd `seq` means a write is in progress;
 * a changed `seq` means the read must be retried.
 *
 * Virtual time is anchored to CLOCK_MONOTONIC, not CLOCK_REALTIME, so host
 * clock corrections (NTP / WSL2 time-sync steps, measured at ~2 ms backwards in
 * M0) never leak into virtual time (ADR-017):
 *
 *   virtual_ns = frozen ? frozen_virtual_ns : CLOCK_MONOTONIC_ns + offset_ns
 *
 * All containers share the kernel's CLOCK_MONOTONIC (no time namespaces).
 *
 * The Rust mirror of this struct is crates/core/src/page.rs; both sides assert
 * the same offsets, so a layout change cannot compile on one side only.
 */
#ifndef CHRONOSTACK_CLOCK_H
#define CHRONOSTACK_CLOCK_H

#include <stddef.h>
#include <stdint.h>

#define CHRONO_MAGIC 0x54534F4E4F524843ULL /* "CHRONOST" as little-endian bytes */
#define CHRONO_LAYOUT_VERSION 2u
#define CHRONO_FILE_SIZE 4096

struct chrono_page {
    uint64_t magic;
    uint32_t layout_version;
    uint32_t page_size;          /* sizeof(struct chrono_page) */
    uint64_t seq;                /* seqlock counter */
    int64_t  offset_ns;          /* relative to CLOCK_MONOTONIC */
    int64_t  frozen_virtual_ns;
    uint32_t frozen;
    uint32_t flags;              /* reserved */
    uint64_t generation;         /* +1 per committed change */
    uint64_t session_id;
} __attribute__((aligned(64)));

_Static_assert(sizeof(struct chrono_page) == 64, "chrono_page size");
_Static_assert(offsetof(struct chrono_page, magic) == 0, "magic");
_Static_assert(offsetof(struct chrono_page, layout_version) == 8, "layout_version");
_Static_assert(offsetof(struct chrono_page, page_size) == 12, "page_size");
_Static_assert(offsetof(struct chrono_page, seq) == 16, "seq");
_Static_assert(offsetof(struct chrono_page, offset_ns) == 24, "offset_ns");
_Static_assert(offsetof(struct chrono_page, frozen_virtual_ns) == 32, "frozen_virtual_ns");
_Static_assert(offsetof(struct chrono_page, frozen) == 40, "frozen");
_Static_assert(offsetof(struct chrono_page, flags) == 44, "flags");
_Static_assert(offsetof(struct chrono_page, generation) == 48, "generation");
_Static_assert(offsetof(struct chrono_page, session_id) == 56, "session_id");

#endif
