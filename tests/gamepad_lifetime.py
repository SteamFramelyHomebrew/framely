#!/usr/bin/env python3
"""Frame integration check: worker exit retains the pad, pipe EOF removes it.

Run as the Steam user with SteamVR awake. The helper remains disabled throughout;
this test creates its own neutral virtual device and never reads physical input.
Usage: python3 tests/gamepad_lifetime.py HELPER ACTION_MANIFEST
"""
import pathlib
import select
import subprocess
import sys
import threading
import time


def check(helper, manifest):
    state = {}

    def worker():
        try:
            child = subprocess.Popen(
                [helper, manifest], stdin=subprocess.PIPE,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            )
            state['child'] = child
            if not select.select([child.stdout], [], [], 8)[0]:
                raise RuntimeError('Virtual gamepad readiness timed out')
            state['node'] = pathlib.Path(child.stdout.readline().decode().strip())
        except Exception as error:
            state['error'] = error

    thread = threading.Thread(target=worker)
    thread.start()
    thread.join(10)
    child = state.get('child')
    try:
        assert not thread.is_alive(), 'Preparation worker did not finish'
        if 'error' in state:
            raise state['error']
        node = state['node']
        assert node.parent == pathlib.Path('/dev/input') and node.name.startswith('event')
        time.sleep(2)
        assert child.poll() is None, 'Worker exit destroyed the virtual gamepad'
        assert node.is_char_device(), 'Virtual gamepad node disappeared'
        child.stdin.close()
        assert child.wait(timeout=5) == 0, 'Control pipe EOF did not cleanly stop the helper'
        assert not node.exists(), 'Virtual gamepad survived control pipe EOF'
        print('PASS: worker exit preserves the gamepad; session pipe close removes it')
    finally:
        if child is not None and child.poll() is None:
            child.terminate()
            try:
                child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                child.kill()
                child.wait()


if __name__ == '__main__':
    if len(sys.argv) != 3:
        raise SystemExit(__doc__)
    check(*sys.argv[1:])
