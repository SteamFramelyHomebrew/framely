#!/usr/bin/env python3
"""Reproduce a terminal's service stopping during install, without root changes."""
import argparse
import os
import pathlib
import shlex
import subprocess
import tempfile
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--installer', type=pathlib.Path,
                    default=pathlib.Path(__file__).parents[1] / 'packaging/install.sh')
args = parser.parse_args()
source = args.installer.read_text()
block = source.split('# A terminal opened by Framely', 1)[1].split('cd "$base"', 1)[0]
parent = f'framely-hook-install-check-{os.getpid()}'
with tempfile.TemporaryDirectory(prefix='framely-install-check-') as work:
    directory = pathlib.Path(work)
    # Use the user manager to test the same cgroup semantics without root access.
    wrapper = directory / 'systemd-run'
    wrapper.write_text('#!/bin/sh\nexec /usr/bin/systemd-run --user "$@"\n')
    wrapper.chmod(0o755)
    (directory / 'install.sh').write_text(
        'set -e\nsleep 0.5\n'
        + 'systemctl --user stop ' + shlex.quote(parent + '.service') + '\n'
        + 'cat /proc/self/cgroup > ' + shlex.quote(str(directory / 'worker.cgroup')) + '\n'
        + 'echo SURVIVED > ' + shlex.quote(str(directory / 'passed')) + '\n')
    (directory / 'entry.sh').write_text(
        'set -e\nbase=' + shlex.quote(work) + '\nsteam_user=' + shlex.quote(str(os.getuid()))
        + '\nrepair=false\n# A terminal opened by Framely' + block)
    (directory / 'parent.sh').write_text(
        'set -e\nexport PATH=' + shlex.quote(work) + ':"$PATH"\n'
        + '/usr/bin/bash ' + shlex.quote(str(directory / 'entry.sh')) + '\n'
        + 'sleep 30\n')
    try:
        subprocess.run(['systemd-run', '--user', '--collect', '--quiet',
                        '--unit=' + parent, '--property=Type=exec',
                        '/usr/bin/bash', str(directory / 'parent.sh')], check=True)
        deadline = time.monotonic() + 15
        while not (directory / 'passed').exists() and time.monotonic() < deadline:
            time.sleep(0.1)
        assert (directory / 'passed').exists(), 'Worker did not survive the parent service stop'
        cgroup = (directory / 'worker.cgroup').read_text()
        assert 'framely-install-' in cgroup and parent not in cgroup, cgroup
        status = subprocess.run(['systemctl', '--user', 'is-active', parent + '.service'],
                                capture_output=True, text=True)
        assert status.returncode != 0, 'Parent service was not stopped'
        print('PASS: installer worker survived the terminal service being stopped')
    finally:
        subprocess.run(['systemctl', '--user', 'stop', parent + '.service'],
                       capture_output=True)
