![Greenbone Logo](https://www.greenbone.net/wp-content/uploads/gb_new-logo_horizontal_rgb_small.png)

# OpenVAS MCP Server <!-- omit in toc -->

A [Model Context Protocol](https://modelcontextprotocol.io) server for
Greenbone Vulnerability Management (GVM/OpenVAS), written in Rust. It exposes
the complete [rust-gvm-api](https://github.com/greenbone-hive/rust-gvm-api)
REST contract as typed MCP tools, forwards each caller's gvmd identity to the
gateway, and lets an AI assistant manage targets, tasks, scan configurations,
reports and compliance audits through a single binary.

## Table of Contents <!-- omit in toc -->

- [Requirements](#requirements)
- [Installation](#installation)
- [Configuration](#configuration)
- [Authentication](#authentication)
- [Toolsets](#toolsets)
- [Development](#development)
- [Documentation](#documentation)
- [Maintainer](#maintainer)
- [License](#license)

## Requirements

- A running [rust-gvm-api](https://github.com/greenbone-hive/rust-gvm-api)
  REST gateway (contract 0.5.0) in front of gvmd; verified end to end against
  gvmd 26.43.0 / GMP 22.8
- Rust 1.90 or later, to build from source
- Docker, for the container image and the live end-to-end tests

No GVM installation is required to build or test the server: the test suite
runs against a mock gateway, and the server itself never speaks GMP.

## Installation

Docker, streamable HTTP on `http://localhost:8000/mcp`. Images are published
under immutable release tags only (no `latest`); take `vX.Y.Z` from the
[releases](https://github.com/greenbone-hive/OpenVAS-MCP/releases):

```bash
docker run --rm \
  --add-host=host.docker.internal:host-gateway \
  -e GVM_GATEWAY_URL=http://host.docker.internal:8080 \
  -e GVM_USERNAME=admin \
  -e GVM_PASSWORD=secret \
  -p 127.0.0.1:8000:8000 \
  ghcr.io/greenbone-hive/openvas-mcp:vX.Y.Z
```

Binary over stdio, from a [release](https://github.com/greenbone-hive/OpenVAS-MCP/releases)
or `cargo build --release`, registered with the MCP client:

```json
{
  "mcpServers": {
    "openvas": {
      "command": "/path/to/gvm-mcp",
      "env": {
        "GVM_GATEWAY_URL": "http://localhost:8080",
        "GVM_USERNAME": "admin",
        "GVM_PASSWORD": "secret"
      }
    }
  }
}
```

Start with the `openvas_test_connection` tool: it checks gateway liveness and
readiness, the gvmd version and the forwarded identity. Release archives,
checksums, the compose example and client registration for both transports
are covered in [docs/installation.md](docs/installation.md).

## Configuration

Every setting is an environment variable and a CLI flag of the same meaning
(`gvm-mcp --help`). `GVM_PASSWORD_FILE` points at a mounted secret and wins
over `GVM_PASSWORD`.

| Required | Variable                  | Default                   | Description                                                                                              |
|----------|---------------------------|---------------------------|----------------------------------------------------------------------------------------------------------|
| no       | `GVM_GATEWAY_URL`         | `http://127.0.0.1:8080`   | Origin of the rust-gvm-api gateway, without `/api/v1`                                                    |
| no*      | `GVM_USERNAME`            |                           | gvmd username of the fallback identity (stdio, and HTTP callers that send no `Authorization`)            |
| no*      | `GVM_PASSWORD`            |                           | gvmd password of the fallback identity; also read from `GVM_PASSWORD_FILE`                               |
| no       | `MCP_TRANSPORT`           | `stdio`                   | `stdio` or `streamable-http`                                                                             |
| no       | `MCP_BIND_ADDR`           | `127.0.0.1:8000`          | Listen address of the streamable-HTTP transport                                                          |
| no       | `MCP_ALLOWED_HOSTS`       | `localhost,127.0.0.1,::1` | `Host` headers accepted by the HTTP transport (DNS-rebinding guard); `*` disables the check              |
| no       | `MCP_REQUIRE_CALLER_AUTH` | `false`                   | Reject HTTP tool calls without an `Authorization` header instead of serving them with the fallback identity |
| no       | `GVM_TOOLSETS`            | `default`                 | Toolsets to expose: `default`, `all` or a comma-separated list                                           |
| no       | `GVM_READ_ONLY`           | `false`                   | Expose only non-mutating tools                                                                           |
| no       | `GVM_HTTP_TIMEOUT`        | `30`                      | Gateway HTTP timeout in seconds                                                                          |
| no       | `GVM_LOG_LEVEL`           | `info`                    | Log level when `RUST_LOG` is unset: `error`, `warn`, `info`, `debug`, `trace`                            |

\* required for stdio clients; HTTP callers may instead authenticate as
themselves.

## Authentication

The server holds no session and invents no authentication of its own. The
`Authorization` value of each gateway call is resolved per request: the
inbound MCP request's `Authorization` header when present (so every
streamable-HTTP caller authenticates as themselves with `Basic` gvmd
credentials or a gateway session token), otherwise a `Basic` header built from
`GVM_USERNAME` / `GVM_PASSWORD`. Without either, the gateway answers `401`
and the tool reports it.

The one exception is asynchronous report exports: the gateway cancels an
export job when the session that created it ends, so `openvas_export_report`
opens a job-bound gateway session with the caller's credentials and closes it
after the download, on cancel or when the job reaches a terminal state.

For HTTP deployments, `MCP_REQUIRE_CALLER_AUTH=true` keeps the fallback
identity for stdio only, `MCP_ALLOWED_HOSTS` guards against DNS rebinding, and
TLS belongs on a reverse proxy in front of the server. The gateway client never
follows redirects, rejects credentials embedded in the gateway URL, redacts
secrets from debug output and caps downloaded artifacts at 3 MB. Details in
[docs/authentication.md](docs/authentication.md).

## Toolsets

194 tools in 25 toolsets. `default` is every toolset except `identity`, which
is always opt-in; `GVM_READ_ONLY` removes every mutating tool from the listing.

```bash
gvm-mcp --list-toolsets                     # the 25 toolsets
gvm-mcp --toolsets targets,tasks,reports    # scoped surface
gvm-mcp --toolsets default,identity         # include user administration
gvm-mcp --read-only                         # reads only
```

| Selection                      | Tools |
|--------------------------------|------:|
| `default`                      | 171   |
| `default`, read-only           | 89    |
| `default,identity`             | 194   |
| `default,identity`, read-only  | 99    |

List tools return summarized rows with pagination; `get` tools return the
gateway's full JSON. The toolset reference, the definition of a mutating tool
and the availability notes are in [docs/toolsets.md](docs/toolsets.md); scan,
audit, export and deletion flows are in [docs/usage.md](docs/usage.md).

## Development

```bash
cargo build --release   # target/release/gvm-mcp
cargo test              # unit and mock-gateway tests, no GVM needed
cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings
python3 scripts/gateway_sync_check.py --spec ../rust-gvm-api/spec/rest-api --write-doc
```

The tool surface mirrors the gateway's `spec/rest-api`: 195 of the contract's
199 operations map to exactly one tool each, and the drift check above fails
on any missing, stale or broken mapping. CI runs the same gate plus the drift
check against the gateway's `main` branch. A live end-to-end suite, gated
behind `#[ignore]`, exercises every endpoint against a real gateway and gvmd;
see [docs/development.md](docs/development.md).

## Documentation

The full documentation is published at
<https://greenbone-hive.github.io/OpenVAS-MCP/latest/>; its sources live in
[docs/](docs/):

- [docs/installation.md](docs/installation.md): gateway prerequisite, Docker,
  release binaries, building from source
- [docs/clients/](docs/clients/index.md): registering the server in Claude
  Desktop, Claude Code, VS Code, Cursor, Windsurf and other MCP clients
- [docs/configuration.md](docs/configuration.md): every flag and environment
  variable, secrets, transports, logging
- [docs/authentication.md](docs/authentication.md): identity forwarding,
  export sessions, HTTP hardening, secrets
- [docs/toolsets.md](docs/toolsets.md): the 25 toolsets, selection and
  read-only gating, availability notes
- [docs/usage.md](docs/usage.md): prompt-driven workflows for scans,
  audits, scan-configuration tuning, report exports and housekeeping
- [docs/reference/tools/](docs/reference/tools/index.md): the generated
  reference of all 194 tools and their parameters
- [docs/troubleshooting.md](docs/troubleshooting.md) and
  [docs/faq.md](docs/faq.md)
- [docs/development.md](docs/development.md): module layout, adding a tool,
  quality gate, test map, live end-to-end tests
- [docs/gateway-sync.md](docs/gateway-sync.md): every gateway endpoint and the
  tool that serves it
- [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md),
  [RELEASING.md](RELEASING.md)

## Maintainer

This project is maintained by [Greenbone AG][Greenbone]

## License

Copyright (C) 2026 [Greenbone AG][Greenbone]

Licensed under the [GNU Affero General Public License v3.0 or later](LICENSE).

[Greenbone]: https://www.greenbone.net/
