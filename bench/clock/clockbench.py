# SPDX-License-Identifier: MIT OR Apache-2.0
"""B1: cost of one time.time() call in CPython.

    python clockbench.py <iterations>

Times the loop with time.perf_counter_ns (CLOCK_MONOTONIC; a preload that only
adds a constant offset doesn't change differences) and subtracts an empty loop
of the same length, so the result is the call itself. Prints one JSON line.
"""
import json
import sys
import time


def main():
    n = int(sys.argv[1]) if len(sys.argv) > 1 else 2_000_000
    now = time.time
    clock = time.perf_counter_ns
    for _ in range(n // 10):
        now()
    t0 = clock()
    for _ in range(n):
        pass
    empty = clock() - t0
    t0 = clock()
    for _ in range(n):
        now()
    full = clock() - t0
    print(json.dumps({
        "ns_per_call": (full - empty) / n,
        "ns_per_iteration": full / n,
        "iterations": n,
        "wall_clock": now(),
    }))


if __name__ == "__main__":
    main()
