# OpenVAS-MCP

OpenVAS-MCP (`gvm-mcp`) is a [Model Context Protocol](https://modelcontextprotocol.io)
server for Greenbone Vulnerability Management (OpenVAS). It lets an AI
assistant such as Claude, GitHub Copilot or Cursor operate your scanner in
plain language: create targets, run scans and compliance audits, read reports,
look up CVEs and tune scan configurations through 194 tools that mirror the
gvmd API one to one.

The server talks only to the [rust-gvm-api](https://github.com/greenbone-hive/rust-gvm-api)
REST gateway. It never opens a gvmd session of its own and forwards each
caller's gvmd identity, so gvmd stays the authority for permissions and
auditing.

```text
MCP client (Claude, IDEs, agents)
      │  MCP over stdio or streamable HTTP
      ▼
    gvm-mcp
      │  HTTP/JSON, forwards the caller's gvmd identity
      ▼
rust-gvm-api REST gateway        /api/v1
      │  GMP over Unix socket
      ▼
    gvmd
```

## What you can ask

- "Check that OpenVAS is reachable and tell me the gvmd version."
- "Scan 192.168.1.0/24 with *Full and fast* and summarize the findings above
  severity 7 when it finishes."
- "Run the *IT-Grundschutz* policy against the web servers target."
- "Export the latest report of the DMZ task as PDF."
- "Which hosts in the last report still have CVE-2024-3094 open?"

Every request turns into tool calls the assistant shows you before running
them. Mutating tools can be removed from the server entirely with
`--read-only`, and the set of exposed tools is chosen per deployment with
[toolsets](toolsets.md).

## Quick start

1. Run the [rust-gvm-api gateway](https://github.com/greenbone-hive/rust-gvm-api)
   in front of your gvmd (its `compose.yaml` starts a complete stack) and note
   its URL, typically `http://localhost:8080`.
2. Start the server with the [Docker image or a release binary](installation.md).
3. [Connect your MCP client](clients/index.md) over stdio or streamable HTTP.
4. Ask the assistant to run `openvas_test_connection`. It checks gateway
   liveness and readiness, the gvmd version and that your identity is
   accepted.

## Requirements

- the rust-gvm-api gateway (0.5 or newer) in front of gvmd 26.43.0 / GMP 22.8
- a gvmd account for each user, or one fallback account for stdio clients
- any MCP client; Docker or a supported platform (Linux x86_64/arm64, macOS)
  for the server itself

```{toctree}
:hidden:

installation
clients/index
configuration
usage
toolsets
authentication
reference/tools/index
troubleshooting
faq
releases/index
```
