#!/usr/bin/env python3
"""Shared device-side installation engine, also embedded in the desktop installer."""
import argparse
import hashlib
import json
import os
import pathlib
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.parse
import urllib.request

DEFAULT_REPO = 'SteamFramelyHomebrew/framely'
MAX_ARCHIVE = 2 * 1024**3


def emit_progress(step, completed=None, total=None):
    payload = {'step': step}
    if total is not None and total > 0:
        payload.update(completed=completed, total=total)
    print('FRAMELY_PROGRESS ' + json.dumps(payload), flush=True)


class HTTPSRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urllib.parse.urlparse(newurl).scheme != 'https':
            raise ValueError('HTTPS redirect downgrade rejected')
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def fetch(url):
    if urllib.parse.urlparse(url).scheme != 'https':
        raise ValueError('HTTPS is required')
    return urllib.request.build_opener(HTTPSRedirect()).open(
        urllib.request.Request(url, headers={'User-Agent': 'Framely-Installer', 'Accept': 'application/vnd.github+json'}), timeout=60)


def download(url, destination, limit, progress=None):
    with fetch(url) as response, open(destination, 'xb') as output:
        size = 0
        reported_at = time.monotonic()
        if progress:
            progress(0)
        while block := response.read(256 * 1024):
            size += len(block)
            if size > limit:
                raise ValueError('Download exceeds size limit')
            output.write(block)
            if progress and time.monotonic() - reported_at >= 0.2:
                progress(size)
                reported_at = time.monotonic()
        if progress:
            progress(size)


def verify_archive(archive, checksums):
    name = pathlib.Path(archive).name
    matches = []
    for line in pathlib.Path(checksums).read_text().splitlines():
        if not line.strip():
            continue
        match = re.fullmatch(r'([a-fA-F0-9]{64}) [ *](.+)', line)
        if not match:
            raise ValueError('Invalid SHA256SUMS entry')
        if match[2] == name:
            matches.append(match[1].lower())
    if len(matches) != 1:
        raise ValueError('SHA256SUMS must contain exactly one entry for ' + name)
    digest = hashlib.sha256()
    size = 0
    with open(archive, 'rb') as source:
        while block := source.read(256 * 1024):
            size += len(block)
            if size > MAX_ARCHIVE:
                raise ValueError('Archive exceeds size limit')
            digest.update(block)
    if digest.hexdigest() != matches[0]:
        raise ValueError('Archive SHA256 mismatch; installation stopped')


def extract_archive(archive, destination, progress=None, runtime=False):
    # Check all headers before writing, and never use tar's link extraction.
    with tarfile.open(archive, 'r:gz') as source:
        members = source.getmembers()
        if len(members) > 20000 or sum(m.size for m in members) > MAX_ARCHIVE:
            raise ValueError('Expanded release exceeds limits')
        seen, prefixes = set(), set()
        for member in members:
            path = pathlib.PurePosixPath(member.name)
            if (not path.parts or path.is_absolute() or '..' in path.parts
                    or '\\' in member.name or member.name in seen
                    or not (member.isdir() or member.isfile())):
                raise ValueError('Unsafe archive member: ' + member.name)
            if not re.fullmatch(r'framely-[a-zA-Z0-9.+-]+', path.parts[0]):
                raise ValueError('Invalid release directory')
            prefixes.add(path.parts[0])
            seen.add(member.name)
        if len(prefixes) != 1:
            raise ValueError('Expected one release directory')
        destination = pathlib.Path(destination)
        destination.mkdir(mode=0o700)
        total, completed, reported_at = sum(m.size for m in members), 0, time.monotonic()
        if progress:
            progress('extract', completed, total)
        for member in members:
            path = destination.joinpath(*pathlib.PurePosixPath(member.name).parts)
            if member.isdir():
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as data, path.open('xb') as output:
                    while block := data.read(256 * 1024):
                        output.write(block)
                        completed += len(block)
                        if progress and time.monotonic() - reported_at >= 0.2:
                            progress('extract', completed, total)
                            reported_at = time.monotonic()
                path.chmod(0o755 if member.mode & 0o111 else 0o644)
        base = destination / prefixes.pop()
        version = (base / 'VERSION').read_text().strip()
        if base.name != ('framely-cef-' if runtime else 'framely-') + version:
            raise ValueError('Release VERSION does not match directory')
        required_files = ['SHA256SUMS', 'lib/cef/libcef.so'] if runtime else ['SHA256SUMS', 'install.sh', 'uninstall.sh', 'bin/framely']
        for required in required_files:
            if not (base / required).is_file():
                raise ValueError('Missing release file: ' + required)
        if progress:
            progress('extract', completed, total)
        return base


