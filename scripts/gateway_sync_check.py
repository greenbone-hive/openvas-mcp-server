#!/usr/bin/env python3
"""Verify that this server's tool surface matches the rust-gvm-api contract.

Resolves the gateway's multi-file OpenAPI spec (``spec/rest-api``), maps every
operation to the MCP tool that serves it, and fails when either side drifts:

* an operation in the spec that no tool serves  -> MISSING (add a tool)
* a tool whose operation left the spec           -> STALE (remove the tool)
* a mapped tool name that does not exist in src  -> BROKEN (rename)

It also renders ``docs/gateway-sync.md`` (the endpoint -> tool matrix).

Usage:
    scripts/gateway_sync_check.py [--spec ../rust-gvm-api/spec/rest-api] [--write-doc]

Requires PyYAML.
"""
from __future__ import annotations

import argparse
import os
import re
import sys
from collections import Counter, defaultdict

try:
    import yaml
except ImportError:  # pragma: no cover
    sys.exit("PyYAML is required: pip install pyyaml")

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
TOOLS_DIR = os.path.join(ROOT, "src", "mcp", "tools")

# Operations that are intentionally not tools. Everything else must map.
NOT_APPLICABLE = {
    ("POST", "/session"): "stateless pass-through: credentials are forwarded per request",
    ("GET", "/session"): "stateless pass-through: no persistent gateway session is held",
    ("DELETE", "/session"): "stateless pass-through: no persistent gateway session is held",
    ("GET", "/openapi.json"): "documentation route; not a workflow",
}

# Operations served by a tool that also serves others (probe calls).
SHARED = {
    ("GET", "/health"): "openvas_test_connection",
    ("GET", "/ready"): "openvas_test_connection",
}

# Resource path segment -> singular tool stem.
STEM = {
    "targets": "target", "tasks": "task", "audits": "audit", "policies": "policy",
    "configs": "config", "scan-configs": "scan_config", "scanners": "scanner",
    "schedules": "schedule", "credentials": "credential",
    "credential-stores": "credential_store", "alerts": "alert", "port-lists": "port_list",
    "results": "result", "reports": "report", "hosts": "asset_host",
    "operating-systems": "operating_system", "assets": "asset",
    "tls-certificates": "tls_certificate", "report-formats": "report_format",
    "filters": "filter", "tags": "tag", "notes": "note", "overrides": "override",
    "nvts": "nvt", "cves": "cve", "cpes": "cpe", "cert-bund-advisories": "cert_bund_advisory",
    "dfn-cert-advisories": "dfn_cert_advisory", "users": "user", "groups": "group",
    "roles": "role", "permissions": "permission", "user-settings": "user_setting",
    "agents": "agent", "agent-groups": "agent_group", "oci-image-targets": "oci_image_target",
    "web-application-targets": "web_application_target", "jobs": "job",
}

# Collection list tools whose name is not `list_<stem>s`.
LIST_NAME = {
    "nvts": "openvas_search_nvts", "nvt-families": "openvas_list_nvt_families",
    "vulnerabilities": "openvas_list_vulnerabilities", "feeds": "openvas_list_feeds",
    "policies": "openvas_list_policies", "cves": "openvas_list_cves", "cpes": "openvas_list_cpes",
    "cert-bund-advisories": "openvas_list_cert_bund_advisories",
    "dfn-cert-advisories": "openvas_list_dfn_cert_advisories",
    "credential-stores": "openvas_list_credential_stores",
    "user-settings": "openvas_list_user_settings",
}

