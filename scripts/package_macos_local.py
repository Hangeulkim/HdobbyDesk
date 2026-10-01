#!/usr/bin/env python3
"""Assemble and ad-hoc sign a local HdobbyDesk macOS candidate without Xcode.

This fallback reuses an already verified native macOS runner and can optionally replace
its App.framework with one produced by ``flutter assemble``. It never installs,
launches, overwrites, or contacts a server.
"""

import argparse
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import stat
import subprocess
import sys
import tempfile


ROOT = Path(__file__).resolve().parents[1]
BUNDLE_ID = "com.hdobby.hdobbydesk"
APP_NAME = "HdobbyDesk"
CORE = Path("Contents/Frameworks/liblibhdobbydesk.dylib")
FLUTTER_APP = Path("Contents/Frameworks/App.framework")
FLUTTER_APP_BINARY = Path("Versions/A/App")
SERVICE = Path("Contents/MacOS/service")
CORE_SOURCE_NAME = "liblibhdobbydesk.dylib"
SERVICE_SOURCE_NAME = "service"
RUNNER_CORE_DEPENDENCY = "@rpath/liblibhdobbydesk.dylib"
RUNNER_CORE_SYMBOL = "_hdobbydesk_core_main"
LEGACY_RUNNER_MARKERS = ("liblibrustdesk.dylib", "_rustdesk_core_main")
REQUIRED_CORE_MARKERS = (b"hdobby2:",)
GENERATED_NATIVE_INPUTS = {ROOT / "src/version.rs"}
PRIVACY_USAGE_DESCRIPTIONS = {
    "NSLocalNetworkUsageDescription": (
        "HdobbyDesk connects directly to devices you choose on your local network."
    ),
    "NSMicrophoneUsageDescription": (
        "HdobbyDesk uses the microphone only when you enable audio during a remote "
        "session."
    ),
}
FORBIDDEN_ENDPOINTS = tuple(
    value.encode()
    for value in (
        "rs-ny.rustdesk.com",
        "rs-sg.rustdesk.com",
        "rs-cn.rustdesk.com",
        "api.rustdesk.com",
        "admin.rustdesk.com",
        "update.rustdesk.com",
        "rendezvous.rustdesk.com",
        "relay.rustdesk.com",
    )
)
PRIVATE_NAMES = {
    "hdobby-direct-tls-identity.json",
    "HdobbyDesk.toml",
    "HdobbyDesk2.toml",
    "HdobbyDesk_local.toml",
    "RustDesk.toml",
    "RustDesk2.toml",
    "RustDesk_local.toml",
}
PRIVATE_SUFFIXES = {".pem", ".key", ".p12", ".pfx"}
SAFE_MACHO_PATH_PREFIXES = (
    "@rpath/",
    "@loader_path/",
    "@executable_path/",
    "/usr/lib/",
    "/System/Library/",
)


class PackagingError(RuntimeError):
    pass


def command(*args: str) -> None:
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode:
        raise PackagingError(f"{Path(args[0]).name} failed")


def command_output(*args: str) -> str:
    result = subprocess.run(args, capture_output=True, text=True)
    if result.returncode:
        raise PackagingError(f"{Path(args[0]).name} failed")
    return result.stdout


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def bundle_entries(root: Path):
    entries = []
    for directory, directories, filenames in os.walk(root, followlinks=False):
        base = Path(directory)
        entries.extend(base / name for name in directories)
        entries.extend(base / name for name in filenames)
    return sorted(entries)


def regular_files(root: Path):
    return [path for path in bundle_entries(root) if stat.S_ISREG(path.lstat().st_mode)]


def parse_macho_rpaths(output: str) -> list[str]:
    lines = output.splitlines()
    paths = []
    for index, line in enumerate(lines):
        if line.strip() != "cmd LC_RPATH":
            continue
        for candidate in lines[index + 1:index + 8]:
            match = re.match(r"\s*path (.+?) \(offset ", candidate)
            if match:
                paths.append(match.group(1))
                break
    return paths


def parse_macho_dependencies(output: str) -> list[str]:
    dependencies = []
    for line in output.splitlines()[1:]:
        value = line.strip().split(" (compatibility", 1)[0]
        if value:
            dependencies.append(value)
    return dependencies


def safe_macho_path(path: str) -> bool:
    return path.startswith(SAFE_MACHO_PATH_PREFIXES)


