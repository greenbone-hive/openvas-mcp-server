# Toolsets

The full surface is 194 tools in 25 toolsets. Tools are grouped so that a
deployment exposes only what its users need; `gvm-mcp --list-toolsets`
prints the list below.

## Selection

`--toolsets` (`GVM_TOOLSETS`) accepts `default`, `all`, or a comma-separated
list of toolset names. `default` is every toolset except `identity`, which is
always opt-in. `--read-only` (`GVM_READ_ONLY=true`) additionally removes every
mutating tool from the server; clients do not see them at all.

```bash
gvm-mcp                                     # default selection
gvm-mcp --read-only                         # reads only
gvm-mcp --toolsets targets,tasks,reports    # scoped surface
gvm-mcp --toolsets default,identity         # include user administration
```

| Selection | Tools |
| --------- | ----: |
| `default` | 171 |
| `default`, read-only | 89 |
| `default,identity` | 194 |
| `default,identity`, read-only | 99 |

The `system` toolset (connection test, version, timezones) is always present.

## Reference

Every tool of every toolset, with its parameters, is listed in the generated
[Tool reference](reference/tools/index.md).

| Toolset | Scope |
| ------- | ----- |
| `system` | Connectivity, health, version and timezone information |
| `targets` | Scan targets, OCI image targets and web application targets |
| `tasks` | Scan tasks and their lifecycle (start, stop, resume, clone) |
| `scan-configs` | Scan configurations, NVT and family selection, preferences, generic configs |
| `scanners` | Scanner instances |
| `schedules` | Scan schedules |
| `credentials` | Authenticated-scan credentials and credential stores |
| `alerts` | Alerts and notifications |
| `port-lists` | Port lists |
| `results` | Individual scan results |
| `reports` | Reports, drill-down pages, imports and asynchronous exports |
| `assets` | Host, operating-system and generic assets |
| `tls-certificates` | TLS certificates discovered by scans |
| `report-formats` | Report formats |
| `filters` | Saved filters |
| `tags` | Tags on resources |
| `notes` | Notes on results |
| `overrides` | Severity overrides |
| `nvts` | NVTs and NVT families |
| `secinfo` | CVE, CPE, CERT-Bund and DFN-CERT catalogs |
| `vulnerabilities` | Vulnerability listing |
| `feeds` | Feed status |
| `compliance` | Compliance audits and policies |
| `agents` | Sensor agents and agent groups |
| `identity` | Users, groups, roles, permissions and user settings (opt-in) |

## Read-only mode

A tool is mutating when it creates, updates, deletes or clones a resource,
starts, stops or resumes a task or audit, writes a scan-configuration
selection or preference, imports a report, verifies a credential store or
triggers an agent sync. Every such tool is marked as mutating (and, where
applicable, destructive) in its MCP metadata, which is what `--read-only`
removes and what clients use to ask for confirmation.

## Tool conventions

- Names follow `openvas_<verb>_<resource>`, for example `openvas_list_tasks`,
  `openvas_get_report`, `openvas_create_target`.
- List tools return summarized rows plus a pagination envelope. Use the
  matching `openvas_get_*` tool for the full record. Filter and pagination
  arguments are described in [usage.md](usage.md).
- Delete tools move resources to the trashcan unless `ultimate: true` is
  passed; resources without a trashcan take only an `id`.

## Availability notes

- `identity` is never part of `default`; add it explicitly.
- Tickets are not exposed by the gateway.
- OCI image targets, web application targets, agents and credential stores
  depend on the connected gvmd build having those features enabled. Without
  them the gateway's problem response surfaces as a legible tool error.
