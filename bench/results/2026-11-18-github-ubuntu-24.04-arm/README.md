# Benchmark results: github-ubuntu-24.04-arm

| | |
|---|---|
| Date (UTC) | 2026-11-18T13:39:25Z |
| Host | Linux-7.0.0-1017-azure-aarch64-with-glibc2.39 (aarch64) |
| Docker | 28.0.4 (linux/arm64) · context `default` |
| Linux (containers) | 7.0.0-1017-azure · 4 CPUs · 0xd49 |
| ChronoStack | chronostack 0.1.0-alpha.6 · commit `eaeed0c891b2` |
| libfaketime | Debian package 0.9.10-2.1 |

## B1: clock-read overhead (ns per read)

Median of 10 runs; p10–p90 in brackets. Overhead = median minus the native median in the same container. Every run's clock reading was checked against the expected offset (all passed).

| Runtime | Native | ChronoStack | libfaketime (cached) | libfaketime (file, no cache) |
|---|---|---|---|---|
| c | 30.9 [30.9–30.9] | 44.4 [44.0–44.4] (+13.5) | 71.8 [71.7–72.3] (+41.0) | 3,765 [3,760–3,777] (+3,735) |
| python | 64.5 [64.5–64.6] | 83.3 [83.3–83.5] (+18.8) | 110.9 [110.4–112.0] (+46.4) | 3,916 [3,895–3,944] (+3,851) |
| node | 50.4 [50.3–51.1] | 71.9 [71.6–72.1] (+21.5) | 96.2 [95.9–96.8] (+45.8) | 4,013 [3,990–4,071] (+3,963) |

Python's figure is the time.time() call with an empty loop of the same length subtracted; Node's includes the loop.

## B2: `chronostack advance 1h` (ms of real time)

Median of 10 runs; p10–p90 in brackets. Idle `debian:bookworm-slim` services, no triggers or barriers. With probes, every service's clock is checked after the move.

| Services | With probes | Without probes (`--no-probe`) |
|---|---|---|
| 1 | 153.4 [149.6–159.7] | 3.2 [3.1–3.3] |
| 5 | 227.1 [220.9–240.5] | 3.7 [3.5–3.8] |
| 20 | 635.9 [622.6–642.0] | 3.2 [3.1–3.3] |

Raw samples: `b1-clock.json`, `b2-advance.json` in this folder.
