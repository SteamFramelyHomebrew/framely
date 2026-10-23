#!/usr/bin/env python3
"""Shared device-side installation engine, also embedded in the desktop installer."""
import argparse
import contextlib
import datetime
import errno
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
import urllib.error
import uuid
import socket
import ssl
import http.client

DEFAULT_REPO = 'SteamFramelyHomebrew/framely'
MAX_ARCHIVE = 2 * 1024**3
TRACE_ID = uuid.uuid4().hex[:12]


def log_event(message):
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    print(f'[{timestamp}] Framely[{TRACE_ID}] {message}', flush=True)


def log_failure(message):
    timestamp = datetime.datetime.now(datetime.timezone.utc).strftime('%Y-%m-%dT%H:%M:%SZ')
    print(f'[{timestamp}] Framely[{TRACE_ID}] {message}', file=sys.stderr, flush=True)


def emit_progress(step, completed=None, total=None):
    payload = {'step': step}
    if total is not None and total > 0:
        payload.update(completed=completed, total=total)
    print('FRAMELY_PROGRESS ' + json.dumps(payload), flush=True)


def terminal_download_progress(total):
    last_reported = 0.0

    def report(completed):
        nonlocal last_reported
        now = time.monotonic()
        if completed != 0 and completed != total and now - last_reported < 5:
            return
        last_reported = now
        if total is None:
            log_event(f'下载进度 {completed / 1024**2:.1f} MiB（总大小待确认）')
        else:
            percent = completed * 100 / total
            log_event(f'下载进度 {percent:5.1f}%  {completed / 1024**2:.1f} / {total / 1024**2:.1f} MiB')

    return report


class InstallationFailure(RuntimeError):
    pass


def failure_reason(error):
    if isinstance(error, urllib.error.HTTPError):
        if error.code == 404:
            return 'HTTP 404：请求的版本或发布文件不存在。'
        if error.code == 403:
            return 'HTTP 403：下载服务拒绝访问，可能触发了访问限制。'
        if error.code == 429:
            return 'HTTP 429：下载服务正在限流。'
        return f'HTTP {error.code}：下载服务返回错误。'
    if isinstance(error, urllib.error.URLError):
        return failure_reason(error.reason)
    if isinstance(error, (ssl.SSLCertVerificationError, ssl.CertificateError)):
        return 'TLS 证书验证失败；请检查设备时间、证书和代理。'
    if isinstance(error, socket.gaierror):
        return '域名解析失败；请检查网络和代理地址。'
    if isinstance(error, (TimeoutError, ConnectionError, http.client.IncompleteRead,
                          http.client.RemoteDisconnected)):
        return '连接超时、中断或文件传输不完整。'
    if isinstance(error, OSError):
        if error.errno == errno.ENOSPC:
            return '设备存储空间不足。'
        if error.errno in (errno.EACCES, errno.EPERM):
            return '没有写入安装文件的权限。'
        if error.errno == errno.ENOENT:
            return '安装暂存文件或目录不存在。'
        if error.errno == errno.EIO:
            return '存储设备读写失败。'
    if isinstance(error, ValueError) and 'SHA256 mismatch' in str(error):
        return '文件校验不通过；下载内容可能损坏或被代理修改。'
    if isinstance(error, (KeyError, TypeError)):
        return '发行版信息缺少必要字段或格式无效。'
    if isinstance(error, subprocess.CalledProcessError):
        return '安装子程序执行失败；详细错误见上方输出。'
    # The dual-route failure already contains only sanitized error types.
    if isinstance(error, RuntimeError) and 'GitHub primary route failed:' in str(error):
        return str(error)
    if isinstance(error, ValueError):
        if str(error) == 'Cancelled':
            return '操作已由用户取消。'
        if str(error).startswith(('No interactive terminal', 'No confirmation input')):
            return '缺少交互终端或确认输入。'
        if str(error) == 'Download exceeds size limit':
            return '下载文件超过允许大小。'
        if str(error) == 'Download temporary file identity changed':
            return '下载暂存文件在传输期间被替换。'
        return '配置或下载数据格式无效。'
    return f'{type(error).__name__}'


