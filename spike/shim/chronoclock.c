/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/*
 * libchronoclock — ChronoStack M0 spike preload library.
 *
 * - Virtualizes wall-clock reads (CLOCK_REALTIME*, CLOCK_TAI, gettimeofday,
 *   time, timespec_get). Monotonic and CPU clocks pass through untouched.
 * - Translates absolute CLOCK_REALTIME deadlines (virtual -> real) for
 *   sem_timedwait, pthread_mutex_timedlock, pthread_rwlock_timed*lock,
 *   pthread_cond_timedwait (per-condvar clock tracked), *_clockwait/clocklock,
 *   clock_nanosleep(TIMER_ABSTIME), timerfd_settime(TFD_TIMER_ABSTIME).
 * - Fails open (real time + one stderr line) unless CHRONOSTACK_STRICT=1.
 *
 * Environment:
 *   CHRONOSTACK_CLOCK_PATH   path of the clock page (required to activate)
 *   CHRONOSTACK_STRICT=1     abort() if the page cannot be attached
 *   CHRONOSTACK_DEADLINE_FIX=0  disable deadline translation (negative tests)
 *
 * Not yet handled in M0 (documented): timer_settime(TIMER_ABSTIME), ftime,
 * mq_timed*, raw futex syscalls, raw clock_gettime syscalls.
 */

/* Hide libc prototypes of the functions we interpose so we can declare them
 * with our own (ABI-compatible) signatures regardless of libc header version. */
#define _GNU_SOURCE
#define clock_gettime           __cs_hide_clock_gettime
#define gettimeofday            __cs_hide_gettimeofday
#define time                    __cs_hide_time
#define timespec_get            __cs_hide_timespec_get
#define clock_nanosleep         __cs_hide_clock_nanosleep
#define sem_timedwait           __cs_hide_sem_timedwait
#define sem_clockwait           __cs_hide_sem_clockwait
#define pthread_mutex_timedlock __cs_hide_pthread_mutex_timedlock
#define pthread_mutex_clocklock __cs_hide_pthread_mutex_clocklock
#define pthread_rwlock_timedrdlock __cs_hide_pthread_rwlock_timedrdlock
#define pthread_rwlock_timedwrlock __cs_hide_pthread_rwlock_timedwrlock
#define pthread_rwlock_clockrdlock __cs_hide_pthread_rwlock_clockrdlock
#define pthread_rwlock_clockwrlock __cs_hide_pthread_rwlock_clockwrlock
#define pthread_cond_timedwait  __cs_hide_pthread_cond_timedwait
#define pthread_cond_clockwait  __cs_hide_pthread_cond_clockwait
#define pthread_cond_init       __cs_hide_pthread_cond_init
#define pthread_cond_destroy    __cs_hide_pthread_cond_destroy
#define timerfd_create          __cs_hide_timerfd_create
#define timerfd_settime         __cs_hide_timerfd_settime
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <pthread.h>
#include <semaphore.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/time.h>
#include <sys/timerfd.h>
#include <time.h>
#include <unistd.h>
#undef clock_gettime
#undef gettimeofday
#undef time
#undef timespec_get
#undef clock_nanosleep
#undef sem_timedwait
#undef sem_clockwait
#undef pthread_mutex_timedlock
#undef pthread_mutex_clocklock
#undef pthread_rwlock_timedrdlock
#undef pthread_rwlock_timedwrlock
#undef pthread_rwlock_clockrdlock
#undef pthread_rwlock_clockwrlock
#undef pthread_cond_timedwait
#undef pthread_cond_clockwait
#undef pthread_cond_init
#undef pthread_cond_destroy
#undef timerfd_create
#undef timerfd_settime

#include "chronoclock.h"

#define EXPORT __attribute__((visibility("default")))
#define NS_PER_SEC 1000000000LL
#define MAX_ABS_OFFSET_NS (100LL * 366 * 24 * 3600 * NS_PER_SEC)

#ifndef CLOCK_REALTIME_ALARM
#define CLOCK_REALTIME_ALARM 8
#endif
#ifndef CLOCK_TAI
#define CLOCK_TAI 11
#endif
#ifndef TIME_UTC
#define TIME_UTC 1
#endif

