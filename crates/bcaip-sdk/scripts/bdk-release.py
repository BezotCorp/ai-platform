#!/usr/bin/env python3
import argparse
import json
import re
import subprocess
from pathlib import Path
from typing import Any, TypedDict, cast

ROOT = Path(__file__).resolve().parents[3]
CONFIG = ROOT / "release-plz.toml"
SDK = "bcaip-sdk"

class CargoDependency(TypedDict):
    name: str

class CargoPackage(TypedDict):
    name: str
    version: str
    dependencies: list[CargoDependency]

class CargoMetadata(TypedDict):
    packages: list[CargoPackage]

def bdk_crates() -> list[str]:
    text = CONFIG.read_text()
    package_sections: list[str] = re.findall(
        r"(?ms)^\[\[package\]\]\s*$(.*?)(?=^\[|\Z)",
        text,
    )

    crates: list[str] = []

    for section in package_sections:
        name = re.search(
            r'^name\s*=\s*"([^"]+)"',
            section,
            re.MULTILINE,
        )
        release = re.search(
            r"^release\s*=\s*true\s*$",
            section,
            re.MULTILINE,
        )
        group = re.search(
            r'^version_group\s*=\s*"bdk"\s*$',
            section,
            re.MULTILINE,
        )

        if name is not None and release is not None and group is not None:
            crates.append(name.group(1))

    if not crates:
        raise SystemExit("release-plz.toml does not define any BDK crates")

    if len(crates) != len(set(crates)):
        raise SystemExit("release-plz.toml contains duplicate BDK crates")

    if SDK not in crates:
        raise SystemExit(f"release-plz.toml BDK group must contain {SDK}")

    return crates

def metadata() -> CargoMetadata:
    result = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )

    parsed: Any = json.loads(result.stdout)
    return cast(CargoMetadata, parsed)

def package_version(path: Path, section: str) -> str:
    text = path.read_text()

    section_match = re.search(
        rf"(?ms)^\[{re.escape(section)}\]\s*$(.*?)(?=^\[|\Z)",
        text,
    )
    if section_match is None:
        raise SystemExit(
            f"missing [{section}] in {path.relative_to(ROOT)}"
        )

    version_match = re.search(
        r'^version\s*=\s*"([^"]+)"',
        section_match.group(1),
        re.MULTILINE,
    )
    if version_match is None:
        raise SystemExit(
            f"missing version in [{section}] in {path.relative_to(ROOT)}"
        )

    return version_match.group(1)

def python_version(rust_version: str) -> str:
    match = re.fullmatch(
        r"(\d+)\.(\d+)\.(\d+)(?:-alpha\.(\d+))?",
        rust_version,
    )
    if match is None:
        raise SystemExit(
            "version must look like 0.1.0 or 0.1.0-alpha.0"
        )

    major, minor, patch, alpha = match.groups()

    if alpha is None:
        return f"{major}.{minor}.{patch}"

    return f"{major}.{minor}.{patch}a{alpha}"

def check_config() -> None:
    crates = bdk_crates()
    cargo_metadata = metadata()

    packages: dict[str, CargoPackage] = {
        package["name"]: package
        for package in cargo_metadata["packages"]
    }

    errors: list[str] = []

    for crate in crates:
        if crate not in packages:
            errors.append(
                f"release-plz.toml BDK crate is not a workspace package: {crate}"
            )

    workflow = ROOT / ".github/workflows/bdk-release-pr.yml"
    workflow_text = workflow.read_text()

    begin_marker = "      # BEGIN BDK CRATE PATHS\n"
    end_marker = "      # END BDK CRATE PATHS\n"

    if (
        workflow_text.count(begin_marker) != 1
        or workflow_text.count(end_marker) != 1
    ):
        errors.append(
            f"{workflow.relative_to(ROOT)} must contain one BDK crate paths marker block"
        )
    else:
        path_block = (
            workflow_text
            .split(begin_marker, 1)[1]
            .split(end_marker, 1)[0]
        )

        expected_block = "".join(
            f'      - "crates/{crate}/**"\n'
            for crate in crates
        )

        if path_block != expected_block:
            errors.append(
                f"{workflow.relative_to(ROOT)} BDK crate paths do not match release-plz.toml"
            )

    positions: dict[str, int] = {
        crate: index
        for index, crate in enumerate(crates)
    }

    for crate in crates:
        package = packages.get(crate)

        if package is None:
            continue

        for dependency in package["dependencies"]:
            dependency_name = dependency["name"]

            if (
                dependency_name in positions
                and positions[dependency_name] >= positions[crate]
            ):
                errors.append(
                    f"release-plz.toml must list {dependency_name} "
                    f"before dependent crate {crate}"
                )

    if errors:
        raise SystemExit("\n".join(errors))

    print(
        f"Validated {len(crates)} BDK crates "
        "and their publication order"
    )

def check_version(rust_version: str) -> None:
    crates = bdk_crates()
    expected_python = python_version(rust_version)

    errors: list[str] = []

    dependency_names: set[str] = set(crates) - {SDK}

    for crate in crates:
        path = ROOT / "crates" / crate / "Cargo.toml"
        text = path.read_text()

        actual = package_version(path, "package")

        if actual != rust_version:
            errors.append(
                f"{path.relative_to(ROOT)}: "
                f"expected {rust_version}, found {actual}"
            )

        for dependency in dependency_names:
            dependency_matches: list[re.Match[str]] = list(
                re.finditer(
                    rf"(?m)^{re.escape(dependency)}\s*=\s*\{{([^}}]*)\}}",
                    text,
                )
            )

            for dependency_match in dependency_matches:
                version_match = re.search(
                    r'version\s*=\s*"([^"]+)"',
                    dependency_match.group(1),
                )

                if version_match is None:
                    errors.append(
                        f"{path.relative_to(ROOT)}: "
                        f"{dependency} is missing a version requirement"
                    )
                elif version_match.group(1) != rust_version:
                    errors.append(
                        f"{path.relative_to(ROOT)}: "
                        f"{dependency} expected {rust_version}, "
                        f"found {version_match.group(1)}"
                    )

    pyproject = ROOT / "crates/bcaip-sdk/python/pyproject.toml"
    actual_python = package_version(pyproject, "project")

    if actual_python != expected_python:
        errors.append(
            f"{pyproject.relative_to(ROOT)}: "
            f"expected {expected_python}, found {actual_python}"
        )

    if errors:
        raise SystemExit("\n".join(errors))

    print(
        f"Validated Rust/Maven version {rust_version} "
        f"and Python version {expected_python}"
    )

def main() -> None:
    parser = argparse.ArgumentParser()

    subparsers = parser.add_subparsers(
        dest="command",
        required=True,
    )

    subparsers.add_parser("crates")
    subparsers.add_parser("check-config")

    check = subparsers.add_parser("check-version")
    check.add_argument("version")

    args = parser.parse_args()

    if args.command == "crates":
        print("\n".join(bdk_crates()))
    elif args.command == "check-config":
        check_config()
    elif args.command == "check-version":
        check_version(args.version)

if __name__ == "__main__":
    main()