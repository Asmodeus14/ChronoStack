# Benchmark results: windows11-wsl2-ryzen7840hs

| | |
|---|---|
| Date (UTC) | 2026-11-21T03:06:22Z |
| Host | Windows-11-10.0.26300-SP0 (AMD64) |
| Docker | 29.6.2 (linux/amd64) · context `desktop-linux` |
| Linux (containers) | 6.6.87.2-microsoft-standard-WSL2 · 16 CPUs · AMD Ryzen 7 7840HS w/ Radeon 780M Graphics |
| ChronoStack | chronostack 0.1.0-alpha.7 · commit `f35890b2503a` |

## B3: `chronostack users run` (simulated users)

200 users, 20 GET requests each, against traefik/whoami on 127.0.0.1. Median of 10 runs; p10–p90 in brackets. Requests per second = all requests / the run's real time, including thread start-up. Latency is the engine's own per-request measurement (p50 and p99 of each run). This measures the engine on this machine, against a server that does almost nothing; it is not a load-testing result.

| max_parallel | Requests/s | Latency p50 (ms) | Latency p99 (ms) |
|---|---|---|---|
| 32 | 8,016 [7,764–8,185] | 3.6 [3.5–3.7] | 10.0 [9.9–11.2] |
| 200 | 7,491 [7,376–7,880] | 21.8 [21.1–22.1] | 77.8 [66.0–84.8] |

Raw samples: `b3-users.json` in this folder.