EXPLICIT = {
    ("GET", "/version"): "openvas_get_version",
    ("GET", "/timezones"): "openvas_list_timezones",
    ("POST", "/reports"): "openvas_import_report",
    ("POST", "/reports/{id}/exports"): "openvas_export_report",
    ("GET", "/jobs/{id}"): "openvas_get_job",
    ("DELETE", "/jobs/{id}"): "openvas_cancel_job",
    ("GET", "/jobs/{id}/result"): "openvas_download_job_result",
    ("GET", "/scan-configs/{id}/nvts"): "openvas_list_scan_config_nvts",
    ("GET", "/scan-configs/{id}/nvts/{oid}"): "openvas_get_scan_config_nvt",
    ("GET", "/scan-configs/{id}/preferences"): "openvas_list_scan_config_preferences",
    ("GET", "/scan-configs/{id}/preferences/{name}"): "openvas_get_scan_config_preference",
    ("PUT", "/scan-configs/{id}/preferences/{name}"): "openvas_set_scan_config_preference",
    ("PUT", "/scan-configs/{id}/family-selection"): "openvas_set_scan_config_family_selection",
    ("PUT", "/scan-configs/{id}/families/{family}/nvts"): "openvas_set_scan_config_nvt_selection",
    ("POST", "/credential-stores/{id}/actions/verify"): "openvas_verify_credential_store",
    ("POST", "/agents/sync"): "openvas_sync_agents",
    ("GET", "/agents/{id}/support-bundle"): "openvas_get_agent_support_bundle",
    ("PUT", "/agent-control-scan-configs/{id}"): "openvas_update_agent_control_scan_config",
    ("GET", "/scanners/{id}/agent-installer-instruction"): "openvas_get_agent_installer_instruction",
}

REPORT_PAGE = {
    "results": "results", "vulnerabilities": "vulnerabilities", "errors": "errors",
    "closed-cves": "closed_cves", "tls-certificates": "tls_certificates", "hosts": "hosts",
    "ports": "ports", "applications": "applications", "cves": "cves",
    "operating-systems": "operating_systems",
}


def tool_for(method: str, path: str) -> str | None:
    key = (method, path)
    if key in EXPLICIT:
        return EXPLICIT[key]
    if key in SHARED:
        return SHARED[key]
    parts = path.strip("/").split("/")
    res = parts[0]
    if res == "reports" and len(parts) == 3 and parts[1] == "{id}" and parts[2] in REPORT_PAGE:
        return f"openvas_get_report_{REPORT_PAGE[parts[2]]}"
    if len(parts) == 1:
        if method == "GET":
            return LIST_NAME.get(res) or f"openvas_list_{STEM[res]}s"
        if method == "POST":
            return f"openvas_create_{STEM[res]}"
    if len(parts) == 2 and parts[1] == "{id}":
        stem = STEM.get(res)
        if not stem:
            return None
        return {"GET": f"openvas_get_{stem}", "PUT": f"openvas_update_{stem}",
                "DELETE": f"openvas_delete_{stem}"}.get(method)
    if len(parts) == 3 and parts[1] == "{id}":
        stem = STEM.get(res)
        action = parts[2]
        if method == "POST" and action in ("clone", "start", "stop", "resume") and stem:
            return f"openvas_{action}_{stem}"
    return None


# ───── spec resolution (mirrors the gateway's $ref layout) ─────
_cache: dict[str, dict] = {}


def load(path: str) -> dict:
    path = os.path.normpath(path)
    if path not in _cache:
        with open(path) as fh:
            _cache[path] = yaml.safe_load(fh)
    return _cache[path]


def deref(node, cur_file):
    hops = 0
    while isinstance(node, dict) and "$ref" in node and hops < 20:
        hops += 1
        fpart, _, ptr = node["$ref"].partition("#")
        f = cur_file if fpart in ("", ".") else os.path.join(os.path.dirname(cur_file), fpart)
        tgt = load(f)
        for seg in ptr.strip("/").split("/"):
            tgt = tgt[seg.replace("~1", "/").replace("~0", "~")]
        node, cur_file = tgt, os.path.normpath(f)
    return node, cur_file


def operations(spec_dir: str):
    root_file = os.path.join(spec_dir, "openapi.yaml")
    root = load(root_file)
    ops = []
    for path, item in root["paths"].items():
        item, f = deref(item, root_file)
        for method in ("get", "post", "put", "patch", "delete"):
            if method in item:
                op = item[method]
                ops.append((method.upper(), path, op.get("operationId", "-"), (op.get("tags") or ["-"])[0]))
    ops.sort(key=lambda o: (o[1], o[0]))
    return root.get("info", {}).get("version", "?"), ops


