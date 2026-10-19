import importlib.util
from pathlib import Path
import unittest


spec = importlib.util.spec_from_file_location(
    "steam_system_menu",
    Path(__file__).parents[1] / "native/input/steam-system-menu.py",
)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class BindingTests(unittest.TestCase):
    def test_removes_only_system_long_recenter(self):
        binding = {
            "bindings": {
                "/actions/system": {"sources": [{
                    "path": "/user/hand/left/input/system",
                    "inputs": {"click": {"output": "dashboard"}, "double": {"output": "room"}},
                }]},
                "/actions/quickrecenter": {"sources": [
                    {"path": "/user/hand/left/input/system", "inputs": {"long": {"output": "recenter"}}},
                    {"path": "/user/hand/right/input/system", "inputs": {"long": {"output": "recenter"}}},
                    {"path": "/user/hand/left/input/grip", "inputs": {"long": {"output": "recenter"}}},
                ]},
            }
        }
        patched, changed = module.without_recenter(binding)
        self.assertTrue(changed)
        self.assertEqual(len(patched["bindings"]["/actions/quickrecenter"]["sources"]), 1)
        self.assertEqual(patched["bindings"]["/actions/system"], binding["bindings"]["/actions/system"])
        self.assertEqual(len(binding["bindings"]["/actions/quickrecenter"]["sources"]), 3)


if __name__ == "__main__":
    unittest.main()
