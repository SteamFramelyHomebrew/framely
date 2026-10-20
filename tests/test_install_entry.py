"""Exercise the public entry point without network access or installation."""
import json
import os
import pathlib
import subprocess
import tempfile
import unittest


class InstallEntryTests(unittest.TestCase):
    def test_engine_matches_selected_release_and_arguments_are_preserved(self):
        for args, release in [
            (['install'], 'latest/download'),
            (['install', '--version', 'v0.4.2-preview.3'], 'download/v0.4.2-preview.3'),
            (['update', '--version=v0.4.2-preview.3'], 'download/v0.4.2-preview.3'),
        ]:
            with self.subTest(args=args):
                result, calls = self.run_entry(args)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(calls['url'], f'https://github.com/TestOwner/framely/releases/{release}/bootstrap.py')
                self.assertEqual(calls['args'], ['--repo', 'TestOwner/framely', *args])

    def test_invalid_or_missing_version_stops_before_download(self):
        for args in [['install', '--version'], ['install', '--version='], ['install', '--version', '../bad']]:
            with self.subTest(args=args):
                result, calls = self.run_entry(args)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, {})

    def test_explicit_github_proxy_covers_bootstrap_and_engine(self):
        args = ['install', '--github-proxy', 'https://gh-proxy.com/']
        result, calls = self.run_entry(args)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(calls['url'], 'https://gh-proxy.com/https://github.com/TestOwner/framely/releases/latest/download/bootstrap.py')
        self.assertEqual(calls['proxy'], 'https://gh-proxy.com')
        self.assertEqual(calls['args'], ['--repo', 'TestOwner/framely', *args])
        self.assertRegex(calls['trace'], r'^[0-9]{8}T[0-9]{6}Z-[0-9]+$')
        self.assertIn(calls['trace'], result.stdout)
        for invalid in ['http://gh-proxy.com', 'https://gh-proxy.com/../bad', 'https://gh-proxy.com?key=1', '']:
            with self.subTest(invalid=invalid):
                result, calls = self.run_entry(['install', '--github-proxy', invalid])
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(calls, {})

    def test_proxy_path_is_not_written_to_installer_log(self):
        result, calls = self.run_entry(['install', '--github-proxy', 'https://proxy.example/private-route'])
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn('private-route', result.stdout + result.stderr)
        self.assertEqual(calls['proxy'], 'https://proxy.example/private-route')

    def run_entry(self, args):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            log = root / 'calls.json'
            curl = root / 'curl'
            curl.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
pathlib.Path(os.environ['FRAMELY_ENTRY_LOG']).write_text(json.dumps({'url': args[-1], 'proxy': os.environ.get('FRAMELY_GITHUB_PROXY', ''), 'trace': os.environ.get('FRAMELY_INSTALL_TRACE_ID', '')}))
pathlib.Path(args[args.index('--output') + 1]).write_text("import json,os,pathlib,sys; p=pathlib.Path(os.environ['FRAMELY_ENTRY_LOG']); d=json.loads(p.read_text()); d['args']=sys.argv[1:]; p.write_text(json.dumps(d))")
''')
            curl.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'],
                       FRAMELY_REPOSITORY='TestOwner/framely', FRAMELY_ENTRY_LOG=str(log))
            env.pop('FRAMELY_GITHUB_PROXY', None)
            result = subprocess.run(['bash', str(pathlib.Path(__file__).parents[1] / 'install.sh'), *args],
                                    env=env, text=True, capture_output=True, timeout=10)
            return result, json.loads(log.read_text()) if log.exists() else {}
