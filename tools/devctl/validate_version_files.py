#!/usr/bin/env python3
"""Validate project version files for devctl patch-quantum versioning."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

VERSION_RE = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
SEMVER_RE = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")


def fail(message: str) -> int:
    print(f"ERROR: {message}", file=sys.stderr)
    return 1


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        raise SystemExit(f"ERROR: missing {path}")
    except json.JSONDecodeError as exc:
        raise SystemExit(f"ERROR: invalid JSON in {path}: {exc}") from exc
    if not isinstance(value, dict):
        raise SystemExit(f"ERROR: {path} root must be an object")
    return value


def main(argv: list[str]) -> int:
    root = Path(argv[1]) if len(argv) == 2 else Path.cwd()
    version_path = root / "VERSION"
    version_json_path = root / "VERSION.json"
    changelog_path = root / "CHANGELOG.md"

    if not version_path.exists():
        return fail("missing VERSION")
    if not version_json_path.exists():
        return fail("missing VERSION.json")
    if not changelog_path.exists():
        return fail("missing CHANGELOG.md")

    version_text = version_path.read_text(encoding="utf-8").strip()
    match = VERSION_RE.match(version_text)
    if not match:
        return fail("VERSION must use MAJOR.MINOR.MICRO.QUANTUM")

    major, minor, micro, quantum = (int(part) for part in match.groups())
    data = load_json(version_json_path)

    required = {
        "schema": "devctl-version-v1",
        "version": version_text,
        "semver": f"{major}.{minor}.{micro}",
        "major": major,
        "minor": minor,
        "micro": micro,
        "quantum": quantum,
    }
    for key, expected in required.items():
        if data.get(key) != expected:
            return fail(f"VERSION.json field {key!r} must be {expected!r}, got {data.get(key)!r}")

    if not SEMVER_RE.match(str(data.get("semver", ""))):
        return fail("VERSION.json semver must use MAJOR.MINOR.MICRO")
    if not isinstance(data.get("released"), bool):
        return fail("VERSION.json released must be boolean")
    if not isinstance(data.get("updatedAt"), str) or not data["updatedAt"].strip():
        return fail("VERSION.json updatedAt must be a non-empty string")
    if not isinstance(data.get("lastPatchId"), str) or not data["lastPatchId"].strip():
        return fail("VERSION.json lastPatchId must be a non-empty string")
    if "lastPatchSha256" not in data:
        return fail("VERSION.json lastPatchSha256 must be present; use null when unknown")
    if data["lastPatchSha256"] is not None and not isinstance(data["lastPatchSha256"], str):
        return fail("VERSION.json lastPatchSha256 must be string or null")

    changelog = changelog_path.read_text(encoding="utf-8")
    if version_text not in changelog:
        return fail("CHANGELOG.md must mention current VERSION")
    if str(data["lastPatchId"]) not in changelog:
        return fail("CHANGELOG.md must mention VERSION.json lastPatchId")

    print(f"OK: version files {version_text}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