def failure_diagnostic(error):
    if isinstance(error, subprocess.CalledProcessError):
        return f'{type(error).__name__} exit={error.returncode}'
    if isinstance(error, urllib.error.URLError):
        return failure_diagnostic(error.reason) if not isinstance(error, urllib.error.HTTPError) else f'HTTP {error.code}'
    if isinstance(error, OSError) and error.errno is not None:
        return f'{type(error).__name__} errno={error.errno}'
    if isinstance(error, RuntimeError) and 'GitHub primary route failed:' in str(error):
        return str(error)
    return type(error).__name__


@contextlib.contextmanager
def installation_step(stage, next_action):
    started = time.monotonic()
    log_event(f'{stage}：开始')
    try:
        yield
    except (OSError, ValueError, RuntimeError, KeyError, TypeError,
            subprocess.CalledProcessError, http.client.IncompleteRead) as error:
        reason = failure_reason(error)
        diagnostic = failure_diagnostic(error)
        raise InstallationFailure(
            f'{stage}失败（耗时 {time.monotonic() - started:.1f} 秒）。\n原因：{reason}'
            f'\n诊断：{diagnostic}\n下一步：{next_action}'
        ) from None
    else:
        log_event(f'{stage}：完成，耗时 {time.monotonic() - started:.1f} 秒')


class HTTPSRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        if urllib.parse.urlparse(newurl).scheme != 'https':
            raise ValueError('HTTPS redirect downgrade rejected')
        return super().redirect_request(req, fp, code, msg, headers, newurl)


GITHUB_HOSTS = {'github.com', 'api.github.com', 'raw.githubusercontent.com',
                'objects.githubusercontent.com', 'release-assets.githubusercontent.com'}
GITHUB_FALLBACK = 'https://gh-proxy.com/'


def device_proxy_settings():
    try:
        with open('/var/lib/framely/state.json', encoding='utf-8') as state:
            return json.load(state).get('proxy', {})
    except (OSError, ValueError):
        return {}


def fallback_enabled():
    if 'FRAMELY_GITHUB_AUTO_FALLBACK' in os.environ:
        return os.environ['FRAMELY_GITHUB_AUTO_FALLBACK'] != '0'
    return device_proxy_settings().get('githubAutoFallback', True)


def github_proxy_url(value):
    value = value.strip().rstrip('/')
    parsed = urllib.parse.urlsplit(value)
    if (not value or any(ord(char) <= 32 for char in value)
            or parsed.scheme != 'https' or not parsed.hostname
            or parsed.username or parsed.password or parsed.query or parsed.fragment
            or '..' in parsed.path.split('/')):
        raise ValueError('GitHub proxy must be an HTTPS base URL')
    try:
        parsed.port
    except ValueError as error:
        raise ValueError('Invalid GitHub proxy port') from error
    return value


def configured_github_proxy():
    settings = device_proxy_settings()
    value = os.environ.get('FRAMELY_GITHUB_PROXY', settings.get('github', '') if settings.get('githubEnabled') else '')
    return github_proxy_url(value) if value.strip() else ''


def download_route():
    proxy = configured_github_proxy()
    if proxy:
        host = urllib.parse.urlsplit(proxy).hostname
        return 'gh-proxy.com' if host == 'gh-proxy.com' else '自定义 GitHub 代理（地址已隐藏）'
    return 'GitHub 直连（自动代理回退已开启）' if fallback_enabled() else 'GitHub 直连'


def public_github(url):
    parsed = urllib.parse.urlsplit(url)
    return (parsed.scheme == 'https' and parsed.hostname in GITHUB_HOSTS
            and parsed.port in (None, 443) and not parsed.username and not parsed.password
            and not parsed.fragment and all(key in ('page', 'per_page') and value.isascii() and value.isdigit() for key, value in
                urllib.parse.parse_qsl(parsed.query, keep_blank_values=True)))


