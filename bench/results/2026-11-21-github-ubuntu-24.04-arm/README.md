# Benchmark results: github-ubuntu-24.04-arm

| | |
|---|---|
| Date (UTC) | 2026-11-21T03:09:30Z |
| Host | Linux-7.0.0-1017-azure-aarch64-with-glibc2.39 (aarch64) |
| Docker | 28.0.4 (linux/arm64) · context `default` |
| Linux (containers) | 7.0.0-1017-azure · 4 CPUs · 0xd49 |
| ChronoStack | chronostack 0.1.0-alpha.7 · commit `45043c2fe593` |

## B3: `chronostack users run` (simulated users)

200 users, 20 GET requests each, against traefik/whoami on 127.0.0.1. Median of 10 runs; p10–p90 in brackets. Requests per second = all requests / the run's real time, including thread start-up. Latency is the engine's own per-request measurement (p50 and p99 of each run). This measures the engine on this machine, against a server that does almost nothing; it is not a load-testing result.

| max_parallel | Requests/s | Latency p50 (ms) | Latency p99 (ms) |
|---|---|---|---|
| 32 | 10,667 [10,257–11,062] | 1.5 [1.5–1.6] | 14.3 [13.5–15.4] |
| 200 | 9,281 [8,867–9,892] | 2.7 [2.5–3.0] | 66.6 [60.5–79.7] |

Raw samples: `b3-users.json` in this folder.
