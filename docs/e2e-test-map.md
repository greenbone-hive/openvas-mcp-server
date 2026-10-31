# End-to-end test map

Produced by `tests/e2e_live.rs` against a live rust-gvm-api gateway (main, spec 0.5.0) in front of gvmd 26.43.0 / GMP 22.8, using the gateway repository's own `compose.yaml` published on `http://127.0.0.1:18080` (admin/admin, scan target host `openvasd`). Every row is one gateway operation exercised through the MCP tool named in the row, exactly as an MCP client would call it; every resource the suite creates is deleted again.

Run it yourself:

```bash
export GVM_E2E_GATEWAY_URL=http://127.0.0.1:18080 GVM_USERNAME=admin GVM_PASSWORD=admin
export GVM_E2E_TARGET_HOST=openvasd GVM_E2E_MATRIX_OUT=/tmp/e2e-matrix.md
GVM_E2E_SCAN=1 cargo test --test e2e_live -- --ignored --test-threads=1
```

## Outcome summary

| Outcome | Rows | Meaning |
| --- | --- | --- |
| PASS | 186 | tool call succeeded (or failed exactly as the contract requires, noted in the row) |
| SKIP | 36 | endpoint reachable, but this stack has no data for it or the connected gvmd lacks the feature (501/`backend_unavailable`) |
| GATEWAY-DEFECT | 3 | the tool sends exactly what the gateway contract specifies and the gateway/gvmd answers wrongly; tracked below |
| **Total** | **225** | |

No row is a tool-side failure.

## Gateway-side findings (reproducible with curl, independent of this server)

1. `GET /api/v1/scan-configs/{id}/preferences/{name}` answers `404 preference … not found` for every preference that `GET …/preferences` on the same config lists — scanner-level and NVT-level alike, with and without `nvtOid`. `PUT` on the same name succeeds. The gateway's `get_scan_config_preference` issues a `GetScanConfigPreferenceRequest` that gvmd 26.43 does not resolve; the list request (`GetScanConfigPreferencesRequest`) works.
2. `GET /api/v1/assets?type=tls_certificate` is rejected by gvmd with `Failed to find type 'tls_certificate'` (404), although the contract's `AssetType` enum lists the value. `host` and `os` work.
3. Feature-gated gvmd commands (OCI image targets, agents on a build without `GVMD_ENABLE_AGENTS`) come back from gvmd as `503 Service unavailable: Command disabled` and the gateway maps that to `502 backend_unavailable` instead of `501 Not Implemented`; clients cannot distinguish "feature off" from "gvmd down".
4. Request-scoped `Basic` authentication tears the gateway session down at the end of each request, and the gateway cancels export jobs bound to a deleted session — so a `POST /reports/{id}/exports` made with Basic credentials always yields a `cancelled` job. gvm-mcp works around this with a job-bound session (`src/gateway/jobs.rs`); the gateway could instead keep request-scoped sessions alive while they own non-terminal jobs, or document that exports require a bearer session.
5. `DELETE /audits/{id}` is non-ultimate only; the trashed audit keeps its target "in use", so the target can only be trashed (not ultimately deleted) afterwards.

## Coverage by scenario

### Bootstrap and read catalogs

