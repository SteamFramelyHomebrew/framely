"""Focused network fallback tests: no public network or device access."""
import http.server
import errno
import importlib.util
import io
import os
import pathlib
import ssl
import subprocess
import tempfile
import threading
import unittest
from unittest.mock import patch
import urllib.error
import urllib.request

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('bootstrap', ROOT / 'tools/bootstrap.py')
network = importlib.util.module_from_spec(spec)
spec.loader.exec_module(network)


class FallbackTests(unittest.TestCase):
    def test_unreachable_network_retries_once_but_local_io_errors_do_not(self):
        url = 'https://github.com/a/b/releases/download/v1/archive'
        for code in (errno.ENETDOWN, errno.ENETUNREACH, errno.EHOSTUNREACH):
            for wrapped in (False, True):
                with self.subTest(code=code, wrapped=wrapped):
                    calls = []
                    def attempt(fallback):
                        calls.append(fallback)
                        error = OSError(code, 'simulated network failure')
                        raise urllib.error.URLError(error) if wrapped else error
                    with self.assertRaisesRegex(RuntimeError, 'primary route failed.*fallback failed'):
                        network.github_attempt(url, attempt, enabled=True)
                    self.assertEqual(calls, [False, True])
                    calls.clear()
                    with self.assertRaises((OSError, urllib.error.URLError)):
                        network.github_attempt(url, attempt, enabled=False)
                    self.assertEqual(calls, [False])
        for code in (errno.ENOSPC, errno.EACCES, errno.ENOENT, errno.EIO):
            with self.subTest(code=code):
                calls = []
                def attempt(fallback):
                    calls.append(fallback)
                    raise OSError(code, 'simulated local failure')
                with self.assertRaises(OSError):
                    network.github_attempt(url, attempt, enabled=True)
                self.assertEqual(calls, [False])

    def test_q2f_checksum_failure_does_not_retry(self):
        spec = importlib.util.spec_from_file_location('q2f', ROOT / 'tools/q2f-runtime.py')
        runtime = importlib.util.module_from_spec(spec); spec.loader.exec_module(runtime)
        calls = []
        def fetch(_url, fallback=False):
            calls.append(fallback)
            response = io.BytesIO(b'payload'); response.headers = {'Content-Length':'7'}
            return response
        with tempfile.TemporaryDirectory() as temp, patch.object(runtime._network, 'fetch_once', fetch):
            with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                runtime.download({'url':'https://github.com/a/b/releases/download/v1/q2f','size':7,'sha256':'0'*64}, pathlib.Path(temp) / 'runtime')
        self.assertEqual(calls, [False])
    def test_scope_and_status_policy(self):
        self.assertTrue(network.public_github('https://api.github.com/repos/a/b/releases?page=1'))
        for url in ['http://github.com/a', 'https://github.com.evil/a', 'https://a.github.com/a',
                    'https://user@github.com/a', 'https://github.com/a?token=secret',
                    'https://gh-proxy.com/https://github.com/a', 'https://github.com/a?X-Amz-Signature=secret']:
            self.assertFalse(network.public_github(url), url)
        for code, headers, expected in [(404, {}, False), (403, {}, False),
                (403, {'X-RateLimit-Remaining': '0'}, True), (429, {}, True), (500, {}, True)]:
            self.assertEqual(network.retryable(urllib.error.HTTPError('https://github.com/a', code, '', headers, None)), expected)
        self.assertFalse(network.retryable(ssl.SSLCertVerificationError('bad certificate')))

    def test_primary_success_disabled_fallback_and_dual_failure(self):
        calls = []
        self.assertEqual(network.github_attempt('https://github.com/a', lambda f: calls.append(f) or 9), 9)
        self.assertEqual(calls, [False])
        with patch.dict(os.environ, {'FRAMELY_GITHUB_AUTO_FALLBACK':'0'}):
            with self.assertRaises(ConnectionResetError):
                network.github_attempt('https://github.com/a', lambda _: (_ for _ in ()).throw(ConnectionResetError()))
        with self.assertRaisesRegex(RuntimeError, 'primary route failed.*fallback failed') as error:
            network.github_attempt('https://github.com/a', lambda _: (_ for _ in ()).throw(ConnectionResetError('https://secret?token=value')))
        self.assertNotIn('secret', str(error.exception))

    def test_partial_body_is_discarded_using_local_http(self):
        calls = []
        class Handler(http.server.BaseHTTPRequestHandler):
            def log_message(self, *_): pass
            def do_GET(self):
                calls.append(self.path)
                self.send_response(200)
                self.send_header('Content-Length', '8')
                self.end_headers()
                self.wfile.write(b'old' if self.path == '/primary' else b'new-data')
                self.close_connection = True
        server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with tempfile.TemporaryDirectory() as temp:
                dest = pathlib.Path(temp) / 'download'
                progress = []
                def fetch(_url, fallback=False):
                    return urllib.request.urlopen(f'http://127.0.0.1:{server.server_port}/' + ('fallback' if fallback else 'primary'))
                with patch.object(network, 'fetch_once', fetch), patch.dict(os.environ, {'FRAMELY_GITHUB_AUTO_FALLBACK':'1'}):
                    network.download('https://github.com/a/b/releases/download/v1/file', dest, 100, progress.append)
                self.assertEqual(dest.read_bytes(), b'new-data')
                self.assertEqual(calls, ['/primary', '/fallback'])
                self.assertEqual(progress, [0, 0, 8])
        finally:
            server.shutdown(); server.server_close(); thread.join()

    def test_bootstrap_curl_failure_policy(self):
        # Fake curl and Python only; execute the actual public bootstrap script.
        for code, status, enabled, attempts in [(7, '000', '1', 2), (22, '404', '1', 1), (60, '000', '1', 1), (7, '000', '0', 1)]:
            with self.subTest(code=code, status=status, enabled=enabled), tempfile.TemporaryDirectory() as temp:
                root = pathlib.Path(temp); log = root / 'calls'
                curl = root / 'curl'
                curl.write_text(f'''#!/bin/sh
echo called >> '{log}'
case "$*" in *gh-proxy.com*) exit 0;; esac
printf '%s' '{status}'
exit {code}
''')
                python = root / 'python3'; python.write_text('#!/bin/sh\nexit 0\n')
                curl.chmod(0o755); python.chmod(0o755)
                env = dict(os.environ, PATH=temp + os.pathsep + os.environ['PATH'], FRAMELY_GITHUB_AUTO_FALLBACK=enabled)
                subprocess.run(['bash', str(ROOT / 'install.sh')], env=env, capture_output=True)
                self.assertEqual(len(log.read_text().splitlines()), attempts)


if __name__ == '__main__':
    unittest.main()
