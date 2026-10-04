#!/usr/bin/python3
"""Small persistent backend; stdout is reserved for Framely protocol messages."""
import json
import os
from pathlib import Path
from framely import serve


def data_file():
    return Path(os.environ['FRAMELY_DATA_DIR']) / 'settings.json'


def initialize(context):
    data_file().parent.mkdir(parents=True, exist_ok=True)
    return {'ready': True}


def cleanup(context):
    # Release external resources here; keep user data across updates/uninstall.
    return {'cleaned': True}


def dispatch(method, params):
    path = data_file()
    if method == 'settings.get':
        return json.loads(path.read_text()) if path.exists() else {'text': ''}
    if method == 'settings.set':
        text = params.get('text', '')
        if not isinstance(text, str) or len(text.encode()) > 4096:
            raise ValueError('Text must be a string of at most 4096 bytes')
        result = {'text': text}
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix('.json.tmp')
        temporary.write_text(json.dumps(result, ensure_ascii=False))
        temporary.replace(path)
        return result
    if method == 'notification.action':
        return {'handled': True}
    raise ValueError('Unknown method: ' + method)


serve(dispatch, {'onInstall': initialize, 'onUpdate': initialize,
                 'onStart': initialize, 'onStop': cleanup, 'onUninstall': cleanup})
