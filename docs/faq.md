# Frequently asked questions

**Does the server talk to gvmd directly?**
No. It only calls the rust-gvm-api REST gateway over HTTP/JSON; the gateway
speaks GMP to gvmd. A reachable gateway is the only hard dependency.

**Does it store credentials or sessions?**
No. The fallback identity is kept in memory and sent as a `Basic` header per
request; forwarded client headers pass through verbatim. The one exception,
a short-lived gateway session for report export jobs, is bound to the
identity that created the job and deleted when the job ends.

**Can several people share one HTTP server?**
Yes. Each client sends its own `Authorization` header and acts as that gvmd
user, so gvmd's permissions and audit trail stay per person. Set
`MCP_REQUIRE_CALLER_AUTH=true` so that nobody falls back to the shared
identity.

**Which gvmd versions are supported?**
Whatever the connected gateway supports. The current release targets the
rust-gvm-api gateway 0.5 in front of gvmd 26.43.0 / GMP 22.8.

**How do I keep an assistant from changing anything?**
Start the server with `--read-only`. The mutating tools are then not
offered; clients cannot call what they cannot see.

**What is the difference between `default` and `all`?**
None. Both select every toolset except `identity`, which must always be named
explicitly.

**Why is there no `latest` image tag?**
Releases publish only immutable `v<version>` tags together with Sigstore
provenance, so a deployment always states exactly what it runs.

**How large can an exported report be?**
Downloads are capped at 3 MB. Export with a filter, choose a lighter format or
page through the report's results.

**Are tickets supported?**
No. The gateway does not expose tickets.

**Where do I report a security issue?**
Privately, as described in the
[security policy](https://github.com/greenbone-hive/OpenVAS-MCP/blob/main/SECURITY.md).
