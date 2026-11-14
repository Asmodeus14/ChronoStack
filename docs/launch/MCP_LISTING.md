# Listing ChronoStack's MCP server

Two places, both done from your own GitHub account.

## 1. Official MCP Registry (registry.modelcontextprotocol.io)

The release carries everything the registry needs:

- one MCP bundle per target, `chronostack-mcp-<version>-<target>.mcpb`. Each is a zip with `manifest.json` (MCPB manifest 0.3) and the binary;
- `server.json` (registry schema 2025-12-11) listing those bundles with their SHA-256.

They are built by `scripts/build_mcpb.py` in the release workflow, and were added by hand to `v0.1.0-alpha.1`.

Steps (once the release is **published**: the registry checks that the files are publicly downloadable):

1. Install the publisher: `brew install mcp-publisher`, or the archive for your OS from https://github.com/modelcontextprotocol/registry/releases.
2. Download `server.json` from the release into an empty folder.
3. `mcp-publisher login github`. Sign in as **Asmodeus14**: the name `io.github.asmodeus14/chronostack` is tied to that account.
4. `mcp-publisher publish`. Expect `✓ Server io.github.asmodeus14/chronostack version 0.1.0-alpha.1`.
5. Check: `curl "https://registry.modelcontextprotocol.io/v0.1/servers?search=io.github.asmodeus14/chronostack"`.

Notes:

- The registry is in preview. The MCPB format has no CPU-architecture field, so the 5 bundles are separate packages, told apart by file name (linux-x64, linux-arm64, darwin-arm64, darwin-x64, win32-x64). How clients pick between them isn't specified yet.
- Optional extra check before publishing: `npx @anthropic-ai/mcpb validate manifest.json` on a bundle's manifest.

## 2. Community lists

A pull request adding one line, in the testing/developer-tools section of:

- https://github.com/punkpeye/awesome-mcp-servers
- https://github.com/appcypher/awesome-mcp-servers

Suggested entry (follow each list's format and emoji legend; add the macOS emoji once issue #5 confirms macOS):

```markdown
- [Asmodeus14/ChronoStack](https://github.com/Asmodeus14/ChronoStack) 🦀 🏠 🪟 🐧 - Move virtual time across a Docker Compose stack (Postgres, Redis, services) so agents can test trials, scheduled jobs and outages; fixed, safe tool set.
```

## 3. Claude Code / Cursor snippet (for the README and posts)

```sh
claude mcp add chronostack -- chronostack mcp --project-dir /path/to/your/project
```

```json
{ "mcpServers": { "chronostack": { "command": "chronostack", "args": ["mcp", "--project-dir", "/path/to/your/project"] } } }
```
