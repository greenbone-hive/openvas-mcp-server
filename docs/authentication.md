# Authentication and security

`gvm-mcp` holds no session and invents no authentication of its own. It
forwards a caller's gvmd identity to the rust-gvm-api gateway, and gvmd is the
sole authority for authorization and auditing.

## Identity resolution

The `Authorization` value for each gateway request is resolved per call:

1. The inbound MCP request's `Authorization` header, when present. Every
   streamable-HTTP caller can therefore authenticate as themselves, with
   `Basic` gvmd credentials or a gateway session token. The header is
   forwarded verbatim.
2. Otherwise a `Basic` header built from `GVM_USERNAME` / `GVM_PASSWORD`
   (or `GVM_PASSWORD_FILE`). This fallback identity serves stdio clients and
   HTTP callers that send no credentials.

When neither is available the request goes out unauthenticated and the
gateway answers `401`, which surfaces as a legible tool error. No credential
is cached as a session; a `Basic` header is rebuilt per request from the
in-memory configuration.

The fallback identity is optional. Omit `GVM_USERNAME` / `GVM_PASSWORD` to
require every HTTP caller to authenticate as themselves, or keep them for
stdio and set `MCP_REQUIRE_CALLER_AUTH=true` so that HTTP callers can never
borrow them.

## Report export sessions

Asynchronous report exports are the one exception to "no session". The
gateway binds an export job to the session that created it and cancels the
job when that session ends; request-scoped `Basic` sessions end with the
request.

`openvas_export_report` therefore opens one gateway session with the caller's
own credentials, runs the job on it, serves `openvas_get_job` and
`openvas_download_job_result` through it, and deletes it as soon as the
artifact is downloaded, the job is cancelled or the job reaches a terminal
state. The session is usable only by the identity that created the job.
Callers that forward their own gateway bearer token are passed through
unchanged. Keep polling at least every few minutes: the gateway closes idle
sessions and cancels their jobs.

## Hardening for HTTP deployments

- `--require-caller-auth` (`MCP_REQUIRE_CALLER_AUTH=true`) refuses HTTP tool
  calls that carry no `Authorization` header instead of serving them with the
  fallback identity. The server logs a warning at startup when fallback
  credentials are configured, the flag is off and the bind address is not
  loopback.
- `--allowed-hosts` (`MCP_ALLOWED_HOSTS`) is a DNS-rebinding guard, not
  authentication. For a public deployment, terminate TLS and authenticate at a
  reverse proxy in front of the server.
- The Docker image binds `0.0.0.0:8000` and disables the Host check for
  container networks. When you pass `GVM_USERNAME` / `GVM_PASSWORD` to it,
  also pass `MCP_REQUIRE_CALLER_AUTH=true` or keep the port on a private
  network.
- The gateway client never follows redirects, rejects credentials embedded in
  `GVM_GATEWAY_URL`, never writes credentials to its logs and caps downloaded
  artifacts (report exports, agent support bundles) at 3 MB.

## Secrets

Prefer `GVM_PASSWORD_FILE` (a mounted secret file) over `GVM_PASSWORD` in
production; it takes precedence when both are set. Passwords and forwarded
tokens never appear in logs or in MCP responses.

Vulnerabilities in this project are reported privately as described in
[SECURITY.md](https://github.com/greenbone-hive/OpenVAS-MCP/blob/main/SECURITY.md).
