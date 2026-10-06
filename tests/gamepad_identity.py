#!/usr/bin/env python3
"""Frame integration: concurrent pads keep independent SteamVR identities.

Usage: gamepad_identity.py HELPER ACTION_MANIFEST IDENTITY_PROBE
Both helpers remain disabled. No physical input is read or forwarded.
"""
import json
import pathlib
import select
import subprocess
import sys
import tempfile
import uuid


def check(helper, actions, probe):
    children = []
    nodes = []
    keys = []
    with tempfile.TemporaryDirectory(prefix='framely-pad-identity-') as directory:
        try:
            for n in range(2):
                key = 'framely.gamepad.' + uuid.uuid4().hex
                path = pathlib.Path(directory) / f'{n}.vrmanifest'
                path.write_text(json.dumps({'applications': [{
                    'app_key': key, 'launch_type': 'binary', 'is_self_identified': True,
                    'binary_path_linux_arm': helper, 'binary_path_linux': helper, 'action_manifest_path': actions,
                    'strings': {'en_us': {'name': 'Framely neutral pad test'}},
                }]}))
                child = subprocess.Popen([helper, actions, str(path), key],
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                         stderr=subprocess.PIPE)
                children.append(child)
                keys.append(key)
                assert select.select([child.stdout], [], [], 8)[0], 'Readiness timeout'
                node = pathlib.Path(child.stdout.readline().decode().strip())
                assert node.parent == pathlib.Path('/dev/input') and node.is_char_device(), child.stderr.read().decode() if child.poll() is not None else 'Missing node'
                nodes.append(node)
            subprocess.run([probe, keys[0], str(children[0].pid), keys[1], str(children[1].pid)], check=True)
            children[0].stdin.close()
            assert children[0].wait(timeout=5) == 0
            subprocess.run([probe, keys[0], '0', keys[1], str(children[1].pid)], check=True)
            children[1].stdin.close()
            assert children[1].wait(timeout=5) == 0
            assert all(not node.exists() for node in nodes)
            print('PASS: independent identities survive another pad closing; EOF removes both devices')
        finally:
            for child in children:
                if child.poll() is None:
                    child.terminate()
                    child.wait(timeout=5)


if __name__ == '__main__':
    check(*sys.argv[1:])
