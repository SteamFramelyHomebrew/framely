#!/usr/bin/env python3
"""Produce core, offline and immutable CEF archives from a built release."""
import hashlib
import json
import pathlib
import re
import shutil
import sys
import tarfile
import tempfile


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def checksums(root):
    return ''.join(f'{digest(p)}  {p.relative_to(root).as_posix()}\n'
                   for p in sorted(root.rglob('*')) if p.is_file() and p.name != 'SHA256SUMS')


def archive(root, output, prefix=None):
    with tarfile.open(output, 'w:gz') as tar:
        tar.add(root, arcname=prefix or root.name)
    output.with_name(output.name + '.sha256').write_text(f'{digest(output)}  {output.name}\n')


def package(stage, cef, repo, tag):
    stage, cef = pathlib.Path(stage), pathlib.Path(cef)
    output = stage.parent
    version = re.search(r'^#define CEF_VERSION "([^"]+)"',
                        (cef / 'include/cef_version.h').read_text(), re.M)[1]
    with tempfile.TemporaryDirectory(prefix='.cef-package-', dir=output) as work:
        runtime = pathlib.Path(work) / 'runtime'
        (runtime / 'lib/cef').mkdir(parents=True)
        (runtime / 'share/licenses').mkdir(parents=True)
        for source in (stage / 'lib/cef').iterdir():
            if source.name != 'framely-vr':
                target = runtime / 'lib/cef' / source.name
                if source.is_dir():
                    shutil.copytree(source, target)
                else:
                    shutil.copy2(source, target)
        for source in (stage / 'share/licenses').glob('cef*'):
            shutil.copy2(source, runtime / 'share/licenses' / source.name)
        payload_sums = checksums(runtime)
        runtime_id = version.split('+chromium')[0].replace('+', '-') + '-' + hashlib.sha256(payload_sums.encode()).hexdigest()[:12]
        (runtime / 'VERSION').write_text(runtime_id + '\n')
        (runtime / 'SHA256SUMS').write_text(checksums(runtime))
        cef_archive = output / f'framely-cef-{runtime_id}-linux-arm64.tar.gz'
        archive(runtime, cef_archive, 'framely-cef-' + runtime_id)
        requirement = dict(schemaVersion=1, id=runtime_id, version=version,
                           archive=cef_archive.name, size=cef_archive.stat().st_size,
                           sha256=digest(cef_archive),
                           url=f'https://github.com/{repo}/releases/download/{tag}/{cef_archive.name}')
        (stage / 'CEF_RUNTIME.json').write_text(json.dumps(requirement, indent=2) + '\n')
        (stage / 'CEF_SHA256SUMS').write_text(payload_sums)
        (output / 'framely-cef.json').write_text(json.dumps(requirement, indent=2) + '\n')
        (stage / 'SHA256SUMS').write_text(checksums(stage))
        archive(stage, output / f'{stage.name}-offline-linux-arm64.tar.gz')
        core = pathlib.Path(work) / 'core'
        shutil.copytree(stage, core, ignore=lambda directory, names:
                        [name for name in names if pathlib.Path(directory) == stage / 'lib/cef' and name != 'framely-vr'
                         or pathlib.Path(directory) == stage / 'share/licenses' and name.startswith('cef')])
        (core / 'SHA256SUMS').write_text(checksums(core))
        archive(core, output / f'{stage.name}-linux-arm64.tar.gz', stage.name)
    outputs = [output / f'{stage.name}-linux-arm64.tar.gz', output / f'{stage.name}-offline-linux-arm64.tar.gz', cef_archive, output / 'framely-cef.json']
    (output / 'SHA256SUMS').write_text(''.join(f'{digest(path)}  {path.name}\n' for path in outputs))
    print(f'Core: {stage.name}-linux-arm64.tar.gz')
    print(f'Offline: {stage.name}-offline-linux-arm64.tar.gz')
    print(f'CEF: {cef_archive.name}')


if __name__ == '__main__':
    package(*sys.argv[1:])
