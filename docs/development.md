# Development

## Layout

```text
src/
  main.rs            CLI entry: transport selection, logging (stderr)
  config.rs          clap CLI → validated Config
  gateway/           HTTP client for the rust-gvm-api gateway
    client.rs        request plumbing; attaches per-request Authorization;
                     no redirects; bounded artifact downloads
    auth.rs          caller-identity forwarding (task-local) + Basic fallback
    models.rs        serde DTOs mirroring the gateway spec
    error.rs         RFC 9457 problem+json → typed GatewayError
  mcp/
    server.rs        router composition, toolset + read-only gating,
                     --require-caller-auth enforcement
    toolset.rs       Toolset enum and --toolsets parsing
    error.rs         GatewayError → legible tool errors
    http.rs          streamable-HTTP transport (axum)
    tools/           one module per toolset; each exports <name>_router
scripts/
  gateway_sync_check.py  spec ↔ tool drift check; renders docs/gateway-sync.md
tests/               mock-gateway (wiremock) integration tests + live e2e
```

Principles (from the roadmap): no business logic in this server — a tool is
validate args → build gateway request → map response/error. Identity
forwarding (the caller's `Authorization`, scoped per call by `server.rs`'s
`call_tool`) and pagination are the only cross-cutting "smart" pieces. List
output is summarized (token budget); `get` output is the gateway's JSON,
unchanged.

## Gateway contract

The gateway's `spec/rest-api` (currently contract 0.5.0: 121 paths, 199
operations) is the source of truth for the tool surface. 195 operations map
to exactly one tool each; the remaining four (session create, inspect and
delete, and the OpenAPI document) do not apply to a stateless pass-through
server. `scripts/gateway_sync_check.py` resolves the spec, maps every
operation to a tool, reports missing, stale or broken mappings, and with
`--write-doc` renders [gateway-sync.md](gateway-sync.md). CI runs the same
check against the gateway's `main` branch.

## Adding a tool

1. Find the endpoint in the gateway's `spec/rest-api/*.yaml` (the contract).
2. Add the tool to the matching `src/mcp/tools/<toolset>.rs` under the
   `#[tool_router(router = <toolset>_router, ...)]` impl block. Reuse the
   `common.rs` helpers (`list_summarized*`, `get_passthrough*`,
   `create_resource`, `clone_resource`, `update_resource`,
   `update_resource_empty`, `delete_resource`, `delete_resource_plain`,
   `Body`). Send only the keys the spec lists: gateway bodies reject unknown
   fields, and `ultimate` is rejected on routes that do not document it.
3. Annotate honestly: `read_only_hint = true` for reads (this is what
   `--read-only` filters on), `destructive_hint` for updates/deletes.
4. If it starts a new toolset, wire the router in `server.rs` and extend
   `toolset.rs`.
5. Add a wiremock test asserting the request shape (path, query, body) and
   the summarized output (`tests/gateway_sync_tools.rs` is the home for
   contract-shape tests), bump the inventory counts in `tests/gating.rs`,
   and re-run the drift check so `docs/gateway-sync.md` stays current:

   ```bash
   python3 scripts/gateway_sync_check.py --spec ../rust-gvm-api/spec/rest-api --write-doc
   ```

## Quality gate

```bash
cargo fmt --all --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test
python3 scripts/gateway_sync_check.py --spec ../rust-gvm-api/spec/rest-api
```

CI runs the same gate plus a release build with a CLI smoke test and the
drift check against the gateway's `main`.

## Test map

| File | Covers |
| --- | --- |
| `tests/gating.rs` | toolset selection, read-only gating, inventory counts |
| `tests/test_connection_tool.rs` | `openvas_test_connection` health/ready/version/auth probe |
| `tests/gateway_auth.rs` | per-request Basic forwarding, problem+json mapping |
| `tests/http_transport.rs` | streamable HTTP, inbound `Authorization` forwarding, Host guard |
| `tests/security_hardening.rs` | no redirects, artifact cap, `--require-caller-auth` |
| `tests/read_surface_tools.rs` / `write_surface_tools.rs` / `tasks_targets_tools.rs` / `report_export_tools.rs` / `identity_tools.rs` | request shapes of the original surface |
| `tests/gateway_sync_tools.rs` | request shapes of every tool added for contract parity (clone, compliance, scan-config sub-resources, credential stores, report pages/import, assets, SecInfo, agents, specialized targets) |
| `tests/e2e_live.rs` | live gateway + gvmd: every endpoint, lifecycles with cleanup, scan + audit runs, HTTP pass-through |

