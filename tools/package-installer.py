#!/usr/bin/env python3
"""Package the native installer, including platform application icons."""
import os
import pathlib
import plistlib
import shutil
import subprocess
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
        (bundle / 'Resources').mkdir()
        iconset = pathlib.Path(temp) / 'Framely.iconset'
        iconset.mkdir()
        for size in (16, 32, 128, 256, 512):
            for scale in (1, 2):
                suffix = '@2x' if scale == 2 else ''
                shutil.copy2(root / f'assets/branding/icons/{size * scale}.png', iconset / f'icon_{size}x{size}{suffix}.png')
        subprocess.run(['iconutil', '-c', 'icns', '-o', str(bundle / 'Resources/Framely.icns'), str(iconset)], check=True)
        shutil.rmtree(iconset)
        shutil.copy2(binary, bundle / 'MacOS/framely-installer')
        (bundle / 'MacOS/framely-installer').chmod(0o755)
        with (bundle / 'Info.plist').open('wb') as output:
            plistlib.dump({'CFBundleName': 'Framely Installer', 'CFBundleDisplayName': 'Framely Installer', 'CFBundleIdentifier': 'org.framely.installer', 'CFBundleExecutable': 'framely-installer', 'CFBundleIconFile': 'Framely.icns', 'CFBundlePackageType': 'APPL', 'CFBundleVersion': version, 'CFBundleShortVersionString': version, 'NSHighResolutionCapable': True, 'LSMinimumSystemVersion': '11.0'}, output)
    else:
        shutil.copy2(binary, stage / exe)
        if not platform.startswith('windows'):
            (stage / exe).chmod(0o755)
            desktop_script = (root / 'installer/install-desktop-entry.sh').read_text()
            marker = 'icon = None  # Embedded by tools/package-installer.py.'
            if desktop_script.count(marker) != 1:
                raise ValueError('Desktop registration script is missing its icon marker')
            icon = (root / 'assets/branding/framely-app-icon.svg').read_bytes()
            (stage / 'install-desktop-entry.sh').write_text(desktop_script.replace(marker, f'icon = {icon!r}'))
            (stage / 'install-desktop-entry.sh').chmod(0o755)
    shutil.copy2(root / 'installer/README.md', stage / 'README.md')
    shutil.copy2(root / 'installer/README.zh-CN.md', stage / 'README.zh-CN.md')
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
