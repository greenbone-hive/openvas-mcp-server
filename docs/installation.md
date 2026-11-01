# Installation

`gvm-mcp` is a client of the
[rust-gvm-api](https://github.com/greenbone-hive/rust-gvm-api) REST gateway.
It speaks only HTTP/JSON to that gateway and never talks to gvmd or GMP
directly, so a reachable gateway is a prerequisite for every install method
below.

## Prerequisite: a running gateway

1. A gvmd installation, typically the
   [Greenbone Community Edition](https://greenbone.github.io/docs/latest/)
   containers.
2. The rust-gvm-api gateway attached to gvmd's Unix socket and listening on
   HTTP. Its `compose.yaml` brings up a full local gvmd + gateway stack; note
   the gateway URL (default `http://localhost:8080`).
3. A gvmd identity to authenticate with. `gvm-mcp` forwards a caller's gvmd
   identity to the gateway (see [Authentication](authentication.md)); for
   stdio, configure a fallback account and pair it with `--read-only` for
   reporting-only deployments.

Verify the gateway is reachable before starting the server:

```bash
curl -fsS http://localhost:8080/health
```

## Option 1: Docker (streamable HTTP)

```bash
docker run --rm \
  -e GVM_GATEWAY_URL=http://gateway:8080 \
  -e GVM_USERNAME=admin \
  -e GVM_PASSWORD=secret \
  -p 127.0.0.1:8000:8000 \
  ghcr.io/greenbone-hive/openvas-mcp-server:latest
```

The image is distroless, runs as a non-root user and defaults to the
streamable-HTTP transport on `0.0.0.0:8000` with the Host-header check
disabled (`MCP_ALLOWED_HOSTS=*`) for container networks. Front it with a
reverse proxy if you expose it beyond localhost.

[docker-compose.example.yml](../docker-compose.example.yml) runs this service
as a client of an external gateway. To run on the gateway's own compose
network, attach to it and use `GVM_GATEWAY_URL=http://gvm-gateway:8080`.

MCP endpoint: `http://localhost:8000/mcp`.

## Option 2: Release binary (stdio or HTTP)

Download the archive for your platform from the
[releases page](https://github.com/greenbone-hive/openvas-mcp-server/releases),
verify the checksum, unpack, and place `gvm-mcp` on your `PATH`:

```bash
tar xzf gvm-mcp-<version>-<target>.tar.gz
shasum -c gvm-mcp-<version>-<target>.tar.gz.sha256
./gvm-mcp --help
```

Each release also ships a CycloneDX SBOM archive alongside the binaries.

## Option 3: Build from source

Requires Rust 1.90 or newer.

```bash
git clone https://github.com/greenbone-hive/openvas-mcp-server.git
cd openvas-mcp-server
cargo build --release
./target/release/gvm-mcp --help
```

## MCP client registration

### stdio

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

### Streamable HTTP

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

Every flag has an environment variable equivalent. `gvm-mcp --help` prints the
same reference.

| Flag | Environment | Default | Purpose |
| ---- | ----------- | ------- | ------- |
| `--gateway-url` | `GVM_GATEWAY_URL` | `http://127.0.0.1:8080` | Gateway origin, without `/api/v1` |
| `--username` | `GVM_USERNAME` | – | gvmd username for the fallback identity |
| `--password` | `GVM_PASSWORD` | – | gvmd password for the fallback identity |
| `--password-file` | `GVM_PASSWORD_FILE` | – | File containing the password; takes precedence |
| `--transport` | `MCP_TRANSPORT` | `stdio` | `stdio` or `streamable-http` |
| `--bind-addr` | `MCP_BIND_ADDR` | `127.0.0.1:8000` | Bind address for streamable HTTP |
| `--allowed-hosts` | `MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | Accepted `Host` headers (`*` disables the check) |
| `--require-caller-auth` | `MCP_REQUIRE_CALLER_AUTH` | `false` | HTTP callers must send `Authorization` |
| `--toolsets` | `GVM_TOOLSETS` | `default` | Comma-separated toolset selection |
| `--read-only` | `GVM_READ_ONLY` | `false` | Expose only non-mutating tools |
| `--timeout-secs` | `GVM_HTTP_TIMEOUT` | `30` | Gateway HTTP timeout in seconds |
| `--log-level` | `GVM_LOG_LEVEL` | `info` | Log level when `RUST_LOG` is unset |

## Next steps

- Run the `openvas_test_connection` tool from your client; it checks gateway
  liveness and readiness, the gvmd version and the forwarded identity.
- Choose the exposed surface with `--toolsets` and `--read-only`
  ([Toolsets](toolsets.md)).
- Review the identity model and HTTP hardening options
  ([Authentication and security](authentication.md)).
