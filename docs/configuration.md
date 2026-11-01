# Configuration

Every setting is a command-line flag with an environment variable of the same
meaning; `gvm-mcp --help` prints the same reference. Flags override
environment variables, and both override the built-in defaults.

## Reference

| Flag | Environment | Default | Purpose |
| ---- | ----------- | ------- | ------- |
| `--gateway-url` | `GVM_GATEWAY_URL` | `http://127.0.0.1:8080` | Gateway origin, without `/api/v1` |
| `--username` | `GVM_USERNAME` | – | gvmd username of the fallback identity |
| `--password` | `GVM_PASSWORD` | – | gvmd password of the fallback identity |
| `--password-file` | `GVM_PASSWORD_FILE` | – | File containing the password; takes precedence over `--password` |
| `--transport` | `MCP_TRANSPORT` | `stdio` | `stdio` or `streamable-http` |
| `--bind-addr` | `MCP_BIND_ADDR` | `127.0.0.1:8000` | Listen address of the streamable-HTTP transport |
| `--allowed-hosts` | `MCP_ALLOWED_HOSTS` | `localhost,127.0.0.1,::1` | `Host` headers accepted by the HTTP transport; `*` disables the check |
| `--require-caller-auth` | `MCP_REQUIRE_CALLER_AUTH` | `false` | Reject HTTP tool calls that send no `Authorization` header |
| `--toolsets` | `GVM_TOOLSETS` | `default` | `default`, `all` or a comma-separated list of toolset names |
| `--read-only` | `GVM_READ_ONLY` | `false` | Expose only non-mutating tools |
| `--timeout-secs` | `GVM_HTTP_TIMEOUT` | `30` | HTTP timeout towards the gateway, in seconds |
| `--gateway-auth-mode` | `GVM_GATEWAY_AUTH_MODE` | `session` | `session` (one gateway session per identity) or `per-request` (credentials on every call) |
| `--gateway-max-in-flight` | `GVM_GATEWAY_MAX_IN_FLIGHT` | `8` | Gateway requests in flight per identity (1-64) |
| `--allow-insecure-gateway` | `GVM_ALLOW_INSECURE_GATEWAY` | `false` | Permit a plain-http gateway URL that is not loopback |
| `--gateway-ca-cert` | `GVM_GATEWAY_CA_CERT` | – | PEM bundle of extra root certificates for the gateway's TLS certificate |
| `--log-level` | `GVM_LOG_LEVEL` | `info` | Log level when `RUST_LOG` is unset |
| `--list-toolsets` | – | – | Print the toolsets and exit |

## Identity

`GVM_USERNAME` and `GVM_PASSWORD` form the *fallback identity*: the gvmd user
for stdio clients and for HTTP callers that send no `Authorization` header.
Both are optional. Without them every HTTP caller must authenticate as
themselves, and a stdio client gets a legible `401` error from every tool.
`MCP_REQUIRE_CALLER_AUTH=true` keeps the fallback for stdio while refusing to
lend it to HTTP callers. The full model, including how report exports obtain
their own gateway session, is described in
[Authentication and security](authentication.md).

## Reaching the gateway

The server usually runs on a different machine than the gateway, often next
to the person using the assistant, so the connection crosses a network:

- **Use TLS.** `GVM_GATEWAY_URL=https://gateway.example:8443` verifies the
  gateway's certificate against the system roots; for a private CA point
  `GVM_GATEWAY_CA_CERT` at its PEM bundle. TLS 1.2 is the minimum, and the
  client never follows redirects.
- **Plain `http://` is refused** unless the host is loopback
  (`localhost`, `127.0.0.1`, `::1`) or you set
  `GVM_ALLOW_INSECURE_GATEWAY=true`. The flag exists for isolated private
  networks such as a Docker network where gvmd credentials cannot be observed
  in transit; do not use it across a shared network.
- **Sessions, not logins per call.** In the default `session` mode the
  server logs an identity into the gateway once, keeps the resulting session
  token in memory and reuses it until the gateway's idle timeout (5 minutes
  by default) would expire it. One gvmd login per identity replaces one per
  tool call, which is both faster over a network and gentler on gvmd's logs.
  Sessions idle for 60 seconds are closed so they do not occupy the
  gateway's per-user cap between bursts; a session the gateway no longer
  accepts is re-created once and the call retried; if the gateway cannot
  create sessions at all, calls fall back to per-request `Basic`
  authentication. `per-request` mode disables the pool.
- **Bounded parallelism.** At most `GVM_GATEWAY_MAX_IN_FLIGHT` requests per
  identity are in flight at once (default 8). The gateway allows ten
  concurrent sessions per gvmd user and 500 requests per minute per
  identity; the gate keeps parallel tool calls below the first limit and
  turns a burst into a short queue instead of errors.

## Secrets

Prefer `GVM_PASSWORD_FILE` over `GVM_PASSWORD`. The file is read once at
start-up, surrounding whitespace is trimmed, an empty file is rejected, and
the value never appears in logs or in MCP responses. In Docker, mount the
secret and point the variable at it:

```bash
docker run --rm \
  -v "$PWD/gvmd-password:/run/secrets/gvmd-password:ro" \
  -e GVM_PASSWORD_FILE=/run/secrets/gvmd-password \
  -e GVM_USERNAME=admin \
  -e GVM_GATEWAY_URL=http://host.docker.internal:8080 \
  -p 127.0.0.1:8000:8000 \
  ghcr.io/greenbone-hive/openvas-mcp:vX.Y.Z
```

## Transports

- **stdio** (default) is launched by the client; the server logs to stderr
  so that stdout stays a clean protocol channel.
- **streamable HTTP** serves `POST`/`GET` on `/mcp` at `--bind-addr`. The
  default bind address is loopback; bind `0.0.0.0:8000` only behind a reverse
  proxy that terminates TLS, and keep `--allowed-hosts` at the names the
  proxy forwards (or `*` when the proxy already validates `Host`).

The Docker image sets `MCP_TRANSPORT=streamable-http`,
`MCP_BIND_ADDR=0.0.0.0:8000` and `MCP_ALLOWED_HOSTS=*` for container
networks; override them like any other variable.

## Selecting tools

```bash
gvm-mcp --list-toolsets                       # names and scopes
gvm-mcp --toolsets targets,tasks,reports      # a scoped surface
gvm-mcp --toolsets default,identity           # add user administration
gvm-mcp --read-only                           # reads only, any selection
```

`default` and `all` are the same selection (every toolset except
`identity`); `system` is always included. Tools filtered out by `--read-only`
or by the selection are absent from `tools/list`, so clients cannot call them
by accident. See [Toolsets](toolsets.md) for the counts and
[Tool reference](reference/tools/index.md) for every tool.

## Logging

The server logs to stderr in a single-line format. `GVM_LOG_LEVEL` sets the
level (`error`, `warn`, `info`, `debug`, `trace`); a `RUST_LOG` filter such as
`RUST_LOG=gvm_mcp=debug,hyper=warn` takes precedence when present. Forwarded
credentials and the fallback password are redacted at every level.
