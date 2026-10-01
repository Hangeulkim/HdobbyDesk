#!/usr/bin/env python3
"""Package a local Windows native core without operational configuration."""

from __future__ import annotations

import argparse
import hashlib
import json
import shutil
from pathlib import Path


RUNTIME_NAMES = ("libgcc_s_seh-1.dll", "libstdc++-6.dll", "libwinpthread-1.dll")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def describe(path: Path) -> dict[str, int | str]:
    return {"bytes": path.stat().st_size, "sha256": sha256(path)}


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--core", type=Path, required=True)
    parser.add_argument("--runtime-source", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    if not args.core.is_file():
        parser.error(f"core DLL does not exist: {args.core}")

    args.output.mkdir(parents=True, exist_ok=False)
    shutil.copy2(args.core, args.output / "libhdobbydesk.dll")
    for name in RUNTIME_NAMES:
        source = args.runtime_source / name
        if not source.is_file():
            parser.error(f"runtime DLL does not exist: {source}")
        shutil.copy2(source, args.output / name)

    archive_name = f"HdobbyDesk-windows-core-collab-{args.version}"
    temporary_zip_base = args.output.parent / f".{archive_name}"
    temporary_zip = Path(shutil.make_archive(str(temporary_zip_base), "zip", args.output, "."))
    zip_path = args.output / f"{archive_name}.zip"
    temporary_zip.replace(zip_path)
    files = {
        path.name: describe(path)
        for path in sorted(args.output.glob("*.dll"), key=lambda item: item.name)
    }
    receipt = {
        "artifact": "HdobbyDesk Windows x64 native core",
        "authenticode_signed": False,
        "collaborative_cursor": True,
        "contains_operational_configuration": False,
        "files": files,
        "matching_rebranded_windows_flutter_shell_required": True,
        "keyboard_to_per_connection_xinput": True,
        "management_window_capture_exclusion": True,
        "scope": "native core; requires the matching rebranded Windows Flutter shell",
        "target": "x86_64-pc-windows-gnu",
        "version": args.version,
        "xinput_capacity_check": True,
        "zip": {"bytes": zip_path.stat().st_size, "name": zip_path.name, "sha256": sha256(zip_path)},
    }
    receipt_path = args.output / f"hdobbydesk-windows-{args.version}-build.json"
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(receipt_path)
    print(json.dumps(receipt, indent=2, sort_keys=True))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
