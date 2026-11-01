# Other clients and the Inspector

Every MCP client that supports stdio or streamable HTTP works with
`gvm-mcp`. The patterns are always the same:

- **stdio**: the client runs `/path/to/gvm-mcp` with the `GVM_*` variables in
  its environment; server flags are passed as arguments.
- **streamable HTTP**: the client connects to `http://<host>:8000/mcp` and,
  if it can send headers, authenticates as a gvmd user with
  `Authorization: Basic base64(user:password)`.

## MCP Inspector

To look at the server without an assistant, use the MCP Inspector. It lists
the tools with their schemas and lets you call them by hand, which is the
quickest way to check a toolset selection or a `--read-only` deployment:

```bash
GVM_GATEWAY_URL=http://localhost:8080 GVM_USERNAME=admin GVM_PASSWORD=secret \
  npx @modelcontextprotocol/inspector /path/to/gvm-mcp
```

For a running HTTP server choose the *Streamable HTTP* transport in the
Inspector and enter `http://localhost:8000/mcp`.

## Your own agent

Frameworks with MCP support (the official MCP SDKs, LangChain, the OpenAI
Agents SDK and others) connect the same way. Start one `gvm-mcp` per identity
over stdio, or share one HTTP server and pass each user's `Authorization`
header so that gvmd's permissions and audit trail stay per person.