def retryable(error):
    if isinstance(error, (ssl.SSLCertVerificationError, ssl.CertificateError)):
        return False
    if isinstance(error, urllib.error.HTTPError):
        return (error.code in (408, 429) or 500 <= error.code <= 599
                or error.code == 403 and (error.headers.get('X-RateLimit-Remaining') == '0'
                    or error.headers.get('Retry-After') is not None))
    if isinstance(error, urllib.error.URLError):
        return retryable(error.reason)
    # These routing failures are plain OSError, not ConnectionError. Keep an
    # explicit network-only allowlist so disk/permission failures never retry.
    if isinstance(error, OSError) and error.errno in (errno.ENETDOWN, errno.ENETUNREACH, errno.EHOSTUNREACH):
        return True
    return isinstance(error, (socket.gaierror, TimeoutError, ConnectionError,
                              http.client.IncompleteRead, http.client.RemoteDisconnected))


def network_diagnostic(error):
    # HTTPError and URLError strings may contain signed redirect URLs.
    if isinstance(error, urllib.error.HTTPError):
        return 'HTTP ' + str(error.code)
    if isinstance(error, urllib.error.URLError):
        return network_diagnostic(error.reason)
    return type(error).__name__


def github_attempt(url, operation, enabled=None):
    try:
        return operation(False)
    except Exception as primary:
        if configured_github_proxy() or not (fallback_enabled() if enabled is None else enabled) or not public_github(url) or not retryable(primary):
            raise
        log_failure(f'GitHub 请求失败（{network_diagnostic(primary)}），正在尝试 gh-proxy.com')
        try:
            return operation(True)
        except Exception as secondary:
            raise RuntimeError('GitHub primary route failed: ' + network_diagnostic(primary)
                + '; gh-proxy.com fallback failed: ' + network_diagnostic(secondary)) from None


def fetch_once(url, fallback=False):
    if urllib.parse.urlparse(url).scheme != 'https':
        raise ValueError('HTTPS is required')
    settings = device_proxy_settings()
    manual = configured_github_proxy()
    if fallback:
        url = GITHUB_FALLBACK + url
    elif manual and public_github(url):
        url = manual + '/' + url
    handlers = [HTTPSRedirect()]
    if 'FRAMELY_HTTP_PROXY' in os.environ or settings.get('httpEnabled'):
        proxy = os.environ.get('FRAMELY_HTTP_PROXY', settings.get('http', ''))
        handlers.append(urllib.request.ProxyHandler({'http': proxy, 'https': proxy} if proxy else {}))
    return urllib.request.build_opener(*handlers).open(
        urllib.request.Request(url, headers={'User-Agent': 'Framely-Installer', 'Accept': 'application/vnd.github+json'}), timeout=60)


def fetch(url):
    return github_attempt(url, lambda fallback: fetch_once(url, fallback))


def read_metadata(url, limit):
    def attempt(fallback):
        with fetch_once(url, fallback) as response:
            data = response.read(limit + 1)
            if len(data) > limit:
                raise ValueError('Release metadata too large')
            length = response.headers.get('Content-Length')
            if length is not None and len(data) != int(length):
                raise http.client.IncompleteRead(b'', max(0, int(length) - len(data)))
            return data
    return github_attempt(url, attempt)


def download(url, destination, limit, progress=None, expected_size=None):
    identity = None
    def attempt(fallback):
        nonlocal identity
        with fetch_once(url, fallback) as response, open(destination, 'r+b' if identity else 'xb') as output:
            metadata = os.fstat(output.fileno())
            current = (metadata.st_dev, metadata.st_ino)
            if identity is not None and identity != current:
                raise ValueError('Download temporary file identity changed')
            identity = current
            if fallback:
                output.truncate(0)
            size = 0
            reported_at = time.monotonic()
            if progress:
                progress(0)
            while block := response.read(256 * 1024):
                size += len(block)
                if size > limit:
                    raise ValueError('Download exceeds size limit')
                output.write(block)
                if progress and time.monotonic() - reported_at >= 0.2 and (expected_size is None or size < expected_size):
                    progress(size)
                    reported_at = time.monotonic()
            length = response.headers.get('Content-Length')
            expected = expected_size if expected_size is not None else int(length) if length is not None else None
            if expected is not None and size != expected:
                raise http.client.IncompleteRead(b'', max(0, expected - size))
            if progress:
                progress(size)
    return github_attempt(url, attempt)


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


