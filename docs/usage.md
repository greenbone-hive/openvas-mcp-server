# Workflows and prompts

This page shows what to ask and which tools the assistant will reach for.
Tool names are listed so that you can recognise them when the client asks for
confirmation; the complete parameter lists are in the
[Tool reference](reference/tools/index.md).

## Before anything else

> Run the OpenVAS connection test and tell me which gvmd version we have.

`openvas_test_connection` checks gateway liveness and readiness, the gvmd/GMP
version and that your identity is accepted; `openvas_get_version` returns the
versions alone. If this fails, nothing else will work; see
[Troubleshooting](troubleshooting.md).

## Filters and pagination

Most list tools accept the same three arguments:

- `filter`: a GMP filter expression, for example `name~web and severity>5`
  or `status=Done`
- `filter_id`: the UUID of a saved filter
- `page` / `per_page`: 1-indexed pagination (default 25 rows, at most 1000)

List tools return summarized rows plus a pagination envelope; the matching
`openvas_get_*` tool returns the full record. `openvas_search_nvts` is the
exception that uses typed selectors (`family`, `config_id`, `sort_field`, …)
instead of a filter expression.

Telling the assistant the filter you have in mind ("only hosts with
severity above 7", "tasks whose name starts with DMZ") produces one precise
call instead of a page-through.

## Running a scan

> Scan 192.168.1.0/24 with the *Full and fast* configuration and summarize
> the findings above severity 7 when it finishes.

What happens:

1. `openvas_list_port_lists` and `openvas_create_target`: gvmd requires a
   port list before a task can run, so the assistant picks one (typically
   *All IANA assigned TCP*) and creates the target with the hosts.
2. `openvas_list_scan_configs` and `openvas_list_scanners`: resolve the
   configuration and scanner UUIDs by name.
3. `openvas_create_task` binds target, configuration and scanner;
   `openvas_start_task` starts it and returns the report UUID of this run.
4. `openvas_get_task` is polled until the status is `Done`.
5. `openvas_get_report_results` with `filter: "severity>7"` lists the
   findings; `openvas_get_report_hosts`, `openvas_get_report_ports`,
   `openvas_get_report_cves` and the other drill-down pages answer follow-up
   questions about the same report.

Scans take minutes to hours. Ask for the task UUID and come back later:
"What is the status of task 3f2a…? Summarize its report when done."

Other task variants exist for agent groups (`type: "agentGroup"`), OCI
images (`ociImage`) and web applications (`webApplication`), each with its
own target type; `type: "import"` creates a container task that only receives
imported reports.

## Reading results

> Which hosts in the latest report of the DMZ task have the most
> high-severity results, and which CVEs are involved?

`openvas_list_tasks` (filter by name) gives the task and its last report
UUID; `openvas_get_report_hosts` ranks hosts by severity;
`openvas_get_report_cves` and `openvas_get_report_results` with a `filter`
narrow down to the vulnerabilities. For questions across reports use
`openvas_list_results`, `openvas_list_vulnerabilities` or the asset tools
(`openvas_list_asset_hosts`).

Security information is available without a scan: `openvas_get_cve`,
`openvas_list_cves`, `openvas_search_nvts` and `openvas_get_nvt` answer
"what does the feed know about CVE-2024-3094?" from the SecInfo catalogs,
and `openvas_list_feeds` shows how current the feeds are.

## Compliance audits

> Run the *IT-Grundschutz* policy against the web servers target and tell
> me the failed requirements.

1. `openvas_list_policies`: the feed ships built-in policies; copy one with
   `openvas_create_policy` when it needs tuning.
2. `openvas_create_audit` with the target, the policy (as `scan_config_id`)
   and a scanner.
3. `openvas_start_audit`, then `openvas_get_audit` until `Done`.
4. `openvas_get_report` on the returned report UUID; the compliance results
   are the report's findings.

Audits and policies never appear under tasks and scan configurations: the
gateway scopes the two families to disjoint usage types.

## Tuning a scan configuration

> Clone *Full and fast*, disable the *Brute force attacks* family and raise
> the timeout for the *Web Servers* family to 600 seconds.

gvmd requires every custom configuration to start as a copy, so the assistant
calls `openvas_create_scan_config` (copy of a base configuration), then
`openvas_set_scan_config_family_selection` and
`openvas_set_scan_config_nvt_selection` to change what is scanned, and
`openvas_set_scan_config_preference` (omit the value to reset to the default)
for individual settings. `openvas_list_scan_config_nvts` and
`openvas_list_scan_config_preferences` show the current state. Both selection
writes replace the whole selection they address, so ask for the current
selection first when you only want to change one family.

## Exporting and importing reports

> Export the latest DMZ report as PDF.

```text
openvas_list_report_formats  (find the PDF format UUID)
openvas_export_report        (report UUID + format; omit the format for JSON)
        │  job id, status: queued
        ▼
openvas_get_job              (poll until status: succeeded)
        │
        ▼
openvas_download_job_result  (JSON inline; PDF/CSV/XML as base64; both ≤ 3 MB)
```

Jobs and artifacts expire about 15 minutes after completion, and
`openvas_cancel_job` aborts a running export. For artifacts larger than 3 MB,
export with a narrower `filter` or page through the results instead. The
export runs on a gateway session bound to your identity; keep polling at
least every few minutes, because the gateway closes idle sessions (default
5 minutes) and cancels their jobs.

`openvas_import_report` accepts one GMP XML report envelope (≤ 10 MiB) into
an existing import task.

## Housekeeping and deletion

> Delete every task whose name starts with `tmp-` together with its target.

Delete tools move resources to gvmd's trashcan by default; `ultimate: true`
deletes permanently. Resources without a trashcan (reports, audits,
policies, assets, agents) take only an `id`. Deleting a
resource that is still referenced, for example a target used by a task,
surfaces the gateway's `409` with the reason, so the assistant deletes the
task first. Current gvmd builds refuse to delete an imported report; it is
retained with its import task.

For assistants that should never delete or start anything, run the server
with `--read-only`; the mutating tools are then not offered at all.

## Writing good prompts

- Name resources the way they are named in gvmd ("the *DMZ weekly* task");
  the assistant resolves names to UUIDs with the list tools.
- State the filter you want up front (severity, host, status, time range).
- For long-running work ask for the task or job UUID, then follow up.
- Keep separate assistants for triage (`--read-only`) and for operations
  that change the scanner, and let the client's confirmation dialog be the
  last check before a mutating tool runs.
