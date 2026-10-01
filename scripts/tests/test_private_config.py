"""Local publication-guard tests; only temporary Git repositories are used."""
from pathlib import Path
import json
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
CHECKER = ROOT / 'scripts/check_private_config.py'
SYNTHETIC_VALUE = 'do-not-print-this-value'
SYNTHETIC_HOST = 'relay.customer.test'


class PublicationGuardTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='hdobby-guard-test-')
        self.repo = Path(self.temp.name)
        self.git('init', '-q')

    def tearDown(self):
        self.temp.cleanup()

    def git(self, *args, check=True):
        return subprocess.run(['git', *args], cwd=self.repo, check=check,
                              capture_output=True, text=True)

    def stage(self, name, content):
        path = self.repo / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
        self.git('add', '-f', '--', name)

    def scan(self):
        return subprocess.run(['python3', str(CHECKER)], cwd=self.repo,
                              capture_output=True, text=True)

    def test_loopback_and_empty_example_are_accepted(self):
        self.stage('example.toml', 'relay-server = "127.0.0.1:21117"\n')
        self.stage('.env.example', 'HDOBBY_TARGET=\n')
        self.assertEqual(self.scan().returncode, 0)

    def test_forced_ignored_operational_file_is_rejected(self):
        (self.repo / '.gitignore').write_text('/runtime/\n')
        self.stage('runtime/connection.toml', 'relay-server = ""\n')
        self.assertEqual(self.scan().returncode, 1)

    def test_unspecified_bind_constant_is_allowed_but_not_a_server_destination(self):
        unspecified = '0.0.0.0'
        self.stage('listener.dart', "final unspecified = '0.0.0.0';\n")
        self.assertEqual(self.scan().returncode, 0)
        self.stage('settings.toml', f'relay-server = "{unspecified}:21117"\n')
        self.assertEqual(self.scan().returncode, 1)

    def test_source_variables_are_not_literal_endpoints(self):
        self.stage('connection.rs', 'relay_server = ph.relay_server;\nrelay_server: String,\n')
        self.assertEqual(self.scan().returncode, 0)

    def test_serialized_identity_and_pairing_codes_are_rejected(self):
        cases = [
            ('identity.json', json.dumps({'private_key': [1, 2, 3]}, indent=2)),
            ('trust.json', json.dumps({'nested': {'certificate': [1, 2, 3]}})),
            ('peer.txt', 'hdobby1:' + 'A' * 128),
            ('peer-v2.txt', 'hdobby2:' + 'MZXW6YTB-' * 20),
            ('connection-v1.txt', 'hdobby-connect1:' + 'A_' * 64),
            ('connection-v2.txt', 'hdobby-connect2:' + 'A2' * 64),
            ('hdobby-direct-tls-identity.json', '{}'),
            ('HdobbyDesk2.toml', '[options]\ndirect-server = "Y"\n'),
            ('signing.jks', 'test-only'),
            ('key.properties', 'storePassword=test-only\n'),
        ]
        for name, content in cases:
            with self.subTest(name=name):
                self.git('read-tree', '--empty')
                self.stage(name, content)
                result = self.scan()
                self.assertEqual(result.returncode, 1)
                self.assertNotIn('A' * 128, result.stdout + result.stderr)

    def test_empty_identity_example_is_allowed_and_staged_json_only_is_read(self):
        self.stage('identity.example.json', '{"private_key": [], "certificate": ""}')
        (self.repo / 'identity.example.json').write_text('{"private_key": [1, 2, 3]}')
        self.assertEqual(self.scan().returncode, 0)

    def test_bare_runtime_endpoint_is_rejected(self):
        self.stage('connect.sh', f'HDOBBY_TARGET={SYNTHETIC_HOST}\n')
        self.assertEqual(self.scan().returncode, 1)

    def test_literals_are_rejected_and_values_are_not_reported(self):
        # Reserved synthetic names/values. They are never resolved or connected.
        cases = [
            ('settings.toml', f'relay-server = "{SYNTHETIC_HOST}:21117"\n'),
            ('settings.toml', f'password = "{SYNTHETIC_VALUE}"\n'),
            ('fixture.txt', '-----BEGIN ' + 'PRIVATE KEY-----\ntest-only\n'),
            ('notes.md', 'host ' + '.'.join(['10', '44', '55', '66']) + '\n'),
            ('rustdesk-host=relay.customer.test,key=do-not-print-this-value.exe', 'test-only'),
        ]
        for name, content in cases:
            with self.subTest(name=name, kind=content.split(' ', 1)[0]):
                self.git('read-tree', '--empty')
                self.stage(name, content)
                result = self.scan()
                self.assertEqual(result.returncode, 1)
                self.assertNotIn('do-not-print-this-value', result.stdout + result.stderr)
                self.assertNotIn('relay.customer.test', result.stdout + result.stderr)

    def test_unstaged_local_values_do_not_enter_staged_check(self):
        self.stage('sample.toml', 'relay-server = ""\n')
        (self.repo / 'sample.toml').write_text(f'relay-server = "{SYNTHETIC_HOST}"\n')
        self.assertEqual(self.scan().returncode, 0)

    def test_endpoint_bearing_log_statements_are_rejected(self):
        unsafe = 'log::info!("direct access ' + 'from {}", addr);\n'
        self.stage('connection.rs', unsafe)
        self.assertEqual(self.scan().returncode, 1)
        self.git('read-tree', '--empty')
        self.stage('connection.rs', 'log::info!("Direct connection accepted");\n')
        self.assertEqual(self.scan().returncode, 0)

    def test_installed_hook_blocks_commit_locally(self):
        scripts = self.repo / 'scripts'
        scripts.mkdir()
        shutil.copyfile(CHECKER, scripts / CHECKER.name)
        hook = self.repo / '.githooks/pre-commit'
        hook.parent.mkdir()
        shutil.copyfile(ROOT / '.githooks/pre-commit', hook)
        hook.chmod(0o755)
        self.git('config', '--local', 'core.hooksPath', '.githooks')
        self.stage('settings.toml', f'password = "{SYNTHETIC_VALUE}"\n')
        result = self.git('-c', 'user.name=Local Test', '-c', 'user.email=local@example.invalid',
                          'commit', '-m', 'Local guard verification', check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Publication blocked', result.stderr)
        self.assertNotIn('do-not-print-this-value', result.stdout + result.stderr)


if __name__ == '__main__':
    unittest.main()
