# Benchmark results: windows11-wsl2-ryzen7840hs

| | |
|---|---|
| Date (UTC) | 2026-11-18T13:06:32Z |
| Host | Windows-11-10.0.26300-SP0 (AMD64) |
| Docker | 29.6.2 (linux/amd64) · context `desktop-linux` |
| Linux (containers) | 6.6.87.2-microsoft-standard-WSL2 · 16 CPUs · AMD Ryzen 7 7840HS w/ Radeon 780M Graphics |
| ChronoStack | chronostack 0.1.0-alpha.6 · commit `f13e90110422` |
| libfaketime | Debian package 0.9.10-2.1 |

## B1: clock-read overhead (ns per read)

Median of 10 runs; p10–p90 in brackets. Overhead = median minus the native median in the same container. Every run's clock reading was checked against the expected offset (all passed).

| Runtime | Native | ChronoStack | libfaketime (cached) | libfaketime (file, no cache) |
|---|---|---|---|---|
| c | 38.8 [38.4–39.4] | 54.5 [53.2–56.5] (+15.7) | 99.2 [97.1–101.2] (+60.4) | 14,156 [14,010–14,637] (+14,117) |
| python | 78.5 [76.4–87.2] | 104.7 [99.9–117.4] (+26.2) | 149.0 [139.0–152.2] (+70.5) | 14,405 [14,167–14,975] (+14,326) |
| node | 70.4 [69.2–74.2] | 90.7 [86.0–92.1] (+20.2) | 138.3 [134.7–146.4] (+67.8) | 14,665 [14,366–15,042] (+14,594) |

Python's figure is the time.time() call with an empty loop of the same length subtracted; Node's includes the loop.

## B2: `chronostack advance 1h` (ms of real time)

Median of 10 runs; p10–p90 in brackets. Idle `debian:bookworm-slim` services, no triggers or barriers. With probes, every service's clock is checked after the move.

| Services | With probes | Without probes (`--no-probe`) |
|---|---|---|
| 1 | 562.3 [546.0–573.5] | 27.9 [27.3–30.4] |
| 5 | 649.2 [634.0–677.3] | 28.0 [26.4–33.7] |
| 20 | 1,163 [1,137–1,243] | 27.5 [26.7–29.0] |

Raw samples: `b1-clock.json`, `b2-advance.json` in this folder.
