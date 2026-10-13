import importlib.util
import io
import pathlib
import struct
import tarfile
import tempfile
import unittest
from unittest.mock import patch

SCRIPT = pathlib.Path(__file__).resolve().parents[1] / 'tools/package-q2f.py'
SPEC = importlib.util.spec_from_file_location('package_q2f', SCRIPT)
PACKAGE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(PACKAGE)


def elf(machine, elf_class):
    data = bytearray(1024)
    data[:6] = b'\x7fELF' + bytes((elf_class, 1))
    struct.pack_into('<H', data, 18, machine)
    return bytes(data)


class FrameBridgePackageTests(unittest.TestCase):
    def test_wraps_both_abis_and_keeps_corresponding_source(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            payload = root / 'payload'
            adapters = {}
            for abi, machine in PACKAGE.FRAMEBRIDGE_ABIS.items():
                lib = payload / 'libraries' / PACKAGE.LIBRARIES_VERSION / 'lib' / abi
                lib.mkdir(parents=True)
                (lib / 'libopenxr_loader_generic.so').write_bytes(b'original ' + abi.encode())
                adapter = root / f'{abi}.so'
                adapter.write_bytes(elf(machine, 2 if abi == 'arm64-v8a' else 1))
                adapters[abi] = adapter
            source = root / 'frameport.tar.gz'
            prefix = f'frameport-{PACKAGE.FRAMEBRIDGE_COMMIT}/'
            with tarfile.open(source, 'w:gz') as archive:
                for name in ('LICENSE', 'THIRD_PARTY_NOTICES.md', 'native/README.md',
                             'native/build.py', 'native/adapter/frame_adapter.c'):
                    content = name.encode()
                    entry = tarfile.TarInfo(prefix + name)
                    entry.size = len(content)
                    archive.addfile(entry, io.BytesIO(content))
            with patch.object(PACKAGE, 'FRAMEBRIDGE_SOURCE_SHA256', PACKAGE.digest(source)):
                PACKAGE.install_framebridge(payload, source, adapters)
            self.assertEqual((payload / 'FRAMEBRIDGE_LICENSE').read_bytes(), b'LICENSE')
            with tarfile.open(payload / 'FRAMEBRIDGE_SOURCE.tar.gz') as archive:
                self.assertIn('framebridge/native/adapter/frame_adapter.c', archive.getnames())
                self.assertIn('framebridge/native/build.py', archive.getnames())
            for abi in adapters:
                lib = payload / 'libraries' / PACKAGE.LIBRARIES_VERSION / 'lib' / abi
                self.assertEqual((lib / 'libopenxr_loader_original.so').read_bytes(), b'original ' + abi.encode())
                self.assertEqual((lib / 'libopenxr_loader_generic.so').read_bytes(), adapters[abi].read_bytes())
                self.assertIn(b'controller_fix=1', (lib / 'libframe_settings.so').read_bytes())

    def test_rejects_wrong_adapter_architecture_before_replacing_loader(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            payload = root / 'payload'
            source = root / 'source.tar.gz'
            prefix = f'frameport-{PACKAGE.FRAMEBRIDGE_COMMIT}/'
            with tarfile.open(source, 'w:gz') as archive:
                for name in ('LICENSE', 'THIRD_PARTY_NOTICES.md', 'native/README.md',
                             'native/build.py', 'native/adapter/frame_adapter.c'):
                    content = name.encode()
                    entry = tarfile.TarInfo(prefix + name)
                    entry.size = len(content)
                    archive.addfile(entry, io.BytesIO(content))
            lib = payload / 'libraries' / PACKAGE.LIBRARIES_VERSION / 'lib/arm64-v8a'
            lib.mkdir(parents=True)
            (lib / 'libopenxr_loader_generic.so').write_bytes(b'original')
            adapter = root / 'wrong.so'
            adapter.write_bytes(elf(40, 1))
            with patch.object(PACKAGE, 'FRAMEBRIDGE_SOURCE_SHA256', PACKAGE.digest(source)):
                with self.assertRaisesRegex(ValueError, 'wrong architecture'):
                    PACKAGE.install_framebridge(payload, source, {'arm64-v8a': adapter, 'armeabi-v7a': adapter})
            self.assertEqual((lib / 'libopenxr_loader_generic.so').read_bytes(), b'original')


if __name__ == '__main__':
    unittest.main()