def package_from_checksums(checksums, tag, action):
    """Find a tagged release package without relying on the GitHub Releases API."""
    plain_tag = tag[1:] if tag.startswith('v') else tag
    package_name = re.compile(r'framely-' + re.escape(plain_tag)
                              + r'-[a-f0-9]{12}(?:-offline)?-linux-arm64\.tar\.gz')
    names = set()
    for line in pathlib.Path(checksums).read_text(encoding='utf-8').splitlines():
        match = re.fullmatch(r'([a-fA-F0-9]{64}) [ *]([a-zA-Z0-9._+-]+)', line)
        if not match or match[2] in names:
            raise ValueError('Invalid or duplicate SHA256SUMS entry')
        names.add(match[2])
    assets = {name: {} for name in names
              if package_name.fullmatch(name) or name == 'framely-cef.json'}
    assets['SHA256SUMS'] = {}
    return select_package(assets, action)


def release_version(tag):
    """Return a SemVer precedence key, excluding build metadata."""
    match = re.fullmatch(
        r'v?(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)'
        r'(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?'
        r'(?:\+[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*)?', tag)
    if not match:
        return None
    identifiers = []
    for part in (match[4] or '').split('.') if match[4] else []:
        if part.isdigit():
            if len(part) > 1 and part.startswith('0'):
                return None
            identifiers.append((0, int(part)))
        else:
            identifiers.append((1, part))
    # A final release has higher precedence than its prereleases.
    return (int(match[1]), int(match[2]), int(match[3]),
            1 if not identifiers else 0, tuple(identifiers))


