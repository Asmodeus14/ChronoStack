# M1 — Controller + core CLI: results

**Date:** 2026-11-13
**Exit gate** ([IMPLEMENTATION_PLAN](IMPLEMENTATION_PLAN.md#m1--controller-cli-core-compose-integration-weeks-34)): *3 external projects work without modification* — **passed**, with one ChronoStack bug found and fixed.

## Automated coverage

| Where | Result |
|---|---|
| Unit tests: Linux x86_64 and arm64, Windows, macOS (CI) | Pass |
| MSRV 1.85 (CI) | Pass |
| `clockd` tests (clock writer, seqlock stress), run inside the image build | Pass |
| e2e `tests/e2e/run_m1.py`: CI on native Linux x86_64 and arm64, plus local Docker Desktop (WSL2) | All checks pass |

The e2e fixture covers:

- Postgres (musl) and Redis (glibc);
- Node (musl) with a pre-existing `LD_PRELOAD`;
- Debian bullseye (glibc 2.31);
- a static Go image (`traefik/whoami`): reported as not virtualizable;
- a Go server inside an Alpine image (`caddy:2-alpine`): flagged by `doctor`;
- advance, Redis expiry, Postgres `now()`;
- freeze and unfreeze;
- `clockd` restart (the clock is kept and the new port is found);
- token auth and input validation;
- clean `down -v`, with the user's file unchanged.

## External projects

These are unmodified projects from [docker/awesome-compose](https://github.com/docker/awesome-compose), run with `chronostack up` → `doctor` → `advance 3d` → in-app time checks → `down -v`.

| Project | Services | ChronoStack result | Notes |
|---|---|---|---|
| wordpress-mysql | WordPress (PHP, glibc), MariaDB 10.6 (glibc) | ✅ Both virtualized. After `advance 3d`, PHP `time()` and MariaDB `UNIX_TIMESTAMP(NOW())` both match virtual time within 1 s. | The first attempt queried MariaDB before its init finished. Rerun with a readiness wait: OK. |
| gitea-postgres | Gitea (Go on Alpine), Postgres `alpine` | ⚠️ **Found a ChronoStack bug, now fixed.** `doctor` reported Gitea healthy. PID 1 is a C supervisor (s6) that loads the shim, but the server is a Go child process launched through a wrapper (`exec -a "$0"`), and Go ignores the shim. | The Postgres container crash-loops **without ChronoStack too**: `postgres:alpine` is now Postgres 18, which rejects the example's `/var/lib/postgresql/data` volume path. Upstream issue. |
| prometheus-grafana | Prometheus (static Go), Grafana (Go on Alpine) | ✅ Prometheus correctly reported as not virtualizable (no loader). Grafana correctly flagged: shim not in PID 1, Go server. | Prometheus crash-loops **without ChronoStack too**: the current image rejects the example's config. Upstream issue. |

**Conclusion:**

- On the three projects, ChronoStack's override was accepted unchanged.
- Every C/C++/PHP/Node runtime saw the virtual time.
- Every Go process is now either excluded or explicitly flagged.
- The project files were left byte-identical.

## Fixes made during M1 (all covered by tests)

1. **`doctor` scans every process, not just PID 1.** It reads each process's real executable (`/proc/<pid>/exe`) as that process's own uid, because the command line can lie. Go binaries are identified by the `.gopclntab` section, and no false positives were found in Postgres, Redis, Node or Debian.
2. **Shim-in-PID-1 check runs as PID 1's uid.** Postgres and Redis run as non-root users, so root can't read their maps without CAP_SYS_PTRACE.
3. **The controller port is re-resolved** after `clockd` restarts, because Docker assigns a new random host port.

## New known limitations (also in [RISKS_AND_LIMITATIONS](RISKS_AND_LIMITATIONS.md))

- **Your normal stack and the ChronoStack copy cannot run at the same time** if the project publishes fixed host ports (80, 3000, 9090, …) or sets `container_name`. The copy keeps both. Candidate fix (M2): strip host ports and container names in the override, via Compose `!reset`, which still needs to be verified.
- **Probes right after `up` can report `unavailable`** while services such as a database initialize. `doctor` is accurate; consider an `up --wait` option.
- **First `up` is dominated by image pulls:** 73–194 s for the three projects, mostly downloads.
