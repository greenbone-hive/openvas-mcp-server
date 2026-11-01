# Connecting an MCP client

`gvm-mcp` speaks the Model Context Protocol over two transports:

- **stdio**: the client launches the binary itself. This is the default and
  what desktop assistants and IDEs expect. The fallback identity
  (`GVM_USERNAME` / `GVM_PASSWORD`) is the gvmd user for every call.
- **streamable HTTP**: one long-running server (the Docker image uses this
  transport) that several clients share at `http://<host>:8000/mcp`. Each
  client may send its own `Authorization` header and is then that gvmd user;
  clients that send none use the fallback identity, unless
  `MCP_REQUIRE_CALLER_AUTH=true` forbids that.

Pick your client:

```{toctree}
:maxdepth: 1

claude-desktop
claude-code
vscode
cursor
windsurf
other
```

In every snippet replace `/path/to/gvm-mcp` with the binary from a
[release](../installation.md) and adjust the gateway URL and account. Secrets
are better kept in `GVM_PASSWORD_FILE` or in the client's own secret input
than in a plain config file; see [Configuration](../configuration.md).

## First contact

Ask the assistant to run `openvas_test_connection`. It verifies, in order,
gateway liveness (`GET /health`), gateway readiness (`GET /ready`), the
gvmd/GMP version and one authenticated call confirming that the forwarded
identity is accepted. Each failure produces a distinct, actionable message
(gateway down, gvmd unreachable, bad credentials), so this one call separates
networking problems from account problems.

## Scoping what a client sees

The client only ever sees the tools the server was started with:

- `--toolsets targets,tasks,reports` (`GVM_TOOLSETS`) limits the surface to
  what a workflow needs and keeps prompts focused.
- `--read-only` (`GVM_READ_ONLY=true`) removes every mutating tool, which is
  the right default for reporting and triage assistants.
- the `identity` toolset (users, roles, permissions) is never enabled unless
  named explicitly.

See [Toolsets](../toolsets.md) for the complete list and counts.
