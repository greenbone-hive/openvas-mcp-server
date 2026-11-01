#!/usr/bin/env python3
"""Validate one exact package release version and its publish requirements."""

from __future__ import annotations

import argparse
import re
import sys
import tomllib
from pathlib import Path
from typing import Any


PACKAGE = "gvm-mcp"
SEMVER = re.compile(
    r"^(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)\."
    r"(0|[1-9][0-9]*)"
    r"(?:-[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?"
    r"(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?$"
)


class ReleaseVersionError(ValueError):
    """The manifest and lockfile are not internally consistent for a release."""


def load_toml(path: Path) -> dict[str, Any]:
    try:
        return tomllib.loads(path.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise ReleaseVersionError(f"cannot read {path}: {error}") from error


def validate_package(root: Path, expected: str) -> None:
    if not SEMVER.fullmatch(expected):
        raise ReleaseVersionError(f"expected version is not SemVer: {expected!r}")

    manifest = load_toml(root / "Cargo.toml")
    package = manifest.get("package")
    if not isinstance(package, dict):
        raise ReleaseVersionError("Cargo.toml has no [package] table")
    if package.get("name") != PACKAGE:
        raise ReleaseVersionError(
            f"package name must be {PACKAGE!r}, found {package.get('name')!r}"
        )
    if package.get("version") != expected:
        raise ReleaseVersionError(
            f"package version must be {expected}, found {package.get('version')!r}"
        )

    lock = load_toml(root / "Cargo.lock")
    packages = lock.get("package")
    if not isinstance(packages, list):
        raise ReleaseVersionError("Cargo.lock contains no package entries")
    entries = [
        entry
        for entry in packages
        if isinstance(entry, dict) and entry.get("name") == PACKAGE
    ]
    if len(entries) != 1:
        raise ReleaseVersionError(
            f"Cargo.lock must contain exactly one {PACKAGE} package, found {len(entries)}"
        )
    entry = entries[0]
    if entry.get("version") != expected:
        raise ReleaseVersionError(
            f"Cargo.lock {PACKAGE} version must be {expected}, found {entry.get('version')!r}"
        )
    if "source" in entry:
        raise ReleaseVersionError(
            f"Cargo.lock {PACKAGE} unexpectedly resolves from {entry['source']!r}"
        )


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("expected", help="exact SemVer release version")
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parents[1],
        help="repository root (default: repository containing this script)",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    try:
        validate_package(args.root.resolve(), args.expected)
    except ReleaseVersionError as error:
        print(f"release version check failed: {error}", file=sys.stderr)
        return 1
    print(
        f"release version {args.expected} is consistent across Cargo.toml and Cargo.lock"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
