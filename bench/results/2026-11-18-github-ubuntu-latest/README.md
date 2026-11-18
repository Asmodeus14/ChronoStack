# Benchmark results: github-ubuntu-latest

| | |
|---|---|
| Date (UTC) | 2026-11-18T13:40:16Z |
| Host | Linux-7.0.0-1019-azure-x86_64-with-glibc2.43 (x86_64) |
| Docker | 29.4.2 (linux/amd64) · context `default` |
| Linux (containers) | 7.0.0-1019-azure · 4 CPUs · INTEL(R) XEON(R) PLATINUM 8573C |
| ChronoStack | chronostack 0.1.0-alpha.6 · commit `eaeed0c891b2` |
| libfaketime | Debian package 0.9.10-2.1 |

## B1: clock-read overhead (ns per read)

Median of 10 runs; p10–p90 in brackets. Overhead = median minus the native median in the same container. Every run's clock reading was checked against the expected offset (all passed).

| Runtime | Native | ChronoStack | libfaketime (cached) | libfaketime (file, no cache) |
|---|---|---|---|---|
| c | 21.8 [21.8–21.9] | 33.7 [33.7–34.1] (+11.9) | 63.5 [63.2–64.4] (+41.7) | 3,695 [3,671–3,750] (+3,673) |
| python | 52.0 [50.8–54.5] | 62.1 [61.4–64.8] (+10.1) | 91.7 [91.5–92.1] (+39.7) | 3,795 [3,755–3,838] (+3,743) |
| node | 46.1 [45.9–46.9] | 57.9 [57.7–58.3] (+11.8) | 89.8 [89.5–91.6] (+43.7) | 3,863 [3,843–3,900] (+3,817) |

Python's figure is the time.time() call with an empty loop of the same length subtracted; Node's includes the loop.

## B2: `chronostack advance 1h` (ms of real time)

Median of 10 runs; p10–p90 in brackets. Idle `debian:bookworm-slim` services, no triggers or barriers. With probes, every service's clock is checked after the move.

| Services | With probes | Without probes (`--no-probe`) |
|---|---|---|
| 1 | 170.7 [168.5–171.6] | 2.7 [2.6–2.8] |
| 5 | 282.1 [269.6–287.5] | 2.6 [2.5–2.9] |
| 20 | 874.0 [857.1–878.6] | 2.6 [2.6–2.7] |

Raw samples: `b1-clock.json`, `b2-advance.json` in this folder.
