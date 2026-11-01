# Windsurf

Windsurf (Cascade) reads MCP servers from
`~/.codeium/windsurf/mcp_config.json`.

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

## Shared server (streamable HTTP)

```json
{
  "mcpServers": {
    "openvas": {
      "serverUrl": "http://localhost:8000/mcp"
    }
  }
}
```

The server's fallback identity is used for every call; keep the endpoint on a
trusted network or put an authenticating proxy in front of it.

## Verify

Refresh the server list in Cascade's MCP settings, then ask: "Run the OpenVAS
connection test."
