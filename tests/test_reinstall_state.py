"""Run the production reinstall block against retained uninstall state."""
import json
import pathlib
import subprocess
import tempfile
import unittest

class ReinstallStateTests(unittest.TestCase):
    def test_only_clean_reinstall_clears_uninstall_guard(self):
        source = (pathlib.Path(__file__).parents[1] / 'packaging/install.sh').read_text()
        block = source.split('# Clear the uninstall guard retained by older releases', 1)[1].split('ln -s "releases/$version"', 1)[0]
        block = '# Clear the uninstall guard retained by older releases' + block
        for repair, old, plugins, expected in [
            ('false', '', {}, False),
            ('true', '', {}, True),
            ('false', 'releases/current', {}, True),
            ('false', '', {'demo': {'enabled': False}}, True),
        ]:
            with self.subTest(repair=repair, old=old, plugins=plugins), tempfile.TemporaryDirectory() as temp:
                root = pathlib.Path(temp)
                path = root / 'state.json'
                original = {'safeMode': True, 'plugins': plugins, 'language': 'en-US', 'sources': [{'url': 'https://example.org'}]}
                path.write_text(json.dumps(original))
                result = subprocess.run(['bash', '-e', '-c', 'root=$1; repair=$2; old=$3\n' + block, 'reinstall', str(root), repair, old], capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                expected_state = dict(original, safeMode=expected)
                self.assertEqual(json.loads(path.read_text()), expected_state)
                if expected:
                    self.assertEqual(path.read_text(), json.dumps(original))
                else:
                    self.assertEqual(path.stat().st_mode & 0o777, 0o600)

if __name__ == '__main__':
    unittest.main()
