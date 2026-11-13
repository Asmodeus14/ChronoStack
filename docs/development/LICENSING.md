# Licensing decision

**Status: DECIDED (2026-11-13) — `MIT OR Apache-2.0`**, chosen by the repository owner.

| Item | Value |
|---|---|
| License expression | `MIT OR Apache-2.0` |
| Files | [LICENSE-MIT](../../LICENSE-MIT), [LICENSE-APACHE](../../LICENSE-APACHE) (official text from https://www.apache.org/licenses/LICENSE-2.0.txt) |
| Copyright line (MIT) | `Copyright (c) 2026 The ChronoStack Contributors`. The owner may replace this with a person or organisation name. |
| `Cargo.toml` | `license = "MIT OR Apache-2.0"` |
| `package.json` (future) | `"license": "(MIT OR Apache-2.0)"` |
| Contributions | Inbound = outbound. The standard Rust wording is in the README and CONTRIBUTING. No CLA or DCO is required at this stage. |

## Rationale

- The Rust API guidelines say crates wanting "maximum compatibility with the Rust ecosystem are recommended to do the same", meaning the dual MIT/Apache-2.0 license used by the Rust project ([source](https://rust-lang.github.io/api-guidelines/necessities.html)).
- Apache-2.0 provides an explicit patent grant; MIT keeps GPLv2 compatibility and is familiar to npm users.
- libfaketime (GPL) is not included or linked. It is used only as an external comparison oracle in M0 tests, pulled in at test time from the distribution package inside a test container.

## Options that were compared

| Option | Pros | Cons |
|---|---|---|
| MIT | Short; universal; npm standard | No explicit patent grant |
| Apache-2.0 | Patent grant; contribution terms (§5) | Reported incompatibility with GPLv2-only code |
| **MIT OR Apache-2.0** | Rust convention; users choose | Two files |
| Copyleft (GPL/AGPL/MPL) | Keeps derivatives open | Less corporate adoption for a developer tool |
