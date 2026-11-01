# Releasing

Releases are managed via the repository's
[Orchestrated Release workflow](https://github.com/greenbone-hive/OpenVAS-MCP/actions/workflows/release-orchestrated.yml),
following the same model as
[rust-gvm](https://github.com/greenbone-hive/rust-gvm/blob/main/RELEASING.md).

## How It Works

First, update `[package].version` in `Cargo.toml` and `Cargo.lock` in a normal
pull request titled `release: prepare gvm-mcp v<version>`. Add reviewed release
notes at `docs/releases/v<version>.md` in the same pull request. Merge it
through the protected `main` branch after all required review and checks pass.
Then dispatch the release workflow from the exact protected `main` commit with
that exact version.

```text
┌─────────────────────────────────────────────────────────────────────────┐
│                        Protected main                                    │
│  Version, Cargo.lock and release notes merged through a reviewed PR      │
└─────────────────────┬───────────────────────────────────────────────────┘
                      │ workflow_dispatch with the exact merged version
                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                    release-orchestrated.yml                              │
│  1. Validates the exact protected-main SHA, version, lockfile, and notes  │
│  2. Creates the changelog, tag, and GitHub release via pontos            │
│  3. Publishes the reviewed release notes                                 │
│  4. Waits for the exact-SHA release.yml run to complete                  │
└─────────────────────┬───────────────────────────────────────────────────┘
                      │ tag push triggers
                      ▼
┌─────────────────────────────────────────────────────────────────────────┐
│                         release.yml                                      │
│  1. Qualifies the tag against protected main and the package version     │
│  2. Runs the test suite                                                  │
│  3. Builds gvm-mcp binaries (5 platforms)                                │
│  4. Publishes the container image to GHCR                                │
│  5. Generates and scores the SBOM (CycloneDX)                            │
│  6. Uploads all artifacts to the GitHub release                          │
│  7. Verifies assets, checksums, attestations, notes and GHCR manifests   │
└─────────────────────────────────────────────────────────────────────────┘
```

## Versioning

The workflow accepts the exact semantic version already merged to `main`,
without a leading `v`. The `Cargo.toml` package version on `main` therefore
represents the release that is about to be, or was last, published; there is
no synthetic next-development version. Versions containing a pre-release
suffix, such as `0.3.0-alpha.1` or `0.3.0-rc.1`, are published as GitHub
pre-releases automatically. There is no scheduled nightly workflow.

## Creating a Release

1. Open a pull request `release: prepare gvm-mcp v<version>` that bumps the
   version in `Cargo.toml`, refreshes `Cargo.lock` (`cargo update --workspace`
   or `cargo generate-lockfile`), and adds `docs/releases/v<version>.md`.
   `python3 scripts/check_release_version.py <version>` must pass.
2. Merge it into `main` after required checks pass.
3. Open the `Orchestrated Release` workflow in GitHub Actions and run it from
   `main` with `<version>`.
4. Confirm that the run created tag `v<version>` and the GitHub release, and
   that it finished only after the `Release` run for that tag succeeded.

## Do NOT

- **Do NOT** dispatch a version that is not already merged to `main`
- **Do NOT** dispatch from a commit other than the current protected `main` head
- **Do NOT** dispatch without reviewed `docs/releases/v<version>.md` notes
- **Do NOT** push version tags manually — let pontos handle it
- **Do NOT** create GitHub releases manually — pontos + release.yml handle everything

## Release Artifacts

Each release includes:

- **Binaries**: five `gvm-mcp` archives for Linux (amd64 GNU, arm64 GNU,
  amd64 musl) and macOS (amd64, arm64), each with a SHA-256 file
- **Container image**: `ghcr.io/greenbone-hive/openvas-mcp:v<version>` as a
  Linux amd64/arm64 image index; neither `latest` nor an unprefixed version
  tag is published
- **SBOM**: `openvas-mcp-sbom.tar.gz` containing CycloneDX 1.5 JSON and XML
  for the `gvm-mcp` crate, its SHA-256 file, and a separate
  `sbomqs-results.json` quality report
- **Attestations**: Sigstore build provenance for each binary archive, the
  SBOM archive, and the registry-backed container index

## Verifying Artifacts

```bash
gh attestation verify gvm-mcp-linux-amd64.tar.gz --owner greenbone-hive
gh attestation verify oci://ghcr.io/greenbone-hive/openvas-mcp:vX.Y.Z \
  --repo greenbone-hive/OpenVAS-MCP
```

The release workflow also runs `scripts/verify_release.py published` against
the immutable tag and qualified commit. It rejects missing or extra assets,
checksum or embedded-version mismatches, incomplete SBOMs, a JSON quality score
below 8.3, missing archive/container provenance, incorrect container platforms
or labels, a wrong runtime version, and release notes that differ from the
reviewed `docs/releases/v<version>.md`.

## Prerequisites

- `RELEASE_TOKEN`: a repository secret holding a token that may push tags to
  `main` and create releases. pontos pushes the tag with it, which is what
  triggers `release.yml` (a tag pushed with `GITHUB_TOKEN` would not).
- `RELEASE_USER` (optional repository variable): the GitHub user pontos acts
  as; defaults to the dispatching actor.
- `main` must be protected against force pushes and deletions, and `v*` tags
  against updates and deletions, so a release can never be rewritten after the
  fact.

## Retry and Recovery

- If `Orchestrated Release` fails before pontos created the tag, fix the cause
  and dispatch it again with the same version.
- If the tag exists but `release.yml` failed, rerun the failed `Release` run
  for that tag; the orchestrator can then be dispatched again only after the
  tag is removed deliberately, so prefer rerunning `Release`.
- If `release.yml` succeeded but the orchestrator timed out waiting, verify
  the release with `scripts/verify_release.py published` and treat it as done.
- Do not move or delete release tags as part of normal recovery. If a tag
  points at the wrong commit, stop and resolve that deliberately.

## Local Checks

```bash
python3 scripts/check_release_version.py <version>   # the version in Cargo.toml
python3 -m unittest \
  tests.test_release_commit tests.test_release_version \
  tests.test_verify_release tests.test_wait_for_release_run \
  tests.test_sbom_postprocess tests.test_sbom_quality \
  tests.test_workflow_policy
cargo build --locked --release
```

The Python tests need `defusedxml` (`pip install -r .github/requirements/release.txt`).
