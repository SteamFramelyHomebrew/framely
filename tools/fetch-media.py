#!/usr/bin/env python3
"""Fetch immutable, checksum-verified upstream media dependencies."""
import hashlib,io,json,tarfile,urllib.request
from pathlib import Path
base=Path(__file__).resolve().parent.parent
pins=json.loads((base/'media/dependencies.json').read_text())
for name,pin in pins.items():
    payload=urllib.request.urlopen(pin['url'],timeout=120).read()
    if hashlib.sha256(payload).hexdigest()!=pin['sha256']:
        raise SystemExit(f'{name}: upstream archive checksum mismatch')
    target=base/'media'/('bin' if name=='mediamtx' else 'source')
    target.mkdir(exist_ok=True)
    with tarfile.open(fileobj=io.BytesIO(payload),mode='r:*') as archive:
        archive.extractall(target,filter='data')
    if name=='mediamtx':(target/'mediamtx').chmod(0o755)
