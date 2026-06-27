#!/usr/bin/env python3
"""Project-local validator for devctl patch manifests.

This is intentionally small. It checks project rules that are easy to break
before devctl dry-run sees the archive.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

REQUIRED_EXCLUDES = {
    ".git/",
    "target/",
    "dist/",
    "build/",
    "coverage/",
    "__pycache__/",
    ".env",
    ".env.*",
    "*.sqlite",
    "*.db",
    "*.dbonrs",
}

MAIN_BRANCH = "master"

VERSION_RE = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$")
ALLOWED_VERSION_BUMPS = {"bootstrap", "quantum", "micro", "minor", "major"}
ALLOWED_COMPATIBILITY = {"compatible", "breaking", "documentation-only", "internal"}



def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except FileNotFoundError:
        raise SystemExit(f"manifest not found: {path}")
    except json.JSONDecodeError as exc:
        raise SystemExit(f"invalid json: {exc}") from exc

    if not isinstance(value, dict):
        raise SystemExit("manifest root must be an object")
    return value


def require_object(root: dict[str, Any], key: str, errors: list[str]) -> dict[str, Any]:
    value = root.get(key)
    if not isinstance(value, dict):
        errors.append(f"{key} must be an object")
        return {}
    return value


def require_non_empty_string(root: dict[str, Any], key: str, errors: list[str]) -> None:
    value = root.get(key)
    if not isinstance(value, str) or not value.strip():
        errors.append(f"{key} must be a non-empty string")



def validate_version_intent(manifest: dict[str, Any], errors: list[str]) -> None:
    value = manifest.get("version")
    if value is None:
        errors.append("version must be present for devctl patch-quantum versioning")
        return
    if not isinstance(value, dict):
        errors.append("version must be an object")
        return

    if value.get("schema") != "devctl-version-intent-v1":
        errors.append("version.schema must be 'devctl-version-intent-v1'")

    base = value.get("base")
    bump = value.get("bump")
    next_version = value.get("next")
    quantum = value.get("quantum")

    if bump not in ALLOWED_VERSION_BUMPS:
        errors.append("version.bump must be one of: " + ", ".join(sorted(ALLOWED_VERSION_BUMPS)))

    if bump == "bootstrap":
        if base is not None:
            errors.append("version.base must be null for bootstrap bump")
    elif not isinstance(base, str) or not VERSION_RE.match(base):
        errors.append("version.base must use MAJOR.MINOR.MICRO.QUANTUM")

    if not isinstance(next_version, str) or not VERSION_RE.match(next_version):
        errors.append("version.next must use MAJOR.MINOR.MICRO.QUANTUM")

    if not isinstance(quantum, int) or quantum <= 0:
        errors.append("version.quantum must be a positive integer")
    elif isinstance(next_version, str) and VERSION_RE.match(next_version):
        next_quantum = int(next_version.rsplit(".", 1)[1])
        if quantum != next_quantum:
            errors.append("version.quantum must match the last component of version.next")

    reason = value.get("reason")
    if not isinstance(reason, str) or not reason.strip():
        errors.append("version.reason must be a non-empty string")

    public_surface = value.get("publicSurface")
    if not isinstance(public_surface, list) or not public_surface:
        errors.append("version.publicSurface must be a non-empty list")
    elif not all(isinstance(item, str) and item.strip() for item in public_surface):
        errors.append("version.publicSurface items must be non-empty strings")

    compatibility = value.get("compatibility")
    if compatibility not in ALLOWED_COMPATIBILITY:
        errors.append("version.compatibility must be one of: " + ", ".join(sorted(ALLOWED_COMPATIBILITY)))


def validate(manifest: dict[str, Any]) -> list[str]:
    errors: list[str] = []

    if manifest.get("formatVersion") != 1:
        errors.append("formatVersion must be 1")

    for key in ["patchId", "title", "summary", "kind", "createdAt"]:
        require_non_empty_string(manifest, key, errors)

    base = require_object(manifest, "base", errors)
    branch = base.get("branch")
    if branch != MAIN_BRANCH:
        errors.append(f"base.branch must be {MAIN_BRANCH!r}, got {branch!r}")
    if "expectedHead" not in base:
        errors.append("base.expectedHead must be present; use null when not pinned")

    apply = require_object(manifest, "apply", errors)
    if apply.get("filesRoot") != "files":
        errors.append("apply.filesRoot must be 'files'")

    delete = apply.get("delete", [])
    if delete is None:
        delete = []
    if not isinstance(delete, list):
        errors.append("apply.delete must be a list")
    else:
        for index, item in enumerate(delete):
            if not isinstance(item, dict):
                errors.append(f"apply.delete[{index}] must be an object, not {type(item).__name__}")
                continue
            path = item.get("path")
            if not isinstance(path, str) or not path.strip():
                errors.append(f"apply.delete[{index}].path must be a non-empty string")
            if "required" in item and not isinstance(item["required"], bool):
                errors.append(f"apply.delete[{index}].required must be boolean when present")
            if "recursive" in item and not isinstance(item["recursive"], bool):
                errors.append(f"apply.delete[{index}].recursive must be boolean when present")

    checks = manifest.get("checks")
    if not isinstance(checks, list) or not checks:
        errors.append("checks must be a non-empty list")
    else:
        for index, check in enumerate(checks):
            if not isinstance(check, dict):
                errors.append(f"checks[{index}] must be an object")
                continue
            for key in ["name", "cwd", "command"]:
                value = check.get(key)
                if not isinstance(value, str) or not value.strip():
                    errors.append(f"checks[{index}].{key} must be a non-empty string")
            timeout = check.get("timeoutSeconds")
            if not isinstance(timeout, int) or timeout <= 0:
                errors.append(f"checks[{index}].timeoutSeconds must be a positive integer")
            required_commands = check.get("requiredCommands", [])
            if not isinstance(required_commands, list):
                errors.append(f"checks[{index}].requiredCommands must be a list when present")

    commit = require_object(manifest, "commit", errors)
    require_non_empty_string(commit, "message", errors)

    push = require_object(manifest, "push", errors)
    if push.get("branch") != MAIN_BRANCH:
        errors.append(f"push.branch must be {MAIN_BRANCH!r}, got {push.get('branch')!r}")
    require_non_empty_string(push, "remote", errors)

    validate_version_intent(manifest, errors)

    archive = require_object(manifest, "archive", errors)
    require_non_empty_string(archive, "nameSlug", errors)
    exclude = archive.get("exclude")
    if not isinstance(exclude, list):
        errors.append("archive.exclude must be a list")
    else:
        missing = sorted(REQUIRED_EXCLUDES.difference(str(item) for item in exclude))
        if missing:
            errors.append("archive.exclude misses: " + ", ".join(missing))

    return errors


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        print("usage: validate_patch_manifest.py path/to/manifest.json", file=sys.stderr)
        return 2

    manifest_path = Path(argv[1])
    manifest = load_json(manifest_path)
    errors = validate(manifest)
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        return 1

    print(f"OK: {manifest_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
