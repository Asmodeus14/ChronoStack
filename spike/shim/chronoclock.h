/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/*
 * ChronoStack clock page, layout v1 (M0 spike).
 *
 * Single writer (clockd / chronoctl), many readers (libchronoclock in every
 * target process). Readers use a seqlock: an odd `seq` means a write is in
 * progress; a changed `seq` means the read must be retried.
 *
 * virtual_ns = frozen ? frozen_virtual_ns : CLOCK_REALTIME_ns + offset_ns
 */
#ifndef CHRONOSTACK_CLOCK_H
#define CHRONOSTACK_CLOCK_H

#include <stdint.h>

#define CHRONO_MAGIC 0x54534F4E4F524843ULL /* "CHRONOST" as little-endian bytes */
#define CHRONO_LAYOUT_VERSION 1u
#define CHRONO_FILE_SIZE 4096

struct chrono_page {
    uint64_t magic;
    uint32_t layout_version;
    uint32_t page_size;          /* sizeof(struct chrono_page) */
    uint64_t seq;                /* seqlock counter */
    int64_t  offset_ns;
    int64_t  frozen_virtual_ns;
    uint32_t frozen;
    uint32_t flags;              /* reserved */
    uint64_t generation;         /* +1 per committed change */
    uint64_t session_id;
} __attribute__((aligned(64)));

#endif