| Method | Gateway path | MCP tool | Outcome |
| --- | --- | --- | --- |
| GET | `/health + /ready + /version + /targets` | `openvas_test_connection` | PASS |
| GET | `/version` | `openvas_get_version` | PASS |
| GET | `/timezones` | `openvas_list_timezones` | PASS |
| GET | `/feeds` | `openvas_list_feeds` | PASS |
| GET | `/feeds?type=NVT` | `openvas_list_feeds` | PASS |
| GET | `/scan-configs` | `openvas_list_scan_configs` | PASS |
| GET | `/scanners` | `openvas_list_scanners` | PASS |
| GET | `/port-lists` | `openvas_list_port_lists` | PASS |
| GET | `/report-formats` | `openvas_list_report_formats` | PASS |
| GET | `/scan-configs/{id}` | `openvas_get_scan_config` | PASS |
| GET | `/scanners/{id}` | `openvas_get_scanner` | PASS |
| GET | `/port-lists/{id}` | `openvas_get_port_list` | PASS |
| GET | `/report-formats/{id}` | `openvas_get_report_format` | PASS |
| GET | `/scan-configs/{id}/nvts` | `openvas_list_scan_config_nvts` | PASS |
| GET | `/scan-configs/{id}/nvts/{oid}` | `openvas_get_scan_config_nvt` | PASS |
| GET | `/scan-configs/{id}/preferences` | `openvas_list_scan_config_preferences` | PASS |
| GET | `/scan-configs/{id}/preferences/{name}` | `openvas_get_scan_config_preference` | GATEWAY-DEFECT (404 for a preference returned by the list; PUT on the same name works) |
| GET | `/scan-configs/{id}/preferences/{name}?nvtOid=` | `openvas_get_scan_config_preference` | GATEWAY-DEFECT (404 for a preference returned by the list; PUT on the same name works) |
| GET | `/configs?usageType=scan` | `openvas_list_configs` | PASS |
| GET | `/configs/{id}` | `openvas_get_config` | PASS |
| GET | `/nvt-families` | `openvas_list_nvt_families` | PASS |
| GET | `/nvts?family=&sortOrder=&sortField=` | `openvas_search_nvts` | PASS |
| GET | `/nvts/{id}` | `openvas_get_nvt` | PASS |
| GET | `/nvts?configId=&includePreferences=` | `openvas_search_nvts` | PASS |
| GET | `/targets` | `list_targets` | PASS |
| GET | `/targets/{id}` | `get_target` | SKIP (no rows in this stack) |
| GET | `/tasks` | `list_tasks` | PASS |
| GET | `/tasks/{id}` | `get_task` | SKIP (no rows in this stack) |
| GET | `/credentials` | `list_credentials` | PASS |
| GET | `/credentials/{id}` | `get_credential` | SKIP (no rows in this stack) |
| GET | `/alerts` | `list_alerts` | PASS |
| GET | `/alerts/{id}` | `get_alert` | SKIP (no rows in this stack) |
| GET | `/schedules` | `list_schedules` | PASS |
| GET | `/schedules/{id}` | `get_schedule` | SKIP (no rows in this stack) |
| GET | `/filters` | `list_filters` | PASS |
| GET | `/filters/{id}` | `get_filter` | SKIP (no rows in this stack) |
| GET | `/tags` | `list_tags` | PASS |
| GET | `/tags/{id}` | `get_tag` | SKIP (no rows in this stack) |
| GET | `/notes` | `list_notes` | PASS |
| GET | `/notes/{id}` | `get_note` | PASS |
| GET | `/overrides` | `list_overrides` | PASS |
| GET | `/overrides/{id}` | `get_override` | PASS |
| GET | `/reports` | `openvas_list_reports` | PASS |
| GET | `/reports/{id}` | `openvas_get_report` | SKIP (no reports yet) |
| GET | `/results` | `list_results` | PASS |
| GET | `/results/{id}` | `get_result` | SKIP (no rows in this stack) |
| GET | `/hosts` | `list_asset_hosts` | PASS |
| GET | `/hosts/{id}` | `get_asset_host` | SKIP (no rows in this stack) |
| GET | `/operating-systems` | `list_operating_systems` | PASS |
| GET | `/operating-systems/{id}` | `get_operating_system` | SKIP (no rows in this stack) |
| GET | `/tls-certificates` | `list_tls_certificates` | PASS |
| GET | `/tls-certificates/{id}` | `get_tls_certificate` | SKIP (no rows in this stack) |
| GET | `/policies` | `list_policies` | PASS |
| GET | `/policies/{id}` | `get_policy` | PASS |
| GET | `/audits` | `list_audits` | PASS |
| GET | `/audits/{id}` | `get_audit` | SKIP (no rows in this stack) |
| GET | `/vulnerabilities` | `openvas_list_vulnerabilities` | PASS |
| GET | `/cves` | `list_cves` | PASS |
| GET | `/cves/{id}` | `get_cve` | SKIP (feed not loaded in this stack) |
| GET | `/cpes` | `list_cpes` | PASS |
| GET | `/cpes/{id}` | `get_cpe` | SKIP (feed not loaded in this stack) |
| GET | `/cert-bund-advisories` | `list_cert_bund_advisories` | PASS |
| GET | `/cert-bund-advisories/{id}` | `get_cert_bund_advisory` | SKIP (feed not loaded in this stack) |
| GET | `/dfn-cert-advisories` | `list_dfn_cert_advisories` | PASS |
| GET | `/dfn-cert-advisories/{id}` | `get_dfn_cert_advisory` | SKIP (feed not loaded in this stack) |
| GET | `/assets?type=` | `openvas_list_assets` | PASS |
| GET | `/assets?type=` | `openvas_list_assets` | PASS |
| GET | `/assets?type=tls_certificate` | `openvas_list_assets` | GATEWAY-DEFECT (spec lists the type; gvmd rejects it with 404) |
| GET | `/oci-image-targets` | `openvas_list_oci_image_targets` | SKIP (capability) Gateway cannot reach gvmd while listing OCI image targets: Bad Gateway (backend_unavailable, HTTP 502): The backend service is unavailable. |
| GET | `/web-application-targets` | `openvas_list_web_application_targets` | SKIP (capability) Gateway cannot reach gvmd while listing web application targets: Bad Gateway (backend_unavailable, HTTP 502): The backend service is unavail |
| GET | `/agents` | `openvas_list_agents` | SKIP (capability) Gateway cannot reach gvmd while listing agents: Bad Gateway (backend_unavailable, HTTP 502): The backend service is unavailable. |
| GET | `/agents/{id}` | `openvas_get_agent` | SKIP (no agents enrolled) |
| GET | `/agents/{id}/support-bundle` | `openvas_get_agent_support_bundle` | SKIP (no agents enrolled) |
| GET | `/agent-groups` | `openvas_list_agent_groups` | SKIP (capability) Gateway cannot reach gvmd while listing agent groups: Bad Gateway (backend_unavailable, HTTP 502): The backend service is unavailable. |
| GET | `/scanners/{id}/agent-installer-instruction` | `openvas_get_agent_installer_instruction` | SKIP (capability) Gateway cannot reach gvmd while fetching the agent installer instruction: Bad Gateway (backend_unavailable, HTTP 502): The backend service i |
| GET | `/credential-stores` | `openvas_list_credential_stores` | SKIP (capability) Not supported by the connected gvmd while listing credential stores: Not Implemented (not_implemented, HTTP 501): credential stores are not  |

