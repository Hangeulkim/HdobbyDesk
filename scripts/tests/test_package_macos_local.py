"""Filesystem and secret-boundary tests for the local macOS packager."""

import importlib.util
import os
from pathlib import Path
import plistlib
import stat
import tempfile
import unittest
from unittest import mock


ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location(
    "package_macos_local", ROOT / "scripts/package_macos_local.py"
)
PACKAGER = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(PACKAGER)


class MacPackageBoundaryTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hdobby-mac-package-test-")
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def test_endpoint_split_across_read_blocks_is_rejected(self):
        marker = PACKAGER.FORBIDDEN_ENDPOINTS[0]
        path = self.root / "binary"
        path.write_bytes(b"x" * (1024 * 1024 - len(marker) // 2) + marker)
        self.assertTrue(PACKAGER.contains_forbidden_endpoint(path))

    def test_large_clean_file_is_accepted(self):
        path = self.root / "binary"
        path.write_bytes(b"x" * (1024 * 1024 + 32))
        self.assertFalse(PACKAGER.contains_forbidden_endpoint(path))

    def test_required_core_marker_can_cross_read_blocks(self):
        marker = PACKAGER.REQUIRED_CORE_MARKERS[0]
        path = self.root / "native-core"
        path.write_bytes(b"x" * (1024 * 1024 - len(marker) // 2) + marker)
        PACKAGER.validate_native_core_protocol(path)

    def test_legacy_core_without_current_pairing_protocol_is_rejected(self):
        path = self.root / "native-core"
        path.write_bytes(b"legacy hdobby1: pairing only")
        with self.assertRaisesRegex(PACKAGER.PackagingError, "pairing protocol"):
            PACKAGER.validate_native_core_protocol(path)

    def test_flutter_app_framework_has_expected_shape(self):
        framework = self.root / "App.framework"
        binary = framework / PACKAGER.FLUTTER_APP_BINARY
        binary.parent.mkdir(parents=True)
        binary.write_bytes(b"test-only")
        with mock.patch.object(PACKAGER, "validate_native") as validate_native:
            PACKAGER.validate_flutter_app_framework(framework)
        validate_native.assert_called_once_with(
            binary, "Flutter App.framework binary"
        )

        wrong_name = self.root / "Unexpected.framework"
        wrong_name.mkdir()
        with self.assertRaisesRegex(PACKAGER.PackagingError, "App.framework"):
            PACKAGER.validate_flutter_app_framework(wrong_name)

    def test_runtime_config_name_is_rejected_without_reading_values(self):
        (self.root / "HdobbyDesk.toml").write_text("test-only")
        with self.assertRaisesRegex(PACKAGER.PackagingError, "runtime credential"):
            PACKAGER.validate_no_runtime_material(self.root)

    def test_internal_framework_style_link_is_allowed(self):
        versions = self.root / "Example.framework/Versions/A"
        versions.mkdir(parents=True)
        (versions / "Example").write_bytes(b"local code")
        (versions.parent / "Current").symlink_to("A")
        (self.root / "Example.framework/Example").symlink_to(
            "Versions/Current/Example"
        )
        PACKAGER.validate_no_runtime_material(self.root)

    def test_external_and_broken_links_are_rejected(self):
        outside = self.root.parent / f"{self.root.name}-outside"
        outside.write_text("test-only")
        try:
            (self.root / "external").symlink_to(outside)
            with self.assertRaisesRegex(PACKAGER.PackagingError, "external or broken"):
                PACKAGER.validate_no_runtime_material(self.root)
            (self.root / "external").unlink()
            (self.root / "broken").symlink_to("missing")
            with self.assertRaisesRegex(PACKAGER.PackagingError, "external or broken"):
                PACKAGER.validate_no_runtime_material(self.root)
        finally:
            outside.unlink(missing_ok=True)

    def test_fifo_is_rejected_without_opening_it(self):
        fifo = self.root / "unexpected-fifo"
        os.mkfifo(fifo)
        with self.assertRaisesRegex(PACKAGER.PackagingError, "special filesystem"):
            PACKAGER.validate_no_runtime_material(self.root)

    def test_privacy_usage_descriptions_are_inserted_and_replaced(self):
        info_path = self.root / "Contents/Info.plist"
        info_path.parent.mkdir()
        with info_path.open("wb") as output:
            plistlib.dump(
                {
                    "CFBundleIdentifier": PACKAGER.BUNDLE_ID,
                    "NSMicrophoneUsageDescription": "legacy placeholder",
                },
                output,
            )

        PACKAGER.normalize_privacy_usage_descriptions(self.root)

        with info_path.open("rb") as source:
            info = plistlib.load(source)
        for key, value in PACKAGER.PRIVACY_USAGE_DESCRIPTIONS.items():
            self.assertEqual(info[key], value)

    def test_macho_path_parsers_distinguish_bundled_and_build_paths(self):
        load_commands = """
          cmd LC_RPATH
      cmdsize 48
         path @executable_path/../Frameworks (offset 12)
          cmd LC_RPATH
      cmdsize 96
         path /Users/builder/private/build (offset 12)
        """
        self.assertEqual(
            PACKAGER.parse_macho_rpaths(load_commands),
            ["@executable_path/../Frameworks", "/Users/builder/private/build"],
        )
        dependencies = """binary:\n\t@rpath/Local.framework/Local (compatibility version 1.0.0)\n\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n\t/opt/local/libUnexpected.dylib (compatibility version 1.0.0)\n"""
        parsed = PACKAGER.parse_macho_dependencies(dependencies)
        self.assertTrue(PACKAGER.safe_macho_path(parsed[0]))
        self.assertTrue(PACKAGER.safe_macho_path(parsed[1]))
        self.assertFalse(PACKAGER.safe_macho_path(parsed[2]))

    def test_hardening_removes_group_and_world_write_permissions(self):
        path = self.root / "ordinary-resource"
        path.write_text("test-only")
        path.chmod(0o666)

        result = PACKAGER.harden_bundle(self.root)

        self.assertEqual(result["removed_group_or_world_write_permissions"], 1)
        self.assertEqual(stat.S_IMODE(path.stat().st_mode), 0o644)
        PACKAGER.validate_hardened_bundle(self.root)

    def test_native_provenance_rejects_alias_and_stale_outputs(self):
        source = self.root / "source.rs"
        core = self.root / PACKAGER.CORE_SOURCE_NAME
        service = self.root / PACKAGER.SERVICE_SOURCE_NAME
        alias = self.root / "libhdobbydesk.dylib"
        for path in [source, core, service, alias]:
            path.write_bytes(b"test-only")

        with self.assertRaisesRegex(PACKAGER.PackagingError, "direct Cargo release"):
            PACKAGER.validate_native_provenance(alias, service, [source])

        os.utime(core, ns=(1_000_000_000, 1_000_000_000))
        os.utime(service, ns=(1_000_000_000, 1_000_000_000))
        os.utime(source, ns=(2_000_000_000, 2_000_000_000))
        with self.assertRaisesRegex(PACKAGER.PackagingError, "older than"):
            PACKAGER.validate_native_provenance(core, service, [source])

        os.utime(core, ns=(3_000_000_000, 3_000_000_000))
        os.utime(service, ns=(3_000_000_000, 3_000_000_000))
        PACKAGER.validate_native_provenance(core, service, [source])

    def test_generated_build_version_is_not_a_source_provenance_input(self):
        self.assertNotIn(
            PACKAGER.ROOT / "src/version.rs", PACKAGER.native_build_inputs()
        )


if __name__ == "__main__":
    unittest.main()
