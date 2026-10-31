# Contributing

Thanks for your interest in `openvas-mcp-server` — the Rust MCP server for
Greenbone Vulnerability Management. This guide covers how to build, test and
submit changes.

## Prerequisites

- Rust 1.90 or newer (`rustup toolchain install stable`)
- For end-to-end work: a running
  [rust-gvm-api](https://github.com/greenbone-hive/rust-gvm-api) gateway in
  front of gvmd. Unit and integration tests run entirely against a mock
  gateway and need neither.

## Development loop

```bash
cargo test                                                   # unit + mock-gateway tests
cargo fmt --all
cargo clippy --all-targets --all-features -- -D warnings
cargo run -- --list-toolsets
```

CI enforces the same gate (format, clippy with `-D warnings`, tests, a release
build) plus the Security workflow (cargo-audit, cargo-machete, Semgrep). Run
them locally before opening a PR.

## Architecture in one paragraph

The server is a thin, typed front end over the gateway's REST API — it holds
**no business logic**. A tool is: validate arguments → build a gateway request
→ map the response or error. Per-request identity forwarding and
pagination are the only cross-cutting pieces. List tools return
summarized rows to protect the model's token budget; `get` tools return the
gateway's JSON unchanged. See [docs/development.md](docs/development.md)
for the module layout.

## Adding or changing a tool

1. Find the endpoint in the gateway's
   [`spec/rest-api`](https://github.com/greenbone-hive/rust-gvm-api/tree/main/spec/rest-api)
   — that spec is the contract.
2. Add the tool to the matching `src/mcp/tools/<toolset>.rs` under its
   `#[tool_router(...)]` impl. Reuse the `common.rs` helpers (`list_summarized`,
   `get_passthrough`, `create_resource`, `update_resource`, `delete_resource`,
   `Body`).
3. Annotate honestly: `read_only_hint = true` for reads (this is what
   `--read-only` filters on), `destructive_hint` for updates/deletes.
4. Add a mock-gateway test asserting the request shape (path, query, body) and
   the summarized output, and update the inventory count in `tests/gating.rs`.
5. If it starts a new toolset, wire the router in `server.rs` and extend
   `toolset.rs`.
6. Run the drift check against the gateway spec and commit the regenerated
   matrix: `python3 scripts/gateway_sync_check.py --spec ../rust-gvm-api/spec/rest-api --write-doc`.
   CI runs the same check against the gateway's `main`.

## Commit and PR conventions

- Conventional-commit prefixes: `feat`, `fix`, `refactor`, `test`, `docs`,
  `ci`, `chore`.
- Keep PRs focused; fill in the pull-request template (type, checklist,
  testing notes).
- No `unsafe` — the crate is `#![forbid(unsafe_code)]`.
- Never log or return credentials; keep them in `secrecy::SecretString`.

## Releases

Releases are cut from in-repo workflows and follow semantic versioning with
`v<version>` tags, matching the `rust-gvm` / `rust-gvm-api` model. See
[RELEASING.md](RELEASING.md). Contributors do not tag releases; maintainers run
the release workflows.

## Reporting security issues

Do not open public issues for vulnerabilities — see [SECURITY.md](SECURITY.md)
for private reporting.

## License

By contributing, you agree that your contributions are licensed under
AGPL-3.0-or-later, the same license as the project.
