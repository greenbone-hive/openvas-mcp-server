# gvm-mcp — MCP server for GVM/OpenVAS.
#
# This app speaks ONLY to the rust-gvm-api REST gateway (HTTP/JSON); it never
# talks to gvmd or GMP directly. A reachable gateway is a hard prerequisite.
#
#   docker build -t gvm-mcp .
#   docker run --rm \
#     -e GVM_GATEWAY_URL=http://gateway:8080 \
#     -e GVM_USERNAME=admin -e GVM_PASSWORD=secret \
#     -p 127.0.0.1:8000:8000 gvm-mcp

FROM rust:1-bookworm AS builder
WORKDIR /build

# Cache the dependency graph separately from the source: build a stub crate
# with the real manifests first so `cargo build` compiles dependencies, then
# swap in the real sources and rebuild only our crate.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src

COPY src ./src
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

# Distroless runtime: no shell or package manager, nonroot by default. rustls
# means no OpenSSL; the cc image provides glibc + libgcc + CA certificates.
FROM gcr.io/distroless/cc-debian12:nonroot

ARG GVM_MCP_VERSION=dev
ARG GVM_MCP_VCS_REF=local
LABEL org.opencontainers.image.title="openvas-mcp" \
      org.opencontainers.image.description="MCP server for Greenbone Vulnerability Management (GVM/OpenVAS) over the rust-gvm-api REST gateway" \
      org.opencontainers.image.source="https://github.com/greenbone-hive/OpenVAS-MCP" \
      org.opencontainers.image.licenses="AGPL-3.0-or-later" \
      org.opencontainers.image.version="${GVM_MCP_VERSION}" \
      org.opencontainers.image.revision="${GVM_MCP_VCS_REF}"

COPY --from=builder /build/target/release/gvm-mcp /gvm-mcp

# Streamable-HTTP defaults for container use. The Host-header (DNS-rebinding)
# guard is disabled inside the container network; put a reverse proxy in front
# if the endpoint is exposed beyond a trusted network.
ENV MCP_TRANSPORT=streamable-http \
    MCP_BIND_ADDR=0.0.0.0:8000 \
    MCP_ALLOWED_HOSTS=*

EXPOSE 8000
USER nonroot
ENTRYPOINT ["/gvm-mcp"]
