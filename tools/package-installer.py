#!/usr/bin/env python3
"""Package the native installer; no external packaging tools needed."""
import os
import pathlib
import plistlib
import shutil
import tarfile
import tomllib
import zipfile

root = pathlib.Path(__file__).resolve().parents[1]
platform = os.environ['INSTALLER_PLATFORM']
version = tomllib.loads((root / 'installer/Cargo.toml').read_text())['package']['version']
exe = 'framely-installer.exe' if platform.startswith('windows') else 'framely-installer'
target = pathlib.Path(os.environ.get('CARGO_TARGET_DIR', str(root / 'installer/target')))
binary = target / os.environ.get('INSTALLER_BUILD_PROFILE', 'release') / exe
out = root / 'installer/dist'
out.mkdir(parents=True, exist_ok=True)
with __import__('tempfile').TemporaryDirectory() as temp:
    stage = pathlib.Path(temp)
    if platform.startswith('macos'):
        bundle = stage / 'Framely Installer.app/Contents'
        (bundle / 'MacOS').mkdir(parents=True)
        shutil.copy2(binary, bundle / 'MacOS/framely-installer')
        (bundle / 'MacOS/framely-installer').chmod(0o755)
        with (bundle / 'Info.plist').open('wb') as output:
            plistlib.dump({'CFBundleName': 'Framely Installer', 'CFBundleDisplayName': 'Framely Installer', 'CFBundleIdentifier': 'org.framely.installer', 'CFBundleExecutable': 'framely-installer', 'CFBundlePackageType': 'APPL', 'CFBundleVersion': version, 'CFBundleShortVersionString': version, 'NSHighResolutionCapable': True, 'LSMinimumSystemVersion': '11.0'}, output)
    else:
        shutil.copy2(binary, stage / exe)
        if not platform.startswith('windows'):
            (stage / exe).chmod(0o755)
    shutil.copy2(root / 'installer/README.md', stage / 'README.md')
    shutil.copy2(root / 'LICENSE', stage / 'LICENSE')
    name = f'framely-installer-{version}-{platform}'
    if platform.startswith('linux'):
        with tarfile.open(out / (name + '.tar.gz'), 'w:gz') as archive:
            for path in stage.iterdir():
                archive.add(path, arcname=name + '/' + path.name)
    else:
        with zipfile.ZipFile(out / (name + '.zip'), 'w', zipfile.ZIP_DEFLATED) as archive:
            for path in stage.rglob('*'):
                archive.write(path, path.relative_to(stage))
