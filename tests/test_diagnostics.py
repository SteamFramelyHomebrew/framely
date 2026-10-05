import base64
import importlib.util
import io
import json
import pathlib
import tempfile
import unittest
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

if __name__ == '__main__':
    unittest.main()