def macho_files(root: Path) -> list[Path]:
    paths = []
    for path in regular_files(root):
        result = subprocess.run(
            ["file", "-b", str(path)], capture_output=True, text=True
        )
        if result.returncode:
            raise PackagingError("file failed")
        if "Mach-O" in result.stdout:
            paths.append(path)
    return paths


def harden_bundle(app: Path) -> dict[str, int]:
    permission_changes = 0
    for path in regular_files(app):
        mode = stat.S_IMODE(path.stat().st_mode)
        hardened = mode & ~0o022
        if mode != hardened:
            os.chmod(path, hardened)
            permission_changes += 1

    removed_rpaths = 0
    for path in macho_files(app):
        rpaths = parse_macho_rpaths(command_output("otool", "-l", str(path)))
        for rpath in rpaths:
            if not safe_macho_path(rpath):
                command("install_name_tool", "-delete_rpath", rpath, str(path))
                removed_rpaths += 1
    return {
        "removed_nonportable_rpaths": removed_rpaths,
        "removed_group_or_world_write_permissions": permission_changes,
    }


def validate_hardened_bundle(app: Path) -> None:
    for path in regular_files(app):
        if stat.S_IMODE(path.stat().st_mode) & 0o022:
            raise PackagingError("candidate contains a group/world-writable file")
    for path in macho_files(app):
        rpaths = parse_macho_rpaths(command_output("otool", "-l", str(path)))
        if any(not safe_macho_path(value) for value in rpaths):
            raise PackagingError("candidate contains a non-portable runtime path")
        dependencies = parse_macho_dependencies(
            command_output("otool", "-L", str(path))
        )
        if any(not safe_macho_path(value) for value in dependencies):
            raise PackagingError("candidate contains an external library dependency")


def validate_bundle_filesystem(app: Path) -> None:
    root = app.resolve(strict=True)
    for path in bundle_entries(app):
        mode = path.lstat().st_mode
        if path.name in PRIVATE_NAMES or path.suffix.lower() in PRIVATE_SUFFIXES:
            raise PackagingError("candidate contains a runtime credential/config file")
        if stat.S_ISLNK(mode):
            try:
                target = path.resolve(strict=True)
                target.relative_to(root)
            except (OSError, ValueError) as error:
                raise PackagingError("candidate contains an external or broken link") from error
        elif not (stat.S_ISREG(mode) or stat.S_ISDIR(mode)):
            raise PackagingError("candidate contains a special filesystem entry")


def contains_forbidden_endpoint(path: Path) -> bool:
    longest = max(map(len, FORBIDDEN_ENDPOINTS))
    carry = b""
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            data = carry + block.lower()
            if any(endpoint in data for endpoint in FORBIDDEN_ENDPOINTS):
                return True
            carry = data[-(longest - 1) :]
    return False


def file_contains_marker(path: Path, marker: bytes) -> bool:
    if not marker:
        raise ValueError("marker must not be empty")
    carry = b""
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            data = carry + block
            if marker in data:
                return True
            carry = data[-(len(marker) - 1) :] if len(marker) > 1 else b""
    return False


def validate_ui_bundle(path: Path) -> None:
    if not path.is_dir() or path.suffix != ".app":
        raise PackagingError("UI input must be an existing .app bundle")
    validate_bundle_filesystem(path)
    info_path = path / "Contents/Info.plist"
    try:
        with info_path.open("rb") as source:
            info = plistlib.load(source)
    except (OSError, plistlib.InvalidFileException) as error:
        raise PackagingError("UI bundle has no valid Info.plist") from error
    if info.get("CFBundleIdentifier") != BUNDLE_ID:
        raise PackagingError("UI bundle identifier is not HdobbyDesk")
    if info.get("CFBundleExecutable") != APP_NAME:
        raise PackagingError("UI bundle executable is not HdobbyDesk")
    runner = path / "Contents" / "MacOS" / APP_NAME
    dependencies = command_output("otool", "-L", str(runner))
    undefined_symbols = command_output("nm", "-u", str(runner))
    if (
        RUNNER_CORE_DEPENDENCY not in dependencies
        or RUNNER_CORE_SYMBOL not in undefined_symbols
        or any(
            marker in dependencies or marker in undefined_symbols
            for marker in LEGACY_RUNNER_MARKERS
        )
    ):
        raise PackagingError("UI runner does not load the renamed HdobbyDesk core")
    command("codesign", "--verify", "--deep", "--strict", str(path))


