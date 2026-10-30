#!/usr/bin/env python3
"""Manage the global Android address-reservation policy with installer rollback."""
import argparse
import json
import os
from pathlib import Path
import tempfile

MARKER = '# Managed for Framely Android memory reservation compatibility.'
CONFIG = MARKER + '''
# Unreal Engine reserves large virtual ranges; physical pages remain demand allocated.
# This policy affects every process on this host.
vm.overcommit_memory = 1
'''


class Policy:
    def __init__(self, state, config=Path('/etc/sysctl.d/99-framely-overcommit.conf'),
                 kernel=Path('/proc/sys/vm/overcommit_memory')):
        self.state = Path(state)
        self.config = Path(config)
        self.kernel = Path(kernel)
        self.original = self.state / 'overcommit-original.txt'
        self.journal = self.state / 'install-backup/android-memory.json'

    @staticmethod
    def read(path):
        if path.is_symlink():
            raise ValueError(f'Refusing symlink: {path}')
        if not path.exists():
            return None
        stat = path.stat()
        if not path.is_file() or stat.st_uid != os.geteuid() or stat.st_size > 8192:
            raise ValueError(f'Invalid memory policy file: {path}')
        return path.read_text()

    @staticmethod
    def write(path, value, mode=0o600):
        Policy.read(path)
        path.parent.mkdir(parents=True, exist_ok=True)
        if value is None:
            path.unlink(missing_ok=True)
            return
        with tempfile.NamedTemporaryFile(mode='w', dir=path.parent, delete=False) as stream:
            temporary = Path(stream.name)
            try:
                os.fchmod(stream.fileno(), mode)
                stream.write(value)
                stream.flush()
                os.fsync(stream.fileno())
                os.replace(temporary, path)
            finally:
                temporary.unlink(missing_ok=True)

    def value(self):
        value = self.kernel.read_text().strip()
        if value not in ('0', '1', '2'):
            raise ValueError('Invalid vm.overcommit_memory value')
        return value

    def set_value(self, value):
        if value not in ('0', '1', '2'):
            raise ValueError('Invalid saved vm.overcommit_memory value')
        self.kernel.write_text(value + '\n')
        if self.value() != value:
            raise RuntimeError('Kernel memory policy did not change')

    def apply(self):
        if self.read(self.journal) is not None:
            self.rollback()  # Recover an interrupted installer before taking a new snapshot.
        config = self.read(self.config)
        if config is not None and MARKER not in config.splitlines():
            raise ValueError('Existing sysctl configuration is not managed by Framely')
        original = self.read(self.original)
        if original is not None and original.strip() not in ('0', '1', '2'):
            raise ValueError('Invalid original memory policy')
        before = self.value()
        self.write(self.journal, json.dumps({'config': config, 'original': original, 'kernel': before}))
        try:
            if original is None:
                self.write(self.original, before + '\n')
            self.write(self.config, CONFIG, 0o644)
            self.set_value('1')
        except Exception:
            self.rollback()
            raise

    def rollback(self):
        text = self.read(self.journal)
        if text is None:
            return
        snapshot = json.loads(text)
        if snapshot['kernel'] not in ('0', '1', '2'):
            raise ValueError('Invalid policy rollback snapshot')
        self.write(self.config, snapshot['config'], 0o644)
        self.write(self.original, snapshot['original'])
        self.set_value(snapshot['kernel'])
        self.journal.unlink()

    def commit(self):
        self.read(self.journal)
        self.journal.unlink(missing_ok=True)

    def uninstall(self):
        config = self.read(self.config)
        if config is None or MARKER not in config.splitlines():
            return  # An administrator replaced it; do not remove their configuration.
        # Restore only while our exact generated configuration and value remain.
        # A later administrator change takes precedence over the recorded default.
        if config != CONFIG:
            print('Memory policy was edited; leaving it unchanged.')
            return
        original = self.read(self.original)
        if original is None or original.strip() not in ('0', '1', '2'):
            raise ValueError('Cannot restore memory policy without its original value')
        if self.value() == '1':
            self.set_value(original.strip())
        self.config.unlink()
        self.original.unlink()
        self.commit()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=['apply', 'rollback', 'commit', 'uninstall'])
    parser.add_argument('--state', type=Path, default=Path('/var/lib/framely'))
    args = parser.parse_args()
    getattr(Policy(args.state), args.operation)()


if __name__ == '__main__':
    main()