/* ------------------------------------------------------------------------ */
/* Real symbol resolution                                                    */
/* ------------------------------------------------------------------------ */

static void *resolve(const char *name, const char *glibc_ver) {
    void *p = NULL;
#ifdef __GLIBC__
    if (glibc_ver) p = dlvsym(RTLD_NEXT, name, glibc_ver);
#else
    (void)glibc_ver;
#endif
    if (!p) p = dlsym(RTLD_NEXT, name);
    return p;
}

#define REAL(var, name, ver)                                                  \
    do {                                                                      \
        if (!__atomic_load_n(&(var), __ATOMIC_ACQUIRE))                       \
            __atomic_store_n(&(var), (__typeof__(var))resolve(name, ver),     \
                             __ATOMIC_RELEASE);                               \
    } while (0)

typedef int (*clock_gettime_fn)(clockid_t, struct timespec *);
static clock_gettime_fn real_clock_gettime_p;

static int raw_clock_gettime(clockid_t clk, struct timespec *ts) {
    REAL(real_clock_gettime_p, "clock_gettime", NULL);
    if (real_clock_gettime_p) return real_clock_gettime_p(clk, ts);
    return (int)syscall(SYS_clock_gettime, clk, ts);
}

/* ------------------------------------------------------------------------ */
/* Clock page attach                                                         */
/* ------------------------------------------------------------------------ */

enum { ST_UNINIT = 0, ST_INITIALIZING = 1, ST_READY = 2, ST_DISABLED = 3 };
static int g_state = ST_UNINIT;
static struct chrono_page *g_page;
static int g_fix_deadlines = 1;

static void cs_warn(const char *msg) {
    static const char pfx[] = "chronostack: ";
    ssize_t r = write(2, pfx, sizeof(pfx) - 1);
    r = write(2, msg, strlen(msg));
    r = write(2, "\n", 1);
    (void)r;
}

static void fail(const char *msg, int strict) {
    if (strict) {
        cs_warn(msg);
        cs_warn("CHRONOSTACK_STRICT=1: aborting");
        abort();
    }
    cs_warn(msg);
    cs_warn("clock unavailable, using real time");
}

static void do_init(void) {
    const char *path = getenv("CHRONOSTACK_CLOCK_PATH");
    const char *strict_s = getenv("CHRONOSTACK_STRICT");
    const char *fix_s = getenv("CHRONOSTACK_DEADLINE_FIX");
    int strict = strict_s && strict_s[0] == '1';
    if (fix_s && fix_s[0] == '0') g_fix_deadlines = 0;

    if (!path || !path[0]) { __atomic_store_n(&g_state, ST_DISABLED, __ATOMIC_RELEASE); return; }

    int fd = open(path, O_RDONLY | O_CLOEXEC);
    if (fd < 0) { fail("cannot open CHRONOSTACK_CLOCK_PATH", strict); goto disabled; }
    struct stat st;
    if (fstat(fd, &st) != 0 || st.st_size < (off_t)sizeof(struct chrono_page)) {
        close(fd); fail("clock page too small", strict); goto disabled;
    }
    void *m = mmap(NULL, CHRONO_FILE_SIZE, PROT_READ, MAP_SHARED, fd, 0);
    close(fd);
    if (m == MAP_FAILED) { fail("mmap of clock page failed", strict); goto disabled; }
    struct chrono_page *p = (struct chrono_page *)m;
    if (p->magic != CHRONO_MAGIC || p->layout_version != CHRONO_LAYOUT_VERSION ||
        p->page_size != sizeof(struct chrono_page)) {
        munmap(m, CHRONO_FILE_SIZE); fail("invalid clock page (magic/version)", strict); goto disabled;
    }
    __atomic_store_n(&g_page, p, __ATOMIC_RELEASE);
    __atomic_store_n(&g_state, ST_READY, __ATOMIC_RELEASE);
    return;
disabled:
    __atomic_store_n(&g_state, ST_DISABLED, __ATOMIC_RELEASE);
}

