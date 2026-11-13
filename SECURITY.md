# Security Policy

## Project status

ChronoStack is in its **design phase**. No releases exist, so there is no deployed code that could contain a vulnerability yet. This policy defines how security will be handled from the first release onward. Design-level security concerns are welcome now.

## Reporting a vulnerability

**Please do not report security vulnerabilities through public issues, discussions or pull requests.**

Preferred channel: **GitHub private vulnerability reporting** (the "Report a vulnerability" button on the repository's Security tab).

> [!WARNING]
> **Owner action required.** Private vulnerability reporting must be enabled in the repository settings (Settings → Code security → Private vulnerability reporting) before the repository goes public. No email contact has been set up yet, and none is invented here.

Please include:

- the affected component (CLI, `clockd`, preload library, MCP server, SDK);
- the version or commit;
- reproduction steps;
- the impact you have identified.

## What to expect

These are targets; the project has no maintainer team yet, so please be patient:

- acknowledgement within 7 days;
- an initial assessment within 14 days;
- coordinated disclosure: we agree on a disclosure date with you, normally within 90 days of the report, or sooner once a fix is released;
- credit in the release notes, unless you prefer to remain anonymous.

## Supported versions

| Version | Supported |
|---|---|
| None released | — |

Planned policy:

- Before 1.0, only the **latest minor release** receives security fixes.
- From 1.0, the latest minor release plus the previous one, for 6 months.

## Security boundaries (by design)

- ChronoStack must **never** change the host or VM operating-system clock. It does not use `settimeofday`/`clock_settime` and does not request `CAP_SYS_TIME`.
- It needs no privileged containers, and no container receives the Docker socket.
- The controller API binds to `127.0.0.1` only and requires a per-session random token stored with user-only permissions.
- The MCP server uses stdio only, exposes a fixed tool set, and cannot execute arbitrary commands. Only triggers declared in `chronostack.yml` can run.
- Reports and logs never include environment variables, and they redact likely secrets.

Any behaviour that breaks these boundaries is a security vulnerability.

## Known architectural risks

- **Preload library in every process.** A bug in `libchronoclock` could crash or hang processes in your stack, including databases. Mitigations:
  - a minimal, allocation-free design;
  - fail-open by default;
  - a dedicated no-hang test suite.
- **Triggers execute commands.** `chronostack.yml` triggers run commands in your containers. They carry the same trust level as `docker compose up` on that repository. Review them before running.
- **Virtual timestamps persist.** Data written while time is shifted stays in ChronoStack's volumes. Never point ChronoStack at a database holding data you care about.
- **Local API.** Any process running as your user can read the session token and change the virtual clock.

Details: [docs/SECURITY_AND_RELIABILITY.md](docs/SECURITY_AND_RELIABILITY.md)