### Resource lifecycles (create → update → clone → delete)

| Method | Gateway path | MCP tool | Outcome |
| --- | --- | --- | --- |
| GET | `/scan-configs` | `openvas_list_scan_configs` | PASS |
| GET | `/scanners` | `openvas_list_scanners` | PASS |
| GET | `/port-lists` | `openvas_list_port_lists` | PASS |
| GET | `/report-formats` | `openvas_list_report_formats` | PASS |
| POST | `/port-lists` | `openvas_create_port_list` | PASS |
| PUT | `/port-lists/{id}` | `openvas_update_port_list` | PASS |
| POST | `/credentials` | `openvas_create_credential` | PASS |
| PUT | `/credentials/{id}` | `openvas_update_credential` | PASS |
| GET | `/credential-stores` | `openvas_list_credential_stores` | SKIP (capability) Not supported by the connected gvmd while listing credential stores: Not Implemented (not_implemented, HTTP 501): credential stores are not  |
| GET | `/credential-stores/{id}` | `openvas_get_credential_store` | SKIP (no credential store in this stack) |
| PUT | `/credential-stores/{id}` | `openvas_update_credential_store` | SKIP (no credential store in this stack) |
| POST | `/credential-stores/{id}/actions/verify` | `openvas_verify_credential_store` | SKIP (no credential store in this stack) |
| POST | `/schedules` | `openvas_create_schedule` | PASS |
| PUT | `/schedules/{id}` | `openvas_update_schedule` | PASS |
| POST | `/alerts` | `openvas_create_alert` | PASS |
| PUT | `/alerts/{id}` | `openvas_update_alert` | PASS |
| POST | `/filters` | `openvas_create_filter` | PASS |
| PUT | `/filters/{id}` | `openvas_update_filter` | PASS |
| POST | `/filters/{id}/clone` | `openvas_clone_filter` | PASS |
| POST | `/targets` | `openvas_create_target` | PASS |
| PUT | `/targets/{id}` | `openvas_update_target` | PASS |
| POST | `/targets/{id}/clone` | `openvas_clone_target` | PASS |
| POST | `/tasks` | `openvas_create_task` | PASS |
| PUT | `/tasks/{id}` | `openvas_update_task` | PASS |
| POST | `/tasks/{id}/clone` | `openvas_clone_task` | PASS |
| POST | `/tags` | `openvas_create_tag` | PASS |
| PUT | `/tags/{id}` | `openvas_update_tag` | PASS |
| POST | `/tags/{id}/clone` | `openvas_clone_tag` | PASS |
| POST | `/notes` | `openvas_create_note` | PASS |
| PUT | `/notes/{id}` | `openvas_update_note` | PASS |
| POST | `/overrides` | `openvas_create_override` | PASS |
| PUT | `/overrides/{id}` | `openvas_update_override` | PASS |
| POST | `/scan-configs` | `openvas_create_scan_config` | PASS |
| PUT | `/scan-configs/{id}` | `openvas_update_scan_config` | PASS |
| PUT | `/scan-configs/{id}/preferences/{name}` | `openvas_set_scan_config_preference` | PASS |
| PUT | `/scan-configs/{id}/family-selection` | `openvas_set_scan_config_family_selection` | PASS |
| PUT | `/scan-configs/{id}/families/{family}/nvts` | `openvas_set_scan_config_nvt_selection` | PASS |
| POST | `/configs/{id}/clone` | `openvas_clone_config` | PASS |
| GET | `/configs/{id}` | `openvas_get_config` | PASS |
| POST | `/hosts` | `openvas_create_asset_host` | PASS |
| PUT | `/hosts/{id}` | `openvas_update_asset_host` | PASS |
| PUT | `/assets/{id}?type=host` | `openvas_update_asset` | PASS |
| PUT | `/operating-systems/{id}` | `openvas_update_operating_system` | SKIP (no OS assets yet; populated by scans) |
| DELETE | `/operating-systems/{id}` | `openvas_delete_operating_system` | SKIP (no OS assets yet; populated by scans) |
| POST | `/oci-image-targets` | `openvas_create_oci_image_target` | SKIP (capability) Gateway cannot reach gvmd while creating the OCI image target: Bad Gateway (backend_unavailable, HTTP 502): The backend service is unavailab |
| POST | `/web-application-targets` | `openvas_create_web_application_target` | SKIP (capability) Gateway cannot reach gvmd while creating the web application target: Bad Gateway (backend_unavailable, HTTP 502): The backend service is una |
| POST | `/agents/sync` | `openvas_sync_agents` | SKIP (capability) Gateway cannot reach gvmd while starting the agent sync: Bad Gateway (backend_unavailable, HTTP 502): The backend service is unavailable. |
| POST | `/agent-groups` | `openvas_create_agent_group` | SKIP (capability) Gateway cannot reach gvmd while creating the agent group: Bad Gateway (backend_unavailable, HTTP 502): The backend service is unavailable. |
| POST | `/tasks (type=import)` | `openvas_create_task` | PASS |
| POST | `/reports?taskId=&inAssets=false` | `openvas_import_report` | PASS |
| GET | `/reports/{id}?page=&perPage=` | `openvas_get_report` | PASS |
| DELETE | `/reports/{id}` | `openvas_delete_report` | SKIP (compose gvmd retains imported reports; exercised in the scan lifecycle) |
| DELETE | `/tasks/{id}?ultimate=true` | `openvas_delete_task` | PASS |
| DELETE | `/overrides/{id}` | `openvas_delete_override` | PASS |
| DELETE | `/notes/{id}` | `openvas_delete_note` | PASS |
| DELETE | `/tags/{id}` | `openvas_delete_tag` | PASS |
| DELETE | `/tags/{id}` | `openvas_delete_tag` | PASS |
| DELETE | `/tasks/{id}` | `openvas_delete_task` | PASS |
| DELETE | `/tasks/{id}` | `openvas_delete_task` | PASS |
| DELETE | `/targets/{id}` | `openvas_delete_target` | PASS |
| DELETE | `/targets/{id}` | `openvas_delete_target` | PASS |
| DELETE | `/configs/{id}` | `openvas_delete_config` | PASS |
| DELETE | `/scan-configs/{id}` | `openvas_delete_scan_config` | PASS |
| DELETE | `/hosts/{id}` | `openvas_delete_asset_host` | PASS |
| DELETE | `/filters/{id}` | `openvas_delete_filter` | PASS |
| DELETE | `/filters/{id}` | `openvas_delete_filter` | PASS |
| DELETE | `/alerts/{id}` | `openvas_delete_alert` | PASS |
| DELETE | `/schedules/{id}` | `openvas_delete_schedule` | PASS |
| DELETE | `/credentials/{id}` | `openvas_delete_credential` | PASS |
| DELETE | `/port-lists/{id}` | `openvas_delete_port_list` | PASS |

