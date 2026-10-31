# OpenVAS MCP Server

MCP server for Greenbone Vulnerability Management (GVM/OpenVAS), written in
Rust. A thin, typed MCP front end over the
[rust-gvm-api](https://github.com/greenbone-hive/rust-gvm-api) REST gateway:
all GMP/gvmd knowledge lives in the gateway, this server maps MCP tools onto
gateway HTTP endpoints — every one of them.

[![CI](https://github.com/greenbone-hive/openvas-mcp-server/actions/workflows/ci.yml/badge.svg)](https://github.com/greenbone-hive/openvas-mcp-server/actions/workflows/ci.yml)
[![License: AGPL v3](https://img.shields.io/badge/License-AGPL_v3-blue.svg)](https://www.gnu.org/licenses/agpl-3.0)

```text
MCP client (Claude, etc.)
        │  stdio / streamable HTTP
        ▼
      gvm-mcp  (this repo, single binary)
        │  HTTP/JSON — forwards the caller's gvmd identity
        ▼
  rust-gvm-api REST gateway  (/api/v1)
        │  GMP over Unix socket
        ▼
      gvmd
```

## Gateway contract

The tool surface is kept in lockstep with the gateway's OpenAPI contract
(`spec/rest-api`, currently **0.5.0**: 121 paths, 199 operations). 195
operations map to exactly one tool each; the remaining 4 (session
create/inspect/delete and the OpenAPI document) are not applicable to a
stateless pass-through server. The full endpoint → tool matrix is in
[docs/gateway-sync.md](docs/gateway-sync.md), and
`scripts/gateway_sync_check.py` regenerates it and fails on drift (missing,
stale or broken mappings) against any checkout of the gateway spec.

## Quick start

### Docker (streamable HTTP)

```bash
docker run --rm \
  -e GVM_GATEWAY_URL=http://gateway:8080 \
  -e GVM_USERNAME=admin \
  -e GVM_PASSWORD=secret \
  -p 127.0.0.1:8000:8000 \
  ghcr.io/greenbone-hive/openvas-mcp-server:latest
```

MCP endpoint: `http://localhost:8000/mcp`. A full stack example (gateway +
gvmd socket wiring) is in
[docker-compose.example.yml](docker-compose.example.yml).

### Binary (stdio)

Download a release binary (or `cargo build --release`), then register it with
your MCP client:

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

Start with the `openvas_test_connection` tool — it checks gateway liveness
and readiness, the gvmd version and makes one authenticated call to confirm
the identity is accepted.

## Authentication

gvm-mcp holds no session and invents no auth of its own — it forwards a
caller's gvmd identity to the gateway, and the gateway (backed by gvmd) is
the sole authority. The `Authorization` used for each gateway call is
resolved per request:

1. the inbound MCP request's `Authorization` header, when present — so each
   **streamable-HTTP caller authenticates as themselves** (send `Basic`
   gvmd credentials, or a gateway session token you obtained); otherwise
2. a fallback `Basic` header built from configured `GVM_USERNAME` /
   `GVM_PASSWORD` — used for **stdio**, and for HTTP callers that send none.

If neither is available, gateway calls go out unauthenticated and are
rejected with `401`, surfaced as a legible tool error. No credentials are
ever cached as a session; `--username`/`--password` are optional (omit them
to require every HTTP caller to authenticate as themselves).

One exception to "no session": asynchronous report exports. The gateway
binds an export job to the session that created it and cancels the job when
that session ends — and request-scoped `Basic` sessions end with the
request. So for `openvas_export_report` gvm-mcp opens one gateway session
with the caller's own credentials, runs the job on it, serves
`openvas_get_job` / `openvas_download_job_result` through it, and deletes it
as soon as the artifact is downloaded, the job is cancelled or it reaches a
terminal state. The session is usable only by the identity that created the
job; callers that forward their own gateway bearer token are used as-is.

Hardening for HTTP deployments:

- `--require-caller-auth` (`MCP_REQUIRE_CALLER_AUTH=true`) refuses HTTP tool
  calls that carry no `Authorization` header instead of serving them with
  the fallback identity; the fallback then only ever serves stdio. The server
  logs a warning at startup when fallback credentials are configured, the
  flag is off and the bind address is not loopback.
- `--allowed-hosts` is a DNS-rebinding guard, not authentication. For a
  public HTTP deployment, terminate TLS and (if you want a single shared
  identity) authenticate at a reverse proxy in front.
- The gateway client never follows redirects, rejects credentials embedded
  in `GVM_GATEWAY_URL`, redacts the fallback credential from all `Debug`
  output, and caps downloaded artifacts (report exports, agent support
  bundles) at 3 MB before buffering them.

## Configuration

Every flag has an environment variable.

| Flag | Env | Default | Purpose |
| ---- | --- | ------- | ------- |
| `--gateway-url` | `GVM_GATEWAY_URL` | `http://127.0.0.1:8080` | rust-gvm-api origin (without `/api/v1`) |
| `--username` | `GVM_USERNAME` | — | gvmd username for the fallback identity (optional) |
| `--password` | `GVM_PASSWORD` | — | gvmd password for the fallback identity (optional) |
| `--password-file` | `GVM_PASSWORD_FILE` | — | file containing the password (takes precedence) |
| `--transport` | `MCP_TRANSPORT` | `stdio` | `stdio` or `streamable-http` |
| `--bind-addr` | `MCP_BIND_ADDR` | `127.0.0.1:8000` | HTTP bind address |
| `--allowed-hosts` | `MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | Host-header allow list (`*` disables) |
| `--require-caller-auth` | `MCP_REQUIRE_CALLER_AUTH` | `false` | HTTP callers must send `Authorization` (no fallback) |
| `--toolsets` | `GVM_TOOLSETS` | `default` | comma-separated toolset selection |
| `--read-only` | `GVM_READ_ONLY` | `false` | expose only non-mutating tools |
| `--timeout-secs` | `GVM_HTTP_TIMEOUT` | `30` | gateway HTTP timeout |
| `--log-level` | `GVM_LOG_LEVEL` | `info` | log level when `RUST_LOG` unset |

## Toolsets

The full surface is 194 tools — far too many to hand an LLM by default, so
tools are grouped into 25 toolsets (`gvm-mcp --list-toolsets` prints them
all). The default selection is every toolset except `identity`; add it
explicitly with `--toolsets default,identity`. `--read-only` additionally
strips every mutating tool from the listing.

| Selection | Tools |
| --------- | ----- |
| default | 171 |
| default, read-only | 89 |
| default + identity | 194 |
| default + identity, read-only | 99 |

Highlights:

- **targets / tasks** — CRUD, clone and the scan lifecycle
  (`openvas_start_task` → report UUID, stop, resume); every task variant the
  gateway accepts (classic, agent group, OCI image, web application, import
  container) plus OCI image and web application target families.
- **reports** — list/get (with embedded-result paging), ten drill-down pages
  (results, vulnerabilities, hosts, ports, applications, CVEs, closed CVEs,
  errors, operating systems, TLS certificates), XML report import, and
  asynchronous exports: `openvas_export_report` → poll `openvas_get_job` →
  `openvas_download_job_result` (JSON inline, binary formats base64, both
  capped at 3 MB).
- **scan-configs** — CRUD, selected NVTs, preferences, family and NVT
  selection writes, and the generic usage-typed config catalog.
- **compliance** — audits (full lifecycle) and policies.
- **credentials / schedules / alerts / port-lists / filters / tags / notes /
  overrides** — CRUD (plus clone where the gateway offers it) and
  credential-store administration.
- **assets / tls-certificates / vulnerabilities / secinfo / nvts / feeds** —
  host, OS and generic assets (with writes), certificate and vulnerability
  listings, CVE/CPE/CERT-Bund/DFN-CERT catalogs, typed NVT browsing.
- **agents** — sensor agents, agent groups, agent-control defaults and
  installer instructions (requires a gvmd build with agents enabled).
- **identity** (opt-in) — users, groups, roles, permissions, user settings.

List tools return summarized rows plus pagination to protect the model's
token budget; `get` tools return the gateway's full JSON.

## Development

```bash
cargo test                                    # unit + mock-gateway tests
cargo fmt --all && cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --list-toolsets
python3 scripts/gateway_sync_check.py --spec ../rust-gvm-api/spec/rest-api --write-doc
```

The test suite runs entirely against a mock gateway (wiremock) — no GVM
installation needed. End-to-end tests against a live stack are gated behind
`#[ignore]` and double as the endpoint coverage map
([docs/e2e-test-map.md](docs/e2e-test-map.md)):

```bash
export GVM_E2E_GATEWAY_URL=http://127.0.0.1:18080
export GVM_USERNAME=admin GVM_PASSWORD=admin
export GVM_E2E_TARGET_HOST=openvasd                     # host the scanner can reach
cargo test --test e2e_live -- --ignored --test-threads=1            # catalogs + lifecycles
GVM_E2E_SCAN=1 cargo test --test e2e_live -- --ignored --test-threads=1   # + scan and audit runs
```

DTOs and endpoint shapes mirror the gateway's
[`spec/rest-api`](https://github.com/greenbone-hive/rust-gvm-api/tree/main/spec/rest-api)
contract. See [docs/](docs/) for detailed install/usage/development
guides.

## Limitations

- Requires a running
  [rust-gvm-api](https://github.com/greenbone-hive/rust-gvm-api) gateway; the
  server never speaks GMP directly.
- Tickets are not available: the gateway removed its ticket routes
  (rust-gvm-api #500), so the former ticket tools were removed with them.
- OCI image targets, web application targets and agents depend on the
  connected gvmd build having those features enabled; otherwise the gateway
  answers with a problem that surfaces as a legible tool error.
- Deleting an imported report is refused by current gvmd builds (the report
  is retained with its import task).

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for the development loop, how to add a
tool, and PR conventions. Security issues: see [SECURITY.md](SECURITY.md).
Releases follow [RELEASING.md](RELEASING.md).

## License

AGPL-3.0-or-later.
