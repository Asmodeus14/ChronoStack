# Benchmark results: windows11-wsl2-ryzen7840hs

| | |
|---|---|
| Date (UTC) | 2026-11-21T03:02:36Z |
| Host | Windows-11-10.0.26300-SP0 (AMD64) |
| Docker | 29.6.2 (linux/amd64) · context `desktop-linux` |
| Linux (containers) | ? · ? CPUs · ? |
| ChronoStack | chronostack 0.1.0-alpha.7 · commit `407d37d7a958` |

## B3: `chronostack users run` (simulated users)

200 users, 20 GET requests each, against traefik/whoami on 127.0.0.1. Median of 10 runs; p10–p90 in brackets. Requests per second = all requests / the run's real time, including thread start-up. Latency is the engine's own per-request measurement (p50 and p99 of each run). This measures the engine on this machine, against a server that does almost nothing; it is not a load-testing result.

| max_parallel | Requests/s | Latency p50 (ms) | Latency p99 (ms) |
|---|---|---|---|
| 32 | 8,048 [7,796–8,251] | 3.5 [3.5–3.6] | 10.0 [9.8–11.0] |
| 200 | 7,715 [7,603–7,923] | 21.4 [21.1–22.2] | 72.7 [68.0–79.7] |

Raw samples: `b3-users.json` in this folder.
