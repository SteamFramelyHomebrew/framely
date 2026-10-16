#!/usr/bin/env python3
"""Download and verify the exact optional Quest conversion runtime."""
import hashlib
import json
import os
import pathlib
import re
import shutil
import sys
import tarfile
import tempfile
import urllib.parse
import importlib.util

_spec = importlib.util.spec_from_file_location('framely_bootstrap', pathlib.Path(__file__).with_name('bootstrap.py'))
_network = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_network)

MAX_ARCHIVE = 512 * 1024 * 1024
MAX_EXPANDED = 1024 * 1024 * 1024


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def validate(root, identity):
    if root.is_symlink() or not root.is_dir() or (root / 'VERSION').read_text().strip() != identity:
        raise ValueError('Invalid q2f runtime cache')
    lines = (root / 'SHA256SUMS').read_text().splitlines()
    expected = set()
    for line in lines:
        match = re.fullmatch(r'([a-f0-9]{64})  ([^\\]+)', line)
        if not match:
            raise ValueError('Invalid q2f checksum list')
        relative = pathlib.PurePosixPath(match[2])
        if relative.is_absolute() or '..' in relative.parts or not relative.parts:
            raise ValueError('Invalid q2f payload path')
        file = root.joinpath(*relative.parts)
        if file.is_symlink() or not file.is_file() or digest(file) != match[1]:
            raise ValueError('q2f payload checksum mismatch: ' + match[2])
        expected.add(relative.as_posix())
    actual = {p.relative_to(root).as_posix() for p in root.rglob('*') if p.is_file()}
    if actual != expected | {'VERSION', 'SHA256SUMS'}:
        raise ValueError('Unexpected q2f payload files')
    for required in ('java/bin/java', 'overportcli.jar', 'overport-installed.json', 'OVR_PORT_LICENSE'):
        if required not in expected:
            raise ValueError('Incomplete q2f runtime: ' + required)
    if '-framebridge-' in identity:
        for required in ('FRAMEBRIDGE_LICENSE', 'FRAMEBRIDGE_SOURCE', 'FRAMEBRIDGE_SOURCE.tar.gz',
                         'FramelyQ2FVerify.java', 'FramelyQ2FVerify.class'):
            if required not in expected:
                raise ValueError('Incomplete q2f runtime: ' + required)
        releases = root / 'libraries'
        for abi in ('arm64-v8a', 'armeabi-v7a'):
            libs = list(releases.glob(f'*/lib/{abi}'))
            if len(libs) != 1 or not all((libs[0] / name).is_file() for name in (
                'libopenxr_loader_generic.so', 'libopenxr_loader_original.so', 'libframe_settings.so')):
                raise ValueError('Incomplete FrameBridge libraries: ' + abi)


def download(requirement, destination):
    url = requirement['url']
    if urllib.parse.urlparse(url).scheme != 'https':
        raise ValueError('q2f URL must use HTTPS')
    _network.download(url, destination, min(requirement['size'], MAX_ARCHIVE), expected_size=requirement['size'])
    total = destination.stat().st_size
    if total != requirement['size'] or digest(destination) != requirement['sha256']:
        raise ValueError('q2f archive checksum mismatch')


def extract(archive, target, identity):
    prefix = f'framely-q2f-{identity}'
    target.mkdir(mode=0o700)
    with tarfile.open(archive, 'r:gz') as source:
        members = source.getmembers()
        if len(members) > 10000 or sum(m.size for m in members) > MAX_EXPANDED:
            raise ValueError('q2f archive is too large')
        seen = set()
        for member in members:
            path = pathlib.PurePosixPath(member.name)
            if (path.is_absolute() or '..' in path.parts or '\\' in member.name
                    or not path.parts or path.parts[0] != prefix
                    or member.name in seen or not (member.isfile() or member.isdir())):
                raise ValueError('Unsafe q2f archive member')
            seen.add(member.name)
        for member in members:
            parts = pathlib.PurePosixPath(member.name).parts[1:]
            if not parts:
                continue
            path = target.joinpath(*parts)
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as data, path.open('xb') as output:
                    shutil.copyfileobj(data, output)
                path.chmod(0o755 if member.mode & 0o111 else 0o644)
    validate(target, identity)


def prepare(descriptor, storage):
    spec = json.loads(pathlib.Path(descriptor).read_text())
    identity = spec['id']
    archive_name = f'framely-q2f-{identity}-linux-arm64.tar.gz'
    if (spec['schemaVersion'] != 1 or not re.fullmatch(r'[a-zA-Z0-9.+-]+', identity)
            or '..' in identity or spec['archive'] != archive_name
            or not 0 < spec['size'] <= MAX_ARCHIVE
            or not re.fullmatch(r'[a-f0-9]{64}', spec['sha256'])):
        raise ValueError('Invalid q2f requirement')
    parent = pathlib.Path(storage) / 'q2f' / 'runtimes'
    if parent.is_symlink():
        raise ValueError('q2f storage must not be a symlink')
    parent.mkdir(parents=True, exist_ok=True)
    cached = parent / identity
    if cached.exists() or cached.is_symlink():
        validate(cached, identity)
        return cached
    with tempfile.TemporaryDirectory(prefix='.download-', dir=parent) as scratch:
        scratch = pathlib.Path(scratch)
        archive = scratch / archive_name
        download(spec, archive)
        payload = scratch / 'payload'
        extract(archive, payload, identity)
        payload.rename(cached)
    return cached


if __name__ == '__main__':
    try:
        if len(sys.argv) != 3:
            raise ValueError('Usage: q2f-runtime.py DESCRIPTOR APK_STORAGE')
        print(prepare(*sys.argv[1:]), flush=True)
    except (KeyError, ValueError, OSError, tarfile.TarError) as error:
        print('Framely q2f: ' + str(error), file=sys.stderr)
        sys.exit(1)