def select_package(assets, action):
    packages = [name for name in assets if re.fullmatch(r'framely-[0-9][a-zA-Z0-9.+-]*-linux-arm64\.tar\.gz', name)]
    offline = [name for name in packages if name.endswith('-offline-linux-arm64.tar.gz')]
    core = [name for name in packages if name not in offline]
    selected = offline if action == 'install' and offline else core
    if action == 'install' and not offline and 'framely-cef.json' in assets:
        raise ValueError('First installation requires the complete offline package')
    if len(selected) != 1 or 'SHA256SUMS' not in assets:
        raise ValueError('Release must contain one matching ARM64 package and SHA256SUMS')
    return selected[0]


def current_release():
    state = pathlib.Path('/home/.framely/state')
    if not state.is_dir():
        state = pathlib.Path('/var/lib/framely')
    current = state / 'current'
    if not current.is_symlink():
        raise ValueError('Framely is not installed, or its installation needs repair')
    link = os.readlink(current)
    if not re.fullmatch(r'releases/[a-zA-Z0-9.+-]+', link) or '..' in link:
        raise ValueError('Invalid current release link')
    return state / link


def confirm(message):
    try:
        with open('/dev/tty', 'r+') as terminal:
            terminal.write(message + ' [y/N]: ')
            terminal.flush()
            return terminal.readline().strip().lower() == 'y'
    except OSError:
        return False


def root_operation(args):
    report = emit_progress if getattr(args, 'progress_json', False) else lambda *args: None
    import fcntl
    with open('/run/framely-installer.lock', 'a') as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise ValueError('Another Framely maintenance operation is running')
        # Validate the package before installing it.
        base, work = None, None
        try:
            if args.action in ('install', 'update'):
                if args.action == 'update':
                    current_release()
                report('verify')
                verify_archive(args.archive, args.checksums)
                report('stage')
                work = tempfile.mkdtemp(prefix='.framely-install-', dir='/home')
                staged_archive = pathlib.Path(work) / args.archive.name
                staged_checksums = pathlib.Path(work) / 'SHA256SUMS'
                shutil.copyfile(args.archive, staged_archive)
                shutil.copyfile(args.checksums, staged_checksums)
                verify_archive(staged_archive, staged_checksums)
                base = extract_archive(staged_archive, pathlib.Path(work) / 'unpacked', report)
                if args.action == 'install' and (base / 'CEF_RUNTIME.json').exists() and not (base / 'lib/cef/libcef.so').is_file():
                    raise ValueError('First installation requires the complete offline package with CEF')
                report('extract')
                subprocess.run(['sha256sum', '--quiet', '-c', 'SHA256SUMS'], cwd=base, check=True)
            else:
                base = current_release()
            report('configure')
            if args.action in ('install', 'update'):
                command = ['bash', str(base / 'install.sh')]
                try:
                    same = (base / 'VERSION').read_bytes() == (current_release() / 'VERSION').read_bytes()
                except ValueError:
                    same = False
                if same:
                    command.append('--repair')
                subprocess.run(command + [args.user], check=True,
                               env=dict(os.environ, FRAMELY_PROGRESS_JSON='1' if getattr(args, 'progress_json', False) else '0'))
                report('activate')
                # Preconfigure the official update source without requiring agreement acceptance.
                state = pathlib.Path('/home/.framely/state')
                config = state / 'update-source.json'
                if not config.exists():
                    config.write_text(json.dumps({'url': f'https://github.com/{args.repo}/releases/latest/download/framely-release.json'}))
                    config.chmod(0o600)
                    subprocess.run(['systemctl', 'restart', 'framely.service'], check=True)
                    subprocess.run(['systemctl', 'start', '--no-block', 'framely-session.service'], check=True)
            elif args.action == 'uninstall':
                if not pathlib.Path('/var/lib/framely/current').exists():
                    subprocess.run(['bash', '/home/.framely/repair.sh', args.user], check=True)
                # Copy out of the release which uninstall deletes.
                work = tempfile.mkdtemp(prefix='.framely-uninstall-', dir='/home')
                script = pathlib.Path(work) / 'uninstall.sh'
                shutil.copyfile(base / 'uninstall.sh', script)
                subprocess.run(['bash', str(script)], check=True)
            elif args.action == 'repair':
                subprocess.run(['bash', '/home/.framely/repair.sh', args.user], check=True)
            elif args.action == 'rollback':
                subprocess.run(['bash', str(base / 'rollback.sh')], check=True)
            report('activate')
            print('Framely operation completed.', flush=True)
        finally:
            if work:
                shutil.rmtree(work)


