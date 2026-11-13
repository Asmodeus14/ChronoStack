/* SPDX-License-Identifier: MIT OR Apache-2.0 */
/*
 * cstest — M0 C-level probes.
 *   cstest now       JSON: every wall-clock API + raw syscall + monotonic
 *   cstest nohang    absolute-deadline APIs with deadline = virtual now + 1s
 *   cstest stress S  4 reader threads for S seconds, check virtual time never goes back
 *   cstest bench N   ns per clock_gettime(CLOCK_REALTIME), N calls
 *
 * nohang output: one JSON line per test: {"test":..., "elapsed_ms":..., "status":...}
 * status: OK (0.9s..2.5s), EARLY (<0.9s), SLOW (>2.5s), HANG (killed after 6s), SKIP
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <poll.h>
#include <pthread.h>
#include <semaphore.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/syscall.h>
#include <sys/time.h>
#include <sys/timerfd.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define NS 1000000000LL

static int64_t now_ns(clockid_t c) {
    struct timespec t;
    clock_gettime(c, &t);
    return (int64_t)t.tv_sec * NS + t.tv_nsec;
}
static struct timespec deadline_in(clockid_t c, int64_t d) {
    int64_t n = now_ns(c) + d;
    struct timespec t = {(time_t)(n / NS), (long)(n % NS)};
    return t;
}

/* ---- now --------------------------------------------------------------- */
static int cmd_now(void) {
    struct timeval tv;
    gettimeofday(&tv, NULL);
    struct timespec ts, raw;
    int (*tsg)(struct timespec *, int) = (int (*)(struct timespec *, int))dlsym(RTLD_DEFAULT, "timespec_get");
    long long tsget = -1;
    if (tsg && tsg(&ts, 1) == 1) tsget = ts.tv_sec;
    syscall(SYS_clock_gettime, CLOCK_REALTIME, &raw);
    printf("{\"clock_gettime_realtime\":%.3f,\"clock_gettime_coarse\":%.3f,\"clock_gettime_tai\":%.3f,"
           "\"gettimeofday\":%.3f,\"time\":%lld,\"timespec_get\":%lld,\"raw_syscall\":%.3f,"
           "\"monotonic\":%.3f}\n",
           now_ns(CLOCK_REALTIME) / 1e9, now_ns(CLOCK_REALTIME_COARSE) / 1e9, now_ns(CLOCK_TAI) / 1e9,
           tv.tv_sec + tv.tv_usec / 1e6, (long long)time(NULL), tsget,
           raw.tv_sec + raw.tv_nsec / 1e9, now_ns(CLOCK_MONOTONIC) / 1e9);
    return 0;
}

/* ---- nohang ------------------------------------------------------------ */
static pthread_mutex_t held_mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_rwlock_t held_rw = PTHREAD_RWLOCK_INITIALIZER;
static void *hold_mutex(void *a) { (void)a; pthread_mutex_lock(&held_mutex); sleep(10); return NULL; }
static void *hold_rw(void *a) { (void)a; pthread_rwlock_wrlock(&held_rw); sleep(10); return NULL; }

