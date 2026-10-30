import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('android_memory', ROOT / 'tools/android-memory.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class AndroidMemoryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.kernel = self.base / 'overcommit_memory'
        self.kernel.write_text('0\n')
        self.policy = module.Policy(self.base / 'state', self.base / 'sysctl.d/99-framely.conf', self.kernel)

    def test_apply_commit_update_and_uninstall_restore_original(self):
        self.policy.apply()
        self.assertEqual(self.kernel.read_text(), '1\n')
        self.assertEqual(self.policy.original.read_text(), '0\n')
        self.policy.commit()
        self.policy.apply()
        self.policy.commit()
        self.assertEqual(self.policy.original.read_text(), '0\n')
        self.policy.uninstall()
        self.assertEqual(self.kernel.read_text(), '0\n')
        self.assertFalse(self.policy.config.exists())
        self.assertFalse(self.policy.original.exists())

    def test_failed_update_rolls_back_file_and_live_state(self):
        self.policy.apply()
        self.policy.rollback()
        self.assertEqual(self.kernel.read_text(), '0\n')
        self.assertFalse(self.policy.config.exists())
        self.assertFalse(self.policy.original.exists())
        self.kernel.write_text('2\n')
        self.policy.apply()
        self.policy.commit()
        self.kernel.write_text('0\n')  # Value at the start of the next update.
        self.policy.apply()
        self.policy.rollback()
        self.assertEqual(self.kernel.read_text(), '0\n')
        self.assertEqual(self.policy.original.read_text(), '2\n')
        self.assertEqual(self.policy.config.read_text(), module.CONFIG)

    def test_adopts_previous_test_script_without_losing_original(self):
        self.policy.write(self.policy.config, module.MARKER + '\nvm.overcommit_memory = 1\n')
        self.policy.write(self.policy.original, '0\n')
        self.kernel.write_text('1\n')
        self.policy.apply()
        self.policy.commit()
        self.assertEqual(self.policy.original.read_text(), '0\n')
        self.policy.uninstall()
        self.assertEqual(self.kernel.read_text(), '0\n')

    def test_admin_configuration_and_symlinks_are_not_overwritten(self):
        self.policy.write(self.policy.config, '# Admin\nvm.overcommit_memory=2\n')
        with self.assertRaises(ValueError):
            self.policy.apply()
        self.policy.uninstall()
        self.assertIn('# Admin', self.policy.config.read_text())
        self.policy.config.unlink()
        self.policy.config.symlink_to(self.kernel)
        with self.assertRaises(ValueError):
            self.policy.apply()
        self.assertEqual(self.kernel.read_text(), '0\n')

    def test_admin_runtime_change_is_not_undone_on_uninstall(self):
        self.policy.apply()
        self.policy.commit()
        self.kernel.write_text('2\n')
        self.policy.uninstall()
        self.assertEqual(self.kernel.read_text(), '2\n')

    def test_interrupted_install_is_recovered_before_retry(self):
        self.policy.apply()
        self.policy.apply()
        self.policy.commit()
        self.assertEqual(self.policy.original.read_text(), '0\n')
        self.policy.uninstall()
        self.assertEqual(self.kernel.read_text(), '0\n')


if __name__ == '__main__':
    unittest.main()
