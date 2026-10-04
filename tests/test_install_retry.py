"""Exercise production install cleanup with all machine paths and tools mocked."""
import os
import hashlib
import json
import pathlib
import subprocess
import tempfile
import unittest


class InstallRetry(unittest.TestCase):
    def test_failed_install_cleans_new_release_and_can_retry(self):
        self.exercise_install(legacy=True)

    def test_fresh_install_and_system_update_repair(self):
        self.exercise_install(legacy=False)

    def test_split_runtime_install_repair_update_rollback_and_uninstall(self):
        self.exercise_install(legacy=False, split=True)

    def exercise_install(self, legacy, split=False):
        with tempfile.TemporaryDirectory() as tmp:
            base = pathlib.Path(tmp)
            state, store, etc = base / 'state', base / 'store', base / 'etc'
            units = etc / 'systemd/system'
            units.mkdir(parents=True)
            home = base / 'home'
            home.mkdir()
            tools = base / 'mock-bin'
            tools.mkdir()
            package = base / 'package'
            package.mkdir()
            for folder in ['bin', 'lib/cef', 'share', 'tools']:
                (package / folder).mkdir(parents=True)
            (package / 'VERSION').write_text('test-version\n')
            (package / 'SHA256SUMS').write_text('')
            (package / 'lib/cef/framely-vr').write_text('mock')
            if split:
                for name, content in [('lib/cef/libcef.so', 'browser'), ('share/licenses/cef.txt', 'license')]:
                    path = package / name
                    path.parent.mkdir(parents=True, exist_ok=True)
                    path.write_text(content)
                (package / 'CEF_RUNTIME.json').write_text(json.dumps({'schemaVersion': 1, 'id': 'test-cef'}))
                (package / 'CEF_SHA256SUMS').write_text(''.join(hashlib.sha256((package / name).read_bytes()).hexdigest() + '  ' + name + '\n' for name in ['lib/cef/libcef.so', 'share/licenses/cef.txt']))
                (package / 'tools/cef-runtime.py').write_text((pathlib.Path(__file__).parents[1] / 'tools/cef-runtime.py').read_text())
            (package / 'bin/framely').write_text('#!/bin/sh\nexit 0\n')
            (package / 'bin/framely').chmod(0o755)
            repair_source = (pathlib.Path(__file__).parents[1] / 'packaging/repair.sh').read_text()
            repair_source = repair_source.replace('if [[ $EUID != 0 ]]; then exec sudo -- bash "$0" "$@"; fi', '')
            repair_source = repair_source.replace('/home/.framely', str(store))
            repair_source = repair_source.replace('== 0 ]]', '== ' + str(os.getuid()) + ' ]]')
            (package / 'repair.sh').write_text(repair_source)
            # Simulate an existing installation with state on the OS partition.
            if legacy:
                state.mkdir()
                (state / 'state.json').write_text('{"settings":"preserved"}')
                (state / 'update-source.json').write_text('{"url":"https://example.org/update.json"}')
                (state / 'data/plugin').mkdir(parents=True)
                (state / 'data/plugin/payload').write_text('plugin data')
            # Keep install logic intact; redirect privileged prerequisites and paths.
            source = (pathlib.Path(__file__).parents[1] / 'packaging/install.sh').read_text()
            source = source.replace('if [[ $EUID != 0 ]]; then exec sudo -- bash "$0" "$@"; fi', '')
            source = source.replace('/etc/systemd/system', str(units)).replace('/var/lib/framely', str(state))
            source = source.replace('/home/.framely', str(store)).replace('/home/framely', str(home))
            source = source.replace('[[ -w /etc &&', '[[ -w ' + str(etc) + ' &&')
            source = source.replace('&& -w /etc && -w /var/lib', '&& -w ' + str(etc) + ' && -w ' + str(base))
            source = source.replace('== 0 ]] || { echo \'Storage', '== ' + str(os.getuid()) + ' ]] || { echo \'Storage')
            script = package / 'install.sh'
            script.write_text(source)
            controls = {
                'uname': '#!/bin/sh\necho aarch64\n',
                'id': '#!/bin/sh\necho 1000\n',
                'getent': '#!/bin/sh\necho "framely:x:1000:1000::' + str(home) + ':/bin/sh"\n',
                'sha256sum': '#!/bin/sh\nexit 0\n',
                'ldd': '#!/bin/sh\nexit 0\n',
                'chown': '#!/bin/sh\nexit 0\n',
                'useradd': '#!/bin/sh\nexit 0\n',
                'systemd-run': '#!/bin/sh\nexit 0\n',
                'sleep': '#!/bin/sh\nexit 0\n',
                'systemctl': '#!/bin/sh\ncase "$1" in\nis-active) [ "$2" = --quiet ] && exit 0; echo inactive; exit 3;;\nis-enabled) echo disabled; exit 1;;\nstart) [ -f "' + str(base / 'fail') + '" ] && exit 1;;\nesac\nexit 0\n',
            }
            for name, code in controls.items():
                path = tools / name
                path.write_text(code)
                path.chmod(0o755)
            env = dict(os.environ, PATH=str(tools) + ':' + os.environ['PATH'])
            failure = base / 'fail'
            failure.touch()
            first = subprocess.run(['bash', str(script), 'steam'], env=env, capture_output=True, text=True)
            self.assertNotEqual(first.returncode, 0, first.stdout + first.stderr)
            self.assertIn('restoring previous', first.stderr)
            self.assertFalse((store / 'releases/test-version').exists())
            self.assertTrue(state.is_symlink())
            if legacy:
                self.assertEqual((store / 'state/state.json').read_text(), '{"settings":"preserved"}')
                self.assertEqual((store / 'data/plugin/payload').read_text(), 'plugin data')
            failure.unlink()
            second = subprocess.run(['bash', str(script), 'steam'], env=env, capture_output=True, text=True)
            self.assertEqual(second.returncode, 0, second.stdout + second.stderr)
            self.assertTrue((store / 'releases/test-version').is_dir())
            if split:
                self.assertTrue((store / 'releases/test-version/lib/cef/libcef.so').is_symlink())
                self.assertEqual((store / 'cef/test-cef/lib/cef/libcef.so').read_text(), 'browser')
            if not legacy:
                (state / 'state.json').write_text('{"settings":"preserved"}')
                (state / 'update-source.json').write_text('{"url":"https://example.org/update.json"}')
                (state / 'data/plugin').mkdir(parents=True)
                (state / 'data/plugin/payload').write_text('plugin data')
            third = subprocess.run(['bash', str(script), 'steam'], env=env, capture_output=True, text=True)
            self.assertNotEqual(third.returncode, 0)
            self.assertTrue((store / 'releases/test-version').is_dir())
            # SteamOS replaced the service definitions and /var compatibility link.
            (state / 'previous-release').write_text('releases/older\n')
            state.unlink()
            for unit in units.iterdir():
                unit.unlink()
            missing_user = base / 'missing-user'
            missing_user.touch()
            (tools / 'getent').write_text('#!/bin/sh\nif [ "$1" = passwd ] && [ "$2" = framely ] && [ -f "' + str(missing_user) + '" ]; then exit 2; fi\necho "framely:x:1000:1000::' + str(home) + ':/bin/sh"\n')
            (tools / 'useradd').write_text('#!/bin/sh\nprintf "%s\\n" "$@" > "' + str(base / 'useradd-args') + '"\nrm -f "' + str(missing_user) + '"\n')
            repair = subprocess.run(['bash', str(store / 'repair.sh')], env=env, capture_output=True, text=True)
            self.assertEqual(repair.returncode, 0, repair.stdout + repair.stderr)
            self.assertTrue(state.is_symlink())
            self.assertTrue((units / 'framely.service').is_file())
            self.assertTrue((units / 'framely-session.service').is_file())
            self.assertFalse((base / 'useradd-args').exists())
            self.assertEqual((state / 'previous-release').read_text(), 'releases/older\n')
            self.assertEqual((state / 'state.json').read_text(), '{"settings":"preserved"}')
            self.assertEqual((state / 'update-source.json').read_text(), '{"url":"https://example.org/update.json"}')
            repeat = subprocess.run(['bash', str(store / 'repair.sh')], env=env, capture_output=True, text=True)
            self.assertEqual(repeat.returncode, 0, repeat.stdout + repeat.stderr)
            # Refuse ambiguous state left by another installation, preserving both.
            state.unlink()
            state.mkdir()
            (state / 'state.json').write_text('conflicting')
            conflict = subprocess.run(['bash', str(store / 'repair.sh')], env=env, capture_output=True, text=True)
            self.assertNotEqual(conflict.returncode, 0)
            self.assertIn('refusing to merge', conflict.stderr)
            self.assertEqual((state / 'state.json').read_text(), 'conflicting')
            self.assertEqual((store / 'state/state.json').read_text(), '{"settings":"preserved"}')
            (state / 'state.json').unlink()
            state.rmdir()
            state.symlink_to(store / 'state')
            if split:
                # Upgrade using only the core payload; old and new releases retain
                # their own host executable while sharing one verified runtime.
                (package / 'lib/cef/libcef.so').unlink()
                (package / 'share/licenses/cef.txt').unlink()
                (package / 'VERSION').write_text('next-version\n')
                (package / 'lib/cef/framely-vr').write_text('new host')
                upgraded = subprocess.run(['bash', str(script), 'steam'], env=env, capture_output=True, text=True)
                self.assertEqual(upgraded.returncode, 0, upgraded.stdout + upgraded.stderr)
                self.assertEqual((state / 'current/lib/cef/framely-vr').read_text(), 'new host')
                self.assertEqual((state / 'previous-release').read_text(), 'releases/test-version\n')
                self.assertEqual((state / 'current/lib/cef/libcef.so').read_text(), 'browser')
                rollback_source = (pathlib.Path(__file__).parents[1] / 'packaging/rollback.sh').read_text().replace('if [[ $EUID != 0 ]]; then exec sudo -- bash "$0" "$@"; fi', '').replace('/var/lib/framely', str(state))
                rollback_script = base / 'rollback.sh'
                rollback_script.write_text(rollback_source)
                rolled = subprocess.run(['bash', str(rollback_script)], env=env, capture_output=True, text=True)
                self.assertEqual(rolled.returncode, 0, rolled.stdout + rolled.stderr)
                self.assertEqual((state / 'current/lib/cef/framely-vr').read_text(), 'mock')
                self.assertEqual((state / 'current/lib/cef/libcef.so').read_text(), 'browser')
            uninstall_source = (pathlib.Path(__file__).parents[1] / 'packaging/uninstall.sh').read_text()
            uninstall_source = uninstall_source.replace('if [[ $EUID != 0 ]]; then exec sudo -- bash "$0" "$@"; fi', '')
            uninstall_source = uninstall_source.replace('/etc/systemd/system', str(units)).replace('/var/lib/framely', str(state))
            uninstall_source = uninstall_source.replace('/home/.framely', str(store)).replace('/home/framely', str(home)).replace('/run/framely', str(base / 'run'))
            uninstall = base / 'uninstall.sh'
            uninstall.write_text(uninstall_source)
            binary = state / 'current/bin/framely'
            binary.write_text('#!/bin/sh\nif [ "$1" = prepare-uninstall ]; then echo "plugin cleanup failed" >&2; exit 1; fi\nexit 0\n')
            failed = subprocess.run(['bash', str(uninstall)], env=env, capture_output=True, text=True)
            self.assertNotEqual(failed.returncode, 0)
            self.assertIn('Framely was retained', failed.stderr)
            self.assertTrue((state / 'current').exists())
            self.assertTrue((units / 'framely.service').exists())
            binary.write_text('#!/bin/sh\nexit 0\n')
            removed = subprocess.run(['bash', str(uninstall)], env=env, capture_output=True, text=True)
            self.assertEqual(removed.returncode, 0, removed.stdout + removed.stderr)
            self.assertFalse((store / 'releases').exists())
            self.assertFalse((store / 'cef').exists())
            self.assertFalse((store / 'repair.sh').exists())
            self.assertEqual((store / 'state/state.json').read_text(), '{"settings":"preserved"}')
            self.assertEqual((store / 'data/plugin/payload').read_text(), 'plugin data')
            rejected = subprocess.run(['bash', str(uninstall), '--purge'], env=env, capture_output=True, text=True)
            self.assertNotEqual(rejected.returncode, 0)
            self.assertTrue(store.exists())
