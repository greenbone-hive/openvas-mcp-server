![Greenbone Logo](https://www.greenbone.net/wp-content/uploads/greenbone-logo-2025.png)

# OpenVAS MCP Server

[![CI](https://github.com/greenbone-hive/openvas-mcp-server/actions/workflows/ci.yml/badge.svg)](https://github.com/greenbone-hive/openvas-mcp-server/actions/workflows/ci.yml)
[![Security](https://github.com/greenbone-hive/openvas-mcp-server/actions/workflows/security.yml/badge.svg)](https://github.com/greenbone-hive/openvas-mcp-server/actions/workflows/security.yml)
[![Release](https://img.shields.io/github/v/release/greenbone-hive/openvas-mcp-server)](https://github.com/greenbone-hive/openvas-mcp-server/releases)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

A [Model Context Protocol](https://modelcontextprotocol.io) server for
Greenbone Vulnerability Management (GVM/OpenVAS), written in Rust. It exposes
the complete [rust-gvm-api](https://github.com/greenbone-hive/rust-gvm-api)
REST contract as typed MCP tools, so an AI assistant can manage targets, tasks,
scan configurations, reports and compliance audits through a single binary.

## Overview

```text
MCP client (Claude, IDEs, agents)
        │  stdio or streamable HTTP
        ▼
      gvm-mcp                      this repository
        │  HTTP/JSON, forwards the caller's gvmd identity
        ▼
  rust-gvm-api REST gateway        /api/v1
        │  GMP over Unix socket
        ▼
      gvmd
```

`gvm-mcp` is a thin, typed front end over the gateway. Every tool validates
its arguments, issues one gateway request and maps the response back to the
client. All GMP knowledge stays in the gateway.

- **Complete and contract-driven.** 194 tools cover every applicable
  operation of the gateway's OpenAPI contract. A drift check runs in CI.
- **Stateless and identity-preserving.** Each gateway call carries the
  caller's own credentials; gvmd remains the single authority.
- **Scoped by default.** 25 toolsets, opt-in user administration and a
  read-only mode that removes every mutating tool.
- **Token-aware.** List tools return summarized rows with pagination; `get`
  tools return the gateway's full JSON.

## Requirements

| Component | Version |
| --------- | ------- |
| rust-gvm-api REST gateway | contract 0.5.0 |
| gvmd / GMP (verified end to end) | 26.43.0 / 22.8 |
| Rust toolchain (building from source) | 1.90 or newer |

The gateway is the only runtime dependency.

## Quick installation

### Docker (streamable HTTP)

```bash
docker run --rm \
  -e GVM_GATEWAY_URL=http://gateway:8080 \
  -e GVM_USERNAME=admin \
  -e GVM_PASSWORD=secret \
  -p 127.0.0.1:8000:8000 \
  ghcr.io/greenbone-hive/openvas-mcp-server:latest
```

The MCP endpoint is `http://localhost:8000/mcp`.

### Binary (stdio)

Download a [release](https://github.com/greenbone-hive/openvas-mcp-server/releases)
or run `cargo build --release`, then register the binary with your MCP client:

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

Start with the `openvas_test_connection` tool. It verifies gateway liveness
and readiness, the gvmd version and the forwarded identity.

## Documentation

| Document | Contents |
| -------- | -------- |
| [Installation](docs/installation.md) | Gateway prerequisite, Docker, release binaries, building from source, client registration, configuration reference |
| [Authentication and security](docs/authentication.md) | Identity forwarding, export sessions, HTTP hardening, secrets |
| [Toolsets](docs/toolsets.md) | The 25 toolsets, selection and read-only gating, availability notes |
| [Usage](docs/usage.md) | Filters and pagination, scan and audit flows, report exports and imports, deletion semantics |
| [Development](docs/development.md) | Module layout, adding a tool, quality gate, test map, live end-to-end tests |
| [Gateway contract matrix](docs/gateway-sync.md) | Every gateway endpoint and the tool that serves it |
| [Contributing](CONTRIBUTING.md) | Development loop and pull-request conventions |
| [Security policy](SECURITY.md) | Reporting vulnerabilities |
| [Releasing](RELEASING.md) | Release process |

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
