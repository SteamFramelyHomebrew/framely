"""Keep media packaging aligned with Cargo's default and overridden output paths."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class MediaBuildOutput(unittest.TestCase):
    def test_copies_compiled_binaries_from_cargo_target_directory(self):
        cargo = shutil.which('cargo')
        self.assertIsNotNone(cargo)
        for setting in (None, 'target/cargo', 'absolute'):
            with self.subTest(target=setting), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                (root / 'tools').mkdir()
                source = (ROOT / 'tools/build-media.sh').read_text()
                # Native dependencies are separate from the Cargo output-copy boundary.
                (root / 'tools/build-media.sh').write_text(source.split('# Native dependencies')[0])
                crate = root / 'media/capture'
                (crate / 'src').mkdir(parents=True)
                (crate / 'Cargo.toml').write_text('[package]\nname="capture-fixture"\nversion="0.1.0"\nedition="2021"\n')
                (crate / 'src/main.rs').write_text('fn main() {}\n')
                env = dict(os.environ)
                env.pop('CARGO_TARGET_DIR', None)
                env.pop('FRAMELY_SYSROOT', None)
                if setting:
                    env['CARGO_TARGET_DIR'] = str(root / 'absolute-output') if setting == 'absolute' else setting
                subprocess.run([cargo, 'generate-lockfile', '--manifest-path', str(crate / 'Cargo.toml')], env=env, check=True)
                mock_bin = root / 'mock-bin'
                mock_bin.mkdir()
                wrapper = mock_bin / 'cargo'
                wrapper.write_text('''#!/usr/bin/env python3
import json,os,pathlib,subprocess,sys
real=os.environ['TEST_REAL_CARGO']
if sys.argv[1]=='build':
    result=subprocess.check_output([real,'metadata','--locked','--no-deps','--format-version','1','--manifest-path','media/capture/Cargo.toml'])
    output=pathlib.Path(json.loads(result)['target_directory'])/'release'
    output.mkdir(parents=True,exist_ok=True)
    for name in ['framely-capture','framely-panel-grab']:(output/name).write_text('compiled '+name)
else:os.execv(real,[real,*sys.argv[1:]])
''')
                wrapper.chmod(0o755)
                env.update(TEST_REAL_CARGO=cargo, PATH=str(mock_bin) + os.pathsep + env['PATH'])
                subprocess.run(['bash', str(root / 'tools/build-media.sh')], cwd=root, env=env, check=True)
                for name in ('framely-capture', 'framely-panel-grab'):
                    self.assertEqual((root / 'media/bin' / name).read_text(), 'compiled ' + name)


if __name__ == '__main__':
    unittest.main()
