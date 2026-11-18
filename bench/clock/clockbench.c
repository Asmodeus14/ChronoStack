// SPDX-License-Identifier: MIT OR Apache-2.0
// B1: cost of one clock_gettime(CLOCK_REALTIME) call.
//
//   clockbench <iterations>
//
// The loop is timed with CLOCK_MONOTONIC_RAW through the raw syscall, so no
// preload library (ChronoStack's or libfaketime) can change the measurement
// itself. Prints one JSON line: ns per call, plus the wall-clock value that was
// read so the harness can confirm which clock was in effect.
#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <sys/syscall.h>
#include <time.h>
#include <unistd.h>

static long long raw_ns(void) {
    struct timespec t;
    syscall(SYS_clock_gettime, CLOCK_MONOTONIC_RAW, &t);
    return (long long)t.tv_sec * 1000000000LL + t.tv_nsec;
}

int main(int argc, char **argv) {
    long n = argc > 1 ? atol(argv[1]) : 10000000L;
    struct timespec ts;
    volatile long sink = 0;
    for (long i = 0; i < n / 10; i++) { // warm-up
        clock_gettime(CLOCK_REALTIME, &ts);
        sink += ts.tv_nsec;
    }
    long long t0 = raw_ns();
    for (long i = 0; i < n; i++) {
        clock_gettime(CLOCK_REALTIME, &ts);
        sink += ts.tv_nsec;
    }
    long long t1 = raw_ns();
    printf("{\"ns_per_call\":%.3f,\"iterations\":%ld,\"wall_clock\":%.3f}\n",
           (double)(t1 - t0) / (double)n, n, ts.tv_sec + ts.tv_nsec / 1e9);
    return sink == 42 ? 1 : 0;
}
