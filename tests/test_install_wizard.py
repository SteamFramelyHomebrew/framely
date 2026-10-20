"""Exercise the public installer wizard through a controlling pseudo-terminal."""
import errno
import fcntl
import json
import os
import pathlib
import pty
import subprocess
import tempfile
import termios
import unittest


ROOT = pathlib.Path(__file__).parents[1]


class InstallWizardTests(unittest.TestCase):
    def test_no_terminal_explains_how_to_run(self):
        result = subprocess.run(['bash', str(ROOT / 'install.sh')], capture_output=True,
                                text=True, preexec_fn=os.setsid)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('interactive installer needs a terminal', result.stderr)

    def run_wizard(self, answers, installed=False, damaged=False, local_bundle=False):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            state = root / 'state'
            releases = root / 'releases'
            legacy = root / 'legacy-current'
            state.mkdir()
            releases.mkdir()
            if installed or damaged:
                current = releases / 'v0.8.3-test'
                current.mkdir()
                if installed:
                    (current / 'VERSION').write_text('0.8.3-test')
                    (current / 'uninstall.sh').write_text('#!/bin/sh\nexit 0\n')
                (state / 'current').symlink_to('releases/v0.8.3-test')
                # The real state layout keeps releases under the state root.
                (state / 'releases').symlink_to(releases)
            script = root / 'install.sh'
            script.write_text((ROOT / 'install.sh').read_text()
                              .replace('/home/.framely/state', str(state))
                              .replace('/home/.framely/releases', str(releases))
                              .replace('/home/.framely/repair.sh', str(root / 'repair.sh'))
                              .replace('/var/lib/framely/current', str(legacy)))
            engine_source = '''
import json, os, pathlib, sys
args=sys.argv[1:]
with open(os.environ['WIZARD_LOG'], 'a') as out:
    out.write(json.dumps({'engine_args':args, 'proxy':os.environ.get('FRAMELY_GITHUB_PROXY',''),
                          'fallback':os.environ.get('FRAMELY_GITHUB_AUTO_FALLBACK','')})+'\\n')
if '--resolve-release' in args:
    print(json.dumps({'tag':'v0.9.0-preview.10','size':1048576}))
'''
            curl = root / 'curl'
            curl.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
args=sys.argv[1:]
with open(os.environ['WIZARD_LOG'], 'a') as out:
    out.write(json.dumps({'url':args[-1], 'proxy':os.environ.get('FRAMELY_GITHUB_PROXY',''),
                          'fallback':os.environ.get('FRAMELY_GITHUB_AUTO_FALLBACK','')})+'\\n')
pathlib.Path(args[args.index('--output')+1]).write_text(''' + repr(engine_source) + ''')
''')
            curl.chmod(0o755)
            bundle = root / 'bundle'
            if local_bundle:
                bundle.mkdir()
                (bundle / 'bootstrap.py').write_text(engine_source)
                (bundle / 'SHA256SUMS').write_text('test\n')
                suffix = '-linux-arm64.tar.gz' if installed else '-offline-linux-arm64.tar.gz'
                (bundle / ('framely-0.9.0-preview.10-012345abcdef' + suffix)).write_bytes(b'archive')
            sudo = root / 'sudo'
            sudo.write_text('#!/bin/sh\nexec "$@"\n')
            sudo.chmod(0o755)
            log = root / 'calls'
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'],
                       WIZARD_LOG=str(log), FRAMELY_REPOSITORY='TestOwner/framely')
            env.pop('FRAMELY_GITHUB_PROXY', None)
            master, slave = pty.openpty()

            def controlling_terminal():
                os.setsid()
                fcntl.ioctl(slave, termios.TIOCSCTTY, 0)

            try:
                command = ['bash', str(script)]
                if local_bundle:
                    command += ['--local-bundle', str(bundle)]
                process = subprocess.Popen(command, stdin=slave, stdout=slave,
                                           stderr=slave, env=env, preexec_fn=controlling_terminal)
                os.close(slave)
                os.write(master, answers.encode())
                code = process.wait(timeout=10)
                output = bytearray()
                while True:
                    try:
                        block = os.read(master, 65536)
                    except OSError as error:
                        if error.errno == errno.EIO:
                            break
                        raise
                    if not block:
                        break
                    output.extend(block)
            finally:
                os.close(master)
            calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
            return code, output.decode(errors='replace'), calls

    def test_fresh_install_with_mirror_and_confirmation(self):
        code, output, calls = self.run_wizard('1\n2\n2\n\n1\n')
        self.assertEqual(code, 0, output)
        self.assertIn('Target release: v0.9.0-preview.10', output)
        self.assertEqual(len([call for call in calls if 'url' in call]), 2)
        self.assertTrue(all(call['url'].startswith('https://gh-proxy.com/') for call in calls if 'url' in call))
        self.assertIn('install', calls[-1]['engine_args'])
        self.assertIn('--yes', calls[-1]['engine_args'])

    def test_existing_install_direct_and_back_navigation(self):
        code, output, calls = self.run_wizard('1\nb\n2\nb\n1\n1\nb\nb\nb\nq\n', installed=True)
        self.assertEqual(code, 0, output)
        self.assertEqual(calls, [])
        self.assertIn('Update Framely', output)
        self.assertIn('Uninstall Framely', output)
        code, output, calls = self.run_wizard('1\n1\n1\n1\n', installed=True)
        self.assertEqual(code, 0, output)
        self.assertIn('update', calls[-1]['engine_args'])
        self.assertTrue(all(call['fallback'] == '0' for call in calls))
        self.assertTrue(all(call['proxy'] == '' for call in calls))

    def test_damaged_state_stops_before_network(self):
        code, output, calls = self.run_wizard('', damaged=True)
        self.assertNotEqual(code, 0)
        self.assertIn('Installation records are incomplete', output)
        self.assertEqual(calls, [])

    def test_uninstall_uses_installed_script_without_network(self):
        code, output, calls = self.run_wizard('2\n1\n', installed=True)
        self.assertEqual(code, 0, output)
        self.assertEqual(calls, [])

    def test_bad_mirror_address_can_be_corrected(self):
        code, output, calls = self.run_wizard('1\n1\n2\nhttp://invalid\nb\n1\n1\n')
        self.assertEqual(code, 0, output)
        self.assertIn('Invalid address', output)
        self.assertTrue(all(call['fallback'] == '0' for call in calls))

    def test_local_bundle_runs_full_interactive_flow_without_fetching_engine(self):
        code, output, calls = self.run_wizard('1\n2\n1\n1\n', installed=True, local_bundle=True)
        self.assertEqual(code, 0, output)
        self.assertIn('Package source: Local staged archive', output)
        self.assertFalse(any('url' in call for call in calls))
        self.assertIn('--archive', calls[-1]['engine_args'])
        self.assertIn('--checksums', calls[-1]['engine_args'])
        self.assertIn('update', calls[-1]['engine_args'])

    def test_local_bundle_first_install_selects_offline_archive(self):
        code, output, calls = self.run_wizard('1\n1\n1\n1\n', local_bundle=True)
        self.assertEqual(code, 0, output)
        self.assertIn('-offline-linux-arm64.tar.gz', calls[-1]['engine_args'][calls[-1]['engine_args'].index('--archive') + 1])


if __name__ == '__main__':
    unittest.main()
