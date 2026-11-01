# Claude Code

Claude Code registers MCP servers with `claude mcp add`, either for the
current project or for the user, and supports both transports.

## Local server (stdio)

```bash
claude mcp add openvas \
  -e GVM_GATEWAY_URL=http://localhost:8080 \
  -e GVM_USERNAME=admin \
  -e GVM_PASSWORD=secret \
  -- /path/to/gvm-mcp
```

Add `--scope user` to make the server available in every project. Server
flags go after the binary, for example `-- /path/to/gvm-mcp --read-only`.

## Shared server (streamable HTTP)

Connect to a running server and authenticate as yourself towards gvmd with a
`Basic` header:

```bash
claude mcp add --transport http openvas http://localhost:8000/mcp \
  --header "Authorization: Basic $(printf 'admin:secret' | base64)"
```

Without `--header` the server's fallback identity is used.

## Team configuration

The same settings can be committed for a team in `.mcp.json` at the project
root; Claude Code asks each member to approve it once:

```json
{
  "mcpServers": {
    "openvas": {
      "type": "http",
      "url": "http://localhost:8000/mcp"
    }
  }
}
```

A stdio entry uses `"command"`, `"args"` and `"env"` instead of `"type"` and
`"url"`.

## Verify

`/mcp` inside Claude Code lists the server and its connection state. Then
ask: "Run the OpenVAS connection test."
