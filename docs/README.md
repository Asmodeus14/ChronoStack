# ChronoStack Documentation

ChronoStack is **pre-alpha** (first pre-release `v0.1.0-alpha.2`). The guide and the results documents describe what is built and tested. The planning documents (vision, architecture, roadmap) also cover work that is not built yet; their status notes say which.

## Start here

| If you want to… | Read |
|---|---|
| Use it on your own project | [GUIDE.md](GUIDE.md) |
| Understand the product in 5 minutes | [PRODUCT_VISION.md](PRODUCT_VISION.md) → [MVP_SCOPE.md](MVP_SCOPE.md) |
| See how it will work | [ARCHITECTURE.md](ARCHITECTURE.md) |
| Know why it is designed this way | [ARCHITECTURE_DECISIONS.md](ARCHITECTURE_DECISIONS.md) |
| Check the evidence | [TECHNICAL_FEASIBILITY.md](TECHNICAL_FEASIBILITY.md) and [SOURCE_VERIFICATION.md](SOURCE_VERIFICATION.md) |
| Know what will (not) work | [RISKS_AND_LIMITATIONS.md](RISKS_AND_LIMITATIONS.md) |
| Contribute | [../CONTRIBUTING.md](../CONTRIBUTING.md) |

## Product

- [PRODUCT_VISION.md](PRODUCT_VISION.md): problem, positioning, principles, non-goals
- [COMPETITIVE_ANALYSIS.md](COMPETITIVE_ANALYSIS.md): libfaketime, Stripe Test Clocks, fake timers, Toxiproxy, Chaos Mesh and more
- [GO_TO_MARKET.md](GO_TO_MARKET.md): audience, personas, scenarios, adoption barriers
- [FEATURE_ROADMAP.md](FEATURE_ROADMAP.md): scored features, v0.1 / v0.2 / v1.0

## Architecture and research

- [TECHNICAL_FEASIBILITY.md](TECHNICAL_FEASIBILITY.md): Linux time interception, shared clock, Postgres, Redis, runtimes, Docker
- [SOURCE_VERIFICATION.md](SOURCE_VERIFICATION.md): which claims were checked against primary sources, and what was found
- [ARCHITECTURE.md](ARCHITECTURE.md): components, time domains, control and data flow, failure modes, API
- [ARCHITECTURE_DECISIONS.md](ARCHITECTURE_DECISIONS.md): ADR-001 to ADR-017
- [M0_RESULTS.md](M0_RESULTS.md): feasibility spike results and exit-gate verdict
- [M1_RESULTS.md](M1_RESULTS.md): controller and CLI results, including 3 external projects
- [M2_RESULTS.md](M2_RESULTS.md): step/trigger/barrier engine, demo and pg_cron results
- [M3_RESULTS.md](M3_RESULTS.md): fault proxy, outage windows, scenarios and reports
- [M4_RESULTS.md](M4_RESULTS.md): MCP server and Node register
- [M5_RESULTS.md](M5_RESULTS.md): release pipeline and install scripts
- [GUIDE.md](GUIDE.md): using ChronoStack on your own project
- [schema/chronostack.schema.json](schema/chronostack.schema.json): JSON Schema for `chronostack.yml`
- [schema/scenario.schema.json](schema/scenario.schema.json): JSON Schema for scenario files

## Planning

- [MVP_SCOPE.md](MVP_SCOPE.md): the v0.1 workflow, reference demo, compatibility targets
- [IMPLEMENTATION_PLAN.md](IMPLEMENTATION_PLAN.md): milestones M0–M5 with exit gates
- [TESTING_STRATEGY.md](TESTING_STRATEGY.md): test layers, the no-hang suite, scheduler tests, performance targets

## Quality, security and operations

- [SECURITY_AND_RELIABILITY.md](SECURITY_AND_RELIABILITY.md): guarantees, threat review, failure behaviour
- [RISKS_AND_LIMITATIONS.md](RISKS_AND_LIMITATIONS.md): ranked risks, published limitations
- [OPEN_SOURCE_READINESS.md](OPEN_SOURCE_READINESS.md): readiness checklist

## Project development

- [development/RELEASE_PROCESS.md](development/RELEASE_PROCESS.md): versioning, release stages, tooling, support policy
- [development/LICENSING.md](development/LICENSING.md): licensing decision: MIT OR Apache-2.0
- [development/REPOSITORY_SETUP.md](development/REPOSITORY_SETUP.md): repository metadata, deferred configuration, GitHub settings

## Glossary

| Term | Meaning |
|---|---|
| **ADR** | Architecture Decision Record — see [ARCHITECTURE_DECISIONS.md](ARCHITECTURE_DECISIONS.md) |
| **Barrier** | A condition (SQL, Redis, HTTP or a command) that ChronoStack waits on after a time step, e.g. "no jobs running" |
| **clockd** | The ChronoStack controller container; the only writer of the virtual clock |
| **`CLOCK_REALTIME` / `CLOCK_MONOTONIC`** | Linux wall-clock time (can jump) vs elapsed-time clock (never jumps). ChronoStack only shifts the former. |
| **`LD_PRELOAD`** | Linux dynamic-loader feature that loads a library into a process before others, letting it intercept functions such as `clock_gettime` |
| **MCP** | Model Context Protocol — a standard for AI agents to call tools |
| **PLT** | Procedure Linkage Table — how dynamically linked programs call shared-library functions; why preloading can intercept them |
| **Seqlock** | Sequence lock: a lock-free single-writer/many-reader pattern that lets readers detect and retry torn reads |
| **Shim** | `libchronoclock`, ChronoStack's preload library |
| **Sub-step** | A slice of a time step, no larger than `max_jump`, so that cron-like schedulers don't treat the advance as a clock correction |
| **Trigger** | A declared command or HTTP call ChronoStack runs after each time step to execute scheduled work |
| **TTL** | Time to live (e.g. Redis key expiry) |
| **vDSO** | virtual Dynamic Shared Object — kernel-provided code that makes `clock_gettime` fast without a syscall |

## Evidence labels used throughout

| Label | Meaning |
|---|---|
| **Verified / [Verified-source]** | Stated in an upstream document or source file that was opened |
| **Partially verified** | Partly supported, or supported only by a summary of unread text |
| **[Search-confirmed]** | Supported by web search results; the page was not opened |
| **[Assumption]** | Expected behaviour that must be proven, usually in M0 |
| **[Unresolved]** | Open question |