def validate_native(path: Path, label: str) -> None:
    if not path.is_file() or path.is_symlink():
        raise PackagingError(f"{label} must be a regular file")
    result = subprocess.run(
        ["lipo", "-archs", str(path)], capture_output=True, text=True
    )
    if result.returncode or "arm64" not in result.stdout.split():
        raise PackagingError(f"{label} must contain an arm64 Mach-O slice")


def validate_native_core_protocol(path: Path) -> None:
    missing = [marker for marker in REQUIRED_CORE_MARKERS
               if not file_contains_marker(path, marker)]
    if missing:
        raise PackagingError(
            "native core does not contain the required HdobbyDesk pairing protocol"
        )


def validate_flutter_app_framework(path: Path) -> None:
    if not path.is_dir() or path.name != "App.framework":
        raise PackagingError("Flutter UI input must be an App.framework directory")
    validate_bundle_filesystem(path)
    validate_native(path / FLUTTER_APP_BINARY, "Flutter App.framework binary")


def native_build_inputs() -> list[Path]:
    inputs = [ROOT / "Cargo.toml", ROOT / "Cargo.lock", ROOT / "build.rs"]
    for directory in [ROOT / "src", ROOT / "libs"]:
        for path in directory.rglob("*"):
            if (
                path.is_file()
                and path not in GENERATED_NATIVE_INPUTS
                and path.suffix in {".rs", ".mm", ".h", ".plist", ".scpt"}
            ):
                inputs.append(path)
    return inputs


def validate_native_provenance(
    core: Path, service: Path, inputs: list[Path] | None = None
) -> None:
    if core.name != CORE_SOURCE_NAME or service.name != SERVICE_SOURCE_NAME:
        raise PackagingError("native inputs must be the direct Cargo release outputs")
    build_inputs = native_build_inputs() if inputs is None else inputs
    try:
        newest_input = max(path.stat().st_mtime_ns for path in build_inputs)
        if core.stat().st_mtime_ns < newest_input or service.stat().st_mtime_ns < newest_input:
            raise PackagingError("native release outputs are older than their source inputs")
    except OSError as error:
        raise PackagingError("native build provenance could not be verified") from error


def normalize_privacy_usage_descriptions(app: Path) -> None:
    info_path = app / "Contents/Info.plist"
    try:
        with info_path.open("rb") as source:
            info = plistlib.load(source)
    except (OSError, plistlib.InvalidFileException) as error:
        raise PackagingError("candidate has no valid Info.plist") from error
    info.update(PRIVACY_USAGE_DESCRIPTIONS)
    try:
        with info_path.open("wb") as output:
            plistlib.dump(info, output, sort_keys=False)
    except OSError as error:
        raise PackagingError("candidate Info.plist could not be updated") from error


def validate_no_runtime_material(app: Path) -> None:
    validate_bundle_filesystem(app)
    for path in regular_files(app):
        if contains_forbidden_endpoint(path):
            raise PackagingError("candidate contains a forbidden public service endpoint")


def write_receipt(
    path: Path,
    app: Path,
    ui: Path,
    flutter_app_framework: Path | None,
    hardening: dict[str, int],
) -> None:
    files = regular_files(app)
    receipt = {
        "tool": "package_macos_local.py",
        "output_bundle": app.name,
        "source_ui_bundle": ui.name,
        "bundle_identifier": BUNDLE_ID,
        "architecture": "arm64",
        "bundle_file_count": len(files),
        "native_core_sha256": sha256(app / CORE),
        "direct_pairing_protocol": "hdobby2",
        "service_sha256": sha256(app / SERVICE),
        "flutter_app_sha256": sha256(app / FLUTTER_APP / FLUTTER_APP_BINARY),
        "codesign_deep_strict": "PASS",
        "forbidden_public_service_endpoint_hits": 0,
        "contains_runtime_endpoint_or_credentials": False,
        "privacy_usage_descriptions": PRIVACY_USAGE_DESCRIPTIONS,
        "bundle_hardening": {
            **hardening,
            "remaining_nonportable_rpaths": 0,
            "remaining_external_library_dependencies": 0,
            "remaining_group_or_world_writable_files": 0,
        },
        "ui_rebuilt_from_source": flutter_app_framework is not None,
        "source_flutter_framework": (
            flutter_app_framework.name if flutter_app_framework is not None else None
        ),
        "distribution": "Local ad-hoc development signature; not notarized.",
    }
    with path.open("x", encoding="utf-8") as output:
        json.dump(receipt, output, indent=2, sort_keys=True)
        output.write("\n")