### Scan lifecycle, report drill-downs and export jobs

| Method | Gateway path | MCP tool | Outcome |
| --- | --- | --- | --- |
| GET | `/scan-configs` | `openvas_list_scan_configs` | PASS |
| GET | `/scanners` | `openvas_list_scanners` | PASS |
| GET | `/port-lists` | `openvas_list_port_lists` | PASS |
| GET | `/report-formats` | `openvas_list_report_formats` | PASS |
| POST | `/targets` | `openvas_create_target` | PASS |
| POST | `/tasks` | `openvas_create_task` | PASS |
| POST | `/tasks/{id}/resume (new task)` | `openvas_resume_task` | PASS (rejected as expected) |
| POST | `/tasks/{id}/start` | `openvas_start_task` | PASS |
| GET | `/tasks/{id} (poll)` | `openvas_get_task` | PASS |
| GET | `/reports/{id}` | `openvas_get_report` | PASS |
| GET | `/reports?filter=` | `openvas_list_reports` | PASS |
| GET | `/reports/{id}/results` | `openvas_get_report_results` | PASS |
| GET | `/reports/{id}/results?filter=` | `openvas_get_report_results` | PASS |
| GET | `/reports/{id}/vulnerabilities` | `openvas_get_report_vulnerabilities` | PASS |
| GET | `/reports/{id}/hosts` | `openvas_get_report_hosts` | PASS |
| GET | `/reports/{id}/ports` | `openvas_get_report_ports` | PASS |
| GET | `/reports/{id}/applications` | `openvas_get_report_applications` | PASS |
| GET | `/reports/{id}/cves` | `openvas_get_report_cves` | PASS |
| GET | `/reports/{id}/closed-cves` | `openvas_get_report_closed_cves` | PASS |
| GET | `/reports/{id}/errors` | `openvas_get_report_errors` | PASS |
| GET | `/reports/{id}/operating-systems` | `openvas_get_report_operating_systems` | PASS |
| GET | `/reports/{id}/tls-certificates` | `openvas_get_report_tls_certificates` | PASS |
| GET | `/results/{id}` | `openvas_get_result` | PASS |
| GET | `/results?filter=` | `openvas_list_results` | PASS |
| POST | `/reports/{id}/exports` | `openvas_export_report (json)` | PASS |
| GET | `/jobs/{id}` | `openvas_get_job` | PASS |
| GET | `/jobs/{id}/result (json)` | `openvas_download_job_result` | PASS |
| POST | `/reports/{id}/exports` | `openvas_export_report (pdf)` | PASS |
| GET | `/jobs/{id}` | `openvas_get_job` | PASS |
| GET | `/jobs/{id}/result (pdf)` | `openvas_download_job_result` | PASS |
| DELETE | `/jobs/{id}` | `openvas_cancel_job` | PASS |
| POST | `/tasks/{id}/stop (finished task)` | `openvas_stop_task` | PASS |
| GET | `/hosts?filter=` | `openvas_list_asset_hosts` | PASS |
| DELETE | `/reports/{id}` | `openvas_delete_report` | PASS |
| DELETE | `/tasks/{id}` | `openvas_delete_task` | PASS |
| DELETE | `/targets/{id}` | `openvas_delete_target` | PASS |

