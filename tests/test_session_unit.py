"""Check the actual installed UI unit waits for the user runtime provider."""
import configparser
import pathlib
import subprocess
import unittest


class SessionUnitTests(unittest.TestCase):
    def test_session_requires_and_orders_after_user_manager(self):
        script = (pathlib.Path(__file__).parents[1] / 'packaging/install.sh').read_text()
        body = script.split('cat > /etc/systemd/system/framely-session.service <<EOF\n', 1)[1].split('\nEOF', 1)[0]
        result = subprocess.run(
            ['bash', '-eu', '-c', 'steam_uid=1000; steam_user=steamos; steam_home=/home/steamos; root=/var/lib/framely\ncat <<EOF\n' + body + '\nEOF\n'],
            capture_output=True, text=True, check=True,
        )
        unit = configparser.ConfigParser(interpolation=None, strict=False)
        unit.read_string(result.stdout)
        for dependency in ['framely.service', f'user@{1000}.service']:
            self.assertIn(dependency, unit['Unit']['Requires'].split())
            self.assertIn(dependency, unit['Unit']['After'].split())


if __name__ == '__main__':
    unittest.main()