static void t_sem_timedwait(void) {
    sem_t s; sem_init(&s, 0, 0);
    struct timespec d = deadline_in(CLOCK_REALTIME, NS);
    while (sem_timedwait(&s, &d) == -1 && errno == EINTR) {}
}
static void t_sem_clockwait(void) {
    int (*f)(sem_t *, clockid_t, const struct timespec *) =
        (int (*)(sem_t *, clockid_t, const struct timespec *))dlsym(RTLD_DEFAULT, "sem_clockwait");
    if (!f) { printf("SKIPPED\n"); _exit(3); }
    sem_t s; sem_init(&s, 0, 0);
    struct timespec d = deadline_in(CLOCK_REALTIME, NS);
    f(&s, CLOCK_REALTIME, &d);
}
static void t_mutex_timedlock(void) {
    pthread_t th; pthread_create(&th, NULL, hold_mutex, NULL); usleep(100000);
    struct timespec d = deadline_in(CLOCK_REALTIME, NS);
    pthread_mutex_timedlock(&held_mutex, &d);
}
static void t_rwlock_timedwrlock(void) {
    pthread_t th; pthread_create(&th, NULL, hold_rw, NULL); usleep(100000);
    struct timespec d = deadline_in(CLOCK_REALTIME, NS);
    pthread_rwlock_timedwrlock(&held_rw, &d);
}
static void t_cond_default(void) {
    pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER; pthread_cond_t c;
    pthread_cond_init(&c, NULL);
    pthread_mutex_lock(&m);
    struct timespec d = deadline_in(CLOCK_REALTIME, NS);
    while (pthread_cond_timedwait(&c, &m, &d) != ETIMEDOUT) {}
}
static void t_cond_static(void) {
    pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER; pthread_cond_t c = PTHREAD_COND_INITIALIZER;
    pthread_mutex_lock(&m);
    struct timespec d = deadline_in(CLOCK_REALTIME, NS);
    while (pthread_cond_timedwait(&c, &m, &d) != ETIMEDOUT) {}
}
static void t_cond_monotonic(void) {
    pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER; pthread_cond_t c; pthread_condattr_t a;
    pthread_condattr_init(&a); pthread_condattr_setclock(&a, CLOCK_MONOTONIC);
    pthread_cond_init(&c, &a);
    pthread_mutex_lock(&m);
    struct timespec d = deadline_in(CLOCK_MONOTONIC, NS);
    while (pthread_cond_timedwait(&c, &m, &d) != ETIMEDOUT) {}
}
static void t_cond_clockwait(void) {
    int (*f)(pthread_cond_t *, pthread_mutex_t *, clockid_t, const struct timespec *) =
        (int (*)(pthread_cond_t *, pthread_mutex_t *, clockid_t, const struct timespec *))
            dlsym(RTLD_DEFAULT, "pthread_cond_clockwait");
    if (!f) { printf("SKIPPED\n"); _exit(3); }
    pthread_mutex_t m = PTHREAD_MUTEX_INITIALIZER; pthread_cond_t c = PTHREAD_COND_INITIALIZER;
    pthread_mutex_lock(&m);
    struct timespec d = deadline_in(CLOCK_REALTIME, NS);
    while (f(&c, &m, CLOCK_REALTIME, &d) != ETIMEDOUT) {}
}
static void t_clock_nanosleep_abs(void) {
    struct timespec d = deadline_in(CLOCK_REALTIME, NS);
    while (clock_nanosleep(CLOCK_REALTIME, TIMER_ABSTIME, &d, NULL) == EINTR) {}
}
static void t_timerfd_abs(void) {
    int fd = timerfd_create(CLOCK_REALTIME, 0);
    struct itimerspec its; memset(&its, 0, sizeof(its));
    its.it_value = deadline_in(CLOCK_REALTIME, NS);
    timerfd_settime(fd, TFD_TIMER_ABSTIME, &its, NULL);
    uint64_t x; ssize_t r = read(fd, &x, sizeof(x)); (void)r;
}
static void t_nanosleep_rel(void) { struct timespec d = {1, 0}; nanosleep(&d, NULL); }
static void t_poll_rel(void) { poll(NULL, 0, 1000); }

struct test { const char *name; void (*fn)(void); };
static const struct test TESTS[] = {
    {"sem_timedwait", t_sem_timedwait},
    {"sem_clockwait_realtime", t_sem_clockwait},
    {"pthread_mutex_timedlock", t_mutex_timedlock},
    {"pthread_rwlock_timedwrlock", t_rwlock_timedwrlock},
    {"pthread_cond_timedwait_default", t_cond_default},
    {"pthread_cond_timedwait_static_init", t_cond_static},
    {"pthread_cond_timedwait_monotonic_attr", t_cond_monotonic},
    {"pthread_cond_clockwait_realtime", t_cond_clockwait},
    {"clock_nanosleep_realtime_abstime", t_clock_nanosleep_abs},
    {"timerfd_realtime_abstime", t_timerfd_abs},
    {"nanosleep_relative", t_nanosleep_rel},
    {"poll_relative", t_poll_rel},
};