def resolve_release(repo, channel, action):
    """Choose an installable GitHub release for the interactive entry point."""
    candidates = []
    for page in range(1, 11):
        url = f'https://api.github.com/repos/{repo}/releases?per_page=100&page={page}'
        for attempt in range(3):
            try:
                metadata = read_metadata(url, 4 * 1024**2)
                break
            except Exception as error:
                if attempt == 2 or not retryable(error):
                    raise
                log_failure('Release metadata transfer was interrupted; retrying the same route.')
                time.sleep(0.5 * (attempt + 1))
        releases = json.loads(metadata)
        if not isinstance(releases, list):
            raise ValueError('Invalid GitHub release list')
        for release in releases:
            if not isinstance(release, dict) or release.get('draft'):
                continue
            tag = release.get('tag_name')
            version = release_version(tag) if isinstance(tag, str) else None
            if version is None or not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9.+-]*', tag):
                continue
            prerelease = version[3] == 0
            if release.get('prerelease') != prerelease or channel == 'stable' and prerelease:
                continue
            if not isinstance(release.get('assets'), list):
                continue
            assets = {asset['name']: asset for asset in release['assets']
                      if isinstance(asset, dict) and isinstance(asset.get('name'), str)}
            if 'bootstrap.py' not in assets:
                continue
            try:
                package = select_package(assets, action)
            except ValueError:
                continue
            # A release must contain a package for its own tag.
            name = package[len('framely-'):-len('-linux-arm64.tar.gz')]
            if name.endswith('-offline'):
                name = name[:-len('-offline')]
            plain_tag = tag[1:] if tag.startswith('v') else tag
            if not re.fullmatch(re.escape(plain_tag) + r'-[a-f0-9]{12}', name):
                continue
            size = assets[package].get('size')
            if not isinstance(size, int) or not 0 < size <= MAX_ARCHIVE:
                continue
            candidates.append((version, tag, size))
        if len(releases) < 100:
            break
    else:
        raise ValueError('Release list exceeded ten pages')
    if not candidates:
        raise ValueError('No installable release found for selected channel')
    _, tag, size = max(candidates, key=lambda item: item[0])
    return {'tag': tag, 'size': size}


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
            answer = terminal.readline()
    except OSError as error:
        raise ValueError('No interactive terminal for confirmation; run in a terminal or pass --yes') from error
    if not answer:
        raise ValueError('No confirmation input; run in a terminal or pass --yes')
    return answer.strip().lower() == 'y'


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
                with installation_step('校验解压文件', '重新运行安装命令；若仍失败，请保存上方校验输出供开发者排查。'):
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
                with installation_step('执行安装脚本', '查看上方脚本输出，修复所提示的问题后重新运行安装命令。'):
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
            log_event('设备端操作完成')
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
    parser.add_argument('--github-proxy', help='HTTPS URL prefix for GitHub downloads, such as https://gh-proxy.com')
    parser.add_argument('--progress-json', action='store_true', help=argparse.SUPPRESS)
    parser.add_argument('--trace-id', help=argparse.SUPPRESS)
    parser.add_argument('--resolve-release', choices=['stable', 'testing'], help=argparse.SUPPRESS)
    parser.add_argument('--selection-action', choices=['install', 'update'], help=argparse.SUPPRESS)
    args = parser.parse_args(argv)
    global TRACE_ID
    candidate = args.trace_id or os.environ.get('FRAMELY_INSTALL_TRACE_ID', '')
    if re.fullmatch(r'[0-9]{8}T[0-9]{6}Z-[0-9]{1,10}|[0-9a-f]{12}', candidate):
        TRACE_ID = candidate
    if args.github_proxy:
        os.environ['FRAMELY_GITHUB_PROXY'] = github_proxy_url(args.github_proxy)
    if not re.fullmatch(r'[a-zA-Z0-9_.-]+/[a-zA-Z0-9_.-]+', args.repo):
        parser.error('Invalid repository')
    if not re.fullmatch(r'[a-z_][a-zA-Z0-9_-]*', args.user) or args.user == 'root':
        parser.error('Invalid Steam user')
    if args.version and not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9.+-]*', args.version):
        parser.error('Invalid release version')
    if args.resolve_release:
        if not args.selection_action:
            parser.error('--selection-action is required with --resolve-release')
        print(json.dumps(resolve_release(args.repo, args.resolve_release, args.selection_action)))
        return
    if sys.platform != 'linux' or os.uname().machine != 'aarch64':
        parser.error('This installation engine requires Linux ARM64 Steam Frame')
    explicit_action = args.action is not None
    if args.action is None:
        with open('/dev/tty', 'r+') as terminal:
            terminal.write('1 Install  2 Update  3 Repair  4 Rollback  5 Uninstall\nChoose [1-5]: ')
            terminal.flush()
            choices = {'1':'install', '2':'update', '3':'repair', '4':'rollback', '5':'uninstall'}
            args.action = choices.get(terminal.readline().strip())
        if args.action is None:
            parser.error('Invalid action')
    if not args.yes and not (explicit_action and args.action in ('install', 'update')) and not confirm(f'{args.action}: ' + ('disable and uninstall ALL plugins, then remove Framely (saved data retained)' if args.action == 'uninstall' else 'continue with Framely maintenance')):
        raise ValueError('Cancelled')
    log_event(f'开始：操作={args.action}，版本={args.version or "latest stable"}，来源={"official" if args.repo == DEFAULT_REPO else "custom"}，'
              f'架构={os.uname().machine}，Python={sys.version_info.major}.{sys.version_info.minor}，下载线路={download_route()}')
    cache = pathlib.Path.home() / '.cache' / 'framely-installer' if args.action in ('install', 'update') and not args.archive else None
    if cache is not None:
        cache.mkdir(parents=True, mode=0o700, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='framely-download-', dir=cache) as folder:
        if args.action in ('install', 'update'):
            if bool(args.archive) != bool(args.checksums):
                parser.error('--archive and --checksums must be provided together')
            if not args.archive:
                args.checksums = pathlib.Path(folder) / 'SHA256SUMS'
                if args.version:
                    # A known tag has stable asset URLs. The proxy may serve release
                    # downloads while rejecting GitHub's API with HTTP 403.
                    base = (f'https://github.com/{args.repo}/releases/download/'
                            + urllib.parse.quote(args.version, safe=''))
                    print('正在下载校验文件…', flush=True)
                    with installation_step('下载校验文件', '确认版本已发布，检查网络或更换 GitHub 代理后重试。'):
                        download(base + '/SHA256SUMS', args.checksums, 1024**2)
                    with installation_step('读取发行版信息', '确认此版本包含适用的 ARM64 安装包。'):
                        package = package_from_checksums(args.checksums, args.version, args.action)
                    args.archive = pathlib.Path(folder) / package
                    log_event(f'发行版：{args.version}，文件={package}，下载线路={download_route()}')
                    with installation_step('下载安装包', '检查网络或更换 GitHub 代理后重新运行安装命令。'):
                        download(base + '/' + package, args.archive, MAX_ARCHIVE,
                                 terminal_download_progress(None))
                    log_event(f'安装包大小：{args.archive.stat().st_size / 1024**2:.1f} MiB')
                else:
                    with installation_step('读取发行版信息', '确认版本已发布，检查 GitHub 或代理连接后重试。'):
                        metadata = read_metadata(f'https://api.github.com/repos/{args.repo}/releases/latest', 4 * 1024**2)
                        release = json.loads(metadata)
                        assets = {asset['name']: asset for asset in release['assets']}
                        package = select_package(assets, args.action)
                        size = assets[package]['size']
                        if not isinstance(size, int) or size <= 0 or size > MAX_ARCHIVE:
                            raise ValueError('发布包大小无效')
                    tag = release.get('tag_name', '')
                    log_event(f'发行版：{tag if isinstance(tag, str) and re.fullmatch(r"[a-zA-Z0-9][a-zA-Z0-9.+-]*", tag) else "unknown"}，文件={package}')
                    args.archive = pathlib.Path(folder) / package
                    log_event(f'安装包大小：{size / 1024**2:.1f} MiB，下载线路：{download_route()}')
                    with installation_step('下载安装包', '检查网络或更换 GitHub 代理后重新运行安装命令。'):
                        download(assets[package]['browser_download_url'], args.archive, MAX_ARCHIVE,
                                 terminal_download_progress(size), expected_size=size)
                    print('正在下载校验文件…', flush=True)
                    with installation_step('下载校验文件', '检查网络或更换 GitHub 代理后重新运行安装命令。'):
                        download(assets['SHA256SUMS']['browser_download_url'], args.checksums, 1024**2)
            args.archive, args.checksums = args.archive.resolve(), args.checksums.resolve()
            if args.progress_json:
                emit_progress('verify')
            print('正在校验安装包…', flush=True)
            with installation_step('校验安装包', '重新运行安装命令；若仍失败，换用可信的下载线路并向开发者提供诊断信息。'):
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
            if args.github_proxy:
                command += ['--github-proxy', args.github_proxy]
            command += ['--trace-id', TRACE_ID]
            # stdin belongs to curl | bash; sudo reads its password from the terminal.
            try:
                subprocess.run(command, check=True)
            except subprocess.CalledProcessError as error:
                raise InstallationFailure('设备端安装失败。\n原因：sudo 安装步骤退出，详细错误见上方输出。'
                                          '\n下一步：按上方提示修复后，重新运行安装命令。') from error
    log_event(f'操作完成：{args.action}')


if __name__ == '__main__':
    try:
        main()
    except (ValueError, OSError, RuntimeError, subprocess.CalledProcessError,
            http.client.IncompleteRead) as error:
        if '--resolve-release' in sys.argv:
            log_failure('Release lookup failed. Diagnostic: '
                        + failure_diagnostic(error)
                        + '. Check the network, mirror, and release assets, then retry.')
        elif isinstance(error, InstallationFailure):
            log_failure(str(error))
        else:
            log_failure(f'安装操作失败。\n原因：{failure_reason(error)}\n诊断：{failure_diagnostic(error)}'
                        '\n下一步：检查上方日志和运行环境，修复问题后重试；若仍失败，请提供追踪编号及完整安装输出。')
        sys.exit(1)