def parse_args():
    parser = argparse.ArgumentParser()
    parser.add_argument("--ui-app", required=True, type=Path)
    parser.add_argument("--flutter-app-framework", type=Path)
    parser.add_argument("--native-core", required=True, type=Path)
    parser.add_argument("--service", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("--receipt", required=True, type=Path)
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    requested_output = args.output.expanduser().absolute()
    requested_receipt = args.receipt.expanduser().absolute()
    ui = args.ui_app.expanduser().absolute()
    flutter_app_framework = (
        args.flutter_app_framework.expanduser().absolute()
        if args.flutter_app_framework is not None
        else None
    )
    core = args.native_core.expanduser().absolute()
    service = args.service.expanduser().absolute()
    if requested_output.suffix != ".app":
        raise PackagingError("output must end in .app")
    if (requested_output.exists() or requested_output.is_symlink()
            or requested_receipt.exists() or requested_receipt.is_symlink()):
        raise PackagingError("output and receipt paths must not already exist")
    if not requested_output.parent.is_dir() or not requested_receipt.parent.is_dir():
        raise PackagingError("output directory must already exist")
    output_parent = requested_output.parent.resolve(strict=True)
    receipt_parent = requested_receipt.parent.resolve(strict=True)
    if output_parent != receipt_parent:
        raise PackagingError("output and receipt must share a parent directory")
    output = output_parent / requested_output.name
    receipt = receipt_parent / requested_receipt.name
    try:
        ui_root = ui.resolve(strict=True)
    except OSError as error:
        raise PackagingError("UI input must be an existing .app bundle") from error
    try:
        output_parent.relative_to(ui_root)
    except ValueError:
        pass
    else:
        raise PackagingError("output directory must not be inside the UI bundle")
    validate_ui_bundle(ui)
    if flutter_app_framework is not None:
        validate_flutter_app_framework(flutter_app_framework)
    validate_native_provenance(core, service)
    validate_native(core, "native core")
    validate_native_core_protocol(core)
    validate_native(service, "service")

    temporary_root = Path(
        tempfile.mkdtemp(prefix=".hdobby-macos-package-", dir=output.parent)
    )
    temporary_app = temporary_root / output.name
    try:
        shutil.copytree(ui, temporary_app, symlinks=True)
        validate_ui_bundle(temporary_app)
        if flutter_app_framework is not None:
            shutil.rmtree(temporary_app / FLUTTER_APP)
            shutil.copytree(
                flutter_app_framework,
                temporary_app / FLUTTER_APP,
                symlinks=True,
            )
            validate_flutter_app_framework(temporary_app / FLUTTER_APP)
        normalize_privacy_usage_descriptions(temporary_app)
        shutil.copy2(core, temporary_app / CORE)
        shutil.copy2(service, temporary_app / SERVICE)
        os.chmod(temporary_app / CORE, 0o755)
        os.chmod(temporary_app / SERVICE, 0o755)
        validate_native(temporary_app / CORE, "copied native core")
        validate_native_core_protocol(temporary_app / CORE)
        validate_native(temporary_app / SERVICE, "copied service")
        hardening = harden_bundle(temporary_app)
        validate_hardened_bundle(temporary_app)
        validate_no_runtime_material(temporary_app)
        command("codesign", "--force", "--deep", "--sign", "-", str(temporary_app))
        command("codesign", "--verify", "--deep", "--strict", str(temporary_app))
        validate_hardened_bundle(temporary_app)
        temporary_app.rename(output)
        write_receipt(
            receipt, output, ui, flutter_app_framework, hardening
        )
    except Exception:
        if output.exists() and not receipt.exists():
            shutil.rmtree(output)
        raise
    finally:
        if temporary_app.exists():
            shutil.rmtree(temporary_app)
        try:
            temporary_root.rmdir()
        except OSError:
            pass
    print("Local macOS candidate packaged and verified.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except PackagingError as error:
        print(f"Packaging failed: {error}", file=sys.stderr)
        raise SystemExit(1)
    except Exception:
        print("Packaging failed: unexpected local filesystem error", file=sys.stderr)
        raise SystemExit(1)
