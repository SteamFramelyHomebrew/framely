#!/usr/bin/env python3
"""用仅支持现代密钥交换的本机 SSH 服务验证安装器的真实握手与指纹。"""

import argparse
import asyncio
import os
import pathlib

import asyncssh


async def run(args):
    """测试密钥只保存在内存中，服务只监听临时回环端口且不开放登录。"""
    root = pathlib.Path(__file__).resolve().parents[1]
    key = asyncssh.generate_private_key('ssh-ed25519')
    connections = []

    class FixtureServer(asyncssh.SSHServer):
        def connection_made(self, connection):
            connections.append(connection)

    server = await asyncssh.create_server(
        FixtureServer,
        '127.0.0.1',
        0,
        config=None,
        server_host_keys=[key],
        kex_algs=['curve25519-sha256'],
        public_key_auth=False,
        password_auth=False,
        kbdint_auth=False,
    )
    env = dict(
        os.environ,
        FRAMELY_TEST_SSH_PORT=str(server.get_port()),
        FRAMELY_TEST_SSH_FINGERPRINT=key.get_fingerprint('sha256'),
    )
    command = [
        'cargo', 'test', '--manifest-path', str(root / 'installer/Cargo.toml'),
        '--locked', '--profile', args.profile, '--lib',
    ]
    if args.no_default_features:
        command.append('--no-default-features')
    command.extend([
        'remote::tests::curve25519_handshake_matches_trusted_fingerprint',
        '--', '--ignored', '--exact',
    ])
    process = None
    try:
        process = await asyncio.create_subprocess_exec(*command, cwd=root, env=env)
        result = await process.wait()
        if result == 0 and len(connections) != 1:
            raise RuntimeError('SSH 测试必须实际连接一次；请检查测试是否被改名或跳过')
        return result
    finally:
        if process is not None and process.returncode is None:
            process.kill()
            await process.wait()
        for connection in connections:
            connection.abort()
        server.close()
        await server.wait_closed()


def main():
    """CI 默认使用发行配置；开发时允许复用无界面的调试构建。"""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--profile', choices=['dev', 'release'], default='release')
    parser.add_argument('--no-default-features', action='store_true')
    return asyncio.run(run(parser.parse_args()))


if __name__ == '__main__':
    raise SystemExit(main())