static int cmd_nohang(void) {
    for (size_t i = 0; i < sizeof(TESTS) / sizeof(TESTS[0]); i++) {
        int pipefd[2]; if (pipe(pipefd) != 0) return 1;
        int64_t t0 = now_ns(CLOCK_MONOTONIC);
        pid_t pid = fork();
        if (pid == 0) {
            close(pipefd[0]); dup2(pipefd[1], 1);
            alarm(6);
            TESTS[i].fn();
            _exit(0);
        }
        close(pipefd[1]);
        int st = 0; waitpid(pid, &st, 0);
        double ms = (now_ns(CLOCK_MONOTONIC) - t0) / 1e6;
        char buf[64] = {0}; ssize_t r = read(pipefd[0], buf, sizeof(buf) - 1); (void)r; close(pipefd[0]);
        const char *status;
        if (WIFEXITED(st) && WEXITSTATUS(st) == 3) status = "SKIP";
        else if (WIFSIGNALED(st) && WTERMSIG(st) == SIGALRM) status = "HANG";
        else if (WIFSIGNALED(st)) status = "CRASH";
        else if (ms < 900) status = "EARLY";
        else if (ms > 2500) status = "SLOW";
        else status = "OK";
        printf("{\"test\":\"%s\",\"elapsed_ms\":%.0f,\"status\":\"%s\"}\n", TESTS[i].name, ms, status);
        fflush(stdout);
    }
    return 0;
}

/* ---- stress ------------------------------------------------------------ */
static volatile int stop_flag;
struct sres { unsigned long reads, backwards, changes; int64_t worst_back_ns; };
static void *reader(void *arg) {
    struct sres *r = (struct sres *)arg;
    int64_t prev = now_ns(CLOCK_REALTIME);
    while (!stop_flag) {
        int64_t v = now_ns(CLOCK_REALTIME);
        r->reads++;
        if (v < prev) { r->backwards++; if (prev - v > r->worst_back_ns) r->worst_back_ns = prev - v; }
        else if (v != prev) r->changes++;
        prev = v;
    }
    return NULL;
}
static int cmd_stress(double secs) {
    enum { N = 4 };
    pthread_t th[N]; struct sres res[N]; memset(res, 0, sizeof(res));
    for (int i = 0; i < N; i++) pthread_create(&th[i], NULL, reader, &res[i]);
    struct timespec d = {(time_t)secs, (long)((secs - (time_t)secs) * 1e9)};
    nanosleep(&d, NULL);
    stop_flag = 1;
    unsigned long reads = 0, back = 0; int64_t worst = 0;
    for (int i = 0; i < N; i++) {
        pthread_join(th[i], NULL);
        reads += res[i].reads; back += res[i].backwards;
        if (res[i].worst_back_ns > worst) worst = res[i].worst_back_ns;
    }
    printf("{\"threads\":%d,\"reads\":%lu,\"backwards\":%lu,\"worst_back_ns\":%lld}\n", N, reads, back, (long long)worst);
    return 0;
}

/* ---- bench ------------------------------------------------------------- */
static int cmd_bench(long n) {
    struct timespec t;
    for (long i = 0; i < 100000; i++) clock_gettime(CLOCK_REALTIME, &t);
    int64_t t0 = now_ns(CLOCK_MONOTONIC);
    for (long i = 0; i < n; i++) clock_gettime(CLOCK_REALTIME, &t);
    int64_t t1 = now_ns(CLOCK_MONOTONIC);
    printf("{\"calls\":%ld,\"ns_per_call\":%.2f}\n", n, (double)(t1 - t0) / n);
    return 0;
}

int main(int argc, char **argv) {
    if (argc < 2) { fprintf(stderr, "usage: cstest now|nohang|stress S|bench N\n"); return 2; }
    if (!strcmp(argv[1], "now")) return cmd_now();
    if (!strcmp(argv[1], "nohang")) return cmd_nohang();
    if (!strcmp(argv[1], "stress")) return cmd_stress(argc > 2 ? atof(argv[2]) : 5);
    if (!strcmp(argv[1], "bench")) return cmd_bench(argc > 2 ? atol(argv[2]) : 20000000L);
    return 2;
}
