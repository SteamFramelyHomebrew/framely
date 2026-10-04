import importlib.util,io,pathlib,tarfile,tempfile,unittest
spec=importlib.util.spec_from_file_location('extract',pathlib.Path(__file__).parents[1]/'tools/extract-release.py');module=importlib.util.module_from_spec(spec);spec.loader.exec_module(module)
class Extraction(unittest.TestCase):
 def archive(self,root,members):
  archive=root/'release.tar.gz'
  with tarfile.open(archive,'w:gz') as tar:
   for name,kind in members:
    entry=tarfile.TarInfo(name);entry.mode=0o755
    if kind=='link':entry.type=tarfile.SYMTYPE;entry.linkname='/etc/shadow';tar.addfile(entry)
    else:entry.size=4;tar.addfile(entry,io.BytesIO(b'test'))
  return archive
 def test_valid_release_and_destination_preservation(self):
  with tempfile.TemporaryDirectory() as tmp:
   root=pathlib.Path(tmp);archive=self.archive(root,[('framely-1/VERSION','file')]);dest=root/'out';module.extract(archive,dest,'1');self.assertEqual((dest/'framely-1/VERSION').read_bytes(),b'test')
   with self.assertRaises(ValueError):module.extract(archive,dest,'1')
   self.assertEqual((dest/'framely-1/VERSION').read_bytes(),b'test')
 def test_links_traversal_duplicates_and_wrong_version(self):
  for members in [[('framely-1/../../escape','file')],[('/absolute','file')],[('framely-1/link','link')],[('framely-2/VERSION','file')],[('framely-1/VERSION','file')]*2]:
   with tempfile.TemporaryDirectory() as tmp:
    root=pathlib.Path(tmp);archive=self.archive(root,members)
    with self.assertRaises(ValueError):module.extract(archive,root/'out','1')
    self.assertFalse((root/'out').exists())
if __name__=='__main__':unittest.main()
