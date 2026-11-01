import shutil
import tempfile
import unittest
from pathlib import Path

from scripts.check_release_version import ReleaseVersionError, validate_package


MANIFEST = """[package]
name = "gvm-mcp"
version = "0.7.0"
edition = "2024"
"""

LOCK = """version = 4

[[package]]
name = "anyhow"
version = "1.0.104"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "gvm-mcp"
version = "0.7.0"
dependencies = [
 "anyhow",
]
"""


class ReleaseVersionTests(unittest.TestCase):
    def setUp(self) -> None:
        self.root = Path(tempfile.mkdtemp(prefix="release-version-"))
        self.addCleanup(shutil.rmtree, self.root)
        self.write("Cargo.toml", MANIFEST)
        self.write("Cargo.lock", LOCK)

    def write(self, name: str, content: str) -> None:
        (self.root / name).write_text(content, encoding="utf-8")

    def test_consistent_package_passes(self) -> None:
        validate_package(self.root, "0.7.0")

    def test_manifest_version_must_match(self) -> None:
        with self.assertRaisesRegex(ReleaseVersionError, "package version must be 0.8.0"):
            validate_package(self.root, "0.8.0")

    def test_lock_version_must_match(self) -> None:
        self.write("Cargo.lock", LOCK.replace('version = "0.7.0"', 'version = "0.6.0"'))

        with self.assertRaisesRegex(ReleaseVersionError, "Cargo.lock gvm-mcp version"):
            validate_package(self.root, "0.7.0")

    def test_package_name_is_pinned(self) -> None:
        self.write("Cargo.toml", MANIFEST.replace('name = "gvm-mcp"', 'name = "other"'))

        with self.assertRaisesRegex(ReleaseVersionError, "package name must be 'gvm-mcp'"):
            validate_package(self.root, "0.7.0")

    def test_lock_must_not_resolve_package_from_a_registry(self) -> None:
        self.write(
            "Cargo.lock",
            LOCK.replace(
                'name = "gvm-mcp"\nversion = "0.7.0"\n',
                'name = "gvm-mcp"\nversion = "0.7.0"\n'
                'source = "registry+https://github.com/rust-lang/crates.io-index"\n',
            ),
        )

        with self.assertRaisesRegex(ReleaseVersionError, "unexpectedly resolves from"):
            validate_package(self.root, "0.7.0")

    def test_workspace_inherited_version_is_rejected(self) -> None:
        self.write(
            "Cargo.toml", MANIFEST.replace('version = "0.7.0"', "version.workspace = true")
        )

        with self.assertRaisesRegex(ReleaseVersionError, "package version must be 0.7.0"):
            validate_package(self.root, "0.7.0")

    def test_invalid_expected_version_is_rejected(self) -> None:
        with self.assertRaisesRegex(ReleaseVersionError, "not SemVer"):
            validate_package(self.root, "release")


if __name__ == "__main__":
    unittest.main()
