#!/usr/bin/env python3
"""A dedicated server that panics in a tick still saves its world, and keeps what the panic left unsure.

`MinecraftServer.runServer` catches what the tick throws and still stops the server. Horizon's
`stop_after_crash` does the same, but a panic can interrupt a guest's turn (the game holds that
player's state swapped in) or a tick that has the hub out. `HORIZON_CRASH_FIXTURE` makes the tick
panic at one of those points. Each run below joins a real Horizon client, panics the server, and
checks, from the files the server leaves:

- `guest`: the panic comes in the joined player's turn (which runs with the hub out of the game,
  as every network turn does). The player's file is byte for byte what the previous clean stop
  wrote, and a block placed before the panic is still there on the next start.
- `hub`: the panic comes while a console line has the hub out with a player in. Same checks.
"""
import hashlib
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import uuid
import time
from pathlib import Path

try:
    from . import lab
    from .integration_env import run_game
    from .test_dedicated_server_parity import Server, write_server_dir, free_port
except ImportError:
    import lab
    from integration_env import run_game
    from test_dedicated_server_parity import Server, write_server_dir, free_port

PLAYER = 'Tester'


def binary():
    metadata = subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT, text=True)
    import json
    return Path(json.loads(metadata)['target_directory']) / 'fast' / 'horizon-client'


def start(directory, fixture=None):
    if fixture:
        os.environ['HORIZON_CRASH_FIXTURE'] = fixture
    try:
        server = Server([binary(), '--resources', lab.RESOURCES, '--dedicated', '--nogui'], directory, f'server-{fixture or "clean"}')
    finally:
        os.environ.pop('HORIZON_CRASH_FIXTURE', None)
    server.wait_for(r'Done \(', timeout=600)
    return server


def join(port, out):
    """A real client in a thread: it joins, chats and leaves, or loses its server."""
    report = out / 'report.json'

    def run():
        try:
            run_game([str(c) for c in [binary(), '--resources', lab.RESOURCES, '--render-distance', 2, '--server',
                                       f'127.0.0.1:{port}', '--name', PLAYER]],
                     cwd=out, env=lab.game_env(HORIZON_NET_REPORT=report), stdout=subprocess.DEVNULL,
                     stderr=subprocess.STDOUT, timeout=120)
        except subprocess.TimeoutExpired:
            pass
    thread = threading.Thread(target=run, daemon=True)
    thread.start()
    return thread


def offline_uuid(name):
    """`UUIDUtil.createOfflinePlayerUUID`."""
    digest = bytearray(hashlib.md5(f'OfflinePlayer:{name}'.encode()).digest())
    digest[6] = digest[6] & 0x0F | 0x30
    digest[8] = digest[8] & 0x3F | 0x80
    return str(uuid.UUID(bytes=bytes(digest)))


def player_files(directory):
    """The joined player's files (data, advancements, stats), by content. The server's own profile is
    not theirs: its files are written by every save."""
    folder = directory / 'world' / 'players'
    me = offline_uuid(PLAYER)
    return {str(p.relative_to(folder)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(folder.rglob(f'*{me}*')) if p.is_file()}


def crash(directory, fixture, port, out, trigger):
    server = start(directory, fixture)
    try:
        server.console('forceload add 0 0')
        # The chunk loads a moment after the forceload.
        for _ in range(100):
            server.console(f'setblock 0 {200 + trigger["y"]} 0 minecraft:gold_block')
            time.sleep(0.5)
            if any('Changed the block' in l for l in server.lines):
                break
        else:
            raise AssertionError('the block was never placed:\n' + '\n'.join(server.lines[-20:]))
        client = join(port, out)
        server.wait_for(rf'{PLAYER} joined the game', timeout=300)
        if trigger.get('console'):
            server.console('say crash now')
        deadline = time.time() + 120
        while server.process.poll() is None and time.time() < deadline:
            time.sleep(0.2)
        assert server.process.poll() not in (None, 0), 'the server did not exit with an error after the panic'
        client.join(timeout=10)
    finally:
        server.stop()
    return '\n'.join(server.lines)


def main():
    subprocess.run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client', '--bin', 'horizon-client'], cwd=lab.ROOT, check=True)
    build = lab.LOCAL / 'dedicated-crash-save'
    shutil.rmtree(build, ignore_errors=True)
    build.mkdir(parents=True)
    port = free_port()
    directory = build / 'server'
    write_server_dir(directory, port)
    out = build / 'client'
    out.mkdir()
    problems = []

    # A clean stop writes the player's file.
    server = start(directory)
    try:
        join(port, out).join(timeout=300)
        server.wait_for(rf'{PLAYER} left the game', timeout=60)
    finally:
        server.stop()
    saved = player_files(directory)
    if not saved:
        problems.append('the clean run wrote no player file')

    for fixture, y, trigger, expect in (
        ('guest', 0, {'y': 0}, 'The panic interrupted the network tick'),
        ('hub', 1, {'y': 1, 'console': True}, 'The panic interrupted the network tick'),
    ):
        log = crash(directory, fixture, port, out, trigger)
        if f'The server thread panicked: crash fixture at {fixture}' not in log:
            problems.append(f'{fixture}: no panic in the log')
        if expect not in log:
            problems.append(f'{fixture}: the log does not say {expect!r}')
        if 'Saving worlds' not in log:
            problems.append(f'{fixture}: the world was not saved')
        if player_files(directory) != saved:
            problems.append(f'{fixture}: a player file changed: {saved} -> {player_files(directory)}')
        # The next start finds the block placed before the panic.
        server = start(directory)
        try:
            server.console('forceload add 0 0')
            time.sleep(3)
            server.console(f'execute if block 0 {200 + y} 0 minecraft:gold_block run say FOUND-{fixture}')
            server.wait_for(rf'\[Server\] FOUND-{fixture}', timeout=60)
        except AssertionError:
            problems.append(f'{fixture}: the block placed before the panic is gone')
        finally:
            server.stop()
    for p in problems:
        print('FAIL', p)
    print('dedicated crash save:', 'ok' if not problems else 'FAILED')
    return 1 if problems else 0


if __name__ == '__main__':
    sys.exit(main())
