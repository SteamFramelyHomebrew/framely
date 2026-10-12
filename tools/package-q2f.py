#!/usr/bin/env python3
"""Build an independently downloadable Quest conversion runtime."""
import argparse
import gzip
import hashlib
import json
import pathlib
import shutil
import struct
import subprocess
import tarfile
import tempfile
import zipfile

CLI_VERSION = '1.2.3'
LIBRARIES_VERSION = '3.4.3-23204ea'
CLI_SHA256 = '307d58a69a5c9722f912d1b6566b0dd1e0ebe1440c6d1e56cab6cb883d74fef2'
LIBRARIES_SHA256 = '6cec6c176e1aeee8d9ec4f9b4e3982f71da7b44a36a293249620073a06f00e5e'
SOURCE_SHA256 = '2db2c0f597a8be01d9742180019627b4c23fd0213f04f5034fbad53f0d296eb3'
FRAMEBRIDGE_COMMIT = '80ef9ca6a77eed1ca6b2cc6f746d9897e4178b4e'
FRAMEBRIDGE_SOURCE_SHA256 = 'ea90ea8dd2f0fe49d54d2df5cc61c1876c9562e55fb40fce2651b1d5594dab7c'
FRAMEBRIDGE_ABIS = {'arm64-v8a': 183, 'armeabi-v7a': 40}


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


def framebridge_source(source, destination):
    """Ship the exact adapter source and its build instructions with the runtime."""
    prefix = f'frameport-{FRAMEBRIDGE_COMMIT}/'
    fixed = {'LICENSE', 'THIRD_PARTY_NOTICES.md', 'native/README.md', 'native/build.py'}
    with tarfile.open(source, 'r:gz') as upstream, destination.open('wb') as output, \
            gzip.GzipFile(filename='', mode='wb', fileobj=output, mtime=0) as compressed, \
            tarfile.open(fileobj=compressed, mode='w') as selected:
        files = [m for m in upstream.getmembers() if m.name.startswith(prefix)
                 and (m.name[len(prefix):] in fixed or m.name[len(prefix):].startswith('native/adapter/'))
                 and m.isfile()]
        if not fixed.issubset({m.name[len(prefix):] for m in files}) or len(files) > 100:
            raise ValueError('Incomplete FrameBridge corresponding source')
        for member in sorted(files, key=lambda m: m.name):
            relative = member.name[len(prefix):]
            if member.size > 2 * 1024 * 1024 or '..' in pathlib.PurePosixPath(relative).parts:
                raise ValueError('Invalid FrameBridge source entry')
            copy = tarfile.TarInfo(f'framebridge/{relative}')
            copy.size, copy.mode, copy.mtime = member.size, 0o644, 0
            selected.addfile(copy, upstream.extractfile(member))


def install_framebridge(payload, source, adapters):
    if digest(source) != FRAMEBRIDGE_SOURCE_SHA256:
        raise ValueError('Pinned FrameBridge source checksum mismatch')
    with tarfile.open(source, 'r:gz') as upstream:
        license_file = upstream.extractfile(f'frameport-{FRAMEBRIDGE_COMMIT}/LICENSE')
        if license_file is None:
            raise ValueError('FrameBridge license missing')
        (payload / 'FRAMEBRIDGE_LICENSE').write_bytes(license_file.read())
    framebridge_source(source, payload / 'FRAMEBRIDGE_SOURCE.tar.gz')
    (payload / 'FRAMEBRIDGE_SOURCE').write_text(
        f'https://github.com/spoopyghosty0/frameport/tree/{FRAMEBRIDGE_COMMIT}\n')
    for abi, machine in FRAMEBRIDGE_ABIS.items():
        adapter = pathlib.Path(adapters[abi])
        data = adapter.read_bytes()
        elf_class = 2 if abi == 'arm64-v8a' else 1
        if (len(data) < 1024 or data[:6] != b'\x7fELF' + bytes((elf_class, 1))
                or struct.unpack_from('<H', data, 18)[0] != machine):
            raise ValueError(f'FrameBridge {abi} binary has the wrong architecture')
        lib = payload / 'libraries' / LIBRARIES_VERSION / 'lib' / abi
        generic = lib / 'libopenxr_loader_generic.so'
        original = lib / 'libopenxr_loader_original.so'
        if not generic.is_file() or original.exists():
            raise ValueError(f'OVRPort {abi} generic loader cannot be wrapped')
        generic.rename(original)
        generic.write_bytes(data)
        (lib / 'libframe_settings.so').write_text('controller_fix=1\nprofile_remap=1\n')


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
        verifier = pathlib.Path(__file__).with_name('FramelyQ2FVerify.java')
        subprocess.run(['javac', '-cp', str(cli), '-d', str(payload), str(verifier)], check=True)
        shutil.copy2(verifier, payload / verifier.name)
        safe_zip_extract(libraries, payload / 'libraries' / LIBRARIES_VERSION)
        install_framebridge(payload, pathlib.Path(args.framebridge_source), {
            'arm64-v8a': args.framebridge_arm64, 'armeabi-v7a': args.framebridge_arm32})
        (payload / 'overport-installed.json').write_text(json.dumps({'releases': [
            {'version': LIBRARIES_VERSION, 'app_required': '1.2.0', 'is_experimental': True}
        ]}) + '\n')
        shutil.copy2(args.license, payload / 'OVR_PORT_LICENSE')
        shutil.copy2(source, payload / 'OVR_PORT_SOURCE.tar.gz')
        (payload / 'SOURCE').write_text('https://github.com/ovrport/app/tree/c790854\n')
        entries = sorted(p for p in payload.rglob('*') if p.is_file())
        checksums = ''.join(f'{digest(p)}  {p.relative_to(payload).as_posix()}\n' for p in entries)
        identity = f'overport-{CLI_VERSION}-{LIBRARIES_VERSION}-framebridge-{FRAMEBRIDGE_COMMIT[:8]}-{hashlib.sha256(checksums.encode()).hexdigest()[:12]}'
        (payload / 'VERSION').write_text(identity + '\n')
        (payload / 'SHA256SUMS').write_text(checksums)
        archive = output / f'framely-q2f-{identity}-linux-arm64.tar.gz'
        with tarfile.open(archive, 'w:gz') as tar:
            tar.add(payload, arcname=f'framely-q2f-{identity}')
        requirement = {'schemaVersion': 1, 'id': identity,
                       'cliVersion': CLI_VERSION, 'librariesVersion': LIBRARIES_VERSION,
                       'framebridgeCommit': FRAMEBRIDGE_COMMIT,
                       'archive': archive.name, 'size': archive.stat().st_size,
                       'sha256': digest(archive),
                       'url': f'https://github.com/{args.repo}/releases/download/{args.tag}/{archive.name}'}
        pathlib.Path(args.stage, 'Q2F_RUNTIME.json').write_text(json.dumps(requirement, indent=2) + '\n')
        pathlib.Path(output, 'framely-q2f.json').write_text(json.dumps(requirement, indent=2) + '\n')
    print(archive)


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    for name in ('stage', 'output', 'java-home', 'cli-jar', 'libraries-zip', 'license', 'source-archive',
                 'framebridge-source', 'framebridge-arm64', 'framebridge-arm32', 'repo', 'tag'):
        parser.add_argument('--' + name, required=True)
    package(parser.parse_args())
