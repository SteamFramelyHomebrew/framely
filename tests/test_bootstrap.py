import hashlib
import importlib.util
import io
import json
import contextlib
import pathlib
import tarfile
import tempfile
import unittest
import errno
import http.client
import urllib.error
from types import SimpleNamespace
from unittest.mock import mock_open, patch

spec = importlib.util.spec_from_file_location('bootstrap', pathlib.Path(__file__).parents[1] / 'tools/bootstrap.py')
bootstrap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bootstrap)


class BootstrapTests(unittest.TestCase):
    def test_incomplete_metadata_has_actionable_error(self):
        with self.assertRaises(bootstrap.InstallationFailure) as failure:
            with bootstrap.installation_step('Read release metadata', 'Retry or choose another mirror.'):
                raise http.client.IncompleteRead(b'partial', 10)
        self.assertIn('Retry or choose another mirror.', str(failure.exception))

    def test_interactive_release_lookup_retries_partial_transfer(self):
        releases = [{'tag_name': 'v0.8.3', 'draft': False, 'prerelease': False,
                     'assets': [{'name': name, 'size': 1024} for name in (
                         'bootstrap.py', 'SHA256SUMS',
                         'framely-0.8.3-012345abcdef-linux-arm64.tar.gz')]}]
        with patch.object(bootstrap, 'read_metadata', side_effect=[
                http.client.IncompleteRead(b'partial', 10), json.dumps(releases).encode()]) as read, \
             patch.object(bootstrap.time, 'sleep'):
            self.assertEqual(bootstrap.resolve_release('owner/repo', 'stable', 'update')['tag'], 'v0.8.3')
            self.assertEqual(read.call_count, 2)

    def test_explicit_direct_route_overrides_saved_proxy(self):
        with patch.dict(bootstrap.os.environ, {'FRAMELY_GITHUB_PROXY': '',
                                               'FRAMELY_GITHUB_AUTO_FALLBACK': '0'}), \
             patch.object(bootstrap, 'device_proxy_settings', return_value={
                 'githubEnabled': True, 'github': 'https://proxy.example'}):
            self.assertEqual(bootstrap.configured_github_proxy(), '')
            self.assertFalse(bootstrap.fallback_enabled())

    def test_explicit_preview_uses_release_assets_without_api(self):
        tag = 'v0.8.4-preview.1'
        package = 'framely-0.8.4-preview.1-012345abcdef-linux-arm64.tar.gz'
        checksums = hashlib.sha256(b'archive').hexdigest() + '  ' + package + '\n'
        urls = []

        def download(url, destination, _limit, _progress=None, expected_size=None):
            urls.append(url)
            pathlib.Path(destination).write_bytes(checksums.encode() if url.endswith('SHA256SUMS') else b'archive')

        with tempfile.TemporaryDirectory() as tmp, \
             patch.dict(bootstrap.os.environ, {}, clear=True), \
             patch.object(bootstrap.pathlib.Path, 'home', return_value=pathlib.Path(tmp)), \
             patch.object(bootstrap.os, 'uname', return_value=SimpleNamespace(machine='aarch64')), \
             patch.object(bootstrap.os, 'geteuid', return_value=0), \
             patch.object(bootstrap, 'read_metadata', side_effect=AssertionError('API must not be used')), \
             patch.object(bootstrap, 'download', side_effect=download), \
             patch.object(bootstrap, 'root_operation') as operation:
            bootstrap.main(['update', '--version', tag, '--github-proxy', 'https://gh-proxy.com', '--yes'])
            operation.assert_called_once()
        self.assertEqual(urls, [
            f'https://github.com/{bootstrap.DEFAULT_REPO}/releases/download/{tag}/SHA256SUMS',
            f'https://github.com/{bootstrap.DEFAULT_REPO}/releases/download/{tag}/{package}',
        ])

    def test_explicit_tag_requires_one_matching_package(self):
        package = 'framely-0.8.4-preview.1-012345abcdef-linux-arm64.tar.gz'
        offline = 'framely-0.8.4-preview.1-012345abcdef-offline-linux-arm64.tar.gz'
        with tempfile.TemporaryDirectory() as tmp:
            sums = pathlib.Path(tmp) / 'SHA256SUMS'
            sums.write_text('0' * 64 + '  framely-0.8.3-012345abcdef-linux-arm64.tar.gz\n')
            with self.assertRaisesRegex(ValueError, 'one matching ARM64 package'):
                bootstrap.package_from_checksums(sums, 'v0.8.4-preview.1', 'update')
            sums.write_text('0' * 64 + '  ' + package + '\n' + '1' * 64 + '  ' + package + '\n')
            with self.assertRaisesRegex(ValueError, 'duplicate SHA256SUMS'):
                bootstrap.package_from_checksums(sums, 'v0.8.4-preview.1', 'update')
            sums.write_text('0' * 64 + '  ' + package + '\n' + '1' * 64 + '  ' + offline + '\n')
            self.assertEqual(bootstrap.package_from_checksums(sums, 'v0.8.4-preview.1', 'update'), package)
            self.assertEqual(bootstrap.package_from_checksums(sums, 'v0.8.4-preview.1', 'install'), offline)

    def test_interactive_release_selection_uses_semver_and_installable_assets(self):
        def release(tag, prerelease=False, missing_offline=False):
            version = tag.removeprefix('v')
            files = ['bootstrap.py', 'SHA256SUMS',
                     f'framely-{version}-012345abcdef-linux-arm64.tar.gz']
            if not missing_offline:
                files.append(f'framely-{version}-012345abcdef-offline-linux-arm64.tar.gz')
            else:
                files.append('framely-cef.json')
            return {'tag_name': tag, 'draft': False, 'prerelease': prerelease,
                    'assets': [{'name': name, 'size': 1024} for name in files]}
        releases = [release('v0.8.9'), release('v0.9.0-preview.2', True),
                    release('v0.9.0-preview.10', True),
                    release('v1.0.0', missing_offline=True), release('v2.0.0', True)]
        with patch.object(bootstrap, 'read_metadata', return_value=json.dumps(releases).encode()):
            self.assertEqual(bootstrap.resolve_release('owner/repo', 'stable', 'install')['tag'], 'v0.8.9')
            self.assertEqual(bootstrap.resolve_release('owner/repo', 'testing', 'install')['tag'], 'v0.9.0-preview.10')
            self.assertEqual(bootstrap.resolve_release('owner/repo', 'stable', 'update')['tag'], 'v1.0.0')
        self.assertGreater(bootstrap.release_version('v0.9.0-preview.10'),
                           bootstrap.release_version('v0.9.0-preview.2'))

    def test_explicit_install_and_update_skip_confirmation(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = pathlib.Path(tmp) / 'package.tar.gz'
            checksums = pathlib.Path(tmp) / 'SHA256SUMS'
            archive.touch()
            checksums.touch()
            with patch.object(bootstrap.os, 'uname', return_value=SimpleNamespace(machine='aarch64')), \
                 patch.object(bootstrap.os, 'geteuid', return_value=0), \
                 patch.object(bootstrap, 'confirm', side_effect=AssertionError('unexpected prompt')), \
                 patch.object(bootstrap, 'verify_archive'), \
                 patch.object(bootstrap, 'root_operation') as operation:
                for action in ('install', 'update'):
                    bootstrap.main([action, '--archive', str(archive), '--checksums', str(checksums)])
                self.assertEqual(operation.call_count, 2)

    def test_github_proxy_option_reaches_engine_downloads(self):
        with tempfile.TemporaryDirectory() as tmp:
            archive = pathlib.Path(tmp) / 'package.tar.gz'
            checksums = pathlib.Path(tmp) / 'SHA256SUMS'
            archive.touch()
            checksums.touch()
            with patch.dict(bootstrap.os.environ, {}, clear=True), \
                 patch.object(bootstrap.os, 'uname', return_value=SimpleNamespace(machine='aarch64')), \
                 patch.object(bootstrap.os, 'geteuid', return_value=0), \
                 patch.object(bootstrap, 'verify_archive'), \
                 patch.object(bootstrap, 'root_operation'):
                bootstrap.main(['install', '--archive', str(archive), '--checksums', str(checksums),
                                '--github-proxy', 'https://gh-proxy.com/'])
                self.assertEqual(bootstrap.configured_github_proxy(), 'https://gh-proxy.com')

    def test_explicit_uninstall_still_requires_confirmation(self):
        with patch.object(bootstrap.os, 'uname', return_value=SimpleNamespace(machine='aarch64')), \
             patch.object(bootstrap, 'confirm', return_value=False) as confirmation:
            with self.assertRaisesRegex(ValueError, 'Cancelled'):
                bootstrap.main(['uninstall'])
            confirmation.assert_called_once()

    def test_confirmation_distinguishes_no_terminal_from_decline(self):
        with patch('builtins.open', side_effect=OSError('no tty')):
            with self.assertRaisesRegex(ValueError, 'No interactive terminal.*--yes'):
                bootstrap.confirm('install')
        with patch('builtins.open', mock_open(read_data='')):
            with self.assertRaisesRegex(ValueError, 'No confirmation input.*--yes'):
                bootstrap.confirm('install')
        with patch('builtins.open', mock_open(read_data='n\n')):
            self.assertFalse(bootstrap.confirm('install'))
        with patch('builtins.open', mock_open(read_data='Y\n')):
            self.assertTrue(bootstrap.confirm('install'))

    def test_external_checksums_match_exact_name_and_reject_tampering(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            archive = root / 'framely-1-linux-arm64.tar.gz'
            sums = root / 'SHA256SUMS'
            archive.write_bytes(b'payload')
            entry = hashlib.sha256(b'payload').hexdigest() + '  ' + archive.name + '\n'
            sums.write_text(entry)
            bootstrap.verify_archive(archive, sums)
            archive.write_bytes(b'changed')
            with self.assertRaises(ValueError):
                bootstrap.verify_archive(archive, sums)
            sums.write_text(entry * 2)
            with self.assertRaises(ValueError):
                bootstrap.verify_archive(archive, sums)
            sums.write_text(entry.replace(archive.name, './' + archive.name))
            with self.assertRaises(ValueError):
                bootstrap.verify_archive(archive, sums)

    def archive(self, root, entries):
        archive = root / 'release.tar.gz'
        with tarfile.open(archive, 'w:gz') as out:
            for name, contents in entries:
                member = tarfile.TarInfo(name)
                if contents is None:
                    member.type = tarfile.SYMTYPE
                    member.linkname = '/etc/shadow'
                    out.addfile(member)
                else:
                    member.size = len(contents)
                    out.addfile(member, io.BytesIO(contents))
        return archive

    def test_safe_extraction_checks_structure_before_writing(self):
        entries = [('framely-1/' + name, b'1' if name == 'VERSION' else b'test') for name in ['VERSION', 'SHA256SUMS', 'install.sh', 'uninstall.sh', 'bin/framely']]
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            base = bootstrap.extract_archive(self.archive(root, entries), root / 'out')
            self.assertEqual(base.name, 'framely-1')
        for extra in [('framely-1/../../escape', b'bad'), ('framely-1/link', None), ('framely-1/VERSION', b'1'), ('framely-2/file', b'bad')]:
            with self.subTest(extra=extra), tempfile.TemporaryDirectory() as tmp:
                root = pathlib.Path(tmp)
                with self.assertRaises(ValueError):
                    bootstrap.extract_archive(self.archive(root, entries + [extra]), root / 'out')
                self.assertFalse((root / 'out').exists())

    def test_redirect_cannot_downgrade_https(self):
        with self.assertRaises(ValueError):
            bootstrap.HTTPSRedirect().redirect_request(None, None, 302, '', {}, 'http://example.org/package')

    def test_extraction_progress_reports_real_bytes_and_never_completes_invalid_release(self):
        entries = [('framely-1/' + name, b'1' if name == 'VERSION' else b'test') for name in ['VERSION', 'SHA256SUMS', 'install.sh', 'uninstall.sh', 'bin/framely']]
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            reports = []
            bootstrap.extract_archive(self.archive(root, entries), root / 'out', lambda *value: reports.append(value))
            total = sum(len(contents) for _, contents in entries)
            self.assertEqual(reports[0], ('extract', 0, total))
            self.assertEqual(reports[-1], ('extract', total, total))
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            reports = []
            with self.assertRaises(ValueError):
                bootstrap.extract_archive(self.archive(root, [('framely-1/VERSION', b'wrong')]), root / 'out', lambda *value: reports.append(value))
            self.assertFalse(any(done == total and total > 0 for _, done, total in reports))
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            bootstrap.emit_progress('configure')
        payload = json.loads(output.getvalue().removeprefix('FRAMELY_PROGRESS '))
        self.assertEqual(payload, {'step': 'configure'})

    def test_installation_failure_explains_step_cause_and_next_action_without_url(self):
        with self.assertRaises(bootstrap.InstallationFailure) as failure:
            with bootstrap.installation_step('下载安装包', '更换代理后重试。'):
                raise urllib.error.HTTPError('https://example.invalid/?token=secret', 502, '', {}, None)
        message = str(failure.exception)
        self.assertIn('下载安装包失败', message)
        self.assertIn('HTTP 502', message)
        self.assertIn('更换代理后重试', message)
        self.assertNotIn('secret', message)
        with self.assertRaises(bootstrap.InstallationFailure) as failure:
            with bootstrap.installation_step('下载安装包', '释放空间后重试。'):
                raise OSError(errno.ENOSPC, 'disk full')
        self.assertIn('设备存储空间不足', str(failure.exception))
        self.assertIn('errno=28', str(failure.exception))
        with self.assertRaises(bootstrap.InstallationFailure) as failure:
            with bootstrap.installation_step('读取发行版信息', '检查连接后重试。'):
                raise ValueError('bad URL https://example.invalid/?token=secret')
        self.assertNotIn('secret', str(failure.exception))

    def test_terminal_download_progress_reports_bytes_and_completion(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            progress = bootstrap.terminal_download_progress(2 * 1024**2)
            progress(0)
            progress(2 * 1024**2)
        self.assertIn('0.0 / 2.0 MiB', output.getvalue())
        self.assertIn('100.0%', output.getvalue())
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            progress = bootstrap.terminal_download_progress(None)
            progress(0)
            progress(2 * 1024**2)
        self.assertIn('总大小待确认', output.getvalue())
        self.assertNotIn('100.0%', output.getvalue())


if __name__ == '__main__':
    unittest.main()
