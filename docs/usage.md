# Usage

## First contact

Ask the model to run `openvas_test_connection`. It verifies, in order:
gateway liveness (`GET /health`), gateway readiness (`GET /ready`), the
gvmd/GMP version, and one authenticated call confirming the forwarded
identity is accepted. Each failure mode produces a distinct, actionable
message (gateway down vs. gvmd unreachable vs. bad credentials).

## Toolsets and gating

`gvm-mcp --list-toolsets` prints every toolset. Selection examples:

```bash
gvm-mcp                                     # default: everything except identity (171 tools)
gvm-mcp --read-only                         # reads only (89 tools)
gvm-mcp --toolsets targets,tasks,reports    # scoped surface
gvm-mcp --toolsets default,identity         # opt into user administration (194 tools)
```

`--read-only` removes every mutating tool from the router — clients do not
even see them in `tools/list`. Mutating means create/update/delete/clone,
task and audit start/stop/resume, scan-config selection and preference
writes, report import, credential-store verification and agent sync. The
`system` toolset (connection test, version, timezones) is always present.

## Filters and pagination

Most list tools accept:

- `filter` — a GMP filter expression, e.g. `name~web and severity>5`
- `filter_id` — UUID of a saved filter
- `page` / `per_page` — 1-indexed pagination (default 25, max 1000)

Exceptions follow the gateway contract: `openvas_search_nvts` uses typed
selectors (`family`, `config_id`, `include_preferences`, `sort_field`, …)
instead of a filter expression; NVT families and scan-config NVT listings
paginate only; user settings and credential stores filter only.

List tools return summarized rows plus a pagination envelope; use the
matching `openvas_get_*` tool for the full record.

## Typical scan flow

1. `openvas_create_target` — hosts, port list (gvmd requires one before a
   task can run), optional credentials
2. `openvas_list_scan_configs` / `openvas_list_scanners` — pick UUIDs
3. `openvas_create_task` — bind target + config + scanner
4. `openvas_start_task` — returns the report UUID for the run
5. `openvas_get_task` — status/progress until `Done`
6. `openvas_get_report_results` — findings, filterable; plus the hosts,
   ports, applications, CVEs, closed CVEs, errors, operating-systems and
   TLS-certificate pages of the same report

Other task variants: pass `type` (`agentGroup`, `ociImage`,
`webApplication`) with the matching selector (`agent_group_id`,
`oci_image_target_id`, `web_application_target_id`), or `type: "import"` to
create a container task for `openvas_import_report`.

## Compliance audits

1. `openvas_list_policies` — the feed ships built-in policies; copy one with
   `openvas_create_policy` to tune it
2. `openvas_create_audit` — target + policy (as `scan_config_id`) + scanner
3. `openvas_start_audit` → `openvas_get_audit` until `Done`
4. `openvas_get_report` on the returned report UUID

Audits and policies never appear under tasks and scan configs: the gateway
scopes the two families to disjoint usage types.

## Tuning a scan configuration

```text
openvas_create_scan_config          (copy of a base config, required by gvmd)
openvas_list_scan_config_nvts       (selected NVTs, optionally one family)
openvas_list_scan_config_preferences
openvas_set_scan_config_preference  (value omitted = reset to default)
openvas_set_scan_config_family_selection
openvas_set_scan_config_nvt_selection
```

The two selection writes replace the whole selection they address.

## Report exports and imports

```text
openvas_export_report        (report format UUID, or omit for native JSON)
        │  job id, status: queued
        ▼
openvas_get_job              (poll until status: succeeded)
        │
        ▼
openvas_download_job_result  (JSON inline; PDF/CSV/XML as base64; both ≤ 3 MB)
```

Jobs and artifacts expire ~15 minutes after completion; `openvas_cancel_job`
aborts a running export. For artifacts larger than 3 MB, export with a
narrower `filter` or page through the results instead.

The gateway binds a job to the session that created it, so `openvas_export_report`
opens a job-bound gateway session for callers that authenticate with `Basic`
credentials (including the stdio fallback) and closes it after the download,
on cancel, or when the job fails. Keep polling at least every few minutes:
the gateway closes idle sessions (default 5 minutes) and cancels their jobs.

`openvas_import_report` accepts one GMP XML report envelope (≤ 10 MiB) into
an existing import task.

## Deletion semantics

Delete tools move resources to gvmd's trashcan by default; pass
`ultimate: true` for permanent deletion. Resources without a trashcan in the
gateway contract (reports, audits, policies, host/OS/generic assets, agents)
take only an `id` — the gateway rejects `ultimate` on those routes. Deleting
a resource that is still referenced (e.g. a target used by a task) surfaces
the gateway's 409 with the reason.
