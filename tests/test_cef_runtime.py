import importlib.util
import io
import json
import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

ROOT = pathlib.Path(__file__).parents[1]


def load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


packager = load('packager', ROOT / 'tools/package-runtime.py')
runtime = load('cef_runtime', ROOT / 'tools/cef-runtime.py')
bootstrap = load('bootstrap', ROOT / 'tools/bootstrap.py')


class CEFRuntimeTests(unittest.TestCase):
    def fixture(self, folder, version='1.0.0-build'):
        stage = folder / ('framely-' + version)
        files = {'VERSION': version, 'bin/framely': 'core', 'lib/cef/framely-vr': 'host',
                 'lib/cef/libcef.so': 'browser', 'lib/cef/icudtl.dat': 'icu',
                 'lib/cef/locales/en-US.pak': 'locale', 'share/licenses/cef.txt': 'license',
                 'share/licenses/cef-credits.html': 'credits', 'share/licenses/openvr.txt': 'openvr',
                 'install.sh': 'install', 'uninstall.sh': 'uninstall'}
        for name, data in files.items():
            p = stage / name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(data)
        (stage / 'tools').mkdir()
        shutil.copyfile(ROOT / 'tools/bootstrap.py', stage / 'tools/bootstrap.py')
        cef = folder / 'cef'
        (cef / 'include').mkdir(parents=True)
        (cef / 'include/cef_version.h').write_text('#define CEF_VERSION "154.0.32+g682c378+chromium-154.0.8037.58"\n')
        packager.package(stage, cef, 'owner/repo', 'v1.0.0')
        return stage

    def test_archives_checksums_and_action_selection(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = pathlib.Path(temp)
            stage = self.fixture(folder)
            archives = list(folder.glob('*.tar.gz'))
            self.assertEqual(len(archives), 3)
            for archive in archives:
                bootstrap.verify_archive(archive, folder / 'SHA256SUMS')
            assets = {p.name: {} for p in archives} | {'SHA256SUMS': {}, 'framely-cef.json': {}}
            self.assertEqual(bootstrap.select_package(assets, 'install'), stage.name + '-offline-linux-arm64.tar.gz')
            self.assertEqual(bootstrap.select_package(assets, 'update'), stage.name + '-linux-arm64.tar.gz')
            del assets[stage.name + '-offline-linux-arm64.tar.gz']
            with self.assertRaises(ValueError):
                bootstrap.select_package(assets, 'install')
            for archive in archives:
                extracted = bootstrap.extract_archive(archive, folder / archive.name.replace('.tar.gz', ''),
                                                      runtime=archive.name.startswith('framely-cef-'))
                subprocess.run(['sha256sum', '--quiet', '-c', 'SHA256SUMS'], cwd=extracted, check=True)
                if archive.name == stage.name + '-linux-arm64.tar.gz':
                    self.assertFalse((extracted / 'lib/cef/libcef.so').exists())
                    self.assertFalse((extracted / 'share/licenses/cef-credits.html').exists())
                    self.assertTrue((extracted / 'lib/cef/framely-vr').exists())

    def test_offline_install_core_update_and_legacy_migration_without_download(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = pathlib.Path(temp)
            stage = self.fixture(folder)
            store = folder / 'store'
            store.mkdir()
            cached = runtime.prepare(stage, store)
            core = bootstrap.extract_archive(folder / (stage.name + '-linux-arm64.tar.gz'), folder / 'core')
            self.assertEqual(runtime.prepare(core, store), cached)
            runtime.attach(core, cached)
            self.assertTrue((core / 'lib/cef/libcef.so').is_symlink())
            self.assertEqual((core / 'lib/cef/libcef.so').read_text(), 'browser')
            self.assertEqual((core / 'lib/cef/framely-vr').read_text(), 'host')
            subprocess.run(['sha256sum', '--quiet', '-c', 'SHA256SUMS'], cwd=core, check=True)
            # A pre-split installation with the same files seeds the new cache.
            legacy_store = folder / 'legacy'
            (legacy_store / 'state').mkdir(parents=True)
            (legacy_store / 'state/current').symlink_to(stage)
            unattached = bootstrap.extract_archive(folder / (stage.name + '-linux-arm64.tar.gz'), folder / 'legacy-core')
            self.assertEqual(runtime.prepare(unattached, legacy_store).name, cached.name)
            (cached / 'lib/cef/libcef.so').write_text('tampered')
            with self.assertRaises(ValueError):
                runtime.prepare(core, store)

    def test_download_matching_runtime_and_reject_tampering(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = pathlib.Path(temp)
            stage = self.fixture(folder)
            core = bootstrap.extract_archive(folder / (stage.name + '-linux-arm64.tar.gz'), folder / 'core')
            requirement = json.loads((core / 'CEF_RUNTIME.json').read_text())
            data = (folder / requirement['archive']).read_bytes()
            # The downloaded helper imports the shipped bootstrap module.
            for bad in (True, False):
                store = folder / ('bad-store' if bad else 'download-store')
                store.mkdir()
                response = io.BytesIO(data[:-1] if bad else data)
                with patch('urllib.request.OpenerDirector.open', return_value=response):
                    if bad:
                        with self.assertRaises(ValueError):
                            runtime.prepare(core, store)
                        self.assertFalse((store / 'cef' / requirement['id']).exists())
                    else:
                        cached = runtime.prepare(core, store)
                        self.assertEqual((cached / 'lib/cef/libcef.so').read_text(), 'browser')

    def test_runtime_extraction_rejects_links(self):
        with tempfile.TemporaryDirectory() as temp:
            folder = pathlib.Path(temp)
            archive = folder / 'bad.tar.gz'
            with tarfile.open(archive, 'w:gz') as tar:
                entry = tarfile.TarInfo('framely-cef-1/lib/cef/libcef.so')
                entry.type = tarfile.SYMTYPE
                entry.linkname = '/etc/shadow'
                tar.addfile(entry)
            with self.assertRaises(ValueError):
                bootstrap.extract_archive(archive, folder / 'out', runtime=True)
            self.assertFalse((folder / 'out').exists())
