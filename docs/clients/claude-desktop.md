# Claude Desktop

Claude Desktop launches local MCP servers over stdio from its configuration
file and connects to remote streamable-HTTP servers as custom connectors.

## Local server (stdio)

Edit `claude_desktop_config.json`:

- macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
- Windows: `%APPDATA%\Claude\claude_desktop_config.json`

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

Restart Claude Desktop. The tools appear under the connector icon in the chat
input; the `env` block is the fallback identity every call runs as. Add
`"args": ["--read-only"]` for a reporting-only assistant, or
`"args": ["--toolsets", "targets,tasks,reports"]` to narrow the surface.

## Shared server (streamable HTTP)

Start the server with the Docker image or `--transport streamable-http`, then
add it in *Settings → Connectors → Add custom connector* with the URL
`http://<host>:8000/mcp`. Claude Desktop sends no gvmd credentials itself, so
the server's fallback identity is used; keep the endpoint on a trusted
network.

## Verify

Ask: "Run the OpenVAS connection test." The `openvas_test_connection` result
reports gateway liveness and readiness, the gvmd version and whether the
identity was accepted.