def source_tools() -> dict[str, str]:
    tools = {}
    for name in sorted(os.listdir(TOOLS_DIR)):
        if not name.endswith(".rs"):
            continue
        with open(os.path.join(TOOLS_DIR, name)) as fh:
            for m in re.finditer(r'name\s*=\s*"(openvas_[a-z0-9_]+)"', fh.read()):
                tools[m.group(1)] = name[:-3]
    return tools


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--spec", default=os.path.join(ROOT, "..", "rust-gvm-api", "spec", "rest-api"))
    ap.add_argument("--write-doc", action="store_true", help="render docs/gateway-sync.md")
    args = ap.parse_args()

    version, ops = operations(args.spec)
    tools = source_tools()
    used: Counter = Counter()
    rows = []
    problems = []
    for method, path, op_id, tag in ops:
        key = (method, path)
        if key in NOT_APPLICABLE:
            rows.append((tag, method, path, op_id, "—", f"n/a: {NOT_APPLICABLE[key]}"))
            continue
        tool = tool_for(method, path)
        if tool is None:
            problems.append(f"MISSING  {method} {path} ({op_id}): no tool maps to this operation")
            rows.append((tag, method, path, op_id, "—", "MISSING"))
            continue
        if tool not in tools:
            problems.append(f"BROKEN   {method} {path} -> {tool}: tool not found in src/mcp/tools")
            rows.append((tag, method, path, op_id, tool, "BROKEN"))
            continue
        used[tool] += 1
        rows.append((tag, method, path, op_id, tool, "ok"))
    for tool in sorted(tools):
        if tool not in used:
            problems.append(f"STALE    {tool} ({tools[tool]}.rs): no operation in the spec uses it")

    mapped_ops = sum(1 for r in rows if r[5] == "ok")
    print(f"gateway spec {version}: {len(ops)} operations over {len({o[1] for o in ops})} paths")
    print(f"tools in src: {len(tools)}; operations served: {mapped_ops}; "
          f"not applicable: {sum(1 for r in rows if r[5].startswith('n/a'))}")
    for p in problems:
        print(p)

    if args.write_doc:
        doc = os.path.join(ROOT, "docs", "gateway-sync.md")
        by_tag = defaultdict(list)
        for r in rows:
            by_tag[r[0]].append(r)
        with open(doc, "w") as fh:
            fh.write("# Gateway ↔ MCP tool sync matrix\n\n")
            fh.write(f"Generated by `scripts/gateway_sync_check.py` against rust-gvm-api "
                     f"spec **{version}** ({len(ops)} operations, "
                     f"{len({o[1] for o in ops})} paths). Every operation maps to exactly "
                     "one MCP tool unless marked not applicable; every tool serves at least "
                     "one operation. Re-run the script after pulling a new gateway spec; it "
                     "exits non-zero on drift.\n\n")
            fh.write(f"| Metric | Value |\n| --- | --- |\n| Operations served by a tool | {mapped_ops} |\n"
                     f"| Operations not applicable to MCP | {sum(1 for r in rows if r[5].startswith('n/a'))} |\n"
                     f"| Tools in `src/mcp/tools` | {len(tools)} |\n\n")
            for tag in sorted(by_tag):
                fh.write(f"## {tag}\n\n| Method | Path | operationId | MCP tool | Status |\n| --- | --- | --- | --- | --- |\n")
                for _, method, path, op_id, tool, status in by_tag[tag]:
                    tool_cell = f"`{tool}`" if tool != "—" else "—"
                    fh.write(f"| {method} | `{path}` | `{op_id}` | {tool_cell} | {status} |\n")
                fh.write("\n")
            fh.write("## Deliberately absent from the gateway\n\n"
                     "- **Tickets** — removed from rust-gvm-api (issue #500 / PR #504); the former "
                     "`openvas_list_tickets` / `openvas_get_ticket` tools were removed with it.\n"
                     "- Scanner administration, report-format/TLS-certificate administration, "
                     "trashcan restore, wizards, license and global-setting administration are "
                     "`omitted`/`deferred` upstream (see the gateway's "
                     "`docs/upstream-surface-dispositions.md`) and therefore have no tool.\n")
        print(f"wrote {doc}")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