## Live end-to-end tests

`tests/e2e_live.rs` is `#[ignore]`d and drives a real gateway + gvmd. The
gateway repository's `compose.yaml` brings up a qualified stack (gvmd
26.43.0 / GMP 22.8 plus the gateway built from source); publish the gateway
on a free port and point the tests at it:

```bash
export GVM_E2E_GATEWAY_URL=http://127.0.0.1:18080
export GVM_USERNAME=admin GVM_PASSWORD=admin
export GVM_E2E_TARGET_HOST=openvasd          # a host the scanner container can reach
export GVM_E2E_MATRIX_OUT=/tmp/e2e-matrix.md # optional: endpoint coverage table
cargo test --test e2e_live -- --ignored --test-threads=1            # catalogs, lifecycles, identity, HTTP
GVM_E2E_SCAN=1 cargo test --test e2e_live -- --ignored --test-threads=1   # + scan and audit runs
```

Every resource the suite creates is named `gvm-mcp-e2e-*` and deleted again.
Capability-dependent endpoints (credential stores, OCI/web application
targets, agents) are recorded as `SKIP (capability)` when the connected gvmd
lacks the feature; gateway-side defects are recorded as `GATEWAY-DEFECT`
rows instead of failing the tool. The matrix is a run artifact: set
`GVM_E2E_MATRIX_OUT` to write it as Markdown (one row per exercised gateway
operation, with the tool that drove it and the outcome) and attach it to
the PR or CI run instead of committing it.

Gateway-side behaviour the suite currently has to tolerate (reproducible
with curl against rust-gvm-api `main` + gvmd 26.43.0, not fixable here):

- `GET /scan-configs/{id}/preferences/{name}` answers 404 for every
  preference the list on the same config returns; `PUT` on the name works.
- `GET /assets?type=tls_certificate` is rejected by gvmd ("Failed to find
  type") although the contract lists the value.
- Feature-disabled gvmd commands (OCI image targets, agents) surface as
  `502 backend_unavailable` rather than `501`.
- Request-scoped `Basic` sessions end per request and take their export
  jobs with them — the reason for the job-bound export sessions in
  `src/gateway/jobs.rs`.
- `DELETE /audits/{id}` is non-ultimate only, so an audit's target can only
  be trashed afterwards.
- A persistent gateway session whose gvmd connection died (for example after
  a feature-disabled command made gvmd drop it) stays `active` at the gateway
  while every further command on it answers `502 backend_unavailable`; the
  gateway does not reconnect. `src/gateway/client.rs` drops and closes such a
  session on the first 502 and logs in again.

## Releasing

Releases follow the rust-gvm model: a reviewed PR bumps `version` in
`Cargo.toml` (and `Cargo.lock`) and adds `docs/releases/v<version>.md`; the
`Orchestrated Release` workflow is then dispatched from `main` with that exact
version. It tags, publishes the notes and waits for `release.yml` to build the
five binaries, the GHCR image and the SBOM. See [RELEASING.md](https://github.com/greenbone-hive/OpenVAS-MCP/blob/main/RELEASING.md).

## Documentation

The user documentation under `docs/` is a Sphinx + MyST project with the Furo
theme, set up like the
[Greenbone Community documentation](https://github.com/greenbone/docs), and
published to GitHub Pages by `.github/workflows/docs.yml` on every push to
`main`. Build it locally with [uv](https://docs.astral.sh/uv/):

```bash
cd docs
uv run make html          # _build/html/index.html
uv run make livehtml      # rebuild on change, http://127.0.0.1:8000
```

The [tool reference](reference/tools/index.md) is generated from the
server's own `tools/list` and must not be edited by hand:

```bash
cargo build --release
python3 scripts/render_tool_reference.py            # rewrite docs/reference/tools/
python3 scripts/render_tool_reference.py --check    # what CI runs
```
