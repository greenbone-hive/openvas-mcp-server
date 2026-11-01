# Authentication and security

`gvm-mcp` invents no authentication of its own. It forwards a caller's gvmd
identity to the rust-gvm-api gateway, and gvmd is the sole authority for
authorization and auditing. What the server keeps is a short-lived gateway
session per identity, described below; it never stores passwords beyond the
configured fallback.

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
gateway answers `401`, which surfaces as a legible tool error.

## Gateway sessions

In the default `session` mode a `Basic` identity (the fallback or a forwarded
`Basic` header) is logged into the gateway once with `POST /api/v1/session`,
and its requests then carry the returned bearer token:

- The pool is keyed by a SHA-256 digest of the `Authorization` value, so a
  forwarded password is never retained; a token is only ever used for the
  identity whose credential created it.
- Tokens are kept as secrets in memory, never logged, and closed at the
  gateway after 60 seconds without use, when the server shuts down (also on
  `SIGTERM`), or when an identity is evicted from the bounded pool
  (64 identities, least recently used first). A conversation therefore costs
  one login per burst of activity, and an idle server holds no session.
- A session whose gvmd connection the gateway lost (gvmd restarted, or the
  connected gvmd dropped the connection on a command it does not support)
  is closed as soon as a call fails with it; reads are retried on a fresh
  session right away, writes report the error once and the next call logs
  in again.
- A session is treated as expired shortly before the gateway's idle timeout
  (5 minutes by default) and replaced after one hour at the latest. If the
  gateway rejects a token, the session is re-created once and the call
  retried; a second rejection is reported as the credential error it is.
- If the gateway refuses to create a session (its session or rate limits, or
  an older gateway without the endpoint), calls fall back to request-scoped
  `Basic` authentication for a short while.

Forwarded bearer tokens are passed through unchanged. `per-request` mode
(`GVM_GATEWAY_AUTH_MODE=per-request`) sends the `Basic` header on every call
instead; the gateway then opens and closes a gvmd session around each one.

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

## Transport to the gateway

The connection to the gateway normally crosses a network. Use `https://` and,
for a private CA, `GVM_GATEWAY_CA_CERT`; the client requires TLS 1.2 or newer
and never follows redirects. A plain `http://` gateway URL is refused unless
its host is loopback or `GVM_ALLOW_INSECURE_GATEWAY=true` acknowledges that the
network is isolated (for example a Docker network); the server then logs a
warning at start-up.

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
