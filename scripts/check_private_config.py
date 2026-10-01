"""Check staged additions locally. Never contacts a server or prints secret values.

This is an extra publication check, not a complete secret-detection system.
Review the staged diff before publishing. Unstaged work is not inspected.
"""
import ipaddress
import json
from pathlib import PurePosixPath
import re
import subprocess
import sys
from urllib.parse import urlsplit

PRIVATE_SUFFIXES = {'.pem', '.key', '.p8', '.p12', '.pfx', '.jks', '.keystore',
                    '.cer', '.crt', '.der', '.mobileprovision', '.pcap', '.pcapng',
                    '.log', '.backup', '.bak', '.apk', '.ipa', '.exe', '.dmg'}
SECRET = re.compile(
    r'''(?i)\b(?:password|access[_-]token|api[_-]key|private[_-]key|server[_-]key|client[_-]secret)\b["']?\s*[:=]\s*(["'])(.*?)\1''')
ENDPOINT = re.compile(
    r'''(?i)\b(?:custom[_-]rendezvous[_-]server|rendezvous[_-]server|relay[_-]server|id[_-]server|api[_-]server|server[_-](?:host|address|url)|HDOBBY_[A-Z_]*(?:SERVER|TARGET|HOST|ENDPOINT))\b["']?\s*[:=]\s*(["'])(.*?)\1''')
BARE_ENDPOINT = re.compile(
    r'''(?i)\b(?:custom-rendezvous-server|rendezvous-server|relay-server|id-server|api-server|server-(?:host|address|url)|HDOBBY_[A-Z_]*(?:SERVER|TARGET|HOST|ENDPOINT))\b\s*=\s*(?!["'])([^\s,"'\}\]]+)''')
IPV4 = re.compile(r'(?<![\w.])(?:\d{1,3}\.){3}\d{1,3}(?![\w.])')
# Public certificates are not private keys, but a certificate bundled with a
# destination is operational pairing data. Keep both legacy Base64 and current
# Base32 forms out of source control without ever printing their values.
PAIRING_CODE = re.compile(
    r'''(?ix)\b(?:
        hdobby1:[A-Za-z0-9+/=]{64,} |
        hdobby2:[A-Z2-7](?:[A-Z2-7\s-]{62,}[A-Z2-7]) |
        hdobby-connect1:[A-Za-z0-9_-]{64,}={0,2} |
        hdobby-connect2:[A-Z2-7]{64,}
    )''')
DOCUMENTATION_NETS = tuple(ipaddress.ip_network(n) for n in
                          ('192.0.2.0/24', '198.51.100.0/24', '203.0.113.0/24'))
ENDPOINT_LOG_MARKERS = (
    'peer address:',
    'direct access from',
    'direct server listening on:',
    'failed to start direct server on port:',
    'failed to accept connection from',
    'failed to create relay connection for',
)


def git(*args):
    return subprocess.check_output(['git', *args])


def private_path(name):
    p = PurePosixPath(name)
    return (p.parts[0] in {'runtime', 'secrets', '.local'} or
            p.suffix.lower() in PRIVATE_SUFFIXES or
            (p.name.startswith('.env') and p.name != '.env.example') or
            p.name.startswith('id_ed25519') or
            p.name.lower() in {'key.properties', 'keystore.properties'} or
            p.name.endswith('direct-tls-identity.json') or
            (p.name.lower().startswith(('rustdesk', 'hdobbydesk')) and
             p.suffix.lower() == '.toml'))


def placeholder(value):
    value = value.strip()
    return (not value or value in {r'\n', r'\r', r'\r\n'} or
            value.startswith(('$', '<', '{')) or
            value.lower() in {'none', 'null', 'changeme', 'replace_me', 'example',
                              'placeholder', 'test-only', 'test-value'})


def permitted_address(value):
    if placeholder(value):
        return True
    try:
        host = urlsplit(value if '://' in value else '//' + value).hostname
    except ValueError:
        return False
    if not host:
        return False
    if host == 'localhost' or host.endswith(('.invalid', '.example')):
        return True
    if host in {'example.com', 'example.net', 'example.org'} or host.endswith(
            ('.example.com', '.example.net', '.example.org')):
        return True
    try:
        address = ipaddress.ip_address(host)
    except ValueError:
        return False
    return address.is_loopback or any(address in n for n in DOCUMENTATION_NETS)


def line_reasons(line):
    reasons = []
    lowered = line.lower()
    if 'log::' in lowered and any(marker in lowered for marker in ENDPOINT_LOG_MARKERS):
        reasons.append('endpoint-bearing log statement')
    if PAIRING_CODE.search(line):
        reasons.append('operational pairing certificate')
    if re.search(r'-----BEGIN (?:[A-Z]+ )?PRIVATE KEY-----', line):
        reasons.append('private key material')
    if any(not placeholder(m.group(2)) for m in SECRET.finditer(line)):
        reasons.append('literal credential')
    if any(not permitted_address(m.group(2)) for m in ENDPOINT.finditer(line)):
        reasons.append('literal server endpoint')
    if any(not permitted_address(m.group(1)) for m in BARE_ENDPOINT.finditer(line)):
        reasons.append('literal server endpoint')
    for match in IPV4.finditer(line):
        try:
            address = ipaddress.ip_address(match.group())
        except ValueError:
            continue
        # The unspecified bind address is a protocol constant, not a device address.
        # Endpoint fields are still checked separately by permitted_address().
        if address.is_private and not address.is_loopback and not address.is_unspecified and not any(
                address in n for n in DOCUMENTATION_NETS):
            reasons.append('private network address')
    return sorted(set(reasons))


def json_contains_identity(value):
    if isinstance(value, dict):
        for key, item in value.items():
            if key.lower().replace('-', '_') in {'private_key', 'certificate'}:
                if isinstance(item, str) and not placeholder(item):
                    return True
                if isinstance(item, list) and item:
                    return True
            if json_contains_identity(item):
                return True
    if isinstance(value, list):
        return any(json_contains_identity(item) for item in value)
    return False


def main():
    failures = []
    names = git('diff', '--cached', '--name-only', '--diff-filter=ACMR', '-z')
    for raw in names.split(b'\0'):
        if not raw:
            continue
        name = raw.decode('utf-8', 'surrogateescape')
        if private_path(name):
            failures.append((name, 'operational or captured-data file'))
            continue
        if PurePosixPath(name).suffix.lower() == '.json':
            try:
                if json_contains_identity(json.loads(git('show', ':' + name))):
                    failures.append((name, 'operational identity material'))
            except (ValueError, UnicodeDecodeError):
                pass  # Other text formats are still checked line by line below.
        patch = git('diff', '--cached', '--no-ext-diff', '--no-textconv',
                    '--unified=0', '--', name).decode('utf-8', 'replace')
        for line in patch.splitlines():
            if line.startswith('+') and not line.startswith('+++'):
                for reason in line_reasons(line[1:]):
                    failures.append((name, reason))
    if failures:
        print('Publication blocked: remove operational data from the staged changes.', file=sys.stderr)
        for name, reason in sorted(set(failures)):
            display_name = '<redacted filename>' if '=' in name else name
            print(f'  {display_name}: {reason}', file=sys.stderr)
        print('Values are intentionally not printed. Use runtime input or an empty example.', file=sys.stderr)
        return 1
    print('Staged configuration check passed (local-only; manual review still required).')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
