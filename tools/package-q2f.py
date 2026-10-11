#!/usr/bin/env python3
"""Build an independently downloadable Quest conversion runtime."""
import argparse
import hashlib
import json
import pathlib
import shutil
import tarfile
import tempfile
import zipfile

CLI_VERSION = '1.2.3'
LIBRARIES_VERSION = '3.4.3-23204ea'
CLI_SHA256 = '307d58a69a5c9722f912d1b6566b0dd1e0ebe1440c6d1e56cab6cb883d74fef2'
LIBRARIES_SHA256 = '6cec6c176e1aeee8d9ec4f9b4e3982f71da7b44a36a293249620073a06f00e5e'
SOURCE_SHA256 = '2db2c0f597a8be01d9742180019627b4c23fd0213f04f5034fbad53f0d296eb3'


def digest(path):
    with path.open('rb') as source:
        return hashlib.file_digest(source, 'sha256').hexdigest()


def safe_zip_extract(source, destination):
    with zipfile.ZipFile(source) as archive:
        for entry in archive.infolist():
            path = pathlib.PurePosixPath(entry.filename)
            if (path.is_absolute() or '..' in path.parts or '\\' in entry.filename
                    or (entry.external_attr >> 16) & 0o170000 == 0o120000):
                raise ValueError('Unsafe OVR Port library entry')
            target = destination.joinpath(*path.parts)
            if entry.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with archive.open(entry) as data, target.open('xb') as output:
                    shutil.copyfileobj(data, output)


def package(args):
    cli, libraries, java, source = map(pathlib.Path, (args.cli_jar, args.libraries_zip, args.java_home, args.source_archive))
    if digest(cli) != CLI_SHA256 or digest(libraries) != LIBRARIES_SHA256 or digest(source) != SOURCE_SHA256:
        raise ValueError('Pinned OVR Port artifact checksum mismatch')
    if not (java / 'bin/java').is_file() or not (java / 'legal/java.base/LICENSE').is_file():
        raise ValueError('Incomplete Java runtime')
    output = pathlib.Path(args.output)
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='q2f-package-', dir=output) as scratch:
        payload = pathlib.Path(scratch) / 'payload'
        shutil.copytree(java, payload / 'java', symlinks=False)
        shutil.copy2(cli, payload / 'overportcli.jar')
        safe_zip_extract(libraries, payload / 'libraries' / LIBRARIES_VERSION)
        (payload / 'overport-installed.json').write_text(json.dumps({'releases': [
            {'version': LIBRARIES_VERSION, 'app_required': '1.2.0', 'is_experimental': True}
        ]}) + '\n')
        shutil.copy2(args.license, payload / 'OVR_PORT_LICENSE')
        shutil.copy2(source, payload / 'OVR_PORT_SOURCE.tar.gz')
        (payload / 'SOURCE').write_text('https://github.com/ovrport/app/tree/c790854\n')
        entries = sorted(p for p in payload.rglob('*') if p.is_file())
        checksums = ''.join(f'{digest(p)}  {p.relative_to(payload).as_posix()}\n' for p in entries)
        identity = f'overport-{CLI_VERSION}-{LIBRARIES_VERSION}-{hashlib.sha256(checksums.encode()).hexdigest()[:12]}'
        (payload / 'VERSION').write_text(identity + '\n')
        (payload / 'SHA256SUMS').write_text(checksums)
        archive = output / f'framely-q2f-{identity}-linux-arm64.tar.gz'
        with tarfile.open(archive, 'w:gz') as tar:
            tar.add(payload, arcname=f'framely-q2f-{identity}')
        requirement = {'schemaVersion': 1, 'id': identity,
                       'cliVersion': CLI_VERSION, 'librariesVersion': LIBRARIES_VERSION,
                       'archive': archive.name, 'size': archive.stat().st_size,
                       'sha256': digest(archive),
                       'url': f'https://github.com/{args.repo}/releases/download/{args.tag}/{archive.name}'}
        pathlib.Path(args.stage, 'Q2F_RUNTIME.json').write_text(json.dumps(requirement, indent=2) + '\n')
        pathlib.Path(output, 'framely-q2f.json').write_text(json.dumps(requirement, indent=2) + '\n')
    print(archive)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    for name in ('stage', 'output', 'java-home', 'cli-jar', 'libraries-zip', 'license', 'source-archive', 'repo', 'tag'):
        parser.add_argument('--' + name, required=True)
    package(parser.parse_args())
