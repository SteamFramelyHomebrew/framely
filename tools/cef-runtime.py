#!/usr/bin/env python3
"""Verify, cache and attach the exact CEF runtime required by a release."""
import hashlib
import importlib.util
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tempfile


def verify(root, checksums):
    for line in checksums.splitlines():
        match = re.fullmatch(r'([a-f0-9]{64})  (.+)', line)
        if not match:
            raise ValueError('Invalid CEF checksums')
        path = pathlib.PurePosixPath(match[2])
        if path.is_absolute() or '..' in path.parts or '\\' in match[2] or not (
                match[2].startswith('lib/cef/') or match[2] in ('share/licenses/cef.txt', 'share/licenses/cef-credits.html')):
            raise ValueError('Invalid CEF payload path')
        with (root / path).open('rb') as source:
            if hashlib.file_digest(source, 'sha256').hexdigest() != match[1]:
                raise ValueError('CEF checksum mismatch: ' + match[2])
    if not any(line.endswith('  lib/cef/libcef.so') for line in checksums.splitlines()):
        raise ValueError('CEF library checksum is missing')


def prepare(base, store):
    base, store = pathlib.Path(base), pathlib.Path(store)
    requirement = json.loads((base / 'CEF_RUNTIME.json').read_text())
    runtime_id = requirement['id']
    if requirement['schemaVersion'] != 1 or not re.fullmatch(r'[a-zA-Z0-9.+-]+', runtime_id) or '..' in runtime_id:
        raise ValueError('Invalid CEF runtime identity')
    expected = (base / 'CEF_SHA256SUMS').read_text()
    parent = store / 'cef'
    if parent.is_symlink():
        raise ValueError('CEF storage must not be a symlink')
    parent.mkdir(mode=0o755, exist_ok=True)
    cached = parent / runtime_id
    if cached.exists() or cached.is_symlink():
        if cached.is_symlink() or not cached.is_dir():
            raise ValueError('Invalid CEF cache directory')
        verify(cached, expected)
        return cached
    with tempfile.TemporaryDirectory(prefix='.install-', dir=parent) as work:
        work = pathlib.Path(work)
        payload = work / 'payload'
        # Older full installations can seed the cache without a network request.
        for candidate in (base, store / 'state/current'):
            try:
                verify(candidate, expected)
            except (OSError, ValueError):
                continue
            payload.mkdir()
            for line in expected.splitlines():
                relative = line[66:]
                target = payload / relative
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(candidate / relative, target)
                target.chmod(0o755 if relative.startswith('lib/cef/') and (candidate / relative).stat().st_mode & 0o111 else 0o644)
            break
        else:
            spec = importlib.util.spec_from_file_location('bootstrap', base / 'tools/bootstrap.py')
            engine = importlib.util.module_from_spec(spec)
            spec.loader.exec_module(engine)
            archive_name = f'framely-cef-{runtime_id}-linux-arm64.tar.gz'
            if requirement['archive'] != archive_name:
                raise ValueError('Invalid CEF archive name')
            archive = work / archive_name
            print('Downloading CEF ' + requirement['version'], file=sys.stderr, flush=True)
            def progress(completed):
                if os.environ.get('FRAMELY_PROGRESS_JSON') == '1':
                    print('FRAMELY_PROGRESS ' + json.dumps(dict(step='cef-download', completed=completed, total=requirement['size'])), file=sys.stderr, flush=True)
            engine.download(requirement['url'], archive, engine.MAX_ARCHIVE, progress)
            if archive.stat().st_size != requirement['size']:
                raise ValueError('CEF archive length mismatch')
            with archive.open('rb') as source:
                if hashlib.file_digest(source, 'sha256').hexdigest() != requirement['sha256']:
                    raise ValueError('CEF archive SHA256 mismatch')
            # The same link-free extraction policy protects standalone runtimes.
            payload = engine.extract_archive(archive, work / 'unpacked', runtime=True)
            if (payload / 'VERSION').read_text().strip() != runtime_id:
                raise ValueError('CEF archive version mismatch')
            subprocess.run(['sha256sum', '--quiet', '-c', 'SHA256SUMS'], cwd=payload, check=True)
        verify(payload, expected)
        for directory, _, _ in os.walk(payload):
            pathlib.Path(directory).chmod(0o755)
        payload.rename(cached)
    return cached


def attach(release, cached):
    release, cached = pathlib.Path(release), pathlib.Path(cached)
    for folder in ('lib/cef', 'share/licenses'):
        target_dir = release / folder
        target_dir.mkdir(parents=True, exist_ok=True)
        for source in (cached / folder).iterdir():
            target = target_dir / source.name
            if target.is_symlink() or target.is_file():
                target.unlink()
            elif target.is_dir():
                shutil.rmtree(target)
            target.symlink_to(source)


if __name__ == '__main__':
    try:
        if sys.argv[1] == 'prepare':
            print(prepare(*sys.argv[2:]))
        elif sys.argv[1] == 'attach':
            attach(*sys.argv[2:])
        else:
            raise ValueError('Invalid CEF operation')
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print('Framely CEF: ' + str(error), file=sys.stderr)
        sys.exit(1)
