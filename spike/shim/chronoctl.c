/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/*
 * chronoctl — M0 spike clock writer (stand-in for the future Rust clockd).
 *
 *   chronoctl <page> init                 create/reset the page (offset 0)
 *   chronoctl <page> get                  print JSON snapshot
 *   chronoctl <page> advance <seconds>    move virtual time forward (float ok)
 *   chronoctl <page> set-offset <seconds> absolute offset (spike only; may go back)
 *   chronoctl <page> set-time <epoch>     set virtual time (unfrozen)
 *   chronoctl <page> freeze | unfreeze
 *   chronoctl <page> churn <seconds>      stress: alternate freeze/advance/unfreeze
 *
 * Must NOT run with libchronoclock preloaded (it needs real time).
 */
#define _GNU_SOURCE
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/random.h>
#include <time.h>
#include <unistd.h>

#include "chronoclock.h"

#define NS 1000000000LL

static int64_t real_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    return (int64_t)ts.tv_sec * NS + ts.tv_nsec;
}

/* Layout v2: offsets are relative to CLOCK_MONOTONIC (see chronoclock.h). */
static int64_t mono_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (int64_t)ts.tv_sec * NS + ts.tv_nsec;
}

static struct chrono_page *map_page(const char *path, int create) {
    int fd = open(path, O_RDWR | (create ? O_CREAT : 0), 0644);
    if (fd < 0) { perror("open"); exit(2); }
    if (create && ftruncate(fd, CHRONO_FILE_SIZE) != 0) { perror("ftruncate"); exit(2); }
    void *m = mmap(NULL, CHRONO_FILE_SIZE, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    close(fd);
    if (m == MAP_FAILED) { perror("mmap"); exit(2); }
    return (struct chrono_page *)m;
}

/* Seqlock writer (single writer).
 * The fence must be SEQ_CST, not just RELEASE: freeze/unfreeze read the real
 * clock *inside* the write window, and that clock read must not happen before
 * the odd seq is globally visible. Otherwise a reader on another CPU can pair
 * the old state with a newer real time and observe virtual time stepping back
 * by a few hundred ns (caught by M0 stress test CC-2). */
static void wbegin(struct chrono_page *p) {
    uint64_t s = __atomic_load_n(&p->seq, __ATOMIC_RELAXED);
    __atomic_store_n(&p->seq, s + 1, __ATOMIC_RELAXED);
    __atomic_thread_fence(__ATOMIC_SEQ_CST);
}
static void wend(struct chrono_page *p) {
    uint64_t s = __atomic_load_n(&p->seq, __ATOMIC_RELAXED);
    __atomic_store_n(&p->generation, p->generation + 1, __ATOMIC_RELAXED);
    __atomic_store_n(&p->seq, s + 1, __ATOMIC_RELEASE);
}

static void op_advance(struct chrono_page *p, int64_t d) {
    wbegin(p);
    if (p->frozen) __atomic_store_n(&p->frozen_virtual_ns, p->frozen_virtual_ns + d, __ATOMIC_RELAXED);
    else __atomic_store_n(&p->offset_ns, p->offset_ns + d, __ATOMIC_RELAXED);
    wend(p);
}
static void op_freeze(struct chrono_page *p) {
    if (p->frozen) return;
    wbegin(p);
    __atomic_store_n(&p->frozen_virtual_ns, mono_ns() + p->offset_ns, __ATOMIC_RELAXED);
    __atomic_store_n(&p->frozen, 1u, __ATOMIC_RELAXED);
    wend(p);
}
static void op_unfreeze(struct chrono_page *p) {
    if (!p->frozen) return;
    wbegin(p);
    __atomic_store_n(&p->offset_ns, p->frozen_virtual_ns - mono_ns(), __ATOMIC_RELAXED);
    __atomic_store_n(&p->frozen, 0u, __ATOMIC_RELAXED);
    wend(p);
}

static void print_get(struct chrono_page *p) {
    int64_t r = real_ns();
    int64_t v = p->frozen ? p->frozen_virtual_ns : mono_ns() + p->offset_ns;
    /* printed offset_ns = virtual - real (human meaning), not the stored mono offset */
    printf("{\"real_ns\":%lld,\"virtual_ns\":%lld,\"offset_ns\":%lld,\"frozen\":%u,"
           "\"generation\":%llu,\"seq\":%llu}\n",
           (long long)r, (long long)v, (long long)(v - r), p->frozen,
           (unsigned long long)p->generation, (unsigned long long)p->seq);
}

int main(int argc, char **argv) {
    if (argc < 3) { fprintf(stderr, "usage: chronoctl <page> <cmd> [arg]\n"); return 2; }
    const char *path = argv[1], *cmd = argv[2];
    double arg = argc > 3 ? atof(argv[3]) : 0;

    if (!strcmp(cmd, "init")) {
        struct chrono_page *p = map_page(path, 1);
        memset(p, 0, sizeof(*p));
        uint64_t sid = 0;
        if (getrandom(&sid, sizeof(sid), 0) != sizeof(sid)) sid = (uint64_t)real_ns();
        p->layout_version = CHRONO_LAYOUT_VERSION;
        p->page_size = sizeof(struct chrono_page);
        p->session_id = sid;
        p->offset_ns = real_ns() - mono_ns(); /* virtual starts equal to real */
        __atomic_store_n(&p->magic, CHRONO_MAGIC, __ATOMIC_RELEASE);
        print_get(p);
        return 0;
    }
    struct chrono_page *p = map_page(path, 0);
    if (p->magic != CHRONO_MAGIC) { fprintf(stderr, "bad magic\n"); return 2; }

    if (!strcmp(cmd, "get")) {
    } else if (!strcmp(cmd, "advance")) {
        if (arg <= 0) { fprintf(stderr, "advance: duration must be > 0\n"); return 2; }
        op_advance(p, (int64_t)(arg * NS));
    } else if (!strcmp(cmd, "set-offset")) {
        if (p->frozen) op_unfreeze(p);
        wbegin(p);
        __atomic_store_n(&p->offset_ns, real_ns() + (int64_t)(arg * NS) - mono_ns(), __ATOMIC_RELAXED);
        wend(p);
    } else if (!strcmp(cmd, "set-time")) {
        if (p->frozen) op_unfreeze(p);
        wbegin(p);
        __atomic_store_n(&p->offset_ns, (int64_t)(arg * NS) - mono_ns(), __ATOMIC_RELAXED);
        wend(p);
    } else if (!strcmp(cmd, "freeze")) {
        op_freeze(p);
    } else if (!strcmp(cmd, "unfreeze")) {
        op_unfreeze(p);
    } else if (!strcmp(cmd, "churn")) {
        int64_t end = real_ns() + (int64_t)(arg * NS);
        unsigned long n = 0;
        while (real_ns() < end) {
            op_freeze(p);
            op_advance(p, 1000000);  /* +1 ms while frozen */
            op_unfreeze(p);
            op_advance(p, 1000000);  /* +1 ms while running */
            n++;
        }
        fprintf(stderr, "churn iterations: %lu\n", n);
    } else {
        fprintf(stderr, "unknown command %s\n", cmd);
        return 2;
    }
    print_get(p);
    return 0;
}