def main(argv=None):
    parser = argparse.ArgumentParser(description='Install, update, repair, rollback or uninstall Framely')
    parser.add_argument('action', nargs='?', choices=['install', 'update', 'repair', 'rollback', 'uninstall'])
    parser.add_argument('--repo', default=DEFAULT_REPO)
    parser.add_argument('--version', help='GitHub release tag; default: latest stable')
    parser.add_argument('--archive', type=pathlib.Path)
    parser.add_argument('--checksums', type=pathlib.Path, help='External SHA256SUMS for the archive')
    parser.add_argument('--user', default=os.environ.get('SUDO_USER', 'steamos'))
    parser.add_argument('--yes', action='store_true', help='Confirm the selected operation')
    parser.add_argument('--progress-json', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    if not re.fullmatch(r'[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+', args.repo):
        parser.error('Invalid repository')
    if not re.fullmatch(r'[a-z_][a-zA-Z0-9_-]*', args.user) or args.user == 'root':
        parser.error('Invalid Steam user')
    if sys.platform != 'linux' or os.uname().machine != 'aarch64':
        parser.error('This installation engine requires Linux ARM64 Steam Frame')
    if args.action is None:
        with open('/dev/tty', 'r+') as terminal:
            terminal.write('1 Install  2 Update  3 Repair  4 Rollback  5 Uninstall\nChoose [1-5]: ')
            terminal.flush()
            choices = {'1':'install', '2':'update', '3':'repair', '4':'rollback', '5':'uninstall'}
            args.action = choices.get(terminal.readline().strip())
        if args.action is None:
            parser.error('Invalid action')
    if not args.yes and not confirm(f'{args.action}: ' + ('disable and uninstall ALL plugins, then remove Framely (saved data retained)' if args.action == 'uninstall' else 'continue with Framely maintenance')):
        raise ValueError('Cancelled')
    cache = pathlib.Path.home() / '.cache' / 'framely-installer' if args.action in ('install', 'update') and not args.archive else None
    if cache is not None:
        cache.mkdir(parents=True, mode=0o700, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='framely-download-', dir=cache) as folder:
        if args.action in ('install', 'update'):
            if bool(args.archive) != bool(args.checksums):
                parser.error('--archive and --checksums must be provided together')
            if not args.archive:
                endpoint = 'latest' if not args.version else 'tags/' + urllib.parse.quote(args.version, safe='')
                with fetch(f'https://api.github.com/repos/{args.repo}/releases/{endpoint}') as response:
                    metadata = response.read(4 * 1024**2 + 1)
                if len(metadata) > 4 * 1024**2:
                    raise ValueError('Release metadata too large')
                release = json.loads(metadata)
                assets = {asset['name']: asset for asset in release['assets']}
                package = select_package(assets, args.action)
                args.archive = pathlib.Path(folder) / package
                args.checksums = pathlib.Path(folder) / 'SHA256SUMS'
                print('Downloading ' + package, flush=True)
                download(assets[package]['browser_download_url'], args.archive, MAX_ARCHIVE)
                download(assets['SHA256SUMS']['browser_download_url'], args.checksums, 1024**2)
            args.archive, args.checksums = args.archive.resolve(), args.checksums.resolve()
            if args.progress_json:
                emit_progress('verify')
            verify_archive(args.archive, args.checksums)
            print('Archive SHA256 verified.', flush=True)
        if os.geteuid() == 0:
            root_operation(args)
        else:
            command = ['sudo', '--', sys.executable, str(pathlib.Path(__file__).resolve()), args.action, '--repo', args.repo, '--user', args.user, '--yes']
            if args.archive:
                command += ['--archive', str(args.archive), '--checksums', str(args.checksums)]
            if args.progress_json:
                command += ['--progress-json']
            # stdin belongs to curl | bash; sudo reads its password from the terminal.
            subprocess.run(command, check=True)


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        print('Framely: ' + str(error), file=sys.stderr)
        sys.exit(1)
