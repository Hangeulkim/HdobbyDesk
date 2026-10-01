"""Compile and run production endpoint-selection tests locally, without Cargo/network."""
from pathlib import Path
import os
import shutil
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parents[1]
    source = root / 'libs/hbb_common/src/private_network.rs'
    compiler = shutil.which('rustc')
    if not compiler or not source.is_file():
        raise SystemExit('Provide rustc and apply the private network submodule patch first.')
    with tempfile.TemporaryDirectory(prefix='hdobby-private-test-') as scratch:
        executable = Path(scratch) / 'private-network-tests'
        if os.name == 'nt':
            executable = executable.with_suffix('.exe')
        subprocess.run([compiler, '--edition', '2018', '--test', str(source),
                        '-o', str(executable)], check=True, timeout=120)
        subprocess.run([str(executable)], check=True, timeout=30)


if __name__ == '__main__':
    main()
