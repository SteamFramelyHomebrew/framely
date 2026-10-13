import importlib.util
import json
import pathlib
import tarfile
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / 'tools/q2f-runtime.py'
SPEC = importlib.util.spec_from_file_location('q2f_runtime', SCRIPT)
RUNTIME = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNTIME)


class Q2FRuntimeTests(unittest.TestCase):
    def test_exact_cache_is_reused_and_tampering_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            base = pathlib.Path(directory)
            identity = 'overport-test-123456789abc'
            payload = base / 'payload'
            (payload / 'java/bin').mkdir(parents=True)
            for name in ('java/bin/java', 'overportcli.jar', 'overport-installed.json', 'OVR_PORT_LICENSE'):
                (payload / name).write_text(name)
            files = sorted(p for p in payload.rglob('*') if p.is_file())
            sums = ''.join(f'{RUNTIME.digest(p)}  {p.relative_to(payload)}\n' for p in files)
            (payload / 'VERSION').write_text(identity + '\n')
            (payload / 'SHA256SUMS').write_text(sums)
            archive = base / f'framely-q2f-{identity}-linux-arm64.tar.gz'
            with tarfile.open(archive, 'w:gz') as tar:
                tar.add(payload, arcname=f'framely-q2f-{identity}')
            descriptor = base / 'requirement.json'
            descriptor.write_text(json.dumps({'schemaVersion': 1, 'id': identity,
                'archive': archive.name, 'size': archive.stat().st_size,
                'sha256': RUNTIME.digest(archive), 'url': 'https://example.invalid/q2f'}))
            copies = []
            def copy(_requirement, destination):
                destination.write_bytes(archive.read_bytes())
                copies.append(destination)
            with patch.object(RUNTIME, 'download', copy):
                cached = RUNTIME.prepare(descriptor, base / 'storage')
                self.assertEqual(cached, RUNTIME.prepare(descriptor, base / 'storage'))
            self.assertEqual(len(copies), 1)
            (cached / 'overportcli.jar').write_text('changed')
            with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                RUNTIME.prepare(descriptor, base / 'storage')

    def test_archive_rejects_link_and_escape(self):
        with tempfile.TemporaryDirectory() as directory:
            base = pathlib.Path(directory)
            identity = 'overport-test-123456789abc'
            archive = base / 'unsafe.tar.gz'
            for index, member in enumerate((f'framely-q2f-{identity}/../escape', f'framely-q2f-{identity}/link')):
                with tarfile.open(archive, 'w:gz') as tar:
                    info = tarfile.TarInfo(member)
                    info.type = tarfile.SYMTYPE if member.endswith('/link') else tarfile.REGTYPE
                    info.size = 0
                    tar.addfile(info)
                with self.assertRaisesRegex(ValueError, 'Unsafe'):
                    RUNTIME.extract(archive, base / f'target-{index}', identity)


if __name__ == '__main__':
    unittest.main()