static struct chrono_page *page(void) {
    int st = __atomic_load_n(&g_state, __ATOMIC_ACQUIRE);
    if (st == ST_READY) return g_page;
    if (st == ST_DISABLED) return NULL;
    int expected = ST_UNINIT;
    if (__atomic_compare_exchange_n(&g_state, &expected, ST_INITIALIZING, 0,
                                    __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) {
        do_init();
    } else {
        /* Another thread is initializing; wait briefly, else pass through. */
        for (long i = 0; i < 10000000L; i++) {
            st = __atomic_load_n(&g_state, __ATOMIC_ACQUIRE);
            if (st != ST_INITIALIZING) break;
        }
    }
    return __atomic_load_n(&g_state, __ATOMIC_ACQUIRE) == ST_READY ? g_page : NULL;
}

/* ------------------------------------------------------------------------ */
/* Seqlock read: returns virtual and real time taken inside one consistent  */
/* snapshot (real clock is read inside the seqlock window so freeze/unfreeze */
/* transitions can never produce a backwards step for a reader).             */
/* ------------------------------------------------------------------------ */

/* Per-thread last good snapshot: used only if the writer dies mid-update
 * (seq stuck odd). Per-thread so it can never mix values across threads
 * (an M0 stress test caught exactly that with a shared snapshot). */
static __thread int64_t tls_off, tls_fv;
static __thread uint32_t tls_frozen, tls_valid;
/* Odd seq value observed to be stuck; reads seeing it skip the spin. */
static uint64_t g_stuck_seq = 0;
/* A writer merely descheduled mid-update must NOT trigger the fallback (M0
 * stress test: a spin-count threshold fired after ~2 ms of preemption and
 * produced a 2.3 ms backward step). Only a seq stuck for this long counts. */
#define STUCK_NS (1000LL * 1000 * 1000)

static inline void cpu_relax(void) {
#if defined(__x86_64__) || defined(__i386__)
    __builtin_ia32_pause();
#elif defined(__aarch64__)
    __asm__ __volatile__("yield");
#endif
}

static inline int64_t ts_ns(const struct timespec *t) { return (int64_t)t->tv_sec * NS_PER_SEC + t->tv_nsec; }
static inline void ns_ts(int64_t ns, struct timespec *t) {
    if (ns < 0) ns = 0;
    t->tv_sec = (time_t)(ns / NS_PER_SEC);
    t->tv_nsec = (long)(ns % NS_PER_SEC);
}

/* Layout v2: virtual time is anchored to the monotonic clock (immune to host
 * clock steps). Coarse wall clock -> coarse monotonic base. */
static inline clockid_t base_clock(clockid_t clk) {
    return clk == CLOCK_REALTIME_COARSE ? CLOCK_MONOTONIC_COARSE : CLOCK_MONOTONIC;
}

/* Fill virt (and real, only if requested: it costs a second clock read on
 * the hot path, which M0 measured at ~+37 ns). CLOCK_TAI keeps its kernel
 * offset from UTC. */
static int finish(clockid_t clk, int64_t v, int64_t *virt_ns, int64_t *real_ns) {
    if (real_ns || clk == CLOCK_TAI) {
        struct timespec rt;
        if (raw_clock_gettime(clk, &rt) != 0) return -1;
        if (real_ns) *real_ns = ts_ns(&rt);
        if (clk == CLOCK_TAI) {
            struct timespec utc;
            if (raw_clock_gettime(CLOCK_REALTIME, &utc) == 0) v += ts_ns(&rt) - ts_ns(&utc);
        }
    }
    *virt_ns = v;
    return 1;
}

/* Read `clk` (a wall clock) and return 1 with virt_ns/real_ns filled if
 * virtualization is active; 0 means "use real time". real_ns may be NULL. */
static int read_virtual(clockid_t clk, int64_t *virt_ns, int64_t *real_ns) {
    struct chrono_page *p = page();
    struct timespec rt;
    if (!p) {
        if (!real_ns) return 0;
        if (raw_clock_gettime(clk, &rt) != 0) return -1;
        *real_ns = ts_ns(&rt);
        return 0;
    }
    /* Retry for as long as the writer is making progress (seq changing);
     * fall back only if seq stays odd and unchanged (writer died mid-update). */
    uint64_t odd_seen = 0;
    long spins = 0;
    int64_t odd_since = 0;
    for (;;) {
        uint64_t s1 = __atomic_load_n(&p->seq, __ATOMIC_ACQUIRE);
        if (s1 & 1) {
            if (s1 == __atomic_load_n(&g_stuck_seq, __ATOMIC_RELAXED)) break;
            if (s1 != odd_seen) {
                struct timespec m;
                raw_clock_gettime(CLOCK_MONOTONIC, &m);
                odd_seen = s1; spins = 0; odd_since = ts_ns(&m);
            } else if ((++spins & 1023) == 0) {
                struct timespec m;
                raw_clock_gettime(CLOCK_MONOTONIC, &m);
                if (ts_ns(&m) - odd_since > STUCK_NS) {
                    __atomic_store_n(&g_stuck_seq, s1, __ATOMIC_RELAXED);
                    break;
                }
            }
            cpu_relax();
            continue;
        }
        int64_t off = __atomic_load_n(&p->offset_ns, __ATOMIC_RELAXED);
        uint32_t fz = __atomic_load_n(&p->frozen, __ATOMIC_RELAXED);
        int64_t fv = __atomic_load_n(&p->frozen_virtual_ns, __ATOMIC_RELAXED);
        struct timespec mt;
        if (raw_clock_gettime(base_clock(clk), &mt) != 0) return -1;
        __atomic_thread_fence(__ATOMIC_ACQUIRE);
        uint64_t s2 = __atomic_load_n(&p->seq, __ATOMIC_RELAXED);
        if (s1 != s2) continue;
        tls_off = off; tls_fv = fv; tls_frozen = fz; tls_valid = 1;
        return finish(clk, fz ? fv : ts_ns(&mt) + off, virt_ns, real_ns);
    }
    /* Writer stuck mid-update: this thread's last good snapshot, else real time. */
    if (!tls_valid) {
        if (!real_ns) return 0;
        if (raw_clock_gettime(clk, &rt) != 0) return -1;
        *real_ns = ts_ns(&rt);
        return 0;
    }
    struct timespec mt;
    if (raw_clock_gettime(base_clock(clk), &mt) != 0) return -1;
    return finish(clk, tls_frozen ? tls_fv : ts_ns(&mt) + tls_off, virt_ns, real_ns);
}

static inline int is_wall(clockid_t c) {
    return c == CLOCK_REALTIME || c == CLOCK_REALTIME_COARSE || c == CLOCK_TAI ||
           c == CLOCK_REALTIME_ALARM;
}

/* Convert an absolute deadline expressed in virtual wall time into the
 * equivalent absolute real wall-clock deadline. Returns 1 if *out was set. */
static int to_real_deadline(const struct timespec *in, struct timespec *out) {
    if (!in || !g_fix_deadlines) return 0;
    if (!page()) return 0;
    if (in->tv_sec > 9000000000LL || in->tv_sec < 0) return 0; /* "infinite" deadlines */
    int64_t v, r;
    if (read_virtual(CLOCK_REALTIME, &v, &r) != 1) return 0;
    if (v - r < 1000000 && r - v < 1000000) return 0; /* shift < 1 ms: leave as-is */
    int64_t remaining = ts_ns(in) - v;
    ns_ts(r + remaining, out);
    return 1;
}

/* ------------------------------------------------------------------------ */
/* Wall-clock reads                                                          */
/* ------------------------------------------------------------------------ */

EXPORT int clock_gettime(clockid_t clk, struct timespec *ts) {
    if (!is_wall(clk) || !ts) return raw_clock_gettime(clk, ts);
    int64_t v;
    if (read_virtual(clk, &v, NULL) != 1) return raw_clock_gettime(clk, ts);
    ns_ts(v, ts);
    return 0;
}

typedef int (*gettimeofday_fn)(struct timeval *, void *);
static gettimeofday_fn real_gettimeofday_p;

EXPORT int gettimeofday(struct timeval *tv, void *tz) {
    REAL(real_gettimeofday_p, "gettimeofday", NULL);
    if (tz && real_gettimeofday_p) {
        struct timeval dummy;
        real_gettimeofday_p(&dummy, tz);
    }
    if (!tv) return 0;
    struct timespec ts;
    if (clock_gettime(CLOCK_REALTIME, &ts) != 0) return -1;
    tv->tv_sec = ts.tv_sec;
    tv->tv_usec = ts.tv_nsec / 1000;
    return 0;
}

EXPORT time_t time(time_t *t) {
    struct timespec ts;
    if (clock_gettime(CLOCK_REALTIME, &ts) != 0) return (time_t)-1;
    if (t) *t = ts.tv_sec;
    return ts.tv_sec;
}

EXPORT int timespec_get(struct timespec *ts, int base) {
    if (base != TIME_UTC) return 0;
    if (clock_gettime(CLOCK_REALTIME, ts) != 0) return 0;
    return base;
}

/* ------------------------------------------------------------------------ */
/* Absolute-deadline APIs                                                    */
/* ------------------------------------------------------------------------ */

typedef int (*clock_nanosleep_fn)(clockid_t, int, const struct timespec *, struct timespec *);
static clock_nanosleep_fn real_clock_nanosleep_p;
EXPORT int clock_nanosleep(clockid_t clk, int flags, const struct timespec *req, struct timespec *rem) {
    REAL(real_clock_nanosleep_p, "clock_nanosleep", NULL);
    struct timespec adj;
    if ((flags & TIMER_ABSTIME) && is_wall(clk) && to_real_deadline(req, &adj))
        req = &adj;
    return real_clock_nanosleep_p(clk, flags, req, rem);
}

typedef int (*sem_timedwait_fn)(sem_t *, const struct timespec *);
static sem_timedwait_fn real_sem_timedwait_p;
EXPORT int sem_timedwait(sem_t *s, const struct timespec *abs) {
    REAL(real_sem_timedwait_p, "sem_timedwait", NULL);
    struct timespec adj;
    if (to_real_deadline(abs, &adj)) abs = &adj;
    return real_sem_timedwait_p(s, abs);
}

/* The *_clockwait / *_clocklock family exists only in glibc >= 2.30. musl does
 * not provide it (verified in M0), and exporting stubs there would make
 * feature detection via dlsym/weak symbols wrongly succeed. */
#ifdef __GLIBC__
typedef int (*sem_clockwait_fn)(sem_t *, clockid_t, const struct timespec *);
static sem_clockwait_fn real_sem_clockwait_p;
EXPORT int sem_clockwait(sem_t *s, clockid_t clk, const struct timespec *abs) {
    REAL(real_sem_clockwait_p, "sem_clockwait", NULL);
    if (!real_sem_clockwait_p) { errno = ENOSYS; return -1; }
    struct timespec adj;
    if (is_wall(clk) && to_real_deadline(abs, &adj)) abs = &adj;
    return real_sem_clockwait_p(s, clk, abs);
}
#endif /* __GLIBC__ */

typedef int (*mutex_timedlock_fn)(pthread_mutex_t *, const struct timespec *);
static mutex_timedlock_fn real_mutex_timedlock_p;
EXPORT int pthread_mutex_timedlock(pthread_mutex_t *m, const struct timespec *abs) {
    REAL(real_mutex_timedlock_p, "pthread_mutex_timedlock", NULL);
    struct timespec adj;
    if (to_real_deadline(abs, &adj)) abs = &adj;
    return real_mutex_timedlock_p(m, abs);
}

#ifdef __GLIBC__
typedef int (*mutex_clocklock_fn)(pthread_mutex_t *, clockid_t, const struct timespec *);
static mutex_clocklock_fn real_mutex_clocklock_p;
EXPORT int pthread_mutex_clocklock(pthread_mutex_t *m, clockid_t clk, const struct timespec *abs) {
    REAL(real_mutex_clocklock_p, "pthread_mutex_clocklock", NULL);
    if (!real_mutex_clocklock_p) return ENOSYS;
    struct timespec adj;
    if (is_wall(clk) && to_real_deadline(abs, &adj)) abs = &adj;
    return real_mutex_clocklock_p(m, clk, abs);
}
#endif /* __GLIBC__ */

typedef int (*rwlock_timed_fn)(pthread_rwlock_t *, const struct timespec *);
static rwlock_timed_fn real_rw_timedrd_p, real_rw_timedwr_p;
EXPORT int pthread_rwlock_timedrdlock(pthread_rwlock_t *l, const struct timespec *abs) {
    REAL(real_rw_timedrd_p, "pthread_rwlock_timedrdlock", NULL);
    struct timespec adj;
    if (to_real_deadline(abs, &adj)) abs = &adj;
    return real_rw_timedrd_p(l, abs);
}
EXPORT int pthread_rwlock_timedwrlock(pthread_rwlock_t *l, const struct timespec *abs) {
    REAL(real_rw_timedwr_p, "pthread_rwlock_timedwrlock", NULL);
    struct timespec adj;
    if (to_real_deadline(abs, &adj)) abs = &adj;
    return real_rw_timedwr_p(l, abs);
}

#ifdef __GLIBC__
typedef int (*rwlock_clock_fn)(pthread_rwlock_t *, clockid_t, const struct timespec *);
static rwlock_clock_fn real_rw_clockrd_p, real_rw_clockwr_p;
EXPORT int pthread_rwlock_clockrdlock(pthread_rwlock_t *l, clockid_t clk, const struct timespec *abs) {
    REAL(real_rw_clockrd_p, "pthread_rwlock_clockrdlock", NULL);
    if (!real_rw_clockrd_p) return ENOSYS;
    struct timespec adj;
    if (is_wall(clk) && to_real_deadline(abs, &adj)) abs = &adj;
    return real_rw_clockrd_p(l, clk, abs);
}
EXPORT int pthread_rwlock_clockwrlock(pthread_rwlock_t *l, clockid_t clk, const struct timespec *abs) {
    REAL(real_rw_clockwr_p, "pthread_rwlock_clockwrlock", NULL);
    if (!real_rw_clockwr_p) return ENOSYS;
    struct timespec adj;
    if (is_wall(clk) && to_real_deadline(abs, &adj)) abs = &adj;
    return real_rw_clockwr_p(l, clk, abs);
}
#endif /* __GLIBC__ */

/* --- Condition variables: track which condvars use a non-wall clock. ---- */

#define COND_TABLE_SIZE 16384 /* power of two */
#define TOMBSTONE ((void *)1)
static void *g_mono_conds[COND_TABLE_SIZE];
static int g_cond_table_overflow;

static inline size_t cond_hash(const void *p) {
    uintptr_t x = (uintptr_t)p;
    x ^= x >> 17; x *= 0xed5ad4bbU; x ^= x >> 11;
    return (size_t)x & (COND_TABLE_SIZE - 1);
}
static void mono_cond_add(void *c) {
    size_t h = cond_hash(c);
    for (size_t i = 0; i < COND_TABLE_SIZE; i++) {
        void **slot = &g_mono_conds[(h + i) & (COND_TABLE_SIZE - 1)];
        void *cur = __atomic_load_n(slot, __ATOMIC_ACQUIRE);
        if (cur == c) return;
        if (cur == NULL || cur == TOMBSTONE) {
            if (__atomic_compare_exchange_n(slot, &cur, c, 0, __ATOMIC_ACQ_REL, __ATOMIC_ACQUIRE)) return;
        }
    }
    __atomic_store_n(&g_cond_table_overflow, 1, __ATOMIC_RELEASE);
}
static int mono_cond_has(const void *c) {
    size_t h = cond_hash(c);
    for (size_t i = 0; i < COND_TABLE_SIZE; i++) {
        void *cur = __atomic_load_n(&g_mono_conds[(h + i) & (COND_TABLE_SIZE - 1)], __ATOMIC_ACQUIRE);
        if (cur == c) return 1;
        if (cur == NULL) return 0;
    }
    return 0;
}
static void mono_cond_del(const void *c) {
    size_t h = cond_hash(c);
    for (size_t i = 0; i < COND_TABLE_SIZE; i++) {
        void **slot = &g_mono_conds[(h + i) & (COND_TABLE_SIZE - 1)];
        void *cur = __atomic_load_n(slot, __ATOMIC_ACQUIRE);
        if (cur == c) { __atomic_store_n(slot, TOMBSTONE, __ATOMIC_RELEASE); return; }
        if (cur == NULL) return;
    }
}

typedef int (*cond_init_fn)(pthread_cond_t *, const pthread_condattr_t *);
typedef int (*cond_destroy_fn)(pthread_cond_t *);
typedef int (*cond_timedwait_fn)(pthread_cond_t *, pthread_mutex_t *, const struct timespec *);
typedef int (*cond_clockwait_fn)(pthread_cond_t *, pthread_mutex_t *, clockid_t, const struct timespec *);
static cond_init_fn real_cond_init_p;
static cond_destroy_fn real_cond_destroy_p;
static cond_timedwait_fn real_cond_timedwait_p;
#ifdef __GLIBC__
static cond_clockwait_fn real_cond_clockwait_p;
#endif

/* glibc keeps a pre-2.3.2 compat version of the condvar API; always bind the
 * modern one. (On aarch64 the baseline is GLIBC_2.17; dlvsym fails and we fall
 * back to the default version, which is the modern one.) */
#define COND_VER "GLIBC_2.3.2"

EXPORT int pthread_cond_init(pthread_cond_t *c, const pthread_condattr_t *attr) {
    REAL(real_cond_init_p, "pthread_cond_init", COND_VER);
    int rc = real_cond_init_p(c, attr);
    if (rc == 0) {
        clockid_t clk = CLOCK_REALTIME;
        if (attr) pthread_condattr_getclock(attr, &clk);
        if (!is_wall(clk)) mono_cond_add(c);
        else mono_cond_del(c); /* address reuse */
    }
    return rc;
}

EXPORT int pthread_cond_destroy(pthread_cond_t *c) {
    REAL(real_cond_destroy_p, "pthread_cond_destroy", COND_VER);
    mono_cond_del(c);
    return real_cond_destroy_p(c);
}

EXPORT int pthread_cond_timedwait(pthread_cond_t *c, pthread_mutex_t *m, const struct timespec *abs) {
    REAL(real_cond_timedwait_p, "pthread_cond_timedwait", COND_VER);
    struct timespec adj;
    if (!__atomic_load_n(&g_cond_table_overflow, __ATOMIC_ACQUIRE) && !mono_cond_has(c) &&
        to_real_deadline(abs, &adj))
        abs = &adj;
    return real_cond_timedwait_p(c, m, abs);
}

#ifdef __GLIBC__
EXPORT int pthread_cond_clockwait(pthread_cond_t *c, pthread_mutex_t *m, clockid_t clk, const struct timespec *abs) {
    REAL(real_cond_clockwait_p, "pthread_cond_clockwait", NULL);
    if (!real_cond_clockwait_p) return ENOSYS;
    struct timespec adj;
    if (is_wall(clk) && to_real_deadline(abs, &adj)) abs = &adj;
    return real_cond_clockwait_p(c, m, clk, abs);
}
#endif /* __GLIBC__ */

/* --- timerfd: remember which fds use a wall clock. ---------------------- */

#define FD_TRACK_MAX 65536
static unsigned char g_wall_timerfd[FD_TRACK_MAX / 8];

typedef int (*timerfd_create_fn)(clockid_t, int);
typedef int (*timerfd_settime_fn)(int, int, const struct itimerspec *, struct itimerspec *);
static timerfd_create_fn real_timerfd_create_p;
static timerfd_settime_fn real_timerfd_settime_p;

EXPORT int timerfd_create(clockid_t clk, int flags) {
    REAL(real_timerfd_create_p, "timerfd_create", NULL);
    int fd = real_timerfd_create_p(clk, flags);
    if (fd >= 0 && fd < FD_TRACK_MAX) {
        unsigned char bit = (unsigned char)(1u << (fd & 7));
        if (is_wall(clk)) __atomic_fetch_or(&g_wall_timerfd[fd >> 3], bit, __ATOMIC_ACQ_REL);
        else __atomic_fetch_and(&g_wall_timerfd[fd >> 3], (unsigned char)~bit, __ATOMIC_ACQ_REL);
    }
    return fd;
}

EXPORT int timerfd_settime(int fd, int flags, const struct itimerspec *nv, struct itimerspec *ov) {
    REAL(real_timerfd_settime_p, "timerfd_settime", NULL);
    struct itimerspec adj;
    if ((flags & TFD_TIMER_ABSTIME) && nv && fd >= 0 && fd < FD_TRACK_MAX &&
        (__atomic_load_n(&g_wall_timerfd[fd >> 3], __ATOMIC_ACQUIRE) & (1u << (fd & 7))) &&
        (nv->it_value.tv_sec || nv->it_value.tv_nsec)) {
        adj = *nv;
        if (to_real_deadline(&nv->it_value, &adj.it_value)) nv = &adj;
    }
    return real_timerfd_settime_p(fd, flags, nv, ov);
}
