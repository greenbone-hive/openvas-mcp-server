"""Regression checks for the release workflow policy."""

import re
import unittest
from pathlib import Path

from scripts.verify_release import BINARY_ARCHIVES, SBOM_ARCHIVE


ROOT = Path(__file__).resolve().parents[1]
WORKFLOWS = ROOT / ".github" / "workflows"
RELEASE_WORKFLOWS = ("release.yml", "release-orchestrated.yml")
PINNED_USES = re.compile(r"(?m)^\s+(?:- )?uses: \S+@[0-9a-f]{40} # \S+$")
ANY_USES = re.compile(r"(?m)^\s+(?:- )?uses: .+$")


def workflow(name: str) -> str:
    return (WORKFLOWS / name).read_text(encoding="utf-8")


def job_block(source: str, job: str) -> str:
    match = re.search(
        rf"^  {re.escape(job)}:\n(?P<body>.*?)(?=^  [a-z0-9-]+:\n|\Z)",
        source,
        re.MULTILINE | re.DOTALL,
    )
    if match is None:
        raise AssertionError(f"workflow has no {job} job")
    return match.group("body")


def job_needs(source: str, job: str) -> tuple[str, ...]:
    block = job_block(source, job)
    inline = re.search(r"(?m)^    needs: (?P<needs>.+)$", block)
    if inline is not None:
        value = inline.group("needs").strip()
        if value.startswith("[") and value.endswith("]"):
            return tuple(item.strip() for item in value[1:-1].split(","))
        return (value,)

    multiline = re.search(
        r"(?m)^    needs:\n(?P<needs>(?:      - [a-z0-9-]+\n)+)", block
    )
    if multiline is None:
        return ()
    return tuple(re.findall(r"(?m)^      - ([a-z0-9-]+)$", multiline.group("needs")))


def job_names(source: str) -> tuple[str, ...]:
    jobs = source.split("\njobs:\n", 1)[1]
    return tuple(re.findall(r"(?m)^  ([a-z0-9-]+):$", jobs))


def depends_on(source: str, job: str, required: str) -> bool:
    direct = job_needs(source, job)
    return required in direct or any(
        depends_on(source, dependency, required) for dependency in direct
    )


class WorkflowPolicyTests(unittest.TestCase):
    def test_release_workflows_pin_every_action_to_a_commit(self) -> None:
        for name in RELEASE_WORKFLOWS:
            source = workflow(name)
            with self.subTest(workflow=name):
                uses = ANY_USES.findall(source)
                self.assertTrue(uses)
                unpinned = [line for line in uses if not PINNED_USES.fullmatch(line)]
                self.assertEqual(unpinned, [])

    def test_release_installed_tools_are_version_pinned(self) -> None:
        release = workflow("release.yml")
        self.assertRegex(release, r'(?m)^  CARGO_CYCLONEDX_VERSION: "\d+\.\d+\.\d+"$')
        self.assertRegex(release, r'(?m)^  SBOMQS_VERSION: "v\d+\.\d+\.\d+"$')
        self.assertRegex(release, r'(?m)^  SBOM_QUALITY_THRESHOLD: "\d+(?:\.\d+)?"$')
        self.assertIn("tool: cargo-cyclonedx@${{ env.CARGO_CYCLONEDX_VERSION }}", release)
        self.assertIn(
            "go install github.com/interlynk-io/sbomqs/v2@${{ env.SBOMQS_VERSION }}",
            release,
        )

    def test_release_write_jobs_require_protected_main_qualification(self) -> None:
        source = workflow("release.yml")
        qualification = job_block(source, "qualify")
        self.assertIn(
            '"+refs/heads/main:refs/remotes/origin/main"', qualification
        )
        self.assertIn("python3 scripts/check_release_commit.py", qualification)
        self.assertIn("python3 scripts/check_release_version.py", qualification)

        write_jobs = tuple(
            job
            for job in job_names(source)
            if re.search(r"(?m)^      [a-z-]+: write$", job_block(source, job))
        )
        self.assertTrue(write_jobs)
        for job in write_jobs:
            with self.subTest(job=job):
                self.assertTrue(depends_on(source, job, "qualify"))

    def test_release_publishes_only_the_immutable_tag(self) -> None:
        container = job_block(workflow("release.yml"), "container")
        tags = re.search(r"(?m)^          tags: \|\n(?P<tags>(?:            .+\n)+)", container)
        self.assertIsNotNone(tags)
        self.assertEqual(
            tags.group("tags").split(),
            ["type=raw,value=${{", "github.ref_name", "}}"],
        )
        self.assertNotIn("value=latest", container)
        self.assertNotIn(":latest", container)

    def test_release_asset_names_match_the_verifier(self) -> None:
        release = workflow("release.yml")
        artifacts = re.findall(r"(?m)^            artifact: (\S+)$", release)
        self.assertEqual(
            sorted(f"{artifact}.tar.gz" for artifact in artifacts),
            sorted(BINARY_ARCHIVES),
        )
        self.assertIn(f"tar czf ../{SBOM_ARCHIVE} ./*.cdx.*", release)
        self.assertIn(f"sha256sum {SBOM_ARCHIVE} > {SBOM_ARCHIVE}.sha256", release)

    def test_release_orchestrator_restores_supported_python_after_pontos(self) -> None:
        source = workflow("release-orchestrated.yml")
        pontos = source.index("greenbone/actions/setup-pontos@")
        setup_python = source.index("actions/setup-python@", pontos)
        qualification = source.index(
            "Qualify exact protected-main commit and release version", setup_python
        )

        self.assertLess(pontos, setup_python)
        self.assertLess(setup_python, qualification)
        self.assertIn('python-version: "3.12"', source[setup_python:qualification])

    def test_release_orchestrator_attaches_qualified_sha_to_main_before_pontos(self) -> None:
        source = workflow("release-orchestrated.yml")
        qualification = source.index(
            "Qualify exact protected-main commit and release version"
        )
        create_release = source.index("Create release with pontos", qualification)
        qualification_block = source[qualification:create_release]

        self.assertIn('git switch -C main "$RELEASE_SHA"', qualification_block)
        self.assertIn(
            "git branch --set-upstream-to=origin/main main", qualification_block
        )
        self.assertIn('notes="docs/releases/v${RELEASE_VERSION}.md"', qualification_block)
        self.assertIn("python3 scripts/check_release_version.py", qualification_block)

    def test_release_orchestrator_never_rewrites_the_manifest(self) -> None:
        source = workflow("release-orchestrated.yml")
        for flag in ("--no-update-project", "--no-next-version", "--git-tag-prefix v"):
            with self.subTest(flag=flag):
                self.assertIn(flag, source)
        self.assertIn("--github-pre-release", source)
        self.assertIn('--notes-file "docs/releases/v${RELEASE_VERSION}.md"', source)


if __name__ == "__main__":
    unittest.main()
