# Security Policy

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅ Current |

We support the latest minor release with security patches. Once a new minor or major version is published, prior versions receive patches only for critical vulnerabilities at maintainer discretion.

## Reporting a Vulnerability

**Please do not open public GitHub issues for security vulnerabilities.**

Instead, use **GitHub Private Vulnerability Reporting**:

1. Go to the [Security Advisories](https://github.com/greenbone-hive/openvas-mcp-server/security/advisories) tab
2. Click **"Report a vulnerability"**
3. Fill in the details — affected component(s), reproduction steps, and impact assessment

### What to expect

- **Acknowledgment** within 48 hours
- **Initial assessment** within 5 business days
- **Patch timeline** depends on severity:
  - **Critical / High**: Target fix within 7 days
  - **Medium**: Target fix within 30 days
  - **Low**: Next scheduled release
- We will coordinate disclosure timing with you. We follow [responsible disclosure](https://en.wikipedia.org/wiki/Coordinated_vulnerability_disclosure) practices.

### What qualifies

- Credential exposure in tool responses, logs, or headers (the server holds
  gvmd credentials and a gateway bearer token — neither must ever leak)
- Authentication/authorization bypass in the streamable-HTTP transport
  (e.g. Host-header/DNS-rebinding protection failures)
- Injection reaching the gateway or gvmd through unvalidated tool arguments
- Improper input validation leading to resource exhaustion (DoS)
- TLS/transport layer vulnerabilities in the gateway client
- Dependency vulnerabilities with a viable attack path through our code

### What doesn't qualify

- Issues in upstream dependencies without a demonstrated attack path through
  openvas-mcp-server
- Vulnerabilities in the rust-gvm-api gateway or gvmd themselves (report those
  to their respective projects)
- Rate limiting or brute-force concerns (expected to be handled by deployment
  infrastructure, e.g. a reverse proxy)

## Security Measures

### Dependency Auditing

- **[cargo-audit](https://github.com/rustsec/rustsec)** runs in CI on every push and weekly via the Security workflow
- **[cargo-machete](https://github.com/bnjbvr/cargo-machete)** checks for unused dependencies in CI
- **[Semgrep](https://semgrep.dev/)** static analysis (`p/rust`, `p/security-audit`, `p/secrets`) runs in the Security workflow
- **[Dependabot](https://docs.github.com/en/code-security/dependabot)** monitors Cargo, GitHub Actions and Docker dependencies with weekly update PRs

### Code Quality

- `cargo clippy` with `-D warnings` in CI
- `#![forbid(unsafe_code)]` — no unsafe blocks anywhere in the crate
- Credentials wrapped in `secrecy::SecretString`; redacted from `Debug` output and never returned in tool responses
- SBOM (CycloneDX) generated and attached to every release

### Authentication

- gvm-mcp holds no session and invents no auth of its own: it forwards a caller's gvmd identity to the rust-gvm-api gateway per request, and gvmd is the sole authority.
- **streamable HTTP**: the inbound `Authorization` header (a gateway session token or `Basic` gvmd credentials) is forwarded verbatim, so each caller authenticates as themselves and gvmd enforces their permissions. When absent, a fallback `Basic` credential (`GVM_USERNAME`/`GVM_PASSWORD`) is used; when neither is present the gateway answers `401`.
- `--require-caller-auth` (`MCP_REQUIRE_CALLER_AUTH=true`) refuses HTTP tool calls without an `Authorization` header instead of serving them with the fallback identity. The server warns at startup when fallback credentials are configured, the flag is off and the HTTP bind address is not loopback — in that configuration every network client of the MCP endpoint acts as the fallback gvmd user.
- **Export jobs** are the one piece of gateway state: the gateway cancels an export job when its creating session ends, so `openvas_export_report` opens a job-bound gateway session with the caller's own credentials. The token is held as a `SecretString`, keyed by job id, usable only by the identity that created the job, bounded to 64 concurrent entries, and deleted at the gateway as soon as the artifact is downloaded, the job is cancelled or it ends. Bearer callers' own sessions are used as-is.
- **stdio** has no network surface; it uses the configured fallback identity.
- `MCP_ALLOWED_HOSTS` is a DNS-rebinding guard, not authentication. Terminate TLS and, if a single shared identity is desired, authenticate at a reverse proxy before exposing the HTTP endpoint beyond a trusted network. No credential is cached as a session.

### Gateway client hardening

- Redirects are never followed, so a misconfigured or hostile intermediary cannot bounce the forwarded credentials to another host.
- Credentials embedded in `GVM_GATEWAY_URL` (`http://user:pass@host`) are rejected at startup; they would otherwise appear in logs and error messages.
- The fallback credential is held as a `SecretString` and redacted from the client's `Debug` output.
- Artifact downloads (report exports, agent support bundles) are streamed with a 3 MB cap and refused before being buffered; report imports are bounded to the gateway's 10 MiB limit client-side.
- Caller-supplied identifiers are sent as single percent-encoded path segments; they cannot escape the addressed route or inject query parameters.
- Request bodies contain only keys the gateway contract lists (gateway bodies reject unknown fields), and the `ultimate` delete option is offered only on routes that document it.

## Changelog

| Date | Change |
|------|--------|
| 2026-08-13 | Initial security policy |
| 2026-10-31 | Gateway client hardening (no redirects, URL-credential rejection, artifact cap), `--require-caller-auth` for HTTP deployments |
