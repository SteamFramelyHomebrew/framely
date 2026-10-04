#!/usr/bin/env bash
# Register the extracted Linux installer with its matching application icon.
set -euo pipefail
base=$(cd -- "$(dirname -- "$0")" && pwd)
python3 - "$base" <<'PY'
import os, pathlib, shutil
base = pathlib.Path(__import__('sys').argv[1])
binary = base / 'framely-installer'
if not binary.is_file():
    raise SystemExit('Run this script from an extracted Framely Linux installer archive.')
data = pathlib.Path(os.environ.get('XDG_DATA_HOME') or pathlib.Path.home() / '.local/share')
apps = data / 'applications'
icons = data / 'icons/hicolor/scalable/apps'
apps.mkdir(parents=True, exist_ok=True)
icons.mkdir(parents=True, exist_ok=True)
shutil.copy2(base / 'assets/branding/framely-app-icon.svg', icons / 'org.framely.installer.svg')
# Desktop entries parse backslash escapes before parsing the quoted Exec value.
quoted = str(binary).replace('%', '%%').replace('\\', '\\\\')
for character in ('"', '`', '$'):
    quoted = quoted.replace(character, '\\' + character)
quoted = quoted.replace('\\', '\\\\')
entry = apps / 'org.framely.installer.desktop'
entry.write_text(f'''[Desktop Entry]
Type=Application
Name=Framely Installer
Name[zh_CN]=Framely 安装器
Comment=Install and manage Framely on Steam Frame
Comment[zh_CN]=安装和维护 Steam Frame 上的 Framely
Exec=/usr/bin/env "{quoted}"
Icon=org.framely.installer
Terminal=false
Categories=Utility;
StartupWMClass=org.framely.installer
''')
print(f'Registered Framely Installer: {entry}')
PY