### Compliance audit lifecycle

| Method | Gateway path | MCP tool | Outcome |
| --- | --- | --- | --- |
| GET | `/scan-configs` | `openvas_list_scan_configs` | PASS |
| GET | `/scanners` | `openvas_list_scanners` | PASS |
| GET | `/port-lists` | `openvas_list_port_lists` | PASS |
| GET | `/report-formats` | `openvas_list_report_formats` | PASS |
| GET | `/policies` | `openvas_list_policies` | PASS |
| POST | `/policies` | `openvas_create_policy` | PASS |
| GET | `/policies/{id}` | `openvas_get_policy` | PASS |
| PUT | `/policies/{id}` | `openvas_update_policy` | PASS |
| POST | `/targets` | `openvas_create_target` | PASS |
| POST | `/audits` | `openvas_create_audit` | PASS |
| GET | `/audits/{id}` | `openvas_get_audit` | PASS |
| PUT | `/audits/{id}` | `openvas_update_audit` | PASS |
| POST | `/audits/{id}/start` | `openvas_start_audit` | PASS |
| POST | `/audits/{id}/stop (finished)` | `openvas_stop_audit` | PASS |
| POST | `/audits/{id}/resume (finished)` | `openvas_resume_audit` | PASS (409 surfaced as tool error) |
| GET | `/reports/{id} (audit report)` | `openvas_get_report` | PASS |
| DELETE | `/audits/{id}` | `openvas_delete_audit` | PASS |
| DELETE | `/targets/{id} (trashcan; audit still references it)` | `openvas_delete_target` | PASS |
| DELETE | `/policies/{id}` | `openvas_delete_policy` | PASS |

