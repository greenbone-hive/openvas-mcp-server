# Installation

`gvm-mcp` is a client of the
[rust-gvm-api](https://github.com/greenbone-hive/rust-gvm-api) REST gateway.
It speaks **only** HTTP/JSON to that gateway — it never talks to gvmd or GMP
directly. A reachable gateway is therefore a hard prerequisite for every
install method below.

## Prerequisite: a running gateway

1. A gvmd installation — typically the
   [Greenbone Community Edition](https://greenbone.github.io/docs/latest/)
   containers.
2. The rust-gvm-api gateway attached to gvmd's Unix socket and listening on
   HTTP. Its `compose.yaml` / `scripts/compose-dev.sh` bring up a full local
   `gvmd` + gateway stack; note the gateway URL (default
   `http://localhost:8080`).
3. A gvmd identity to authenticate with. gvm-mcp forwards a caller's gvmd
   identity to the gateway (see [Authentication](#authentication)); for
   stdio, configure a fallback account and pair it with `--read-only` for
   reporting-only deployments.

Verify the gateway is reachable before starting the server:

```bash
curl -fsS http://localhost:8080/health
```

## Option 1 — Docker (streamable HTTP)

```bash
docker run --rm \
  -e GVM_GATEWAY_URL=http://gateway:8080 \
  -e GVM_USERNAME=admin \
  -e GVM_PASSWORD=secret \
  -p 127.0.0.1:8000:8000 \
  ghcr.io/greenbone-hive/openvas-mcp-server:latest
```

The image is distroless, runs as nonroot, and defaults to the
streamable-HTTP transport on `0.0.0.0:8000` with the Host-header check
disabled (`MCP_ALLOWED_HOSTS=*`) for container networks. Front it with a
reverse proxy if you expose it beyond localhost.

A compose example (this service as a client of an external gateway) is in
[docker-compose.example.yml](../docker-compose.example.yml). To run on the
gateway's own compose network, attach to it and use
`GVM_GATEWAY_URL=http://gvm-gateway:8080`.

MCP endpoint: `http://localhost:8000/mcp`.

## Option 2 — Release binary (stdio or HTTP)

Download the archive for your platform from the
[releases page](https://github.com/greenbone-hive/openvas-mcp-server/releases),
verify the checksum, unpack, and place `gvm-mcp` on your `PATH`:

```bash
tar xzf gvm-mcp-<version>-<target>.tar.gz
shasum -c gvm-mcp-<version>-<target>.tar.gz.sha256
./gvm-mcp --help
```

Each release also ships a CycloneDX SBOM archive alongside the binaries.

## Option 3 — Build from source

Requires Rust 1.90 or newer.

```bash
git clone https://github.com/greenbone-hive/openvas-mcp-server.git
cd openvas-mcp-server
cargo build --release
./target/release/gvm-mcp --help
```

## MCP client registration

### stdio (Claude Desktop / Claude Code)

Best for the binary or source install; the client launches the process and
talks over stdio.

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

### Streamable HTTP (Docker or a long-running process)

Start the server with `--transport streamable-http` (the Docker image does
this by default), then register the URL:

```json
{
  "mcpServers": {
    "openvas": {
      "url": "http://localhost:8000/mcp"
    }
  }
}
```

## Configuration reference

All flags and their environment variables are documented in the
[README configuration table](../README.md#configuration). The essentials:

| Setting | Env | Notes |
| ------- | --- | ----- |
| Gateway URL | `GVM_GATEWAY_URL` | Origin of the rust-gvm-api gateway (no `/api/v1`) |
| Username | `GVM_USERNAME` | gvmd fallback identity (optional) |
| Password | `GVM_PASSWORD` / `GVM_PASSWORD_FILE` | Prefer the file form in production |
| Transport | `MCP_TRANSPORT` | `stdio` (default) or `streamable-http` |
| Bind address | `MCP_BIND_ADDR` | HTTP only, default `127.0.0.1:8000` |
| Allowed hosts | `MCP_ALLOWED_HOSTS` | HTTP DNS-rebinding guard; `*` to disable |
| Require caller auth | `MCP_REQUIRE_CALLER_AUTH` | HTTP callers must send `Authorization`; the fallback identity then serves stdio only |

## Authentication

gvm-mcp holds no session and invents no auth of its own — it forwards a
caller's gvmd identity to the gateway, and gvmd is the sole authority. The
`Authorization` used for each gateway call is resolved per request:

- **stdio / fallback**: a `Basic` header built from `GVM_USERNAME` /
  `GVM_PASSWORD`. This is the identity for stdio clients and for HTTP callers
  that send no `Authorization`.
- **streamable HTTP, per caller**: if the inbound MCP request carries an
  `Authorization` header (a gateway session token, or `Basic` gvmd
  credentials), gvm-mcp forwards it verbatim, so each HTTP caller
  authenticates as themselves and gvmd enforces their permissions.

If neither is available the request goes out unauthenticated and the gateway
answers `401`. Credentials are optional — omit `GVM_USERNAME` / `GVM_PASSWORD`
to require every HTTP caller to authenticate as themselves, or keep them for
stdio and set `MCP_REQUIRE_CALLER_AUTH=true` so HTTP callers can never borrow
the fallback identity. `MCP_ALLOWED_HOSTS` is a DNS-rebinding guard, not
authentication; terminate TLS at a reverse proxy for public HTTP deployments.

The Docker image binds `0.0.0.0:8000` and disables the Host check for container
networks; when you pass `GVM_USERNAME`/`GVM_PASSWORD` to it, also pass
`MCP_REQUIRE_CALLER_AUTH=true` or keep the port on a private network — the
server logs a warning at startup otherwise.

One exception to "no session": asynchronous report exports. The gateway binds
an export job to the session that created it and cancels the job when that
session ends, so for `openvas_export_report` gvm-mcp opens one gateway session
with the caller's credentials, keeps it for the job's polling and download,
and deletes it as soon as the artifact is downloaded, the job is cancelled or
it reaches a terminal state. Callers that forward their own gateway bearer
token are used as-is.

## Secrets

Prefer `GVM_PASSWORD_FILE` (a mounted secret file) over `GVM_PASSWORD` in
production; it takes precedence when both are set. Passwords and forwarded
tokens never appear in logs, `Debug` output or MCP responses, and no
credential is cached as a session.
