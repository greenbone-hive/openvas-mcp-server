# Troubleshooting

Start every investigation with `openvas_test_connection`. Its four checks
(gateway liveness, gateway readiness, gvmd version, authenticated call) tell
you which layer fails.

## The client does not list any tools

- The binary is not where the client config points to, or it is not
  executable. Run `/path/to/gvm-mcp --list-toolsets` in a shell first.
- The server logs to stderr; most clients show it in an MCP output panel.
  Start-up errors such as an unknown toolset name or an empty
  `GVM_PASSWORD_FILE` appear there.
- A streamable-HTTP client needs the full endpoint `http://<host>:8000/mcp`,
  not the origin alone.

## "Gateway unreachable" or connection refused

- Check `GVM_GATEWAY_URL`; it is the gateway origin without `/api/v1`.
- The compose stack publishes the gateway on `127.0.0.1:8080` of the host.
  From a container that address is the container itself; use
  `http://host.docker.internal:8080` (with
  `--add-host=host.docker.internal:host-gateway` on Linux) or join the
  gateway's compose network and use `http://gvm-gateway:8080`.
- `curl -fsS http://localhost:8080/ready` fails while gvmd is still starting
  or its socket is not mounted into the gateway container.

## Every tool answers 401

- No identity reached the gateway: for stdio clients set `GVM_USERNAME` and
  `GVM_PASSWORD` (or `GVM_PASSWORD_FILE`); for HTTP clients send an
  `Authorization` header or configure the fallback identity on the server.
- With `MCP_REQUIRE_CALLER_AUTH=true` HTTP calls without `Authorization` are
  refused by design.
- The credentials are gvmd credentials, not gateway or operating-system
  accounts.

## A tool I expect is missing

- It belongs to a toolset that is not selected; `identity` is opt-in and
  must be named explicitly (`--toolsets default,identity`).
- The server runs with `--read-only`, which removes every mutating tool.
- Tickets are not exposed by the gateway.

## The HTTP server rejects requests with a Host error

The DNS-rebinding guard only accepts the `Host` values in
`MCP_ALLOWED_HOSTS` (default `localhost,127.0.0.1,::1`). Add the hostname the
client uses, or set `*` behind a reverse proxy that validates `Host` itself.

## Export jobs expire or downloads fail

- Jobs and artifacts are deleted about 15 minutes after completion; poll
  `openvas_get_job` and download promptly.
- The gateway closes idle sessions after 5 minutes and cancels their jobs;
  keep polling at least every few minutes.
- Artifacts above 3 MB are refused. Export with a narrower `filter` or read
  the results through the report pages instead.

## The gateway answers 404, 409 or 502

- `404`: the UUID does not exist or belongs to a resource of another type.
- `409`: the resource is still referenced (a target used by a task, a
  credential used by a target). Remove the reference first.
- `502 backend_unavailable`: the connected gvmd lacks the feature (OCI image
  targets, web application targets, agents, credential stores) or is not
  running.

## docker pull is denied

Packages of the `greenbone-hive` organization are private. Log in with a
token that has `read:packages`:

```bash
echo "$GITHUB_TOKEN" | docker login ghcr.io -u <github-user> --password-stdin
```

Only immutable `v<version>` tags exist; there is no `latest`.

## Slow or timed-out calls

Large reports and feed-wide NVT searches can exceed the default 30-second
gateway timeout. Raise `GVM_HTTP_TIMEOUT`, narrow the filter or reduce
`per_page`. `GVM_LOG_LEVEL=debug` shows each gateway request and its
duration on stderr.
