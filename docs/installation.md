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
   HTTP. Its `compose.yaml` brings up a full local gvmd + gateway stack and
   publishes the gateway on `127.0.0.1:8080`.
3. A gvmd identity to authenticate with. `gvm-mcp` forwards a caller's gvmd
   identity to the gateway (see [Authentication](authentication.md)); for
   stdio clients, configure a fallback account and pair it with `--read-only`
   for reporting-only deployments.

Verify the gateway before starting the server:

```bash
curl -fsS http://localhost:8080/health
curl -fsS http://localhost:8080/ready
```

## Option 1: Docker (streamable HTTP)

Images are published to `ghcr.io/greenbone-hive/openvas-mcp` for Linux amd64
and arm64 under the immutable release tag only (`v<version>`, never
`latest`). Take the tag from the
[releases page](https://github.com/greenbone-hive/OpenVAS-MCP/releases).

```{note}
Packages of the `greenbone-hive` organization are private. Log in to GHCR
with a GitHub token that has the `read:packages` scope before pulling:
`echo "$GITHUB_TOKEN" | docker login ghcr.io -u <github-user> --password-stdin`.
```

The gateway from the compose stack listens on the host's loopback interface,
so a container reaches it through `host.docker.internal`. From the container's
point of view that is a remote host over plain http, which the server refuses
by default; `GVM_ALLOW_INSECURE_GATEWAY=true` acknowledges that the traffic
stays on this machine:

```bash
docker run --rm \
  --add-host=host.docker.internal:host-gateway \
  -e GVM_GATEWAY_URL=http://host.docker.internal:8080 \
  -e GVM_ALLOW_INSECURE_GATEWAY=true \
  -e GVM_USERNAME=admin \
  -e GVM_PASSWORD=secret \
  -p 127.0.0.1:8000:8000 \
  ghcr.io/greenbone-hive/openvas-mcp:vX.Y.Z
```

`--add-host` is needed on Linux; Docker Desktop on macOS and Windows resolves
`host.docker.internal` by itself. To run on the gateway's own compose network
instead, attach to it (`--network <project>_default`) and use
`GVM_GATEWAY_URL=http://gvm-gateway:8080`, again with
`GVM_ALLOW_INSECURE_GATEWAY=true`.

When the gateway runs on another machine, give it TLS (its own
`transport_security_mode = "native"` or a TLS-terminating proxy), use an
`https://` URL and drop the insecure flag; a private CA is trusted through
`GVM_GATEWAY_CA_CERT=/path/to/ca.pem` mounted into the container. See
[Configuration](configuration.md) for the details.

The image is distroless, runs as a non-root user and defaults to the
streamable-HTTP transport on `0.0.0.0:8000` with the Host-header check
disabled (`MCP_ALLOWED_HOSTS=*`) for container networks. Front it with a
reverse proxy if you expose it beyond localhost, and add
`-e MCP_REQUIRE_CALLER_AUTH=true` when several people share the endpoint so
that nobody borrows the fallback identity.

The MCP endpoint is `http://localhost:8000/mcp`. To verify the image's
Sigstore build provenance:

```bash
gh attestation verify oci://ghcr.io/greenbone-hive/openvas-mcp:vX.Y.Z --owner greenbone-hive
```

The repository's
[docker-compose.example.yml](https://github.com/greenbone-hive/OpenVAS-MCP/blob/main/docker-compose.example.yml)
runs the same container as a service; set `GVM_MCP_TAG` to a release tag to
pull the published image instead of building locally.

## Option 2: Release binary (stdio or HTTP)

Every release ships one archive per platform, each with a SHA-256 checksum
and Sigstore build provenance:

| Platform | Archive |
| --- | --- |
| Linux x86_64 (glibc) | `gvm-mcp-linux-amd64.tar.gz` |
| Linux x86_64 (musl, static) | `gvm-mcp-linux-amd64-musl.tar.gz` |
| Linux ARM64 | `gvm-mcp-linux-arm64.tar.gz` |
| macOS x86_64 | `gvm-mcp-macos-amd64.tar.gz` |
| macOS ARM64 | `gvm-mcp-macos-arm64.tar.gz` |

Download the archive for your platform from the
[releases page](https://github.com/greenbone-hive/OpenVAS-MCP/releases),
verify it, unpack it and place `gvm-mcp` on your `PATH`:

```bash
shasum -c gvm-mcp-linux-amd64.tar.gz.sha256
gh attestation verify gvm-mcp-linux-amd64.tar.gz --owner greenbone-hive
tar xzf gvm-mcp-linux-amd64.tar.gz
./gvm-mcp --version
```

Each release also ships a software bill of materials
(`openvas-mcp-sbom.tar.gz`, CycloneDX format) with its checksum.

The binary defaults to stdio, which is what desktop clients launch. For a
shared HTTP endpoint start it with `--transport streamable-http`; see
[Configuration](configuration.md) for the bind address and the Host guard.

## Option 3: Build from source

Requires Rust 1.90 or newer.

```bash
git clone https://github.com/greenbone-hive/OpenVAS-MCP.git
cd OpenVAS-MCP
cargo build --release
./target/release/gvm-mcp --help
```

## Next steps

- [Connect your MCP client](clients/index.md) and run `openvas_test_connection`.
- Choose the exposed surface with `--toolsets` and `--read-only`
  ([Toolsets](toolsets.md)).
- Review the identity model and HTTP hardening options
  ([Authentication and security](authentication.md)).
