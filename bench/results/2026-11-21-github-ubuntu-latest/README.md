# Benchmark results: github-ubuntu-latest

| | |
|---|---|
| Date (UTC) | 2026-11-21T03:09:19Z |
| Host | Linux-7.0.0-1020-azure-x86_64-with-glibc2.43 (x86_64) |
| Docker | 29.4.2 (linux/amd64) · context `default` |
| Linux (containers) | 7.0.0-1020-azure · 4 CPUs · INTEL(R) XEON(R) PLATINUM 8573C |
| ChronoStack | chronostack 0.1.0-alpha.7 · commit `45043c2fe593` |

## B3: `chronostack users run` (simulated users)

200 users, 20 GET requests each, against traefik/whoami on 127.0.0.1. Median of 10 runs; p10–p90 in brackets. Requests per second = all requests / the run's real time, including thread start-up. Latency is the engine's own per-request measurement (p50 and p99 of each run). This measures the engine on this machine, against a server that does almost nothing; it is not a load-testing result.

| max_parallel | Requests/s | Latency p50 (ms) | Latency p99 (ms) |
|---|---|---|---|
| 32 | 24,026 [23,108–24,860] | 0.8 [0.8–0.8] | 7.4 [6.9–7.7] |
| 200 | 20,409 [17,691–21,541] | 1.1 [1.0–1.2] | 39.4 [34.0–45.9] |

Raw samples: `b3-users.json` in this folder.
