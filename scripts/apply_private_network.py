"""Apply the reviewed hbb_common patch locally, without fetching or committing."""
import argparse
from pathlib import Path
import subprocess

BASE = '7e1c392c62d39c364127307cd408421dd5f8cfb0'
ROOT = Path(__file__).resolve().parents[1]
SUBMODULE = ROOT / 'libs/hbb_common'
PATCH = ROOT / 'patches/hbb-common-private-network.patch'


def check(*args):
    return subprocess.run(['git', '-C', str(SUBMODULE), *args],
                          stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode == 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--apply', action='store_true', help='Apply after checking the base and patch')
    args = parser.parse_args()
    if not (SUBMODULE / '.git').exists() or not PATCH.is_file():
        parser.exit(1, 'Initialize the pinned submodule and provide the reviewed patch first.\n')
    head = subprocess.check_output(['git', '-C', str(SUBMODULE), 'rev-parse', 'HEAD'], text=True).strip()
    if head != BASE:
        parser.exit(1, 'Submodule base differs from the reviewed version; rebase and review the patch first.\n')
    if check('apply', '--unidiff-zero', '--reverse', '--check', str(PATCH)):
        print('Private network patch is already applied.')
        return
    dirty = subprocess.check_output(
        ['git', '-C', str(SUBMODULE), 'status', '--porcelain'], text=True)
    if dirty.strip():
        parser.exit(1, 'Submodule has other changes; preserve and review them before applying the patch.\n')
    if not check('apply', '--unidiff-zero', '--check', str(PATCH)):
        parser.exit(1, 'Patch does not apply cleanly. Existing changes were left untouched.\n')
    if args.apply:
        subprocess.run(['git', '-C', str(SUBMODULE), 'apply', '--unidiff-zero', str(PATCH)], check=True)
        print('Private network patch applied locally; no commit or network operation performed.')
    else:
        print('Patch check passed. Use --apply to apply it locally.')


if __name__ == '__main__':
    main()
