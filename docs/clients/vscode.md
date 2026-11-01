# Visual Studio Code

GitHub Copilot's agent mode reads MCP servers from `.vscode/mcp.json` in the
workspace (or from the user settings for all workspaces).

## Local server (stdio)

The `inputs` block asks for the password once and keeps it out of the file:

```json
{
  "inputs": [
    {
      "id": "gvm-password",
      "type": "promptString",
      "description": "gvmd password",
      "password": true
    }
  ],
  "servers": {
    "openvas": {
      "type": "stdio",
      "command": "/path/to/gvm-mcp",
      "env": {
        "GVM_GATEWAY_URL": "http://localhost:8080",
        "GVM_USERNAME": "admin",
        "GVM_PASSWORD": "${input:gvm-password}"
      }
    }
  }
}
```

Add `"args": ["--read-only"]` for an assistant that must not change the
scanner.

## Shared server (streamable HTTP)

```json
{
  "servers": {
    "openvas": {
      "type": "http",
      "url": "http://localhost:8000/mcp",
      "headers": {
        "Authorization": "Basic YWRtaW46c2VjcmV0"
      }
    }
  }
}
```

`headers` is optional; with it you act as your own gvmd user, without it the
server's fallback identity is used. The value is `base64("user:password")`.

## Verify

Start the server from the *MCP Servers* view or the code lens above the
entry, open Copilot Chat in agent mode and ask: "Run the OpenVAS connection
test."
