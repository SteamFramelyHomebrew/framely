import hashlib
import importlib.util
import io
import pathlib
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('bootstrap', pathlib.Path(__file__).parents[1] / 'tools/bootstrap.py')
bootstrap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(bootstrap)


class BootstrapTests(unittest.TestCase):
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


if __name__ == '__main__':
    unittest.main()
