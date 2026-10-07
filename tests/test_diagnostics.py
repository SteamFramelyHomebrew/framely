import base64
import importlib.util
import io
import json
import pathlib
import tempfile
import unittest
from unittest import mock
from types import SimpleNamespace
import subprocess
import zipfile

spec = importlib.util.spec_from_file_location('diagnostics', pathlib.Path(__file__).parents[1] / 'tools/export-diagnostics.py')
diag = importlib.util.module_from_spec(spec)
spec.loader.exec_module(diag)

class DiagnosticsTests(unittest.TestCase):
    def archive(self, root):
        result = diag.collect(root, commands=False, runtime=root / 'runtime')
        self.assertTrue(result['name'].endswith('.zip'))
        return zipfile.ZipFile(io.BytesIO(base64.b64decode(result['data'])))

    def test_stopped_service_partial_logs_and_credentials(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / 'logs').mkdir()
            (root / 'current').mkdir()
            (root / 'current/VERSION').write_text('0.4.3-preview.2-test')
            (root / 'logs/demo.log').write_text('password=private token=secret\nAuthorization: Bearer abc.def\n{"password":"hidden"}\nhttps://user:pass@example.org\nUseful error\n')
            (root / 'network-password.json').write_text('do-not-export')
            (root / 'state.json').write_text(json.dumps({'plugins': {'demo': {'manifest': {'name':'Demo', 'version':'1.0.0'}, 'enabled': True, 'config': {'secret':'do-not-export'}}}, 'proxy': {'http': 'do-not-export'}}))
            archive = self.archive(root)
            text = '\n'.join(archive.read(n).decode() for n in archive.namelist())
            for value in ('private', 'abc.def', 'hidden', 'user:pass', 'do-not-export'):
                self.assertNotIn(value, text)
            self.assertIn('Useful error', text)
            self.assertIn('0.4.3-preview.2-test', text)
            self.assertIn('Demo', text)
            self.assertNotIn('network-password.json', archive.namelist())

    def test_corrupt_state_and_missing_logs_still_export(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / 'state.json').write_text('invalid')
            archive = self.archive(root)
            self.assertTrue(json.loads(archive.read('report.json'))['warnings'])
            self.assertIn('system.json', archive.namelist())

    def test_large_logs_and_symlinks(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root / 'logs').mkdir()
            (root / 'secret').write_text('DO_NOT_FOLLOW')
            (root / 'logs/link.log').symlink_to(root / 'secret')
            (root / 'logs/large.log').write_text('X' * (diag.MAX_FILE * 2) + '\nLAST ERROR')
            archive = self.archive(root)
            self.assertNotIn('logs/link.log', archive.namelist())
            data = archive.read('logs/large.log')
            self.assertLessEqual(len(data), diag.MAX_FILE)
            self.assertTrue(data.endswith(b'LAST ERROR'))

    def test_apk_logs_summary_are_bounded_and_exclude_application_files(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            home = root / 'user'
            apk = home / '.local/share/framely/apk-manager'
            (apk / 'logs').mkdir(parents=True)
            (apk / 'apks').mkdir()
            (apk / 'apks/private.apk').write_text('PRIVATE_APK_CONTENT')
            (apk / 'state.json').write_text(json.dumps({'records': {'ctx/pkg': {
                'context':'ctx', 'steamToken':'PRIVATE_BINDING_TOKEN',
                'metadata': {'package':'test.pkg', 'name':'Test app', 'icon':'PRIVATE_ICON'},
                'removed':False, 'pending':'uninstall'}}}))
            for i in range(14):
                path = apk / 'logs' / f'{i:02}.log'
                path.write_text(f'operation {i} token=PRIVATE_LOG_TOKEN')
                import os
                os.utime(path, (i + 1, i + 1))
            saved = home / '.local/share/Steam/logs/lepton-logcats/ctx'
            saved.mkdir(parents=True)
            (saved / 'logcat-crash.log').write_text('Android boot failure password=PRIVATE_PASSWORD')
            (saved / 'video.mp4').write_text('PRIVATE_MEDIA')
            (saved / 'logcat-symlink.log').symlink_to(saved / 'video.mp4')
            result = diag.collect(root, commands=False, runtime=root/'runtime', steam_home=home)
            archive = zipfile.ZipFile(io.BytesIO(base64.b64decode(result['data'])))
            self.assertIn('apk/summary.json', archive.namelist())
            self.assertIn('apk/android-saved/ctx/logcat-crash.log', archive.namelist())
            self.assertNotIn('apk/operations/00.log', archive.namelist())
            self.assertIn('apk/operations/13.log', archive.namelist())
            text = '\n'.join(archive.read(n).decode() for n in archive.namelist())
            self.assertIn('Android boot failure', text)
            self.assertIn('Test app', text)
            self.assertNotIn('PRIVATE_', text)

    def test_active_container_diagnostics_use_session_user_and_keep_timeout_output(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            (root/'steam-user').write_text('teststeam')
            home = root/'user'
            user = SimpleNamespace(pw_name='teststeam', pw_dir=str(home), pw_uid=1000)
            calls = []
            def run(args, **kwargs):
                calls.append(args)
                if 'ps' in args:
                    kwargs['stdout'].write(b'lepton-active\tUp 1 minute\nlepton-stopped\tExited (0)\n')
                elif 'exec' in args:
                    kwargs['stdout'].write(b'=== boot ===\n0\n=== user ===\nRUNNING_LOCKED\n')
                    raise subprocess.TimeoutExpired(args, 8)
                return SimpleNamespace(returncode=0)
            with mock.patch.object(diag.pwd,'getpwnam',return_value=user), mock.patch.object(diag.os,'geteuid',return_value=0), mock.patch.object(diag.subprocess,'run',side_effect=run):
                result = diag.collect(root, runtime=root/'runtime')
            archive = zipfile.ZipFile(io.BytesIO(base64.b64decode(result['data'])))
            text = archive.read('apk/android-live/lepton-active.log').decode()
            self.assertIn('RUNNING_LOCKED', text)
            executions = [a for a in calls if 'exec' in a]
            self.assertEqual(len(executions), 1)
            self.assertEqual(executions[0][:4], ['runuser','-u','teststeam','--'])
            self.assertIn('HOME='+str(home), executions[0])
            self.assertIn('XDG_RUNTIME_DIR=/run/user/1000', executions[0])
            self.assertNotIn('start', executions[0])
            self.assertTrue(any('partial output' in w for w in json.loads(archive.read('report.json'))['warnings']))

if __name__ == '__main__':
    unittest.main()
