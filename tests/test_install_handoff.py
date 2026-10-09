"""Keep an installer launched from the UI alive when the UI is stopped."""
import json
import os
import pathlib
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).parents[1]


class InstallHandoffTests(unittest.TestCase):
    def run_handoff(self, cgroup, repair=False, launch_status=0):
        source = (ROOT / 'packaging/install.sh').read_text()
        block = source.split('# A terminal opened by Framely', 1)[1].split('cd "$base"', 1)[0]
        with tempfile.TemporaryDirectory() as temp:
            folder = pathlib.Path(temp)
            log = folder / 'handoff.json'
            # The package path intentionally includes spaces: arguments must survive.
            package = folder / 'release package'
            package.mkdir()
            cat = folder / 'cat'
            cat.write_text('#!/bin/sh\nprintf "%s\\n" "$TEST_CGROUP"\n')
            runner = folder / 'systemd-run'
            runner.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
pathlib.Path(os.environ['TEST_HANDOFF_LOG']).write_text(json.dumps(sys.argv[1:]))
sys.exit(int(os.environ['TEST_LAUNCH_STATUS']))
''')
            cat.chmod(0o755)
            runner.chmod(0o755)
            env = dict(os.environ, PATH=str(folder) + ':' + os.environ['PATH'],
                       TEST_CGROUP=cgroup, TEST_HANDOFF_LOG=str(log),
                       TEST_LAUNCH_STATUS=str(launch_status))
            result = subprocess.run(
                ['bash', '-e', '-c', 'base=$1; steam_user=steamos; repair=$2\n'
                 + '# A terminal opened by Framely' + block
                 + '\necho FOREGROUND_INSTALL', 'install-handoff', str(package),
                 'true' if repair else 'false'],
                env=env, capture_output=True, text=True, timeout=10)
            return result, json.loads(log.read_text()) if log.exists() else None, package

    def test_ui_and_plugin_terminals_hand_off_before_installing(self):
        for cgroup in [
            '0::/system.slice/framely-session.service',
            '0::/system.slice/framely.service',
            '0::/user.slice/user-1000.slice/user@1000.service/app.slice/framely-ui-recovery.service',
            '0::/system.slice/framely-backend-demo.service/child',
            '1:name=systemd:/system.slice/framely-session.service\n0::/other',
        ]:
            with self.subTest(cgroup=cgroup):
                result, args, package = self.run_handoff(cgroup)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotIn('FOREGROUND_INSTALL', result.stdout)
                self.assertEqual(args[-3:], ['/usr/bin/bash', str(package / 'install.sh'), 'steamos'])
                self.assertIn('--no-block', args)
                self.assertIn('--property=StandardInput=null', args)
                self.assertIn('--property=StandardOutput=journal', args)
                self.assertIn('--property=StandardError=journal', args)
                self.assertNotIn('--pipe', args)
                self.assertNotIn('--wait', args)

    def test_repair_keeps_arguments_and_launch_failure_stops(self):
        result, args, _ = self.run_handoff('0::/system.slice/framely-session.service', repair=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(args[-2:], ['--repair', 'steamos'])
        result, _, _ = self.run_handoff('0::/system.slice/framely-session.service', launch_status=1)
        self.assertNotEqual(result.returncode, 0)
        self.assertNotIn('FOREGROUND_INSTALL', result.stdout)

    def test_ssh_and_independent_update_workers_do_not_recurse(self):
        for cgroup in [
            '0::/user.slice/user-1000.slice/session-42.scope',
            '0::/system.slice/framely-install-123-456.service',
            '0::/system.slice/framely-update-123.service',
            '0::/system.slice/framely-session.service-other',
        ]:
            with self.subTest(cgroup=cgroup):
                result, args, _ = self.run_handoff(cgroup)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIsNone(args)
                self.assertIn('FOREGROUND_INSTALL', result.stdout)
