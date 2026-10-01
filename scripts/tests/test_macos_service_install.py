"""macOS service template syntax and unprivileged transaction tests."""

import getpass
import os
from pathlib import Path
import plistlib
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
TEMPLATES = ROOT / "src/platform/privileges_scripts"


def render_hdobby(source: str) -> str:
    return (
        source.replace("__APP_FULL_NAME__", "com.hdobby.HdobbyDesk")
        .replace("__APP_BUNDLE_ID__", "com.hdobby.hdobbydesk")
        .replace("__APP_NAME_LOWER__", "hdobbydesk")
        .replace("__APP_NAME__", "HdobbyDesk")
    )


@unittest.skipUnless(sys.platform == "darwin", "macOS-only service templates")
class MacServiceInstallTest(unittest.TestCase):
    def test_scripts_compile_and_plists_parse_after_branding(self):
        with tempfile.TemporaryDirectory(prefix="hdobby-service-syntax-") as temp:
            temp_root = Path(temp)
            for name in ["install.scpt", "uninstall.scpt", "update.scpt"]:
                source = temp_root / name
                rendered = render_hdobby((TEMPLATES / name).read_text())
                self.assertNotIn("__APP_", rendered)
                source.write_text(rendered)
                output = temp_root / f"{name}.compiled"
                result = subprocess.run(
                    ["/usr/bin/osacompile", "-o", str(output), str(source)],
                    capture_output=True,
                    text=True,
                )
                self.assertEqual(result.returncode, 0, result.stderr)

            for name in ["daemon.plist", "agent.plist"]:
                data = plistlib.loads(render_hdobby((TEMPLATES / name).read_text()).encode())
                self.assertTrue(data["Label"].startswith("com.hdobby.HdobbyDesk_"))
                self.assertEqual(data["AssociatedBundleIdentifiers"], "com.hdobby.hdobbydesk")

    def test_install_transaction_copies_private_configs_and_publishes_plists(self):
        with tempfile.TemporaryDirectory(prefix="hdobby-service-install-") as temp:
            temp_root = Path(temp)
            daemons = temp_root / "LaunchDaemons"
            agents = temp_root / "LaunchAgents"
            root_preferences = temp_root / "root-preferences"
            daemons.mkdir()
            agents.mkdir()

            script = render_hdobby((TEMPLATES / "install.scpt").read_text())
            script = script.replace("/Library/LaunchDaemons", str(daemons))
            script = script.replace("/Library/LaunchAgents", str(agents))
            script = script.replace(
                "/var/root/Library/Preferences", str(root_preferences)
            )
            script = script.replace("/usr/sbin/chown root:wheel", "/usr/bin/true")
            script = script.replace("-o root -g wheel ", "")
            script = script.replace(
                'set load_daemon to "/bin/launchctl bootstrap system " & quoted form of daemon_plist & " 2>/dev/null || /bin/launchctl load -w " & quoted form of daemon_plist & ";"',
                'set load_daemon to "/usr/bin/true;"',
            )
            script = script.replace(" with administrator privileges", "")
            script_path = temp_root / "install-test.scpt"
            script_path.write_text(script)

            config = temp_root / "HdobbyDesk.toml"
            config2 = temp_root / "HdobbyDesk2.toml"
            config.write_text("config-one")
            config2.write_text("config-two")
            config.chmod(0o600)
            config2.chmod(0o600)
            daemon_body = "daemon-test"
            agent_body = "agent-test"

            result = subprocess.run(
                [
                    "/usr/bin/osascript",
                    str(script_path),
                    daemon_body,
                    agent_body,
                    getpass.getuser(),
                    str(config),
                    str(config2),
                ],
                capture_output=True,
                text=True,
            )
            self.assertEqual(result.returncode, 0, result.stderr)

            daemon = daemons / "com.hdobby.HdobbyDesk_service.plist"
            agent = agents / "com.hdobby.HdobbyDesk_server.plist"
            copied = root_preferences / "com.hdobby.HdobbyDesk/HdobbyDesk.toml"
            copied2 = root_preferences / "com.hdobby.HdobbyDesk/HdobbyDesk2.toml"
            self.assertEqual(daemon.read_text(), daemon_body)
            self.assertEqual(agent.read_text(), agent_body)
            self.assertEqual(copied.read_text(), "config-one")
            self.assertEqual(copied2.read_text(), "config-two")
            self.assertEqual(os.stat(daemon).st_mode & 0o777, 0o644)
            self.assertEqual(os.stat(agent).st_mode & 0o777, 0o644)
            self.assertEqual(os.stat(copied).st_mode & 0o777, 0o600)
            self.assertEqual(os.stat(copied2).st_mode & 0o777, 0o600)
            self.assertEqual(list(daemons.glob(".*.??????")), [])
            self.assertEqual(list(agents.glob(".*.??????")), [])


if __name__ == "__main__":
    unittest.main()
