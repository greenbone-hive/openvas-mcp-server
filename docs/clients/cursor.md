# Cursor

Cursor reads MCP servers from `~/.cursor/mcp.json` (all projects) or
`.cursor/mcp.json` (one project).

## Local server (stdio)

```json
{
  "mcpServers": {
    "openvas": {
      "command": "/path/to/gvm-mcp",
      "args": [],
      "env": {
        "GVM_GATEWAY_URL": "http://localhost:8080",
        "GVM_USERNAME": "admin",
        "GVM_PASSWORD": "secret"
      }
    }
  }
}
```

Put server flags such as `--read-only` or `--toolsets targets,tasks,reports`
into `args`.

## Shared server (streamable HTTP)

```json
{
  "mcpServers": {
    "openvas": {
      "url": "http://localhost:8000/mcp",
      "headers": {
        "Authorization": "Basic YWRtaW46c2VjcmV0"
      }
    }
  }
}
```

`headers` is optional; it makes the calls run as your own gvmd user instead
of the server's fallback identity.

## Verify

*Settings → MCP* shows the server and its tools. In the agent chat ask: "Run
the OpenVAS connection test."
