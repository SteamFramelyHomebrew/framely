#!/usr/bin/env python3
"""Collect a bounded diagnostic ZIP without requiring a running Framely service."""
import argparse
import base64
import datetime
import io
import json
import os
import pathlib
import platform
import pwd
import re
import subprocess
import zipfile

MAX_FILE = 256 * 1024
MAX_TOTAL = 8 * 1024 * 1024
SECRET = re.compile(r'(?i)((?:password|passwd|token|secret|authorization|api[_-]?key)[\"\']?\s*[=:]\s*)(?:"[^"]*"|\'[^\']*\'|[^\s,;]+)')
URL_CREDENTIALS = re.compile(r'(https?://)[^/\s:@]+:[^/\s@]+@')

def redact(text):
    text = re.sub(r'(?i)\bBearer\s+[A-Za-z0-9._~+/=-]+', 'Bearer [redacted]', text)
    text = SECRET.sub(r'\1[redacted]', text)
    text = re.sub(r'(/boot/)[a-f0-9]{32,}', r'\1[redacted]', text)
    return URL_CREDENTIALS.sub(r'\1[redacted]@', text)

def collect(state, commands=True, runtime=pathlib.Path('/run/user'), installer_log=None, steam_home=None):
    report = {'format': 1, 'createdAt': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'warnings': []}
    entries = {}
    budget = MAX_TOTAL
    def add(name, data):
        nonlocal budget
        data = redact(data).encode('utf-8', errors='replace')
        if len(data) > MAX_FILE:
            report['warnings'].append(name + ': truncated to recent output')
            data = data[-MAX_FILE:]
        if len(data) > budget:
            report['warnings'].append(name + ': omitted (archive budget)')
            return
        entries[name] = data
        budget -= len(data)
    def tail(path, name):
        try:
            if path.is_symlink() or not path.is_file():
                return
            with path.open('rb') as source:
                size = os.fstat(source.fileno()).st_size
                if size > MAX_FILE:
                    report['warnings'].append(name + ': truncated to recent output')
                source.seek(max(0, size - MAX_FILE))
                add(name, source.read(MAX_FILE).decode('utf-8', errors='replace'))
        except (OSError, ValueError) as error:
            report['warnings'].append(name + ': ' + str(error))
    def command(name, args):
        try:
            # Temporary files avoid buffering unbounded journal output in memory.
            import tempfile
            with tempfile.TemporaryFile() as output:
                try:
                    result = subprocess.run(args, stdout=output, stderr=subprocess.STDOUT, timeout=8, check=False)
                    code = result.returncode
                except subprocess.TimeoutExpired:
                    code = None
                    report['warnings'].append(name + ': command timed out; partial output included')
                output.seek(0, 2)
                size = output.tell()
                if size > MAX_FILE:
                    report['warnings'].append(name + ': truncated to recent output')
                output.seek(max(0, size - MAX_FILE))
                text = output.read(MAX_FILE).decode('utf-8', errors='replace')
                add(name, text)
                if code:
                    report['warnings'].append(name + ': command exited ' + str(code))
                return text if code == 0 else None
        except (OSError, subprocess.TimeoutExpired) as error:
            report['warnings'].append(name + ': ' + str(error))
    if installer_log is not None:
        add('installer.log', installer_log)
    add('system.json', json.dumps({'system': platform.system(), 'release': platform.release(), 'machine': platform.machine()}, indent=2))
    for name in ('current', 'previous'):
        tail(state / name / 'VERSION', 'versions/' + name + '.txt')
    tail(state / 'previous-release', 'versions/previous-release.txt')
    # Only a summary is included; plugin configuration, proxy credentials,
    # inbox contents and authentication files must never enter the archive.
    try:
        database = json.loads((state / 'state.json').read_text())
        plugins = database.get('plugins', {})
        add('plugins.json', json.dumps({'safeMode': database.get('safeMode'), 'plugins': [
            {'id': key, 'name': value.get('manifest', {}).get('name'), 'version': value.get('manifest', {}).get('version'), 'enabled': value.get('enabled'), 'error': value.get('error')}
            for key, value in plugins.items()
        ]}, ensure_ascii=False, indent=2))
    except (OSError, ValueError, TypeError, AttributeError) as error:
        report['warnings'].append('plugins.json: ' + str(error))
    # APK management runs as the configured Steam user, not the root daemon.
    # Resolve that user explicitly: root's HOME and rootful Podman are wrong.
    steam_user = None
    try:
        if steam_home is None:
            user_file = state / 'steam-user'
            if user_file.is_file() and not user_file.is_symlink():
                name = user_file.read_text()[:256].strip()
                steam_user = pwd.getpwnam(name)
            elif os.geteuid() != 0:
                steam_user = pwd.getpwuid(os.geteuid())
            if steam_user is not None and steam_user.pw_uid >= 1000:
                steam_home = pathlib.Path(steam_user.pw_dir)
        if steam_home is None:
            report['warnings'].append('apk: Steam session user unavailable; APK logs not collected')
        else:
            steam_home = pathlib.Path(steam_home)
            apk = steam_home / '.local/share/framely/apk-manager'
            try:
                with (apk / 'state.json').open('rb') as source:
                    raw = source.read(16 * 1024 * 1024 + 1)
                if len(raw) > 16 * 1024 * 1024:
                    raise ValueError('APK state exceeds diagnostic size limit')
                database = json.loads(raw)
                records = database.get('records', {})
                # Never include tokens, bindings, icons or the complete state.
                summary = {'apps': [
                    {'id': key, 'context': item.get('context'), 'removed': item.get('removed'),
                     'pending': item.get('pending'), 'steamAppId': item.get('steamAppId'),
                     'steamLaunch': item.get('steamLaunch'),
                     'package': item.get('metadata', {}).get('package'),
                     'name': item.get('metadata', {}).get('name'),
                     'version': item.get('metadata', {}).get('version')}
                    for key, item in records.items()
                ], 'gamepadEnabled': database.get('gamepad_enabled'),
                   'gamepadSource': database.get('gamepad_source')}
                add('apk/summary.json', json.dumps(summary, ensure_ascii=False, indent=2))
            except (OSError, ValueError, TypeError, AttributeError) as error:
                report['warnings'].append('apk/summary.json: ' + str(error))

            def recent_logs(directory, pattern, limit, prefix):
                try:
                    if directory.is_symlink():
                        raise ValueError('Diagnostic log directory must not be a symlink')
                    if not directory.is_dir():
                        report['warnings'].append(prefix + ': log directory unavailable')
                        return
                    paths = [p for p in directory.glob(pattern)
                             if not p.is_symlink() and not p.parent.is_symlink() and p.is_file()]
                    paths.sort(key=lambda p: p.stat().st_mtime, reverse=True)
                    for path in paths[:limit]:
                        tail(path, prefix + '/' + path.relative_to(directory).as_posix())
                    if len(paths) > limit:
                        report['warnings'].append(prefix + ': only the ' + str(limit) + ' newest logs included')
                except (OSError, ValueError) as error:
                    report['warnings'].append(prefix + ': ' + str(error))

            recent_logs(apk / 'logs', '*.log', 12, 'apk/operations')
            steam = steam_home / '.local/share/Steam'
            if not steam.is_dir():
                steam = steam_home / '.steam/steam'
            # Only known diagnostic files, never media, APKs, saves or tombstones.
            recent_logs(steam / 'logs/lepton-logcats', '*/logcat-*.log', 12, 'apk/android-saved')
            if commands and steam_user is not None:
                prefix = []
                if os.geteuid() == 0:
                    prefix = ['runuser', '-u', steam_user.pw_name, '--']
                prefix += ['env', 'HOME=' + str(steam_home),
                           'XDG_RUNTIME_DIR=/run/user/' + str(steam_user.pw_uid), 'podman']
                listing = command('apk/containers.txt', prefix + ['ps', '-a', '--filter', 'name=^lepton-', '--format', '{{.Names}}\t{{.Status}}'])
                if listing:
                    active = []
                    for line in listing.splitlines():
                        name, _, status = line.partition('\t')
                        if re.fullmatch(r'lepton-[A-Za-z0-9_.-]+', name) and status.startswith('Up '):
                            active.append(name)
                    for name in active[:4]:
                        # A single read-only exec per active container bounds export
                        # latency and works even when PackageManager is not ready.
                        probe = ('echo "=== boot ==="; getprop sys.boot_completed; '
                                 'echo "=== user ==="; am get-started-user-state 0; '
                                 'echo "=== storage ==="; ls -ld /storage/emulated/0; dumpsys mount; '
                                 'echo "=== package manager ==="; pm path android; '
                                 'echo "=== recent Android logs ==="; logcat -d -b main -b system -b crash -t 500')
                        command('apk/android-live/' + name + '.log', prefix + ['exec', name, 'sh', '-c', probe])
                    if len(active) > 4:
                        report['warnings'].append('apk/android-live: only four active containers queried')
    except (OSError, ValueError, KeyError) as error:
        report['warnings'].append('apk: ' + str(error))

    try:
        for path in sorted((state / 'logs').glob('*.log'))[:64]:
            tail(path, 'logs/' + path.name)
        for path in sorted(runtime.glob('*/framely-session-*/cef.log'))[-8:]:
            tail(path, 'renderer/' + path.parent.parent.name + '-' + path.parent.name + '-cef.log')
    except OSError as error:
        report['warnings'].append('logs: ' + str(error))
    # Record GPU evidence without copying command lines or authentication tokens.
    gpu = []
    if commands:
        for process in pathlib.Path('/proc').glob('[0-9]*'):
            try:
                if pathlib.Path(os.readlink(process / 'exe')).name != 'framely-vr':
                    continue
                arguments = (process / 'cmdline').read_bytes()
                match = re.search(rb'--type=([a-z-]+)', arguments)
                kind = match.group(1).decode() if match else 'browser'
                info = {'pid': int(process.name), 'type': kind, 'gpuDisabled': b'--disable-gpu' in arguments.replace(b'\0', b' ').split()}
                if kind == 'gpu-process':
                    maps = (process / 'maps').read_text(errors='replace')
                    info['drivers'] = sorted(set(line.split()[-1] for line in maps.splitlines() if any(word in line.lower() for word in ('freedreno', 'adreno', 'swiftshader', 'llvmpipe', 'vulkan', 'libegl', 'libgles'))))
                    devices = []
                    for fd in (process / 'fd').iterdir():
                        try:
                            target = os.readlink(fd)
                            if target.startswith('/dev/dri/') or target == '/dev/kgsl-3d0':
                                devices.append(target)
                        except OSError:
                            pass
                    info['devices'] = sorted(set(devices))
                gpu.append(info)
            except (OSError, ValueError):
                continue
        add('renderer/gpu.json', json.dumps(gpu, indent=2))
    if commands:
        command('services.txt', ['systemctl', 'status', 'framely.service', 'framely-session.service', '--no-pager', '--full'])
        command('journal.txt', ['journalctl', '-u', 'framely.service', '-u', 'framely-session.service', '-u', 'framely-plugin-*', '-u', 'framely-backend-*', '-u', 'framely-hook-*', '--since=-24h', '-n', '3000', '--no-pager', '-o', 'short-iso'])
    add('README.txt', 'Framely diagnostics. Recent Framely and APK operation logs, saved/active Lepton Android log tails, boot/unlock/storage diagnostics, service state and installed version summary only. Application files, media, APK archives and raw memory dumps are excluded. Missing sources are listed in report.json. Logs may contain application data; inspect before sharing.\n')
    entries['report.json'] = json.dumps(report, ensure_ascii=False, indent=2).encode()
    memory = io.BytesIO()
    with zipfile.ZipFile(memory, 'w', zipfile.ZIP_DEFLATED) as archive:
        for name, data in entries.items():
            archive.writestr(name, data)
    name = 'framely-logs-' + datetime.datetime.now(datetime.timezone.utc).strftime('%Y%m%d-%H%M%S') + '.zip'
    return {'name': name, 'data': base64.b64encode(memory.getvalue()).decode('ascii')}

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--state', type=pathlib.Path)
    parser.add_argument('--output', type=pathlib.Path)
    parser.add_argument('--installer-log-base64')
    args = parser.parse_args()
    state = args.state or (pathlib.Path('/home/.framely/state') if pathlib.Path('/home/.framely/state').is_dir() else pathlib.Path('/var/lib/framely'))
    extra = base64.b64decode(args.installer_log_base64).decode('utf-8', errors='replace') if args.installer_log_base64 else None
    result = collect(state, installer_log=extra)
    if args.output:
        with args.output.open('xb') as output:
            os.chmod(args.output, 0o600)
            output.write(base64.b64decode(result['data']))
        print(args.output)
    else:
        print(json.dumps(result))

if __name__ == '__main__':
    main()