### Identity administration (opt-in toolset)

| Method | Gateway path | MCP tool | Outcome |
| --- | --- | --- | --- |
| GET | `/users` | `openvas_list_users` | PASS |
| GET | `/users/{id}` | `openvas_get_user` | PASS |
| GET | `/roles` | `openvas_list_roles` | PASS |
| GET | `/roles/{id}` | `openvas_get_role` | PASS |
| GET | `/permissions` | `openvas_list_permissions` | PASS |
| GET | `/permissions/{id}` | `openvas_get_permission` | PASS |
| POST | `/groups` | `openvas_create_group` | PASS |
| GET | `/groups` | `openvas_list_groups` | PASS |
| GET | `/groups/{id}` | `openvas_get_group` | PASS |
| PUT | `/groups/{id}` | `openvas_update_group` | PASS |
| DELETE | `/groups/{id}` | `openvas_delete_group` | PASS |
| POST | `/roles` | `openvas_create_role` | PASS |
| PUT | `/roles/{id}` | `openvas_update_role` | PASS |
| DELETE | `/roles/{id}` | `openvas_delete_role` | PASS |
| GET | `/user-settings` | `openvas_list_user_settings` | PASS |
| GET | `/user-settings/{id}` | `openvas_get_user_setting` | PASS |
| PUT | `/user-settings/{id}` | `openvas_update_user_setting` | PASS |

### Negative paths and gating

| Method | Gateway path | MCP tool | Outcome |
| --- | --- | --- | --- |
| GET | `/targets/{id} (unknown)` | `openvas_get_target` | PASS (404 → legible tool error) |
| GET | `/targets/{id} (malformed)` | `openvas_get_target` | PASS (400/404 → legible tool error) |
| GET | `/targets (bad credentials)` | `openvas_test_connection` | PASS (401 → legible tool error) |
| GET | `/targets (read-only server)` | `openvas_list_targets` | PASS |

### Streamable HTTP pass-through against the live gateway

| Method | Gateway path | MCP tool | Outcome |
| --- | --- | --- | --- |
| HTTP | `/mcp tools/call (Basic forwarded)` | `openvas_test_connection` | PASS |
| HTTP | `/mcp tools/call (wrong Basic)` | `openvas_list_targets` | PASS (401 → legible tool error) |
| HTTP | `/mcp tools/call (anonymous, no fallback)` | `openvas_list_targets` | PASS (401 → legible tool error) |

